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
    pub seconds: f64,
    pub beat_seconds: f64,
    pub meter: NonZeroU16,
}

/// Sections on charter's grid: the timing they make, the sections kept with the tick each
/// starts on, and how many fell on the tick of a later one and gave way to it.
#[derive(Clone, Debug, PartialEq)]
pub struct Sectioned {
    pub timing: Timing,
    pub kept: Vec<(i64, Section)>,
    pub merged: usize,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SectionError {
    #[error("there is no section to start the tempo")]
    Empty,
    #[error("{0}")]
    OutOfRange(String),
}

/// The timing `sections` make at `ticks_per_beat`: an anchor at each section's start and the
/// last one's tempo after it, with a time signature wherever a section changes the meter or
/// starts partway through a bar, since each section starts a bar of its own.
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
    let mut merged = 0;
    for section in sorted {
        let Some(&(tick, last)) = kept.last() else {
            kept.push((0, section));
            continue;
        };
        #[allow(clippy::cast_possible_truncation)] // a song's worth of ticks
        let apart =
            ((section.seconds - last.seconds) / last.beat_seconds * per_beat).round() as i64;
        let tick = tick
            .checked_add(apart)
            .ok_or_else(|| out_of_range("a section's tick"))?;
        // Two sections on one tick: the later one is the one that plays.
        if tick == kept.last().map_or(i64::MIN, |(tick, _)| *tick) {
            merged += 1;
            kept.pop();
        }
        kept.push((tick, section));
    }
    let first = kept.first().ok_or(SectionError::Empty)?.1;
    let mut tempo = bpm(first.beat_seconds)?;
    let mut anchors = Vec::with_capacity(kept.len());
    let mut events = Vec::new();
    let four = NonZeroU16::new(4).unwrap_or(NonZeroU16::MIN);
    let (mut measure_from, mut meter) = (0, four);
    #[allow(clippy::cast_possible_truncation)] // a resolution, whole and small
    let ticks_per_beat = per_beat as i64;
    for &(tick, section) in &kept {
        tempo = bpm(section.beat_seconds)?;
        anchors.push(Anchor {
            tick: tick_of(tick)?,
            seconds: micro(section.seconds)
                .and_then(|seconds| Origin::try_from(seconds).ok())
                .ok_or_else(|| out_of_range(format!("{} s", section.seconds)))?,
        });
        let measure = i64::from(meter.get()) * ticks_per_beat;
        let mid_measure = (tick - measure_from).rem_euclid(measure.max(1)) != 0;
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

fn tick_of(tick: i64) -> Result<Tick, SectionError> {
    i32::try_from(tick)
        .map(Tick::new)
        .map_err(|_| out_of_range(format!("tick {tick} is past the song's end")))
}

/// `value` to six places, the finest the format holds.
fn micro(value: f64) -> Option<f64> {
    let rounded = (value * 1e6).round() / 1e6;
    rounded.is_finite().then_some(rounded)
}

fn out_of_range(what: impl std::fmt::Display) -> SectionError {
    SectionError::OutOfRange(format!("{what} is out of range"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::DEFAULT_TICKS_PER_BEAT;

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
            DEFAULT_TICKS_PER_BEAT,
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
            DEFAULT_TICKS_PER_BEAT,
        )
        .unwrap();
        let ticks: Vec<_> = made.timing.events.iter().map(|event| event.tick).collect();
        assert_eq!(ticks, [Tick::new(2880), Tick::new(6720)]);
    }

    #[test]
    fn a_section_on_the_tick_of_the_one_before_takes_its_place() {
        let made = sectioned(
            &[section(0.0, 120.0, 4), section(0.0001, 180.0, 4)],
            DEFAULT_TICKS_PER_BEAT,
        )
        .unwrap();
        assert_eq!(made.merged, 1);
        assert_eq!(made.kept.len(), 1);
        assert_eq!(made.timing.tempo, Bpm::try_from(180.0).unwrap());
    }

    #[test]
    fn no_sections_or_a_tempo_past_the_format_are_refused() {
        assert_eq!(
            sectioned(&[], DEFAULT_TICKS_PER_BEAT),
            Err(SectionError::Empty)
        );
        assert!(sectioned(&[section(0.0, 0.0, 4)], DEFAULT_TICKS_PER_BEAT).is_err());
    }
}
