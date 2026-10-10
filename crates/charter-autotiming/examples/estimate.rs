//! Estimates a song's tempo map with Beat This! and scores it against a human one, by hand:
//!
//!     cargo run -p charter-autotiming --release --example estimate -- final0.ckpt song.mp3 [map.osu]
//!
//! With an osu! beatmap it lists the map's red lines and, for every beat of the map, how far the
//! estimate's nearest grid line is: the median and 95th percentile, and the share within 5, 10
//! and 20 ms, the benchmark redline reports, as estimated and with the median offset removed. It
//! also says how the tracker's beats and the map's lines sit against charter's onsets, which is
//! where an offset between the estimate and the map comes from.

use std::path::Path;
use std::time::Instant;

use charter_autotiming::estimate;
use charter_beatthis_burn::Tracker;
use charter_core::{BeatLength, ResolvedTime, Section, TicksPerBeat};

#[expect(
    clippy::print_stdout,
    clippy::use_debug,
    reason = "this interactive benchmark reports timings and estimates to stdout"
)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (Some(weights), Some(audio)) = (args.get(1), args.get(2)) else {
        return Err("usage: estimate <final0.ckpt> <audio> [map.osu]".into());
    };
    let extension = Path::new(audio).extension().and_then(|ext| ext.to_str());
    let track = charter_audio::Track::decode(std::fs::read(audio)?, extension)?;
    let mono: Vec<f32> = track.frames().iter().map(|[l, r]| (l + r) / 2.0).collect();
    let started = Instant::now();
    let onsets = charter_audio::onsets(&track);
    println!("{} onsets", onsets.len());
    let beats = Tracker::from_checkpoint(Path::new(weights))?.track(&mono, track.rate().get())?;
    let mut shifts: Vec<f64> = beats
        .beats
        .iter()
        .filter_map(|beat| {
            onsets
                .iter()
                .map(|onset| onset.seconds - beat)
                .filter(|shift| shift.abs() <= 0.080)
                .min_by(|a, b| a.abs().total_cmp(&b.abs()))
        })
        .map(|shift| shift * 1000.0)
        .collect();
    shifts.sort_by(f64::total_cmp);
    if !shifts.is_empty() {
        println!(
            "{} of {} beats have an onset within 80 ms; onset minus beat: p10 {:.0}, median {:.0}, p90 {:.0} ms",
            shifts.len(),
            beats.beats.len(),
            percentile(&shifts, 0.1),
            percentile(&shifts, 0.5),
            percentile(&shifts, 0.9)
        );
    }
    let found = estimate(
        &beats.beats,
        &beats.downbeats,
        &onsets,
        TicksPerBeat::default(),
    )?
    .ok_or("too few beats")?;
    println!("estimated in {:.2?}", started.elapsed());
    for fitted in &found.fitted {
        println!(
            "{:8.3} s  {:8.3} BPM  {}/4  {:4} beats  {}  rms {:.1} ms",
            fitted.section.start.seconds(),
            fitted.section.beat.bpm(),
            fitted.section.meter,
            fitted.beats,
            if fitted.snapped { "snapped" } else { "free   " },
            fitted.error * 1000.0
        );
    }
    let Some(map) = args.get(3) else {
        return Ok(());
    };
    let reds = red_lines(&std::fs::read_to_string(map)?);
    println!("the map's {} red lines", reds.len());
    for red in reds.iter().take(12) {
        println!("{:8.3} s  {:8.3} BPM", red.start.seconds(), red.beat.bpm());
    }
    let mut against: Vec<f64> = beats_of(&reds, track.seconds())
        .into_iter()
        .filter_map(|beat| {
            onsets
                .iter()
                .map(|onset| onset.seconds - beat)
                .filter(|shift| shift.abs() <= 0.040)
                .min_by(|a, b| a.abs().total_cmp(&b.abs()))
        })
        .map(|shift| shift * 1000.0)
        .collect();
    against.sort_by(f64::total_cmp);
    if !against.is_empty() {
        println!(
            "onset minus the map's beat: p10 {:.1}, median {:.1}, p90 {:.1} ms over {} beats",
            percentile(&against, 0.1),
            percentile(&against, 0.5),
            percentile(&against, 0.9),
            against.len()
        );
    }
    let ours: Vec<Section> = found.fitted.iter().map(|fitted| fitted.section).collect();
    let signed: Vec<f64> = beats_of(&reds, track.seconds())
        .into_iter()
        .map(|beat| distance(&ours, beat) * 1000.0)
        .collect();
    let mut sorted = signed.clone();
    sorted.sort_by(f64::total_cmp);
    let bias = percentile(&sorted, 0.5);
    println!("median signed error {bias:+.1} ms (map minus estimate)");
    report(
        "as estimated",
        signed.iter().map(|error| error.abs()).collect(),
    );
    report(
        "bias removed",
        signed.iter().map(|error| (error - bias).abs()).collect(),
    );
    Ok(())
}

#[expect(
    clippy::print_stdout,
    reason = "this benchmark prints its error summary"
)]
/// Prints the distribution of `errors` in milliseconds for `what`.
fn report(what: &str, mut errors: Vec<f64>) {
    errors.sort_by(f64::total_cmp);
    let share = |ms: f64| {
        let within = errors.iter().filter(|error| **error <= ms).count();

        count_float(within) / count_float(errors.len().max(1))
    };
    println!(
        "{what}: {} beats: median {:.1} ms, p95 {:.1} ms, within 5/10/20 ms {:.0}% / {:.0}% / {:.0}%",
        errors.len(),
        percentile(&errors, 0.5),
        percentile(&errors, 0.95),
        share(5.0) * 100.0,
        share(10.0) * 100.0,
        share(20.0) * 100.0
    );
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "song-length counts are converted for benchmark percentages"
)]
/// A beat count as a floating-point value for benchmark percentages.
const fn count_float(count: usize) -> f64 {
    count as f64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the percentile index is bounded by the slice before use"
)]
/// A bounded percentile position as an integer slice index.
const fn percentile_index(index: f64) -> usize {
    index as usize
}

/// The uninherited timing points of an .osu file as sections.
fn red_lines(text: &str) -> Vec<Section> {
    text.lines()
        .skip_while(|line| line.trim() != "[TimingPoints]")
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(',').collect();
            let uninherited = fields.get(6).is_none_or(|flag| flag.trim() == "1");
            let (time, length) = (
                fields.first()?.trim().parse::<f64>().ok()?,
                fields.get(1)?.trim().parse::<f64>().ok()?,
            );
            if !uninherited {
                return None;
            }
            Some(Section {
                start: ResolvedTime::from_seconds(time / 1000.0)?,
                beat: BeatLength::from_millis(length)?,
                meter: fields
                    .get(2)
                    .and_then(|m| m.trim().parse().ok())
                    .unwrap_or(std::num::NonZeroU16::MIN),
            })
        })
        .collect()
}

/// Every beat `sections` put in the audio, up to `end` seconds.
fn beats_of(sections: &[Section], end: f64) -> Vec<f64> {
    let mut beats = Vec::new();
    for (at, section) in sections.iter().enumerate() {
        let until = sections
            .get(at.saturating_add(1))
            .map_or(end, |next| next.start.seconds());
        let mut beat = section.start.seconds();
        while beat < until {
            beats.push(beat);
            beat += section.beat.seconds();
        }
    }
    beats
}

/// How far `seconds` is past the nearest grid line of `sections`, negative before it, allowing
/// half-beat and double-beat grids, since a tempo's octave is a charting convention.
fn distance(sections: &[Section], seconds: f64) -> f64 {
    let at = sections
        .partition_point(|section| section.start.seconds() <= seconds)
        .max(1)
        .saturating_sub(1);
    let Some(section) = sections.get(at) else {
        return f64::INFINITY;
    };
    [0.5, 1.0, 2.0]
        .into_iter()
        .map(|scale| {
            let period = section.beat.seconds() * scale;
            let phase = (seconds - section.start.seconds()) / period;
            (phase - phase.round()) * period
        })
        .min_by(|a, b| a.abs().total_cmp(&b.abs()))
        .unwrap_or(f64::INFINITY)
}

/// The value a `fraction` of the way through `sorted`, or NaN for none.
fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    let at = percentile_index(count_float(sorted.len()) * fraction);
    sorted
        .get(at.min(sorted.len().saturating_sub(1)))
        .copied()
        .unwrap_or(f64::NAN)
}
