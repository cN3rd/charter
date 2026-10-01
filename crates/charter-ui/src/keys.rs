//! What the keyboard asks the editor to do: shortcuts by what a key means, and lane keys by where
//! they sit on the keyboard.

use charter_core::{LaneIndex, Tick};
use egui::{Key, KeyboardShortcut, Modifiers};

use charter_editor::Step;

use crate::notefield::Zoom;
use crate::{Action, Note};

/// Undo the latest edit with the platform's command modifier.
pub(crate) const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
/// Redo an edit with the shifted command key.
pub(crate) const REDO: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
/// Redo as Windows programs spell it.
const REDO_Y: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
/// Ask the host to open a song.
pub(crate) const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
/// Save the open song to its current file.
pub(crate) const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
/// Ask the host where to save the open song.
pub(crate) const SAVE_AS: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
/// Show the editor's settings window.
pub(crate) const SETTINGS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma);
/// Start a song from the new-song dialog.
pub(crate) const NEW_SONG: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
/// Add or remove a bookmark at the cursor.
pub(crate) const BOOKMARK: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::B);
/// Toggle the timing window.
pub(crate) const SYNC: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::T);
/// Toggle the keyboard shortcut help.
pub(crate) const HELP: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F1);

/// The keys that place notes on lanes 1 to 10, as StepMania and ArrowVortex bind them.
const LANE_KEYS: [Key; 10] = [
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

/// Where one lane key is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum LaneKey {
    /// Ready to place a note on the next press.
    #[default]
    Up,
    /// Down without having placed or removed a note: pressed with a modifier, or past the last
    /// lane.
    Down,
    /// Down since its press placed or removed a note.
    Toggled,
}

/// Every lane key, by its place in [`LANE_KEYS`]. Kept across frames, since the keys of a chord
/// go down and up frames apart, and kept by where each key sits on the keyboard: Shift can turn
/// the `1` a key went down as into the `!` it comes up as.
#[derive(Debug, Default)]
pub(crate) struct LaneKeys([LaneKey; LANE_KEYS.len()]);

impl LaneKeys {
    /// Pushes what the lane key at `lane` going down or up asks for: alone it places or removes a
    /// tap, and under Shift a mine. A key already down is repeating, whatever egui calls it, and
    /// toggles nothing.
    fn went(
        &mut self,
        lane: usize,
        pressed: bool,
        modifiers: Modifiers,
        lanes: u16,
        actions: &mut Vec<Action>,
    ) {
        let (Some(key), Ok(index)) = (self.0.get_mut(lane), u16::try_from(lane)) else {
            return;
        };
        let index = LaneIndex::new(index);
        if !pressed {
            if std::mem::take(key) == LaneKey::Toggled {
                actions.push(Action::LaneUp(index));
                if !self.0.contains(&LaneKey::Toggled) {
                    actions.push(Action::LanesReleased);
                }
            }
            return;
        }
        if *key != LaneKey::Up {
            return;
        }
        let note = match modifiers {
            _ if u16::from(index) >= lanes => None,
            Modifiers::NONE => Some(Note::Tap),
            Modifiers::SHIFT => Some(Note::Mine),
            _ => None,
        };
        *key = if note.is_some() {
            LaneKey::Toggled
        } else {
            LaneKey::Down
        };
        actions.extend(note.map(|note| Action::Toggle(index, note)));
    }

    /// Marks every lane key up, pushing what that asks for.
    fn release_all(&mut self, actions: &mut Vec<Action>) {
        let was = std::mem::take(&mut self.0);
        let toggled = (0..).zip(was).filter(|(_, key)| *key == LaneKey::Toggled);
        let before = actions.len();
        actions.extend(toggled.map(|(lane, _)| Action::LaneUp(LaneIndex::new(lane))));
        if actions.len() > before {
            actions.push(Action::LanesReleased);
        }
    }
}

/// What this frame's keys ask for, in the order they went down and up. Lane keys edit the first
/// `lanes` lanes of the open chart.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "other egui events do not trigger actions"
)]
pub(crate) fn key_actions(
    input: &egui::InputState,
    lanes: u16,
    keys: &mut LaneKeys,
) -> Vec<Action> {
    let mut actions = Vec::new();
    for event in &input.events {
        match event {
            &egui::Event::Key {
                key,
                physical_key,
                pressed,
                modifiers,
                ..
            } => {
                // What the key means comes first: some layouts put minus on a digit key, and the
                // numpad sends arrows from digit keys with Num Lock off.
                let command = if pressed {
                    command(key, modifiers)
                } else {
                    None
                };
                let place = physical_key.unwrap_or(key);
                let lane = LANE_KEYS.iter().position(|lane| *lane == place);
                match (command, lane) {
                    (Some(action), _) => actions.push(action),
                    (None, Some(lane)) => keys.went(lane, pressed, modifiers, lanes, &mut actions),
                    (None, None) => {}
                }
            }
            // No key comes up while the window is away, so every lane key counts as up.
            egui::Event::WindowFocused(false) => keys.release_all(&mut actions),
            egui::Event::Copy => actions.push(Action::Copy),
            egui::Event::Cut => actions.push(Action::Cut),
            egui::Event::Paste(text) => actions.push(Action::Paste(Some(text.clone()))),
            _ => {}
        }
    }
    actions
}

/// What a key press asks for when the key is not placing a note.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "unmapped egui keys do not trigger shortcuts"
)]
#[expect(clippy::too_many_lines, reason = "shortcut table grouped by modifiers")]
fn command(key: Key, modifiers: Modifiers) -> Option<Action> {
    let is = |shortcut: KeyboardShortcut| {
        key == shortcut.logical_key && modifiers.matches_logically(shortcut.modifiers)
    };
    // Redo first: its shortcut is undo's plus Shift, which undo's also matches.
    if is(REDO) || is(REDO_Y) {
        return Some(Action::Redo);
    }
    if is(UNDO) {
        return Some(Action::Undo);
    }
    // Save as first, for the same reason.
    if is(SAVE_AS) {
        return Some(Action::SaveAs);
    }
    if is(SAVE) {
        return Some(Action::Save);
    }
    if is(OPEN) {
        return Some(Action::Open);
    }
    if modifiers.command && modifiers.shift {
        match key {
            Key::N => return Some(Action::AddLayer),
            Key::H => return Some(Action::ToggleActiveLayerView),
            Key::ArrowUp => return Some(Action::StepLayer(Step::Earlier)),
            Key::ArrowDown => return Some(Action::StepLayer(Step::Later)),
            _ => {}
        }
    }
    if is(NEW_SONG) {
        return Some(Action::NewSong);
    }
    if is(SETTINGS) {
        return Some(Action::Settings);
    }
    if is(BOOKMARK) {
        return Some(Action::ToggleBookmark);
    }
    if is(SYNC) {
        return Some(Action::ToggleSync);
    }
    if is(HELP) {
        return Some(Action::Help);
    }
    if modifiers.alt {
        if !modifiers.command
            && let Some(index) = LANE_KEYS[..9].iter().position(|digit| *digit == key)
        {
            return Some(if modifiers.shift {
                Action::MoveSelectionToLayer(index)
            } else {
                Action::SetActiveLayer(index)
            });
        }
        let nudge = if modifiers.shift { 0.010 } else { 0.001 };
        return match key {
            Key::ArrowDown => Some(Action::JumpSection(Step::Later)),
            Key::ArrowUp => Some(Action::JumpSection(Step::Earlier)),
            Key::ArrowRight => Some(Action::NudgeOffset(nudge)),
            Key::ArrowLeft => Some(Action::NudgeOffset(-nudge)),
            _ => None,
        };
    }
    if modifiers.command && modifiers.shift {
        return match key {
            Key::ArrowRight => Some(Action::NudgeRate(0.01)),
            Key::ArrowLeft => Some(Action::NudgeRate(-0.01)),
            _ => None,
        };
    }
    if modifiers.shift {
        match key {
            Key::ArrowRight => return Some(Action::NudgeRate(0.1)),
            Key::ArrowLeft => return Some(Action::NudgeRate(-0.1)),
            Key::F4 => return Some(Action::ToggleTunedLanes),
            _ => {}
        }
    }
    if modifiers.command {
        return match key {
            Key::E => Some(Action::MergeActiveLayer),
            Key::Plus | Key::Equals => Some(Action::ZoomBy(Zoom::STEP)),
            Key::Minus => Some(Action::ZoomBy(Zoom::STEP.recip())),
            Key::A => Some(Action::SelectAll),
            Key::D => Some(Action::Duplicate),
            Key::M => Some(Action::Mirror),
            Key::ArrowLeft => Some(Action::ShiftSelection {
                snaps: 0,
                lanes: -1,
            }),
            Key::ArrowRight => Some(Action::ShiftSelection { snaps: 0, lanes: 1 }),
            Key::ArrowUp => Some(Action::ShiftSelection {
                snaps: -1,
                lanes: 0,
            }),
            Key::ArrowDown => Some(Action::ShiftSelection { snaps: 1, lanes: 0 }),
            Key::Backslash => Some(Action::ToggleSongPanel),
            _ => None,
        };
    }
    match key {
        Key::Delete | Key::Backspace => Some(Action::DeleteSelection),
        Key::Escape => Some(Action::ClearSelection),
        Key::G => Some(Action::Quantize),
        Key::Q => Some(Action::PlaySnippet(Step::Earlier)),
        Key::W => Some(Action::PlaySnippet(Step::Later)),
        Key::CloseBracket => Some(Action::ResizeNotes(Step::Later)),
        Key::OpenBracket => Some(Action::ResizeNotes(Step::Earlier)),
        Key::ArrowDown => Some(Action::Move(Step::Later)),
        Key::ArrowUp => Some(Action::Move(Step::Earlier)),
        Key::PageDown => Some(Action::MoveMeasure(Step::Later)),
        Key::PageUp => Some(Action::MoveMeasure(Step::Earlier)),
        Key::ArrowRight => Some(Action::FinerSnap),
        Key::ArrowLeft => Some(Action::CoarserSnap),
        Key::Space => Some(Action::TogglePlay),
        Key::Home => Some(Action::JumpTo(Tick::new(0))),
        Key::End => Some(Action::JumpEnd),
        Key::F3 => Some(Action::ToggleMetronome),
        Key::F4 => Some(Action::ToggleTicks),
        Key::F7 => Some(Action::NudgeBpm(-1.0)),
        Key::F8 => Some(Action::NudgeBpm(1.0)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;

    const NONE: Modifiers = Modifiers::NONE;

    #[test]
    fn modifiers_choose_between_moving_the_snap_the_rate_and_sections() {
        let shift = Modifiers::SHIFT;
        let both = Modifiers::COMMAND.plus(Modifiers::SHIFT);
        let cases = [
            (Key::ArrowRight, NONE, Some(Action::FinerSnap)),
            (Key::ArrowLeft, NONE, Some(Action::CoarserSnap)),
            (Key::ArrowRight, shift, Some(Action::NudgeRate(0.1))),
            (Key::ArrowLeft, shift, Some(Action::NudgeRate(-0.1))),
            (Key::ArrowRight, both, Some(Action::NudgeRate(0.01))),
            (
                Key::ArrowDown,
                Modifiers::ALT,
                Some(Action::JumpSection(Step::Later)),
            ),
            (
                Key::ArrowUp,
                Modifiers::ALT,
                Some(Action::JumpSection(Step::Earlier)),
            ),
            (Key::ArrowDown, NONE, Some(Action::Move(Step::Later))),
            (Key::Z, both, Some(Action::Redo)),
            (Key::Z, Modifiers::COMMAND, Some(Action::Undo)),
            (Key::S, both, Some(Action::SaveAs)),
            (Key::Space, NONE, Some(Action::TogglePlay)),
            (Key::F4, NONE, Some(Action::ToggleTicks)),
            (Key::F4, shift, Some(Action::ToggleTunedLanes)),
            (
                Key::CloseBracket,
                NONE,
                Some(Action::ResizeNotes(Step::Later)),
            ),
            (
                Key::OpenBracket,
                NONE,
                Some(Action::ResizeNotes(Step::Earlier)),
            ),
            (Key::A, Modifiers::ALT, None),
        ];
        for (key, modifiers, expected) in cases {
            assert_eq!(command(key, modifiers), expected, "{key:?} {modifiers:?}");
        }
    }

    #[test]
    fn layer_shortcuts_do_not_place_lane_notes() {
        let both = Modifiers::COMMAND.plus(Modifiers::SHIFT);
        assert_eq!(command(Key::N, both), Some(Action::AddLayer));
        assert_eq!(
            command(Key::E, Modifiers::COMMAND),
            Some(Action::MergeActiveLayer)
        );
        assert_eq!(command(Key::H, both), Some(Action::ToggleActiveLayerView));
        assert_eq!(
            command(Key::ArrowUp, both),
            Some(Action::StepLayer(Step::Earlier))
        );
        assert_eq!(
            command(Key::Num2, Modifiers::ALT),
            Some(Action::SetActiveLayer(1))
        );
        assert_eq!(
            command(Key::Num2, Modifiers::ALT.plus(Modifiers::SHIFT)),
            Some(Action::MoveSelectionToLayer(1))
        );
    }

    #[test]
    fn a_lane_key_under_alt_places_nothing_and_ends_no_chord() {
        let mut keys = LaneKeys::default();
        let mut actions = Vec::new();
        keys.went(0, true, Modifiers::ALT, 4, &mut actions);
        keys.went(0, false, NONE, 4, &mut actions);
        assert_eq!(actions, []);
    }

    #[test]
    fn a_lane_key_past_the_charts_lanes_places_nothing() {
        let mut keys = LaneKeys::default();
        let mut actions = Vec::new();
        keys.went(5, true, NONE, 4, &mut actions);
        assert_eq!(actions, []);
    }

    #[test]
    fn every_shortcut_dispatches_its_distinct_command() {
        let cmd = Modifiers::COMMAND;
        let shift = Modifiers::SHIFT;
        let alt = Modifiers::ALT;
        let both = cmd.plus(shift);
        let cases = [
            (Key::Y, cmd, Action::Redo),
            (Key::O, cmd, Action::Open),
            (Key::N, cmd, Action::NewSong),
            (Key::Comma, cmd, Action::Settings),
            (Key::B, cmd, Action::ToggleBookmark),
            (Key::T, cmd, Action::ToggleSync),
            (Key::F1, NONE, Action::Help),
            (Key::ArrowRight, alt, Action::NudgeOffset(0.001)),
            (Key::ArrowLeft, alt, Action::NudgeOffset(-0.001)),
            (Key::ArrowRight, alt.plus(shift), Action::NudgeOffset(0.010)),
            (Key::ArrowLeft, alt.plus(shift), Action::NudgeOffset(-0.010)),
            (Key::ArrowLeft, both, Action::NudgeRate(-0.01)),
            (Key::A, cmd, Action::SelectAll),
            (Key::D, cmd, Action::Duplicate),
            (Key::M, cmd, Action::Mirror),
            (Key::Plus, cmd, Action::ZoomBy(Zoom::STEP)),
            (Key::Equals, cmd, Action::ZoomBy(Zoom::STEP)),
            (Key::Minus, cmd, Action::ZoomBy(Zoom::STEP.recip())),
            (
                Key::ArrowLeft,
                cmd,
                Action::ShiftSelection {
                    snaps: 0,
                    lanes: -1,
                },
            ),
            (
                Key::ArrowRight,
                cmd,
                Action::ShiftSelection { snaps: 0, lanes: 1 },
            ),
            (
                Key::ArrowUp,
                cmd,
                Action::ShiftSelection {
                    snaps: -1,
                    lanes: 0,
                },
            ),
            (
                Key::ArrowDown,
                cmd,
                Action::ShiftSelection { snaps: 1, lanes: 0 },
            ),
            (Key::Backslash, cmd, Action::ToggleSongPanel),
            (Key::Delete, NONE, Action::DeleteSelection),
            (Key::Backspace, NONE, Action::DeleteSelection),
            (Key::Escape, NONE, Action::ClearSelection),
            (Key::G, NONE, Action::Quantize),
            (Key::Q, NONE, Action::PlaySnippet(Step::Earlier)),
            (Key::W, NONE, Action::PlaySnippet(Step::Later)),
            (Key::ArrowUp, NONE, Action::Move(Step::Earlier)),
            (Key::PageDown, NONE, Action::MoveMeasure(Step::Later)),
            (Key::PageUp, NONE, Action::MoveMeasure(Step::Earlier)),
            (Key::Home, NONE, Action::JumpTo(Tick::new(0))),
            (Key::End, NONE, Action::JumpEnd),
            (Key::F3, NONE, Action::ToggleMetronome),
            (Key::F7, NONE, Action::NudgeBpm(-1.0)),
            (Key::F8, NONE, Action::NudgeBpm(1.0)),
        ];
        for (key, modifiers, expected) in cases {
            assert_eq!(
                command(key, modifiers),
                Some(expected),
                "{key:?} {modifiers:?}"
            );
        }
        assert_eq!(command(Key::A, alt), None);
    }

    #[test]
    fn physical_digit_keys_place_mines_even_when_shift_changes_the_symbol() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, (keys, actions): &mut (LaneKeys, Vec<Action>)| {
                actions.extend(ui.input(|input| key_actions(input, 10, keys)));
            },
            (LaneKeys::default(), Vec::new()),
        );
        harness.run();
        let event = |key, physical_key, pressed, modifiers| egui::Event::Key {
            key,
            physical_key,
            pressed,
            repeat: false,
            modifiers,
        };
        harness.event(event(
            Key::Exclamationmark,
            Some(Key::Num1),
            true,
            Modifiers::SHIFT,
        ));
        harness.event(event(Key::Num2, None, true, NONE));
        harness.event(event(Key::Num2, None, true, NONE));
        harness.run();
        assert_eq!(
            harness.state().1,
            [
                Action::Toggle(LaneIndex::new(0), Note::Mine),
                Action::Toggle(LaneIndex::new(1), Note::Tap)
            ]
        );
        harness.state_mut().1.clear();
        harness.event(event(Key::Num1, Some(Key::Num1), false, NONE));
        harness.event(event(Key::Num2, None, false, NONE));
        harness.run();
        assert_eq!(
            harness.state().1,
            [
                Action::LaneUp(LaneIndex::new(0)),
                Action::LaneUp(LaneIndex::new(1)),
                Action::LanesReleased
            ]
        );
    }

    #[test]
    fn focus_loss_releases_a_chord_and_clipboard_events_keep_their_payload() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, (keys, actions): &mut (LaneKeys, Vec<Action>)| {
                actions.extend(ui.input(|input| key_actions(input, 4, keys)));
            },
            (LaneKeys::default(), Vec::new()),
        );
        harness.run();
        harness.key_down(Key::Num1);
        harness.run();
        harness.state_mut().1.clear();
        harness.event(egui::Event::WindowFocused(false));
        harness.event(egui::Event::Copy);
        harness.event(egui::Event::Cut);
        harness.event(egui::Event::Paste("notes".to_owned()));
        harness.run();
        assert_eq!(
            harness.state().1,
            [
                Action::LaneUp(LaneIndex::new(0)),
                Action::LanesReleased,
                Action::Copy,
                Action::Cut,
                Action::Paste(Some("notes".to_owned()))
            ]
        );
        harness.state_mut().1.clear();
        harness.event(egui::Event::WindowFocused(false));
        harness.run();
        assert!(harness.state().1.is_empty());
    }

    #[test]
    fn unknown_lanes_and_modifier_combinations_do_not_create_actions() {
        let mut keys = LaneKeys::default();
        let mut actions = Vec::new();
        keys.went(10, true, NONE, 10, &mut actions);
        keys.went(0, true, Modifiers::COMMAND, 10, &mut actions);
        keys.went(0, false, NONE, 10, &mut actions);
        assert!(actions.is_empty());
        assert_eq!(
            command(Key::F2, Modifiers::COMMAND.plus(Modifiers::SHIFT)),
            None
        );
        assert_eq!(command(Key::F2, Modifiers::COMMAND), None);
        assert_eq!(command(Key::F2, Modifiers::ALT), None);
    }
}

/// Proofs for Kani, run with `cargo kani -p charter-ui`.
#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    #[kani::unwind(12)]
    fn every_digit_key_maps_back_to_its_lane_index() {
        let lane: usize = usize::from(kani::any::<u8>());
        if let Some(&key) = LANE_KEYS.get(lane) {
            assert_eq!(
                LANE_KEYS.iter().position(|&candidate| candidate == key),
                Some(lane)
            );
        }
    }
}
