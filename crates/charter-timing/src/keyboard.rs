//! Keyboard shortcuts for the timing window.

use egui::{Key, Modifiers};

use crate::pointer::Gesture;

use super::{
    Adjusted, Bpm, Command, Edit, Frame, Grid, Layout, RESOLUTIONS, ROW, Rounding, Scale, Tick,
    TimingWindow, audition_from, style,
};

/// What a key asks of the timing window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Intent {
    /// Toggle playback at the current position.
    Play,
    /// Accept the preview currently offered.
    Enter,
    /// Dismiss the preview or current interaction.
    Escape,
    /// Pin an anchor to the audio under the playhead.
    AddAtPlayhead,
    /// Focus the anchor before, or after.
    Focus(bool),
    /// Go to the issue before, or after.
    Issue(bool),
    /// Nudge the targets this many seconds.
    Nudge(f64),
    /// Move the targets this many grid steps.
    Move(i32),
    /// Change the tail tempo by this many BPM.
    Tail(f64),
    /// Use the grid resolution at this place among [`RESOLUTIONS`].
    Grid(usize),
    /// Preview fitting the focused or selected anchors to the onsets.
    Fit {
        /// How to round fitted tempos.
        rounding: Rounding,
    },
    /// Double or halve the tempo at the selected anchors.
    Scale {
        /// Whether to double or halve.
        scale: Scale,
    },
    /// Toggle whether rows scroll to keep the playhead visible.
    Follow,
    /// Switch the rows between the bar layout and the time layout.
    Layout,
    /// Remove selected anchors.
    Delete,
    /// Select every anchor.
    SelectAll,
    /// Toggle the keyboard shortcut reference.
    Help,
    /// Loop the bar under the cursor, or the selection.
    Loop {
        /// Loop the selected range instead of the cursor's bar.
        selection: bool,
    },
    /// Switch playback speed between the usual and alternate rate.
    ToggleRate,
    /// Toggle click-track playback.
    ToggleMetronome,
    /// Mute the click track while the key is held.
    Mute,
}

/// What `key` pressed with `modifiers` asks of the timing window, if anything.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "other keys only map if in the numeric shortcut list"
)]
pub(crate) fn intent(key: Key, modifiers: Modifiers) -> Option<Intent> {
    let (shift, ctrl) = (modifiers.shift, modifiers.command || modifiers.ctrl);
    let nudge = if ctrl {
        0.010
    } else if shift {
        0.0001
    } else {
        0.001
    };
    Some(match key {
        Key::Space => Intent::Play,
        Key::Enter => Intent::Enter,
        Key::Escape => Intent::Escape,
        Key::A if ctrl => Intent::SelectAll,
        Key::A => Intent::AddAtPlayhead,
        Key::J => Intent::Focus(false),
        Key::K => Intent::Focus(true),
        Key::N => Intent::Issue(!shift),
        Key::ArrowLeft => Intent::Nudge(-nudge),
        Key::ArrowRight => Intent::Nudge(nudge),
        Key::ArrowUp => Intent::Move(-1),
        Key::ArrowDown => Intent::Move(1),
        Key::Plus | Key::Equals => Intent::Tail(if shift { 1.0 } else { 0.1 }),
        Key::Minus => Intent::Tail(if shift { -1.0 } else { -0.1 }),
        Key::F => Intent::Fit {
            rounding: if shift {
                Rounding::HalfBpm
            } else {
                Rounding::Unrounded
            },
        },
        Key::D => Intent::Scale {
            scale: if shift { Scale::Halve } else { Scale::Double },
        },
        Key::O => Intent::Follow,
        Key::V => Intent::Layout,
        Key::L => Intent::Loop { selection: shift },
        Key::R => Intent::ToggleRate,
        Key::C if shift => Intent::Mute,
        Key::C => Intent::ToggleMetronome,
        Key::Delete | Key::Backspace => Intent::Delete,
        Key::Questionmark | Key::Slash => Intent::Help,
        _ => {
            let numbers = [
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
                Key::Num7,
                Key::Num8,
                Key::Num9,
                Key::Num0,
            ];
            Intent::Grid(numbers.iter().position(|number| *number == key)?)
        }
    })
}

/// The keys of the timing window, for its shortcut sheet.
#[expect(
    clippy::non_ascii_literal,
    reason = "the timing help displays arrow, minus, ellipsis and separator symbols as UI labels"
)]
const KEYS: &[(&str, &str)] = &[
    ("Space", "Play or pause"),
    (
        "Enter",
        "Play from a beat before the focused anchor; apply a fit",
    ),
    ("Esc", "Cancel a drag or fit, then clear the selection"),
    ("A", "Add an anchor at the playhead"),
    ("J, K", "Focus the anchor before, after"),
    ("N, Shift+N", "Next, previous issue"),
    (
        "←, →",
        "Nudge in the audio by 1 ms · Shift 0.1 ms · Ctrl 10 ms",
    ),
    ("↑, ↓", "Move on the grid by one step"),
    ("+, −", "Tail tempo by 0.1 BPM · Shift 1 BPM"),
    ("1 … 0", "Grid 1/1 … 1/32"),
    ("F, Shift+F", "Fit tempo, fit to half BPMs"),
    ("D, Shift+D", "Double, halve the tempo"),
    ("O", "Follow the playhead"),
    ("V", "Rows of bars or rows of seconds"),
    ("L, Shift+L", "Loop the bar under the cursor, the selection"),
    ("R", "Switch to the alternate speed and back"),
    ("C, Shift+C held", "Metronome on or off; muted while held"),
    ("?", "Show or hide these keys"),
    ("Del", "Remove the focused or selected anchors"),
    ("Ctrl+A", "Select every anchor"),
    (
        "Ctrl+wheel",
        "Nudge the focused anchor by 1 ms · Shift 0.1 ms",
    ),
];

/// Draws the scrollable keyboard shortcut reference.
pub(crate) fn help(ui: &mut egui::Ui) {
    let narrow = ui.available_width() < 600.0;
    let height = (ui.available_height() - ROW - 20.0).max(120.0);
    style::solid_scroll_bars(ui);
    egui::ScrollArea::vertical()
        .id_salt("timing help")
        .max_height(height)
        .show(ui, |ui| {
            egui::Frame::new()
                .fill(style::RAISED)
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    if narrow {
                        for (keys, does) in KEYS {
                            ui.monospace(*keys);
                            ui.label(*does);
                            ui.add_space(6.0);
                        }
                    } else {
                        style::table(ui, "timing keys", |ui| {
                            for (keys, does) in KEYS {
                                ui.monospace(*keys);
                                ui.label(*does);
                                ui.end_row();
                            }
                        });
                    }
                });
        });
}

/// The window's keys, while it has the keyboard.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "only pressed key events make keyboard shortcuts"
)]
pub(crate) fn keyboard(
    ctx: &egui::Context,
    window: &mut TimingWindow,
    frame: &Frame<'_>,
    commands: &mut Vec<Command>,
) {
    let pressed: Vec<(Key, Modifiers)> = ctx.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
            .collect()
    });
    // Shift+C mutes only while held.
    let let_go = ctx.input(|input| {
        input.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key: Key::C,
                    pressed: false,
                    ..
                }
            )
        })
    });
    if window.muting && let_go {
        window.muting = false;
        commands.push(Command::MuteMetronome(false));
    }
    let dragging = !matches!(window.gesture, Gesture::Idle);
    for (key, modifiers) in pressed {
        let Some(intent) = intent(key, modifiers) else {
            continue;
        };
        // During a drag only Esc and Space count.
        if dragging && !matches!(intent, Intent::Escape | Intent::Play) {
            continue;
        }
        if frame.editable
            || matches!(
                intent,
                Intent::Play
                    | Intent::Focus(_)
                    | Intent::Issue(_)
                    | Intent::Grid(_)
                    | Intent::Follow
                    | Intent::Layout
                    | Intent::Help
                    | Intent::Escape
                    | Intent::Loop { .. }
                    | Intent::ToggleRate
                    | Intent::ToggleMetronome
                    | Intent::Mute
            )
        {
            act(window, frame, intent, commands);
        }
    }
}

/// Does what `intent` asks.
#[expect(clippy::too_many_lines, reason = "one arm for every window key")]
pub(crate) fn act(
    window: &mut TimingWindow,
    frame: &Frame<'_>,
    intent: Intent,
    commands: &mut Vec<Command>,
) {
    let index = |tick: Tick| frame.pins.iter().position(|pin| pin.tick == tick);
    match intent {
        Intent::Play => commands.push(Command::TogglePlay),
        Intent::Enter => {
            if let Some((fitted, _)) = window.preview.take() {
                commands.push(Command::Edit(Edit::Repin {
                    pins: fitted.pins,
                    tail: fitted.tail,
                }));
            } else if let Some(pin) = window
                .focus
                .and_then(index)
                .and_then(|at| frame.pins.get(at))
                && let Some(from) = audition_from(frame, pin.seconds, pin.tick)
            {
                commands.push(Command::Audition(from));
            }
        }
        Intent::Escape => {
            if !matches!(window.gesture, Gesture::Idle) {
                window.gesture = Gesture::Idle;
            } else if window.preview.is_some() {
                window.preview = None;
            } else if window.help {
                window.help = false;
            } else {
                window.selection.clear();
                window.focus = None;
            }
        }
        Intent::AddAtPlayhead => {
            let seconds = frame.timing.time_at(frame.cursor).seconds();
            window.add(frame, seconds, false, commands);
        }
        Intent::Focus(later) => {
            let current = window.focus.and_then(index);
            let next = match (current, later) {
                (Some(at), true) => at.saturating_add(1),
                (Some(at), false) => at.saturating_sub(1),
                (None, _) => frame
                    .pins
                    .iter()
                    .position(|pin| pin.tick >= frame.cursor)
                    .unwrap_or(0),
            };
            if let Some(pin) = frame.pins.get(next.min(frame.pins.len().saturating_sub(1))) {
                window.focus_on(pin.tick);
            }
        }
        Intent::Issue(later) => {
            let count = frame.issues.len();
            if count > 0 {
                let at = match (window.issue, later) {
                    (Some(at), true) => at.saturating_add(1).rem_euclid(count),
                    (Some(at), false) => {
                        at.saturating_add(count).saturating_sub(1).rem_euclid(count)
                    }
                    (None, _) => 0,
                };
                window.go_to_issue(frame, at);
            }
        }
        Intent::Nudge(by) => window.nudge(by, frame.now, commands),
        Intent::Move(steps) => {
            let moved: Vec<(Tick, Tick)> = window
                .targets()
                .into_iter()
                .map(|tick| (tick, window.grid.stepped(frame.timing, tick, steps)))
                .collect();
            if let Some(&(_, to)) = moved.iter().find(|(from, _)| Some(*from) == window.focus) {
                window.focus = Some(to);
            }
            window.selection = moved.iter().map(|(_, to)| *to).collect();
            commands.push(Command::Edit(Edit::MoveAnchors(moved)));
        }
        Intent::Tail(by) => {
            let tempo = f64::from(frame.tail) + by;
            match Bpm::nearest(tempo) {
                Ok(bpm) => {
                    let merge = window.adjusting(Adjusted::Tail, frame.now);
                    commands.push(Command::Edit(Edit::TailBpm { bpm, merge }));
                }
                Err(_) => window.status = Some(format!("{tempo} is not a tempo")),
            }
        }
        Intent::Grid(at) => {
            if let Some(&resolution) = RESOLUTIONS.get(at) {
                window.grid = Grid { resolution };
            }
        }
        Intent::Fit { rounding } => window.fit(frame, rounding),
        Intent::Scale { scale } => {
            commands.push(Command::Edit(Edit::Scale {
                ticks: window.targets(),
                scale,
            }));
        }
        Intent::Follow => window.follow = !window.follow,
        Intent::Layout => {
            window.layout = match window.layout {
                Layout::Bars => Layout::Time,
                Layout::Time => Layout::Bars,
            };
            // Keep what was in view there.
            window.scroll_to = window.focus.or(Some(frame.cursor));
        }
        Intent::Delete => {
            let targets = window.targets();
            if !targets.is_empty() {
                commands.push(Command::Edit(Edit::Unsync(targets)));
                window.selection.clear();
            }
        }
        Intent::SelectAll => {
            window.selection = frame.pins.iter().map(|pin| pin.tick).collect();
            window.focus = window.selection.first().copied();
        }
        Intent::Help => window.help = !window.help,
        Intent::Loop { selection } => {
            let span = if selection {
                window.selected_span(frame)
            } else {
                None
            };
            let span = span.or_else(|| {
                let measure = frame.timing.measures_from(frame.cursor).next()?;
                Some((measure.start, measure.start.checked_add(measure.length)?))
            });
            if let Some((from, to)) = span {
                commands.push(Command::Loop { from, to });
            }
        }
        Intent::ToggleRate => commands.push(Command::ToggleRate),
        Intent::ToggleMetronome => commands.push(Command::ToggleMetronome),
        Intent::Mute => {
            if !window.muting {
                window.muting = true;
                commands.push(Command::MuteMetronome(true));
            }
        }
    }
}

/// Ctrl+wheel notches among `events`, later first, and whether Shift made them coarse.
pub(crate) fn nudge_notches(events: &[egui::Event]) -> Option<(i32, bool)> {
    let mut notches = 0_i32;
    let mut coarse = false;
    for event in events {
        if let egui::Event::MouseWheel {
            delta, modifiers, ..
        } = event
            && (modifiers.command || modifiers.ctrl)
            && delta.y != 0.0
        {
            notches = notches.saturating_add(if delta.y > 0.0 { 1 } else { -1 });
            coarse |= modifiers.shift;
        }
    }
    (notches != 0).then_some((notches, coarse))
}

#[cfg(test)]
mod tests {
    #![allow(
        unused_imports,
        reason = "shared imports keep moved tests close to their original form"
    )]

    use charter_audio::Onset;
    use charter_core::{Bpm, Tick, ValidDocument};
    use charter_editor::{Pin, Song};
    use egui::{Key, Modifiers, Pos2, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use serde_json::json;

    use super::*;
    use crate::test_support::*;
    use crate::*;

    #[test]
    fn escape_cancels_a_drag_before_clearing_the_selection() {
        let mut harness = timing_harness();
        let at = row_point(&harness, 960);
        click(&mut harness, at, Modifiers::NONE);
        harness.state_mut().window.gesture = Gesture::DragBeat {
            from: Tick::new(960),
            to: Tick::new(1440),
            origin: 0.25,
        };
        harness.key_press(Key::Delete);
        harness.run();
        assert_eq!(harness.state().commands, []);
        harness.key_press(Key::Escape);
        harness.run();
        assert_eq!(harness.state().window.gesture, pointer::Gesture::Idle);
        assert_eq!(harness.state().window.selection, [Tick::new(960)]);
        harness.key_press(Key::Escape);
        harness.run();
        assert_eq!(harness.state().window.selection, []);
    }

    #[test]
    fn selecting_two_anchors_loops_their_span_and_layout_switch_keeps_the_focus() {
        let mut harness = timing_harness();
        let first = row_point(&harness, 0);
        let last = row_point(&harness, 1920);
        click(&mut harness, first, Modifiers::NONE);
        click(&mut harness, last, Modifiers::SHIFT);
        harness.key_press_modifiers(Modifiers::SHIFT, Key::L);
        harness.step();
        assert_eq!(
            harness.state().commands,
            [Command::Loop {
                from: Tick::new(0),
                to: Tick::new(1920)
            }]
        );
        harness.key_press(Key::V);
        harness.step();
        assert_eq!(harness.state().window.layout, Layout::Time);
        assert_eq!(harness.state().window.scroll_to, None);
        assert!(harness.query_by_label("Row at 0.0 s").is_some());
    }

    #[test]
    fn a_key_after_clicking_outside_the_window_does_not_edit_the_song() {
        let mut harness = timing_harness();
        let at = row_point(&harness, 960);
        click(&mut harness, at, Modifiers::NONE);
        assert!(harness.state().window.has_keyboard());
        click(&mut harness, egui::pos2(20.0, 20.0), Modifiers::NONE);
        assert!(!harness.state().window.has_keyboard());
        harness.key_press(Key::Delete);
        harness.step();
        assert_eq!(harness.state().commands, []);
    }

    #[test]
    fn only_a_focused_existing_anchor_can_be_auditioned_with_enter() {
        let mut harness = timing_harness();
        harness.key_press(Key::Enter);
        harness.step();
        assert_eq!(harness.state().commands, []);
        let at = row_point(&harness, 960);
        click(&mut harness, at, Modifiers::NONE);
        harness.key_press(Key::Enter);
        harness.step();
        assert_eq!(
            harness.state().commands,
            [Command::Audition(
                charter_core::ResolvedTime::from_seconds(0.0).unwrap()
            )]
        );
    }

    #[test]
    fn nudge_merge_expires_at_the_window_and_is_scoped_to_the_same_targets() {
        let mut window = TimingWindow::default();
        let mut commands = Vec::new();
        window.focus = Some(Tick::new(960));
        window.nudge(0.001, 1.0, &mut commands);
        window.nudge(0.001, 1.8, &mut commands);
        window.selection = vec![Tick::new(0), Tick::new(960)];
        window.nudge(0.001, 1.9, &mut commands);
        assert_eq!(
            commands,
            [
                Command::Edit(Edit::Nudge {
                    ticks: vec![Tick::new(960)],
                    by: charter_core::TimeShift::from_seconds(0.001).unwrap(),
                    merge: false
                }),
                Command::Edit(Edit::Nudge {
                    ticks: vec![Tick::new(960)],
                    by: charter_core::TimeShift::from_seconds(0.001).unwrap(),
                    merge: false
                }),
                Command::Edit(Edit::Nudge {
                    ticks: vec![Tick::new(0), Tick::new(960)],
                    by: charter_core::TimeShift::from_seconds(0.001).unwrap(),
                    merge: false
                })
            ]
        );
    }

    #[test]
    fn keys_ask_for_what_the_specification_gives_them() {
        let none = Modifiers::NONE;
        assert_eq!(intent(Key::ArrowRight, none), Some(Intent::Nudge(0.001)));
        assert_eq!(
            intent(Key::ArrowLeft, Modifiers::SHIFT),
            Some(Intent::Nudge(-0.0001))
        );
        assert_eq!(
            intent(Key::ArrowRight, Modifiers::COMMAND),
            Some(Intent::Nudge(0.010))
        );
        assert_eq!(intent(Key::ArrowUp, none), Some(Intent::Move(-1)));
        assert_eq!(intent(Key::Num1, none), Some(Intent::Grid(0)));
        assert_eq!(intent(Key::Num0, none), Some(Intent::Grid(9)));
        assert_eq!(
            intent(Key::F, Modifiers::SHIFT),
            Some(Intent::Fit {
                rounding: Rounding::HalfBpm
            })
        );
        assert_eq!(
            intent(Key::D, Modifiers::SHIFT),
            Some(Intent::Scale {
                scale: Scale::Halve
            })
        );
        assert_eq!(intent(Key::A, Modifiers::COMMAND), Some(Intent::SelectAll));
        assert_eq!(intent(Key::A, none), Some(Intent::AddAtPlayhead));
        assert_eq!(intent(Key::N, Modifiers::SHIFT), Some(Intent::Issue(false)));
        assert_eq!(
            intent(Key::Minus, Modifiers::SHIFT),
            Some(Intent::Tail(-1.0))
        );
        assert_eq!(intent(Key::Q, none), None);
    }

    #[test]
    fn ctrl_wheel_notches_add_up_and_shift_makes_them_fine() {
        let wheel = |up: f32, modifiers| egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(0.0, up),
            modifiers,
            phase: egui::TouchPhase::Move,
        };
        let ctrl = Modifiers::COMMAND;
        assert_eq!(nudge_notches(&[wheel(1.0, Modifiers::NONE)]), None);
        let fine = [wheel(1.0, ctrl), wheel(1.0, ctrl), wheel(-1.0, ctrl)];
        assert_eq!(nudge_notches(&fine), Some((1, false)));
        let coarse = [wheel(-1.0, ctrl.plus(Modifiers::SHIFT))];
        assert_eq!(nudge_notches(&coarse), Some((-1, true)));
    }

    #[test]
    fn issue_keys_wrap_in_both_directions_and_focus_the_named_anchor() {
        let mut harness = timing_harness();
        harness.state_mut().onsets = [0.012, 0.512, 1.012]
            .map(|seconds| Onset {
                seconds,
                strength: 1.0,
            })
            .to_vec();
        harness.run();
        let count = harness.state().window.cache.as_ref().unwrap().issues.len();
        assert!(count >= 3, "expected displaced anchors to be issues");
        harness.key_press(Key::N);
        harness.run();
        assert_eq!(harness.state().window.issue, Some(0));
        assert_eq!(harness.state().window.focus, Some(Tick::new(0)));
        harness.key_press_modifiers(Modifiers::SHIFT, Key::N);
        harness.run();
        assert_eq!(harness.state().window.issue, Some(count - 1));
        assert_eq!(harness.state().window.focus, Some(Tick::new(1920)));
        harness.key_press(Key::N);
        harness.run();
        assert_eq!(harness.state().window.issue, Some(0));
    }

    #[test]
    fn playback_and_metronome_keys_work_on_a_read_only_song_but_edits_do_not() {
        let mut harness = timing_harness();
        let document = harness.state().song.document().clone();
        harness.state_mut().song = Song::new(document, &[]);
        harness.run();
        let anchor = row_point(&harness, 960);
        click(&mut harness, anchor, Modifiers::NONE);
        harness.key_press(Key::ArrowRight);
        harness.key_press(Key::Delete);
        harness.key_press(Key::Space);
        harness.key_press(Key::R);
        harness.key_press(Key::C);
        harness.step();
        assert_eq!(
            harness.state().commands,
            [
                Command::TogglePlay,
                Command::ToggleRate,
                Command::ToggleMetronome
            ]
        );
        harness.key_press_modifiers(Modifiers::SHIFT, Key::C);
        harness.step();
        assert!(
            harness
                .state()
                .commands
                .ends_with(&[Command::MuteMetronome(true), Command::MuteMetronome(false)])
        );
        harness.event(egui::Event::Key {
            key: Key::C,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        harness.step();
        assert_eq!(
            harness.state().commands.last(),
            Some(&Command::MuteMetronome(false))
        );
    }

    #[test]
    fn a_fit_stays_a_preview_until_enter_and_escape_discards_it() {
        let mut harness = timing_harness();
        harness.state_mut().onsets = [0.0, 0.5, 1.0, 1.5, 2.0]
            .map(|seconds| Onset {
                seconds,
                strength: 1.0,
            })
            .to_vec();
        harness.run();
        let at = row_point(&harness, 960);
        click(&mut harness, at, Modifiers::NONE);
        harness.key_press(Key::F);
        harness.step();
        assert!(harness.state().window.preview.is_some());
        assert_eq!(harness.state().commands, []);
        harness.key_press(Key::Escape);
        harness.step();
        assert!(harness.state().window.preview.is_none());
        assert_eq!(harness.state().commands, []);
        harness.key_press(Key::F);
        harness.step();
        harness.key_press(Key::Enter);
        harness.step();
        assert!(matches!(
            harness.state().commands.as_slice(),
            [Command::Edit(Edit::Repin { .. })]
        ));
        assert!(harness.state().window.preview.is_none());
    }

    #[test]
    fn looping_one_selected_anchor_ends_at_the_next_anchor() {
        let mut harness = timing_harness();
        let at = row_point(&harness, 960);
        click(&mut harness, at, Modifiers::NONE);
        harness.key_press_modifiers(Modifiers::SHIFT, Key::L);
        harness.step();
        assert_eq!(
            harness.state().commands,
            [Command::Loop {
                from: Tick::new(960),
                to: Tick::new(1920),
            }]
        );
    }

    #[test]
    fn focusing_from_the_playhead_finds_the_next_anchor_and_moving_it_keeps_focus() {
        let mut harness = timing_harness();
        harness.state_mut().song.jump_to(Tick::new(1200));
        harness.run();
        let state = harness.state();
        let cache = state.window.cache.as_ref().unwrap();
        let frame = frame(&state.song, &[], (cache, Layout::Bars), 0.0);
        let mut window = TimingWindow::default();
        let mut commands = Vec::new();
        act(&mut window, &frame, Intent::Focus(true), &mut commands);
        assert_eq!(window.focus, Some(Tick::new(1920)));
        act(&mut window, &frame, Intent::Move(1), &mut commands);
        assert_eq!(window.focus, Some(Tick::new(2880)));
        assert_eq!(window.selection, [Tick::new(2880)]);
        assert_eq!(
            commands,
            [Command::Edit(Edit::MoveAnchors(vec![(
                Tick::new(1920),
                Tick::new(2880)
            )]))]
        );
    }

    #[test]
    fn consecutive_tail_tempo_adjustments_ask_to_merge_into_one_edit() {
        let harness = timing_harness();
        let state = harness.state();
        let cache = state.window.cache.as_ref().unwrap();
        let frame = frame(&state.song, &[], (cache, Layout::Bars), 1.0);
        let mut window = TimingWindow::default();
        let mut commands = Vec::new();
        act(&mut window, &frame, Intent::Tail(1.0), &mut commands);
        act(&mut window, &frame, Intent::Tail(1.0), &mut commands);
        assert_eq!(
            commands,
            [
                Command::Edit(Edit::TailBpm {
                    bpm: Bpm::try_from(121.0).unwrap(),
                    merge: false,
                }),
                Command::Edit(Edit::TailBpm {
                    bpm: Bpm::try_from(121.0).unwrap(),
                    merge: true,
                }),
            ]
        );
    }

    #[test]
    fn a_tail_adjustment_does_not_merge_into_a_nudge_of_the_anchors() {
        let harness = timing_harness();
        let state = harness.state();
        let cache = state.window.cache.as_ref().unwrap();
        let frame = frame(&state.song, &[], (cache, Layout::Bars), 1.0);
        let mut window = TimingWindow::default();
        let mut commands = Vec::new();

        // A nudge and a tail change are different adjustments, so one never joins the other's
        // undo step however close together they fall.
        window.focus_on(Tick::new(0));
        window.nudge(0.01, frame.now, &mut commands);
        act(&mut window, &frame, Intent::Tail(1.0), &mut commands);
        act(&mut window, &frame, Intent::Tail(1.0), &mut commands);
        window.nudge(0.01, frame.now, &mut commands);

        let mut merges = Vec::new();
        for command in &commands {
            if let Command::Edit(Edit::Nudge { merge, .. } | Edit::TailBpm { merge, .. }) = command
            {
                merges.push(*merge);
            }
        }
        assert_eq!(merges, [false, false, true, false]);
    }

    #[test]
    fn the_wide_keyboard_help_lists_timing_shortcuts() {
        let mut harness = timing_harness();
        harness.state_mut().window.help = true;
        harness.run();
        assert!(harness.query_by_label("Ctrl+wheel").is_some());
    }
}
