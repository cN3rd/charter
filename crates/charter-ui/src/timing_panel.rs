//! The song panel's timing section, laid out as osu!'s timing panel is: where the song starts and
//! at what tempo, every timing point in a list by its moment in the audio, and below it the
//! fields of the point in effect at the cursor, which the list highlights. Clicking a point in
//! the list moves the cursor to it.

use charter_core::{Bpm, Origin, Tick, TimeSignature, Timing, TimingKind, Valid};
use charter_editor::Song;
use egui::{RichText, Ui};

use crate::Action;
use crate::panels::entry;
use crate::style::{self, icon, table};

/// What is being typed into the section's fields, while one has the keyboard.
#[derive(Debug, Default)]
pub struct Drafts {
    bpm: Option<String>,
    meter: Option<String>,
    offset: Option<String>,
    start_bpm: Option<String>,
    time: Option<String>,
}

/// A tick of the timing holding events, with what they set.
#[derive(Debug)]
struct Point {
    tick: Tick,
    bpm: Option<Bpm>,
    meter: Option<TimeSignature>,
    /// Stops, delays, warps and game events, as words.
    others: Vec<String>,
}

/// Every timing point of `timing`, in order.
fn points(timing: Valid<'_, charter_core::Timing>) -> Vec<Point> {
    let mut points: Vec<Point> = timing
        .anchors
        .iter()
        .map(|anchor| Point {
            tick: anchor.tick,
            bpm: timing.bpm_at(anchor.tick),
            meter: None,
            others: Vec::new(),
        })
        .collect();
    points.extend(
        timing
            .events
            .chunk_by(|a, b| a.tick == b.tick)
            .filter_map(|events| {
                let tick = events.first()?.tick;
                let mut point = Point {
                    tick,
                    bpm: None,
                    meter: None,
                    others: Vec::new(),
                };
                for event in events {
                    match &event.kind {
                        TimingKind::TimeSignature(meter) => point.meter = Some(*meter),
                        TimingKind::Stop(pause) => point.others.push(format!("stop {pause} s")),
                        TimingKind::Delay(pause) => point.others.push(format!("delay {pause} s")),
                        TimingKind::Warp(ticks) => point.others.push(format!("warp {ticks}")),
                        TimingKind::Other { name, value } => {
                            point.others.push(format!("{} {value}", name.name()));
                        }
                    }
                }
                Some(point)
            }),
    );
    points.sort_by_key(|point| point.tick);
    let mut merged: Vec<Point> = Vec::new();
    for point in points {
        if let Some(last) = merged.last_mut()
            && last.tick == point.tick
        {
            last.bpm = last.bpm.or(point.bpm);
            last.meter = last.meter.or(point.meter);
            last.others.extend(point.others);
            continue;
        }
        merged.push(point);
    }
    merged
}

/// The tempo in effect on `tick`: the latest BPM change at or before it, or the first.
fn tempo_at(timing: Valid<'_, charter_core::Timing>, tick: Tick) -> Option<Bpm> {
    timing.bpm_at(tick)
}

/// The meter in effect on `tick`: the latest time signature at or before it, or common time.
fn meter_at(timing: &Timing, tick: Tick) -> TimeSignature {
    timing
        .events
        .iter()
        .filter(|event| event.tick <= tick)
        .filter_map(|event| match event.kind {
            TimingKind::TimeSignature(meter) => Some(meter),
            _ => None,
        })
        .next_back()
        .unwrap_or(TimeSignature::COMMON)
}

/// A meter typed as `numerator/denominator`.
fn meter(typed: &str) -> Option<TimeSignature> {
    let (numerator, denominator) = typed.split_once('/')?;
    Some(TimeSignature {
        numerator: numerator.trim().parse().ok()?,
        denominator: denominator.trim().parse().ok()?,
    })
}

/// The timing section of the song panel. Says why something typed into it is not a value.
pub fn section(
    ui: &mut Ui,
    song: &Song,
    drafts: &mut Drafts,
    actions: &mut Vec<Action>,
) -> Option<String> {
    let mut refused = None;
    style::section(ui, "Timing", |ui| {
        refused = body(ui, song, drafts, actions);
    });
    refused
}

fn body(
    ui: &mut Ui,
    song: &Song,
    drafts: &mut Drafts,
    actions: &mut Vec<Action>,
) -> Option<String> {
    let editable = song.read_only().is_none();
    let timing = song.timing();
    let mut refused = None;
    let first = timing.anchors.first().map(|anchor| anchor.tick);
    table(ui, "offset", |ui| {
        let caption = ui.weak("Offset").on_hover_text(OFFSET_ABOUT);
        let seconds = timing.time_at(Tick::new(0)).seconds();
        refused = offset_rows(
            ui,
            (editable, caption.id),
            seconds,
            &mut drafts.offset,
            actions,
        );
        if let Some(first) = first {
            let caption = ui
                .weak("Starting BPM")
                .on_hover_text("The tempo from the first timing point on, until the next.");
            let offered = bpm_row(
                ui,
                (editable, caption.id, "starting BPM"),
                (first, tempo_at(timing, first)),
                &mut drafts.start_bpm,
                actions,
            );
            refused = refused.take().or(offered);
            ui.end_row();
        }
    });
    if editable {
        let sync = style::icon_button(ui, icon::WAVEFORM, "Sync to the audio", false)
            .on_hover_text("Open the timing window (Ctrl+T) to line beats up with the waveform");
        if sync.clicked() {
            actions.push(Action::ShowSync);
        }
    }
    ui.add_space(10.0);
    let points = points(timing);
    let current = song.timing_point();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Timing points").strong());
        ui.weak(points.len().to_string());
        if editable {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let free = current != Some(song.cursor());
                let add = ui
                    .add_enabled_ui(free, |ui| {
                        style::icon_button(ui, icon::PLUS, "Add a timing point here", false)
                    })
                    .inner
                    .on_hover_text("A new point at the cursor, at the tempo it has there")
                    .on_disabled_hover_text("The cursor is on a timing point already");
                if add.clicked() {
                    actions.push(Action::AddBpm);
                }
            });
        }
    });
    list(ui, timing, &points, current, actions);
    let Some(tick) = current else {
        return refused;
    };
    ui.add_space(6.0);
    let offered = point_fields(ui, song, tick, drafts, actions);
    refused.or(offered)
}

/// The timing points, one a row, the one in effect at the cursor highlighted.
fn list(
    ui: &mut Ui,
    timing: Valid<'_, Timing>,
    points: &[Point],
    current: Option<Tick>,
    actions: &mut Vec<Action>,
) {
    let height = ui.text_style_height(&egui::TextStyle::Monospace) + 4.0;
    egui::Frame::new()
        .fill(style::FIELD)
        .corner_radius(4)
        .inner_margin(egui::Margin::same(3))
        .show(ui, |ui| {
            style::solid_scroll_bars(ui);
            egui::ScrollArea::vertical()
                .id_salt("timing points")
                .max_height(height * 6.0)
                .auto_shrink([false, true])
                .show_rows(ui, height, points.len(), |ui, rows| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.spacing_mut().button_padding.y = 1.0;
                    for point in points.get(rows).unwrap_or_default() {
                        let at = style::clock(timing.time_at(point.tick).seconds());
                        let tempo = point
                            .bpm
                            .map_or_else(String::new, |bpm| format!("{bpm} BPM"));
                        let meter = point
                            .meter
                            .map_or_else(String::new, |meter| meter.to_string());
                        let text =
                            format!("{at:<10}{tempo:<12}{meter:<6}{}", point.others.join(", "));
                        let selected = current == Some(point.tick);
                        let row =
                            egui::Button::selectable(selected, RichText::new(text).monospace())
                                .min_size(egui::vec2(ui.available_width(), height));
                        let name = format!("Timing point at {at}");
                        let response = ui.add(row);
                        response.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Button,
                                true,
                                selected,
                                &name,
                            )
                        });
                        if response.clicked() {
                            actions.push(Action::JumpTo(point.tick));
                        }
                    }
                });
        });
}

/// The fields of the timing point on `tick`: its tempo and meter, in effect there whether or not
/// it sets them, and a button that removes it unless it starts the tempo.
fn point_fields(
    ui: &mut Ui,
    song: &Song,
    tick: Tick,
    drafts: &mut Drafts,
    actions: &mut Vec<Action>,
) -> Option<String> {
    let timing = song.timing();
    let editable = song.read_only().is_none();
    let bpm = tempo_at(timing, tick);
    let meter_now = meter_at(&timing, tick);
    let mut refused = None;
    let first = timing.anchors.first().map(|anchor| anchor.tick);
    let anchored = timing.anchors.iter().any(|anchor| anchor.tick == tick);
    table(ui, "timing point", |ui| {
        let caption = ui
            .weak("Time")
            .on_hover_text("Where the point is in the audio. A tempo point moves there.");
        let seconds = timing.time_at(tick).seconds();
        if editable && anchored {
            let to = |typed: &str| {
                moment(typed)
                    .map(|at| Action::MovePointTo(tick, at))
                    .ok_or_else(|| format!("{typed} is not a time, such as 00:26.100"))
            };
            refused = entry(
                ui,
                caption.id,
                &style::clock(seconds),
                &mut drafts.time,
                to,
                actions,
            );
        } else {
            ui.label(style::clock(seconds));
        }
        ui.end_row();
        let caption = ui.weak("BPM");
        let offered = bpm_row(
            ui,
            (editable, caption.id, "BPM"),
            (tick, bpm),
            &mut drafts.bpm,
            actions,
        );
        refused = refused.take().or(offered);
        ui.end_row();
        let caption = ui.weak("Meter").on_hover_text("Beats a measure over the note that gets one, such as 3/4 or 7/8. Measures start over at the point.");
        if editable {
            let read = |typed: &str| {
                meter(typed)
                    .map(|meter| Action::SetMeterAt(tick, meter))
                    .ok_or_else(|| format!("{typed} is not a meter, such as 3/4"))
            };
            let offered = entry(
                ui,
                caption.id,
                &meter_now.to_string(),
                &mut drafts.meter,
                read,
                actions,
            );
            refused = refused.take().or(offered);
        } else {
            ui.label(meter_now.to_string());
        }
        ui.end_row();
    });
    if editable && Some(tick) != first {
        let remove = style::icon_button(ui, icon::TRASH, "Remove this timing point", false)
            .on_hover_text("The tempo and meter before it carry on");
        if remove.clicked() {
            actions.push(Action::RemoveTimingAt(tick));
        }
    }
    refused
}

/// The BPM on `tick`, `bpm`, with steppers when `editable`, each setting the tempo there.
fn bpm_row(
    ui: &mut Ui,
    (editable, caption, what): (bool, egui::Id, &str),
    (tick, bpm): (Tick, Option<Bpm>),
    draft: &mut Option<String>,
    actions: &mut Vec<Action>,
) -> Option<String> {
    let Some(bpm) = bpm.filter(|_| editable) else {
        ui.label(bpm.map_or_else(|| "none".to_owned(), |bpm| bpm.to_string()));
        return None;
    };
    let mut refused = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let step = |by: f64| {
            Bpm::try_from(f64::from(bpm) + by)
                .ok()
                .map(|bpm| Action::SetBpmAt(tick, bpm))
        };
        if stepper(
            ui,
            "−",
            &format!("Lower the {what} by 1"),
            "F7 at the cursor",
        ) {
            actions.extend(step(-1.0));
        }
        let tempo = |typed: &str| {
            typed
                .parse::<f64>()
                .ok()
                .and_then(|bpm| Bpm::try_from(bpm).ok())
                .map(|bpm| Action::SetBpmAt(tick, bpm))
                .ok_or_else(|| format!("{typed} is not a tempo"))
        };
        refused = entry(ui, caption, &bpm.to_string(), draft, tempo, actions);
        if stepper(
            ui,
            "+",
            &format!("Raise the {what} by 1"),
            "F8 at the cursor",
        ) {
            actions.extend(step(1.0));
        }
    });
    refused
}

/// A moment typed as seconds, such as `26.1`, or minutes and seconds, such as `00:26.100`.
fn moment(typed: &str) -> Option<f64> {
    let typed = typed.trim();
    let (sign, unsigned) = typed
        .strip_prefix('-')
        .map_or((1.0, typed), |rest| (-1.0, rest));
    let seconds = match unsigned.split_once(':') {
        Some((minutes, seconds)) => {
            let seconds: f64 = seconds.parse().ok()?;
            if !(0.0..60.0).contains(&seconds) {
                return None;
            }
            f64::from(minutes.parse::<u32>().ok()?) * 60.0 + seconds
        }
        None => unsigned.parse().ok()?,
    };
    seconds.is_finite().then_some(sign * seconds)
}

/// What the offset is, for its tooltips.
const OFFSET_ABOUT: &str = "Where beat 0 falls in the audio, in milliseconds. Raise it if the \
    notes come before the music, lower it if they come after.";

/// The offset's rows of the tempo table: a field in milliseconds with steppers, and what it is.
fn offset_rows(
    ui: &mut Ui,
    (editable, caption): (bool, egui::Id),
    seconds: f64,
    draft: &mut Option<String>,
    actions: &mut Vec<Action>,
) -> Option<String> {
    let mut refused = None;
    if editable {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let coarse = ui.input(|input| input.modifiers.shift);
            let by = if coarse { 0.010 } else { 0.001 };
            if stepper(
                ui,
                "−",
                "Lower the offset by 1 ms",
                "Alt+Left, 10 ms with Shift",
            ) {
                actions.push(Action::NudgeOffset(-by));
            }
            let milliseconds = |typed: &str| {
                let offset = typed
                    .parse::<f64>()
                    .ok()
                    .filter(|ms| ms.is_finite())
                    .and_then(|ms| Origin::try_from((ms * 1000.0).round() / 1e6).ok());
                offset
                    .map(Action::SetOffset)
                    .ok_or_else(|| format!("{typed} is not an offset in milliseconds"))
            };
            let shown = milliseconds_of(seconds);
            refused = entry(ui, caption, &shown, draft, milliseconds, actions);
            if stepper(
                ui,
                "+",
                "Raise the offset by 1 ms",
                "Alt+Right, 10 ms with Shift",
            ) {
                actions.push(Action::NudgeOffset(by));
            }
            ui.weak("ms");
        });
    } else {
        ui.label(format!("{} ms", milliseconds_of(seconds)));
    }
    ui.end_row();
    ui.label("");
    ui.weak("Raise it if notes come early")
        .on_hover_text(OFFSET_ABOUT);
    ui.end_row();
    refused
}

/// A small step button showing `symbol`, named `name`, with its key in the tooltip.
fn stepper(ui: &mut Ui, symbol: &str, name: &str, key: &str) -> bool {
    let button = egui::Button::new(symbol).min_size(egui::vec2(20.0, 0.0));
    let response = ui.add(button).on_hover_text(format!("{name} ({key})"));
    let clicked = response.clicked();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    clicked
}

/// `seconds` as milliseconds to the microsecond, without trailing zeros.
fn milliseconds_of(seconds: f64) -> String {
    let text = format!("{:.3}", seconds * 1000.0);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".to_owned()
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moment_reads_as_seconds_or_minutes_and_seconds() {
        assert_eq!(moment("0:26.25"), Some(26.25));
        assert_eq!(moment("1:23.5"), Some(83.5));
        assert_eq!(moment(" 12.5 "), Some(12.5));
        assert_eq!(moment("-0:00.014"), Some(-0.014));
        for bad in ["", "a", "1:", "1:60", "1:-2", "nan"] {
            assert_eq!(moment(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_meter_reads_as_beats_over_a_note() {
        assert_eq!(meter("3/4").map(|m| m.to_string()), Some("3/4".to_owned()));
        assert_eq!(
            meter(" 7 / 8 ").map(|m| m.to_string()),
            Some("7/8".to_owned())
        );
        for bad in ["", "3", "0/4", "3/0", "a/4", "3/4/4"] {
            assert!(meter(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn events_on_one_tick_make_one_point() {
        let value = serde_json::json!({
            "format_version": 1,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "a.ogg" },
            "timing": {
                "anchors": [
                    { "tick": 0, "seconds": 0.0 },
                    { "tick": 960, "seconds": 0.8 }
                ],
                "tempo": 150.0,
                "events": [
                    { "tick": 960, "kind": { "time_signature": [3, 4] } },
                    { "tick": 1920, "kind": { "stop": 0.5 } }
                ]
            }
        });
        let document =
            charter_core::ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        let timing = document.timing();
        let points = points(timing);
        assert_eq!(points.len(), 3);
        assert_eq!(
            points[1].meter.map(|m| m.to_string()),
            Some("3/4".to_owned())
        );
        assert_eq!(points[2].others, ["stop 0.5 s"]);
        assert_eq!(
            tempo_at(timing, Tick::new(1920)).map(|b| b.to_string()),
            Some("150".to_owned())
        );
        assert_eq!(meter_at(&timing, Tick::new(0)).to_string(), "4/4");
    }
}
