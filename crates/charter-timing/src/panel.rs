//! The toolbar, inspector, issue list and status line of the timing window: the numbers behind
//! what the rows show, each one that can be edited editable in place.

use charter_core::{Bpm, ResolvedTime, Tick};
use charter_style as style;
use egui::{RichText, Ui};

use crate::accuracy::{self, Issue, Metrics, ON_ONSET_MS, Rounding};
use crate::grid::{Grid, RESOLUTIONS};
use crate::rows::{BarsPerRow, Layout, SecondsPerRow};
use crate::{Command, Edit, Frame, Gesture, TimingWindow};

/// What is being typed into the inspector, while a field has the keyboard.
#[derive(Debug, Default)]
pub struct Drafts {
    /// Uncommitted audio moment in seconds.
    seconds: Option<String>,
    /// Uncommitted bar position typed for an anchor.
    bar: Option<String>,
    /// Uncommitted tempo typed for an anchor.
    bpm: Option<String>,
    /// Uncommitted tempo typed for the first anchor.
    start_bpm: Option<String>,
    /// A field asked to take the keyboard, as a double-click on an anchor asks of its moment.
    pub(crate) focus_seconds: bool,
}

/// The tempo from the first anchor on, set from wherever the window is: the one tempo a map
/// is most often given before it is fitted anchor by anchor.
fn start_bpm(
    ui: &mut Ui,
    window: &mut TimingWindow,
    frame: &Frame<'_>,
    commands: &mut Vec<Command>,
) {
    let Some(first) = frame.pins.first() else {
        return;
    };
    let caption = ui
        .weak("Start BPM")
        .on_hover_text("The tempo from the first anchor on, until the next");
    let bpm = frame.timing.bpm_at(first.tick).unwrap_or(frame.tail);
    let text = style::trimmed_decimal(f64::from(bpm), 3);
    if !frame.editable {
        ui.label(text);
        return;
    }
    let Some(typed) = style::text_field(ui, caption.id, &text, &mut window.drafts.start_bpm, false)
    else {
        return;
    };
    match typed
        .parse::<f64>()
        .ok()
        .and_then(|parsed| Bpm::try_from(parsed).ok())
    {
        Some(tempo) if frame.pins.len() > 1 => commands.push(Command::Edit(Edit::SetBpmAt {
            tick: first.tick,
            bpm: tempo,
        })),
        Some(tempo) => commands.push(Command::Edit(Edit::TailBpm {
            bpm: tempo,
            merge: false,
        })),
        None => window.status = Some(format!("{typed} is not a tempo")),
    }
}

/// The toolbar: the grid, how many bars a row holds, what snaps, and how strong an onset has
/// to be to count.
pub(crate) fn toolbar(
    ui: &mut Ui,
    window: &mut TimingWindow,
    frame: &Frame<'_>,
    commands: &mut Vec<Command>,
) {
    ui.horizontal_wrapped(|ui| {
        start_bpm(ui, window, frame, commands);
        ui.separator();
        let caption = ui.weak("Grid");
        egui::ComboBox::from_id_salt("timing grid")
            .selected_text(format!("1/{}", window.grid.resolution.get()))
            .show_ui(ui, |ui| {
                for (key, resolution) in (1..).zip(RESOLUTIONS) {
                    let shortcut = if key == 10 { 0 } else { key };
                    let text = format!("1/{}   {shortcut}", resolution.get());
                    ui.selectable_value(&mut window.grid, Grid { resolution }, text);
                }
            })
            .response
            .labelled_by(caption.id);
        ui.weak("Layout");
        ui.selectable_value(&mut window.layout, Layout::Bars, "Bars")
            .on_hover_text("Rows of bars: the grid stays, the audio moves as you edit (V)");
        ui.selectable_value(&mut window.layout, Layout::Time, "Time")
            .on_hover_text("Rows of seconds: the audio stays, the grid moves as you edit (V)");
        let rows_caption = ui.weak("Rows");
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
        rows.response.labelled_by(rows_caption.id);
        ui.separator();
        ui.weak("Snap");
        style::check(ui, &mut window.snap_grid, "Grid");
        style::check(ui, &mut window.snap_onset, "Onset");
    });
    ui.horizontal(|ui| {
        let caption = ui.weak("Onsets");
        let sensitivity = egui::Slider::new(&mut window.sensitivity, 0.0..=1.0)
            .fixed_decimals(2)
            .text("least strength");
        ui.add(sensitivity).labelled_by(caption.id);
        ui.separator();
        style::check(ui, &mut window.audition, "Audition")
            .on_hover_text("Play from a beat before each anchor added, with the metronome");
        style::check(ui, &mut window.follow, "Follow")
            .on_hover_text("Keep the playhead on the second row while playing (O)");
    });
}

/// The status line: what the gesture under the pointer does, why the last edit was refused,
/// and how many issues the map has.
#[expect(
    clippy::non_ascii_literal,
    reason = "timing status labels use middle dots and a visible warning sign"
)]
pub(crate) fn status(
    ui: &mut Ui,
    window: &TimingWindow,
    frame: &Frame<'_>,
    (hovering, latency_ms): (bool, f64),
) {
    ui.horizontal(|ui| {
        let hint = match (&window.gesture, window.preview.is_some()) {
            (_, true) if !frame.editable => "Esc: discard",
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
                many => format!("{many} issues ⚠"),
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
        if let Some((fitted, about)) = &window.preview {
            ui.label(RichText::new("FIT PREVIEW").small().strong());
            ui.label(about);
            ui.label(format!(
                "error {:.1} → {:.1} ms",
                fitted.before_ms, fitted.after_ms
            ));
            let (apply, discard) = ui
                .horizontal(|ui| {
                    let apply = frame.editable && ui.add(style::primary("Apply")).clicked();
                    let discard = ui.button("Discard").clicked();
                    (apply, discard)
                })
                .inner;
            if apply && let Some((accepted, _)) = window.preview.take() {
                commands.push(Command::Edit(Edit::Repin {
                    pins: accepted.pins,
                    tail: accepted.tail,
                }));
            }
            if discard {
                window.preview = None;
            }
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

#[expect(
    clippy::too_many_lines,
    reason = "one field after another, typed in place"
)]
/// Draws the inspector for the anchor at `index`, with editable timing fields.
#[expect(
    clippy::non_ascii_literal,
    reason = "the timing inspector marks onset accuracy with delta and check symbols"
)]
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
    let next = frame.pins.get(index.saturating_add(1)).copied();
    let editable = frame.editable;
    ui.label(
        RichText::new(format!("ANCHOR {}", index.saturating_add(1)))
            .small()
            .strong(),
    );
    let bpm = frame.bpm_at(pin.tick);
    let delta = accuracy::nearest_onset(frame.onsets, window.sensitivity, pin.seconds.seconds())
        .map(|onset| (onset - pin.seconds.seconds()) * 1000.0);
    compact(ui, "anchor", |ui| {
        let caption = ui.weak("t");
        let text = format!("{:.4}", pin.seconds.seconds());
        if editable {
            let field = style::text_field(
                ui,
                caption.id,
                &text,
                &mut window.drafts.seconds,
                window.drafts.focus_seconds,
            );
            window.drafts.focus_seconds = false;
            if let Some(typed) = field {
                match typed
                    .parse::<f64>()
                    .ok()
                    .and_then(ResolvedTime::from_seconds)
                {
                    Some(seconds) => {
                        commands.push(Command::Edit(Edit::Sync {
                            tick: pin.tick,
                            seconds,
                        }));
                    }
                    None => window.status = Some(format!("{typed} is not a time in seconds")),
                }
            }
        } else {
            ui.label(text);
        }
        ui.weak("s");
        ui.end_row();
        let bar_caption = ui.weak("b");
        let bar = frame
            .bars
            .bar_of(crate::rows::float_from_i64(i64::from(pin.tick)));
        let bar_text = format!("{bar:.3}");
        if editable {
            if let Some(typed) =
                style::text_field(ui, bar_caption.id, &bar_text, &mut window.drafts.bar, false)
            {
                match typed.parse::<f64>() {
                    Ok(bar_position) if bar_position.is_finite() => {
                        let tick = frame.bars.tick_at(bar_position).round();
                        let to = Tick::new(crate::rows::float_to_i32(tick));
                        commands.push(Command::Edit(Edit::MoveAnchors(vec![(pin.tick, to)])));
                    }
                    _ => window.status = Some(format!("{typed} is not a bar position")),
                }
            }
        } else {
            ui.label(bar_text);
        }
        ui.weak(frame.timing.grid_position(pin.tick).to_string());
        ui.end_row();
        let tempo_caption = ui.weak("BPM");
        let tempo_text = style::trimmed_decimal(f64::from(bpm), 3);
        if editable {
            if let Some(typed) = style::text_field(
                ui,
                tempo_caption.id,
                &tempo_text,
                &mut window.drafts.bpm,
                false,
            ) {
                match typed
                    .parse::<f64>()
                    .ok()
                    .and_then(|parsed| Bpm::try_from(parsed).ok())
                {
                    Some(tempo) => commands.push(Command::Edit(match next {
                        Some(_) => Edit::SetBpmAt {
                            tick: pin.tick,
                            bpm: tempo,
                        },
                        None => Edit::TailBpm {
                            bpm: tempo,
                            merge: false,
                        },
                    })),
                    None => window.status = Some(format!("{typed} is not a tempo")),
                }
            }
        } else {
            ui.label(tempo_text);
        }
        ui.weak(if next.is_some() {
            "to the next"
        } else {
            "tail"
        });
        ui.end_row();
        ui.weak("Δ")
            .on_hover_text("Onset minus anchor: negative means early, positive means late");
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
        || format!("SEGMENT {} → tail", index.saturating_add(1)),
        |_| {
            format!(
                "SEGMENT {} → {}",
                index.saturating_add(1),
                index.saturating_add(2)
            )
        },
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
        window.fit(frame, Rounding::Unrounded);
    }
}

/// Displays the selected anchors' tempo range and audio-fit measurements.
fn selection(ui: &mut Ui, window: &TimingWindow, frame: &Frame<'_>) {
    let (Some(&first), Some(&last)) = (window.selection.first(), window.selection.last()) else {
        return;
    };
    ui.label(
        RichText::new(format!("SELECTION · {} anchors", window.selection.len()))
            .small()
            .strong(),
    );
    let chosen = frame
        .pins
        .get(
            frame.pins.partition_point(|pin| pin.tick < first)
                ..frame.pins.partition_point(|pin| pin.tick <= last),
        )
        .unwrap_or_default();
    let tempos: Vec<f64> = chosen
        .windows(2)
        .filter_map(|pair| match pair {
            [a, _] => Some(f64::from(frame.bpm_at(a.tick))),
            _ => None,
        })
        .collect();
    if let (Some(low), Some(high)) = (
        tempos.iter().copied().reduce(f64::min),
        tempos.iter().copied().reduce(f64::max),
    ) {
        let mean = tempos.iter().sum::<f64>() / crate::rows::double_from_usize(tempos.len());
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

/// Displays fit error and grid coverage for `metrics` in a compact grid.
#[expect(
    clippy::non_ascii_literal,
    reason = "the timing inspector shows an en dash for unavailable metrics"
)]
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

/// Lists timing issues and lets a click focus the affected anchor.
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

/// Something the issue list names, for the window to go to.
pub(crate) const fn issue_tick(issue: &Issue) -> Tick {
    issue.tick()
}

#[cfg(test)]
mod tests {
    use charter_core::{RulesetPlugin, ValidDocument};
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use serde_json::json;

    use super::*;
    use crate::test_support::{WindowTest, timing_harness};

    fn type_field(harness: &mut Harness<'_, WindowTest>, label: &str, text: &str) {
        harness
            .get_by_role_and_label(Role::TextInput, label)
            .focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness
            .get_by_role_and_label(Role::TextInput, label)
            .type_text(text);
        harness.run();
        harness.key_press(egui::Key::Tab);
        harness.run();
    }

    #[test]
    fn inspector_seconds_accept_finite_values_and_reject_invalid_values() {
        let mut harness = timing_harness();
        harness.state_mut().window.focus = Some(Tick::new(960));
        harness.run();
        type_field(&mut harness, "t", "0.625");
        assert_eq!(
            harness.state().commands,
            [Command::Edit(Edit::Sync {
                tick: Tick::new(960),
                seconds: ResolvedTime::from_seconds(0.625).unwrap()
            })]
        );
        harness.state_mut().commands.clear();
        for invalid in ["not a time", "NaN", "inf"] {
            type_field(&mut harness, "t", invalid);
            assert_eq!(harness.state().commands, []);
            assert!(
                harness
                    .query_by_label_contains("is not a time in seconds")
                    .is_some()
            );
        }
    }

    #[test]
    fn inspector_bar_clamps_out_of_range_positions_and_rejects_nonfinite_input() {
        let mut harness = timing_harness();
        harness.state_mut().window.focus = Some(Tick::new(960));
        harness.run();
        type_field(&mut harness, "b", "1.5");
        assert_eq!(
            harness.state().commands,
            [Command::Edit(Edit::MoveAnchors(vec![(
                Tick::new(960),
                Tick::new(5760)
            )]))]
        );
        harness.state_mut().commands.clear();
        type_field(&mut harness, "b", "1e100");
        assert_eq!(
            harness.state().commands,
            [Command::Edit(Edit::MoveAnchors(vec![(
                Tick::new(960),
                Tick::new(i32::MAX)
            )]))]
        );
        harness.state_mut().commands.clear();
        type_field(&mut harness, "b", "NaN");
        assert_eq!(harness.state().commands, []);
        assert!(
            harness
                .query_by_label_contains("is not a bar position")
                .is_some()
        );
    }

    #[test]
    fn inspector_bpm_edits_a_segment_or_the_tail_and_rejects_invalid_tempo() {
        let mut harness = timing_harness();
        harness.state_mut().window.focus = Some(Tick::new(960));
        harness.run();
        type_field(&mut harness, "BPM", "150");
        assert_eq!(
            harness.state().commands,
            [Command::Edit(Edit::SetBpmAt {
                tick: Tick::new(960),
                bpm: Bpm::try_from(150.0).unwrap()
            })]
        );
        harness.state_mut().commands.clear();
        type_field(&mut harness, "BPM", "0");
        assert_eq!(harness.state().commands, []);
        assert!(harness.query_by_label_contains("is not a tempo").is_some());
        harness.state_mut().window.focus = Some(Tick::new(1920));
        harness.run();
        type_field(&mut harness, "BPM", "90");
        assert_eq!(
            harness.state().commands,
            [Command::Edit(Edit::TailBpm {
                bpm: Bpm::try_from(90.0).unwrap(),
                merge: false
            })]
        );
    }

    #[test]
    fn inspector_delta_is_negative_for_early_onsets_and_positive_for_late_onsets() {
        let mut harness = timing_harness();
        harness.state_mut().window.focus = Some(Tick::new(960));
        harness.state_mut().onsets = vec![crate::Onset {
            seconds: 0.488,
            strength: 1.0,
        }];
        harness.run();
        assert!(harness.query_by_label("-12.0 ms").is_some());

        harness.state_mut().onsets = vec![crate::Onset {
            seconds: 0.512,
            strength: 1.0,
        }];
        harness.run();
        assert!(harness.query_by_label("+12.0 ms").is_some());
    }

    #[test]
    fn start_bpm_edits_the_first_segment_and_reports_invalid_input() {
        let mut harness = timing_harness();
        type_field(&mut harness, "Start BPM", "140");
        assert_eq!(
            harness.state().commands,
            [Command::Edit(Edit::SetBpmAt {
                tick: Tick::new(0),
                bpm: Bpm::try_from(140.0).unwrap()
            })]
        );
        harness.state_mut().commands.clear();
        type_field(&mut harness, "Start BPM", "999999999999999999");
        assert_eq!(harness.state().commands, []);
        assert!(harness.query_by_label_contains("is not a tempo").is_some());
    }

    #[test]
    fn a_song_without_its_plugin_cannot_edit_start_bpm_or_apply_a_fit() {
        let mut harness = timing_harness();
        let document = harness.state().song.document().clone();
        harness.state_mut().song = crate::Song::new(document, &[]);
        harness.run();
        assert!(harness.state().song.read_only().is_some());
        assert!(
            harness
                .query_by_role_and_label(Role::TextInput, "Start BPM")
                .is_none()
        );
        harness.state_mut().window.focus = Some(Tick::new(960));
        harness.run();
        assert!(
            harness
                .query_by_role_and_label(Role::TextInput, "t")
                .is_none()
        );
        assert!(
            harness
                .query_by_role_and_label(Role::TextInput, "b")
                .is_none()
        );
        assert!(
            harness
                .query_by_role_and_label(Role::TextInput, "BPM")
                .is_none()
        );
        assert!(harness.query_by_label("Fit tempo  F").is_none());
        assert_eq!(harness.state().commands, []);
    }

    #[test]
    fn toolbar_switches_layout_rows_grid_and_snap_controls() {
        let mut harness = timing_harness();
        harness.get_by_label("Time").click();
        harness.run();
        assert_eq!(harness.state().window.layout, Layout::Time);
        harness.get_by_label("Rows").click();
        harness.run();
        let rows = SecondsPerRow::ALL.last().copied().unwrap();
        harness.get_by_label(rows.name()).click();
        harness.run();
        assert_eq!(harness.state().window.seconds_per_row, rows);
        harness
            .get_by_role_and_label(Role::ComboBox, "Grid")
            .click();
        harness.run();
        harness.get_by_label("1/8   6").click();
        harness.run();
        assert_eq!(harness.state().window.grid.resolution.get(), 8);
        harness.get_by_label("Onset").click();
        harness.get_by_label("Audition").click();
        harness.get_by_label("Follow").click();
        harness.run();
        assert!(!harness.state().window.snap_onset);
        assert!(!harness.state().window.audition);
        assert!(harness.state().window.follow);
    }

    fn preview(harness: &mut Harness<'_, WindowTest>) {
        let pins = harness.state().song.anchors();
        harness.state_mut().window.preview = Some((
            crate::accuracy::Fitted {
                pins,
                tail: Bpm::try_from(135.0).unwrap(),
                before_ms: 10.0,
                after_ms: 2.0,
            },
            "3 anchors fitted".to_owned(),
        ));
        harness.run();
    }

    #[test]
    #[expect(
        clippy::non_ascii_literal,
        reason = "the test matches the inspector's right-arrow error label"
    )]
    fn fit_preview_can_be_discarded_or_applied_once_from_the_inspector() {
        let mut harness = timing_harness();
        preview(&mut harness);
        assert!(
            harness
                .query_by_label_contains("error 10.0 → 2.0 ms")
                .is_some()
        );
        assert!(harness.query_by_label_contains("Enter: apply").is_some());
        harness.get_by_label("Discard").click();
        harness.run();
        assert_eq!(harness.state().window.preview, None);
        assert_eq!(harness.state().commands, []);
        preview(&mut harness);
        harness.get_by_label("Apply").click();
        harness.run();
        assert_eq!(harness.state().window.preview, None);
        assert_eq!(
            harness.state().commands,
            [Command::Edit(Edit::Repin {
                pins: harness.state().song.anchors(),
                tail: Bpm::try_from(135.0).unwrap()
            })]
        );
    }

    #[test]
    fn read_only_fit_preview_does_not_offer_an_apply_button() {
        let mut harness = timing_harness();
        preview(&mut harness);
        let document = harness.state().song.document().clone();
        harness.state_mut().song = crate::Song::new(document, &[]);
        harness.run();
        assert!(harness.query_by_label("Apply").is_none());
        assert!(harness.query_by_label("Discard").is_some());
        assert!(harness.query_by_label_contains("Enter: apply").is_none());
        assert_eq!(harness.state().commands, []);
    }

    #[test]
    fn status_line_shows_the_refusal_and_changes_its_hint_during_a_drag() {
        let mut harness = timing_harness();
        assert!(harness.query_by_label("No issues").is_some());
        assert!(
            harness
                .query_by_label_contains("Click: add anchor")
                .is_some()
        );
        harness.state_mut().window.status = Some("Would cross the next anchor".to_owned());
        harness.state_mut().window.gesture = Gesture::DragAudio {
            tick: Tick::new(960),
            from: 0.5,
            to: 0.6,
            origin: 0.5,
        };
        harness.run();
        assert!(
            harness
                .query_by_label("Would cross the next anchor")
                .is_some()
        );
        assert!(
            harness
                .query_by_label_contains("Moving in the audio")
                .is_some()
        );
    }

    #[test]
    fn clicking_an_issue_in_the_inspector_selects_its_anchor() {
        let mut harness = timing_harness();
        harness.state_mut().onsets = vec![crate::Onset {
            seconds: 0.512,
            strength: 1.0,
        }];
        harness.run();
        assert!(
            harness
                .state()
                .window
                .cache
                .as_ref()
                .is_some_and(|cache| !cache.issues.is_empty())
        );
        harness
            .get_by_label("The anchor at 00:00.500 is 12.0 ms from its onset")
            .click();
        harness.run();
        assert_eq!(harness.state().window.focus, Some(Tick::new(960)));
        assert_eq!(harness.state().window.selection, [Tick::new(960)]);
    }

    #[derive(Debug)]
    struct AcceptsTiming;

    #[expect(
        clippy::missing_trait_methods,
        reason = "a test plugin: the trait's defaults for every optional method"
    )]
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
        let show = |events, draft_window: &mut TimingWindow, emitted: &mut Vec<Command>| {
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
                    anchor(ui, draft_window, &frame, 0, emitted);
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
        assert!((song.timing().tempo_at(Tick::new(0)).bpm() - 150.0).abs() < 1e-4);
    }
}
