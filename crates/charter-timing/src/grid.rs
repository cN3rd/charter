//! The timing window's grid: lines a fraction of a whole note apart, restarting on every
//! downbeat, so a measure of 7/8 at quarters ends on a short step rather than carrying the grid
//! into the next measure.

use std::num::NonZeroU16;

use charter_core::{Measure, Tick, Ticks, TicksPerBeat, Timing, Valid};

/// The resolutions offered, as the fraction of a whole note between lines: keys 1 to 0.
pub const RESOLUTIONS: [u16; 10] = [1, 2, 3, 4, 6, 8, 12, 16, 24, 32];

/// The resolution the window opens at: beats, since most songs are mapped on beats first.
pub const DEFAULT: u16 = 4;

/// A kind of line, drawn so that each is told apart by its shape as well as its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    Downbeat,
    Beat,
    /// A line between beats: dashed, or dotted for the triplet family.
    Sub {
        triplet: bool,
    },
}

/// A grid of lines `1/resolution` of a whole note apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grid {
    pub resolution: NonZeroU16,
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            resolution: NonZeroU16::new(DEFAULT).unwrap_or(NonZeroU16::MIN),
        }
    }
}

impl Grid {
    /// The ticks from one line to the next at `ticks_per_beat`, if that is whole.
    #[must_use]
    pub fn step(self, ticks_per_beat: TicksPerBeat) -> Option<Ticks> {
        ticks_per_beat.note_length(self.resolution)
    }

    /// Whether the lines between beats come in threes.
    #[must_use]
    pub const fn triplet(self) -> bool {
        self.resolution.get().is_multiple_of(3)
    }

    /// The lines of `measure`, its downbeat first, each with its kind. The last step may be
    /// short where the measure does not divide.
    #[must_use]
    pub fn lines(self, measure: &Measure, ticks_per_beat: TicksPerBeat) -> Vec<(Tick, Line)> {
        let Some(step) = self.step(ticks_per_beat) else {
            return vec![(measure.start, Line::Downbeat)];
        };
        let beat = i64::from(u16::from(ticks_per_beat));
        let (start, step) = (i64::from(measure.start), i64::from(u32::from(step)));
        let length = i64::from(u32::from(measure.length));
        (0..=length.div_euclid(step.max(1)))
            .map(|index| index.saturating_mul(step))
            .take_while(|offset| *offset < length)
            .filter_map(|offset| {
                let tick = Tick::new(i32::try_from(start.checked_add(offset)?).ok()?);
                let line = if offset == 0 {
                    Line::Downbeat
                } else if offset.rem_euclid(beat) == 0 {
                    Line::Beat
                } else {
                    Line::Sub {
                        triplet: self.triplet(),
                    }
                };
                Some((tick, line))
            })
            .collect()
    }

    /// The line of this grid nearest `tick`, in the measure holding it or on the next downbeat.
    #[must_use]
    pub fn nearest(self, timing: Valid<'_, Timing>, tick: Tick) -> Tick {
        self.around(timing, tick).map_or(tick, |(below, above)| {
            let (from, to) = (
                i64::from(tick).saturating_sub(i64::from(below)),
                i64::from(above).saturating_sub(i64::from(tick)),
            );
            if to < from { above } else { below }
        })
    }

    /// The line of this grid nearest in the audio to `seconds`, and the lines on either side of
    /// the moment, from which a step's length in time follows.
    #[must_use]
    pub fn nearest_in_time(self, timing: Valid<'_, Timing>, seconds: f64) -> Option<Near> {
        let tick = timing.latest_tick_by(seconds);
        let (below, above) = self.around(timing, tick)?;
        let (at_below, at_above) = (
            timing.time_at(below).seconds(),
            timing.time_at(above).seconds(),
        );
        let line = if (at_above - seconds).abs() < (seconds - at_below).abs() {
            above
        } else {
            below
        };
        Some(Near {
            line,
            seconds: timing.time_at(line).seconds(),
            step_seconds: at_above - at_below,
        })
    }

    /// The line `steps` lines after the one at or before `tick`, or before it for a negative
    /// count, across measures.
    #[must_use]
    pub fn stepped(self, timing: Valid<'_, Timing>, tick: Tick, steps: i32) -> Tick {
        let mut at = self.around(timing, tick).map_or(tick, |(below, _)| below);
        let forward = steps > 0;
        for _ in 0..steps.unsigned_abs() {
            let next = if forward {
                self.around(timing, at).map(|(_, above)| above)
            } else {
                let before =
                    Tick::new(i32::try_from(i64::from(at).saturating_sub(1)).unwrap_or(i32::MIN));
                self.around(timing, before).map(|(below, _)| below)
            };
            match next {
                Some(next) if next != at => at = next,
                _ => break,
            }
        }
        at
    }

    /// The line at or before `tick` and the one after it.
    fn around(self, timing: Valid<'_, Timing>, tick: Tick) -> Option<(Tick, Tick)> {
        let resolution = timing_resolution(timing);
        let measure = timing.measures_from(tick).next()?;
        let lines = self.lines(&measure, resolution);
        let end = measure.start.checked_add(measure.length)?;
        let below = lines
            .iter()
            .rev()
            .find(|(line, _)| *line <= tick)
            .map(|(line, _)| *line)?;
        let above = lines
            .iter()
            .find(|(line, _)| *line > tick)
            .map_or(end, |(line, _)| *line);
        Some((below, above))
    }
}

/// Where the nearest grid line to a moment is, and how long a step of the grid is there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Near {
    pub line: Tick,
    pub seconds: f64,
    pub step_seconds: f64,
}

/// The resolution `timing` counts ticks at.
const fn timing_resolution(timing: Valid<'_, Timing>) -> TicksPerBeat {
    timing.ticks_per_beat()
}

#[cfg(test)]
mod tests {
    use charter_core::ValidDocument;
    use proptest::prelude::*;

    use super::*;

    fn song(events: &serde_json::Value) -> ValidDocument {
        let value = serde_json::json!({
            "format_version": 0,
            "ruleset": { "id": "t", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "a.ogg" },
            "timing": { "offset_seconds": 0.0, "events": events },
            "charts": []
        });
        ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap()
    }

    fn grid(resolution: u16) -> Grid {
        Grid {
            resolution: NonZeroU16::new(resolution).unwrap(),
        }
    }

    fn tick(at: i32) -> Tick {
        Tick::new(at)
    }

    #[test]
    fn a_seven_eight_measure_at_quarters_ends_on_a_short_step_and_restarts() {
        let document = song(&serde_json::json!([
            { "tick": 0, "kind": { "bpm": 120.0 } },
            { "tick": 0, "kind": { "time_signature": [7, 8] } }
        ]));
        let timing = document.timing();
        let measure = timing.measures_from(tick(0)).next().unwrap();
        let lines = grid(4).lines(&measure, timing.ticks_per_beat());
        let ticks: Vec<Tick> = lines.iter().map(|(at, _)| *at).collect();
        assert_eq!(ticks, [tick(0), tick(960), tick(1920), tick(2880)]);
        assert_eq!(lines[0].1, Line::Downbeat);
        assert_eq!(lines[1].1, Line::Beat);
        assert_eq!(grid(4).nearest(timing, tick(3200)), tick(3360));
        assert_eq!(grid(4).stepped(timing, tick(2880), 1), tick(3360));
        assert_eq!(grid(4).stepped(timing, tick(3360), -1), tick(2880));
    }

    #[test]
    fn lines_between_beats_are_dashed_or_dotted_for_triplets() {
        let document = song(&serde_json::json!([{ "tick": 0, "kind": { "bpm": 120.0 } }]));
        let timing = document.timing();
        let measure = timing.measures_from(tick(0)).next().unwrap();
        let eighths = grid(8).lines(&measure, timing.ticks_per_beat());
        assert_eq!(eighths.len(), 8);
        assert_eq!(eighths[1].1, Line::Sub { triplet: false });
        let triplets = grid(12).lines(&measure, timing.ticks_per_beat());
        assert_eq!(triplets.len(), 12);
        assert_eq!(triplets[1].1, Line::Sub { triplet: true });
        assert_eq!(triplets[3].1, Line::Beat);
    }

    #[test]
    fn the_nearest_line_to_a_moment_comes_with_the_length_of_its_step() {
        let document = song(&serde_json::json!([{ "tick": 0, "kind": { "bpm": 120.0 } }]));
        let near = grid(4).nearest_in_time(document.timing(), 0.74).unwrap();
        assert_eq!(near.line, tick(960));
        assert!((near.seconds - 0.5).abs() < 1e-9);
        assert!((near.step_seconds - 0.5).abs() < 1e-9);
        let later = grid(4).nearest_in_time(document.timing(), 0.76).unwrap();
        assert_eq!(later.line, tick(1920));
    }

    #[test]
    fn a_one_beat_measure_has_only_its_downbeat_at_beats() {
        let document = song(&serde_json::json!([
            { "tick": 0, "kind": { "bpm": 120.0 } },
            { "tick": 0, "kind": { "time_signature": [1, 4] } }
        ]));
        let timing = document.timing();
        let measure = timing.measures_from(tick(0)).next().unwrap();
        assert_eq!(
            grid(4).lines(&measure, timing.ticks_per_beat()),
            vec![(tick(0), Line::Downbeat)]
        );
        assert_eq!(grid(4).nearest(timing, tick(960)), tick(960));
        assert_eq!(grid(4).stepped(timing, tick(0), -1), tick(-960));
    }

    #[test]
    fn a_resolution_without_whole_tick_steps_still_shows_the_downbeat() {
        let document = song(&serde_json::json!([{ "tick": 0, "kind": { "bpm": 120.0 } }]));
        let timing = document.timing();
        let measure = timing.measures_from(tick(0)).next().unwrap();
        assert_eq!(
            grid(7).lines(&measure, timing.ticks_per_beat()),
            vec![(tick(0), Line::Downbeat)]
        );
    }

    proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn grid_lines_are_sorted_inside_their_measure_even_before_tick_zero(
            numerator in 1u16..=15,
            resolution in prop::sample::select(RESOLUTIONS.to_vec()),
            bar in -5i32..=5,
        ) {
            let document = song(&serde_json::json!([
                { "tick": 0, "kind": { "bpm": 120.0 } },
                { "tick": 0, "kind": { "time_signature": [numerator, 8] } }
            ]));
            let timing = document.timing();
            let at = tick(bar.saturating_mul(i32::from(numerator)).saturating_mul(480));
            let measure = timing.measures_from(at).next().unwrap();
            let lines = grid(resolution).lines(&measure, timing.ticks_per_beat());
            prop_assert_eq!(lines.first(), Some(&(measure.start, Line::Downbeat)));
            let end = measure.start.checked_add(measure.length).unwrap();
            prop_assert!(lines.iter().all(|(line, _)| *line >= measure.start && *line < end));
            prop_assert!(lines.windows(2).all(|pair| pair.first().zip(pair.get(1)).is_some_and(|(first, second)| first.0 < second.0)));
        }
    }
}
