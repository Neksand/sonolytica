//! Speech must not be reported as music.
//!
//! `jfk.wav` is the 11-second public-domain John F. Kennedy excerpt shipped in
//! the whisper.cpp samples. It is clean speech with no music; the tempo/key/chord
//! heuristics used to report a confident-looking BPM and key for it.

use sonolytica_dsp::{FrameConfig, analyze_music};

fn jfk() -> (Vec<f32>, u32) {
    let mut reader = hound::WavReader::open("tests/fixtures/jfk.wav").expect("jfk fixture");
    let spec = reader.spec();
    assert_eq!(spec.channels, 1, "fixture must be mono");
    let samples = reader
        .samples::<i16>()
        .map(|sample| f32::from(sample.expect("PCM sample")) / 32_768.0)
        .collect();
    (samples, spec.sample_rate)
}

#[test]
fn clean_speech_is_not_music() {
    let (samples, rate) = jfk();
    let features = analyze_music(&samples, rate, FrameConfig::default()).expect("music analysis");
    assert!(
        !features.present(),
        "clean speech must not produce music evidence"
    );
}
