//! Timing from sections: stretches of constant tempo each starting at a moment of the audio, as
//! osu!'s red lines are written and as a tempo map estimated from a song's beats comes out. The
//! first section's start is tick 0, and each later one sits where the section before puts its
//! start, a beat every `beat_seconds`, rounded to the nearest tick, so every section starts at
//! its own moment and keeps its own tempo.

use std::num::NonZeroU16;

use crate::document::{Anchor, Extensions, Timing, TimingEvent, TimingKind};
use crate::unit::{Bpm, Origin, Tick, TicksPerBeat, TimeSignature};

/// A stretch of constant tempo from `seconds` into the audio, a beat every `beat_seconds`,
/// `meter` beats to a bar starting on its first beat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    /// Starting moment in the audio.
    pub seconds: f64,
    /// Duration of one beat in seconds.
    pub beat_seconds: f64,
    /// Beats per bar from this section's start.
    pub meter: NonZeroU16,
}

/// Sections on charter's grid: the timing they make, the sections kept with the tick each
/// starts on, and how many fell on the tick of a later one and gave way to it.
#[derive(Clone, Debug, PartialEq)]
pub struct Sectioned {
    /// Pinned beats and meter changes produced from the sections.
    pub timing: Timing,
    /// Sections retained, paired with their computed starting ticks.
    pub kept: Vec<(i64, Section)>,
    /// Number of sections displaced by another on the same tick.
    pub merged: usize,
}

/// A section could not be converted to chart timing.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SectionError {
    /// No sections were supplied.
    #[error("there is no section to start the tempo")]
    Empty,
    /// A time, tick or tempo falls outside the representable range.
    #[error("{0}")]
    OutOfRange(String),
}

/// The timing `sections` make at `ticks_per_beat`.
///
/// That is an anchor at each section's start and the last one's tempo after it, with a time
/// signature wherever a section changes the meter or starts partway through a bar, since each
/// section starts a bar of its own.
///
/// # Errors
///
/// Returns [`SectionError::Empty`] without sections, and [`SectionError::OutOfRange`] for a
/// start, tick or tempo the format cannot hold.
pub fn sectioned(
    sections: &[Section],
    ticks_per_beat: TicksPerBeat,
) -> Result<Sectioned, SectionError> {
    let per_beat = f64::from(u16::from(ticks_per_beat));
    let mut sorted = sections.to_vec();
    sorted.sort_by(|a, b| a.seconds.total_cmp(&b.seconds));
    let mut kept: Vec<(i64, Section)> = Vec::new();
    let mut merged = 0_usize;
    for section in sorted {
        let Some(&(tick, last)) = kept.last() else {
            kept.push((0, section));
            continue;
        };
        let distance = ((section.seconds - last.seconds) / last.beat_seconds * per_beat).round();
        if !distance.is_finite()
            || distance < i64_bound_to_f64(i64::MIN)
            || distance >= i64_bound_to_f64(i64::MAX)
        {
            return Err(out_of_range("a section's tick"));
        }
        let apart = rounded_tick(distance);
        let next_tick = tick
            .checked_add(apart)
            .ok_or_else(|| out_of_range("a section's tick"))?;
        // Two sections on one tick: the later one is the one that plays.
        if next_tick
            == kept
                .last()
                .map_or(i64::MIN, |(previous_tick, _)| *previous_tick)
        {
            merged = merged.saturating_add(1);
            kept.pop();
        }
        kept.push((next_tick, section));
    }
    let first = kept.first().ok_or(SectionError::Empty)?.1;
    let mut tempo = bpm(first.beat_seconds)?;
    let mut anchors = Vec::with_capacity(kept.len());
    let mut events = Vec::new();
    let four = NonZeroU16::new(4).unwrap_or(NonZeroU16::MIN);
    let (mut measure_from, mut meter) = (0, four);
    let resolution_ticks = i64::from(ticks_per_beat.get());
    for &(tick, section) in &kept {
        tempo = bpm(section.beat_seconds)?;
        anchors.push(Anchor {
            tick: tick_of(tick)?,
            seconds: micro(section.seconds)
                .and_then(|seconds| Origin::try_from(seconds).ok())
                .ok_or_else(|| out_of_range(format!("{} s", section.seconds)))?,
        });
        let measure = std::num::NonZeroI64::from(meter)
            .get()
            .saturating_mul(resolution_ticks);
        let mid_measure = tick.saturating_sub(measure_from).rem_euclid(measure.max(1)) != 0;
        if section.meter != meter || mid_measure {
            let signature = TimeSignature {
                numerator: section.meter,
                denominator: four,
            };
            events.push(TimingEvent {
                tick: tick_of(tick)?,
                kind: TimingKind::TimeSignature(signature),
                ext: Extensions::new(),
            });
            (measure_from, meter) = (tick, section.meter);
        }
    }
    Ok(Sectioned {
        timing: Timing {
            anchors,
            tempo,
            events,
        },
        kept,
        merged,
    })
}

/// The tempo of a beat `beat_seconds` long, to six places.
fn bpm(beat_seconds: f64) -> Result<Bpm, SectionError> {
    micro(60.0 / beat_seconds)
        .and_then(|bpm| Bpm::try_from(bpm).ok())
        .ok_or_else(|| out_of_range(format!("a beat of {beat_seconds} s is no tempo")))
}

#[expect(
    clippy::map_err_ignore,
    reason = "the range error already names the offending tick"
)]
/// A grid tick in the chart's `i32` range, or a range error.
fn tick_of(tick: i64) -> Result<Tick, SectionError> {
    i32::try_from(tick)
        .map(Tick::new)
        .map_err(|_| out_of_range(format!("tick {tick} is past the song's end")))
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "caller checked finite i64 bounds"
)]
/// Nearest whole tick after the caller checks the `i64` bounds.
fn rounded_tick(value: f64) -> i64 {
    value as i64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "i64 bound comparison is conservative at the upper edge"
)]
/// Float bound for comparing a calculated tick before integer conversion.
fn i64_bound_to_f64(value: i64) -> f64 {
    value as f64
}

/// `value` to six places, the finest the format holds.
fn micro(value: f64) -> Option<f64> {
    let rounded = (value * 1e6).round() / 1e6;
    rounded.is_finite().then_some(rounded)
}

/// Section error naming the value that cannot fit the chart format.
fn out_of_range(what: impl std::fmt::Display) -> SectionError {
    SectionError::OutOfRange(format!("{what} is out of range"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unit::TicksPerBeat;

    fn section(seconds: f64, bpm: f64, meter: u16) -> Section {
        Section {
            seconds,
            beat_seconds: 60.0 / bpm,
            meter: NonZeroU16::new(meter).unwrap(),
        }
    }

    fn anchors(sectioned: &Sectioned) -> Vec<(i32, f64)> {
        sectioned
            .timing
            .anchors
            .iter()
            .map(|anchor| {
                let tick = serde_json::to_value(anchor.tick).unwrap().as_i64().unwrap();
                (
                    i32::try_from(tick).unwrap(),
                    crate::ResolvedTime::from(anchor.seconds).seconds(),
                )
            })
            .collect()
    }

    #[test]
    fn each_section_starts_at_its_moment_where_the_one_before_puts_it() {
        // A bar at 120 BPM, then 150 from two seconds in, given out of order.
        let made = sectioned(
            &[section(2.0, 150.0, 4), section(0.0, 120.0, 4)],
            TicksPerBeat::default(),
        )
        .unwrap();
        assert_eq!(anchors(&made), [(0, 0.0), (3840, 2.0)]);
        assert_eq!(made.timing.tempo, Bpm::try_from(150.0).unwrap());
        assert!(made.timing.events.is_empty());
        assert_eq!(made.merged, 0);
    }

    #[test]
    fn a_section_starting_mid_bar_or_in_another_meter_restarts_the_bars() {
        // Three beats in, so partway through a bar of 4/4, then a bar of 3/4.
        let made = sectioned(
            &[
                section(0.0, 60.0, 4),
                section(3.0, 60.0, 4),
                section(7.0, 60.0, 3),
            ],
            TicksPerBeat::default(),
        )
        .unwrap();
        let ticks: Vec<_> = made.timing.events.iter().map(|event| event.tick).collect();
        assert_eq!(ticks, [Tick::new(2880), Tick::new(6720)]);
    }

    #[test]
    fn a_section_on_the_tick_of_the_one_before_takes_its_place() {
        let made = sectioned(
            &[section(0.0, 120.0, 4), section(0.0001, 180.0, 4)],
            TicksPerBeat::default(),
        )
        .unwrap();
        assert_eq!(made.merged, 1);
        assert_eq!(made.kept.len(), 1);
        assert_eq!(made.timing.tempo, Bpm::try_from(180.0).unwrap());
    }

    #[test]
    fn no_sections_or_a_tempo_past_the_format_are_refused() {
        assert_eq!(
            sectioned(&[], TicksPerBeat::default()),
            Err(SectionError::Empty)
        );
        sectioned(&[section(0.0, 0.0, 4)], TicksPerBeat::default()).unwrap_err();
    }

    #[test]
    fn sections_refuse_ticks_that_overflow_while_placing_later_sections() {
        let sections = [
            section(0.0, 60.0, 4),
            section(9_000_000_000_000_000.0, 60.0, 4),
            section(18_000_000_000_000_000.0, 60.0, 4),
        ];
        assert_eq!(
            sectioned(&sections, TicksPerBeat::default()),
            Err(SectionError::OutOfRange(
                "a section's tick is out of range".into()
            ))
        );
    }

    #[test]
    fn sections_refuse_audio_times_and_ticks_outside_the_format() {
        let far_tick = sectioned(
            &[section(0.0, 60.0, 4), section(3_000_000.0, 60.0, 4)],
            TicksPerBeat::default(),
        )
        .unwrap_err();
        assert!(matches!(far_tick, SectionError::OutOfRange(message) if message.contains("tick")));

        let far_time = sectioned(&[section(1e30, 120.0, 4)], TicksPerBeat::default()).unwrap_err();
        assert!(
            matches!(far_time, SectionError::OutOfRange(message) if message.contains("s is out of range"))
        );
    }
}
