//! Body pose with MoveNet Multipose Lightning (ONNX, Apache-2.0).
//!
//! One image per call, at most six people, 17 COCO keypoints per person. The
//! input is a 256×256 letterboxed RGB image as `int32`; the output is
//! `[1, 6, 56]` = six rows of `17 × (y, x, score)` followed by
//! `[ymin, xmin, ymax, xmax, person_score]`.
//!
//! Detection only: no tracking, no identity, and no claim about frames that
//! were not sampled. Everything that does not need ONNX Runtime is compiled
//! unconditionally so a model-free build can test the decode.

#![cfg_attr(not(feature = "vision"), allow(dead_code))]

use sonolytica_core::{BBox, Keypoint};

use crate::models::ModelStore;
use crate::traits::{ProviderError, ProviderInfo, ProviderResult};

pub const MODEL_ID: &str = "movenet-multipose";
pub const INPUT: usize = 256;
pub const MAX_PEOPLE: usize = 6;
const VALUES_PER_PERSON: usize = 56;
const KEYPOINTS: usize = 17;
pub const PERSON_THRESHOLD: f32 = 0.3;
pub const KEYPOINT_THRESHOLD: f32 = 0.3;

/// COCO keypoint order used by MoveNet.
pub const KEYPOINT_NAMES: [&str; KEYPOINTS] = [
    "nose",
    "left_eye",
    "right_eye",
    "left_ear",
    "right_ear",
    "left_shoulder",
    "right_shoulder",
    "left_elbow",
    "right_elbow",
    "left_wrist",
    "right_wrist",
    "left_hip",
    "right_hip",
    "left_knee",
    "right_knee",
    "left_ankle",
    "right_ankle",
];

#[derive(Debug, Clone)]
pub struct PoseDetection {
    pub bbox: BBox,
    pub score: f32,
    pub keypoints: Vec<Keypoint>,
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
use ort::value::Tensor;

#[cfg(feature = "vision")]
use sonolytica_core::Capability;

pub struct MoveNet {
    #[cfg(feature = "vision")]
    session: Mutex<Session>,
    info: ProviderInfo,
}

impl std::fmt::Debug for MoveNet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MoveNet").field("info", &self.info).finish()
    }
}

impl MoveNet {
    #[cfg(feature = "vision")]
    pub fn load(store: &ModelStore, threads: usize) -> ProviderResult<Self> {
        let spec = crate::models::find_model(MODEL_ID).ok_or_else(|| {
            ProviderError::Inference("the pose model is not in this registry".into())
        })?;
        let path = store.require(spec)?;
        let session = crate::onnx::build_session(&path, threads)?;
        Ok(Self {
            session: Mutex::new(session),
            info: ProviderInfo {
                name: "movenet-multipose",
                capability: Capability::Vision,
                device: "cpu",
                models: vec![spec.reference()],
                notes: "Body pose on extracted keyframes: up to 6 people, 17 COCO keypoints, \
                        detection only. No tracking, no identity, no per-frame coverage.",
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
    pub fn detect_jpeg(&self, bytes: &[u8]) -> ProviderResult<Vec<PoseDetection>> {
        let decoded = image::load_from_memory_with_format(bytes, ImageFormat::Jpeg)
            .map_err(|e| ProviderError::Unsupported(format!("cannot decode JPEG: {e}")))?;
        let rgb = decoded.into_rgb8();
        let (width, height) = rgb.dimensions();
        self.detect_rgb(width as usize, height as usize, rgb.as_raw())
    }

    #[cfg(not(feature = "vision"))]
    pub fn detect_jpeg(&self, _bytes: &[u8]) -> ProviderResult<Vec<PoseDetection>> {
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
    ) -> ProviderResult<Vec<PoseDetection>> {
        if width == 0 || height == 0 || rgb.len() != width * height * 3 {
            return Err(ProviderError::Unsupported(
                "expected a non-empty RGB image".into(),
            ));
        }
        let source = image::RgbImage::from_raw(width as u32, height as u32, rgb.to_vec())
            .ok_or_else(|| ProviderError::Unsupported("invalid RGB buffer".into()))?;
        let (canvas, scale, pad_x, pad_y) = letterbox(&source);

        let mut input = Vec::with_capacity(INPUT * INPUT * 3);
        for pixel in canvas.pixels() {
            input.extend([
                i32::from(pixel[0]),
                i32::from(pixel[1]),
                i32::from(pixel[2]),
            ]);
        }
        let tensor = Tensor::from_array((vec![1_i64, INPUT as i64, INPUT as i64, 3], input))
            .map_err(|e| ProviderError::Inference(e.to_string()))?;

        let mut guard = self
            .session
            .lock()
            .map_err(|_| ProviderError::Inference("movenet session poisoned".into()))?;
        let outputs = guard
            .run(ort::inputs!["input" => tensor])
            .map_err(|e| ProviderError::Inference(format!("movenet run failed: {e}")))?;
        let (shape, data) = outputs
            .get("output_0")
            .ok_or_else(|| ProviderError::Inference("movenet output missing".into()))?
            .try_extract_tensor::<f32>()
            .map_err(|e| ProviderError::Inference(e.to_string()))?;
        let dims: Vec<usize> = shape.iter().map(|d| *d as usize).collect();
        if dims != [1, MAX_PEOPLE, VALUES_PER_PERSON] {
            return Err(ProviderError::Inference(format!(
                "unexpected movenet output shape {dims:?}"
            )));
        }

        Ok(decode(
            data,
            width as f32,
            height as f32,
            scale,
            pad_x,
            pad_y,
        ))
    }

    #[cfg(not(feature = "vision"))]
    pub fn detect_rgb(
        &self,
        _width: usize,
        _height: usize,
        _rgb: &[u8],
    ) -> ProviderResult<Vec<PoseDetection>> {
        Err(ProviderError::Unsupported(
            "this build was compiled without the `vision` feature".into(),
        ))
    }
}

/// Resize while preserving the aspect ratio, pad with black, and return the
/// scale and offsets needed to map normalised keypoints back.
#[cfg(feature = "vision")]
fn letterbox(source: &image::RgbImage) -> (image::RgbImage, f32, f32, f32) {
    let (width, height) = source.dimensions();
    let scale = (INPUT as f32 / width as f32).min(INPUT as f32 / height as f32);
    let new_width = ((width as f32 * scale).round() as u32).max(1);
    let new_height = ((height as f32 * scale).round() as u32).max(1);
    let resized = image::imageops::resize(source, new_width, new_height, FilterType::Triangle);
    let mut canvas = image::RgbImage::from_pixel(INPUT as u32, INPUT as u32, image::Rgb([0, 0, 0]));
    let pad_x = (INPUT as u32 - new_width) / 2;
    let pad_y = (INPUT as u32 - new_height) / 2;
    image::imageops::overlay(&mut canvas, &resized, i64::from(pad_x), i64::from(pad_y));
    (canvas, scale, pad_x as f32, pad_y as f32)
}

/// Decode the `[6, 56]` output. Coordinates in the row are normalised to the
/// letterboxed 256×256 input; this maps them back to the original image and
/// normalises again, so the document is resolution-independent.
fn decode(
    data: &[f32],
    width: f32,
    height: f32,
    scale: f32,
    pad_x: f32,
    pad_y: f32,
) -> Vec<PoseDetection> {
    let mut people = Vec::new();
    for person in 0..MAX_PEOPLE {
        let row = &data[person * VALUES_PER_PERSON..(person + 1) * VALUES_PER_PERSON];
        let person_score = row[55];
        if !person_score.is_finite() || person_score < PERSON_THRESHOLD {
            continue;
        }
        let mut keypoints = Vec::with_capacity(KEYPOINTS);
        for index in 0..KEYPOINTS {
            let y = row[index * 3];
            let x = row[index * 3 + 1];
            let score = row[index * 3 + 2];
            if !score.is_finite() || score < KEYPOINT_THRESHOLD {
                continue;
            }
            keypoints.push(Keypoint {
                name: KEYPOINT_NAMES[index].to_string(),
                x: map_axis(x, pad_x, scale, width),
                y: map_axis(y, pad_y, scale, height),
                score: score.clamp(0.0, 1.0),
            });
        }
        if keypoints.is_empty() {
            continue;
        }
        // Box order is [ymin, xmin, ymax, xmax].
        let ymin = map_axis(row[51], pad_y, scale, height);
        let xmin = map_axis(row[52], pad_x, scale, width);
        let ymax = map_axis(row[53], pad_y, scale, height);
        let xmax = map_axis(row[54], pad_x, scale, width);
        people.push(PoseDetection {
            bbox: BBox {
                x: xmin.min(xmax),
                y: ymin.min(ymax),
                width: (xmax - xmin).abs(),
                height: (ymax - ymin).abs(),
            },
            score: person_score.clamp(0.0, 1.0),
            keypoints,
        });
    }
    people
}

fn map_axis(value: f32, pad: f32, scale: f32, size: f32) -> f32 {
    ((value * INPUT as f32 - pad) / scale / size).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_with(nose: (f32, f32, f32), score: f32, bbox: [f32; 4]) -> Vec<f32> {
        let mut row = vec![0.0_f32; VALUES_PER_PERSON];
        row[0] = nose.0;
        row[1] = nose.1;
        row[2] = nose.2;
        row[51] = bbox[0];
        row[52] = bbox[1];
        row[53] = bbox[2];
        row[54] = bbox[3];
        row[55] = score;
        row
    }

    #[test]
    fn decodes_keypoints_and_box_in_original_coordinates() {
        // A 640×640 image is scaled by 0.4 with no padding. Nose at the centre
        // and a box covering the middle half.
        let mut data = vec![0.0_f32; MAX_PEOPLE * VALUES_PER_PERSON];
        data[..VALUES_PER_PERSON].copy_from_slice(&row_with(
            (0.5, 0.5, 0.9),
            0.8,
            [0.25, 0.25, 0.75, 0.75],
        ));
        let people = decode(&data, 640.0, 640.0, 0.4, 0.0, 0.0);
        assert_eq!(people.len(), 1);
        let person = &people[0];
        assert_eq!(person.keypoints.len(), 1);
        assert_eq!(person.keypoints[0].name, "nose");
        assert!((person.keypoints[0].x - 0.5).abs() < 1e-5);
        assert!((person.keypoints[0].y - 0.5).abs() < 1e-5);
        assert!((person.bbox.x - 0.25).abs() < 1e-5);
        assert!((person.bbox.width - 0.5).abs() < 1e-5);
    }

    #[test]
    fn letterbox_offset_is_removed() {
        // 640×480 -> scale 0.4, 256×192, 32 px pad top and bottom. A point at
        // y=0.5 in the padded input maps to the middle of the original image.
        let mut data = vec![0.0_f32; MAX_PEOPLE * VALUES_PER_PERSON];
        data[..VALUES_PER_PERSON].copy_from_slice(&row_with(
            (0.5, 0.5, 0.9),
            0.8,
            [0.25, 0.25, 0.75, 0.75],
        ));
        let people = decode(&data, 640.0, 480.0, 0.4, 0.0, 32.0);
        assert_eq!(people.len(), 1);
        assert!((people[0].keypoints[0].y - 0.5).abs() < 1e-5);
    }

    #[test]
    fn low_scores_are_not_claimed() {
        let mut data = vec![0.0_f32; MAX_PEOPLE * VALUES_PER_PERSON];
        data[..VALUES_PER_PERSON].copy_from_slice(&row_with(
            (0.5, 0.5, 0.9),
            0.1,
            [0.0, 0.0, 1.0, 1.0],
        ));
        assert!(decode(&data, 100.0, 100.0, 2.56, 0.0, 0.0).is_empty());
    }
}
