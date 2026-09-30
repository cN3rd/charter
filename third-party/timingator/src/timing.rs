//! Beat events into osu! timing points: one uninherited timing point per change of tempo or
//! meter, each BPM rounded as far as the beats allow.
//!
//! Ported from OliBomby/Mapperatorinator (MIT), `osuT5/osuT5/inference/postprocessor.py`
//! (`Postprocessor.generate_timing` and its helpers) and `timing_points_change.py`
//! (`TimingPointsChange.add_change` for uninherited points, the only kind this makes). Offsets
//! are whole microseconds, as the `timedelta`s upstream keeps them in.

use crate::super_timing::Beat;
use crate::tokenizer::{Event, TIME_SHIFT, round_half_even};

#[derive(Clone, Debug, PartialEq)]
pub struct TimingPoint {
    pub offset_us: i64,
    pub ms_per_beat: f64,
    pub meter: i64,
}

impl TimingPoint {
    fn new(offset_us: i64, ms_per_beat: f64, meter: i64) -> Self {
        Self {
            offset_us,
            ms_per_beat,
            meter,
        }
    }

    pub fn bpm(&self) -> f64 {
        60_000.0 / self.ms_per_beat
    }

    /// Milliseconds, as `round(tp.offset.total_seconds() * 1000)`.
    fn offset_ms(&self) -> i64 {
        round_half_even(self.offset_us as f64 / 1e6 * 1000.0) as i64
    }

    /// Its line in a .osu file's `[TimingPoints]`, as slider's `TimingPoint.pack` writes it.
    pub fn to_osu(&self) -> String {
        let beat = if self.ms_per_beat.trunc() == self.ms_per_beat {
            format!("{}", self.ms_per_beat as i64)
        } else {
            format!("{}", self.ms_per_beat)
        };
        format!(
            "{},{},{},2,-1,100,1,0",
            self.offset_us.div_euclid(1000),
            beat,
            self.meter
        )
    }
}

/// `timedelta(milliseconds=ms)` in microseconds.
fn us(ms: f64) -> i64 {
    round_half_even(ms * 1000.0) as i64
}

/// `TimingPointsChange(tp, mpb=mpb, meter=meter, uninherited=True).add_change(timing)` on a list
/// of uninherited points.
fn add_change(timing: &mut Vec<TimingPoint>, tp: &TimingPoint, mpb: bool, meter: bool) {
    let mut prev: Option<usize> = None;
    let mut on: Vec<usize> = Vec::new();
    for (i, t) in timing.iter().enumerate() {
        if t.offset_us < tp.offset_us && prev.is_none_or(|p| t.offset_us >= timing[p].offset_us) {
            prev = Some(i);
        }
        let (a, b) = (t.offset_us as f64 / 1e6, tp.offset_us as f64 / 1e6);
        if (a - b).abs() <= (1e-9 * a.abs().max(b.abs())).max(0.002) {
            on.push(i);
        }
    }
    if on.is_empty() {
        let mut adding = prev.map_or_else(|| tp.clone(), |p| timing[p].clone());
        adding.offset_us = tp.offset_us;
        timing.push(adding);
        on.push(timing.len() - 1);
    }
    for i in on {
        if mpb {
            timing[i].ms_per_beat = tp.ms_per_beat;
        }
        if meter {
            timing[i].meter = tp.meter;
        }
    }
    timing.sort_by_key(|t| t.offset_us);
}

/// The last point at or before `time_us`, or the first.
fn at(timing: &[TimingPoint], time_us: i64) -> usize {
    timing
        .iter()
        .rposition(|t| t.offset_us <= time_us)
        .unwrap_or(0)
}

#[derive(Clone, Copy)]
struct Marker {
    time: i64,
    is_measure: bool,
    is_redline: bool,
    beats_from_last_marker: f64,
}

pub struct TimingBuilder {
    /// Milliseconds a beat may be off the grid before the grid changes.
    pub leniency: f64,
}

impl TimingBuilder {
    fn is_snapped(time: f64, resnapped: f64, leniency: f64) -> bool {
        (time - resnapped).abs() <= leniency
    }

    fn check_ms_per_beat(&self, mpb: f64, markers: &[Marker], redline_offset: i64) -> bool {
        let mut beats = 0.0;
        markers.iter().all(|m| {
            beats += m.beats_from_last_marker;
            Self::is_snapped(
                m.time as f64,
                redline_offset as f64 + mpb * beats,
                self.leniency,
            )
        })
    }

    /// The beat lengths of whole, half, tenth, hundredth and thousandth BPMs near `bpm`.
    fn candidates(bpm: f64) -> [f64; 5] {
        [1.0, 2.0, 10.0, 100.0, 1000.0].map(|d| 60_000.0 / (round_half_even(bpm * d) / d))
    }

    fn human_round_ms_per_beat(&self, mpb: f64, markers: &[Marker], redline_offset: i64) -> f64 {
        if mpb == 0.0 || mpb > 60_000.0 {
            return mpb;
        }
        Self::candidates(60_000.0 / mpb)
            .into_iter()
            .find(|&c| self.check_ms_per_beat(c, markers, redline_offset))
            .unwrap_or(mpb)
    }

    fn get_ms_per_beat(time_from_redline: f64, beats_from_redline: f64, leniency: f64) -> f64 {
        if beats_from_redline == 0.0 || time_from_redline == 0.0 {
            return 1000.0;
        }
        let mpb = time_from_redline / beats_from_redline;
        let bpm = 60_000.0 / mpb;
        if bpm < 1.0 {
            return 1000.0;
        }
        Self::candidates(bpm)
            .into_iter()
            .find(|&c| Self::is_snapped(time_from_redline, c * beats_from_redline, leniency))
            .unwrap_or(mpb)
    }

    fn get_mpb(markers: &[Marker], redline_offset: i64) -> f64 {
        let beats: f64 = markers.iter().map(|m| m.beats_from_last_marker).sum();
        Self::get_ms_per_beat(
            (markers[markers.len() - 1].time - redline_offset) as f64,
            beats,
            0.0,
        )
    }

    /// The timing points of `events`, time shift then type, earliest first.
    pub fn generate_timing(&self, events: &[Event]) -> Vec<TimingPoint> {
        let mut markers: Vec<Marker> = Vec::new();
        for i in 1..events.len() {
            let kind = match events[i].kind.as_str() {
                "beat" => Beat::Beat,
                "measure" => Beat::Measure,
                "timing_point" => Beat::TimingPoint,
                _ => continue,
            };
            if events[i - 1].kind == TIME_SHIFT {
                markers.push(Marker {
                    time: i64::from(events[i - 1].value),
                    is_measure: kind == Beat::Measure,
                    is_redline: kind == Beat::TimingPoint,
                    beats_from_last_marker: if kind == Beat::TimingPoint { 0.0 } else { 1.0 },
                });
            }
        }
        if markers.is_empty() {
            return Vec::new();
        }
        markers.sort_by_key(|m| m.time);

        let mut timing: Vec<TimingPoint> = Vec::new();
        for m in markers.iter().filter(|m| m.is_redline) {
            add_change(
                &mut timing,
                &TimingPoint::new(us(m.time as f64), 1000.0, 4),
                false,
                false,
            );
        }
        if timing.is_empty() {
            timing.push(TimingPoint::new(us(markers[0].time as f64), 1000.0, 4));
        }

        // Redlines to keep the measure count right.
        let mut counter = 0_i64;
        let mut last_measure_time = markers[0].time;
        for m in &markers {
            let time = m.time;
            if m.is_redline {
                counter = 0;
                last_measure_time = time;
                continue;
            }
            let r = at(&timing, us((time - 1) as f64));
            let redline_offset = timing[r].offset_ms();
            if redline_offset == time {
                continue;
            }
            counter += 1;
            if !m.is_measure {
                continue;
            }
            if timing[r].meter != counter {
                if last_measure_time <= redline_offset {
                    timing[r].meter = counter;
                } else {
                    add_change(
                        &mut timing,
                        &TimingPoint::new(us(last_measure_time as f64), 1000.0, counter),
                        false,
                        true,
                    );
                }
            }
            counter = 0;
            last_measure_time = time;
        }

        // Redlines to snap each beat.
        counter = 0;
        let mut last_mpb = 1000.0;
        for idx in 0..markers.len() {
            let time = markers[idx].time;
            let r = at(&timing, us((time - 1) as f64));
            let redline_offset = timing[r].offset_ms();
            if redline_offset == time {
                counter = 0;
                continue;
            }
            // Never the marker on the redline itself.
            let mut before: Vec<Marker> = markers
                .iter()
                .filter(|o| redline_offset < o.time && o.time < time)
                .copied()
                .collect();
            before.push(markers[idx]);

            if markers[idx].beats_from_last_marker == 0.0 {
                if before.len() != 1 {
                    counter = 0;
                    continue;
                }
                // A redline after a redline: give the earlier a BPM from the beats between them.
                let beats = (time - redline_offset) as f64 / last_mpb;
                let rounded = [round_half_even(beats), 1.0, 0.5, 0.25, 0.125, 0.0625];
                let pick = rounded.iter().copied().fold(rounded[0], |best, x| {
                    if (x - beats).abs() < (best - beats).abs() {
                        x
                    } else {
                        best
                    }
                });
                markers[idx].beats_from_last_marker = pick;
                let last = before.len() - 1;
                before[last].beats_from_last_marker = pick;
            }

            let mut mpb = Self::get_mpb(&before, redline_offset);
            if self.check_ms_per_beat(mpb, &before, redline_offset) {
                mpb = self.human_round_ms_per_beat(mpb, &before, redline_offset);
                timing[r].ms_per_beat = mpb;
            } else if before.len() > 1 {
                // Split the section where the two halves fit their own tempos best.
                let test_split = |m: &[Marker], o: i64| -> f64 {
                    let mpb = Self::get_mpb(m, o);
                    let mut beats = 0.0;
                    let loss: f64 = m
                        .iter()
                        .map(|b| {
                            beats += b.beats_from_last_marker;
                            (b.time as f64 - (o as f64 + beats * mpb)).powi(2)
                        })
                        .sum();
                    loss / m.len() as f64
                };
                let mut best_loss = f64::INFINITY;
                let mut best_split = before.len() - 1;
                for i in 1..before.len() {
                    let split = before[i - 1];
                    if !split.is_measure && i != before.len() - 1 {
                        continue;
                    }
                    let loss = test_split(&before[..i], redline_offset)
                        + test_split(&before[i..], split.time);
                    if loss < best_loss {
                        best_loss = loss;
                        best_split = i;
                    }
                }
                if best_split < before.len() - 1 {
                    let m = Self::get_mpb(&before[..best_split], redline_offset);
                    timing[r].ms_per_beat =
                        self.human_round_ms_per_beat(m, &before[..best_split], redline_offset);
                }
                let last_time = before[best_split - 1].time;
                let beats_from_split: f64 = before[best_split..]
                    .iter()
                    .map(|m| m.beats_from_last_marker)
                    .sum();
                mpb = Self::get_ms_per_beat(
                    (time - last_time) as f64,
                    beats_from_split,
                    self.leniency,
                );
                add_change(
                    &mut timing,
                    &TimingPoint::new(us(last_time as f64), mpb, 4),
                    true,
                    false,
                );
                counter = 0;
                for i in (best_split..before.len().saturating_sub(1)).rev() {
                    if before[i].is_measure {
                        break;
                    }
                    counter += 1;
                }
            }
            last_mpb = mpb;
            counter += 1;

            let r = at(&timing, us(time as f64));
            if timing[r].offset_ms() == time {
                counter = 0;
            }
            if markers[idx].is_measure {
                // A redline where the measure count is out of step.
                if counter % timing[r].meter != 0 {
                    let tp =
                        TimingPoint::new(us(time as f64), timing[r].ms_per_beat, timing[r].meter);
                    add_change(&mut timing, &tp, true, false);
                }
                counter = 0;
            }
        }
        timing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(beats: &[(i32, &str)]) -> Vec<Event> {
        beats
            .iter()
            .flat_map(|&(t, k)| [Event::new(TIME_SHIFT, t), Event::new(k, 0)])
            .collect()
    }

    #[test]
    fn a_steady_grid_is_one_timing_point_at_a_round_bpm() {
        let beats: Vec<(i32, &str)> = (0..32)
            .map(|i| {
                (
                    59 + i * 300 + [0, 1, -1][i as usize % 3],
                    if i % 4 == 0 { "measure" } else { "beat" },
                )
            })
            .collect();
        let timing = TimingBuilder { leniency: 20.0 }.generate_timing(&grid(&beats));
        assert_eq!(timing, vec![TimingPoint::new(59_000, 300.0, 4)]);
        assert_eq!(timing[0].to_osu(), "59,300,4,2,-1,100,1,0");
    }

    #[test]
    fn a_tempo_change_starts_a_new_timing_point() {
        let mut beats: Vec<(i32, &str)> = (0..16)
            .map(|i| (1000 + i * 500, if i % 4 == 0 { "measure" } else { "beat" }))
            .collect();
        beats
            .extend((0..16).map(|i| (9000 + i * 400, if i % 4 == 0 { "measure" } else { "beat" })));
        let timing = TimingBuilder { leniency: 20.0 }.generate_timing(&grid(&beats));
        assert_eq!(
            timing,
            vec![
                TimingPoint::new(1_000_000, 500.0, 4),
                TimingPoint::new(9_000_000, 400.0, 4)
            ]
        );
    }

    #[test]
    fn three_beats_to_a_bar_set_the_meter() {
        let beats: Vec<(i32, &str)> = (0..12)
            .map(|i| (500 + i * 600, if i % 3 == 0 { "measure" } else { "beat" }))
            .collect();
        let timing = TimingBuilder { leniency: 20.0 }.generate_timing(&grid(&beats));
        assert_eq!(timing, vec![TimingPoint::new(500_000, 600.0, 3)]);
    }

    #[test]
    fn fractional_beat_lengths_print_as_python_does() {
        assert_eq!(
            TimingPoint::new(1_500, 60_000.0 / 180.0, 4).to_osu(),
            "1,333.3333333333333,4,2,-1,100,1,0"
        );
    }
}
