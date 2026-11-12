//! Keyframe captions with Florence-2-base-ft (MIT).
//!
//! Four ONNX graphs: a 768×768 vision encoder, a text encoder, a token
//! embedding table and a decoder. The decoder is fed the whole prefix on every
//! step (no KV cache); that is the simplest loop and the export's intended
//! path. Captions are model-generated descriptions, not ground truth: they are
//! bound to one keyframe instant and never replace OCR or speech.
//!
//! Everything that does not need ONNX Runtime is compiled unconditionally so a
//! model-free build can test the pure helpers.

#![cfg_attr(not(feature = "vision"), allow(dead_code))]

use crate::models::ModelStore;
use crate::traits::{ProviderError, ProviderInfo, ProviderResult};

pub const MODEL_ID: &str = "florence-2-base-ft";
pub const INPUT: usize = 768;
pub const IMAGE_TOKENS: usize = 577;
pub const HIDDEN: usize = 768;
/// The processor rewrites `<MORE_DETAILED_CAPTION>` to this English question.
pub const PROMPT: &str = "Describe with a paragraph what is shown in the image.";
pub const MAX_NEW_TOKENS: usize = 120;
const EOS: i64 = 2;

#[cfg(feature = "vision")]
use std::sync::Mutex;

#[cfg(feature = "vision")]
use image::ImageFormat;
#[cfg(feature = "vision")]
use image::imageops::FilterType;
#[cfg(feature = "vision")]
use ort::session::Session;
#[cfg(feature = "vision")]
use ort::value::Tensor;
#[cfg(feature = "vision")]
use tokenizers::Tokenizer;

#[cfg(feature = "vision")]
use sonolytica_core::Capability;

pub struct Florence2 {
    #[cfg(feature = "vision")]
    inner: Mutex<Inner>,
    info: ProviderInfo,
}

#[cfg(feature = "vision")]
struct Inner {
    vision: Session,
    encoder: Session,
    decoder: Session,
    embed: Session,
    tokenizer: Tokenizer,
}

impl std::fmt::Debug for Florence2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Florence2")
            .field("info", &self.info)
            .finish()
    }
}

impl Florence2 {
    #[cfg(feature = "vision")]
    pub fn load(store: &ModelStore, threads: usize) -> ProviderResult<Self> {
        let spec = crate::models::find_model(MODEL_ID).ok_or_else(|| {
            ProviderError::Inference("the Florence-2 model is not in this registry".into())
        })?;
        let dir = store.require(spec)?;
        let vision = crate::onnx::build_session(&dir.join("vision_encoder.onnx"), threads)?;
        let encoder = crate::onnx::build_session(&dir.join("encoder_model_int8.onnx"), threads)?;
        let decoder = crate::onnx::build_session(&dir.join("decoder_model_int8.onnx"), threads)?;
        let embed = crate::onnx::build_session(&dir.join("embed_tokens_int8.onnx"), threads)?;
        let tokenizer = Tokenizer::from_file(dir.join("tokenizer.json")).map_err(|error| {
            ProviderError::Inference(format!("cannot load the Florence tokenizer: {error}"))
        })?;
        Ok(Self {
            inner: Mutex::new(Inner {
                vision,
                encoder,
                decoder,
                embed,
                tokenizer,
            }),
            info: ProviderInfo {
                name: "florence-2-base-ft",
                capability: Capability::Vision,
                device: "cpu",
                models: vec![spec.reference()],
                notes: "Model-generated keyframe captions (Florence-2-base-ft, MIT). One \
                        observation per sampled keyframe; not OCR, not ground truth, no \
                        tracking and no identity.",
            },
        })
    }

    #[cfg(not(feature = "vision"))]
    pub fn load(_store: &ModelStore, _threads: usize) -> ProviderResult<Self> {
        Err(ProviderError::Unsupported(
            "this build was compiled without the `vision` feature; rebuild with `--features full` \
             (or `--features sonolytica-providers/vision`)"
                .into(),
        ))
    }

    pub fn info(&self) -> ProviderInfo {
        self.info.clone()
    }

    #[cfg(feature = "vision")]
    pub fn describe_jpeg(&self, bytes: &[u8]) -> ProviderResult<String> {
        let decoded = image::load_from_memory_with_format(bytes, ImageFormat::Jpeg)
            .map_err(|error| ProviderError::Unsupported(format!("cannot decode JPEG: {error}")))?;
        let rgb = decoded.into_rgb8();
        let resized =
            image::imageops::resize(&rgb, INPUT as u32, INPUT as u32, FilterType::CatmullRom);
        let mut pixels = Vec::with_capacity(3 * INPUT * INPUT);
        // ImageNet normalisation, CHW, as the Florence processor does.
        let mean = [0.485_f32, 0.456, 0.406];
        let std = [0.229_f32, 0.224, 0.225];
        for channel in 0..3 {
            for y in 0..INPUT {
                for x in 0..INPUT {
                    let value = f32::from(resized.get_pixel(x as u32, y as u32)[channel]) / 255.0;
                    pixels.push((value - mean[channel]) / std[channel]);
                }
            }
        }

        let mut inner = self
            .inner
            .lock()
            .map_err(|_| ProviderError::Inference("florence session poisoned".into()))?;

        let pixel_tensor = Tensor::from_array((vec![1_i64, 3, INPUT as i64, INPUT as i64], pixels))
            .map_err(|error| ProviderError::Inference(error.to_string()))?;
        let vision_out = inner
            .vision
            .run(ort::inputs!["pixel_values" => pixel_tensor])
            .map_err(|error| {
                ProviderError::Inference(format!("florence vision failed: {error}"))
            })?;
        let (vision_shape, vision_data) = vision_out
            .get("image_features")
            .ok_or_else(|| ProviderError::Inference("florence image features missing".into()))?
            .try_extract_tensor::<f32>()
            .map_err(|error| ProviderError::Inference(error.to_string()))?;
        let dims: Vec<usize> = vision_shape.iter().map(|d| *d as usize).collect();
        if dims != [1, IMAGE_TOKENS, HIDDEN] {
            return Err(ProviderError::Inference(format!(
                "unexpected florence vision shape {dims:?}"
            )));
        }
        let image_features = vision_data.to_vec();
        drop(vision_out);

        let prompt = inner
            .tokenizer
            .encode(PROMPT, false)
            .map_err(|error| ProviderError::Inference(format!("tokenizer failed: {error}")))?;
        let prompt_ids: Vec<i64> = prompt.get_ids().iter().map(|id| i64::from(*id)).collect();
        if prompt_ids.is_empty() {
            return Err(ProviderError::Inference("florence prompt is empty".into()));
        }
        let text_embeds = self.embed(&mut inner, &prompt_ids)?;
        let text_len = prompt_ids.len();
        let mut encoder_input = Vec::with_capacity((IMAGE_TOKENS + text_len) * HIDDEN);
        encoder_input.extend_from_slice(&image_features);
        encoder_input.extend_from_slice(&text_embeds);
        let encoder_len = IMAGE_TOKENS + text_len;
        let mask: Vec<i64> = vec![1; encoder_len];

        let encoder_input = Tensor::from_array((
            vec![1_i64, encoder_len as i64, HIDDEN as i64],
            encoder_input,
        ))
        .map_err(|error| ProviderError::Inference(error.to_string()))?;
        let attention = Tensor::from_array((vec![1_i64, encoder_len as i64], mask.clone()))
            .map_err(|error| ProviderError::Inference(error.to_string()))?;
        let encoder_out = inner
            .encoder
            .run(ort::inputs![
                "inputs_embeds" => encoder_input,
                "attention_mask" => attention,
            ])
            .map_err(|error| {
                ProviderError::Inference(format!("florence encoder failed: {error}"))
            })?;
        let (encoder_shape, encoder_data) = encoder_out
            .get("last_hidden_state")
            .ok_or_else(|| ProviderError::Inference("florence encoder output missing".into()))?
            .try_extract_tensor::<f32>()
            .map_err(|error| ProviderError::Inference(error.to_string()))?;
        let dims: Vec<usize> = encoder_shape.iter().map(|d| *d as usize).collect();
        if dims != [1, encoder_len, HIDDEN] {
            return Err(ProviderError::Inference(format!(
                "unexpected florence encoder shape {dims:?}"
            )));
        }
        let encoder_hidden = encoder_data.to_vec();
        drop(encoder_out);

        let mut tokens = vec![EOS];
        for _ in 0..MAX_NEW_TOKENS {
            let embeds = self.embed(&mut inner, &tokens)?;
            let decoder_input =
                Tensor::from_array((vec![1_i64, tokens.len() as i64, HIDDEN as i64], embeds))
                    .map_err(|error| ProviderError::Inference(error.to_string()))?;
            let encoder_hidden_tensor = Tensor::from_array((
                vec![1_i64, encoder_len as i64, HIDDEN as i64],
                encoder_hidden.clone(),
            ))
            .map_err(|error| ProviderError::Inference(error.to_string()))?;
            let attention = Tensor::from_array((vec![1_i64, encoder_len as i64], mask.clone()))
                .map_err(|error| ProviderError::Inference(error.to_string()))?;
            let decoder_out = inner
                .decoder
                .run(ort::inputs![
                    "inputs_embeds" => decoder_input,
                    "encoder_hidden_states" => encoder_hidden_tensor,
                    "encoder_attention_mask" => attention,
                ])
                .map_err(|error| {
                    ProviderError::Inference(format!("florence decoder failed: {error}"))
                })?;
            let (logits_shape, logits) = decoder_out
                .get("logits")
                .ok_or_else(|| ProviderError::Inference("florence logits missing".into()))?
                .try_extract_tensor::<f32>()
                .map_err(|error| ProviderError::Inference(error.to_string()))?;
            let dims: Vec<usize> = logits_shape.iter().map(|d| *d as usize).collect();
            if dims.len() != 3 || dims[0] != 1 || dims[1] != tokens.len() {
                return Err(ProviderError::Inference(format!(
                    "unexpected florence logits shape {dims:?}"
                )));
            }
            let vocab = dims[2];
            let base = (tokens.len() - 1) * vocab;
            let next = argmax(&logits[base..base + vocab]);
            drop(decoder_out);
            if next == EOS as usize {
                break;
            }
            tokens.push(next as i64);
        }

        let generated: Vec<u32> = tokens.iter().skip(1).map(|token| *token as u32).collect();
        let text = inner.tokenizer.decode(&generated, true).map_err(|error| {
            ProviderError::Inference(format!("tokenizer decode failed: {error}"))
        })?;
        Ok(text.trim().to_string())
    }

    #[cfg(not(feature = "vision"))]
    pub fn describe_jpeg(&self, _bytes: &[u8]) -> ProviderResult<String> {
        Err(ProviderError::Unsupported(
            "this build was compiled without the `vision` feature".into(),
        ))
    }

    #[cfg(feature = "vision")]
    fn embed(&self, inner: &mut Inner, ids: &[i64]) -> ProviderResult<Vec<f32>> {
        let tensor = Tensor::from_array((vec![1_i64, ids.len() as i64], ids.to_vec()))
            .map_err(|error| ProviderError::Inference(error.to_string()))?;
        let outputs = inner
            .embed
            .run(ort::inputs!["input_ids" => tensor])
            .map_err(|error| ProviderError::Inference(format!("florence embed failed: {error}")))?;
        let (shape, data) = outputs
            .get("inputs_embeds")
            .ok_or_else(|| ProviderError::Inference("florence embeddings missing".into()))?
            .try_extract_tensor::<f32>()
            .map_err(|error| ProviderError::Inference(error.to_string()))?;
        let dims: Vec<usize> = shape.iter().map(|d| *d as usize).collect();
        if dims != [1, ids.len(), HIDDEN] {
            return Err(ProviderError::Inference(format!(
                "unexpected florence embedding shape {dims:?}"
            )));
        }
        let out = data.to_vec();
        drop(outputs);
        Ok(out)
    }
}

fn argmax(values: &[f32]) -> usize {
    let mut best = 0;
    let mut best_value = f32::NEG_INFINITY;
    for (index, value) in values.iter().enumerate() {
        if value.is_finite() && *value > best_value {
            best_value = *value;
            best = index;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argmax_skips_non_finite_values() {
        assert_eq!(argmax(&[0.1, f32::NAN, 0.3, f32::INFINITY]), 2);
        assert_eq!(argmax(&[]), 0);
    }
}
