//! The toolbar, inspector, issue list and status line of the timing window: the numbers behind
//! what the rows show, each one that can be edited editable in place.

use charter_core::{Bpm, Tick};
use charter_editor::Pin;
use charter_style as style;
use egui::{RichText, Ui};

use crate::accuracy::{self, Issue, Metrics, ON_ONSET_MS};
use crate::grid::{Grid, RESOLUTIONS};
use crate::rows::{BarsPerRow, Layout, SecondsPerRow};
use crate::{Command, Edit, Frame, Gesture, TimingWindow};

/// What is being typed into the inspector, while a field has the keyboard.
#[derive(Debug, Default)]
pub struct Drafts {
    seconds: Option<String>,
    bar: Option<String>,
    bpm: Option<String>,
    /// A field asked to take the keyboard, as a double-click on an anchor asks of its moment.
    pub(crate) focus_seconds: bool,
}

/// The toolbar: the grid, how many bars a row holds, what snaps, and how strong an onset has
/// to be to count.
pub(crate) fn toolbar(ui: &mut Ui, window: &mut TimingWindow) {
    ui.horizontal_wrapped(|ui| {
        let caption = ui.weak("Grid");
        egui::ComboBox::from_id_salt("timing grid")
            .selected_text(format!("1/{}", window.grid.resolution))
            .show_ui(ui, |ui| {
                for (key, resolution) in (1..).zip(RESOLUTIONS) {
                    let shortcut = if key == 10 { 0 } else { key };
                    let text = format!("1/{resolution}   {shortcut}");
                    if let Some(chosen) = std::num::NonZeroU16::new(resolution) {
                        ui.selectable_value(&mut window.grid, Grid { resolution: chosen }, text);
                    }
                }
            })
            .response
            .labelled_by(caption.id);
        ui.weak("Layout");
        ui.selectable_value(&mut window.layout, Layout::Bars, "Bars")
            .on_hover_text("Rows of bars: the grid stays, the audio moves as you edit (V)");
        ui.selectable_value(&mut window.layout, Layout::Time, "Time")
            .on_hover_text("Rows of seconds: the audio stays, the grid moves as you edit (V)");
        let caption = ui.weak("Rows");
        let rows = egui::ComboBox::from_id_salt("timing rows");
        let rows = match window.layout {
            Layout::Bars => rows
                .selected_text(window.bars_per_row.name())
                .show_ui(ui, |ui| {
                    for per in BarsPerRow::ALL {
                        ui.selectable_value(&mut window.bars_per_row, per, per.name());
                    }
                }),
            Layout::Time => rows
                .selected_text(window.seconds_per_row.name())
                .show_ui(ui, |ui| {
                    for per in SecondsPerRow::ALL {
                        ui.selectable_value(&mut window.seconds_per_row, per, per.name());
                    }
                }),
        };
        rows.response.labelled_by(caption.id);
        ui.separator();
        ui.weak("Snap");
        ui.checkbox(&mut window.snap_grid, "Grid");
        ui.checkbox(&mut window.snap_onset, "Onset");
        ui.separator();
        let caption = ui.weak("Onsets");
        let sensitivity = egui::Slider::new(&mut window.sensitivity, 0.0..=1.0)
            .fixed_decimals(2)
            .text("least strength");
        ui.add(sensitivity).labelled_by(caption.id);
        ui.separator();
        ui.checkbox(&mut window.audition, "Audition")
            .on_hover_text("Play from a beat before each anchor added, with the metronome");
        ui.checkbox(&mut window.follow, "Follow")
            .on_hover_text("Keep the playhead on the second row while playing (O)");
    });
}

/// The status line: what the gesture under the pointer does, why the last edit was refused,
/// and how many issues the map has.
pub(crate) fn status(
    ui: &mut Ui,
    window: &TimingWindow,
    frame: &Frame<'_>,
    (hovering, latency_ms): (bool, f64),
) {
    ui.horizontal(|ui| {
        let hint = match (&window.gesture, window.preview.is_some()) {
            (_, true) => "Enter: apply · Esc: discard",
            (Gesture::DragBeat { .. }, _) => "Moving the beat · Shift: fine · Esc: cancel",
            (Gesture::DragAudio { .. }, _) => {
                "Moving in the audio · Shift: no snap, fine · Esc: cancel"
            }
            (Gesture::Band { .. }, _) => "Selecting · Esc: cancel",
            _ if hovering => {
                "Drag: move beat · Ctrl-drag: move audio · Double-click: edit · Del: remove"
            }
            _ => "Click: add anchor · Drag: select · Right-click: menu · ?: keys",
        };
        // Text that could grow the window as gestures change stays on one line, cut short.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let count = frame.issues.len();
            let issues = match count {
                0 => "No issues".to_owned(),
                1 => "1 issue ⚠".to_owned(),
                count => format!("{count} issues ⚠"),
            };
            if count > 0 {
                ui.colored_label(style::WARN, issues);
            } else {
                ui.weak(issues);
            }
            ui.weak(format!("🎧 {latency_ms:+.0} ms"))
                .on_hover_text("The calibrated latency; Settings > Sounds calibrates it");
            ui.separator();
            if let Some(why) = &window.status {
                ui.separator();
                ui.add(egui::Label::new(RichText::new(why).color(style::WARN)).truncate());
            }
            ui.separator();
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(RichText::new(hint).color(style::WEAK_TEXT)).truncate());
            });
        });
    });
}

/// The inspector: the focused anchor's numbers, its segment's fit, the selection, the fit on
/// preview, and the issue list.
pub(crate) fn inspector(
    ui: &mut Ui,
    window: &mut TimingWindow,
    frame: &Frame<'_>,
    commands: &mut Vec<Command>,
) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        if let Some((fitted, about)) = window.preview.clone() {
            ui.label(RichText::new("FIT PREVIEW").small().strong());
            ui.label(about);
            ui.label(format!(
                "error {:.1} → {:.1} ms",
                fitted.before_ms, fitted.after_ms
            ));
            ui.horizontal(|ui| {
                if ui.add(style::primary("Apply")).clicked() {
                    commands.push(Command::Edit(Edit::Repin {
                        pins: fitted.pins.clone(),
                        tail: fitted.tail,
                    }));
                    window.preview = None;
                }
                if ui.button("Discard").clicked() {
                    window.preview = None;
                }
            });
            ui.separator();
        }
        let index = window
            .focus
            .and_then(|tick| frame.pins.iter().position(|pin| pin.tick == tick));
        if let Some(index) = index {
            anchor(ui, window, frame, index, commands);
        } else {
            ui.label(RichText::new("ANCHOR").small().strong());
            ui.weak("Click an anchor to see its numbers, or J and K to step through them.");
        }
        if window.selection.len() > 1 {
            ui.separator();
            selection(ui, window, frame);
        }
        ui.separator();
        issues(ui, window, frame);
    });
}

#[allow(clippy::too_many_lines)] // one field after another, each typed into in place
fn anchor(
    ui: &mut Ui,
    window: &mut TimingWindow,
    frame: &Frame<'_>,
    index: usize,
    commands: &mut Vec<Command>,
) {
    let Some(&pin) = frame.pins.get(index) else {
        return;
    };
    let next = frame.pins.get(index + 1).copied();
    let editable = frame.editable;
    ui.label(
        RichText::new(format!("ANCHOR {}", index + 1))
            .small()
            .strong(),
    );
    let bpm = derived(frame, pin);
    let delta = accuracy::nearest_onset(frame.onsets, window.sensitivity, pin.seconds)
        .map(|onset| (pin.seconds - onset) * 1000.0);
    compact(ui, "anchor", |ui| {
        let caption = ui.weak("t");
        let text = format!("{:.4}", pin.seconds);
        if editable {
            let field = field(
                ui,
                caption.id,
                &text,
                &mut window.drafts.seconds,
                window.drafts.focus_seconds,
            );
            window.drafts.focus_seconds = false;
            if let Some(typed) = field {
                match typed.parse::<f64>() {
                    Ok(seconds) if seconds.is_finite() => {
                        commands.push(Command::Edit(Edit::Sync {
                            tick: pin.tick,
                            seconds,
                        }));
                    }
                    _ => window.status = Some(format!("{typed} is not a time in seconds")),
                }
            }
        } else {
            ui.label(text);
        }
        ui.weak("s");
        ui.end_row();
        let caption = ui.weak("b");
        #[allow(clippy::cast_precision_loss)]
        let bar = frame.bars.bar_of(i64::from(pin.tick) as f64);
        let text = format!("{bar:.3}");
        if editable {
            if let Some(typed) = field(ui, caption.id, &text, &mut window.drafts.bar, false) {
                match typed.parse::<f64>() {
                    Ok(bar) if bar.is_finite() => {
                        let tick = frame.bars.tick_at(bar).round();
                        #[allow(clippy::cast_possible_truncation)] // clamped to a document's ticks
                        let to =
                            Tick::new(tick.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32);
                        commands.push(Command::Edit(Edit::MoveAnchors(vec![(pin.tick, to)])));
                    }
                    _ => window.status = Some(format!("{typed} is not a bar position")),
                }
            }
        } else {
            ui.label(text);
        }
        ui.weak(frame.timing.grid_position(pin.tick).to_string());
        ui.end_row();
        let caption = ui.weak("BPM");
        let text = trimmed(f64::from(bpm), 3);
        if editable {
            if let Some(typed) = field(ui, caption.id, &text, &mut window.drafts.bpm, false) {
                match typed
                    .parse::<f64>()
                    .ok()
                    .and_then(|bpm| Bpm::try_from(bpm).ok())
                {
                    Some(bpm) => commands.push(Command::Edit(match next {
                        Some(_) => Edit::SetBpmAt {
                            tick: pin.tick,
                            bpm,
                        },
                        None => Edit::TailBpm { bpm, merge: false },
                    })),
                    None => window.status = Some(format!("{typed} is not a tempo")),
                }
            }
        } else {
            ui.label(text);
        }
        ui.weak(if next.is_some() {
            "to the next"
        } else {
            "tail"
        });
        ui.end_row();
        ui.weak("Δ");
        match delta {
            Some(ms) => {
                let mark = if ms.abs() < ON_ONSET_MS { " ✓" } else { "" };
                ui.label(format!("{ms:+.1} ms{mark}"));
            }
            None => {
                ui.weak("no onset near");
            }
        }
        ui.label("");
        ui.end_row();
    });
    ui.add_space(6.0);
    let segment = next.map_or_else(
        || format!("SEGMENT {} → tail", index + 1),
        |_| format!("SEGMENT {} → {}", index + 1, index + 2),
    );
    ui.label(RichText::new(segment).small().strong());
    let metrics = accuracy::metrics(
        frame.timing,
        window.grid,
        frame.residuals,
        (pin.tick, next.map(|next| next.tick)),
    );
    metrics_rows(ui, "segment", &metrics);
    if editable
        && ui
            .button("Fit tempo  F")
            .on_hover_text("Fit this anchor, and the tail tempo after the last, to the onsets")
            .clicked()
    {
        window.fit(frame, false);
    }
}

fn selection(ui: &mut Ui, window: &TimingWindow, frame: &Frame<'_>) {
    let (Some(&first), Some(&last)) = (window.selection.first(), window.selection.last()) else {
        return;
    };
    ui.label(
        RichText::new(format!("SELECTION · {} anchors", window.selection.len()))
            .small()
            .strong(),
    );
    let chosen: Vec<Pin> = frame
        .pins
        .iter()
        .filter(|pin| pin.tick >= first && pin.tick <= last)
        .copied()
        .collect();
    let tempos: Vec<f64> = chosen
        .windows(2)
        .filter_map(|pair| match pair {
            [a, _] => Some(f64::from(derived(frame, *a))),
            _ => None,
        })
        .collect();
    if let (Some(low), Some(high)) = (
        tempos.iter().copied().reduce(f64::min),
        tempos.iter().copied().reduce(f64::max),
    ) {
        #[allow(clippy::cast_precision_loss)]
        let mean = tempos.iter().sum::<f64>() / tempos.len() as f64;
        ui.label(format!("BPM {low:.2} · {mean:.2} · {high:.2}"));
    }
    let metrics = accuracy::metrics(
        frame.timing,
        window.grid,
        frame.residuals,
        (first, Some(last)),
    );
    metrics_rows(ui, "selection", &metrics);
}

fn metrics_rows(ui: &mut Ui, id: &str, metrics: &Metrics) {
    compact(ui, id, |ui| {
        ui.weak("fit error");
        ui.label(
            metrics
                .error_ms
                .map_or_else(|| "–".to_owned(), |ms| format!("{ms:.1} ms")),
        );
        ui.end_row();
        ui.weak("coverage");
        ui.label(metrics.coverage.map_or_else(
            || "–".to_owned(),
            |share| format!("{:.0}% of lines", share * 100.0),
        ));
        ui.end_row();
    });
}

fn issues(ui: &mut Ui, window: &mut TimingWindow, frame: &Frame<'_>) {
    ui.label(
        RichText::new(format!("ISSUES · {}", frame.issues.len()))
            .small()
            .strong(),
    );
    if frame.onsets.is_empty() {
        ui.weak("Onsets show once the song's audio is analysed.");
    }
    if frame.issues.is_empty() {
        ui.weak("Nothing to look at.");
        return;
    }
    for (at, issue) in frame.issues.iter().enumerate() {
        let text = issue.describe(frame.timing);
        let chosen = window.issue == Some(at);
        if ui.selectable_label(chosen, text).clicked() {
            window.go_to_issue(frame, at);
        }
    }
    ui.weak("N and Shift+N step through them.");
}

/// A grid of short columns, narrow enough for the inspector's width.
fn compact(ui: &mut Ui, id: &str, rows: impl FnOnce(&mut Ui)) {
    egui::Grid::new(id)
        .num_columns(3)
        .min_col_width(20.0)
        .spacing([8.0, 5.0])
        .show(ui, rows);
}

/// A one-line field showing `current`, kept in `draft` while it has the keyboard, and what was
/// typed once it is entered or left; Escape leaves it as it was.
fn field(
    ui: &mut Ui,
    caption: egui::Id,
    current: &str,
    draft: &mut Option<String>,
    take_focus: bool,
) -> Option<String> {
    let mut text = draft.take().unwrap_or_else(|| current.to_owned());
    let response = ui
        .add(egui::TextEdit::singleline(&mut text).desired_width(84.0))
        .labelled_by(caption);
    if take_focus {
        response.request_focus();
    }
    let left = response.lost_focus()
        && !ui.input(|input| input.key_pressed(egui::Key::Escape))
        && text.trim() != current;
    if response.has_focus() {
        *draft = Some(text);
        None
    } else if left {
        Some(text.trim().to_owned())
    } else {
        None
    }
}

/// The tempo of the segment from `from`, as the song holds it: stops and delays in the segment
/// take time the tempo does not.
fn derived(frame: &Frame<'_>, from: Pin) -> Bpm {
    Bpm::nearest(frame.timing.tempo_at(from.tick)).unwrap_or(frame.tail)
}

/// `value` to `places`, without trailing zeros.
fn trimmed(value: f64, places: usize) -> String {
    let text = format!("{value:.places$}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Something the issue list names, for the window to go to.
pub(crate) fn issue_tick(issue: &Issue) -> Tick {
    issue.tick()
}

#[cfg(test)]
mod tests {
    use charter_core::{RulesetPlugin, ValidDocument};
    use serde_json::json;

    use super::*;

    #[derive(Debug)]
    struct AcceptsTiming;

    impl RulesetPlugin for AcceptsTiming {
        fn id(&self) -> &'static str {
            "test"
        }

        fn check(&self, _: &ValidDocument) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[test]
    fn editing_segment_bpm_accounts_for_a_stop() {
        let value = json!({
            "format_version": 1,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "a.ogg" },
            "timing": {
                "anchors": [
                    { "tick": 0, "seconds": 0.0 },
                    { "tick": 3840, "seconds": 2.5 }
                ],
                "tempo": 120.0,
                "events": [{ "tick": 1920, "kind": { "stop": 0.5 } }]
            },
            "charts": [{ "name": "Easy", "lanes": 4 }]
        });
        let document = ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        let mut song = crate::Song::new(document, &[&AcceptsTiming]);
        let mut window = TimingWindow::default();
        window.drafts.focus_seconds = true;
        let cache = crate::refreshed(None, &song, &[], &window);
        let frame = crate::frame(&song, &[], (&cache, crate::rows::Layout::Bars), 0.0);
        let ctx = egui::Context::default();
        let mut commands = Vec::new();
        let show = |events, window: &mut TimingWindow, commands: &mut Vec<Command>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500.0, 500.0),
                )),
                events,
                ..egui::RawInput::default()
            };
            let mut output = ctx.run_ui(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    anchor(ui, window, &frame, 0, commands);
                });
            });
            output.textures_delta.clear();
        };
        let tab = || egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        show(Vec::new(), &mut window, &mut commands);
        show(vec![tab()], &mut window, &mut commands);
        show(vec![tab()], &mut window, &mut commands);
        window.drafts.bpm = Some("150".to_owned());
        show(vec![tab()], &mut window, &mut commands);

        assert_eq!(commands.len(), 1, "{commands:?}");
        let crate::Command::Edit(edit) = commands.remove(0) else {
            panic!("expected a tempo edit");
        };
        edit.apply(&mut song).unwrap();
        assert!((song.timing().tempo_at(Tick::new(0)) - 150.0).abs() < 1e-4);
    }
}
