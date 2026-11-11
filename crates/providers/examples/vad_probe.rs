//! Development probe: print VAD regions for an audio file.
//!
//! cargo run -p sonolytica-providers --features silero --example vad_probe -- audio.wav

use sonolytica_core::VadConfig;
use sonolytica_media::{DecodeOptions, MediaTools, decode_mono};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("audio path")?;
    let store = sonolytica_providers::ModelStore::default();
    let spec = sonolytica_providers::find_model("silero-vad-v5").ok_or("spec")?;
    store.require(spec)?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let audio = runtime.block_on(decode_mono(
        &MediaTools::detect(),
        std::path::Path::new(&path),
        &DecodeOptions::default(),
    ))?;

    let registry = sonolytica_providers::Registry::discover(store, 4);
    let vad = registry
        .vad()
        .ok_or_else(|| format!("{:?}", registry.unavailable))?;
    for region in vad.detect(&audio, &VadConfig::default())? {
        println!(
            "{:.3} -> {:.3}  p={:?}",
            region.range.start.as_secs_f64(),
            region.range.end.as_secs_f64(),
            region.probability
        );
    }
    Ok(())
}
