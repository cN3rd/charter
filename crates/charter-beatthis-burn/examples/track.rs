//! Tracks the beats of one audio file with the released weights, for checking the network
//! against the reference by hand:
//!
//!     cargo run -p charter-beatthis-burn --release --example track -- final0.ckpt song.ogg [logits.f32]
//!
//! With a third path it writes each frame's beat logit there, little-endian, then the downbeat
//! logits.

use std::path::Path;
use std::time::Instant;

use charter_beatthis_burn::Tracker;

#[expect(
    clippy::print_stdout,
    clippy::use_debug,
    reason = "this interactive tracker example prints timing and beat results"
)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (Some(weights), Some(audio)) = (args.get(1), args.get(2)) else {
        return Err("usage: track <final0.ckpt> <audio> [logits.f32]".into());
    };
    let extension = Path::new(audio).extension().and_then(|ext| ext.to_str());
    let track = charter_audio::Track::decode(std::fs::read(audio)?, extension)?;
    let mono = track.to_mono();
    let loaded_at = Instant::now();
    let tracker = Tracker::from_checkpoint(Path::new(weights))?;
    println!("loaded in {:.2?}", loaded_at.elapsed());
    let tracked_at = Instant::now();
    let found = tracker.track(&mono)?;
    println!(
        "tracked {:.1} s of audio in {:.2?}",
        track.seconds(),
        tracked_at.elapsed()
    );
    let first = |times: &[f64], count| times.iter().take(count).copied().collect::<Vec<_>>();
    println!(
        "{} beats, first {:?}",
        found.beats.len(),
        first(&found.beats, 8)
    );
    println!(
        "{} downbeats, first {:?}",
        found.downbeats.len(),
        first(&found.downbeats, 4)
    );
    if let Some(out) = args.get(3) {
        #[expect(
            clippy::little_endian_bytes,
            reason = "the parity fixture reads the logits back as little-endian float32"
        )]
        let bytes: Vec<u8> = found
            .beat_logits
            .iter()
            .chain(&found.downbeat_logits)
            .flat_map(|logit| logit.to_le_bytes())
            .collect();
        std::fs::write(out, bytes)?;
    }
    Ok(())
}
