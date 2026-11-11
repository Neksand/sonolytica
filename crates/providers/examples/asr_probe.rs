//! Development probe: run Whisper with selected parameters and print segments.
//!
//! cargo run -p sonolytica-providers --features whisper --example asr_probe -- model.bin audio.wav [flags]

use std::path::PathBuf;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let model = PathBuf::from(args.next().ok_or("model path")?);
    let audio_path = PathBuf::from(args.next().ok_or("audio path")?);
    let flags: Vec<String> = args.collect();

    let tool = sonolytica_media::MediaTools::detect();
    let audio = futures_lite_block(sonolytica_media::decode_mono(
        &tool,
        &audio_path,
        &sonolytica_media::DecodeOptions::default(),
    ))?;
    println!("samples {} rate {}", audio.len(), audio.sample_rate);

    let ctx = WhisperContext::new_with_params(
        model.to_str().ok_or("utf8")?,
        WhisperContextParameters::default(),
    )?;
    let mut state = ctx.create_state()?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("en"));
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_token_timestamps(flags.iter().any(|f| f == "--token-ts"));
    params.set_no_context(flags.iter().any(|f| f == "--no-context"));
    params.set_suppress_blank(flags.iter().any(|f| f == "--suppress-blank"));
    params.set_no_speech_thold(0.6);
    state.full(params, &audio.samples)?;
    for i in 0..state.full_n_segments() {
        let segment = state.get_segment(i).ok_or("segment")?;
        println!(
            "[{:>6.2} -> {:>6.2}] {:?}",
            segment.start_timestamp() as f32 / 100.0,
            segment.end_timestamp() as f32 / 100.0,
            segment.to_str_lossy()?
        );
    }
    Ok(())
}

fn futures_lite_block<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}
