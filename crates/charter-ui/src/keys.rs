//! What the keyboard asks the editor to do: shortcuts by what a key means, and lane keys by where
//! they sit on the keyboard.

use charter_core::{LaneIndex, Tick};
use egui::{Key, KeyboardShortcut, Modifiers};

use charter_editor::Step;

use crate::notefield::Zoom;
use crate::{Action, Note};

pub(crate) const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
pub(crate) const REDO: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
/// Redo as Windows programs spell it.
const REDO_Y: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
pub(crate) const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
pub(crate) const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
pub(crate) const SAVE_AS: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
pub(crate) const SETTINGS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma);
pub(crate) const NEW_SONG: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
pub(crate) const BOOKMARK: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::B);
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
pub struct LaneKeys([LaneKey; LANE_KEYS.len()]);

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
pub fn key_actions(input: &egui::InputState, lanes: u16, keys: &mut LaneKeys) -> Vec<Action> {
    let mut actions = Vec::new();
    for event in &input.events {
        match *event {
            egui::Event::Key {
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
            _ => {}
        }
    }
    actions
}

/// What a key press asks for when the key is not placing a note.
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
    if is(NEW_SONG) {
        return Some(Action::NewSong);
    }
    if is(SETTINGS) {
        return Some(Action::Settings);
    }
    if is(BOOKMARK) {
        return Some(Action::ToggleBookmark);
    }
    if is(HELP) {
        return Some(Action::Help);
    }
    if modifiers.alt {
        return match key {
            Key::ArrowDown => Some(Action::JumpSection(Step::Later)),
            Key::ArrowUp => Some(Action::JumpSection(Step::Earlier)),
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
            _ => {}
        }
    }
    if modifiers.command {
        return match key {
            Key::Plus | Key::Equals => Some(Action::ZoomBy(Zoom::STEP)),
            Key::Minus => Some(Action::ZoomBy(Zoom::STEP.recip())),
            Key::A => Some(Action::SelectAll),
            _ => None,
        };
    }
    match key {
        Key::Delete | Key::Backspace => Some(Action::DeleteSelection),
        Key::Escape => Some(Action::ClearSelection),
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
