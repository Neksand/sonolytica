//! Object detection with YOLOS-tiny (ONNX, Apache-2.0).
//!
//! One image per call, DETR-style queries: sigmoid over 91 COCO classes,
//! threshold 0.5, boxes normalised to the resized image. No tracking, no
//! attributes, no identity — a class, a box and a score.

#![cfg_attr(not(feature = "vision"), allow(dead_code))]

use sonolytica_core::BBox;

use crate::models::ModelStore;
use crate::traits::{ProviderError, ProviderInfo, ProviderResult};

pub const MODEL_ID: &str = "yolos-tiny";
/// The processor's shortest edge; the long edge is capped at 1333.
pub const SHORTEST_EDGE: f32 = 512.0;
pub const LONGEST_EDGE: f32 = 1333.0;
pub const SCORE_THRESHOLD: f32 = 0.5;
pub const MAX_DETECTIONS: usize = 20;
const QUERIES: usize = 100;
const CLASSES: usize = 92;

/// COCO class names by the model's label id (0 is `N/A`, gaps are `N/A`).
pub const COCO: [&str; 91] = [
    "N/A",
    "person",
    "bicycle",
    "car",
    "motorcycle",
    "airplane",
    "bus",
    "train",
    "truck",
    "boat",
    "traffic light",
    "fire hydrant",
    "N/A",
    "stop sign",
    "parking meter",
    "bench",
    "bird",
    "cat",
    "dog",
    "horse",
    "sheep",
    "cow",
    "elephant",
    "bear",
    "zebra",
    "giraffe",
    "N/A",
    "backpack",
    "umbrella",
    "N/A",
    "N/A",
    "handbag",
    "tie",
    "suitcase",
    "frisbee",
    "skis",
    "snowboard",
    "sports ball",
    "kite",
    "baseball bat",
    "baseball glove",
    "skateboard",
    "surfboard",
    "tennis racket",
    "bottle",
    "N/A",
    "wine glass",
    "cup",
    "fork",
    "knife",
    "spoon",
    "bowl",
    "banana",
    "apple",
    "sandwich",
    "orange",
    "broccoli",
    "carrot",
    "hot dog",
    "pizza",
    "donut",
    "cake",
    "chair",
    "couch",
    "potted plant",
    "bed",
    "N/A",
    "dining table",
    "N/A",
    "N/A",
    "toilet",
    "N/A",
    "tv",
    "laptop",
    "mouse",
    "remote",
    "keyboard",
    "cell phone",
    "microwave",
    "oven",
    "toaster",
    "sink",
    "refrigerator",
    "N/A",
    "book",
    "clock",
    "vase",
    "scissors",
    "teddy bear",
    "hair drier",
    "toothbrush",
];

#[derive(Debug, Clone)]
pub struct ObjectDetection {
    pub class: String,
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

pub struct Yolos {
    #[cfg(feature = "vision")]
    session: Mutex<Session>,
    info: ProviderInfo,
}

impl std::fmt::Debug for Yolos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Yolos").field("info", &self.info).finish()
    }
}

impl Yolos {
    #[cfg(feature = "vision")]
    pub fn load(store: &ModelStore, threads: usize) -> ProviderResult<Self> {
        let spec = crate::models::find_model(MODEL_ID).ok_or_else(|| {
            ProviderError::Inference("the object detection model is not in this registry".into())
        })?;
        let path = store.require(spec)?;
        let session = build_session(&path, threads)?;
        Ok(Self {
            session: Mutex::new(session),
            info: ProviderInfo {
                name: "yolos-tiny",
                capability: Capability::Vision,
                device: "cpu",
                models: vec![spec.reference()],
                notes: "COCO object detection on keyframes: 91 classes, score and box. \
                        No tracking, attributes or identity.",
            },
        })
    }

    #[cfg(not(feature = "vision"))]
    pub fn load(_store: &ModelStore, _threads: usize) -> ProviderResult<Self> {
        Err(ProviderError::Unsupported(
            "this build was compiled without the `vision` feature; rebuild with `--features full`"
                .into(),
        ))
    }

    pub fn info(&self) -> ProviderInfo {
        self.info.clone()
    }

    #[cfg(feature = "vision")]
    pub fn detect_jpeg(&self, bytes: &[u8]) -> ProviderResult<Vec<ObjectDetection>> {
        let decoded = image::load_from_memory_with_format(bytes, ImageFormat::Jpeg)
            .map_err(|e| ProviderError::Unsupported(format!("cannot decode JPEG: {e}")))?;
        let rgb = decoded.into_rgb8();
        let (width, height) = rgb.dimensions();
        self.detect_rgb(width as usize, height as usize, rgb.as_raw())
    }

    #[cfg(not(feature = "vision"))]
    pub fn detect_jpeg(&self, _bytes: &[u8]) -> ProviderResult<Vec<ObjectDetection>> {
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
    ) -> ProviderResult<Vec<ObjectDetection>> {
        if width == 0 || height == 0 || rgb.len() != width * height * 3 {
            return Err(ProviderError::Unsupported(
                "expected a non-empty RGB image".into(),
            ));
        }
        let (target_w, target_h) = resize_to(width, height);
        let source = image::RgbImage::from_raw(width as u32, height as u32, rgb.to_vec())
            .ok_or_else(|| ProviderError::Unsupported("bad RGB buffer".into()))?;
        let resized = image::imageops::resize(
            &source,
            target_w as u32,
            target_h as u32,
            FilterType::Triangle,
        );
        // ImageNet mean/std, RGB, NCHW.
        let mut input = vec![0.0_f32; 3 * target_w * target_h];
        for (index, pixel) in resized.pixels().enumerate() {
            let [r, g, b] = pixel.0;
            input[index] = (f32::from(r) / 255.0 - 0.485) / 0.229;
            input[target_w * target_h + index] = (f32::from(g) / 255.0 - 0.456) / 0.224;
            input[2 * target_w * target_h + index] = (f32::from(b) / 255.0 - 0.406) / 0.225;
        }
        let tensor = Tensor::from_array((vec![1_i64, 3, target_h as i64, target_w as i64], input))
            .map_err(|e| ProviderError::Inference(e.to_string()))?;
        let mut session = self
            .session
            .lock()
            .map_err(|_| ProviderError::Inference("object session poisoned".into()))?;
        let outputs = session
            .run(ort::inputs!["pixel_values" => tensor])
            .map_err(|e| ProviderError::Inference(format!("object detection run failed: {e}")))?;
        let logits = outputs
            .get("logits")
            .ok_or_else(|| ProviderError::Inference("object logits are missing".into()))?
            .try_extract_tensor::<f32>()
            .map_err(|e| ProviderError::Inference(e.to_string()))?
            .1
            .to_vec();
        let boxes = outputs
            .get("pred_boxes")
            .ok_or_else(|| ProviderError::Inference("object boxes are missing".into()))?
            .try_extract_tensor::<f32>()
            .map_err(|e| ProviderError::Inference(e.to_string()))?
            .1
            .to_vec();
        Ok(decode(
            &logits,
            &boxes,
            QUERIES,
            CLASSES,
            width as f32,
            height as f32,
            SCORE_THRESHOLD,
        ))
    }

    #[cfg(not(feature = "vision"))]
    pub fn detect_rgb(
        &self,
        _width: usize,
        _height: usize,
        _rgb: &[u8],
    ) -> ProviderResult<Vec<ObjectDetection>> {
        Err(ProviderError::Unsupported(
            "this build was compiled without the `vision` feature".into(),
        ))
    }
}

/// Shortest edge 512, long edge capped at 1333, aspect ratio kept.
pub fn resize_to(width: usize, height: usize) -> (usize, usize) {
    let (w, h) = (width as f32, height as f32);
    let (mut target_w, mut target_h) = if w <= h {
        (SHORTEST_EDGE, (h / w * SHORTEST_EDGE).round())
    } else {
        ((w / h * SHORTEST_EDGE).round(), SHORTEST_EDGE)
    };
    let longest = target_w.max(target_h);
    if longest > LONGEST_EDGE {
        let scale = LONGEST_EDGE / longest;
        target_w = (target_w * scale).round();
        target_h = (target_h * scale).round();
    }
    (target_w.max(1.0) as usize, target_h.max(1.0) as usize)
}

/// YOLOS decode: softmax over the 92 ids (0 is N/A, 91 is background), one best
/// class per query, boxes normalised `(cx, cy, w, h)`.
pub fn decode(
    logits: &[f32],
    boxes: &[f32],
    queries: usize,
    classes: usize,
    width: f32,
    height: f32,
    threshold: f32,
) -> Vec<ObjectDetection> {
    if logits.len() < queries * classes || boxes.len() < queries * 4 {
        return Vec::new();
    }
    let mut found = Vec::new();
    for query in 0..queries {
        let row = &logits[query * classes..(query + 1) * classes];
        let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let sum: f32 = row.iter().map(|value| (value - max).exp()).sum();
        let mut best = (0_usize, f32::NEG_INFINITY);
        // Class 0 is N/A; the last id is background.
        for (index, value) in row.iter().enumerate().take(classes - 1).skip(1) {
            let score = (value - max).exp() / sum;
            if score > best.1 {
                best = (index, score);
            }
        }
        let (class_index, score) = best;
        if score < threshold || COCO.get(class_index).is_none_or(|name| *name == "N/A") {
            continue;
        }
        let cx = boxes[query * 4];
        let cy = boxes[query * 4 + 1];
        let w = boxes[query * 4 + 2];
        let h = boxes[query * 4 + 3];
        if !cx.is_finite() || !cy.is_finite() || !w.is_finite() || !h.is_finite() {
            continue;
        }
        let x1 = ((cx - w / 2.0) * width).clamp(0.0, width);
        let y1 = ((cy - h / 2.0) * height).clamp(0.0, height);
        let x2 = ((cx + w / 2.0) * width).clamp(0.0, width);
        let y2 = ((cy + h / 2.0) * height).clamp(0.0, height);
        found.push(ObjectDetection {
            class: COCO[class_index].to_string(),
            bbox: BBox {
                x: x1,
                y: y1,
                width: x2 - x1,
                height: y2 - y1,
            },
            score,
        });
    }
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    found.truncate(MAX_DETECTIONS);
    found
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

    fn grid(queries: usize, classes: usize) -> Vec<f32> {
        vec![-10.0; queries * classes]
    }

    #[test]
    fn a_confident_query_becomes_a_person() {
        let mut logits = grid(QUERIES, CLASSES);
        logits[CLASSES + 1] = 3.0; // query 1, class 1 = person
        let mut boxes = vec![0.0_f32; QUERIES * 4];
        boxes[4] = 0.5;
        boxes[5] = 0.5;
        boxes[6] = 0.2;
        boxes[7] = 0.2;
        let found = decode(&logits, &boxes, QUERIES, CLASSES, 640.0, 480.0, 0.5);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].class, "person");
        assert!((found[0].bbox.width - 128.0).abs() < 0.01);
        assert!((found[0].bbox.height - 96.0).abs() < 0.01);
        assert!((found[0].bbox.x - (320.0 - 64.0)).abs() < 0.01);
    }

    #[test]
    fn low_scores_and_na_classes_are_skipped() {
        let mut logits = grid(QUERIES, CLASSES);
        for value in logits.iter_mut().take(CLASSES) {
            *value = 0.0; // uniform: 1/91 per class, below 0.5
        }
        logits[CLASSES] = 10.0; // class 0 is N/A and must be skipped
        let boxes = vec![0.5_f32; QUERIES * 4];
        let found = decode(&logits, &boxes, QUERIES, CLASSES, 100.0, 100.0, 0.5);
        assert!(found.is_empty());
    }

    #[test]
    fn resize_keeps_aspect_and_caps_the_long_edge() {
        assert_eq!(resize_to(640, 480), (683, 512));
        assert_eq!(resize_to(4000, 500), (1333, 167));
    }
}
