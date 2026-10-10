//! Pointer gestures and context menus for timing rows.

use crate::keyboard::nudge_notches;
use crate::rows::{GRAB, Ghost, Reach, Row};
use charter_core::{ResolvedTime, TickOffset};

use super::{
    Command, Edit, FINE, Frame, Pos2, Tick, TimeShift, TimingWindow, accuracy, audition_from, rows,
};

/// What a press or drag in the rows is doing, as the specification's state machine has it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Gesture {
    /// No press or drag is active.
    Idle,
    /// Moving an anchor's beat, `from` where it was to `to`, with the others selected alike.
    DragBeat {
        /// Tick where the dragged anchor began.
        from: Tick,
        /// Tick where the anchor would land if released now.
        to: Tick,
        /// Pointer position at the start of the drag, in screen points.
        origin: f64,
    },
    /// Moving an anchor in the audio, from `from` seconds to `to`.
    DragAudio {
        /// Tick identifying the dragged anchor.
        tick: Tick,
        /// Audio moment before the drag, in seconds.
        from: f64,
        /// Audio moment if released now, in seconds.
        to: f64,
        /// Pointer position at the start of the drag, in screen points.
        origin: f64,
    },
    /// Selecting the anchors between two points on screen.
    Band {
        /// Screen position where the selection drag began.
        origin: Pos2,
        /// Current opposite corner of the selection rectangle.
        to: Pos2,
    },
}

/// The bar positions the anchor being dragged on the grid may reach, if one is; the rest is
/// dimmed.
pub(crate) fn reach(window: &TimingWindow, frame: &Frame<'_>) -> Option<Reach> {
    let Gesture::DragBeat { from, .. } = window.gesture else {
        return None;
    };
    let bar = |tick: Tick| {
        let at = rows::float_from_i64(i64::from(tick));
        frame.bars.position(at)
    };
    let moving = moving(window, from);
    let (first, last) = (moving.first().copied()?, moving.last().copied()?);
    let before = frame
        .pins
        .iter()
        .rev()
        .find(|pin| pin.tick < first)
        .map(|pin| pin.tick);
    let after = frame
        .pins
        .iter()
        .find(|pin| pin.tick > last)
        .map(|pin| pin.tick);
    Some(Reach {
        from: before.map_or(f64::MIN, bar),
        to: after.map_or(f64::MAX, bar),
    })
}

/// Where in `row` what is under way would land, and whether that is on a snap target: a
/// dragged anchor's target, or where a click at `hover` would add one.
pub(crate) fn ghost(
    window: &TimingWindow,
    frame: &Frame<'_>,
    row: &Row,
    hover: Option<Pos2>,
    shift: bool,
) -> Option<Ghost> {
    match window.gesture {
        Gesture::DragBeat { to, .. } if row.holds(&frame.bars, to) => Some(Ghost {
            x: row.x(&frame.bars, to),
            snapped: window.snap_grid && !shift,
        }),
        Gesture::DragAudio { to, .. } => {
            let snapped = window.snap_onset
                && !shift
                && frame
                    .onsets
                    .iter()
                    .any(|onset| (onset.seconds - to).abs() < 1e-9);
            Some(Ghost {
                x: seconds_x(frame, row, to)?,
                snapped,
            })
        }
        Gesture::Idle if frame.editable => {
            let pos = hover?;
            if anchor_near(frame, row, pos.x).is_some() {
                return None;
            }
            let seconds = row.seconds_at(&frame.bars, frame.timing, pos.x);
            let snapped = (window.snap_onset && !shift)
                .then(|| accuracy::snap(frame.onsets, window.sensitivity, seconds))
                .flatten();
            let target = snapped.unwrap_or(seconds);
            Some(Ghost {
                x: seconds_x(frame, row, target)?,
                snapped: snapped.is_some(),
            })
        }
        Gesture::Idle | Gesture::Band { .. } | Gesture::DragBeat { .. } => None,
    }
}

/// Where the moment `seconds` lies across `row`, if it shows there.
pub(crate) fn seconds_x(frame: &Frame<'_>, row: &Row, seconds: f64) -> Option<f32> {
    let moment = ResolvedTime::from_seconds(seconds)?;
    let tick = frame.timing.latest_tick_by(moment);
    row.holds(&frame.bars, tick).then(|| {
        let at = frame.timing.time_at(tick).seconds();
        let next = frame
            .timing
            .time_at(Tick::new(
                i32::try_from(i64::from(tick).saturating_add(1)).unwrap_or(i32::MAX),
            ))
            .seconds();
        let within = if next > at {
            (seconds - at) / (next - at)
        } else {
            0.0
        };
        let tick = rows::float_from_i64(i64::from(tick)) + within.clamp(0.0, 1.0);
        row.x_of(frame.bars.position(tick))
    })
}

/// The anchors a drag of the one on `tick` moves: the selection when it is in it.
pub(crate) fn moving(window: &TimingWindow, tick: Tick) -> Vec<Tick> {
    if window.selection.contains(&tick) {
        window.selection.clone()
    } else {
        vec![tick]
    }
}

/// The endpoints of the anchors moving with `tick`, without copying the selection.
pub(crate) fn moving_bounds(window: &TimingWindow, tick: Tick) -> (Tick, Tick) {
    if window.selection.contains(&tick) {
        window
            .selection
            .first()
            .copied()
            .zip(window.selection.last().copied())
            .unwrap_or((tick, tick))
    } else {
        (tick, tick)
    }
}

/// The anchor in `row` within grabbing distance of `x`, the nearest if several.
pub(crate) fn anchor_near(frame: &Frame<'_>, row: &Row, x: f32) -> Option<Tick> {
    frame
        .pins
        .iter()
        .filter(|pin| row.holds(&frame.bars, pin.tick))
        .map(|pin| (pin.tick, (row.x(&frame.bars, pin.tick) - x).abs()))
        .filter(|(_, off)| *off <= GRAB)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(tick, _)| tick)
}

/// A row's presses, clicks and the start and run of drags on it.
#[expect(
    clippy::too_many_lines,
    reason = "handles row pointer gestures and context menu"
)]
pub(crate) fn row_input(
    response: &egui::Response,
    row: &Row,
    frame: &Frame<'_>,
    window: &mut TimingWindow,
    commands: &mut Vec<Command>,
) {
    let modifiers = response.ctx.input(|input| input.modifiers);
    let (shift, ctrl) = (modifiers.shift, modifiers.command || modifiers.ctrl);
    let pointer = response
        .interact_pointer_pos()
        .or_else(|| response.hover_pos());
    let anchor = pointer.and_then(|pos| anchor_near(frame, row, pos.x));
    // Where the button went down, not where the drag was first noticed a few points on.
    if response.drag_started()
        && let Some(origin) = response.ctx.input(|input| input.pointer.press_origin())
    {
        let pressed = anchor_near(frame, row, origin.x);
        let seconds = row.seconds_at(&frame.bars, frame.timing, origin.x);
        window.gesture = match pressed {
            Some(tick) if ctrl => {
                let at = frame
                    .pins
                    .iter()
                    .find(|pin| pin.tick == tick)
                    .map_or(seconds, |pin| pin.seconds.seconds());
                Gesture::DragAudio {
                    tick,
                    from: at,
                    to: at,
                    origin: seconds,
                }
            }
            Some(tick) => Gesture::DragBeat {
                from: tick,
                to: tick,
                origin: row.position_at(origin.x),
            },
            None => Gesture::Band { origin, to: origin },
        };
        if let Some(tick) = pressed
            && !window.selection.contains(&tick)
        {
            window.focus = Some(tick);
            window.selection = vec![tick];
        }
    }
    if response.dragged()
        && let Some(pos) = response.ctx.input(|input| input.pointer.latest_pos())
    {
        drag(window, frame, row, pos, shift);
    }
    if response.double_clicked() {
        if let Some(tick) = anchor {
            window.focus = Some(tick);
            window.selection = vec![tick];
            window.drafts.focus_seconds = true;
        }
    } else if response.clicked()
        && let Some(pos) = pointer
    {
        match anchor {
            Some(tick) if shift => {
                let from = window.focus.unwrap_or(tick);
                window.select_between(frame, from, tick);
                window.focus = Some(tick);
            }
            Some(tick) => {
                window.focus = Some(tick);
                window.selection = vec![tick];
            }
            None => {
                let seconds = row.seconds_at(&frame.bars, frame.timing, pos.x);
                window.add(frame, seconds, shift, commands);
            }
        }
    }
    if response.middle_clicked()
        && let Some(pos) = pointer
    {
        match anchor.and_then(|tick| frame.pins.iter().find(|pin| pin.tick == tick)) {
            Some(pin) => {
                if let Some(from) = audition_from(frame, pin.seconds, pin.tick) {
                    commands.push(Command::Audition(from));
                }
            }
            None => commands.push(Command::PlayFrom(row.tick_at(&frame.bars, pos.x))),
        }
    }
    if response.secondary_clicked()
        && let Some(pos) = pointer
    {
        window.menu_target = Some((anchor, pos));
    }
    response.context_menu(|ui| {
        let (anchor, pointer) = window
            .menu_target
            .map_or((anchor, pointer), |(tick, pos)| (tick, Some(pos)));
        menu(ui, window, frame, row, anchor, pointer, commands);
    });
    if response.hovered()
        && let Some((notches, fine)) = response.ctx.input(|input| nudge_notches(&input.events))
    {
        let target = window.focus.or(anchor);
        if let Some(tick) = target {
            window.focus = Some(tick);
            if window.selection.is_empty() {
                window.selection = vec![tick];
            }
            let by = if fine { 0.0001 } else { 0.001 };
            window.nudge(by * f64::from(notches), frame.now, commands);
        }
    }
}

/// Moves the drag under way to `pos`, over `row`: a beat snaps to the grid, a moment to an onset,
/// both finely and unsnapped with Shift, and neither past the anchors beside what moves; running
/// into one pulses it and stops there.
pub(crate) fn drag(
    window: &mut TimingWindow,
    frame: &Frame<'_>,
    row: &Row,
    pos: Pos2,
    shift: bool,
) {
    match window.gesture {
        Gesture::DragBeat { from, origin, .. } => {
            let pointer = row.position_at(pos.x);
            let bar = if shift {
                origin + (pointer - origin) / FINE
            } else {
                pointer
            };
            let tick = frame.bars.tick_at_position(bar).round();
            let tick = Tick::new(rows::float_to_i32(tick));
            let snapped = if window.snap_grid && !shift {
                window.grid.nearest(frame.timing, tick)
            } else {
                tick
            };
            let to = clamp_beat(window, frame, from, snapped);
            window.gesture = Gesture::DragBeat { from, to, origin };
        }
        Gesture::DragAudio {
            tick, from, origin, ..
        } => {
            let pointer = row.seconds_at(&frame.bars, frame.timing, pos.x);
            let moment = if shift {
                origin + (pointer - origin) / FINE
            } else {
                pointer
            };
            let target = from + (moment - origin);
            let target = if window.snap_onset && !shift {
                accuracy::snap(frame.onsets, window.sensitivity, target).unwrap_or(target)
            } else {
                target
            };
            let to = clamp_audio(window, frame, tick, from, target);
            window.gesture = Gesture::DragAudio {
                tick,
                from,
                to,
                origin,
            };
        }
        Gesture::Band { .. } | Gesture::Idle => {}
    }
}

/// `to`, or the nearest beat to it that the anchors moving with the one on `from` can reach
/// without meeting a neighbour, which then pulses.
pub(crate) fn clamp_beat(
    window: &mut TimingWindow,
    frame: &Frame<'_>,
    from: Tick,
    to: Tick,
) -> Tick {
    let (first, last) = moving_bounds(window, from);
    let delta = from.offset_to(to).get();
    let before = frame.pins.iter().rev().find(|pin| pin.tick < first);
    let after = frame.pins.iter().find(|pin| pin.tick > last);
    let mut clamped = delta;
    if let Some(before) = before {
        let lowest = first.offset_to(before.tick).get().saturating_add(1);
        if clamped < lowest {
            clamped = lowest;
            window.pulse = Some((before.tick, frame.now));
        }
    }
    if let Some(after) = after {
        let highest = last.offset_to(after.tick).get().saturating_sub(1);
        if clamped > highest {
            clamped = highest;
            window.pulse = Some((after.tick, frame.now));
        }
    }
    from.checked_offset(TickOffset::new(clamped))
        .unwrap_or(Tick::new(i32::MAX))
}

/// `to` seconds, or the nearest moment the anchors moving with the one on `tick` can reach
/// without meeting a neighbour in the audio, which then pulses.
pub(crate) fn clamp_audio(
    window: &mut TimingWindow,
    frame: &Frame<'_>,
    tick: Tick,
    from: f64,
    to: f64,
) -> f64 {
    const MARGIN: f64 = 0.001;
    let (first, last) = moving_bounds(window, tick);
    let seconds = |at: Tick| {
        frame
            .pins
            .iter()
            .find(|pin| pin.tick == at)
            .map(|pin| pin.seconds.seconds())
    };
    let (Some(first_at), Some(last_at)) = (seconds(first), seconds(last)) else {
        return to;
    };
    let mut delta = to - from;
    if let Some(before) = frame.pins.iter().rev().find(|pin| pin.tick < first)
        && first_at + delta < before.seconds.seconds() + MARGIN
    {
        delta = before.seconds.seconds() + MARGIN - first_at;
        window.pulse = Some((before.tick, frame.now));
    }
    if let Some(after) = frame.pins.iter().find(|pin| pin.tick > last)
        && last_at + delta > after.seconds.seconds() - MARGIN
    {
        delta = after.seconds.seconds() - MARGIN - last_at;
        window.pulse = Some((after.tick, frame.now));
    }
    from + delta
}

/// Ends the drag under way where the pointer let go, at `at` among the `laid` rows.
pub(crate) fn release(
    window: &mut TimingWindow,
    frame: &Frame<'_>,
    laid: &[Row],
    at: Pos2,
    commands: &mut Vec<Command>,
) {
    match std::mem::replace(&mut window.gesture, Gesture::Idle) {
        Gesture::DragBeat { from, to, .. } if from != to => {
            let delta = from.offset_to(to);
            let moved: Vec<(Tick, Tick)> = moving(window, from)
                .into_iter()
                .filter_map(|tick| Some((tick, tick.checked_offset(delta)?)))
                .collect();
            window.selection = moved.iter().map(|(_, destination)| *destination).collect();
            window.focus = Some(to);
            commands.push(Command::Edit(Edit::MoveAnchors(moved)));
        }
        Gesture::DragAudio { tick, from, to, .. } if (to - from).abs() > 1e-7 => {
            let ticks = moving(window, tick);
            if let Some(by) = TimeShift::from_seconds(to - from) {
                commands.push(Command::Edit(Edit::Nudge {
                    ticks,
                    by,
                    merge: false,
                }));
            }
        }
        Gesture::Band { origin, .. } => {
            let tick_of =
                |pos: Pos2| rows::row_at(laid, pos).map(|row| row.tick_at(&frame.bars, pos.x));
            let (Some(a), Some(b)) = (tick_of(origin), tick_of(at)) else {
                return;
            };
            window.select_between(frame, a, b);
            window.focus = window.selection.first().copied();
        }
        Gesture::DragBeat { .. } | Gesture::DragAudio { .. } | Gesture::Idle => {}
    }
}

/// The right-click menu of a row, on an anchor or off one.
pub(crate) fn menu(
    ui: &mut egui::Ui,
    window: &mut TimingWindow,
    frame: &Frame<'_>,
    row: &Row,
    anchor: Option<Tick>,
    pointer: Option<Pos2>,
    commands: &mut Vec<Command>,
) {
    if let Some(tick) = anchor {
        if window.focus != Some(tick) && !window.selection.contains(&tick) {
            window.focus = Some(tick);
            window.selection = vec![tick];
        }
        if ui.button("Play from a beat before").clicked()
            && let Some(pin) = frame.pins.iter().find(|pin| pin.tick == tick)
        {
            if let Some(from) = audition_from(frame, pin.seconds, tick) {
                commands.push(Command::Audition(from));
            }
            ui.close();
        }
        if ui.button("Fit tempo").clicked() {
            window.fit(frame, false);
            ui.close();
        }
        if ui.button("Double tempo").clicked() {
            commands.push(Command::Edit(Edit::Scale {
                ticks: window.targets(),
                double: true,
            }));
            ui.close();
        }
        if ui.button("Halve tempo").clicked() {
            commands.push(Command::Edit(Edit::Scale {
                ticks: window.targets(),
                double: false,
            }));
            ui.close();
        }
        if ui.button("Select to the start").clicked() {
            if let Some(first) = frame.pins.first() {
                window.select_between(frame, first.tick, tick);
            }
            ui.close();
        }
        if ui.button("Select to the end").clicked() {
            if let Some(last) = frame.pins.last() {
                window.select_between(frame, tick, last.tick);
            }
            ui.close();
        }
        ui.separator();
        if ui.button("Remove").clicked() {
            commands.push(Command::Edit(Edit::Unsync(window.targets())));
            ui.close();
        }
    } else if let Some(pos) = pointer {
        if ui.button("Play from here").clicked() {
            commands.push(Command::PlayFrom(row.tick_at(&frame.bars, pos.x)));
            ui.close();
        }
        if ui.button("Add an anchor here").clicked() {
            let seconds = row.seconds_at(&frame.bars, frame.timing, pos.x);
            window.add(frame, seconds, false, commands);
            ui.close();
        }
    }
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
    fn a_row_click_emits_a_sync_and_an_audition_before_the_new_anchor() {
        let mut harness = timing_harness();
        assert_eq!(harness.state().song.read_only(), None);
        let pos = row_point(&harness, 2880);
        click(&mut harness, pos, Modifiers::NONE);
        let [
            Command::Edit(Edit::Sync { tick, seconds }),
            Command::Audition(from),
        ] = harness.state().commands.as_slice()
        else {
            panic!("expected sync and audition: {:?}", harness.state().commands);
        };
        assert_eq!(*tick, Tick::new(2880));
        assert!((seconds.seconds() - 1.5).abs() < 1e-4);
        assert!((from.seconds() - 1.0).abs() < 1e-4);
        assert_eq!(harness.state().window.gesture, pointer::Gesture::Idle);
    }

    #[test]
    fn dragging_a_beat_emits_one_move_and_focuses_its_destination() {
        let mut harness = timing_harness();
        harness.state_mut().window.grid = Grid {
            resolution: charter_core::NoteValue::new(8).unwrap(),
        };
        harness.run();
        let from = row_point(&harness, 960);
        let to = row_point(&harness, 1440);
        crate::test_support::drag(&mut harness, from, to, Modifiers::NONE);
        assert_eq!(
            harness.state().commands,
            [Command::Edit(Edit::MoveAnchors(vec![(
                Tick::new(960),
                Tick::new(1440)
            )]))]
        );
        assert_eq!(harness.state().window.gesture, pointer::Gesture::Idle);
    }

    #[test]
    fn ctrl_dragging_an_anchor_emits_an_audio_nudge_without_changing_its_beat() {
        let mut harness = timing_harness();
        let from = row_point(&harness, 960);
        let to = row_point(&harness, 1200);
        crate::test_support::drag(&mut harness, from, to, Modifiers::COMMAND);
        let [Command::Edit(Edit::Nudge { ticks, by, merge })] = harness.state().commands.as_slice()
        else {
            panic!("expected audio nudge: {:?}", harness.state().commands);
        };
        assert_eq!(ticks, &[Tick::new(960)]);
        assert!((by.seconds() - 0.125).abs() < 1e-4);
        assert!(!merge);
        assert_eq!(harness.state().window.focus, Some(Tick::new(960)));
    }

    #[test]
    fn shift_click_selects_the_run_and_delete_emits_one_unsync() {
        let mut harness = timing_harness();
        let first = row_point(&harness, 0);
        let last = row_point(&harness, 1920);
        click(&mut harness, first, Modifiers::NONE);
        click(&mut harness, last, Modifiers::SHIFT);
        assert_eq!(
            harness.state().window.selection,
            [Tick::new(0), Tick::new(960), Tick::new(1920)]
        );
        harness.key_press(Key::Delete);
        harness.run();
        assert_eq!(
            harness.state().commands,
            [Command::Edit(Edit::Unsync(vec![
                Tick::new(0),
                Tick::new(960),
                Tick::new(1920)
            ]))]
        );
    }

    #[test]
    fn a_band_selects_only_the_anchors_between_its_endpoints() {
        let mut harness = timing_harness();
        let from = row_point(&harness, 400);
        let to = row_point(&harness, 1500);
        crate::test_support::drag(&mut harness, from, to, Modifiers::NONE);
        assert_eq!(harness.state().window.selection, [Tick::new(960)]);
        assert_eq!(harness.state().commands, []);
        assert_eq!(harness.state().window.gesture, pointer::Gesture::Idle);
    }

    #[test]
    fn dragging_into_a_neighbour_stops_short_and_pulses_that_anchor() {
        let mut harness = timing_harness();
        let from = row_point(&harness, 960);
        let to = row_point(&harness, 2400);
        crate::test_support::drag(&mut harness, from, to, Modifiers::NONE);
        assert_eq!(
            harness.state().commands,
            [Command::Edit(Edit::MoveAnchors(vec![(
                Tick::new(960),
                Tick::new(1919)
            )]))]
        );
        assert!(matches!(
            harness.state().window.pulse,
            Some((tick, _)) if tick == Tick::new(1920)
        ));
    }

    #[test]
    fn adding_an_anchor_snaps_to_an_onset_unless_shift_is_held() {
        let mut snapped = timing_harness();
        snapped.state_mut().onsets = vec![Onset {
            seconds: 1.512,
            strength: 1.0,
        }];
        snapped.run();
        let at = row_point(&snapped, 2880);
        click(&mut snapped, at, Modifiers::NONE);
        let [Command::Edit(Edit::Sync { seconds, .. }), ..] = snapped.state().commands.as_slice()
        else {
            panic!("expected sync: {:?}", snapped.state().commands);
        };
        assert!((seconds.seconds() - 1.512).abs() < 1e-9);

        let mut plain = timing_harness();
        plain.state_mut().onsets = vec![Onset {
            seconds: 1.512,
            strength: 1.0,
        }];
        plain.run();
        let plain_point = row_point(&plain, 2880);
        click(&mut plain, plain_point, Modifiers::SHIFT);
        let [
            Command::Edit(Edit::Sync {
                seconds: plain_seconds,
                ..
            }),
            ..,
        ] = plain.state().commands.as_slice()
        else {
            panic!("expected sync: {:?}", plain.state().commands);
        };
        assert!((plain_seconds.seconds() - 1.5).abs() < 1e-4);
    }

    #[test]
    fn adding_too_near_an_anchor_refuses_and_identifies_the_blocker() {
        let mut harness = timing_harness();
        let at = row_point(&harness, 1030);
        click(&mut harness, at, Modifiers::NONE);
        assert_eq!(harness.state().commands, []);
        assert!(matches!(
            harness.state().window.pulse,
            Some((tick, _)) if tick == Tick::new(960)
        ));
        assert!(
            harness
                .query_by_label_contains("Too close to the anchor")
                .is_some()
        );
    }

    #[test]
    fn ctrl_dragging_past_a_neighbour_keeps_a_millisecond_between_them() {
        let mut harness = timing_harness();
        let from = row_point(&harness, 960);
        let to = row_point(&harness, 2400);
        crate::test_support::drag(&mut harness, from, to, Modifiers::COMMAND);
        let [Command::Edit(Edit::Nudge { ticks, by, merge })] = harness.state().commands.as_slice()
        else {
            panic!("expected constrained nudge: {:?}", harness.state().commands);
        };
        assert_eq!(ticks, &[Tick::new(960)]);
        assert!((by.seconds() - 0.499).abs() < 1e-5);
        assert!(!merge);
        assert!(matches!(
            harness.state().window.pulse,
            Some((tick, _)) if tick == Tick::new(1920)
        ));
    }

    #[test]
    fn an_anchor_context_menu_keeps_its_target_while_the_pointer_moves_to_a_button() {
        let mut harness = timing_harness();
        let at = row_point(&harness, 960);
        harness.hover_at(at);
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        harness.step();
        harness.get_by_label("Double tempo").click();
        harness.step();
        assert_eq!(
            harness.state().commands,
            [Command::Edit(Edit::Scale {
                ticks: vec![Tick::new(960)],
                double: true
            })]
        );
    }

    #[test]
    fn dragging_one_of_several_selected_anchors_moves_the_whole_run() {
        let mut harness = timing_harness();
        harness.state_mut().window.grid = Grid {
            resolution: charter_core::NoteValue::new(8).unwrap(),
        };
        harness.run();
        let first = row_point(&harness, 0);
        let second = row_point(&harness, 960);
        click(&mut harness, first, Modifiers::NONE);
        click(&mut harness, second, Modifiers::SHIFT);
        let to = row_point(&harness, 1440);
        crate::test_support::drag(&mut harness, second, to, Modifiers::NONE);
        assert_eq!(
            harness.state().commands,
            [Command::Edit(Edit::MoveAnchors(vec![
                (Tick::new(0), Tick::new(480)),
                (Tick::new(960), Tick::new(1440))
            ]))]
        );
    }

    #[test]
    fn the_anchor_menu_can_audition_select_a_run_and_remove_that_run() {
        let mut harness = timing_harness();
        let at = row_point(&harness, 960);
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        harness.step();
        harness.get_by_label("Select to the end").click();
        harness.step();
        assert_eq!(
            harness.state().window.selection,
            [Tick::new(960), Tick::new(1920)]
        );
        harness.hover_at(at);
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        harness.step();
        harness.get_by_label("Remove").click();
        harness.step();
        assert_eq!(
            harness.state().commands,
            [Command::Edit(Edit::Unsync(vec![
                Tick::new(960),
                Tick::new(1920)
            ]))]
        );
    }

    #[test]
    fn a_dragged_audio_anchor_shows_when_it_snaps_to_an_onset() {
        let harness = timing_harness();
        let state = harness.state();
        let cache = state.window.cache.as_ref().unwrap();
        let onsets = [Onset {
            seconds: 0.75,
            strength: 1.0,
        }];
        let frame = frame(&state.song, &onsets, (cache, Layout::Bars), 0.0);
        let span = frame.bars.rows(BarsPerRow::One, SecondsPerRow::default())[0];
        let row = Row {
            from: span.from,
            start: span.start,
            to: span.to,
            rect: egui::Rect::from_min_size(Pos2::ZERO, vec2(1000.0, ROW)),
        };
        let mut window = TimingWindow {
            gesture: Gesture::DragAudio {
                tick: Tick::new(960),
                from: 0.5,
                to: 0.75,
                origin: 0.5,
            },
            ..TimingWindow::default()
        };
        let target = ghost(&window, &frame, &row, None, false).unwrap();
        assert!((target.x - row.x(&frame.bars, Tick::new(1440))).abs() < 1.0);
        assert!(target.snapped);
        assert_eq!(
            ghost(&window, &frame, &row, None, true),
            Some(Ghost {
                x: target.x,
                snapped: false
            })
        );
        window.gesture = pointer::Gesture::Idle;
        assert_eq!(
            anchor_near(&frame, &row, row.x(&frame.bars, Tick::new(960))),
            Some(Tick::new(960))
        );
    }

    #[test]
    fn middle_click_auditions_an_anchor_but_plays_from_empty_audio() {
        let mut harness = timing_harness();
        for (tick, expected) in [
            (
                960,
                Command::Audition(ResolvedTime::from_seconds(0.0).unwrap()),
            ),
            (2880, Command::PlayFrom(Tick::new(2880))),
        ] {
            let at = row_point(&harness, tick);
            harness.hover_at(at);
            for pressed in [true, false] {
                harness.event(egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Middle,
                    pressed,
                    modifiers: Modifiers::NONE,
                });
            }
            harness.step();
            assert_eq!(harness.state().commands.last(), Some(&expected));
        }
    }

    #[test]
    fn the_anchor_menu_plays_from_one_beat_before_the_chosen_anchor() {
        let mut harness = timing_harness();
        let at = row_point(&harness, 960);
        harness.hover_at(at);
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        harness.step();
        harness.get_by_label("Play from a beat before").click();
        harness.step();
        assert_eq!(
            harness.state().commands,
            [Command::Audition(ResolvedTime::from_seconds(0.0).unwrap())]
        );
    }

    #[test]
    fn an_anchor_menu_can_select_every_anchor_up_to_its_target() {
        let mut harness = timing_harness();
        let at = row_point(&harness, 960);
        harness.hover_at(at);
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        harness.step();
        harness.get_by_label("Select to the start").click();
        harness.step();
        assert_eq!(
            harness.state().window.selection,
            [Tick::new(0), Tick::new(960)]
        );
    }
}
