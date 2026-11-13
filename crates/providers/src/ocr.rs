//! Local OCR: PP-OCRv5 mobile detection + recognition through ONNX Runtime.
//!
//! The pipeline is the one measured in `docs/OCR.md`:
//!
//! 1. Decode a JPEG keyframe to RGB.
//! 2. Detection: resize the long side to 1280 with both dimensions rounded to
//!    multiples of 32, feed BGR normalized with the ImageNet mean/std, get an
//!    already-sigmoided probability map, threshold at 0.3, find connected
//!    components with an explicit stack, box them, and unclip.
//! 3. Merge boxes that belong to the same text line, then sort.
//! 4. Recognition: crop each line, resize to height 48 with a width derived
//!    from the aspect ratio, feed BGR in `[-1, 1]`, and CTC-greedy-decode with
//!    `[blank] + dictionary + space`.
//!
//! Everything that does not need the model runtime (geometry, CTC decoding,
//! input sizing) is compiled unconditionally so it can be unit-tested in a
//! build without ONNX Runtime.

// Those helpers are used by the `ocr` feature and by the tests; a minimal
// build keeps them only for the latter, so `dead_code` is expected there.
#![cfg_attr(not(feature = "ocr"), allow(dead_code))]

use std::sync::OnceLock;

use sonolytica_core::BBox;

use crate::models::ModelStore;
use crate::traits::{ProviderError, ProviderInfo, ProviderResult};

#[cfg(feature = "ocr")]
use sonolytica_core::Capability;

#[cfg(feature = "ocr")]
use std::sync::Mutex;

#[cfg(feature = "ocr")]
use image::ImageFormat;
#[cfg(feature = "ocr")]
use image::RgbImage;
#[cfg(feature = "ocr")]
use image::imageops::FilterType;
#[cfg(feature = "ocr")]
use ort::session::Session;
#[cfg(feature = "ocr")]
use ort::session::builder::GraphOptimizationLevel;
#[cfg(feature = "ocr")]
use ort::value::Tensor;

pub const DET_MODEL_ID: &str = "pp-ocrv5-det";
pub const REC_MODEL_ID: &str = "pp-ocrv5-rec";

/// Normalised character dictionary from the recognition model's
/// `inference.yml`: 18383 lines, the first one U+3000. It must be loaded
/// verbatim; trimming or re-quoting shifts every punctuation index.
const DICT: &str = include_str!("ocr_dict.txt");

/// Long side of the detection input. 960 loses spaces at 16 px; 1280 was the
/// measured setting for keyframes extracted at 1280 px.
const DET_LIMIT: usize = 1280;
const DET_STRIDE: usize = 32;
const DET_THRESHOLD: f32 = 0.3;
const BOX_THRESHOLD: f32 = 0.6;
const UNCLIP_RATIO: f32 = 1.5;
const MAX_CANDIDATES: usize = 1000;
const MIN_BOX_SIDE: usize = 3;

/// Recognition input height and the maximum dynamic width. The width cap keeps
/// the `[1, T, 18385]` output at or below ~51 MB.
const REC_HEIGHT: usize = 48;
const REC_MAX_WIDTH: usize = 4000;

/// One recognised line of text with its box in source-image pixels.
#[derive(Debug, Clone)]
pub struct OcrLine {
    pub text: String,
    /// Mean probability of the emitted CTC tokens, `0.0..=1.0`.
    pub confidence: f32,
    pub bbox: BBox,
}

#[derive(Debug, Clone, Default)]
pub struct OcrOutput {
    pub width: u32,
    pub height: u32,
    pub lines: Vec<OcrLine>,
}

/// Axis-aligned box in source-image pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
struct DetBox {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
}

impl DetBox {
    fn height(self) -> f32 {
        self.y2 - self.y1
    }

    fn union(self, other: Self) -> Self {
        Self {
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
            x2: self.x2.max(other.x2),
            y2: self.y2.max(other.y2),
        }
    }
}

/// Detection input dimensions: long side at most [`DET_LIMIT`] (never
/// upscaled), both sides rounded to a multiple of 32.
///
/// The model silently rounds a non-multiple input internally, which breaks the
/// coordinate mapping, so this must be the same rounding the model does.
/// Ties round to even, matching the measured Python reference (1280x720 becomes
/// 1280x704, not 1280x736).
fn detect_size(width: usize, height: usize) -> (usize, usize) {
    let long_side = width.max(height).max(1);
    let scale = (DET_LIMIT as f32 / long_side as f32).min(1.0);
    let round = |value: usize| {
        let scaled = (value as f32 * scale / DET_STRIDE as f32).round_ties_even() as usize;
        (scaled * DET_STRIDE).max(DET_STRIDE)
    };
    (round(width), round(height))
}

/// Recognition width for a crop: `ceil(48 * w / h)`, rounded up to a multiple
/// of 8, capped at [`REC_MAX_WIDTH`]. A fixed width fragments wide lines.
fn rec_width(width: usize, height: usize) -> usize {
    if height == 0 {
        return 8;
    }
    let scaled = (REC_HEIGHT * width)
        .div_ceil(height)
        .clamp(8, REC_MAX_WIDTH);
    scaled.div_ceil(8) * 8
}

/// The CTC table: `[blank] + dictionary + space`, 18385 classes.
fn ctc_table() -> &'static [&'static str] {
    static TABLE: OnceLock<Vec<&'static str>> = OnceLock::new();
    TABLE.get_or_init(|| {
        std::iter::once("")
            .chain(DICT.lines())
            .chain(std::iter::once(" "))
            .collect()
    })
}

/// Greedy CTC decoding over a `[timesteps, classes]` probability matrix.
///
/// A token is emitted when its index differs from the blank index (0) and from
/// the previous timestep's index. Confidence is the mean probability of the
/// emitted tokens; an empty transcription has confidence 0.
fn ctc_greedy_decode(
    probabilities: &[f32],
    timesteps: usize,
    classes: usize,
    table: &[&str],
) -> (String, f32) {
    if classes == 0 || probabilities.len() < timesteps * classes {
        return (String::new(), 0.0);
    }
    let mut text = String::new();
    let mut total = 0.0_f64;
    let mut emitted = 0_usize;
    let mut previous = usize::MAX;

    for timestep in 0..timesteps {
        let row = &probabilities[timestep * classes..(timestep + 1) * classes];
        let mut best_index = 0;
        let mut best_probability = f32::NEG_INFINITY;
        for (index, &probability) in row.iter().enumerate() {
            if probability > best_probability {
                best_probability = probability;
                best_index = index;
            }
        }
        if best_index != 0 && best_index != previous {
            if let Some(token) = table.get(best_index) {
                text.push_str(token);
                total += f64::from(best_probability);
                emitted += 1;
            }
        }
        previous = best_index;
    }

    let confidence = if emitted == 0 {
        0.0
    } else {
        (total / emitted as f64) as f32
    };
    (text, confidence)
}

/// True when two boxes sit on the same text line.
///
/// Vertical overlap is measured against the taller box and the horizontal gap
/// against the shorter one: both are conservative, so an accidental merge of
/// two lines is less likely than a split of one line, which the merge loop
/// repairs repeatedly until no pair matches.
fn same_line(a: &DetBox, b: &DetBox) -> bool {
    let overlap = (a.y2.min(b.y2) - a.y1.max(b.y1)).max(0.0);
    let tallest = a.height().max(b.height());
    let shortest = a.height().min(b.height());
    if tallest <= 0.0 || shortest <= 0.0 {
        return false;
    }
    let gap = (b.x1 - a.x2).max(a.x1 - b.x2);
    overlap > 0.6 * tallest && gap < 0.8 * shortest
}

/// Merge boxes belonging to one text line and sort top-to-bottom, then
/// left-to-right (by vertical centre).
fn merge_line_boxes(boxes: Vec<DetBox>) -> Vec<DetBox> {
    let mut merged: Vec<DetBox> = Vec::with_capacity(boxes.len());
    for candidate in boxes {
        let mut current = candidate;
        'grow: loop {
            for index in 0..merged.len() {
                if same_line(&merged[index], &current) {
                    let other = merged.remove(index);
                    current = current.union(other);
                    continue 'grow;
                }
            }
            break;
        }
        merged.push(current);
    }
    merged.sort_by(|a, b| {
        let a_centre = a.y1 + a.y2;
        let b_centre = b.y1 + b.y2;
        a_centre.total_cmp(&b_centre).then(a.x1.total_cmp(&b.x1))
    });
    merged
}

/// DB post-processing over the detection probability map.
///
/// `probabilities` is `[det_height, det_width]`, already sigmoided. The
/// returned boxes are in source-image pixels, merged and sorted.
fn detection_boxes(
    probabilities: &[f32],
    det_width: usize,
    det_height: usize,
    width: usize,
    height: usize,
) -> Vec<DetBox> {
    if probabilities.len() != det_width * det_height {
        return Vec::new();
    }

    // Explicit label array plus an iterative stack: a recursive flood fill on
    // a 1280x704 mask overflows the stack.
    let mut labels = vec![0_u32; probabilities.len()];
    let mut stack: Vec<usize> = Vec::new();
    let mut next_label = 0_u32;
    let mut boxes = Vec::new();

    for start in 0..probabilities.len() {
        if probabilities[start] <= DET_THRESHOLD || labels[start] != 0 {
            continue;
        }
        if next_label as usize >= MAX_CANDIDATES {
            break;
        }
        next_label += 1;
        labels[start] = next_label;
        stack.push(start);

        let (mut x1, mut x2, mut y1, mut y2) = (usize::MAX, 0_usize, usize::MAX, 0_usize);
        while let Some(index) = stack.pop() {
            let y = index / det_width;
            let x = index % det_width;
            x1 = x1.min(x);
            x2 = x2.max(x + 1);
            y1 = y1.min(y);
            y2 = y2.max(y + 1);

            for dy in -1_i32..=1 {
                for dx in -1_i32..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0 || ny < 0 || nx >= det_width as i32 || ny >= det_height as i32 {
                        continue;
                    }
                    let neighbour = ny as usize * det_width + nx as usize;
                    if probabilities[neighbour] > DET_THRESHOLD && labels[neighbour] == 0 {
                        labels[neighbour] = next_label;
                        stack.push(neighbour);
                    }
                }
            }
        }

        let box_width = x2 - x1;
        let box_height = y2 - y1;
        if box_width.min(box_height) < MIN_BOX_SIDE {
            continue;
        }

        // The reference checks the mean over the bounding rectangle, not just
        // the component pixels, which rejects speckle that happens to cluster.
        let mut sum = 0.0_f32;
        for row in y1..y2 {
            let base = row * det_width;
            for &probability in &probabilities[base + x1..base + x2] {
                sum += probability;
            }
        }
        let mean = sum / (box_width * box_height) as f32;
        if mean < BOX_THRESHOLD {
            continue;
        }

        // DB unclip: the probability map covers a shrunk text region, so the
        // box is expanded by area*ratio/perimeter on every side. Empirically
        // this is what makes recognition work; a tight crop loses glyph edges
        // and the first/last characters.
        let pad = (box_width as f32 * box_height as f32 * UNCLIP_RATIO)
            / (2.0 * (box_width + box_height) as f32);
        let scale_x = width as f32 / det_width as f32;
        let scale_y = height as f32 / det_height as f32;
        boxes.push(DetBox {
            x1: (x1 as f32 - pad).max(0.0) * scale_x,
            y1: (y1 as f32 - pad).max(0.0) * scale_y,
            x2: (x2 as f32 + pad).min(det_width as f32) * scale_x,
            y2: (y2 as f32 + pad).min(det_height as f32) * scale_y,
        });
    }

    merge_line_boxes(boxes)
}

/// The OCR provider: two ONNX sessions and the shared runtime environment.
pub struct PaddleOcr {
    #[cfg(feature = "ocr")]
    detection: Mutex<Session>,
    #[cfg(feature = "ocr")]
    recognition: Mutex<Session>,
    info: ProviderInfo,
}

impl std::fmt::Debug for PaddleOcr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaddleOcr")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

/// Build an OCR session pinned to the CPU execution provider.
///
/// `ort`'s default device policy is `MaxEfficiency`, which on macOS
/// automatically registers the CoreML EP. CoreML mis-executes PP-OCRv5's
/// dynamic-shape graphs (detection collapses to a few blobs, recognition emits
/// blank at every timestep), so OCR explicitly asks for the CPU EP.
#[cfg(feature = "ocr")]
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

impl PaddleOcr {
    #[cfg(feature = "ocr")]
    pub fn load(store: &ModelStore, threads: usize) -> ProviderResult<Self> {
        let detection_spec = crate::models::find_model(DET_MODEL_ID).ok_or_else(|| {
            ProviderError::Inference("the OCR detection model is not in this registry".into())
        })?;
        let recognition_spec = crate::models::find_model(REC_MODEL_ID).ok_or_else(|| {
            ProviderError::Inference("the OCR recognition model is not in this registry".into())
        })?;
        let detection_path = store.require(detection_spec)?;
        let recognition_path = store.require(recognition_spec)?;

        Ok(Self {
            detection: Mutex::new(build_session(&detection_path, threads)?),
            recognition: Mutex::new(build_session(&recognition_path, threads)?),
            info: ProviderInfo {
                name: "pp-ocrv5",
                capability: Capability::Vision,
                device: "cpu",
                models: vec![detection_spec.reference(), recognition_spec.reference()],
                notes: "Keyframes only: DB detection at 1280 px plus CTC recognition. \
                        Boxes are axis-aligned and same-line fragments are merged; rotated text \
                        and text smaller than 3 px are not recovered.",
            },
        })
    }

    #[cfg(not(feature = "ocr"))]
    pub fn load(_store: &ModelStore, _threads: usize) -> ProviderResult<Self> {
        Err(ProviderError::Unsupported(
            "this build was compiled without the `ocr` feature; rebuild with `--features full` \
             (or `--features sonolytica-providers/ocr`)"
                .into(),
        ))
    }

    pub fn info(&self) -> ProviderInfo {
        self.info.clone()
    }

    #[cfg(feature = "ocr")]
    pub fn recognize_jpeg(&self, bytes: &[u8]) -> ProviderResult<OcrOutput> {
        let decoded = image::load_from_memory_with_format(bytes, ImageFormat::Jpeg)
            .map_err(|e| ProviderError::Unsupported(format!("cannot decode JPEG: {e}")))?;
        let rgb = decoded.into_rgb8();
        let (width, height) = rgb.dimensions();
        self.recognize_rgb(width as usize, height as usize, rgb.as_raw())
    }

    #[cfg(not(feature = "ocr"))]
    pub fn recognize_jpeg(&self, _bytes: &[u8]) -> ProviderResult<OcrOutput> {
        Err(ProviderError::Unsupported(
            "this build was compiled without the `ocr` feature".into(),
        ))
    }

    #[cfg(feature = "ocr")]
    pub fn recognize_rgb(
        &self,
        width: usize,
        height: usize,
        rgb: &[u8],
    ) -> ProviderResult<OcrOutput> {
        if width == 0 || height == 0 || rgb.len() != width * height * 3 {
            return Err(ProviderError::Unsupported(format!(
                "RGB buffer of {} bytes does not match {width}x{height}",
                rgb.len()
            )));
        }
        let image = RgbImage::from_raw(width as u32, height as u32, rgb.to_vec())
            .ok_or_else(|| ProviderError::Inference("could not wrap the RGB buffer".into()))?;

        let boxes = self.detect(&image)?;
        let mut lines = Vec::with_capacity(boxes.len());
        for bbox in boxes {
            if let Some(line) = self.recognize_line(&image, &bbox)? {
                lines.push(line);
            }
        }

        Ok(OcrOutput {
            width: width as u32,
            height: height as u32,
            lines,
        })
    }

    #[cfg(not(feature = "ocr"))]
    pub fn recognize_rgb(
        &self,
        _width: usize,
        _height: usize,
        _rgb: &[u8],
    ) -> ProviderResult<OcrOutput> {
        Err(ProviderError::Unsupported(
            "this build was compiled without the `ocr` feature".into(),
        ))
    }

    #[cfg(feature = "ocr")]
    fn detect(&self, image: &RgbImage) -> ProviderResult<Vec<DetBox>> {
        let (width, height) = (image.width() as usize, image.height() as usize);
        let (det_width, det_height) = detect_size(width, height);
        let resized = image::imageops::resize(
            image,
            det_width as u32,
            det_height as u32,
            FilterType::Triangle,
        );

        // BGR, /255, ImageNet mean/std, CHW.
        const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
        const STD: [f32; 3] = [0.229, 0.224, 0.225];
        let plane = det_width * det_height;
        let mut input = vec![0.0_f32; 3 * plane];
        for y in 0..det_height {
            for x in 0..det_width {
                let pixel = resized.get_pixel(x as u32, y as u32);
                for channel in 0..3 {
                    let value = f32::from(pixel[2 - channel]) / 255.0;
                    input[channel * plane + y * det_width + x] =
                        (value - MEAN[channel]) / STD[channel];
                }
            }
        }

        let tensor =
            Tensor::from_array((vec![1_i64, 3, det_height as i64, det_width as i64], input))
                .map_err(|e| ProviderError::Inference(e.to_string()))?;

        let mut session = self
            .detection
            .lock()
            .map_err(|_| ProviderError::Inference("detection session poisoned".into()))?;
        let outputs = session
            .run(ort::inputs!["x" => tensor])
            .map_err(|e| ProviderError::Inference(format!("detection run failed: {e}")))?;
        let (shape, data) = outputs
            .get("fetch_name_0")
            .ok_or_else(|| ProviderError::Inference("detection output is missing".into()))?
            .try_extract_tensor::<f32>()
            .map_err(|e| ProviderError::Inference(e.to_string()))?;

        let dims: Vec<usize> = shape.iter().map(|dimension| *dimension as usize).collect();
        if dims != [1, 1, det_height, det_width] {
            return Err(ProviderError::Inference(format!(
                "detection output shape {dims:?} does not match the {det_width}x{det_height} \
                 input; the coordinate mapping would be wrong"
            )));
        }

        Ok(detection_boxes(data, det_width, det_height, width, height))
    }

    #[cfg(feature = "ocr")]
    fn recognize_line(&self, image: &RgbImage, bbox: &DetBox) -> ProviderResult<Option<OcrLine>> {
        let (width, height) = (image.width() as usize, image.height() as usize);
        let x1 = bbox.x1.floor().max(0.0) as usize;
        let y1 = bbox.y1.floor().max(0.0) as usize;
        let x2 = (bbox.x2.ceil().max(0.0) as usize).min(width);
        let y2 = (bbox.y2.ceil().max(0.0) as usize).min(height);
        if x2 <= x1 || y2 <= y1 {
            return Ok(None);
        }

        let crop_width = x2 - x1;
        let crop_height = y2 - y1;
        let mut crop = RgbImage::new(crop_width as u32, crop_height as u32);
        let source = image.as_raw();
        let destination = crop.as_mut();
        for row in 0..crop_height {
            let from = ((y1 + row) * width + x1) * 3;
            let to = row * crop_width * 3;
            destination[to..to + crop_width * 3]
                .copy_from_slice(&source[from..from + crop_width * 3]);
        }

        let target_width = rec_width(crop_width, crop_height);
        let resized = image::imageops::resize(
            &crop,
            target_width as u32,
            REC_HEIGHT as u32,
            FilterType::CatmullRom,
        );
        let plane = target_width * REC_HEIGHT;
        let mut input = vec![0.0_f32; 3 * plane];
        for y in 0..REC_HEIGHT {
            for x in 0..target_width {
                let pixel = resized.get_pixel(x as u32, y as u32);
                for channel in 0..3 {
                    input[channel * plane + y * target_width + x] =
                        f32::from(pixel[2 - channel]) / 127.5 - 1.0;
                }
            }
        }

        let tensor = Tensor::from_array((
            vec![1_i64, 3, REC_HEIGHT as i64, target_width as i64],
            input,
        ))
        .map_err(|e| ProviderError::Inference(e.to_string()))?;
        let mut session = self
            .recognition
            .lock()
            .map_err(|_| ProviderError::Inference("recognition session poisoned".into()))?;
        let outputs = session
            .run(ort::inputs!["x" => tensor])
            .map_err(|e| ProviderError::Inference(format!("recognition run failed: {e}")))?;
        let (shape, data) = outputs
            .get("fetch_name_0")
            .ok_or_else(|| ProviderError::Inference("recognition output is missing".into()))?
            .try_extract_tensor::<f32>()
            .map_err(|e| ProviderError::Inference(e.to_string()))?;

        let dims: Vec<usize> = shape.iter().map(|dimension| *dimension as usize).collect();
        if dims.len() != 3 || dims[0] != 1 {
            return Err(ProviderError::Inference(format!(
                "unexpected recognition output shape {dims:?}"
            )));
        }
        let table = ctc_table();
        if dims[2] != table.len() {
            return Err(ProviderError::Inference(format!(
                "recognition model has {} classes but the dictionary has {}",
                dims[2],
                table.len()
            )));
        }

        let (text, confidence) = ctc_greedy_decode(data, dims[1], dims[2], table);
        if text.trim().is_empty() {
            return Ok(None);
        }

        Ok(Some(OcrLine {
            text,
            confidence,
            bbox: BBox {
                x: x1 as f32,
                y: y1 as f32,
                width: crop_width as f32,
                height: crop_height as f32,
            },
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `[timesteps, classes]` matrix where one class per timestep is
    /// `probability` and the rest are zero.
    fn matrix(steps: &[(usize, f32)], classes: usize) -> Vec<f32> {
        let mut out = vec![0.0_f32; steps.len() * classes];
        for (timestep, &(index, probability)) in steps.iter().enumerate() {
            out[timestep * classes + index] = probability;
        }
        out
    }

    #[test]
    fn ctc_skips_blanks_and_adjacent_repeats() {
        // a a blank b b -> "ab"
        let table = ["", "a", "b", " "];
        let probabilities = matrix(&[(1, 0.9), (1, 0.8), (0, 0.7), (2, 0.7), (2, 0.6)], 4);
        let (text, confidence) = ctc_greedy_decode(&probabilities, 5, 4, &table);
        assert_eq!(text, "ab");
        assert!((confidence - 0.8).abs() < 1e-6, "got {confidence}");
    }

    #[test]
    fn ctc_keeps_a_repeat_separated_by_a_blank() {
        let table = ["", "a", "b", " "];
        let probabilities = matrix(&[(1, 0.9), (0, 0.9), (1, 0.8)], 4);
        let (text, _) = ctc_greedy_decode(&probabilities, 3, 4, &table);
        assert_eq!(text, "aa");
    }

    #[test]
    fn ctc_emits_spaces_and_handles_an_empty_transcription() {
        let table = ["", "a", "b", " "];
        let probabilities = matrix(&[(1, 0.9), (3, 0.9), (2, 0.8)], 4);
        let (text, confidence) = ctc_greedy_decode(&probabilities, 3, 4, &table);
        assert_eq!(text, "a b");
        assert!(confidence > 0.8);

        let blank = matrix(&[(0, 1.0), (0, 1.0)], 4);
        assert_eq!(
            ctc_greedy_decode(&blank, 2, 4, &table),
            (String::new(), 0.0)
        );
    }

    #[test]
    fn the_shipped_dictionary_has_the_pinned_order() {
        let table = ctc_table();
        assert_eq!(
            table.len(),
            18_385,
            "18383 dictionary entries + blank + space"
        );
        assert_eq!(table[0], "", "blank is index 0");
        assert_eq!(
            table[1], "\u{3000}",
            "dictionary line 0 is ideographic space"
        );
        assert_eq!(table.last(), Some(&" "), "the last class is a space");
        assert_eq!(DICT.lines().count(), 18_383);
    }

    #[test]
    fn detection_input_is_rounded_to_thirty_two() {
        assert_eq!(detect_size(1280, 720), (1280, 704));
        assert_eq!(detect_size(960, 541), (960, 544));
        assert_eq!(detect_size(1920, 1080), (1280, 704));
        // Never upscales the long side.
        assert_eq!(detect_size(640, 360), (640, 352));
    }

    #[test]
    fn recognition_width_is_aspect_preserving_and_capped() {
        assert_eq!(rec_width(100, 48), 104);
        assert_eq!(rec_width(80, 48), 80);
        assert_eq!(rec_width(1, 100), 8, "tiny crops still get a valid width");
        assert_eq!(rec_width(100_000, 48), 4000, "width is capped");
        assert_eq!(rec_width(4000, 48), 4000);
    }

    fn bbox(x1: f32, y1: f32, x2: f32, y2: f32) -> DetBox {
        DetBox { x1, y1, x2, y2 }
    }

    #[test]
    fn same_line_fragments_merge_into_one_box() {
        let boxes = vec![
            bbox(10.0, 10.0, 40.0, 30.0),  // "$"
            bbox(45.0, 10.0, 200.0, 30.0), // rest of the line
            bbox(10.0, 60.0, 200.0, 80.0), // next line
        ];
        let merged = merge_line_boxes(boxes);
        assert_eq!(merged.len(), 2, "{merged:?}");
        assert_eq!(merged[0], bbox(10.0, 10.0, 200.0, 30.0));
        assert_eq!(merged[1], bbox(10.0, 60.0, 200.0, 80.0));
    }

    #[test]
    fn different_lines_and_wide_gaps_stay_separate() {
        let boxes = vec![
            bbox(10.0, 10.0, 100.0, 30.0),
            bbox(10.0, 40.0, 100.0, 60.0), // next line, no vertical overlap
            bbox(300.0, 10.0, 400.0, 30.0), // same line, gap far too wide
        ];
        let merged = merge_line_boxes(boxes);
        assert_eq!(merged.len(), 3, "{merged:?}");
    }

    #[test]
    fn merged_lines_are_sorted_top_to_bottom_then_left_to_right() {
        let boxes = vec![
            bbox(400.0, 60.0, 500.0, 80.0),
            bbox(10.0, 10.0, 100.0, 30.0),
            bbox(400.0, 10.0, 500.0, 30.0), // same row, far apart, must not merge
        ];
        let merged = merge_line_boxes(boxes);
        assert_eq!(merged.len(), 3, "{merged:?}");
        assert!((merged[0].x1 - 10.0).abs() < 1e-3, "{merged:?}");
        assert!((merged[1].x1 - 400.0).abs() < 1e-3, "{merged:?}");
        assert!((merged[2].y1 - 60.0).abs() < 1e-3, "{merged:?}");
    }

    #[test]
    fn detection_rejects_specks_and_maps_boxes_back() {
        // A 64x32 detection map scaled to a 128x64 source: a 10x10 blob at
        // (20, 10) becomes (40, 20)..(60, 40) before unclip.
        let (det_width, det_height) = (64_usize, 32_usize);
        let mut probabilities = vec![0.0_f32; det_width * det_height];
        for y in 10..20 {
            for x in 20..30 {
                probabilities[y * det_width + x] = 0.9;
            }
        }
        // Speck below the minimum side.
        probabilities[2 * det_width + 50] = 0.9;

        let boxes = detection_boxes(&probabilities, det_width, det_height, 128, 64);
        assert_eq!(boxes.len(), 1, "{boxes:?}");
        let found = boxes[0];
        let pad = (10.0 * 10.0 * UNCLIP_RATIO) / (2.0 * 20.0);
        assert!((found.x1 - (20.0 - pad) * 2.0).abs() < 1e-3, "{found:?}");
        assert!((found.x2 - (30.0 + pad) * 2.0).abs() < 1e-3, "{found:?}");
        assert!((found.y1 - (10.0 - pad) * 2.0).abs() < 1e-3, "{found:?}");
        assert!((found.y2 - (20.0 + pad) * 2.0).abs() < 1e-3, "{found:?}");
    }

    #[test]
    fn a_mismatched_probability_map_is_ignored() {
        assert!(detection_boxes(&[0.9; 10], 8, 8, 64, 64).is_empty());
    }

    /// Opt-in end-to-end test: FFmpeg draws two known lines, the real models
    /// read them back.
    ///
    /// Run with the pinned models present:
    ///   SONOLYTICA_E2E=1 cargo test -p sonolytica-providers --features ocr \
    ///     ocr::tests::drawn_text_is_recognised -- --nocapture
    #[cfg(feature = "ocr")]
    #[test]
    fn drawn_text_is_recognised() {
        if std::env::var("SONOLYTICA_E2E").is_err() {
            eprintln!("skipped: set SONOLYTICA_E2E=1 to run the model-backed OCR test");
            return;
        }

        let store = ModelStore::default();
        let (Some(detection), Some(recognition)) = (
            crate::models::find_model(DET_MODEL_ID),
            crate::models::find_model(REC_MODEL_ID),
        ) else {
            eprintln!("skipped: the OCR models are not in this build's registry");
            return;
        };
        if !store.is_present(detection) || !store.is_present(recognition) {
            eprintln!("skipped: run `sonolytica models fetch {DET_MODEL_ID} {REC_MODEL_ID}` first");
            return;
        }

        let font = [
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/dejavu/DejaVuSans.ttf",
            r"C:\Windows\Fonts\arial.ttf",
            r"C:\Windows\Fonts\segoeui.ttf",
        ]
        .into_iter()
        .find(|path| std::path::Path::new(path).is_file())
        .expect("a TrueType font is needed for drawtext");

        let directory = tempfile::tempdir().expect("temp dir");
        let image = directory.path().join("drawn.jpg");
        let filter = format!(
            "drawtext=fontfile='{font}':text='SONOLYTICA OCR TEST':fontcolor=black:fontsize=64:\
             x=80:y=120,drawtext=fontfile='{font}':text='hello world 123':fontcolor=black:\
             fontsize=48:x=80:y=260"
        );
        let output = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
            ])
            .arg("color=c=white:s=1280x720")
            .args(["-vf", &filter, "-frames:v", "1"])
            .arg(&image)
            .output()
            .expect("ffmpeg is required");
        if !output.status.success()
            && String::from_utf8_lossy(&output.stderr).contains("No such filter")
        {
            eprintln!("skipped: this ffmpeg was built without the drawtext filter (libfreetype)");
            return;
        }
        assert!(
            output.status.success(),
            "ffmpeg drawtext failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let provider = PaddleOcr::load(&store, 4).expect("OCR models load");
        let bytes = std::fs::read(&image).expect("read the drawn frame");
        let recognised = provider.recognize_jpeg(&bytes).expect("OCR runs");
        let text = recognised
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        eprintln!("recognised: {text}");
        assert!(
            text.contains("sonolytica"),
            "expected SONOLYTICA, got {text}"
        );
        assert!(text.contains("hello"), "expected hello, got {text}");
        assert!(
            recognised.lines.iter().all(|line| line.bbox.width > 0.0),
            "every line carries a box"
        );
    }
}
