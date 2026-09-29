//! Timing read from osu!'s `[TimingPoints]`, pasted or in a whole `.osu` file.
//!
//! Each line is `time,beatLength,meter,sampleSet,sampleIndex,volume,uninherited,effects`, with
//! times in milliseconds into the audio. An uninherited point, a red line, sets the tempo, a
//! beat of `beatLength` milliseconds, and starts a measure of `meter` quarter notes. An inherited
//! point, a green line, has a negative `beatLength` and sets the scroll speed to `-100 /
//! beatLength` times.

use std::num::NonZeroU16;

use charter_core::{
    Decimal, Extensions, ImportedTiming, Key, Name, Namespace, Section, SectionError, Tick, Timing,
    TimingEvent, TimingKind, ValidDocument, sectioned,
};

use crate::{ID, SCROLL_SPEED};

/// Why pasted text holds no timing.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum ImportError {
    /// A malformed timing point at the given line.
    #[error("line {line}: {why}")]
    Line {
        /// One-based line number in the input.
        line: usize,
        /// Reason the timing point was rejected.
        why: String,
    },
    /// No red timing point sets an initial tempo.
    #[error("there is no uninherited timing point, which osu! needs to start its tempo")]
    NoTempo,
    /// A timing value cannot fit the document format.
    #[error("{0}")]
    OutOfRange(String),
}

/// A timing point as osu! writes it, with what charter reads of it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Point {
    /// Milliseconds into the audio.
    time: f64,
    /// Milliseconds per beat for a red line; negative inverse scroll speed for a green line.
    beat_length: f64,
    /// Quarter-note beats in a measure.
    meter: NonZeroU16,
    /// True for a red tempo line, false for an inherited green scroll-speed line.
    uninherited: bool,
}

/// A parsed timing point with its source line.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LocatedPoint {
    /// One-based line number in the pasted text or `.osu` file.
    line: usize,
    /// Timing fields parsed from that line.
    point: Point,
}

/// A red line placed on charter's grid: its tick, where that tick falls in milliseconds, which
/// is within half a tick of where osu! has it, and its beat in milliseconds.
#[derive(Clone, Copy, Debug)]
struct Placed {
    /// Tick nearest the red line's audio position.
    tick: i64,
    /// Milliseconds into the audio at `tick`.
    at: f64,
    /// Milliseconds per beat from the red line.
    beat_length: f64,
}

/// The timing in `text` for the song `into`, at its resolution. Scroll speeds are kept only
/// when `into` is an osu!mania song.
pub(crate) fn import(text: &str, into: &ValidDocument) -> Result<ImportedTiming, ImportError> {
    let points = points(text)?;
    let ticks_per_beat = f64::from(u16::from(into.document().ticks_per_beat));
    let mut reds: Vec<LocatedPoint> = points
        .iter()
        .copied()
        .filter(|located| located.point.uninherited)
        .collect();
    reds.sort_by(|a, b| a.point.time.total_cmp(&b.point.time));
    let sections: Vec<Section> = reds
        .iter()
        .map(|red| Section {
            seconds: red.point.time / 1000.0,
            beat_seconds: red.point.beat_length / 1000.0,
            meter: red.point.meter,
        })
        .collect();
    let made = sectioned(&sections, into.document().ticks_per_beat).map_err(|why| match why {
        SectionError::Empty => ImportError::NoTempo,
        SectionError::OutOfRange(what) => ImportError::OutOfRange(what),
    })?;
    let mut left_out = Vec::new();
    if made.merged > 0 {
        left_out.push(format!(
            "{} uninherited points on the tick of a later one",
            made.merged
        ));
    }
    let placed: Vec<(Placed, NonZeroU16)> = made
        .kept
        .iter()
        .map(|&(tick, section)| {
            let red = Placed {
                tick: i64::from(tick),
                at: section.seconds * 1000.0,
                beat_length: section.beat_seconds * 1000.0,
            };
            (red, section.meter)
        })
        .collect();
    let Timing {
        anchors,
        tempo,
        mut events,
    } = made.timing;
    let mut greens: Vec<LocatedPoint> = points
        .iter()
        .copied()
        .filter(|located| !located.point.uninherited)
        .collect();
    greens.sort_by(|a, b| a.point.time.total_cmp(&b.point.time));
    match (into.document().ruleset.id.as_str() == ID, greens.is_empty()) {
        (true, _) => events.extend(scroll_speeds(&placed, &greens, ticks_per_beat)?),
        (false, false) => left_out.push(format!(
            "{} scroll speed changes, which only osu!mania songs keep",
            greens.len()
        )),
        (false, true) => {}
    }
    left_out.push("volumes, hit sounds and kiai time, which are not timing".to_owned());
    events.sort_by_key(|event| event.tick);
    Ok(ImportedTiming {
        timing: Timing {
            anchors,
            tempo,
            events,
        },
        left_out,
    })
}

/// A scroll speed change for each green line, on the red line's grid in effect there, and back
/// to 1 on each red line without one of its own, as osu! resets it.
fn scroll_speeds(
    placed: &[(Placed, NonZeroU16)],
    greens: &[LocatedPoint],
    ticks_per_beat: f64,
) -> Result<Vec<TimingEvent>, ImportError> {
    let mut changes: Vec<(i64, f64, f64, usize)> = greens
        .iter()
        .map(|located| {
            let green = located.point;
            let red = placed
                .iter()
                .rev()
                .find(|(red, _)| red.at <= green.time + 0.5)
                .or_else(|| placed.first())
                .map_or(
                    Placed {
                        tick: 0,
                        at: green.time,
                        beat_length: 1.0,
                    },
                    |(red, _)| *red,
                );
            let tick = red
                .tick
                .checked_add(ticks(green.time - red.at, red.beat_length, ticks_per_beat))
                .ok_or_else(|| line_out_of_range(located.line, "timing point tick"))?;
            Ok((tick, green.time, -100.0 / green.beat_length, located.line))
        })
        .collect::<Result<_, ImportError>>()?;
    let mut resets = Vec::new();
    for (red, _) in placed {
        // The latest change before this red line, a reset at an earlier one included.
        let before = changes
            .iter()
            .chain(&resets)
            .filter(|(_, time, _, _)| *time < red.at)
            .max_by(|left, right| left.1.total_cmp(&right.1))
            .map_or(1.0, |(_, _, ratio, _)| *ratio);
        #[expect(
            clippy::float_cmp,
            reason = "identical source timestamps assign green to red"
        )]
        let own = changes.iter().any(|(_, time, _, _)| *time == red.at);
        #[expect(clippy::float_cmp, reason = "osu!'s own 1x is exact")]
        if !own && before != 1.0 {
            resets.push((red.tick, red.at, 1.0, 0));
        }
    }
    changes.extend(resets);
    changes.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.total_cmp(&right.1))
    });
    // A red reset after an earlier green line wins when both round to the same tick.
    changes.dedup_by(|later, earlier| {
        if later.0 == earlier.0 {
            earlier.2 = later.2;
            earlier.3 = later.3;
            true
        } else {
            false
        }
    });
    changes
        .into_iter()
        .map(|(tick, _, ratio, line)| {
            let ratio = micro(ratio)
                .and_then(|ratio| Decimal::try_from(ratio).ok())
                .filter(|_| ratio > 0.0)
                .ok_or_else(|| {
                    if line == 0 {
                        ImportError::OutOfRange("scroll speed is out of range".into())
                    } else {
                        line_out_of_range(line, "scroll speed")
                    }
                })?;
            let name = Key::new(
                Namespace::new(ID).map_err(out_of_range)?,
                Name::new(SCROLL_SPEED).map_err(out_of_range)?,
            );
            let value = serde_json::to_value(ratio).map_err(out_of_range)?;
            event(tick, TimingKind::Other { name, value }).map_err(|error| match error {
                ImportError::OutOfRange(_) if line != 0 => {
                    line_out_of_range(line, "timing point tick")
                }
                other @ (ImportError::Line { .. }
                | ImportError::NoTempo
                | ImportError::OutOfRange(_)) => other,
            })
        })
        .collect()
}

/// The timing points in `text`: its `[TimingPoints]` section when it has one, as a whole `.osu`
/// file does, or else every line, as a pasted section is.
fn points(text: &str) -> Result<Vec<LocatedPoint>, ImportError> {
    let lines: Vec<(usize, &str)> = (1..).zip(text.lines()).collect();
    let section = lines
        .iter()
        .position(|(_, line)| line.trim() == "[TimingPoints]");
    if section.is_none() && lines.iter().any(|(_, line)| line.trim().starts_with('[')) {
        return Ok(Vec::new());
    }
    let from = section.map_or(0, |at| at.saturating_add(1));
    let mut points = Vec::new();
    for &(number, line) in lines.get(from..).unwrap_or_default() {
        let line = line.trim();
        if line.starts_with('[') {
            break;
        }
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        points.push(LocatedPoint {
            line: number,
            point: point(line).map_err(|why| ImportError::Line { line: number, why })?,
        });
    }
    Ok(points)
}

/// One line of `[TimingPoints]`. Missing fields take osu!'s defaults.
fn point(line: &str) -> Result<Point, String> {
    let fields: Vec<&str> = line.split(',').map(str::trim).collect();
    let number = |index: usize, what: &str| -> Result<Option<f64>, String> {
        match fields.get(index) {
            None | Some(&"") => Ok(None),
            Some(field) => field
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .map(Some)
                .ok_or_else(|| format!("`{field}` is not {what}")),
        }
    };
    let time = number(0, "a time")?.ok_or("there is no time")?;
    let beat_length = number(1, "a beat length")?.ok_or("there is no beat length")?;
    let meter = number(2, "a meter")?.unwrap_or(4.0);
    let uninherited = match number(6, "0 or 1")? {
        None => beat_length > 0.0,
        Some(flag) => flag != 0.0,
    };
    if uninherited && beat_length <= 0.0 {
        return Err(format!(
            "an uninherited point needs a beat length above 0, not {beat_length}"
        ));
    }
    if !uninherited && beat_length >= 0.0 {
        return Err(format!(
            "an inherited point needs a negative beat length, not {beat_length}"
        ));
    }
    // osu! reads a meter of 0 as 4.
    let meter = if (1.0..=f64::from(u16::MAX)).contains(&meter) && meter.fract() == 0.0 {
        NonZeroU16::new(meter_u16(meter))
    } else if meter == 0.0 {
        NonZeroU16::new(4)
    } else {
        None
    };
    let meter = meter.ok_or_else(|| {
        let field = fields.get(2).copied().unwrap_or_default();
        format!("`{field}` is not a meter")
    })?;
    Ok(Point {
        time,
        beat_length,
        meter,
        uninherited,
    })
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "caller checks finite integral meter in the u16 range"
)]
/// Meter already checked as a whole number within the `u16` range.
const fn meter_u16(meter: f64) -> u16 {
    meter as u16
}

/// Whole ticks nearest `milliseconds` at a beat of `beat_length` milliseconds.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "float-to-integer conversion saturates until event checks the i32 range"
)]
fn ticks(milliseconds: f64, beat_length: f64, ticks_per_beat: f64) -> i64 {
    (milliseconds / beat_length * ticks_per_beat).round() as i64
}

/// A format event at `tick`, rejecting ticks outside the chart's `i32` range.
fn event(tick: i64, kind: TimingKind) -> Result<TimingEvent, ImportError> {
    let tick = i32::try_from(tick).map_err(|_out_of_range| {
        ImportError::OutOfRange(format!("tick {tick} is past the song's end"))
    })?;
    Ok(TimingEvent {
        tick: Tick::new(tick),
        kind,
        ext: Extensions::new(),
    })
}

/// `value` to six places, the finest the format holds.
fn micro(value: f64) -> Option<f64> {
    let rounded = (value * 1e6).round() / 1e6;
    rounded.is_finite().then_some(rounded)
}

/// An import error retaining the failing conversion's message.
fn out_of_range(error: impl std::fmt::Display) -> ImportError {
    ImportError::OutOfRange(error.to_string())
}

/// Line-scoped range error for a parsed timing point.
fn line_out_of_range(line: usize, what: &str) -> ImportError {
    ImportError::OutOfRange(format!("line {line}: {what} is out of range"))
}

/// Proofs for Kani, run with `cargo kani -p charter-osu-mania`.
#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    fn an_empty_point_line_is_rejected_without_panicking() {
        assert!(point("").is_err());
    }

    #[kani::proof]
    fn bounded_numeric_point_lines_have_valid_meter_and_inheritance() {
        if kani::any() {
            let parsed = point("1,2,3").unwrap();
            assert!(parsed.meter.get() == 3 && parsed.uninherited);
        } else {
            let parsed = point("0,-1,0").unwrap();
            assert!(parsed.meter.get() == 4 && !parsed.uninherited);
        }
    }

    #[kani::proof]
    fn a_meter_conversion_preserves_every_integral_value_in_range() {
        let meter: u16 = kani::any();
        kani::assume(meter != 0);
        assert!(meter_u16(f64::from(meter)) == meter);
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde_json::{Value, json};

    use super::*;

    const EXAMPLE: &str = "[TimingPoints]
895,288.322921672273,4,1,0,60,1,0
9833,-100,4,1,0,45,0,0
18194,-100,4,1,0,60,0,0
37728,-100,4,1,0,45,0,0
38380,288.322921672273,4,1,0,60,1,0
38380,-100,4,1,0,45,0,0
75285,-100,4,1,0,60,0,1
109884,-100,4,1,0,60,0,0
112190,-100,4,1,0,60,0,1
150249,-100,4,1,0,60,0,0
";

    fn song(ruleset: &str) -> ValidDocument {
        let value = json!({
            "format_version": 0,
            "ruleset": { "id": ruleset, "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "a.ogg" },
            "timing": { "offset_seconds": 0.0, "events": [ { "tick": 0, "kind": { "bpm": 120.0 } } ] },
            "charts": []
        });
        ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap()
    }

    fn events(imported: &ImportedTiming) -> Value {
        serde_json::to_value(&imported.timing.events).unwrap()
    }

    fn speed(tick: i64) -> Value {
        json!({ "tick": tick, "kind": { "other": { "name": "osu_mania.scroll_speed", "value": 1.0 } } })
    }

    #[test]
    fn red_lines_become_tempo_and_green_lines_scroll_speeds_on_the_grid() {
        let imported = import(EXAMPLE, &song(ID)).unwrap();
        assert_eq!(
            serde_json::to_value(&imported.timing.anchors).unwrap(),
            json!([
                { "tick": 0, "seconds": 0.895 },
                { "tick": 124_810, "seconds": 38.38 }
            ])
        );
        let mut expected = vec![
            speed(29760),
            speed(57599),
            speed(122_639),
            json!({ "tick": 124_810, "kind": { "time_signature": [4, 4] } }),
            speed(124_810),
        ];
        expected.extend([247_689, 362_890, 370_568, 497_289].map(speed));
        assert_eq!(events(&imported), Value::Array(expected));
        let timing = ValidDocument::new(charter_core::Document {
            timing: imported.timing,
            ..song(ID).document().clone()
        });
        timing.unwrap();
    }

    #[test]
    fn a_song_of_another_game_takes_the_tempo_and_leaves_the_scroll_speeds() {
        let imported = import(EXAMPLE, &song("sm")).unwrap();
        let kinds: Vec<Value> = imported
            .timing
            .events
            .iter()
            .map(|e| json!(e.kind))
            .collect();
        assert!(kinds.iter().all(|kind| kind.get("other").is_none()));
        assert_eq!(imported.timing.anchors.len(), 2);
        assert!(
            imported
                .left_out
                .iter()
                .any(|note| note.starts_with("8 scroll speed changes"))
        );
    }

    #[test]
    fn a_whole_osu_file_gives_its_timing_points_alone() {
        let file = format!(
            "osu file format v14\n\n[General]\nAudioFilename: a.mp3\n\n{EXAMPLE}\n[HitObjects]\n64,192,1000,1,0,0:0:0:0:\n"
        );
        let whole = import(&file, &song(ID)).unwrap();
        assert_eq!(whole, import(EXAMPLE, &song(ID)).unwrap());
    }

    #[test]
    fn a_meter_other_than_four_starts_its_measures_at_its_red_line() {
        let imported = import("0,500,3,1,0,100,1,0\n6000,500,4,1,0,100,1,0", &song("sm")).unwrap();
        assert_eq!(
            events(&imported),
            json!([
                { "tick": 0, "kind": { "time_signature": [3, 4] } },
                { "tick": 11520, "kind": { "time_signature": [4, 4] } },
            ])
        );
    }

    #[test]
    fn a_red_line_on_a_measure_line_needs_no_new_measure() {
        let imported = import("0,500,4,1,0,100,1,0\n4000,400,4,1,0,100,1,0", &song("sm")).unwrap();
        assert_eq!(events(&imported), json!([]));
        assert_eq!(imported.timing.anchors.len(), 2);
    }

    #[test]
    fn green_lines_after_a_red_line_are_reset_by_the_next_red_line() {
        let text = "0,500,4,1,0,100,1,0\n1000,-50,4,1,0,100,0,0\n4000,500,4,1,0,100,1,0";
        let imported = import(text, &song(ID)).unwrap();
        let speeds: Vec<(Value, Value)> = imported
            .timing
            .events
            .iter()
            .filter_map(|e| match &e.kind {
                TimingKind::Other { value, .. } => Some((json!(e.tick), value.clone())),
                TimingKind::Stop(_)
                | TimingKind::Delay(_)
                | TimingKind::Warp(_)
                | TimingKind::TimeSignature(_) => None,
            })
            .collect();
        assert_eq!(
            speeds,
            [(json!(1920), json!(2.0)), (json!(7680), json!(1.0))]
        );
        let again = format!("{text}\n6000,500,4,1,0,100,1,0");
        let second_import = import(&again, &song(ID)).unwrap();
        let reset_speeds = second_import
            .timing
            .events
            .iter()
            .filter(|e| matches!(e.kind, TimingKind::Other { .. }));
        assert_eq!(reset_speeds.count(), 2);
    }

    #[test]
    fn a_red_line_resets_a_green_line_snapped_to_the_same_tick() {
        let text = "0,2000,4,1,0,100,1,0\n999,-50,4,1,0,100,0,0\n1000,2000,4,1,0,100,1,0";
        let imported = import(text, &song(ID)).unwrap();
        let speeds: Vec<_> = imported
            .timing
            .events
            .iter()
            .filter_map(|event| match &event.kind {
                TimingKind::Other { value, .. } => Some((event.tick, value.clone())),
                TimingKind::Stop(_)
                | TimingKind::Delay(_)
                | TimingKind::Warp(_)
                | TimingKind::TimeSignature(_) => None,
            })
            .collect();
        assert_eq!(speeds, [(Tick::new(480), json!(1.0))]);
    }

    #[test]
    fn finite_red_points_past_the_tick_range_return_an_error() {
        let text = "0,500\n1e100,500\n2e100,500";
        assert!(matches!(
            import(text, &song(ID)),
            Err(ImportError::OutOfRange(_))
        ));
    }

    #[test]
    fn finite_green_points_past_the_tick_range_return_an_error() {
        let text = "0,500\n500,500\n1e100,-100";
        assert!(matches!(
            import(text, &song(ID)),
            Err(ImportError::OutOfRange(_))
        ));
    }

    #[test]
    fn text_without_a_tempo_or_with_a_bad_line_says_so() {
        assert_eq!(import("", &song(ID)), Err(ImportError::NoTempo));
        assert_eq!(
            import("100,-100,4,1,0,60,0,0", &song(ID)),
            Err(ImportError::NoTempo)
        );
        let bad = import("0,500,4,1,0,100,1,0\nsoon,500", &song(ID)).unwrap_err();
        assert_eq!(bad.to_string(), "line 2: `soon` is not a time");
        let backwards = import("0,-500,4,1,0,100,1,0", &song(ID)).unwrap_err();
        assert!(
            backwards
                .to_string()
                .starts_with("line 1: an uninherited point")
        );
    }

    #[test]
    fn missing_required_fields_and_nonfinite_numbers_report_their_line() {
        for (text, why) in [
            (",500", "there is no time"),
            ("0,", "there is no beat length"),
            ("0,nan", "`nan` is not a beat length"),
            ("inf,500", "`inf` is not a time"),
            ("0,500,3.5", "`3.5` is not a meter"),
            ("0,500,65536", "`65536` is not a meter"),
            ("0,500,4,1,0,100,no", "`no` is not 0 or 1"),
        ] {
            assert_eq!(
                import(&format!("// a comment\n{text}"), &song(ID)),
                Err(ImportError::Line {
                    line: 2,
                    why: why.to_owned()
                }),
                "{text}"
            );
        }
    }

    #[test]
    fn zero_and_wrongly_signed_beat_lengths_are_refused() {
        for text in ["0,0", "0,-500,4,1,0,100,1", "0,500,4,1,0,100,0"] {
            assert!(
                matches!(
                    import(text, &song(ID)),
                    Err(ImportError::Line { line: 1, .. })
                ),
                "{text}"
            );
        }
    }

    #[test]
    fn crlf_comments_and_missing_optional_fields_preserve_timing() {
        let text = "[TimingPoints]\r\n// ignore\r\n125,500\r\n\r\n[HitObjects]\r\nbad";
        let imported = import(text, &song(ID)).unwrap();
        assert_eq!(
            imported.timing.anchors,
            vec![charter_core::Anchor {
                tick: Tick::new(0),
                seconds: charter_core::Origin::try_from(0.125).unwrap()
            }]
        );
        assert_eq!(
            imported.timing.tempo,
            charter_core::Bpm::try_from(120.0).unwrap()
        );
        assert!(imported.timing.events.is_empty());
    }

    #[test]
    fn a_zero_meter_uses_osus_four_beat_default() {
        let imported = import("0,500,0\n2000,500,3", &song(ID)).unwrap();
        assert_eq!(
            events(&imported),
            json!([{ "tick": 3840, "kind": { "time_signature": [3, 4] } }])
        );
    }

    #[test]
    fn a_whole_file_without_timing_points_does_not_invent_a_tempo() {
        let text = "osu file format v14\n[General]\nAudioFilename: a.ogg\n[HitObjects]\n";
        assert_eq!(import(text, &song(ID)), Err(ImportError::NoTempo));
    }

    #[test]
    fn duplicate_red_times_keep_the_last_tempo_and_report_the_discarded_point() {
        let imported = import("0,500\n0,250\n1000,250", &song(ID)).unwrap();
        assert_eq!(
            imported.timing.tempo,
            charter_core::Bpm::try_from(240.0).unwrap()
        );
        assert_eq!(imported.timing.anchors.len(), 2);
        assert!(
            imported
                .left_out
                .iter()
                .any(|item| item.starts_with("1 uninherited point"))
        );
        let timing = ValidDocument::new(charter_core::Document {
            timing: imported.timing,
            ..song(ID).document().clone()
        });
        timing.unwrap();
    }

    #[test]
    fn a_red_point_before_audio_zero_keeps_its_negative_anchor() {
        let imported = import("-500,500\n500,500", &song(ID)).unwrap();
        assert_eq!(
            serde_json::to_value(&imported.timing.anchors).unwrap(),
            json!([{"tick": 0, "seconds": -0.5}, {"tick": 1920, "seconds": 0.5}])
        );
    }

    #[test]
    fn a_green_point_before_the_first_red_uses_the_first_red_grid() {
        let imported = import("-500,-50\n0,500", &song(ID)).unwrap();
        assert_eq!(
            events(&imported),
            json!([
                {"tick": -960, "kind": {"other": {"name": "osu_mania.scroll_speed", "value": 2.0}}},
                {"tick": 0, "kind": {"other": {"name": "osu_mania.scroll_speed", "value": 1.0}}}
            ])
        );
    }

    #[test]
    fn enormous_red_and_green_points_are_refused_instead_of_wrapping_ticks_or_ratios() {
        for text in ["0,1e-300\n1,500", "0,500\n1,-1e-300", "0,500\n1e100,-50"] {
            assert!(
                matches!(import(text, &song(ID)), Err(ImportError::OutOfRange(_))),
                "{text}"
            );
        }
    }

    #[test]
    fn out_of_range_green_points_report_their_line() {
        let tick = import("0,500\n1e100,-50", &song(ID)).unwrap_err();
        assert_eq!(
            tick.to_string(),
            "line 2: timing point tick is out of range"
        );
        let speed = import("0,500\n1,-1e-300", &song(ID)).unwrap_err();
        assert_eq!(speed.to_string(), "line 2: scroll speed is out of range");
    }

    proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn arbitrary_timing_text_never_panics(text in proptest::collection::vec(any::<char>(), 0..256)) {
            let text: String = text.into_iter().collect();
            drop(import(&text, &song(ID)));
        }
    }
}
