//! Face detection with YuNet (ONNX, MIT).
//!
//! Count and boxes only: no embeddings, no identity, no names. The model is
//! OpenCV Zoo's `face_detection_yunet_2023mar.onnx`, fixed 640×640 input,
//! three strides (8/16/32), score `sqrt(cls * obj)`, IoU NMS at 0.3.
//!
//! Everything that does not need ONNX Runtime (decoding, NMS, scaling) is
//! compiled unconditionally so a model-free build can test it.

#![cfg_attr(not(feature = "vision"), allow(dead_code))]

use sonolytica_core::BBox;

use crate::models::ModelStore;
use crate::traits::{ProviderError, ProviderInfo, ProviderResult};

pub const MODEL_ID: &str = "yunet-face-detection";
/// The exported model has a fixed 640×640 input.
pub const INPUT: usize = 640;
pub const SCORE_THRESHOLD: f32 = 0.6;
pub const NMS_IOU: f32 = 0.3;
const STRIDES: [usize; 3] = [8, 16, 32];

#[derive(Debug, Clone, Copy)]
pub struct FaceDetection {
    pub bbox: BBox,
    pub score: f32,
}

#[cfg(feature = "vision")]
use std::sync::Mutex;

#[cfg(feature = "vision")]
use image::ImageFormat;
#[cfg(feature = "vision")]
use image::imageops::FilterType;
#[cfg(feature = "vision")]
use ort::session::Session;
#[cfg(feature = "vision")]
use ort::session::builder::GraphOptimizationLevel;
#[cfg(feature = "vision")]
use ort::value::Tensor;

#[cfg(feature = "vision")]
use sonolytica_core::Capability;

pub struct YuNet {
    #[cfg(feature = "vision")]
    session: Mutex<Session>,
    info: ProviderInfo,
}

impl std::fmt::Debug for YuNet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YuNet").field("info", &self.info).finish()
    }
}

impl YuNet {
    #[cfg(feature = "vision")]
    pub fn load(store: &ModelStore, threads: usize) -> ProviderResult<Self> {
        let spec = crate::models::find_model(MODEL_ID).ok_or_else(|| {
            ProviderError::Inference("the face detection model is not in this registry".into())
        })?;
        let path = store.require(spec)?;
        let session = build_session(&path, threads)?;
        Ok(Self {
            session: Mutex::new(session),
            info: ProviderInfo {
                name: "yunet",
                capability: Capability::Vision,
                device: "cpu",
                models: vec![spec.reference()],
                notes: "Face detection only: count and boxes, no identity. \
                        Runs on extracted keyframes, not on every frame.",
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
    pub fn detect_jpeg(&self, bytes: &[u8]) -> ProviderResult<Vec<FaceDetection>> {
        let decoded = image::load_from_memory_with_format(bytes, ImageFormat::Jpeg)
            .map_err(|e| ProviderError::Unsupported(format!("cannot decode JPEG: {e}")))?;
        let rgb = decoded.into_rgb8();
        let (width, height) = rgb.dimensions();
        self.detect_rgb(width as usize, height as usize, rgb.as_raw())
    }

    #[cfg(not(feature = "vision"))]
    pub fn detect_jpeg(&self, _bytes: &[u8]) -> ProviderResult<Vec<FaceDetection>> {
        Err(ProviderError::Unsupported(
            "this build was compiled without the `vision` feature".into(),
        ))
    }

    #[cfg(feature = "vision")]
    pub fn detect_rgb(
        &self,
        width: usize,
        height: usize,
        rgb: &[u8],
    ) -> ProviderResult<Vec<FaceDetection>> {
        if width == 0 || height == 0 || rgb.len() != width * height * 3 {
            return Err(ProviderError::Unsupported(
                "expected a non-empty RGB image".into(),
            ));
        }
        let resized = image::imageops::resize(
            &image::RgbImage::from_raw(width as u32, height as u32, rgb.to_vec())
                .ok_or_else(|| ProviderError::Unsupported("bad RGB buffer".into()))?,
            INPUT as u32,
            INPUT as u32,
            FilterType::Triangle,
        );
        // OpenCV's blobFromImage defaults: BGR order, 0..255 floats, no mean
        // subtraction. Feeding 0..1 silently returns zero detections.
        let mut input = vec![0.0_f32; 3 * INPUT * INPUT];
        for (index, pixel) in resized.pixels().enumerate() {
            let [r, g, b] = pixel.0;
            input[index] = f32::from(b);
            input[INPUT * INPUT + index] = f32::from(g);
            input[2 * INPUT * INPUT + index] = f32::from(r);
        }
        let tensor = Tensor::from_array((vec![1_i64, 3, INPUT as i64, INPUT as i64], input))
            .map_err(|e| ProviderError::Inference(e.to_string()))?;
        let mut session = self
            .session
            .lock()
            .map_err(|_| ProviderError::Inference("face session poisoned".into()))?;
        let outputs = session
            .run(ort::inputs!["input" => tensor])
            .map_err(|e| ProviderError::Inference(format!("face detection run failed: {e}")))?;

        let mut candidates = Vec::new();
        for stride in STRIDES.iter() {
            let cls = tensor_at(&outputs, &format!("cls_{stride}"))?;
            let obj = tensor_at(&outputs, &format!("obj_{stride}"))?;
            let bbox = tensor_at(&outputs, &format!("bbox_{stride}"))?;
            candidates.extend(decode_stride(
                &cls,
                &obj,
                &bbox,
                *stride,
                INPUT,
                SCORE_THRESHOLD,
            ));
        }
        let faces = nms(candidates, NMS_IOU);
        let scale_x = width as f32 / INPUT as f32;
        let scale_y = height as f32 / INPUT as f32;
        Ok(faces
            .into_iter()
            .map(|face| FaceDetection {
                bbox: BBox {
                    x: (face.bbox.x * scale_x).clamp(0.0, width as f32),
                    y: (face.bbox.y * scale_y).clamp(0.0, height as f32),
                    width: (face.bbox.width * scale_x)
                        .clamp(0.0, width as f32 - face.bbox.x * scale_x),
                    height: (face.bbox.height * scale_y)
                        .clamp(0.0, height as f32 - face.bbox.y * scale_y),
                },
                score: face.score,
            })
            .collect())
    }

    #[cfg(not(feature = "vision"))]
    pub fn detect_rgb(
        &self,
        _width: usize,
        _height: usize,
        _rgb: &[u8],
    ) -> ProviderResult<Vec<FaceDetection>> {
        Err(ProviderError::Unsupported(
            "this build was compiled without the `vision` feature".into(),
        ))
    }
}

/// Decode one stride head. `cls`/`obj` are already sigmoided by the export.
pub fn decode_stride(
    cls: &[f32],
    obj: &[f32],
    bbox: &[f32],
    stride: usize,
    input: usize,
    threshold: f32,
) -> Vec<FaceDetection> {
    let cols = input / stride;
    let rows = cols;
    let cells = cols * rows;
    if cls.len() < cells || obj.len() < cells || bbox.len() < cells * 4 {
        return Vec::new();
    }
    let mut faces = Vec::new();
    for row in 0..rows {
        for col in 0..cols {
            let index = row * cols + col;
            let score = (cls[index].clamp(0.0, 1.0) * obj[index].clamp(0.0, 1.0)).sqrt();
            if score < threshold {
                continue;
            }
            let cx = (col as f32 + bbox[index * 4]) * stride as f32;
            let cy = (row as f32 + bbox[index * 4 + 1]) * stride as f32;
            let width = bbox[index * 4 + 2].exp() * stride as f32;
            let height = bbox[index * 4 + 3].exp() * stride as f32;
            if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
                continue;
            }
            faces.push(FaceDetection {
                bbox: BBox {
                    x: cx - width / 2.0,
                    y: cy - height / 2.0,
                    width,
                    height,
                },
                score,
            });
        }
    }
    faces
}

/// Greedy non-maximum suppression, highest score first.
pub fn nms(mut faces: Vec<FaceDetection>, iou_threshold: f32) -> Vec<FaceDetection> {
    faces.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<FaceDetection> = Vec::new();
    for face in faces {
        if kept
            .iter()
            .all(|other| iou(&face.bbox, &other.bbox) <= iou_threshold)
        {
            kept.push(face);
        }
    }
    kept
}

pub fn iou(a: &BBox, b: &BBox) -> f32 {
    let x1 = a.x.max(b.x);
    let y1 = a.y.max(b.y);
    let x2 = (a.x + a.width).min(b.x + b.width);
    let y2 = (a.y + a.height).min(b.y + b.height);
    let intersection = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let union = a.width * a.height + b.width * b.height - intersection;
    if union <= 0.0 {
        0.0
    } else {
        intersection / union
    }
}

#[cfg(feature = "vision")]
fn tensor_at(outputs: &ort::session::SessionOutputs<'_>, name: &str) -> ProviderResult<Vec<f32>> {
    let (_, data) = outputs
        .get(name)
        .ok_or_else(|| ProviderError::Inference(format!("face output `{name}` is missing")))?
        .try_extract_tensor::<f32>()
        .map_err(|e| ProviderError::Inference(e.to_string()))?;
    Ok(data.to_vec())
}

#[cfg(feature = "vision")]
fn build_session(path: &std::path::Path, threads: usize) -> ProviderResult<Session> {
    use ort::session::builder::AutoDevicePolicy;
    let environment = crate::onnx::environment()?;
    Session::builder(environment)
        .map_err(|error| ProviderError::Inference(error.to_string()))?
        .with_auto_device(AutoDevicePolicy::PreferCPU)
        .map_err(|error| ProviderError::Inference(error.to_string()))?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|error| ProviderError::Inference(error.to_string()))?
        .with_intra_threads(threads.max(1))
        .map_err(|error| ProviderError::Inference(error.to_string()))?
        .commit_from_file(path)
        .map_err(|error| {
            ProviderError::Inference(format!("could not load {}: {error}", path.display()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bbox(x: f32, y: f32, width: f32, height: f32) -> BBox {
        BBox {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn one_cell_decodes_to_a_centered_box() {
        let cls = [1.0];
        let obj = [1.0];
        // Center offset 0.5, log-size 0 → one stride-sized box (8 px).
        let bbox_values = [0.5, 0.5, 0.0, 0.0];
        let faces = decode_stride(&cls, &obj, &bbox_values, 8, 8, 0.6);
        assert_eq!(faces.len(), 1);
        assert!((faces[0].bbox.width - 8.0).abs() < 1e-5);
        assert!((faces[0].bbox.x - (4.0 - 4.0)).abs() < 1e-5);
        assert!((faces[0].bbox.y - (4.0 - 4.0)).abs() < 1e-5);
    }

    #[test]
    fn low_scores_are_dropped() {
        let faces = decode_stride(&[0.3], &[0.3], &[0.0; 4], 8, 8, 0.6);
        assert!(faces.is_empty());
    }

    #[test]
    fn nms_keeps_the_highest_score() {
        let faces = vec![
            FaceDetection {
                bbox: bbox(0.0, 0.0, 10.0, 10.0),
                score: 0.7,
            },
            FaceDetection {
                bbox: bbox(1.0, 1.0, 10.0, 10.0),
                score: 0.9,
            },
            FaceDetection {
                bbox: bbox(50.0, 50.0, 10.0, 10.0),
                score: 0.8,
            },
        ];
        let kept = nms(faces, 0.3);
        assert_eq!(kept.len(), 2);
        assert!((kept[0].score - 0.9).abs() < 1e-6);
        assert!((kept[1].score - 0.8).abs() < 1e-6);
        assert!(iou(&bbox(0.0, 0.0, 10.0, 10.0), &bbox(0.0, 0.0, 10.0, 10.0)) > 0.99);
    }
}
