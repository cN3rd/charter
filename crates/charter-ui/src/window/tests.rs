//! Tests for the window module.

use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use std::error::Error;

use charter_core::{Chart, LaneCount, LaneIndex, LaneSpan, Position, Tick, Valid, ValidDocument};
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use super::*;
use crate::flip::Flip;
use crate::recovery;
use crate::settings::Sound;
use charter_audio::{Gain, Kit, Tone, Volumes};

/// The format chapter, embedded at compile time so the tests do no I/O.
const SPEC: &str = include_str!("../../../../docs/src/format.md");

/// The format chapter's full example song.
fn example() -> &'static [u8] {
    let start = SPEC
        .rfind("```json\n")
        .unwrap()
        .saturating_add("```json\n".len());
    let end = start.saturating_add(
        SPEC.as_bytes()[start..]
            .windows(3)
            .position(|part| part == b"```")
            .unwrap(),
    );
    &SPEC.as_bytes()[start..end]
}

fn window(app: Window) -> Harness<'static, Window> {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut Window| app.ui(ui), app);
    harness.run();
    harness
}

fn with_demo() -> Harness<'static, Window> {
    let mut app = Window::default();
    app.open_demo();
    window(app)
}

/// A file dropped on the window, held in memory.
#[derive(Debug)]
struct Dropped(&'static str, Vec<u8>);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        Path::new(self.0)
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        Ok(self.1.clone())
    }
}

/// Accepts every `sm` document, standing in for the StepMania plugin.
#[derive(Debug)]
struct AnySm;

#[expect(
    clippy::missing_trait_methods,
    reason = "a test plugin: the trait's defaults for every optional method"
)]
impl RulesetPlugin for AnySm {
    fn id(&self) -> &'static str {
        "sm"
    }

    fn check(&self, _: &ValidDocument) -> Result<(), Box<dyn Error + Send + Sync>> {
        Ok(())
    }
}

/// Refuses an `sm` document with a note on the fourth lane, which no format rule does.
#[derive(Debug)]
struct NoFourthLane;

#[expect(
    clippy::missing_trait_methods,
    reason = "a test plugin: the trait's defaults for every optional method"
)]
impl RulesetPlugin for NoFourthLane {
    fn id(&self) -> &'static str {
        "sm"
    }

    fn check(&self, document: &ValidDocument) -> Result<(), Box<dyn Error + Send + Sync>> {
        let fourth = Some(LaneSpan::new(LaneIndex::new(3), LaneCount::ONE));
        let on_fourth = |chart: Valid<'_, Chart>| {
            chart
                .objects()
                .any(|object| object.position.lanes() == fourth)
        };
        if document.charts().any(on_fourth) {
            return Err("a note on lane 4".into());
        }
        Ok(())
    }
}

const PLUGINS: &[&dyn RulesetPlugin] = &[&AnySm];

fn editable_demo() -> Harness<'static, Window> {
    let mut app = Window::new(PLUGINS);
    app.open_demo();
    window(app)
}

/// The lanes of the open chart's objects on `tick`, lowest first.
fn lanes_at(harness: &Harness<'_, Window>, tick: i32) -> Vec<u16> {
    let song = harness.state().song.as_ref().unwrap();
    let chart = song.chart().unwrap();
    let mut lanes: Vec<u16> = chart
        .objects
        .iter()
        .filter(|object| object.tick == Tick::new(tick))
        .filter_map(|object| match object.position {
            Position::Lane { index, .. } => Some(u16::from(index)),
            Position::Plane { .. } => None,
        })
        .collect();
    lanes.sort_unstable();
    lanes
}

#[test]
fn hidden_notes_stop_ticking_but_the_metronome_keeps_playing() {
    let mut song = Song::new(charter_editor::demo::song(), PLUGINS);
    let settings = Settings {
        note_ticks: true,
        ..Settings::default()
    };
    let before = clicks(
        &song,
        &settings,
        false,
        (0.0, 30.0),
        charter_timing::grid::Grid::default(),
    );
    assert!(!before.is_empty());
    song.set_layer_view(0, charter_core::LayerView::Hidden)
        .unwrap();
    let after = clicks(
        &song,
        &settings,
        false,
        (0.0, 30.0),
        charter_timing::grid::Grid::default(),
    );
    assert!(after.is_empty());
    let metronome = clicks(
        &song,
        &settings,
        true,
        (0.0, 1.0),
        charter_timing::grid::Grid::default(),
    );
    assert!(!metronome.is_empty());
}

#[test]
fn panel_layer_edits_each_undo_as_one_step() {
    let edits = [
        Action::RenameLayer(0, "Kick".into()),
        Action::RecolourLayer(0, charter_core::LayerColour::Pink),
        Action::SetLayerView(0, charter_core::LayerView::Ghosted),
        Action::SetLayerLocked(0, true),
        Action::SoloLayer(0),
        Action::MoveLayer(0, 1),
        Action::MergeLayer(0),
        Action::DeleteLayer(0, charter_editor::LayerNotes::Delete),
    ];
    for action in edits {
        let mut app = Window::new(PLUGINS);
        app.open_demo();
        app.apply(Action::AddLayer);
        let before = app.song.as_ref().unwrap().document().to_json().unwrap();
        app.apply(action.clone());
        assert!(app.failure.is_none(), "{action:?}: {:?}", app.failure);
        app.apply(Action::Undo);
        assert_eq!(
            app.song.as_ref().unwrap().document().to_json().unwrap(),
            before,
            "{action:?}"
        );
    }
}

#[test]
fn add_layer_button_and_shortcut_both_create_undoable_layers() {
    let mut harness = editable_demo();
    harness.get_by_label("Add layer").click();
    harness.run();
    assert_eq!(harness.state().song.as_ref().unwrap().layers().len(), 2);
    harness.key_press_modifiers(
        egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
        egui::Key::N,
    );
    harness.run();
    assert_eq!(harness.state().song.as_ref().unwrap().layers().len(), 3);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(harness.state().song.as_ref().unwrap().layers().len(), 2);
}

#[test]
fn alt_digits_switch_layers_and_move_notes_without_placing_lane_taps() {
    let mut harness = editable_four_taps();
    harness.state_mut().apply(Action::AddLayer);
    let before = note_count(&harness);
    harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::Num2);
    harness.run();
    assert_eq!(harness.state().song.as_ref().unwrap().active_layer(), 1);
    assert!(harness.query_by_label("Layer 0").is_some());
    select_note(&mut harness, 3, 720);
    harness.key_press_modifiers(
        egui::Modifiers::ALT.plus(egui::Modifiers::SHIFT),
        egui::Key::Num1,
    );
    harness.run();
    let song = harness.state().song.as_ref().unwrap();
    assert_eq!(song.layers()[0].notes, 1);
    assert_eq!(note_count(&harness), before);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(harness.state().song.as_ref().unwrap().layers()[0].notes, 0);
}

#[test]
fn view_menu_toggles_the_persisted_layer_tint_setting() {
    let mut harness = editable_demo();
    assert!(!harness.state().settings.colour_notes_by_layer);
    harness.get_by_label("View").click();
    harness.run();
    harness.get_by_label("Colour notes by layer").click();
    harness.run();
    let settings = &harness.state().settings;
    assert!(settings.colour_notes_by_layer);
    assert!(Settings::from_kept(&settings.to_kept()).colour_notes_by_layer);
}

#[test]
fn copying_a_layer_to_another_chart_is_one_undo_step() {
    let mut app = Window::new(PLUGINS);
    app.open_demo();
    app.apply(Action::AddLayer);
    let before = app.song.as_ref().unwrap().document().to_json().unwrap();
    app.apply(Action::CopyLayerToChart(1, 1));
    assert_ne!(
        app.song.as_ref().unwrap().document().to_json().unwrap(),
        before
    );
    app.apply(Action::Undo);
    assert_eq!(
        app.song.as_ref().unwrap().document().to_json().unwrap(),
        before
    );
}

#[test]
fn hidden_or_locked_layers_contribute_no_notes_to_select_all() {
    for action in [
        Action::SetLayerView(1, charter_core::LayerView::Hidden),
        Action::SetLayerLocked(1, true),
    ] {
        let mut harness = editable_four_taps();
        harness.state_mut().apply(Action::AddLayer);
        harness.state_mut().apply(action);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.run();
        assert_eq!(
            harness.state().song.as_ref().unwrap().selection().count(),
            0
        );
    }
}

fn cursor_shows(harness: &Harness<'_, Window>, position: &str) -> bool {
    harness
        .query_by_label(&format!("Cursor at {position}"))
        .is_some()
}

#[test]
fn a_song_its_plugin_accepts_opens_for_editing() {
    let harness = editable_demo();
    assert!(harness.query_by_label("Editing").is_some());
    assert!(harness.query_by_label_contains("Read only").is_none());
    let song = harness.state().song.as_ref().unwrap();
    assert_eq!(song.layers()[0].name, "Layer 0");
    assert!(!song.is_modified());
    assert!(!song.can_undo());
}

#[test]
fn a_song_without_its_plugin_is_read_only_and_names_the_plugin() {
    let mut app = Window::new(PLUGINS);
    app.open(Path::new("example.json"), example());
    let harness = window(app);
    assert!(
        harness
            .query_by_label("Read only: charter has no arcaea plugin")
            .is_some()
    );
    assert!(harness.query_by_label("Read only").is_some());
}

#[test]
#[expect(
    clippy::non_ascii_literal,
    reason = "the test locates the Choose audio menu label with its ellipsis"
)]
fn read_only_songs_disable_menu_items_that_would_edit_them() {
    let mut app = Window::new(PLUGINS);
    app.open(Path::new("example.json"), example());
    let mut harness = window(app);

    harness.get_by_label("File").click();
    harness.run();
    assert!(
        harness
            .get_by_label("Choose audio…")
            .accesskit_node()
            .is_disabled()
    );
    harness.get_by_label("Timing").click();
    harness.run();
    assert!(
        harness
            .get_by_label_contains("Raise the tempo")
            .accesskit_node()
            .is_disabled()
    );
    assert!(
        !harness
            .get_by_label_contains("Timing window")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn a_lane_key_places_a_tap_at_the_cursor_and_moves_on_a_snap() {
    let mut harness = editable_demo();
    harness.key_press(egui::Key::Num1);
    harness.run();
    assert_eq!(lanes_at(&harness, 0), [0, 3]);
    assert!(cursor_shows(&harness, "0:0:240"));
}

#[test]
fn lane_keys_held_together_place_a_chord_and_move_on_once_all_are_up() {
    let mut harness = editable_demo();
    harness.key_down(egui::Key::Num1);
    harness.key_down(egui::Key::Num2);
    harness.run();
    harness.key_up(egui::Key::Num1);
    harness.run();
    assert!(cursor_shows(&harness, "0:0:0"));
    harness.key_up(egui::Key::Num2);
    harness.run();
    assert_eq!(lanes_at(&harness, 0), [0, 1, 3]);
    assert!(cursor_shows(&harness, "0:0:240"));
}

/// Presses and releases each of `keys` in turn, all in the next frame, as a fast typist does
/// when frames are slow. Queued events would each get a frame of their own.
fn tap_in_one_frame(harness: &mut Harness<'_, Window>, keys: &[egui::Key]) {
    for &key in keys {
        for pressed in [true, false] {
            harness.input_mut().events.push(egui::Event::Key {
                key,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
                physical_key: None,
            });
        }
    }
}

#[test]
fn a_lane_key_tapped_twice_in_one_frame_places_two_notes_down_the_lane() {
    let mut harness = editable_demo();
    tap_in_one_frame(&mut harness, &[egui::Key::Num1, egui::Key::Num1]);
    harness.run();
    assert_eq!(lanes_at(&harness, 0), [0, 3]);
    assert_eq!(lanes_at(&harness, 240), [0]);
    assert!(cursor_shows(&harness, "0:0:480"));
}

#[test]
fn two_lane_keys_tapped_in_one_frame_make_a_stream_not_a_chord() {
    let mut harness = editable_demo();
    tap_in_one_frame(&mut harness, &[egui::Key::Num1, egui::Key::Num2]);
    harness.run();
    assert_eq!(lanes_at(&harness, 0), [0, 3]);
    assert_eq!(lanes_at(&harness, 240), [1]);
}

#[test]
fn keys_in_one_frame_act_in_the_order_they_were_pressed() {
    let mut harness = editable_demo();
    tap_in_one_frame(&mut harness, &[egui::Key::Num1, egui::Key::ArrowDown]);
    harness.run();
    assert_eq!(lanes_at(&harness, 0), [0, 3]);
    assert!(cursor_shows(&harness, "0:0:480"));
}

#[test]
fn a_lane_key_released_under_ctrl_still_moves_the_cursor_on() {
    let mut harness = editable_demo();
    harness.key_down(egui::Key::Num1);
    harness.run();
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::COMMAND));
    harness.event(egui::Event::Key {
        key: egui::Key::Num1,
        pressed: false,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
        physical_key: None,
    });
    harness.run();
    assert!(cursor_shows(&harness, "0:0:240"));
}

/// A key event as a keyboard sends it: what the layout makes of the key under `modifiers`,
/// and where the key sits.
fn keyboard(
    key: egui::Key,
    place: egui::Key,
    pressed: bool,
    modifiers: egui::Modifiers,
) -> egui::Event {
    egui::Event::Key {
        key,
        pressed,
        repeat: false,
        modifiers,
        physical_key: Some(place),
    }
}

#[test]
fn a_lane_key_released_under_shift_still_moves_the_cursor_on() {
    let mut harness = editable_demo();
    let shift = egui::Modifiers::SHIFT;
    harness.event(keyboard(
        egui::Key::Num1,
        egui::Key::Num1,
        true,
        egui::Modifiers::NONE,
    ));
    harness.run();
    harness.event(egui::Event::ModifiersChanged(shift));
    harness.event(keyboard(
        egui::Key::Exclamationmark,
        egui::Key::Num1,
        false,
        shift,
    ));
    harness.run();
    assert!(cursor_shows(&harness, "0:0:240"));
}

#[test]
fn a_lane_key_held_through_shift_coming_up_repeats_without_toggling() {
    let mut harness = editable_demo();
    shift_down(&harness, egui::Key::Exclamationmark, egui::Key::Num1);
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    let none = egui::Modifiers::NONE;
    harness.event(keyboard(egui::Key::Num1, egui::Key::Num1, true, none));
    harness.run();
    assert_eq!(note_at(&harness, 0, 0), Some(mine()));
}

#[test]
fn ctrl_minus_zooms_out_where_the_layout_puts_minus_on_a_digit_key() {
    let mut harness = editable_demo();
    let command = egui::Modifiers::COMMAND;
    harness.event(egui::Event::ModifiersChanged(command));
    harness.event(keyboard(egui::Key::Minus, egui::Key::Num6, true, command));
    harness.run();
    assert!(harness.query_by_label("Zoom 80%").is_some());
}

#[test]
fn a_numpad_arrow_moves_the_cursor_rather_than_placing_a_note() {
    let mut harness = editable_demo();
    let none = egui::Modifiers::NONE;
    harness.event(keyboard(egui::Key::ArrowDown, egui::Key::Num2, true, none));
    harness.event(keyboard(egui::Key::ArrowDown, egui::Key::Num2, false, none));
    harness.run();
    assert!(cursor_shows(&harness, "0:0:240"));
    assert_eq!(lanes_at(&harness, 0), [3]);
}

#[test]
fn a_lane_key_released_while_a_widget_has_focus_still_moves_the_cursor_on() {
    let mut harness = editable_demo();
    harness.key_down(egui::Key::Num1);
    harness.run();
    harness.key_press(egui::Key::Tab);
    harness.run();
    assert!(harness.ctx.egui_wants_keyboard_input());
    harness.key_up(egui::Key::Num1);
    harness.run();
    assert!(cursor_shows(&harness, "0:0:240"));
}

#[test]
fn losing_focus_with_a_lane_key_down_moves_on_as_if_it_came_up() {
    let mut harness = editable_demo();
    harness.key_down(egui::Key::Num1);
    harness.run();
    harness.event(egui::Event::WindowFocused(false));
    harness.run();
    assert!(cursor_shows(&harness, "0:0:240"));
    harness.event(egui::Event::WindowFocused(true));
    harness.key_press(egui::Key::Num1);
    harness.run();
    assert_eq!(lanes_at(&harness, 0), [0, 3]);
    assert_eq!(lanes_at(&harness, 240), [0]);
}

#[test]
fn undoing_a_note_whose_key_is_still_down_keeps_the_cursor_when_it_comes_up() {
    let mut harness = editable_demo();
    harness.key_down(egui::Key::Num1);
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    harness.key_up(egui::Key::Num1);
    harness.run();
    assert_eq!(lanes_at(&harness, 0), [3]);
    assert!(cursor_shows(&harness, "0:0:0"));
}

#[test]
fn opening_a_song_with_a_lane_key_down_leaves_its_cursor_alone() {
    let mut harness = editable_demo();
    harness.key_down(egui::Key::Num1);
    harness.run();
    harness.state_mut().open_demo();
    harness.run();
    harness.get_by_label("Don't save").click();
    harness.key_up(egui::Key::Num1);
    harness.run();
    assert!(cursor_shows(&harness, "0:0:0"));
}

#[test]
fn keys_wait_while_the_window_asks_about_unsaved_changes() {
    let mut harness = edited_four_taps();
    harness
        .state_mut()
        .open(Path::new("example.json"), example());
    harness.run();
    harness.key_press(egui::Key::Num3);
    harness.run();
    assert_eq!(note_count(&harness), 5);
}

/// What starts on `lane` at `tick` in the open chart, as the format writes it.
fn note_at(harness: &Harness<'_, Window>, tick: i32, lane: u16) -> Option<serde_json::Value> {
    let song = harness.state().song.as_ref()?;
    let chart = song.chart()?;
    chart
            .objects
            .iter()
            .find(|object| {
                object.tick == Tick::new(tick)
                    && matches!(object.position, Position::Lane { index, .. } if index == LaneIndex::new(lane))
            })
            .map(|object| serde_json::json!({ "kind": object.kind.to_string(), "shape": object.shape }))
}

fn tap() -> serde_json::Value {
    serde_json::json!({ "kind": "tap", "shape": "instant" })
}

fn hold(duration: u32) -> serde_json::Value {
    serde_json::json!({ "kind": "hold", "shape": { "sustain": { "duration": duration } } })
}

fn mine() -> serde_json::Value {
    serde_json::json!({ "kind": "mine", "shape": "instant" })
}

/// Presses the lane key for `lane` under Shift, which a US layout reports as a symbol.
fn shift_down(harness: &Harness<'_, Window>, symbol: egui::Key, place: egui::Key) {
    let shift = egui::Modifiers::SHIFT;
    harness.event(egui::Event::ModifiersChanged(shift));
    harness.event(keyboard(symbol, place, true, shift));
}

#[test]
fn shift_and_a_lane_key_place_a_mine_and_move_on() {
    let mut harness = editable_demo();
    shift_down(&harness, egui::Key::Exclamationmark, egui::Key::Num1);
    harness.event(keyboard(
        egui::Key::Exclamationmark,
        egui::Key::Num1,
        false,
        egui::Modifiers::SHIFT,
    ));
    harness.run();
    assert_eq!(note_at(&harness, 0, 0), Some(mine()));
    assert!(cursor_shows(&harness, "0:0:240"));
}

#[test]
fn holding_a_lane_key_while_the_cursor_moves_draws_a_hold() {
    let mut harness = editable_demo();
    harness.key_down(egui::Key::Num1);
    harness.key_press(egui::Key::ArrowDown);
    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    assert_eq!(note_at(&harness, 0, 0), Some(hold(480)));
    harness.key_up(egui::Key::Num1);
    harness.run();
    assert!(cursor_shows(&harness, "0:0:720"));
}

#[test]
fn a_hold_drawn_with_a_lane_key_undoes_in_one_step() {
    let mut harness = editable_demo();
    harness.key_down(egui::Key::Num1);
    harness.key_press(egui::Key::ArrowDown);
    harness.key_up(egui::Key::Num1);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(note_at(&harness, 0, 0), None);
    assert!(cursor_shows(&harness, "0:0:0"));
}

#[test]
fn moving_back_to_where_the_key_went_down_makes_the_hold_a_tap_again() {
    let mut harness = editable_demo();
    harness.key_down(egui::Key::Num1);
    harness.key_press(egui::Key::ArrowDown);
    harness.key_press(egui::Key::ArrowUp);
    harness.run();
    assert_eq!(note_at(&harness, 0, 0), Some(tap()));
}

#[test]
fn a_mine_does_not_stretch() {
    let mut harness = editable_demo();
    shift_down(&harness, egui::Key::Exclamationmark, egui::Key::Num1);
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    assert_eq!(note_at(&harness, 0, 0), Some(mine()));
}

/// An `sm` song with one four-lane chart of taps walking across the lanes a sixteenth apart.
fn four_taps() -> Vec<u8> {
    let tap = |tick: i32, lane: u16| {
        serde_json::json!({ "tick": tick, "position": { "lane": { "index": lane } },
                                "kind": "tap", "shape": "instant" })
    };
    let song = serde_json::json!({
        "format_version": 0,
        "ruleset": { "id": "sm", "version": "1" },
        "metadata": { "title": "Four", "artist": "a", "audio": "a.ogg" },
        "timing": { "offset_seconds": 0.0, "events": [ { "tick": 0, "kind": { "bpm": 120.0 } } ] },
        "charts": [ { "name": "Easy", "lanes": 4,
                      "objects": [tap(0, 0), tap(240, 1), tap(480, 2), tap(720, 3)] } ]
    });
    serde_json::to_vec(&song).unwrap()
}

fn editable_four_taps() -> Harness<'static, Window> {
    let mut app = Window::new(PLUGINS);
    app.open(Path::new("four.json"), &four_taps());
    window(app)
}

fn note_count(harness: &Harness<'_, Window>) -> usize {
    let song = harness.state().song.as_ref().unwrap();
    song.chart().unwrap().objects.len()
}

#[test]
fn brackets_lengthen_the_note_at_the_cursor_to_a_hold_and_shorten_it_back() {
    let mut harness = editable_four_taps();
    let first = |view: &Harness<'_, Window>| {
        let song = view.state().song.as_ref().unwrap();
        song.chart().unwrap().objects.first().unwrap().kind.clone()
    };
    harness.key_press(egui::Key::CloseBracket);
    harness.run();
    assert_eq!(first(&harness), charter_core::Kind::Hold);
    harness.key_press(egui::Key::OpenBracket);
    harness.run();
    assert_eq!(first(&harness), charter_core::Kind::Tap);
}

#[test]
fn clicking_the_minimap_moves_the_cursor_there_and_the_setting_hides_it() {
    let mut harness = editable_four_taps();
    let map = harness.get_by_label("Minimap").rect();
    // The last tap starts at tick 720 of a map running to its end.
    click(
        &mut harness,
        egui::pos2(map.center().x, map.bottom() - 1.0),
        egui::Modifiers::NONE,
    );
    let cursor = harness.state().song.as_ref().unwrap().cursor();
    assert!(cursor > Tick::new(0), "{cursor:?}");
    let mut app = Window::new(PLUGINS).with_settings(Settings {
        minimap: false,
        ..Settings::default()
    });
    app.open(Path::new("four.json"), &four_taps());
    let open_harness = window(app);
    assert!(open_harness.query_by_label("Minimap").is_none());
}

#[test]
fn ctrl_a_selects_the_chart_and_delete_removes_it_in_one_step() {
    let mut harness = editable_four_taps();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run();
    assert!(harness.query_by_label("4 notes selected").is_some());
    harness.key_press(egui::Key::Delete);
    harness.run();
    assert_eq!(note_count(&harness), 0);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(note_count(&harness), 4);
}

fn nothing_selected(harness: &Harness<'_, Window>) -> bool {
    harness.query_by_label_contains("selected").is_none()
}

#[test]
fn the_status_bar_counts_the_selection() {
    let mut harness = editable_four_taps();
    assert!(nothing_selected(&harness));
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run();
    assert!(harness.query_by_label("4 notes selected").is_some());
}

/// Types `text` over the song panel's field `label`, then presses Enter.
fn retype(harness: &mut Harness<'_, Window>, label: &str, text: &str) {
    harness
        .get_by_role_and_label(Role::TextInput, label)
        .focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.key_press(egui::Key::Backspace);
    harness
        .get_by_role_and_label(Role::TextInput, label)
        .type_text(text);
    harness.key_press(egui::Key::Enter);
    harness.run();
}

fn metadata(harness: &Harness<'_, Window>) -> charter_core::Metadata {
    let song = harness.state().song.as_ref().unwrap();
    song.document().document().metadata.clone()
}

fn picture(bytes: &[u8]) -> Picture {
    (
        bytes.to_vec(),
        egui::ColorImage::filled([4, 4], egui::Color32::RED),
    )
}

fn artwork_name(harness: &Harness<'_, Window>) -> Option<String> {
    metadata(harness).artwork.map(|path| path.to_string())
}

#[test]
fn chosen_artwork_previews_and_is_written_beside_the_song_on_save() {
    let mut harness = four_taps_at("/songs/Four/four.json");
    harness.get_by_label("Choose the song's artwork").click();
    harness.run();
    assert_eq!(request(&mut harness), Request::ChooseArtwork);
    harness
        .state_mut()
        .use_artwork(Path::new("/pictures/art.png"), Some(picture(b"png")));
    harness.run();
    assert_eq!(artwork_name(&harness).as_deref(), Some("art.png"));
    assert!(
        harness
            .query_by_label("Not a picture beside the song")
            .is_none()
    );
    let saving = Path::new("/songs/Four/four.json");
    assert_eq!(
        harness.state().artwork_write_for(saving),
        Some((PathBuf::from("/songs/Four/art.png"), b"png".to_vec()))
    );
    harness.state_mut().saved(Saved::To(saving.to_path_buf()));
    assert_eq!(harness.state().artwork_write_for(saving), None);
}

#[test]
fn artwork_named_before_the_first_save_is_read_once_the_song_is_saved() {
    let mut harness = created();
    retype(&mut harness, "Artwork", "art.png");
    assert!(
        harness
            .query_by_label("Shown once the song is saved")
            .is_some()
    );
    harness.state_mut().take_requests();
    harness
        .state_mut()
        .saved(Saved::To(PathBuf::from("/songs/New/New.json")));
    harness.run();
    let requests = harness.state_mut().take_requests();
    let file = PathBuf::from("/songs/New/art.png");
    assert!(
        requests.contains(&Request::ReadArtwork { file }),
        "{requests:?}"
    );
}

#[test]
fn a_file_that_is_not_a_picture_is_refused_as_artwork() {
    let mut harness = four_taps_at("/songs/four.json");
    harness
        .state_mut()
        .use_artwork(Path::new("/pictures/notes.txt"), None);
    harness.run();
    assert_eq!(artwork_name(&harness), None);
    assert!(
        harness
            .query_by_label_contains("is not a PNG or JPEG picture")
            .is_some()
    );
}

#[test]
fn new_audio_brings_its_embedded_cover_unless_the_song_has_artwork() {
    let mut harness = four_taps_at("/songs/four.json");
    let audio = Path::new("/music/song.flac");
    harness.state_mut().use_audio(audio);
    harness.run();
    harness
        .state_mut()
        .artwork_embedded(Path::new("/music/other.flac"), "jpg", picture(b"x"));
    assert_eq!(artwork_name(&harness), None);
    harness
        .state_mut()
        .artwork_embedded(audio, "jpg", picture(b"jpeg"));
    harness.run();
    assert_eq!(artwork_name(&harness).as_deref(), Some("cover.jpg"));
    harness
        .state_mut()
        .artwork_embedded(audio, "png", picture(b"png"));
    assert_eq!(artwork_name(&harness).as_deref(), Some("cover.jpg"));
}

#[test]
#[expect(
    clippy::non_ascii_literal,
    reason = "the test locates the artwork Reading label with its ellipsis"
)]
fn a_song_naming_artwork_asks_for_it_beside_the_song_and_previews_it() {
    let mut harness = four_taps_at("/songs/four.json");
    retype(&mut harness, "Artwork", "art.png");
    let requests = harness.state_mut().take_requests();
    let file = PathBuf::from("/songs/art.png");
    assert!(
        requests.contains(&Request::ReadArtwork { file: file.clone() }),
        "{requests:?}"
    );
    assert!(harness.query_by_label("Reading…").is_some());
    harness.state_mut().artwork_read(&file, None);
    harness.run();
    assert!(
        harness
            .query_by_label("Not a picture beside the song")
            .is_some()
    );
}

#[test]
fn the_song_panel_renames_the_song_and_sets_its_preview() {
    let mut harness = editable_demo();
    retype(&mut harness, "Title", "Mine");
    retype(&mut harness, "Artist", "Me");
    assert_eq!(
        (metadata(&harness).title, metadata(&harness).artist),
        ("Mine".to_owned(), "Me".to_owned())
    );
    retype(&mut harness, "Preview", "0:45.5");
    retype(&mut harness, "Preview length", "15");
    let preview = metadata(&harness).preview.unwrap();
    assert_eq!(
        serde_json::to_value(preview).unwrap(),
        serde_json::json!({ "start_seconds": 45.5, "length_seconds": 15.0 })
    );
    retype(&mut harness, "Preview", "");
    assert_eq!(metadata(&harness).preview, None);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert!(metadata(&harness).preview.is_some());
    harness.get_by_label("Choose the song's audio").click();
    harness.run();
    assert_eq!(request(&mut harness), Request::ChooseAudio);
}

#[test]
fn the_song_panel_sets_the_genre_and_artwork_and_clears_them() {
    let mut harness = editable_demo();
    retype(&mut harness, "Genre", "Dance");
    retype(&mut harness, "Artwork", "art/artwork.png");
    assert_eq!(metadata(&harness).genre.as_deref(), Some("Dance"));
    assert_eq!(
        metadata(&harness).artwork.map(|path| path.to_string()),
        Some("art/artwork.png".to_owned())
    );
    retype(&mut harness, "Artwork", "../outside.png");
    assert!(
        harness
            .query_by_label_contains("is not a file beside the song")
            .is_some()
    );
    retype(&mut harness, "Genre", "");
    retype(&mut harness, "Artwork", "");
    assert_eq!(
        (metadata(&harness).genre, metadata(&harness).artwork),
        (None, None)
    );
}

#[test]
fn a_games_settings_show_as_fields_for_the_song_and_its_open_chart() {
    const TTR: &[&dyn RulesetPlugin] = &[&charter_ttr::Plugin];
    let draft = charter_core::NewSong {
        metadata: charter_editor::demo::song().document().metadata.clone(),
        layout: 0,
        charts: vec![charter_core::NewChart::named("Extreme")],
    };
    let song = charter_ttr::Plugin.new_song(&draft).unwrap();
    let mut app = Window::new(TTR);
    app.open(Path::new("/songs/ttr.json"), &song.to_json().unwrap());
    let mut harness = window(app);
    retype(&mut harness, "Theme", "DeltaHeavyTheme");
    retype(&mut harness, "Chart theme", "Neon");
    let edited_song = harness.state().song.as_ref().unwrap();
    let document = edited_song.document().document();
    assert_eq!(
        document.settings["theme"],
        serde_json::json!("DeltaHeavyTheme")
    );
    assert_eq!(
        document.charts[0].settings["theme"],
        serde_json::json!("Neon")
    );
    retype(&mut harness, "Theme", "");
    let cleared_song = harness.state().song.as_ref().unwrap();
    assert!(cleared_song.document().document().settings.is_empty());
    let sm = editable_demo();
    assert!(sm.query_by_label("Chart theme").is_none());
}

#[test]
fn a_preview_that_is_not_a_time_says_so() {
    let mut harness = editable_demo();
    retype(&mut harness, "Preview", "soon");
    assert!(
        harness
            .query_by_label_contains("soon is not a time")
            .is_some()
    );
    assert_eq!(metadata(&harness).preview, None);
}

#[test]
fn the_song_panel_folds_away_and_comes_back() {
    let mut harness = editable_demo();
    assert!(harness.query_by_label("Verse").is_some());
    harness.get_by_label("Hide the song panel").click();
    harness.run();
    assert!(harness.query_by_label("Verse").is_none());
    assert!(!harness.state().settings().song_panel);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Backslash);
    harness.run();
    assert!(harness.query_by_label("Verse").is_some());
    harness.get_by_label("View").click();
    harness.run();
    harness.get_by_label_contains("Song panel").click();
    harness.run();
    assert!(harness.query_by_label("Verse").is_none());
}

#[test]
fn escape_clears_the_selection() {
    let mut harness = editable_four_taps();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(nothing_selected(&harness));
    harness.key_press(egui::Key::Backspace);
    harness.run();
    assert_eq!(note_count(&harness), 4);
}

/// Where the middle of `lane` meets the line of `tick` on screen, `below` points lower.
fn spot(harness: &Harness<'_, Window>, lane: u16, tick: i32, below: f32) -> egui::Pos2 {
    let area = harness.get_by_label("Notefield").rect();
    let app = harness.state();
    let song = app.song.as_ref().unwrap();
    let chart = song.chart().unwrap();
    let timing = chart.timing();
    let lanes = chart.lanes.map_or(0, u16::from);
    let field = notefield::Field::new(
        area,
        lanes,
        timing.time_at(song.cursor()),
        app.zoom,
        app.strip().is_some(),
    );
    let x = field
        .columns
        .span(LaneIndex::new(lane), LaneCount::ONE)
        .center();
    egui::pos2(x, field.view.y(timing.time_at(Tick::new(tick))) + below)
}

fn press(
    harness: &Harness<'_, Window>,
    pos: egui::Pos2,
    pressed: bool,
    modifiers: egui::Modifiers,
) {
    harness.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers,
    });
}

fn click(harness: &mut Harness<'_, Window>, pos: egui::Pos2, modifiers: egui::Modifiers) {
    harness.hover_at(pos);
    harness.event(egui::Event::ModifiersChanged(modifiers));
    press(harness, pos, true, modifiers);
    press(harness, pos, false, modifiers);
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    harness.run();
}

fn drag(
    harness: &mut Harness<'_, Window>,
    from: egui::Pos2,
    to: egui::Pos2,
    modifiers: egui::Modifiers,
) {
    harness.hover_at(from);
    harness.event(egui::Event::ModifiersChanged(modifiers));
    press(harness, from, true, modifiers);
    for step in 1..=4 {
        harness.hover_at(from.lerp(to, f32::from(u16::try_from(step).unwrap_or(u16::MAX)) / 4.0));
    }
    press(harness, to, false, modifiers);
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    harness.run();
}

fn lane_x(harness: &Harness<'_, Window>, lane: u16) -> f32 {
    field(harness)
        .columns
        .span(LaneIndex::new(lane), LaneCount::ONE)
        .center()
}

fn select_note(harness: &mut Harness<'_, Window>, lane: u16, tick: i32) {
    let pos = on_line(harness, lane_x(harness, lane), tick);
    click(harness, pos, egui::Modifiers::NONE);
}

#[test]
fn copied_notes_paste_at_the_cursor_from_the_clipboard_or_from_memory() {
    let mut harness = editable_four_taps();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.event(egui::Event::Copy);
    harness.run();
    let clipped = Clipped {
        charter_notes: harness.state().clipboard.clip.clone(),
    };
    assert_eq!(clipped.charter_notes.len(), 4);
    let text = serde_json::to_string(&clipped).unwrap();
    for _ in 0..8 {
        harness.key_press(egui::Key::ArrowDown);
    }
    harness.event(egui::Event::Paste(text));
    harness.run();
    assert_eq!(note_count(&harness), 8);
    assert_eq!(lanes_at(&harness, 1920), [0]);
    harness.key_press(egui::Key::PageDown);
    harness.event(egui::Event::Paste("some other text".to_owned()));
    harness.run();
    assert_eq!(note_count(&harness), 8, "other text is not notes");
    assert!(harness.query_by_label_contains("holds no notes").is_some());
    harness.state_mut().apply(Action::Paste(None));
    harness.run();
    assert_eq!(
        note_count(&harness),
        12,
        "the menu pastes those copied last"
    );
}

#[test]
fn ctrl_arrows_move_the_selection_and_ctrl_d_and_ctrl_m_duplicate_and_mirror() {
    let mut harness = editable_four_taps();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.key_press(egui::Key::Q);
    harness.run();
    assert_eq!(note_count(&harness), 4);
    select_note(&mut harness, 3, 720);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::ArrowLeft);
    harness.run();
    assert_eq!(lanes_at(&harness, 720), [2]);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::ArrowDown);
    harness.run();
    assert_eq!(lanes_at(&harness, 720), Vec::<u16>::new());
    assert_eq!(lanes_at(&harness, 960), [2]);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::M);
    harness.run();
    assert_eq!(lanes_at(&harness, 960), [1]);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::D);
    harness.run();
    assert_eq!(lanes_at(&harness, 1200), [1]);
    assert_eq!(note_count(&harness), 5);
}

#[test]
fn a_duplicated_hold_starts_after_it_ends() {
    let mut harness = editable_four_taps();
    select_note(&mut harness, 0, 0);
    harness.key_press(egui::Key::CloseBracket);
    harness.key_press(egui::Key::CloseBracket);
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::D);
    harness.run();
    assert_eq!(lanes_at(&harness, 240), [1]);
    assert_eq!(lanes_at(&harness, 720), [0, 3]);
}

#[test]
fn ctrl_d_keeps_the_source_layer_when_another_is_active() {
    let mut harness = editable_four_taps();
    harness.state_mut().apply(Action::AddLayer);
    select_note(&mut harness, 3, 720);
    let source = harness
        .state()
        .song
        .as_ref()
        .unwrap()
        .chart()
        .unwrap()
        .objects
        .iter()
        .find(|o| o.tick == Tick::new(720))
        .unwrap()
        .layer
        .clone();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::D);
    harness.run();
    let song = harness.state().song.as_ref().unwrap();
    assert!(
        song.chart()
            .unwrap()
            .objects
            .iter()
            .any(|o| o.tick == Tick::new(960) && o.layer == source)
    );
}

#[test]
fn dragging_a_note_with_shift_changes_its_lane_and_keeps_its_time() {
    let mut harness = editable_four_taps();
    let from = on_line(&harness, lane_x(&harness, 0), 0);
    let to = on_line(&harness, lane_x(&harness, 2), 480);
    drag(&mut harness, from, to, egui::Modifiers::SHIFT);
    assert_eq!(lanes_at(&harness, 0), [2]);
    assert_eq!(lanes_at(&harness, 480), [2]);
}

#[test]
fn a_middle_drag_pans_the_chart_and_shift_wheel_moves_by_measure() {
    let mut harness = editable_four_taps();
    let from = on_line(&harness, lane_x(&harness, 1), 960);
    let to = on_line(&harness, lane_x(&harness, 1), 0);
    let middle = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Middle,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    harness.hover_at(from);
    harness.event(middle(from, true));
    for step in 1..=4 {
        harness.hover_at(from.lerp(to, f32::from(u16::try_from(step).unwrap_or(u16::MAX)) / 4.0));
    }
    harness.event(middle(to, false));
    harness.run();
    let cursor = harness.state().song.as_ref().unwrap().cursor();
    assert_eq!(cursor, Tick::new(960), "dragging the chart up goes on");
    assert_eq!(note_count(&harness), 4);
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, -1.0),
        modifiers: egui::Modifiers::SHIFT,
        phase: egui::TouchPhase::Move,
    });
    harness.run();
    let moved_cursor = harness.state().song.as_ref().unwrap().cursor();
    assert_eq!(moved_cursor, Tick::new(3840));
}

#[test]
fn a_right_click_opens_the_menu_and_leaves_the_cursor_where_it_was() {
    let mut harness = editable_four_taps();
    let pos = on_line(&harness, lane_x(&harness, 2), 480);
    harness.hover_at(pos);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    harness.run();
    assert_eq!(
        harness.state().song.as_ref().unwrap().cursor(),
        Tick::new(0)
    );
    assert!(harness.query_by_label("1 note selected").is_some());
    assert!(
        harness
            .query_by_label_contains("Insert pattern here")
            .is_some()
    );
    assert!(harness.query_by_label("Select lane 3").is_some());
    harness
        .get_by_label_contains("Mirror across the lanes")
        .click();
    harness.run();
    assert_eq!(lanes_at(&harness, 480), [1]);
}

#[test]
fn right_clicking_an_already_selected_note_keeps_that_selection() {
    let mut harness = editable_four_taps();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run();
    assert_eq!(
        harness.state().song.as_ref().unwrap().selection().count(),
        4
    );
    let pos = on_line(&harness, lane_x(&harness, 2), 480);
    harness.hover_at(pos);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    harness.run();
    assert!(harness.query_by_label("4 notes selected").is_some());
    let selected = harness.state().song.as_ref().unwrap().selection().count();
    assert_eq!(selected, 4);
}

#[test]
fn what_the_menu_places_here_goes_where_it_was_opened_though_the_cursor_moved() {
    let mut harness = editable_four_taps();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.event(egui::Event::Copy);
    harness.run();
    harness.key_press(egui::Key::PageDown);
    harness.run();
    let before = harness.state().song.as_ref().unwrap().cursor();
    let paste = Action::Here(Tick::new(7680), Box::new(Action::Paste(None)));
    harness.state_mut().apply(paste);
    harness.run();
    assert_eq!(lanes_at(&harness, 7680), [0]);
    assert_eq!(harness.state().song.as_ref().unwrap().cursor(), before);
}

#[test]
fn the_menu_pastes_the_clipboard_where_it_was_opened_or_else_the_last_copy() {
    let mut harness = editable_four_taps();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.event(egui::Event::Copy);
    harness.run();
    let clipped = Clipped {
        charter_notes: harness
            .state()
            .clipboard
            .clip
            .iter()
            .take(1)
            .cloned()
            .collect(),
    };
    let text = serde_json::to_string(&clipped).unwrap();
    harness
        .state_mut()
        .apply(Action::PasteHere(Tick::new(3840)));
    harness.event(egui::Event::Paste(text));
    harness.run();
    assert_eq!(note_count(&harness), 5, "the clipboard's one note");
    assert_eq!(lanes_at(&harness, 3840), [0]);
    harness
        .state_mut()
        .apply(Action::PasteHere(Tick::new(7680)));
    for _ in 0..5 {
        harness.step();
    }
    assert_eq!(note_count(&harness), 9, "no text, so the last copy");
    assert_eq!(lanes_at(&harness, 7680), [0]);
}

#[test]
fn a_pattern_places_its_steps_from_the_lane_and_moves_the_cursor_past_it() {
    let mut app = Window::new(PLUGINS);
    app.open(Path::new("four.json"), &four_taps());
    let song = app.song.as_mut().unwrap();
    song.jump_to(Tick::new(1920));
    let steps: &'static [u16] = &[0, 1, 2, 0, 1, 2];
    assert_eq!(pattern(song, steps, 4, LaneIndex::new(2)), None);
    let lanes: Vec<(i64, u16)> = song
        .chart()
        .unwrap()
        .objects
        .iter()
        .filter(|object| object.tick >= Tick::new(1920))
        .filter_map(|object| match object.position {
            charter_core::Position::Lane { index, .. } => {
                Some((i64::from(object.tick), u16::from(index)))
            }
            charter_core::Position::Plane { .. } => None,
        })
        .collect();
    assert_eq!(
        lanes,
        [
            (1920, 2),
            (2160, 3),
            (2400, 0),
            (2640, 2),
            (2880, 3),
            (3120, 0)
        ]
    );
    assert_eq!(song.cursor(), Tick::new(3360));
}

#[test]
fn a_pattern_wraps_each_step_around_a_two_lane_chart() {
    let mut document: serde_json::Value = serde_json::from_slice(&four_taps()).unwrap();
    document["charts"][0]["lanes"] = serde_json::json!(2);
    document["charts"][0]["objects"] = serde_json::json!([]);
    let mut app = Window::new(PLUGINS);
    app.open(
        Path::new("two.json"),
        &serde_json::to_vec(&document).unwrap(),
    );
    let song = app.song.as_mut().unwrap();
    assert!(song.read_only().is_none());
    song.jump_to(Tick::new(1920));
    assert_eq!(
        pattern(song, &[0, 1, 2, 0, 1, 2], 4, LaneIndex::new(1)),
        None
    );
    let placed: Vec<_> = song
        .chart()
        .unwrap()
        .objects
        .iter()
        .map(|object| {
            let Position::Lane { index, .. } = object.position else {
                panic!("the pattern places lane notes");
            };
            (object.tick, u16::from(index))
        })
        .collect();
    assert_eq!(
        placed,
        [
            (Tick::new(1920), 1),
            (Tick::new(2160), 0),
            (Tick::new(2400), 1),
            (Tick::new(2640), 1),
            (Tick::new(2880), 0),
            (Tick::new(3120), 1),
        ]
    );
    assert_eq!(song.cursor(), Tick::new(3360));
}

#[test]
fn ctrl_clicking_a_lane_places_a_tap_on_the_nearest_snap_line() {
    let mut harness = editable_four_taps();
    let pos = spot(&harness, 3, 240, 4.0);
    click(&mut harness, pos, egui::Modifiers::COMMAND);
    assert_eq!(note_at(&harness, 240, 3), Some(tap()));
}

#[test]
fn ctrl_clicking_a_lane_number_does_not_place_a_hidden_note() {
    let mut harness = editable_four_taps();
    let x = spot(&harness, 0, 0, 0.0).x;
    let top = harness.get_by_label("Notefield").rect().top();
    click(
        &mut harness,
        egui::pos2(x, top + 13.0),
        egui::Modifiers::COMMAND,
    );

    assert_eq!(note_count(&harness), 4);
    assert!(!harness.state().song.as_ref().unwrap().is_modified());
}

#[test]
fn ctrl_shift_clicking_places_a_mine() {
    let mut harness = editable_four_taps();
    let pos = spot(&harness, 1, 480, 0.0);
    click(
        &mut harness,
        pos,
        egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
    );
    assert_eq!(note_at(&harness, 480, 1), Some(mine()));
}

#[test]
fn ctrl_dragging_down_a_lane_draws_a_hold_that_undoes_in_one_step() {
    let mut harness = editable_four_taps();
    let (from, to) = (spot(&harness, 3, 240, 0.0), spot(&harness, 3, 720, -3.0));
    drag(&mut harness, from, to, egui::Modifiers::COMMAND);
    assert_eq!(note_at(&harness, 240, 3), Some(hold(480)));
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(note_at(&harness, 240, 3), None);
}

#[test]
fn clicking_notes_selects_them_as_shift_adds_and_alt_takes_away() {
    let mut harness = editable_four_taps();
    let (first, second) = (spot(&harness, 0, 0, 0.0), spot(&harness, 1, 240, 2.0));
    click(&mut harness, first, egui::Modifiers::NONE);
    assert!(harness.query_by_label("1 note selected").is_some());
    click(&mut harness, second, egui::Modifiers::SHIFT);
    assert!(harness.query_by_label("2 notes selected").is_some());
    click(&mut harness, first, egui::Modifiers::ALT);
    assert!(harness.query_by_label("1 note selected").is_some());
    let empty = spot(&harness, 3, 0, 0.0);
    click(&mut harness, empty, egui::Modifiers::NONE);
    assert!(nothing_selected(&harness));
}

#[test]
fn dragging_a_box_selects_the_notes_whose_heads_it_covers() {
    let mut harness = editable_four_taps();
    let (from, to) = (spot(&harness, 1, 180, 0.0), spot(&harness, 2, 540, 0.0));
    drag(&mut harness, from, to, egui::Modifiers::NONE);
    assert!(harness.query_by_label("2 notes selected").is_some());
    let song = harness.state().song.as_ref().unwrap();
    let ticks: Vec<_> = song.selection().map(|note| note.tick).collect();
    assert_eq!(ticks, [Tick::new(240), Tick::new(480)]);
}

#[test]
fn dragging_a_note_moves_it_along_the_snap_and_across_lanes_in_one_step() {
    let mut harness = editable_four_taps();
    let (from, to) = (spot(&harness, 0, 0, 0.0), spot(&harness, 1, 480, 3.0));
    drag(&mut harness, from, to, egui::Modifiers::NONE);
    assert_eq!(note_at(&harness, 0, 0), None);
    assert_eq!(note_at(&harness, 480, 1), Some(tap()));
    assert_eq!(note_count(&harness), 4);
    assert!(harness.query_by_label("1 note selected").is_some());
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(note_at(&harness, 0, 0), Some(tap()));
    assert_eq!(note_at(&harness, 480, 1), None);
}

#[test]
fn dragging_a_selected_note_moves_the_selection_and_keeps_it_on_the_lanes() {
    let mut harness = editable_four_taps();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run();
    let (from, to) = (spot(&harness, 1, 240, 0.0), spot(&harness, 2, 480, 0.0));
    drag(&mut harness, from, to, egui::Modifiers::NONE);
    for (tick, lane) in [(240, 0), (480, 1), (720, 2), (960, 3)] {
        assert_eq!(note_at(&harness, tick, lane), Some(tap()), "{tick} {lane}");
    }
    assert!(harness.query_by_label("4 notes selected").is_some());
}

#[test]
fn the_mouse_places_nothing_in_a_read_only_song() {
    let mut app = Window::default();
    app.open(Path::new("four.json"), &four_taps());
    let mut harness = window(app);
    let pos = spot(&harness, 3, 240, 0.0);
    click(&mut harness, pos, egui::Modifiers::COMMAND);
    assert_eq!(note_at(&harness, 240, 3), None);
    assert!(harness.query_by_label_contains("Refused").is_none());
}

/// The one request the window made since the last were taken.
fn request(harness: &mut Harness<'_, Window>) -> Request {
    let mut requests = harness.state_mut().take_requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    requests.remove(0)
}

/// Whether `bytes` hold a song whose open chart has a tap on `lane` at `tick`.
fn holds_tap(bytes: &[u8], tick: i32, lane: u16) -> bool {
    let song = charter_core::ValidDocument::from_json(bytes).unwrap();
    let chart = song.charts().next().unwrap();
    chart.objects().any(|object| {
        object.tick == Tick::new(tick)
            && object.position.lanes() == Some(LaneSpan::new(LaneIndex::new(lane), LaneCount::ONE))
    })
}

#[test]
fn ctrl_s_writes_the_song_back_to_its_file() {
    let mut harness = editable_four_taps();
    harness.key_press(egui::Key::Num4);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    harness.run();
    let Request::Write { path, bytes } = request(&mut harness) else {
        panic!("expected a write");
    };
    assert_eq!(path, Path::new("four.json"));
    assert!(holds_tap(&bytes, 0, 3));
}

#[test]
fn saving_a_song_without_a_file_asks_where() {
    let mut harness = editable_demo();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    harness.run();
    let Request::SaveAs { name, .. } = request(&mut harness) else {
        panic!("expected save as");
    };
    assert_eq!(name, "Demo.json");
}

#[test]
fn ctrl_shift_s_asks_where_even_for_a_song_with_a_file() {
    let mut harness = editable_four_taps();
    let command_shift = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);
    harness.key_press_modifiers(command_shift, egui::Key::S);
    harness.run();
    let Request::SaveAs { name, .. } = request(&mut harness) else {
        panic!("expected save as");
    };
    assert_eq!(name, "four.json");
}

#[test]
#[expect(
    clippy::non_ascii_literal,
    reason = "the test locates the Open menu label with its ellipsis"
)]
fn ctrl_o_and_the_file_menu_ask_for_a_song_to_open() {
    let mut harness = editable_four_taps();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::O);
    harness.run();
    assert_eq!(request(&mut harness), Request::Open);
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label_contains("Open…").click();
    harness.run();
    assert_eq!(request(&mut harness), Request::Open);
}

#[test]
fn the_title_names_the_song_and_marks_unsaved_changes() {
    let mut harness = editable_four_taps();
    assert_eq!(harness.state().title(), "four.json - charter");
    harness.key_press(egui::Key::Num4);
    harness.run();
    assert_eq!(harness.state().title(), "*four.json - charter");
    harness
        .state_mut()
        .saved(Saved::To(PathBuf::from("four.json")));
    harness.run();
    assert_eq!(harness.state().title(), "four.json - charter");
}

#[test]
fn a_song_saved_elsewhere_is_saved_there_from_then_on() {
    let mut harness = editable_demo();
    harness
        .state_mut()
        .saved(Saved::To(PathBuf::from("mine.json")));
    harness.run();
    assert_eq!(harness.state().title(), "mine.json - charter");
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    harness.run();
    let Request::Write { path, .. } = request(&mut harness) else {
        panic!("expected a write");
    };
    assert_eq!(path, Path::new("mine.json"));
}

#[test]
fn a_failed_save_says_why_and_keeps_the_changes_unsaved() {
    let mut harness = editable_four_taps();
    harness.key_press(egui::Key::Num4);
    harness.run();
    harness
        .state_mut()
        .saved(Saved::Failed("the disk is full".to_owned()));
    harness.run();
    assert!(
        harness
            .query_by_label_contains("the disk is full")
            .is_some()
    );
    assert_eq!(harness.state().title(), "*four.json - charter");
}

/// The four taps with one more placed, so there is something to lose.
fn edited_four_taps() -> Harness<'static, Window> {
    let mut harness = editable_four_taps();
    harness.key_press(egui::Key::Num4);
    harness.run();
    harness
}

/// Asks to close the window as its close button does, and runs the one frame that answers.
fn close(harness: &mut Harness<'_, Window>) {
    let root = harness.input_mut().viewports.entry(egui::ViewportId::ROOT);
    root.or_default().events.push(egui::ViewportEvent::Close);
    harness.step();
}

/// Whether the last frame sent `command` to the host's window.
fn sent(harness: &Harness<'_, Window>, command: &egui::ViewportCommand) -> bool {
    let output = harness
        .output()
        .viewport_output
        .get(&egui::ViewportId::ROOT);
    output.is_some_and(|viewport| viewport.commands.contains(command))
}

fn asks_to_save(harness: &Harness<'_, Window>) -> bool {
    harness
        .query_by_label("Save changes to four.json?")
        .is_some()
}

#[test]
fn closing_with_unsaved_changes_asks_first() {
    let mut harness = edited_four_taps();
    close(&mut harness);
    assert!(sent(&harness, &egui::ViewportCommand::CancelClose));
    harness.run();
    assert!(asks_to_save(&harness));
}

#[test]
fn closing_without_changes_does_not_ask() {
    let mut harness = editable_four_taps();
    close(&mut harness);
    assert!(!sent(&harness, &egui::ViewportCommand::CancelClose));
    harness.run();
    assert!(!asks_to_save(&harness));
}

#[test]
fn dont_save_closes_and_lets_the_close_through() {
    let mut harness = edited_four_taps();
    close(&mut harness);
    harness.run();
    harness.get_by_label("Don't save").click();
    harness.step();
    harness.step();
    assert!(sent(&harness, &egui::ViewportCommand::Close));
    close(&mut harness);
    assert!(!sent(&harness, &egui::ViewportCommand::CancelClose));
}

#[test]
fn save_writes_and_closes_once_the_host_says_it_saved() {
    let mut harness = edited_four_taps();
    close(&mut harness);
    harness.run();
    harness.get_by_label("Save").click();
    harness.run();
    let Request::Write { path, .. } = request(&mut harness) else {
        panic!("expected a write");
    };
    harness.state_mut().saved(Saved::To(path));
    harness.step();
    assert!(sent(&harness, &egui::ViewportCommand::Close));
}

#[test]
fn cancel_keeps_the_window_and_its_changes() {
    let mut harness = edited_four_taps();
    close(&mut harness);
    harness.run();
    harness.get_by_label("Cancel").click();
    harness.run();
    assert!(!asks_to_save(&harness));
    assert!(!sent(&harness, &egui::ViewportCommand::Close));
    assert_eq!(harness.state().title(), "*four.json - charter");
}

#[test]
fn opening_another_song_over_unsaved_changes_asks_first() {
    let mut harness = edited_four_taps();
    harness
        .state_mut()
        .open(Path::new("example.json"), example());
    harness.run();
    assert!(asks_to_save(&harness));
    assert_eq!(harness.state().title(), "*four.json - charter");
    harness.get_by_label("Don't save").click();
    harness.run();
    assert_eq!(harness.state().title(), "example.json - charter");
}

#[test]
fn a_save_that_fails_leaves_everything_as_it_was() {
    let mut harness = edited_four_taps();
    harness.get_by_label("Help").click();
    harness.run();
    harness.get_by_label("Open the demo song").click();
    harness.run();
    harness.get_by_label("Save").click();
    harness.run();
    assert!(matches!(request(&mut harness), Request::Write { .. }));
    harness
        .state_mut()
        .saved(Saved::Failed("the disk is full".to_owned()));
    harness.run();
    assert_eq!(harness.state().title(), "*four.json - charter");
    assert!(!asks_to_save(&harness));
}

/// Shows the settings page named `name`.
fn settings_page(harness: &mut Harness<'_, Window>, name: &str) {
    harness.get_by_label(name).click();
    harness.run();
}

#[test]
fn the_settings_window_shows_one_page_at_a_time_starting_with_editing() {
    let mut harness = editable_demo();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    harness.run();
    assert!(
        harness
            .query_by_role_and_label(Role::CheckBox, "Move on a snap after placing a note")
            .is_some()
    );
    assert!(
        harness
            .query_by_role_and_label(Role::ComboBox, "Notefield")
            .is_none()
    );
    settings_page(&mut harness, "Display");
    assert!(
        harness
            .query_by_role_and_label(Role::CheckBox, "Move on a snap after placing a note")
            .is_none()
    );
    assert!(
        harness
            .query_by_role_and_label(Role::ComboBox, "Notefield")
            .is_some()
    );
}

#[test]
fn the_settings_window_closes_from_its_own_button() {
    let mut harness = editable_demo();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    harness.run();
    harness.get_by_label("Close settings").click();
    harness.run();
    assert!(harness.query_all_by_label("Settings").next().is_none());
}

#[test]
#[expect(
    clippy::non_ascii_literal,
    reason = "the test locates the Settings menu label with its ellipsis"
)]
fn the_settings_window_opens_from_the_edit_menu_and_with_ctrl_comma() {
    let mut harness = editable_demo();
    harness.get_by_label("Edit").click();
    harness.run();
    harness.get_by_label_contains("Settings…").click();
    harness.run();
    assert!(harness.query_all_by_label("Settings").next().is_some());
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    harness.run();
    assert!(harness.query_all_by_label("Settings").next().is_none());
}

#[test]
fn lane_keys_and_commands_still_work_with_the_settings_open() {
    let mut harness = editable_demo();
    let before = harness
        .state()
        .song
        .as_ref()
        .unwrap()
        .chart()
        .unwrap()
        .objects
        .clone();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    harness.run();
    harness.key_press(egui::Key::Num1);
    harness.run();
    let after = &harness
        .state()
        .song
        .as_ref()
        .unwrap()
        .chart()
        .unwrap()
        .objects;
    assert_ne!(after, &before);
    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    assert!(harness.state().song.as_ref().unwrap().cursor() > Tick::new(0));
}

#[test]
fn a_button_tab_focused_leaves_the_keys_to_the_editor() {
    let mut harness = editable_demo();
    harness.key_press(egui::Key::Tab);
    harness.run();
    assert!(harness.ctx.memory(egui::Memory::focused).is_some());
    let before = harness.state().song.as_ref().unwrap().cursor();
    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    assert!(harness.state().song.as_ref().unwrap().cursor() > before);
}

#[test]
fn restore_defaults_puts_every_setting_back_but_the_calibration_and_recent_songs() {
    let mut app = Window::new(PLUGINS).with_settings(Settings {
        metronome: true,
        tuned_lanes: true,
        zoom: 150.0,
        input_latency_ms: 25.0,
        ..Settings::default()
    });
    app.open(Path::new("/songs/four.json"), &four_taps());
    let mut harness = window(app);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    harness.run();
    harness.get_by_label("Restore defaults").click();
    harness.run();
    let calibrated = Settings {
        input_latency_ms: 25.0,
        recent: vec![PathBuf::from("/songs/four.json")],
        ..Settings::default()
    };
    assert_eq!(harness.state().settings(), &calibrated);
}

#[test]
fn the_settings_window_turns_the_notefield_and_picks_sounds() {
    let mut harness = editable_demo();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    harness.run();
    settings_page(&mut harness, "Display");
    harness
        .get_by_role_and_label(Role::ComboBox, "Notefield")
        .click();
    harness.run();
    harness
        .get_by_label("Horizontal, notes travel left")
        .click();
    harness.run();
    assert_eq!(harness.state().settings().direction, Direction::Left);
    settings_page(&mut harness, "Sounds");
    harness
        .get_by_role_and_label(Role::CheckBox, "Metronome")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::ComboBox, "Metronome sound")
        .scroll_to_me();
    harness.run();
    harness
        .get_by_role_and_label(Role::ComboBox, "Metronome sound")
        .click();
    harness.run();
    harness.get_by_label("Shaker burst").click();
    harness.run();
    assert!(harness.state().settings().metronome);
    assert_eq!(harness.state().settings().metronome_sound, Kit::ShakerBurst);
    harness.get_by_label("Preview the metronome").click();
    harness.run();
    let preview = harness.state_mut().take_requests();
    let Some(Request::Preview { clicks, .. }) = preview.last() else {
        panic!("expected a preview, got {preview:?}");
    };
    let tones: Vec<Tone> = clicks.iter().map(|(_, tone)| *tone).collect();
    let (accent, beat) = (Tone::Accent(Kit::ShakerBurst), Tone::Beat(Kit::ShakerBurst));
    assert_eq!(tones, [accent, beat, beat, beat]);
    assert!(
        harness
            .query_by_role_and_label(Role::ComboBox, "Note tick sound")
            .is_some()
    );
}

/// The demo, open for editing with the notefield running `direction`.
fn turned_demo(direction: Direction) -> Harness<'static, Window> {
    let settings = Settings {
        direction,
        ..Settings::default()
    };
    let mut app = Window::new(PLUGINS).with_settings(settings);
    app.open_demo();
    window(app)
}

/// Where the middle of `lane` meets the line of `tick` on screen, however the notefield runs.
fn turned_spot(harness: &Harness<'_, Window>, lane: u16, tick: i32) -> egui::Pos2 {
    let area = harness.get_by_label("Notefield").rect();
    let app = harness.state();
    let song = app.song.as_ref().unwrap();
    let chart = song.chart().unwrap();
    let timing = chart.timing();
    let lanes = chart.lanes.map_or(0, u16::from);
    let flip = Flip {
        area,
        direction: app.settings().direction,
    };
    let field = notefield::Field::new(
        flip.field(),
        lanes,
        timing.time_at(song.cursor()),
        app.zoom,
        app.strip().is_some(),
    );
    let x = field
        .columns
        .span(LaneIndex::new(lane), LaneCount::ONE)
        .center();
    flip.pos(egui::pos2(x, field.view.y(timing.time_at(Tick::new(tick)))))
}

#[test]
fn ctrl_clicking_places_a_tap_however_the_notefield_runs() {
    for direction in Direction::ALL {
        let mut harness = turned_demo(direction);
        let at = turned_spot(&harness, 1, 240);
        click(&mut harness, at, egui::Modifiers::COMMAND);
        assert_eq!(lanes_at(&harness, 240), [1], "{direction:?}");
    }
}

#[test]
fn clicking_a_note_selects_it_however_the_notefield_runs() {
    for direction in [Direction::Down, Direction::Right] {
        let mut harness = turned_demo(direction);
        let at = turned_spot(&harness, 3, 0);
        click(&mut harness, at, egui::Modifiers::NONE);
        let song = harness.state().song.as_ref().unwrap();
        assert_eq!(song.selection().count(), 1, "{direction:?}");
    }
}

/// Clicks what `label` names in the song panel, scrolled into view first, since the panel
/// runs past the bottom of the test's window.
fn click_in_panel(harness: &mut Harness<'_, Window>, label: &str) {
    harness.get_by_label(label).scroll_to_me();
    harness.run();
    harness.get_by_label(label).click();
    harness.run();
}

fn bpm_field<'a>(harness: &'a Harness<'_, Window>) -> egui_kittest::Node<'a> {
    harness.get_by_role_and_label(Role::TextInput, "BPM")
}

/// Types `text` over whatever the BPM field holds, then presses `key`.
fn enter_bpm(harness: &mut Harness<'_, Window>, text: &str, key: egui::Key) {
    bpm_field(harness).focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    bpm_field(harness).type_text(text);
    harness.run();
    harness.key_press(key);
    harness.run();
}

/// The tick and BPM of the change in effect at the cursor.
fn bpm_at_cursor(harness: &Harness<'_, Window>) -> Option<(i64, String)> {
    let song = harness.state().song.as_ref()?;
    let (tick, bpm) = song.bpm()?;
    let tick = serde_json::to_value(tick).ok()?.as_i64()?;
    let value = f64::from(bpm);
    let one = (value * 10.0).round() / 10.0;
    let shown = if (value - one).abs() < 1e-5 {
        charter_core::Bpm::try_from(one).map_or_else(|_| bpm.to_string(), |bpm| bpm.to_string())
    } else {
        bpm.to_string()
    };
    Some((tick, shown))
}

#[test]
fn the_tempo_at_the_cursor_shows_beside_the_song_and_in_the_status_bar() {
    let harness = editable_demo();
    assert_eq!(bpm_field(&harness).value().as_deref(), Some("150"));
    assert!(harness.query_by_label("150 BPM").is_some());
}

#[test]
fn a_bpm_typed_changes_the_tempo_of_the_section_the_cursor_is_in() {
    let mut harness = editable_demo();
    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    enter_bpm(&mut harness, "175.5", egui::Key::Enter);
    assert_eq!(bpm_at_cursor(&harness), Some((0, "175.5".to_owned())));
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
}

/// The tick and time of the first note after the start of the open chart.
fn first_note(harness: &Harness<'_, Window>) -> (Tick, f64) {
    let song = harness.state().song.as_ref().unwrap();
    let chart = song.chart().unwrap();
    let note = chart
        .objects
        .iter()
        .find(|note| note.tick > Tick::new(0))
        .unwrap();
    (note.tick, song.timing().time_at(note.tick).seconds())
}

#[test]
fn a_bpm_keeps_notes_where_they_sound_unless_the_setting_keeps_them_on_their_beats() {
    let mut harness = editable_demo();
    let (tick, seconds) = first_note(&harness);
    enter_bpm(&mut harness, "175.5", egui::Key::Enter);
    let (moved, heard) = first_note(&harness);
    assert_ne!(moved, tick);
    assert!((heard - seconds).abs() < 1e-3, "{heard} {seconds}");

    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.state_mut().settings.notes_keep_time = false;
    harness.run();
    enter_bpm(&mut harness, "175.5", egui::Key::Enter);
    assert_eq!(first_note(&harness).0, tick);
}

#[test]
fn a_bpm_typed_applies_when_the_field_is_left() {
    let mut harness = editable_demo();
    bpm_field(&harness).focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    bpm_field(&harness).type_text("160");
    harness.run();
    offset_field(&harness).focus();
    harness.run();
    assert_eq!(bpm_at_cursor(&harness), Some((0, "160".to_owned())));
}

#[test]
fn the_bpm_steps_up_and_down_a_whole_bpm_at_a_time() {
    let mut harness = editable_demo();
    click_in_panel(&mut harness, "Raise the BPM by 1");
    assert_eq!(bpm_at_cursor(&harness), Some((0, "151".to_owned())));
    harness.get_by_label("Lower the BPM by 1").click();
    click_in_panel(&mut harness, "Lower the BPM by 1");
    assert_eq!(bpm_at_cursor(&harness), Some((0, "149".to_owned())));
}

#[test]
fn a_bpm_left_with_escape_changes_nothing() {
    let mut harness = editable_demo();
    enter_bpm(&mut harness, "90", egui::Key::Escape);
    assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
    assert_eq!(bpm_field(&harness).value().as_deref(), Some("150"));
}

#[test]
fn a_bpm_that_is_not_a_tempo_says_so() {
    let mut harness = editable_demo();
    enter_bpm(&mut harness, "fast", egui::Key::Enter);
    assert!(
        harness
            .query_by_label_contains("fast is not a tempo")
            .is_some()
    );
    assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
}

#[test]
fn a_timing_point_is_added_at_the_cursor_and_removed_but_not_the_first() {
    let mut harness = editable_demo();
    harness.set_size(egui::vec2(1000.0, 1100.0));
    harness.run();
    assert!(harness.query_by_label("Remove this timing point").is_none());
    let add = |view: &Harness<'_, Window>| {
        view.get_by_label("Add a timing point here")
            .accesskit_node()
            .is_disabled()
    };
    assert!(add(&harness), "the cursor is on the first point");
    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    assert!(!add(&harness));
    harness.get_by_label("Add a timing point here").click();
    harness.run();
    assert_eq!(bpm_at_cursor(&harness), Some((240, "150".to_owned())));
    assert!(add(&harness));
    click_in_panel(&mut harness, "Remove this timing point");
    assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
}

#[test]
fn the_timing_points_list_shows_every_point_and_jumps_to_one_clicked() {
    let mut harness = editable_demo();
    // Tall enough that the whole song panel, the list with it, is on screen.
    harness.set_size(egui::vec2(1000.0, 1100.0));
    harness.run();
    let rows = harness
        .query_all_by_label_contains("Timing point at")
        .count();
    assert_eq!(rows, 5);
    harness.get_by_label("Timing point at 00:26.100").click();
    harness.run();
    assert!(cursor_shows(&harness, "16:0:0"));
    assert_eq!(bpm_field(&harness).value().as_deref(), Some("180"));
}

#[test]
fn a_time_typed_for_a_point_moves_it_in_the_audio() {
    let mut harness = editable_demo();
    harness.set_size(egui::vec2(1000.0, 1100.0));
    harness.run();
    harness.get_by_label("Timing point at 00:26.100").click();
    harness.run();
    let time = harness.get_by_role_and_label(Role::TextInput, "Time");
    assert_eq!(time.value().as_deref(), Some("00:26.100"));
    time.focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_role_and_label(Role::TextInput, "Time")
        .type_text("0:26.25");
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(anchor_seconds(&harness), [0.0, 26.25]);
    harness.get_by_label("Timing point at 00:26.250").click();
    harness.run();
}

#[test]
fn the_starting_bpm_sets_the_first_points_tempo_from_anywhere() {
    let mut harness = editable_demo();
    harness.get_by_label("Timing point at 00:26.100").click();
    harness.run();
    let start = harness.get_by_role_and_label(Role::TextInput, "Starting BPM");
    assert_eq!(start.value().as_deref(), Some("150"));
    start.focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_role_and_label(Role::TextInput, "Starting BPM")
        .type_text("160");
    harness.key_press(egui::Key::Enter);
    harness.run();
    let song = harness.state().song.as_ref().unwrap();
    assert_eq!(
        song.timing().bpm_at(Tick::new(0)).map(f64::from),
        Some(160.0)
    );
    assert_eq!(anchor_seconds(&harness), [0.0, 26.1]);
}

fn meter_field<'a>(harness: &'a Harness<'_, Window>) -> egui_kittest::Node<'a> {
    harness.get_by_role_and_label(Role::TextInput, "Meter")
}

#[test]
fn a_meter_typed_starts_measures_of_it_at_the_timing_point() {
    let mut harness = editable_demo();
    assert_eq!(meter_field(&harness).value().as_deref(), Some("4/4"));
    meter_field(&harness).focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    meter_field(&harness).type_text("3/4");
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    let song = harness.state().song.as_ref().unwrap();
    let meter = song
        .timing()
        .measures_from(Tick::new(0))
        .next()
        .unwrap()
        .meter;
    assert_eq!(meter.to_string(), "3/4");
    meter_field(&harness).focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    meter_field(&harness).type_text("waltz");
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(
        harness
            .query_by_label_contains("waltz is not a meter")
            .is_some()
    );
}

#[test]
fn a_read_only_song_shows_its_tempo_without_changing_it() {
    let mut harness = window(Window::new(&[]));
    harness.state_mut().open_demo();
    harness.run();
    assert!(
        harness
            .query_by_role_and_label(Role::TextInput, "BPM")
            .is_none()
    );
    assert!(harness.query_by_label("150 BPM").is_some());
}

fn section_field<'a>(harness: &'a Harness<'_, Window>) -> egui_kittest::Node<'a> {
    harness.get_by_role_and_label(Role::TextInput, "Section here")
}

/// Types `text` over whatever the section field holds, then presses `key`.
fn enter_section(harness: &mut Harness<'_, Window>, text: &str, key: egui::Key) {
    section_field(harness).focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    section_field(harness).type_text(text);
    harness.run();
    harness.key_press(key);
    harness.run();
}

fn marker_texts(harness: &Harness<'_, Window>) -> Vec<String> {
    let song = harness.state().song.as_ref().unwrap();
    let markers = &song.document().document().markers;
    markers.iter().map(|marker| marker.text.clone()).collect()
}

#[test]
fn clicking_a_marker_in_the_list_jumps_there() {
    let mut harness = editable_demo();
    // The list sits below the tempo controls, past the bottom of a short window.
    harness.get_by_label("Chorus").scroll_to_me();
    harness.run();
    harness.get_by_label("Chorus").click();
    harness.run();
    assert!(cursor_shows(&harness, "16:0:0"));
    assert_eq!(section_field(&harness).value().as_deref(), Some("Chorus"));
}

#[test]
fn a_section_named_at_the_cursor_is_added_and_listed() {
    let mut harness = editable_demo();
    harness.key_press(egui::Key::PageDown);
    harness.run();
    assert_eq!(section_field(&harness).value().as_deref(), Some(""));
    enter_section(&mut harness, "Build", egui::Key::Enter);
    assert_eq!(marker_texts(&harness)[1], "Build");
    assert!(harness.query_by_label("Build").is_some());
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert!(!marker_texts(&harness).contains(&"Build".to_owned()));
}

#[test]
fn a_section_renamed_at_the_cursor_keeps_its_place() {
    let mut harness = editable_demo();
    enter_section(&mut harness, "Opening", egui::Key::Enter);
    assert_eq!(marker_texts(&harness)[0], "Opening");
}

#[test]
fn a_section_name_left_with_escape_changes_nothing() {
    let mut harness = editable_demo();
    enter_section(&mut harness, "Opening", egui::Key::Escape);
    assert_eq!(marker_texts(&harness)[0], "Intro");
}

#[test]
fn a_marker_can_be_removed_from_the_list() {
    let mut harness = editable_demo();
    let remove = "Remove marker Check the stream";
    harness.get_by_label(remove).scroll_to_me();
    harness.run();
    harness.get_by_label(remove).click();
    harness.run();
    assert!(!marker_texts(&harness).contains(&"Check the stream".to_owned()));
    assert!(harness.query_by_label("Check the stream").is_none());
}

#[test]
fn a_read_only_song_jumps_to_markers_without_editing_them() {
    let mut harness = window(Window::new(&[]));
    harness.state_mut().open_demo();
    harness.run();
    click_in_panel(&mut harness, "Verse");
    assert!(cursor_shows(&harness, "8:0:0"));
    assert!(
        harness
            .query_by_role_and_label(Role::TextInput, "Section here")
            .is_none()
    );
    assert!(harness.query_by_label("Remove marker Verse").is_none());
}

#[test]
fn home_and_end_jump_to_the_start_and_to_where_the_last_note_ends() {
    let mut harness = editable_demo();
    harness.key_press(egui::Key::End);
    harness.run();
    let song = harness.state().song.as_ref().unwrap();
    assert_eq!(song.cursor(), song.end());
    assert_ne!(song.cursor(), Tick::new(0));
    harness.key_press(egui::Key::Home);
    harness.run();
    assert!(cursor_shows(&harness, "0:0:0"));
}

#[test]
fn alt_up_and_down_jump_between_sections() {
    let mut harness = editable_demo();
    harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowDown);
    harness.run();
    assert!(cursor_shows(&harness, "8:0:0"));
    harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowDown);
    harness.run();
    assert!(cursor_shows(&harness, "16:0:0"));
    harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowUp);
    harness.run();
    assert!(cursor_shows(&harness, "8:0:0"));
}

#[test]
fn ctrl_b_toggles_a_bookmark_at_the_cursor() {
    let mut harness = editable_demo();
    let before = marker_texts(&harness).len();
    harness.key_press(egui::Key::ArrowDown);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::B);
    harness.run();
    assert_eq!(marker_texts(&harness).len(), before + 1);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::B);
    harness.run();
    assert_eq!(marker_texts(&harness).len(), before);
}

#[test]
fn f8_and_f7_raise_and_lower_the_tempo_at_the_cursor_a_bpm_at_a_time() {
    let mut harness = editable_demo();
    harness.key_press(egui::Key::F8);
    harness.run();
    assert_eq!(bpm_at_cursor(&harness), Some((0, "151".to_owned())));
    harness.key_press(egui::Key::F7);
    harness.key_press(egui::Key::F7);
    harness.run();
    assert_eq!(bpm_at_cursor(&harness), Some((0, "149".to_owned())));
}

#[test]
fn f1_shows_the_keyboard_shortcuts() {
    let mut harness = editable_demo();
    harness.key_press(egui::Key::F1);
    harness.run();
    let shown = |view: &Harness<'_, Window>| {
        view.query_all_by_label("Keyboard shortcuts")
            .next()
            .is_some()
    };
    assert!(shown(&harness));
    harness.key_press(egui::Key::F1);
    harness.run();
    assert!(!shown(&harness));
}

fn four_taps_at(path: &str) -> Harness<'static, Window> {
    let mut app = Window::new(PLUGINS);
    app.open(Path::new(path), &four_taps());
    window(app)
}

/// Four taps whose audio the host has loaded.
fn playable() -> Harness<'static, Window> {
    let mut harness = four_taps_at("/songs/four.json");
    harness
        .state_mut()
        .audio_ready(Path::new("/songs/a.ogg"), 60.0);
    harness.run();
    harness
}

fn space(harness: &mut Harness<'_, Window>) {
    harness.key_press(egui::Key::Space);
    harness.run();
}

#[test]
fn a_song_from_a_file_wants_the_audio_beside_it() {
    let harness = four_taps_at("/songs/four.json");
    assert_eq!(
        harness.state().audio_file(),
        Some(PathBuf::from("/songs/a.ogg"))
    );
}

#[test]
fn a_new_song_wants_no_audio_until_some_is_dropped_on_it() {
    let mut harness = created();
    assert_eq!(harness.state().audio_file(), None);
    drop_file(&mut harness, "/music/track.ogg");
    assert_eq!(
        harness.state().audio_file(),
        Some(PathBuf::from("/music/track.ogg"))
    );
}

#[test]
fn play_after_an_audio_load_failure_explains_how_to_try_again() {
    let mut harness = four_taps_at("/songs/four.json");
    harness
        .state_mut()
        .audio_failed(Path::new("/songs/a.ogg"), "unsupported codec");
    harness.run();
    harness.get_by_label("Dismiss").click();
    harness.run();
    space(&mut harness);
    assert!(
        harness
            .query_by_label_contains("Choose another audio file")
            .is_some()
    );
    assert_eq!(harness.state_mut().take_requests(), []);
}

#[test]
fn space_plays_from_the_cursor_and_pauses() {
    let mut harness = playable();
    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    space(&mut harness);
    let Request::Play { from, rate, .. } = request(&mut harness) else {
        panic!("expected play");
    };
    assert!((from - 0.125).abs() < 1e-9, "{from}");
    assert!((rate - 1.0).abs() < f64::EPSILON);
    space(&mut harness);
    assert_eq!(request(&mut harness), Request::Pause);
}

#[test]
fn space_before_the_audio_is_ready_says_so() {
    let mut harness = four_taps_at("/songs/four.json");
    space(&mut harness);
    assert_eq!(harness.state_mut().take_requests(), []);
    assert!(harness.query_by_label_contains("still loading").is_some());
}

#[test]
fn audio_that_failed_to_load_says_why_and_does_not_play() {
    let mut harness = four_taps_at("/songs/four.json");
    harness
        .state_mut()
        .audio_failed(Path::new("/songs/a.ogg"), "no such file");
    harness.run();
    assert!(harness.query_by_label_contains("no such file").is_some());
    space(&mut harness);
    assert_eq!(harness.state_mut().take_requests(), []);
}

#[test]
fn the_cursor_follows_the_playhead() {
    let mut harness = playable();
    space(&mut harness);
    harness.state_mut().take_requests();
    harness.state_mut().playhead(0.5);
    harness.run();
    assert!(cursor_shows(&harness, "0:1:0"));
    assert_eq!(harness.state_mut().take_requests(), []);
}

#[test]
fn a_lane_key_while_playing_places_on_the_nearest_snap_line_without_seeking() {
    let mut harness = playable();
    space(&mut harness);
    harness.state_mut().take_requests();
    // 0.63 s is tick 1209 at 120 BPM; the nearest sixteenth is 1200.
    harness.state_mut().playhead(0.63);
    harness.run();
    assert!(lanes_at(&harness, 1200).is_empty());
    harness.key_press(egui::Key::Num1);
    harness.run();
    assert_eq!(lanes_at(&harness, 1200), [0]);
    assert_eq!(harness.state_mut().take_requests(), []);
}

#[test]
fn a_lane_key_while_playing_places_the_input_latency_before_the_cursor() {
    let mut harness = playable_with(Settings {
        input_latency_ms: 250.0,
        ..Settings::default()
    });
    space(&mut harness);
    harness.state_mut().take_requests();
    // Heard at 0.88 s, meant at 0.63 s: tick 1209, the nearest sixteenth 1200.
    harness.state_mut().playhead(0.88);
    harness.run();
    harness.key_press(egui::Key::Num1);
    harness.run();
    assert_eq!(lanes_at(&harness, 1200), [0]);
}

/// Moves the playhead of a playing song to `seconds`, as the host reports each frame.
fn play_to(harness: &mut Harness<'_, Window>, seconds: f64) {
    harness.state_mut().playhead(seconds);
    harness.run();
}

#[test]
fn a_lane_key_held_while_playing_grows_a_hold_until_it_comes_up() {
    let mut harness = playable();
    space(&mut harness);
    play_to(&mut harness, 0.5);
    harness.key_down(egui::Key::Num1);
    harness.run();
    play_to(&mut harness, 0.6);
    assert_eq!(note_at(&harness, 960, 0), Some(tap()));
    play_to(&mut harness, 0.9);
    assert_eq!(note_at(&harness, 960, 0), Some(hold(720)));
    play_to(&mut harness, 1.13);
    harness.key_up(egui::Key::Num1);
    harness.run();
    play_to(&mut harness, 1.6);
    assert_eq!(note_at(&harness, 960, 0), Some(hold(1200)));
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(note_at(&harness, 960, 0), None);
}

#[test]
fn a_chord_held_while_playing_undoes_in_one_step() {
    let mut harness = playable();
    space(&mut harness);
    play_to(&mut harness, 0.5);
    harness.key_down(egui::Key::Num1);
    harness.key_down(egui::Key::Num2);
    harness.run();
    for seconds in [0.75, 0.9, 1.1] {
        play_to(&mut harness, seconds);
    }
    harness.key_up(egui::Key::Num1);
    harness.key_up(egui::Key::Num2);
    harness.run();
    assert!(note_at(&harness, 960, 0).is_some_and(|note| note["kind"] == "hold"));
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(note_at(&harness, 960, 0), None);
    assert_eq!(note_at(&harness, 960, 1), None);
}

#[test]
fn an_edit_made_while_a_chord_is_held_is_its_own_step() {
    let mut harness = editable_four_taps();
    harness.key_press(egui::Key::PageDown);
    harness.run();
    harness.key_down(egui::Key::Num1);
    harness.key_down(egui::Key::Num2);
    harness.run();
    harness.key_press(egui::Key::F8);
    harness.run();
    harness.key_up(egui::Key::Num1);
    harness.key_up(egui::Key::Num2);
    harness.run();
    let bpm = |view: &Harness<'_, Window>| {
        let song = view.state().song.as_ref().unwrap();
        song.timing().tempo_at(Tick::new(0))
    };
    let raised = bpm(&harness);
    assert_eq!(note_count(&harness), 6);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert!(bpm(&harness) < raised, "the BPM change undoes alone");
    assert_eq!(note_count(&harness), 6);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(
        lanes_at(&harness, 3840),
        Vec::<u16>::new(),
        "then the chord as one"
    );
}

#[test]
fn a_chord_of_mines_undoes_in_one_step() {
    let mut harness = editable_four_taps();
    harness.key_press(egui::Key::PageDown);
    harness.run();
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
    for key in [egui::Key::Num1, egui::Key::Num2] {
        harness.event(egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::SHIFT,
        });
    }
    harness.run();
    for key in [egui::Key::Num1, egui::Key::Num2] {
        harness.event(egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::SHIFT,
        });
    }
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    harness.run();
    assert_eq!(note_count(&harness), 6);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(note_count(&harness), 4);
}

#[test]
fn a_lane_key_let_go_quickly_while_playing_stays_a_tap() {
    let mut harness = playable();
    space(&mut harness);
    play_to(&mut harness, 0.5);
    harness.key_down(egui::Key::Num1);
    harness.run();
    play_to(&mut harness, 0.62);
    harness.key_up(egui::Key::Num1);
    harness.run();
    play_to(&mut harness, 1.0);
    assert_eq!(note_at(&harness, 960, 0), Some(tap()));
}

#[test]
fn moving_the_cursor_while_playing_seeks_there() {
    let mut harness = playable();
    space(&mut harness);
    harness.state_mut().take_requests();
    harness.key_press(egui::Key::PageDown);
    harness.run();
    let Request::Seek(to) = request(&mut harness) else {
        panic!("expected a seek");
    };
    assert!((to - 2.0).abs() < 1e-9, "{to}");
}

#[test]
fn audio_under_clicks_plays_again_at_a_new_speed_even_letting_its_pitch_follow() {
    let mut harness = playable_with(Settings {
        keep_pitch: false,
        metronome: true,
        ..Settings::default()
    });
    space(&mut harness);
    let Request::Play { clicks, .. } = request(&mut harness) else {
        panic!("expected playback");
    };
    assert!(!clicks.is_empty());
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowLeft);
    harness.run();
    let requests = harness.state_mut().take_requests();
    let Some(Request::Play { rate, .. }) = requests.last() else {
        panic!("expected playback again, got {requests:?}");
    };
    assert!((rate - 0.9).abs() < 1e-9, "{rate}");
}

#[test]
fn playback_that_ends_stops() {
    let mut harness = playable();
    space(&mut harness);
    harness.state_mut().take_requests();
    harness.state_mut().playback_ended();
    harness.run();
    space(&mut harness);
    assert!(matches!(request(&mut harness), Request::Play { .. }));
}

#[test]
fn shift_left_and_right_change_the_playback_rate_without_touching_the_chart() {
    let mut harness = playable_with(Settings {
        keep_pitch: false,
        ..Settings::default()
    });
    let before = harness.state().song.as_ref().unwrap().document().clone();
    space(&mut harness);
    harness.state_mut().take_requests();
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowLeft);
    harness.run();
    let Request::Rate(rate) = request(&mut harness) else {
        panic!("expected a rate");
    };
    assert!((rate - 0.9).abs() < 1e-9, "{rate}");
    assert!(harness.query_by_label("Speed 90%").is_some());
    let command = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);
    harness.key_press_modifiers(command, egui::Key::ArrowRight);
    harness.run();
    let Request::Rate(requested_rate) = request(&mut harness) else {
        panic!("expected a rate");
    };
    assert!((requested_rate - 0.91).abs() < 1e-9, "{requested_rate}");
    let after = harness.state().song.as_ref().unwrap().document();
    assert_eq!(after.document(), before.document());
}

#[test]
fn audio_keeping_its_pitch_plays_again_at_a_new_speed() {
    let mut harness = playable();
    space(&mut harness);
    let Request::Play { keep_pitch, .. } = request(&mut harness) else {
        panic!("expected playback");
    };
    assert!(keep_pitch);
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowLeft);
    harness.run();
    let requests = harness.state_mut().take_requests();
    let Some(Request::Play { rate, .. }) = requests.last() else {
        panic!("expected playback again, got {requests:?}");
    };
    assert!((rate - 0.9).abs() < 1e-9, "{rate}");
}

#[test]
fn changing_the_sounds_in_the_settings_while_playing_plays_again_with_them() {
    let mut harness = playable_with(Settings {
        note_ticks: true,
        ..Settings::default()
    });
    space(&mut harness);
    harness.state_mut().take_requests();
    harness.state_mut().settings.tuned_lanes = true;
    harness.run();
    let requests = harness.state_mut().take_requests();
    let Some(Request::Play { clicks, .. }) = requests.last() else {
        panic!("expected playback again, got {requests:?}");
    };
    assert!(
        clicks
            .iter()
            .any(|(_, tone)| *tone != charter_audio::Tick::Click.tone())
    );
    harness.run();
    assert_eq!(harness.state_mut().take_requests(), [], "once");
}

#[test]
fn turning_keep_pitch_off_while_playing_plays_again_and_speeds_follow() {
    let mut harness = playable();
    harness.state_mut().set_rate(Rate::of(0.6));
    space(&mut harness);
    harness.state_mut().take_requests();
    harness.state_mut().settings.keep_pitch = false;
    harness.run();
    let requests = harness.state_mut().take_requests();
    assert!(
        matches!(
            requests.last(),
            Some(Request::Play {
                keep_pitch: false,
                ..
            })
        ),
        "{requests:?}"
    );
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowRight);
    harness.run();
    let Request::Rate(rate) = request(&mut harness) else {
        panic!("expected a rate");
    };
    assert!((rate - 0.7).abs() < 1e-9, "{rate}");
}

#[test]
fn playback_menu_exposes_the_fine_speed_shortcuts() {
    let mut harness = playable();
    harness.get_by_label("Playback").click();
    harness.run();

    harness.get_by_label_contains("Slower by 1%").click();
    harness.run();

    assert!(harness.query_by_label("Speed 99%").is_some());
}

#[test]
fn the_playback_rate_stays_between_a_tenth_and_four_times() {
    let mut harness = playable();
    for _ in 0..40 {
        harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowLeft);
    }
    harness.run();
    assert!(harness.query_by_label("Speed 10%").is_some());
    space(&mut harness);
    let Request::Play { rate, .. } = request(&mut harness) else {
        panic!("expected play");
    };
    assert!((rate - 0.1).abs() < 1e-9, "{rate}");
}

fn playable_with(settings: Settings) -> Harness<'static, Window> {
    let mut app = Window::new(PLUGINS).with_settings(settings);
    app.open(Path::new("/songs/four.json"), &four_taps());
    let mut harness = window(app);
    harness
        .state_mut()
        .audio_ready(Path::new("/songs/a.ogg"), 2.0);
    harness.run();
    harness
}

fn clicks_played(harness: &mut Harness<'_, Window>) -> Vec<(f64, Tone)> {
    space(harness);
    let Request::Play { clicks, .. } = request(harness) else {
        panic!("expected play");
    };
    clicks
}

fn quieter() -> Volumes {
    Volumes {
        music: Gain::from(0.5),
        ticks: Gain::from(0.25),
        metronome: Gain::from(0.75),
        subdivision: Gain::from(0.5),
    }
}

#[test]
fn playback_plays_at_the_volumes_the_settings_hold() {
    let mut harness = playable_with(Settings {
        volumes: quieter(),
        ..Settings::default()
    });
    space(&mut harness);
    let Request::Play { volumes, .. } = request(&mut harness) else {
        panic!("expected play");
    };
    assert_eq!(volumes, quieter());
}

#[test]
fn with_no_clicks_the_music_volume_changes_live() {
    let mut harness = playable_with(Settings::default());
    space(&mut harness);
    harness.state_mut().take_requests();
    harness.state_mut().settings.volumes.music = Gain::from(0.3);
    harness.run();
    assert_eq!(request(&mut harness), Request::Volume(Gain::from(0.3)));
}

#[test]
fn with_clicks_mixed_in_any_volume_change_plays_on_from_the_cursor() {
    let mut harness = playable_with(Settings {
        metronome: true,
        ..Settings::default()
    });
    space(&mut harness);
    harness.state_mut().take_requests();
    for change in [
        |volumes: &mut Volumes| volumes.music = Gain::from(0.3),
        |volumes: &mut Volumes| volumes.metronome = Gain::from(0.2),
    ] {
        change(&mut harness.state_mut().settings.volumes);
        harness.run();
        let requests = harness.state_mut().take_requests();
        let wanted = harness.state().settings.volumes;
        assert!(
            matches!(
                requests.as_slice(),
                [Request::Pause, Request::Play { volumes, .. }] if *volumes == wanted
            ),
            "{requests:?}"
        );
    }
}

#[test]
fn the_toolbar_and_the_sounds_page_both_set_the_volumes() {
    let mut harness = playable_with(Settings::default());
    harness.get_by_label("Volume").click();
    harness.run();
    assert!(
        harness
            .query_by_role_and_label(Role::Slider, "Music volume")
            .is_some()
    );
    harness.key_press(egui::Key::Escape);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    harness.run();
    settings_page(&mut harness, "Sounds");
    for name in ["Music volume", "Note tick volume", "Metronome volume"] {
        assert!(
            harness
                .query_by_role_and_label(Role::Slider, name)
                .is_some(),
            "{name}"
        );
    }
}

#[test]
fn with_no_sounds_chosen_playback_clicks_nothing() {
    let mut harness = playable_with(Settings::default());
    assert_eq!(clicks_played(&mut harness), []);
}

#[test]
fn the_toolbar_says_when_the_grid_draws_the_snap_s_lines_too() {
    let mut harness = editable_four_taps();
    assert!(harness.query_by_label("+ snap").is_some());
    for _ in 0..8 {
        harness.key_press(egui::Key::ArrowLeft);
    }
    harness.run();
    assert!(harness.query_by_label("+ snap").is_none());
}

#[test]
fn the_grid_draws_every_snap_line() {
    let n = |value: u16| NonZeroU16::new(value).unwrap();
    assert_eq!(shown_grid(n(4), n(16)), n(16));
    assert_eq!(shown_grid(n(16), n(4)), n(16));
    assert_eq!(shown_grid(n(48), n(12)), n(48));
    assert_eq!(shown_grid(n(16), n(12)), n(48));
    assert_eq!(shown_grid(n(48), n(64)), n(192));
    assert_eq!(shown_grid(n(16), n(16)), n(16));
}

#[test]
fn tuned_lanes_step_a_whole_tone_from_the_middle_and_span_an_octave_at_most() {
    let at = |index: u16, width: u16, lanes: u16| {
        let position = charter_core::Position::Lane {
            index: LaneIndex::new(index),
            width: LaneCount::new(width).unwrap(),
        };
        lane_cents(position, lanes)
    };
    let row = |lanes: u16| {
        (0..lanes)
            .map(|lane| at(lane, 1, lanes))
            .collect::<Vec<_>>()
    };
    assert_eq!(row(1), [0]);
    assert_eq!(row(3), [-200, 0, 200]);
    assert_eq!(row(4), [-300, -100, 100, 300]);
    let wide = row(13);
    assert_eq!(
        (wide.first(), wide.get(6), wide.last()),
        (Some(&-600), Some(&0), Some(&600))
    );
    assert_eq!(at(0, 2, 4), -200);
}

#[test]
fn shift_f4_tunes_the_note_ticks_by_lane() {
    let mut harness = playable_with(Settings {
        note_ticks: true,
        tick_sound: Sound::Clap,
        ..Settings::default()
    });
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F4);
    harness.run();
    assert!(harness.state().settings().tuned_lanes);
    let clicks = clicks_played(&mut harness);
    let tones: Vec<_> = clicks.iter().map(|(_, tone)| *tone).collect();
    let clap = |cents| charter_audio::Tick::Clap.pitched(cents);
    assert_eq!(tones, [clap(-300), clap(-100), clap(100), clap(300)]);
    space(&mut harness);
    harness.get_by_label("Tuned lanes").click();
    harness.run();
    assert!(!harness.state().settings().tuned_lanes);
}

#[test]
fn the_tick_sound_plays_on_every_note_even_before_the_cursor() {
    let mut harness = playable_with(Settings {
        note_ticks: true,
        tick_sound: Sound::Beep,
        ..Settings::default()
    });
    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    let clicks = clicks_played(&mut harness);
    let times: Vec<f64> = clicks.iter().map(|(at, _)| *at).collect();
    assert_eq!(times, [0.0, 0.125, 0.25, 0.375]);
    assert!(
        clicks
            .iter()
            .all(|(_, tone)| *tone == charter_audio::Tick::Beep.tone())
    );
}

#[test]
fn the_metronome_plays_on_every_beat_until_the_audio_ends_accenting_each_measure() {
    let mut harness = playable_with(Settings {
        metronome: true,
        metronome_sound: Kit::PureSine,
        ..Settings::default()
    });
    let clicks = clicks_played(&mut harness);
    let times: Vec<f64> = clicks.iter().map(|(at, _)| *at).collect();
    assert_eq!(times, [0.0, 0.5, 1.0, 1.5, 2.0]);
    let tones: Vec<Tone> = clicks.iter().map(|(_, tone)| *tone).collect();
    let (accent, beat) = (Tone::Accent(Kit::PureSine), Tone::Beat(Kit::PureSine));
    assert_eq!(tones, [accent, beat, beat, beat, accent]);
}

#[test]
fn the_metronome_follows_every_timing_point_offset_tempo_meter_and_stop() {
    let song = serde_json::json!({
        "format_version": 0,
        "ruleset": { "id": "sm", "version": "1" },
        "metadata": { "title": "Timed", "artist": "a", "audio": "a.ogg" },
        "timing": { "offset_seconds": 0.5, "events": [
            { "tick": 0, "kind": { "bpm": 120.0 } },
            { "tick": 3840, "kind": { "bpm": 240.0 } },
            { "tick": 3840, "kind": { "time_signature": [3, 4] } },
            { "tick": 4800, "kind": { "stop": 0.5 } }
        ] },
        "charts": [ { "name": "Easy", "lanes": 4 } ]
    });
    let settings = Settings {
        metronome: true,
        metronome_sound: Kit::PureSine,
        ..Settings::default()
    };
    let mut app = Window::new(PLUGINS).with_settings(settings);
    app.open(
        Path::new("/songs/timed.json"),
        &serde_json::to_vec(&song).unwrap(),
    );
    let mut harness = window(app);
    harness
        .state_mut()
        .audio_ready(Path::new("/songs/a.ogg"), 4.2);
    harness.run();
    let clicks = clicks_played(&mut harness);
    let (a, b) = (Tone::Accent(Kit::PureSine), Tone::Beat(Kit::PureSine));
    // 120 BPM in 4/4 from 0.5 s, its grid carried back into the lead-in; from 2.5 s,
    // 240 BPM in 3/4, with half a second stopped after the beat at 2.75 s.
    let expected = [
        (0.0, b),
        (0.5, a),
        (1.0, b),
        (1.5, b),
        (2.0, b),
        (2.5, a),
        (2.75, b),
        (3.5, b),
        (3.75, a),
        (4.0, b),
    ];
    assert_eq!(clicks.len(), expected.len(), "{clicks:?}");
    for ((at, tone), (want_at, want_tone)) in clicks.iter().zip(expected) {
        assert!((at - want_at).abs() < 1e-9, "{clicks:?}");
        assert_eq!(*tone, want_tone, "{clicks:?}");
    }
}

/// Four taps whose two seconds of audio have loaded, with the timing window open.
fn listening(settings: Settings) -> Harness<'static, Window> {
    let mut harness = playable_with(settings);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
    harness.run();
    harness.state_mut().take_requests();
    harness
}

/// Where a play started, what it loops and what clicks over it.
type Played = (f64, Option<(f64, f64)>, Vec<(f64, Tone)>);

fn played(harness: &mut Harness<'_, Window>) -> Played {
    let requests = harness.state_mut().take_requests();
    let play = requests.into_iter().rev().find_map(|request| {
        if let Request::Play {
            from,
            looping,
            clicks,
            ..
        } = request
        {
            Some((from, looping, clicks))
        } else {
            None
        }
    });
    play.expect("a play")
}

#[test]
fn l_loops_the_bar_under_the_cursor_with_the_metronome() {
    let mut harness = listening(Settings::default());
    harness.key_press(egui::Key::L);
    harness.run();
    let (from, looping, clicks) = played(&mut harness);
    assert!(from.abs() < 1e-9);
    assert_eq!(looping, Some((0.0, 2.0)));
    assert!(
        clicks
            .iter()
            .any(|(_, tone)| matches!(tone, Tone::Accent(_)))
    );
    harness.key_press(egui::Key::L);
    harness.run();
    assert_eq!(harness.state_mut().take_requests(), [Request::Pause]);
}

#[test]
fn an_edit_while_looping_plays_the_loop_again_from_its_start() {
    let mut harness = listening(Settings::default());
    harness.key_press(egui::Key::L);
    harness.run();
    played(&mut harness);
    harness.key_press(egui::Key::Plus);
    harness.run();
    let (from, looping, _) = played(&mut harness);
    assert!(from.abs() < 1e-9);
    let (_, end) = looping.expect("looping");
    assert!(
        (end - 2.0).abs() > 1e-3,
        "the bar follows the new tempo: {end}"
    );
}

#[test]
fn r_switches_to_the_alternate_speed_and_back() {
    let mut harness = listening(Settings::default());
    harness.key_press(egui::Key::R);
    harness.run();
    assert!((harness.state().playback.rate.factor() - 0.6).abs() < 1e-9);
    harness.key_press(egui::Key::R);
    harness.run();
    assert!((harness.state().playback.rate.factor() - 1.0).abs() < 1e-9);
}

#[test]
fn a_metronome_following_the_grid_clicks_between_beats_too() {
    let mut harness = listening(Settings {
        metronome: true,
        metronome_grid: true,
        ..Settings::default()
    });
    harness.key_press(egui::Key::Num6);
    harness.run();
    space(&mut harness);
    let (_, _, clicks) = played(&mut harness);
    let subs = clicks
        .iter()
        .filter(|(_, tone)| matches!(tone, Tone::Sub(_)))
        .count();
    let beats = clicks
        .iter()
        .filter(|(_, tone)| matches!(tone, Tone::Beat(_)))
        .count();
    assert_eq!((beats, subs), (3, 4), "{clicks:?}");
}

#[test]
fn c_toggles_the_metronome_and_shift_c_mutes_it_while_held() {
    let mut harness = listening(Settings {
        metronome: true,
        ..Settings::default()
    });
    space(&mut harness);
    played(&mut harness);
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
    harness.key_down_modifiers(egui::Modifiers::SHIFT, egui::Key::C);
    harness.run();
    let (_, _, muted) = played(&mut harness);
    assert!(muted.is_empty(), "{muted:?}");
    harness.key_up_modifiers(egui::Modifiers::SHIFT, egui::Key::C);
    harness.run();
    let (_, _, back) = played(&mut harness);
    assert!(!back.is_empty());
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    harness.key_press(egui::Key::C);
    harness.run();
    assert!(!harness.state().settings().metronome);
}

#[test]
#[expect(
    clippy::non_ascii_literal,
    reason = "the test locates the Calibrate button with its ellipsis"
)]
fn calibrating_latency_plays_clicks_at_the_songs_first_tempo() {
    let mut harness = editable_demo();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    harness.run();
    settings_page(&mut harness, "Sounds");
    harness.get_by_label("Calibrate…").scroll_to_me();
    harness.run();
    harness.get_by_label("Calibrate…").click();
    harness.run();
    harness.get_by_label("Start").click();
    // The flash keeps the window drawing while the clicks play.
    harness.run_steps(2);
    let requests = harness.state_mut().take_requests();
    let Some(Request::Preview { clicks, .. }) = requests.last() else {
        panic!("expected clicks, got {requests:?}");
    };
    assert_eq!(clicks.len(), 32);
    let gap = clicks[1].0 - clicks[0].0;
    assert!(
        (gap - 0.4).abs() < 1e-9,
        "150 BPM, the demo's first tempo: {gap}"
    );
    assert!(harness.query_by_label_contains("of 16 taps").is_some());
}

#[test]
fn the_cursor_shows_what_is_heard_by_the_calibrated_latency() {
    let mut harness = playable_with(Settings {
        audio_latency_ms: 250.0,
        ..Settings::default()
    });
    space(&mut harness);
    harness.state_mut().take_requests();
    harness.state_mut().playhead(0.75);
    harness.run();
    assert!(cursor_shows(&harness, "0:1:0"));
}

#[test]
fn the_cursor_scales_calibrated_latency_to_slowed_audio_time() {
    let mut harness = playable_with(Settings {
        audio_latency_ms: 250.0,
        ..Settings::default()
    });
    harness.state_mut().set_rate(Rate::of(0.5));
    space(&mut harness);
    let Request::Play { rate, .. } = request(&mut harness) else {
        panic!("expected playback");
    };
    assert!((rate - 0.5).abs() < f64::EPSILON);

    harness.state_mut().playhead(0.75);
    harness.run();
    assert_eq!(
        harness.state().song.as_ref().unwrap().cursor(),
        Tick::new(1200)
    );
}

#[test]
fn the_toolbar_draws_grid_lines_between_the_beats() {
    let mut harness = editable_demo();
    harness
        .get_by_role_and_label(Role::ComboBox, "Grid lines")
        .click();
    harness.run();
    harness.get_by_label("16th notes").click();
    harness.run();
    assert_eq!(harness.state().settings().grid.get(), 16);
    let grid = harness.get_by_role_and_label(Role::ComboBox, "Grid lines");
    assert_eq!(grid.value().as_deref(), Some("16th notes"));
}

#[test]
fn f3_and_f4_turn_the_metronome_and_the_note_ticks_on_and_off() {
    let mut harness = editable_demo();
    harness.key_press(egui::Key::F3);
    harness.key_press(egui::Key::F4);
    harness.run();
    let settings = harness.state().settings();
    assert_eq!((settings.metronome, settings.note_ticks), (true, true));
    harness.key_press(egui::Key::F3);
    harness.run();
    assert!(!harness.state().settings().metronome);
}

#[test]
fn saving_a_song_into_another_folder_plans_its_audio_copy() {
    let mut harness = four_taps_at("/songs/four.json");
    let path = PathBuf::from("/elsewhere/four.json");
    assert_eq!(
        harness.state().audio_copy_for(&path),
        Some((
            PathBuf::from("/songs/a.ogg"),
            PathBuf::from("/elsewhere/a.ogg"),
        ))
    );
    harness.state_mut().saved(Saved::To(path));
    harness.run();
    assert_eq!(harness.state_mut().take_requests(), []);
}

#[test]
fn saving_elsewhere_keeps_the_audios_relative_subdirectory() {
    let mut document: serde_json::Value = serde_json::from_slice(&four_taps()).unwrap();
    document["metadata"]["audio"] = serde_json::json!("assets/track.ogg");
    let mut app = Window::new(PLUGINS);
    app.open(
        Path::new("/songs/four.json"),
        &serde_json::to_vec(&document).unwrap(),
    );
    let mut harness = window(app);

    harness.key_press_modifiers(
        egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
        egui::Key::S,
    );
    harness.run();
    let Request::SaveAs { bytes, .. } = request(&mut harness) else {
        panic!("expected Save As");
    };
    let saved: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(saved["metadata"]["audio"], "assets/track.ogg");
    let path = PathBuf::from("/elsewhere/four.json");
    assert_eq!(
        harness.state().audio_copy_for(&path),
        Some((
            PathBuf::from("/songs/assets/track.ogg"),
            PathBuf::from("/elsewhere/assets/track.ogg"),
        ))
    );
    harness.state_mut().saved(Saved::To(path));
    harness.run();
    assert_eq!(harness.state_mut().take_requests(), []);
}

#[test]
fn a_host_can_preflight_audio_without_marking_a_failed_save_as_saved() {
    let mut harness = created();
    drop_file(&mut harness, "/music/track.ogg");
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    harness.run();
    assert!(matches!(request(&mut harness), Request::SaveAs { .. }));

    let original = harness.state().audio_file();
    assert_eq!(
        harness
            .state()
            .audio_copy_for(Path::new("/songs/mine.json")),
        Some((
            PathBuf::from("/music/track.ogg"),
            PathBuf::from("/songs/track.ogg"),
        ))
    );
    assert_eq!(harness.state().audio_file(), original);
    harness
        .state_mut()
        .saved(Saved::Failed("the copy failed".to_owned()));
    harness.run();
    assert_eq!(harness.state().audio_file(), original);
    assert!(harness.state().song.as_ref().unwrap().is_modified());
    assert_eq!(harness.state().path, None);
    assert_eq!(harness.state_mut().take_requests(), []);
}

#[test]
fn a_confirmed_save_after_audio_preflight_queues_no_second_copy() {
    let mut harness = created();
    drop_file(&mut harness, "/music/track.ogg");
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    harness.run();
    assert!(matches!(request(&mut harness), Request::SaveAs { .. }));
    let path = PathBuf::from("/songs/mine.json");
    assert!(harness.state().audio_copy_for(&path).is_some());

    harness.state_mut().saved(Saved::To(path));
    harness.run();
    assert_eq!(harness.state_mut().take_requests(), []);
    assert_eq!(
        harness.state().audio_file(),
        Some(PathBuf::from("/songs/track.ogg"))
    );
    assert!(!harness.state().song.as_ref().unwrap().is_modified());
}

#[test]
fn saving_a_song_in_its_own_folder_copies_nothing() {
    let mut harness = four_taps_at("/songs/four.json");
    let path = PathBuf::from("/songs/renamed.json");
    assert_eq!(harness.state().audio_copy_for(&path), None);
    harness.state_mut().saved(Saved::To(path));
    harness.run();
    assert_eq!(harness.state_mut().take_requests(), []);
}

#[test]
fn audio_that_moves_under_the_song_stops_playing_until_it_loads_again() {
    let mut harness = playable();
    space(&mut harness);
    harness.state_mut().take_requests();
    harness
        .state_mut()
        .saved(Saved::To(PathBuf::from("/elsewhere/four.json")));
    harness.run();
    harness.state_mut().take_requests();
    space(&mut harness);
    assert_eq!(harness.state_mut().take_requests(), []);
    assert!(harness.query_by_label_contains("still loading").is_some());
}

#[test]
fn ctrl_dragging_from_a_note_stretches_it_rather_than_removing_it() {
    let mut harness = editable_four_taps();
    let (from, to) = (spot(&harness, 1, 240, 0.0), spot(&harness, 1, 720, -3.0));
    drag(&mut harness, from, to, egui::Modifiers::COMMAND);
    assert_eq!(note_at(&harness, 240, 1), Some(hold(480)));
}

#[test]
fn opening_another_song_while_playing_pauses_first() {
    let mut harness = playable();
    space(&mut harness);
    harness.state_mut().take_requests();
    harness.state_mut().open_demo();
    harness.run();
    assert_eq!(harness.state_mut().take_requests(), [Request::Pause]);
    assert!(!harness.state().is_playing());
}

#[test]
fn saving_while_playing_saves_and_keeps_playing() {
    let mut harness = playable();
    space(&mut harness);
    harness.state_mut().take_requests();
    harness.key_press(egui::Key::Num1);
    harness.run();
    harness.state_mut().take_requests();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    harness.run();
    assert!(matches!(request(&mut harness), Request::Write { .. }));
    assert!(harness.state().is_playing());
}

#[test]
fn a_restore_offered_while_starting_a_song_can_still_be_answered() {
    let (_, bytes) = autosaved_four_taps();
    let mut harness = new_song_dialog();
    harness.state_mut().recover(&bytes);
    harness.run();
    harness.get_by_label("Restore").click();
    harness.run();
    assert_eq!(harness.state().title(), "*four.json - charter");
}

#[test]
fn audio_whose_name_cannot_be_a_song_path_is_refused() {
    let mut harness = created();
    drop_file(&mut harness, "/music/a\\b.ogg");
    assert_eq!(audio(&harness), NO_AUDIO);
    assert!(
        harness
            .query_by_label_contains("cannot be a song's audio")
            .is_some()
    );
}

#[test]
fn audio_dropped_then_undone_is_not_copied_on_save() {
    let mut harness = created();
    drop_file(&mut harness, "/music/track.ogg");
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    harness
        .state_mut()
        .saved(Saved::To(PathBuf::from("/songs/mine.json")));
    harness.run();
    assert_eq!(harness.state_mut().take_requests(), []);
}

#[test]
fn lowering_the_tempo_below_nothing_says_so() {
    let mut harness = editable_demo();
    enter_bpm(&mut harness, "0.5", egui::Key::Enter);
    harness.key_press(egui::Key::F7);
    harness.run();
    assert!(harness.query_by_label_contains("is not a tempo").is_some());
    assert_eq!(bpm_at_cursor(&harness), Some((0, "0.5".to_owned())));
}

#[test]
fn a_box_and_a_dragged_hold_work_however_the_notefield_runs() {
    for direction in Direction::ALL {
        let mut harness = turned_demo(direction);
        let (from, to) = (turned_spot(&harness, 0, 480), turned_spot(&harness, 0, 960));
        drag(&mut harness, from, to, egui::Modifiers::COMMAND);
        assert_eq!(note_at(&harness, 480, 0), Some(hold(480)), "{direction:?}");
        let corner = |lane, tick| turned_spot(&harness, lane, tick);
        let (a, b) = (corner(0, 470), corner(3, 490));
        let (a, b) = (
            a + (a - b).normalized() * 30.0,
            b + (b - a).normalized() * 30.0,
        );
        drag(&mut harness, a, b, egui::Modifiers::NONE);
        let song = harness.state().song.as_ref().unwrap();
        assert!(song.selection().count() >= 1, "{direction:?}");
    }
}

#[test]
#[expect(
    clippy::non_ascii_literal,
    reason = "the test clicks the Open song button with its ellipsis"
)]
fn the_empty_window_opens_a_song_from_a_file() {
    let mut harness = window(Window::new(GAMES));
    harness.get_by_label("Open song…").click();
    harness.run();
    assert_eq!(request(&mut harness), Request::Open);
}

#[test]
fn a_song_whose_audio_cannot_be_found_says_how_to_play_it() {
    let mut harness = editable_demo();
    space(&mut harness);
    assert!(
        harness
            .query_by_label_contains("drop demo.ogg on the window")
            .is_some()
    );
}

#[test]
fn a_message_can_be_dismissed() {
    let mut harness = editable_demo();
    space(&mut harness);
    harness.get_by_label("Dismiss").click();
    harness.run();
    assert_eq!(harness.state().message(), None);
    assert!(harness.query_by_label("Dismiss").is_none());
}

#[test]
#[expect(
    clippy::non_ascii_literal,
    reason = "the test clicks the Choose audio button with its ellipsis"
)]
fn a_new_song_asks_the_host_to_choose_its_audio() {
    let mut harness = created();
    harness.get_by_label("Choose audio…").click();
    harness.run();
    assert_eq!(request(&mut harness), Request::ChooseAudio);
    harness.state_mut().use_audio(Path::new("/music/track.ogg"));
    harness.run();
    assert_eq!(audio(&harness), "track.ogg");
    assert!(!no_audio(&harness));
}

#[test]
#[expect(
    clippy::non_ascii_literal,
    reason = "the test matches both Choose audio buttons with their ellipses"
)]
fn where_files_cannot_be_dropped_the_window_offers_choosing_them_and_says_why() {
    let mut harness = window(Window::new(GAMES).without_file_drops());
    assert!(
        harness
            .query_by_label_contains("drop a charter song")
            .is_none()
    );
    harness.get_by_label("New song").click();
    harness.run();
    assert!(harness.query_by_label_contains("Drop an audio").is_none());
    assert!(
        harness
            .query_by_label_contains("doesn't work under Wayland yet")
            .is_some()
    );
    harness.get_by_label("Create").click();
    harness.run();
    assert!(harness.query_by_label_contains("Drop an audio").is_none());
    harness.get_by_label("Choose audio…").click();
    harness.run();
    assert_eq!(harness.state_mut().take_requests(), [Request::ChooseAudio]);
    harness.get_by_label("File").click();
    harness.run();
    // The banner's button, and now the File menu's.
    assert_eq!(harness.query_all_by_label("Choose audio…").count(), 2);
}

#[test]
fn the_new_song_dialog_says_where_the_audio_comes_from() {
    let harness = new_song_dialog();
    assert!(
        harness
            .query_by_label_contains("Drop an audio file here")
            .is_some()
    );
}

#[test]
fn saving_opening_and_help_work_while_a_field_has_the_keyboard() {
    let mut harness = editable_four_taps();
    bpm_field(&harness).focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    harness.run();
    assert!(matches!(request(&mut harness), Request::Write { .. }));
    harness.key_press(egui::Key::Space);
    harness.run();
    assert_eq!(harness.state_mut().take_requests(), []);
    assert!(!harness.state().is_playing());
}

#[test]
fn the_timing_menu_adds_a_bookmark_and_jumps_between_sections() {
    let mut harness = editable_demo();
    let before = marker_texts(&harness).len();
    harness.get_by_label("Timing").click();
    harness.run();
    harness
        .get_by_label_contains("Add or remove a bookmark")
        .click();
    harness.run();
    assert_eq!(marker_texts(&harness).len(), before + 1);
    harness.get_by_label("Timing").click();
    harness.run();
    harness.get_by_label_contains("Next section").click();
    harness.run();
    assert!(cursor_shows(&harness, "8:0:0"));
}

#[test]
fn the_edit_menu_selects_and_deletes() {
    let mut harness = editable_four_taps();
    harness.get_by_label("Edit").click();
    harness.run();
    harness.get_by_label_contains("Select all").click();
    harness.run();
    harness.get_by_label("Edit").click();
    harness.run();
    harness
        .get_by_label_contains("Delete the selection")
        .click();
    harness.run();
    assert_eq!(note_count(&harness), 0);
}

#[test]
fn the_shortcuts_filter_to_what_is_typed() {
    let mut harness = editable_demo();
    harness.key_press(egui::Key::F1);
    harness.run();
    let filter = harness.get_by_role_and_label(Role::TextInput, "Filter");
    filter.focus();
    filter.type_text("bookmark");
    harness.run();
    assert!(harness.query_by_label("Ctrl+B").is_some());
    assert!(harness.query_by_label("Ctrl+Z").is_none());
}

#[test]
fn the_status_bar_says_how_the_audio_stands() {
    let harness = created();
    assert!(harness.query_by_label("No audio").is_some());
    let loading_harness = four_taps_at("/songs/four.json");
    assert!(loading_harness.query_by_label("Audio loading").is_some());
    let mut ready_harness = playable();
    assert!(ready_harness.query_by_label("Audio ready").is_some());
    space(&mut ready_harness);
    assert!(ready_harness.query_by_label("Playing").is_some());
}

/// Four lanes at 120 BPM that change to 150 BPM at the second beat, with a section at the
/// first eighth.
fn timed_song() -> Harness<'static, Window> {
    let song = serde_json::json!({
        "format_version": 0,
        "ruleset": { "id": "sm", "version": "0.1.0" },
        "metadata": { "title": "Timed", "artist": "a", "audio": "a.ogg" },
        "timing": { "offset_seconds": 0.0, "events": [
            { "tick": 0, "kind": { "bpm": 120.0 } },
            { "tick": 960, "kind": { "bpm": 150.0 } }
        ] },
        "markers": [ { "tick": 480, "kind": "section", "text": "Drop" } ],
        "charts": [ { "name": "Easy", "lanes": 4 } ]
    });
    let mut app = Window::new(PLUGINS);
    app.open(Path::new("timed.json"), &serde_json::to_vec(&song).unwrap());
    window(app)
}

fn field(harness: &Harness<'_, Window>) -> notefield::Field {
    let area = harness.get_by_label("Notefield").rect();
    let app = harness.state();
    let song = app.song.as_ref().unwrap();
    let chart = song.chart().unwrap();
    let lanes = chart.lanes.map_or(0, u16::from);
    notefield::Field::new(
        area,
        lanes,
        chart.timing().time_at(song.cursor()),
        app.zoom,
        app.strip().is_some(),
    )
}

/// Where the line of `tick` crosses the column at `x`.
fn on_line(harness: &Harness<'_, Window>, x: f32, tick: i32) -> egui::Pos2 {
    let field = field(harness);
    let song = harness.state().song.as_ref().unwrap();
    let y = field.view.y(song.timing().time_at(Tick::new(tick)));
    egui::pos2(x, y)
}

fn event_ticks(harness: &Harness<'_, Window>) -> Vec<Tick> {
    let song = harness.state().song.as_ref().unwrap();
    let timing = song.timing();
    let mut ticks: Vec<_> = timing
        .anchors
        .iter()
        .map(|anchor| anchor.tick)
        .chain(timing.events.iter().map(|event| event.tick))
        .collect();
    ticks.sort_unstable();
    ticks
}

/// The timing window open over `timed_song`, laid out a bar a row, as the gesture tests
/// measure their spots.
fn syncing() -> Harness<'static, Window> {
    let mut harness = timed_song();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
    harness.run();
    harness
        .state_mut()
        .sync
        .set_layout(charter_timing::rows::Layout::Bars);
    harness.run();
    harness
}

#[test]
fn the_timing_window_opens_on_rows_of_seconds() {
    let mut harness = timed_song();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
    harness.run();
    assert!(harness.query_by_label("Row at 0.0 s").is_some());
    assert!(harness.query_by_label("Measure 0").is_none());
}

#[test]
fn the_timing_window_uses_only_the_open_songs_waveform() {
    let mut harness = syncing();
    let waiting = "The waveform shows once the song's audio loads.";
    assert!(harness.query_by_label(waiting).is_some());
    harness
        .state_mut()
        .waveform_ready(Path::new("other.ogg"), peaks());
    harness.run();
    assert!(harness.query_by_label(waiting).is_some());
    harness
        .state_mut()
        .waveform_ready(Path::new("a.ogg"), peaks());
    harness.run();
    assert!(harness.query_by_label(waiting).is_none());
    assert!(harness.output().shapes.iter().any(|shape| matches!(
        &shape.shape,
        egui::Shape::LineSegment { stroke, .. }
            if stroke.color == egui::Color32::from_rgb(118, 158, 210)
    )));
}

#[test]
fn the_timing_windows_start_bpm_sets_the_first_anchors_tempo() {
    let mut harness = syncing();
    let start = harness.get_by_role_and_label(Role::TextInput, "Start BPM");
    let before = start.value();
    start.focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_role_and_label(Role::TextInput, "Start BPM")
        .type_text("140");
    harness.key_press(egui::Key::Enter);
    harness.run();
    let song = harness.state().song.as_ref().unwrap();
    assert_eq!(
        song.timing().bpm_at(Tick::new(0)).map(f64::from),
        Some(140.0)
    );
    assert_ne!(before.as_deref(), Some("140"));
}

/// Where `tick` lies across its bar's row in the timing window, a bar a row with a tenth of
/// the bar before showing ahead of its downbeat.
fn in_row(harness: &Harness<'_, Window>, tick: i32) -> egui::Pos2 {
    let song = harness.state().song.as_ref().unwrap();
    let measure = song.timing().measures_from(Tick::new(tick)).next().unwrap();
    let row = harness
        .get_by_label(&format!("Measure {}", measure.number))
        .rect();
    let lead = charter_timing::rows::LEAD_IN;
    let across = crate::cast::f64_f32((measure.fraction(Tick::new(tick)) + lead) / (1.0 + lead));
    egui::pos2(row.left() + across * row.width(), row.top() + 30.0)
}

fn seconds_at(harness: &Harness<'_, Window>, tick: i32) -> f64 {
    let song = harness.state().song.as_ref().unwrap();
    song.timing().time_at(Tick::new(tick)).seconds()
}

fn anchor_seconds(harness: &Harness<'_, Window>) -> Vec<f64> {
    let song = harness.state().song.as_ref().unwrap();
    song.anchors().iter().map(|pin| pin.seconds).collect()
}

#[expect(
    clippy::non_ascii_literal,
    reason = "the test clicks the Reset timing menu label with its ellipsis"
)]
fn reset_from_the_menu(harness: &mut Harness<'_, Window>) {
    harness.query_all_by_label("Timing").next().unwrap().click();
    harness.run();
    harness.get_by_label("Reset timing…").click();
    harness.run();
}

#[expect(
    clippy::non_ascii_literal,
    reason = "the test clicks the Find timing menu label with its ellipsis"
)]
fn find_from_the_menu(harness: &mut Harness<'_, Window>) {
    harness.query_all_by_label("Timing").next().unwrap().click();
    harness.run();
    harness.get_by_label("Find timing from the audio…").click();
    harness.run();
}

#[test]
fn finding_timing_asks_the_host_to_find_it_in_the_songs_audio() {
    let mut harness = playable();
    find_from_the_menu(&mut harness);
    assert_eq!(
        harness.state_mut().take_requests(),
        [Request::FindTiming {
            file: PathBuf::from("/songs/a.ogg"),
            ticks_per_beat: charter_core::TicksPerBeat::default(),
        }]
    );
    assert!(
        harness
            .query_by_label_contains("Finding the timing")
            .is_some()
    );
}

#[test]
fn the_weights_are_downloaded_only_once_the_user_says_so() {
    let mut harness = playable();
    find_from_the_menu(&mut harness);
    harness.state_mut().take_requests();
    harness.state_mut().weights_needed();
    harness.run();
    assert!(harness.query_by_label_contains("81 MB").is_some());
    harness.get_by_label("Cancel").click();
    harness.run();
    assert_eq!(harness.state_mut().take_requests(), []);
    assert!(
        harness
            .query_by_label_contains("Finding the timing")
            .is_none()
    );

    find_from_the_menu(&mut harness);
    harness.state_mut().take_requests();
    harness.state_mut().weights_needed();
    harness.run();
    harness.get_by_label("Download").click();
    harness.run();
    assert_eq!(
        harness.state_mut().take_requests(),
        [Request::DownloadWeights]
    );
    harness.state_mut().weights_progress(0.42);
    harness.run();
    assert!(harness.query_by_label_contains("42%").is_some());
}

#[test]
fn timing_found_takes_the_songs_place_in_one_step_and_says_what_it_is() {
    let mut harness = playable();
    find_from_the_menu(&mut harness);
    harness.state_mut().take_requests();
    let before = anchor_seconds(&harness);
    let found: charter_core::Timing = serde_json::from_value(serde_json::json!({
        "anchors": [ { "tick": 0, "seconds": 3.616 } ],
        "tempo": 132.0,
        "events": []
    }))
    .unwrap();
    harness.state_mut().timing_found(Ok(found));
    harness.run();
    assert_eq!(anchor_seconds(&harness), [3.616]);
    assert!(
        harness
            .query_by_label_contains("Timing found: 132 BPM")
            .is_some()
    );
    assert!(
        harness
            .query_by_label_contains("Finding the timing")
            .is_none()
    );
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(anchor_seconds(&harness), before);
    harness
        .state_mut()
        .timing_found(Err("the tracker would not load".to_owned()));
    harness.run();
    assert!(harness.query_by_label_contains("would not load").is_some());
}

#[test]
fn resetting_the_timing_asks_first_then_leaves_120_bpm_from_zero_in_one_step() {
    let mut harness = editable_demo();
    let before = harness
        .state()
        .song
        .as_ref()
        .unwrap()
        .timing()
        .events
        .clone();
    reset_from_the_menu(&mut harness);
    harness.get_by_label("Cancel").click();
    harness.run();
    assert_eq!(
        harness.state().song.as_ref().unwrap().timing().events,
        before
    );
    reset_from_the_menu(&mut harness);
    harness.get_by_label("Reset").click();
    harness.run();
    let song = harness.state().song.as_ref().unwrap();
    assert_eq!(song.timing().tempo.to_string(), "120");
    assert_eq!(
        serde_json::to_value(song.timing().anchors[0].seconds).unwrap(),
        serde_json::json!(0.0)
    );
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(
        harness.state().song.as_ref().unwrap().timing().events,
        before
    );
}

#[test]
fn v_lays_the_rows_out_in_seconds_and_a_click_anchors_the_moment_under_it() {
    let mut harness = syncing();
    harness.key_press(egui::Key::V);
    harness.run();
    assert!(harness.query_by_label("Measure 0").is_none());
    let row = harness.get_by_label("Row at 0.0 s").rect();
    // Two seconds a row, a tenth of that from before it: 0.95 s is 1.15 of 2.2 across.
    let across = crate::cast::f64_f32(1.15 / 2.2);
    let at = egui::pos2(row.left() + across * row.width(), row.top() + 30.0);
    click(&mut harness, at, egui::Modifiers::NONE);
    let seconds = anchor_seconds(&harness);
    assert_eq!(seconds.len(), 3, "{seconds:?}");
    assert!((seconds[2] - 0.95).abs() < 5e-3, "{seconds:?}");
    assert!(
        (seconds[1] - 0.5).abs() < 1e-9,
        "the other anchors keep their moments"
    );
    harness.key_press(egui::Key::V);
    harness.run();
    assert!(harness.query_by_label("Measure 0").is_some());
}

#[test]
fn ctrl_t_opens_and_closes_the_timing_window() {
    let mut harness = timed_song();
    assert!(harness.query_by_label("Row at 0.0 s").is_none());
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
    harness.run();
    assert!(harness.query_by_label("Row at 0.0 s").is_some());
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
    harness.run();
    assert!(harness.query_by_label("Row at 0.0 s").is_none());
}

#[test]
fn clicking_a_row_adds_an_anchor_pinning_the_nearest_grid_line_to_the_moment_clicked() {
    let mut harness = syncing();
    let heard = seconds_at(&harness, 1950);
    let near = in_row(&harness, 1950);
    click(&mut harness, near, egui::Modifiers::NONE);
    assert_eq!(
        event_ticks(&harness),
        [Tick::new(0), Tick::new(960), Tick::new(1920)]
    );
    assert!((seconds_at(&harness, 1920) - heard).abs() < 5e-3);
    assert!((seconds_at(&harness, 960) - 0.5).abs() < 1e-9);
}

/// The timed song with its audio loaded and one strong onset `seconds` in.
fn syncing_with_an_onset(seconds: f64) -> Harness<'static, Window> {
    let mut harness = syncing();
    let file = harness.state().audio_file().unwrap();
    harness.state_mut().audio_ready(&file, 60.0);
    let onset = charter_audio::Onset {
        seconds,
        strength: 1.0,
    };
    harness.state_mut().onsets_ready(&file, vec![onset].into());
    harness.run();
    harness.state_mut().take_requests();
    harness
}

#[test]
fn a_click_ten_ms_from_an_onset_anchors_on_it_and_auditions_a_beat_before() {
    // Tick 1920 falls 0.9 s in; the onset is a little later.
    let mut harness = syncing_with_an_onset(0.912);
    let pos = in_row(&harness, 1920);
    assert!((seconds_at(&harness, 1920) - 0.9).abs() < 1e-9);
    click(&mut harness, pos, egui::Modifiers::NONE);
    let seconds = anchor_seconds(&harness);
    assert_eq!(event_ticks(&harness)[2], Tick::new(1920));
    assert!((seconds[2] - 0.912).abs() < 1e-4, "{seconds:?}");
    let requests = harness.state_mut().take_requests();
    let from = requests.iter().find_map(|request| {
        if let Request::Play { from, .. } = request {
            Some(*from)
        } else {
            None
        }
    });
    assert!((from.expect("an audition") - (0.912 - 0.4)).abs() < 1e-6);
}

#[test]
fn a_click_forty_ms_from_any_onset_stays_where_it_was_clicked() {
    let mut harness = syncing_with_an_onset(0.94);
    let heard = seconds_at(&harness, 1920);
    let pos = in_row(&harness, 1920);
    click(&mut harness, pos, egui::Modifiers::NONE);
    let seconds = anchor_seconds(&harness);
    assert!((seconds[2] - heard).abs() < 5e-3, "{seconds:?}");
}

#[test]
fn an_anchor_is_not_added_within_half_a_grid_step_of_another_and_says_why() {
    let mut harness = syncing();
    let near = in_row(&harness, 1150);
    click(&mut harness, near, egui::Modifiers::NONE);
    assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
    assert!(
        harness
            .query_by_label_contains("Too close to the anchor")
            .is_some()
    );
}

#[test]
fn dragging_an_anchor_moves_it_on_the_grid_keeping_its_moment() {
    let mut harness = syncing();
    let (from, to) = (in_row(&harness, 960), in_row(&harness, 1900));
    drag(&mut harness, from, to, egui::Modifiers::NONE);
    assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(1920)]);
    assert!((seconds_at(&harness, 1920) - 0.5).abs() < 1e-6);
}

#[test]
fn ctrl_dragging_an_anchor_moves_it_in_the_audio_keeping_its_beat() {
    let mut harness = syncing();
    let heard = seconds_at(&harness, 1200);
    let (from, to) = (in_row(&harness, 960), in_row(&harness, 1200));
    drag(&mut harness, from, to, egui::Modifiers::COMMAND);
    assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
    assert!((seconds_at(&harness, 960) - heard).abs() < 5e-3);
}

#[test]
fn an_anchor_dragged_into_its_neighbour_stops_short_of_it_in_one_step() {
    let mut harness = syncing();
    // Into the tenth of the bar before, shown ahead of the first downbeat.
    let (from, to) = (
        in_row(&harness, 960),
        in_row(&harness, 0) - egui::vec2(30.0, 0.0),
    );
    drag(&mut harness, from, to, egui::Modifiers::COMMAND);
    let seconds = anchor_seconds(&harness);
    assert!(seconds[1] > seconds[0] && seconds[1] < 0.01, "{seconds:?}");
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert!((anchor_seconds(&harness)[1] - 0.5).abs() < 1e-9);
}

#[test]
fn dragging_across_a_row_selects_the_anchors_inside_and_delete_removes_them() {
    let mut harness = syncing();
    let (from, to) = (in_row(&harness, 200), in_row(&harness, 1500));
    drag(&mut harness, from, to, egui::Modifiers::NONE);
    assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
    harness.key_press(egui::Key::Delete);
    harness.run();
    assert_eq!(event_ticks(&harness), [Tick::new(0)]);
}

#[test]
fn a_double_click_on_an_anchor_opens_its_moment_to_type_not_a_removal() {
    let mut harness = syncing();
    harness
        .ctx
        .options_mut(|options| options.input_options.max_double_click_delay = 2.0);
    let at = in_row(&harness, 960);
    harness.hover_at(at);
    for _ in 0..2 {
        press(&harness, at, true, egui::Modifiers::NONE);
        press(&harness, at, false, egui::Modifiers::NONE);
        harness.step();
    }
    harness.run();
    harness.run();
    assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
    let field = harness.get_by_role_and_label(Role::TextInput, "t");
    assert_eq!(field.value().as_deref(), Some("0.5000"));
    field.type_text("0.6");
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_role_and_label(Role::TextInput, "t")
        .type_text("0.6");
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!((anchor_seconds(&harness)[1] - 0.6).abs() < 1e-6);
}

#[test]
fn ctrl_wheel_nudges_the_focused_anchor_by_a_millisecond() {
    let mut harness = syncing();
    let at = in_row(&harness, 960);
    click(&mut harness, at, egui::Modifiers::NONE);
    harness.hover_at(in_row(&harness, 1000));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 1.0),
        modifiers: egui::Modifiers::COMMAND,
        phase: egui::TouchPhase::Move,
    });
    harness.run();
    assert!((seconds_at(&harness, 960) - 0.501).abs() < 1e-6);
}

#[test]
fn twenty_arrow_presses_nudge_twenty_milliseconds_as_one_step() {
    let mut harness = syncing();
    let at = in_row(&harness, 960);
    click(&mut harness, at, egui::Modifiers::NONE);
    for _ in 0..20 {
        harness.key_press(egui::Key::ArrowRight);
    }
    harness.run();
    assert!((anchor_seconds(&harness)[1] - 0.52).abs() < 1e-6);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert!((anchor_seconds(&harness)[1] - 0.5).abs() < 1e-9);
}

#[test]
fn number_keys_set_the_grid_while_the_timing_window_has_the_keyboard() {
    let mut harness = syncing();
    let at = in_row(&harness, 960);
    click(&mut harness, at, egui::Modifiers::NONE);
    harness.key_press(egui::Key::Num8);
    harness.run();
    assert_eq!(harness.state().sync.grid().resolution.get(), 16);
    assert!(lanes_at(&harness, 0).is_empty());
    let grid = harness.get_by_role_and_label(Role::ComboBox, "Grid");
    assert_eq!(grid.value().as_deref(), Some("1/16"));
}

#[test]
fn the_end_of_a_row_in_the_timing_window_is_clear_of_its_scroll_bar() {
    // The demo runs to more bars than the window shows, so its rows scroll.
    let mut harness = editable_demo();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
    harness.run();
    harness
        .state_mut()
        .sync
        .set_layout(charter_timing::rows::Layout::Bars);
    harness.run();
    let heard = seconds_at(&harness, 3830);
    let at = in_row(&harness, 3830);
    click(&mut harness, at, egui::Modifiers::NONE);
    assert!(
        event_ticks(&harness).contains(&Tick::new(3840)),
        "{:?}",
        event_ticks(&harness)
    );
    assert!((seconds_at(&harness, 3840) - heard).abs() < 5e-3);
}

/// Reads `offset,bpm` as a timing, standing in for a game's timing format.
#[derive(Debug)]
struct Importer;

#[expect(
    clippy::missing_trait_methods,
    reason = "a test plugin: the trait's defaults for every optional method"
)]
impl RulesetPlugin for Importer {
    fn id(&self) -> &'static str {
        "importer"
    }

    fn check(&self, _: &ValidDocument) -> Result<(), Box<dyn Error + Send + Sync>> {
        Ok(())
    }

    fn timing_format(&self) -> Option<&str> {
        Some("test timing")
    }

    fn import_timing(
        &self,
        text: &str,
        _: &ValidDocument,
    ) -> Result<charter_core::ImportedTiming, Box<dyn Error + Send + Sync>> {
        let (offset, bpm) = text.trim().split_once(',').ok_or("no comma")?;
        let timing = serde_json::from_value(serde_json::json!({
            "anchors": [ { "tick": 0, "seconds": offset.parse::<f64>()? } ],
            "tempo": bpm.parse::<f64>()?,
            "events": []
        }))?;
        Ok(charter_core::ImportedTiming {
            timing,
            left_out: vec!["kiai time".to_owned()],
        })
    }
}

/// Reads the same text differently depending on the destination game's ruleset.
#[derive(Debug)]
struct ContextImporter;

#[expect(
    clippy::missing_trait_methods,
    reason = "a test plugin: the trait's defaults for every optional method"
)]
impl RulesetPlugin for ContextImporter {
    fn id(&self) -> &'static str {
        "context-importer"
    }

    fn check(&self, _: &ValidDocument) -> Result<(), Box<dyn Error + Send + Sync>> {
        Ok(())
    }

    fn timing_format(&self) -> Option<&str> {
        Some("context timing")
    }

    fn import_timing(
        &self,
        text: &str,
        into: &ValidDocument,
    ) -> Result<charter_core::ImportedTiming, Box<dyn Error + Send + Sync>> {
        let bpm = if into.document().ruleset.id.as_str() == "ttr" {
            "208.1"
        } else {
            "150"
        };
        Importer.import_timing(&format!("{text},{bpm}"), into)
    }
}

const IMPORTING: &[&dyn RulesetPlugin] = &[&AnySm, &Importer];
const CONTEXT_IMPORTING: &[&dyn RulesetPlugin] = &[
    &charter_stepmania::Plugin,
    &charter_ttr::Plugin,
    &ContextImporter,
];

#[expect(
    clippy::non_ascii_literal,
    reason = "the import test opens the Import timing menu label with its ellipsis"
)]
fn importing() -> Harness<'static, Window> {
    let mut app = Window::new(IMPORTING);
    app.open_demo();
    let mut harness = window(app);
    // The menu, which comes before the song panel's section of the same name.
    harness.query_all_by_label("Timing").next().unwrap().click();
    harness.run();
    harness.get_by_label("Import timing…").click();
    harness.run();
    harness
}

#[test]
#[expect(
    clippy::non_ascii_literal,
    reason = "the test reopens the Import timing menu label with its ellipsis"
)]
fn reopening_import_after_opening_another_game_previews_for_that_song() {
    let draft = charter_core::NewSong {
        metadata: charter_editor::demo::song().document().metadata.clone(),
        layout: 0,
        charts: vec![charter_core::NewChart::named("Easy")],
    };
    let first = charter_stepmania::Plugin.new_song(&draft).unwrap();
    let second = charter_ttr::Plugin.new_song(&draft).unwrap();
    let mut app = Window::new(CONTEXT_IMPORTING);
    app.open(
        Path::new("/songs/stepmania.json"),
        &first.to_json().unwrap(),
    );
    let mut harness = window(app);

    harness.state_mut().timing_file_read("0.895");
    harness.run();
    assert!(
        harness
            .query_by_label_contains("150 BPM at the start")
            .is_some()
    );
    harness.get_by_label("Close window").click();
    harness.run();
    harness
        .state_mut()
        .open(Path::new("/songs/ttr.json"), &second.to_json().unwrap());
    harness.run();
    harness.query_all_by_label("Timing").next().unwrap().click();
    harness.run();
    harness.get_by_label("Import timing…").click();
    harness.run();

    assert!(
        harness
            .query_by_label_contains("208.1 BPM at the start")
            .is_some()
    );
    harness.get_by_label("Replace the timing").click();
    harness.run();
    assert_eq!(bpm_at_cursor(&harness), Some((0, "208.1".to_owned())));
}

#[test]
fn imported_timing_shows_what_it_holds_and_replaces_the_songs_in_one_step() {
    let mut harness = importing();
    assert!(harness.query_by_label_contains("test timing").is_some());
    harness.state_mut().timing_file_read("0.895,208.1");
    harness.run();
    assert!(harness.query_by_label_contains("208.1 BPM").is_some());
    assert!(harness.query_by_label_contains("895 ms").is_some());
    assert!(harness.query_by_label_contains("kiai time").is_some());
    harness.get_by_label("Replace the timing").click();
    harness.run();
    assert_eq!(bpm_at_cursor(&harness), Some((0, "208.1".to_owned())));
    assert_eq!(offset(&harness), serde_json::json!(0.895));
    assert!(harness.query_by_label("Replace the timing").is_none());
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
}

#[test]
fn timing_that_does_not_read_says_why_and_imports_nothing() {
    let mut harness = importing();
    harness.state_mut().timing_file_read("nonsense");
    harness.run();
    assert!(harness.query_by_label_contains("no comma").is_some());
    harness.get_by_label("Replace the timing").click();
    harness.run();
    assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
}

#[test]
#[expect(
    clippy::non_ascii_literal,
    reason = "the test clicks the Read a file button with its ellipsis"
)]
fn the_import_window_reads_timing_from_a_file_the_host_opens() {
    let mut harness = importing();
    harness.get_by_label("Read a file…").click();
    harness.run();
    assert_eq!(
        harness.state_mut().take_requests(),
        [Request::ChooseTimingFile]
    );
}

#[test]
fn the_import_window_can_be_cancelled_without_importing() {
    let mut harness = importing();
    harness.state_mut().timing_file_read("0.895,208.1");
    harness.run();

    harness.get_by_label("Cancel").click();
    harness.run();

    assert!(harness.query_by_label("Replace the timing").is_none());
    assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
}

fn moments(harness: &Harness<'_, Window>) -> Vec<f64> {
    let song = harness.state().song.as_ref().unwrap();
    song.anchors().iter().map(|pin| pin.seconds).collect()
}

#[test]
fn dragging_a_tempo_point_moves_it_to_the_moment_under_the_pointer() {
    let mut harness = timed_song();
    let before = moments(&harness);
    let heard = seconds_at(&harness, 1920);
    let x = field(&harness).columns.timing_x();
    let (from, to) = (on_line(&harness, x, 960), on_line(&harness, x, 1920));
    drag(&mut harness, from, to, egui::Modifiers::NONE);
    assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
    let after = moments(&harness);
    assert!((after[1] - heard).abs() < 0.01, "{after:?} {heard}");
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(moments(&harness), before);
}

#[test]
fn a_bpm_change_and_a_meter_at_one_tick_drag_as_one_timing_point() {
    let mut harness = timed_song();
    let meter = serde_json::json!({ "tick": 960, "kind": { "time_signature": [3, 4] } });
    let mut document = harness
        .state()
        .song
        .as_ref()
        .unwrap()
        .document()
        .document()
        .clone();
    document
        .timing
        .events
        .push(serde_json::from_value(meter).unwrap());
    let bytes = serde_json::to_vec(&document).unwrap();
    harness.state_mut().open(Path::new("timed.json"), &bytes);
    harness.run();
    let x = field(&harness).columns.timing_x();
    let (from, to) = (on_line(&harness, x, 960), on_line(&harness, x, 1920));
    drag(&mut harness, from, to, egui::Modifiers::NONE);
    assert_eq!(
        event_ticks(&harness),
        [Tick::new(0), Tick::new(960), Tick::new(960)]
    );
}

#[test]
fn dragging_the_first_tempo_point_moves_the_whole_grid() {
    let mut harness = timed_song();
    let before = moments(&harness);
    let x = field(&harness).columns.timing_x();
    let (from, to) = (on_line(&harness, x, 0), on_line(&harness, x, 480));
    let later = seconds_at(&harness, 480) - seconds_at(&harness, 0);
    drag(&mut harness, from, to, egui::Modifiers::NONE);
    assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
    let after = moments(&harness);
    for (was, is) in before.iter().zip(&after) {
        assert!((is - was - later).abs() < 0.01, "{before:?} {after:?}");
    }
}

#[test]
fn dragging_a_marker_in_the_ruler_moves_it() {
    let mut harness = timed_song();
    let x = field(&harness).columns.ruler_x();
    let (from, to) = (on_line(&harness, x, 480), on_line(&harness, x, 1440));
    drag(&mut harness, from, to, egui::Modifiers::NONE);
    let song = harness.state().song.as_ref().unwrap();
    assert_eq!(song.document().document().markers[0].tick, Tick::new(1440));
}

#[test]
fn dragging_the_cursor_handle_scrubs_through_the_song() {
    let mut harness = timed_song();
    let field = field(&harness);
    let from = egui::pos2(field.columns.handle_x(), field.view.now_y);
    let beat = field.view.y_of(0.5) - field.view.y_of(0.0);
    drag(
        &mut harness,
        from,
        from + egui::vec2(0.0, beat),
        egui::Modifiers::NONE,
    );
    assert!(cursor_shows(&harness, "0:1:0"));
}

#[test]
fn the_handles_of_a_read_only_song_stay_put() {
    let mut harness = timed_song();
    harness.state_mut().plugins = &[];
    let song = harness.state().song.as_ref().unwrap().document().clone();
    harness.state_mut().song = Some(Song::new(song, &[]));
    harness.run();
    let x = field(&harness).columns.timing_x();
    let (from, to) = (on_line(&harness, x, 960), on_line(&harness, x, 1920));
    drag(&mut harness, from, to, egui::Modifiers::NONE);
    assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
}

fn peaks() -> Arc<charter_audio::Peaks> {
    let frames = vec![[0.25, -0.25]; 4000];
    Arc::new(charter_audio::Peaks::of(
        &charter_audio::Track::from_frames(1000, frames),
    ))
}

#[test]
fn the_waveform_of_the_songs_audio_shows_and_that_of_other_audio_does_not() {
    let mut harness = four_taps_at("/songs/four.json");
    assert!(harness.state().strip().is_none());
    harness
        .state_mut()
        .waveform_ready(Path::new("/songs/other.ogg"), peaks());
    harness.run();
    assert!(harness.state().strip().is_none());
    harness
        .state_mut()
        .waveform_ready(Path::new("/songs/a.ogg"), peaks());
    harness.run();
    assert!(harness.state().strip().is_some());
    let lanes = field(&harness)
        .columns
        .span(LaneIndex::new(0), LaneCount::ONE);
    let tap = on_line(&harness, lanes.center(), 0);
    click(&mut harness, tap, egui::Modifiers::NONE);
    assert!(harness.query_by_label("1 note selected").is_some());
}

#[test]
fn the_strip_shows_the_frequencies_once_ready_when_the_settings_ask() {
    let mut app = Window::new(PLUGINS).with_settings(Settings {
        frequencies: true,
        ..Settings::default()
    });
    app.open(Path::new("/songs/four.json"), &four_taps());
    let mut harness = window(app);
    harness
        .state_mut()
        .waveform_ready(Path::new("/songs/a.ogg"), peaks());
    harness.run();
    assert!(matches!(
        harness.state().strip(),
        Some(notefield::Strip::Waveform(_))
    ));
    let track = charter_audio::Track::from_frames(1000, vec![[0.25, -0.25]; 4000]);
    harness
        .state_mut()
        .spectrum_ready(Path::new("/songs/a.ogg"), Arc::new(Spectrum::of(&track)));
    harness.run();
    assert!(matches!(
        harness.state().strip(),
        Some(notefield::Strip::Frequencies(_))
    ));
}

#[test]
fn the_waveform_can_be_turned_off_in_the_settings() {
    let mut app = Window::new(PLUGINS).with_settings(Settings {
        waveform: false,
        ..Settings::default()
    });
    app.open(Path::new("/songs/four.json"), &four_taps());
    let mut harness = window(app);
    harness
        .state_mut()
        .waveform_ready(Path::new("/songs/a.ogg"), peaks());
    harness.run();
    assert!(harness.state().strip().is_none());
}

#[test]
fn turning_auto_jump_off_keeps_the_cursor_where_notes_go() {
    let mut harness = editable_demo();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    harness.run();
    harness
        .get_by_role_and_label(Role::CheckBox, "Move on a snap after placing a note")
        .click();
    harness.run();
    assert!(!harness.state().settings().auto_jump);
    harness.get_by_label("Close settings").click();
    harness.run();
    harness.key_press(egui::Key::Num1);
    harness.run();
    assert_eq!(lanes_at(&harness, 0), [0, 3]);
    assert!(cursor_shows(&harness, "0:0:0"));
}

#[test]
fn a_song_opens_on_the_snap_and_zoom_the_settings_choose() {
    let settings = Settings {
        snap: NonZeroU16::new(32).unwrap(),
        zoom: 200.0,
        ..Settings::default()
    };
    let mut app = Window::new(PLUGINS).with_settings(settings);
    app.open_demo();
    let harness = window(app);
    assert!(harness.query_by_label("Snap 32nd").is_some());
    assert!(harness.query_by_label("Zoom 200%").is_some());
}

#[test]
fn the_interface_size_setting_scales_the_whole_window() {
    let settings = Settings {
        scale: 1.5,
        ..Settings::default()
    };
    let harness = window(Window::new(PLUGINS).with_settings(settings));
    assert!((harness.ctx.zoom_factor() - 1.5).abs() < f32::EPSILON);
}

#[test]
fn a_ctrl_drag_draws_its_hold_while_a_message_comes_and_goes() {
    let mut harness = editable_four_taps();
    harness
        .state_mut()
        .refuse(Path::new("other.json"), &"it is not a song");
    harness.run();
    let (from, to) = (spot(&harness, 3, 240, 0.0), spot(&harness, 3, 720, -3.0));
    drag(&mut harness, from, to, egui::Modifiers::COMMAND);
    assert_eq!(note_at(&harness, 240, 3), Some(hold(480)));
}

#[test]
fn a_hold_reaches_where_the_drag_lets_go() {
    let mut harness = editable_four_taps();
    let (from, near, far) = (
        spot(&harness, 3, 240, 0.0),
        spot(&harness, 3, 480, 0.0),
        spot(&harness, 3, 960, 0.0),
    );
    harness.hover_at(from);
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::COMMAND));
    press(&harness, from, true, egui::Modifiers::COMMAND);
    harness.hover_at(from.lerp(near, 0.5));
    harness.hover_at(near);
    press(&harness, far, false, egui::Modifiers::COMMAND);
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    harness.run();
    assert_eq!(note_at(&harness, 240, 3), Some(hold(720)));
}

#[test]
fn escape_during_a_box_selection_cancels_it() {
    let mut harness = editable_four_taps();
    let (from, to) = (spot(&harness, 1, 180, 0.0), spot(&harness, 2, 540, 0.0));
    harness.hover_at(from);
    press(&harness, from, true, egui::Modifiers::NONE);
    harness.hover_at(from.lerp(to, 0.5));
    harness.hover_at(to);
    harness.key_press(egui::Key::Escape);
    press(&harness, to, false, egui::Modifiers::NONE);
    harness.run();
    assert!(nothing_selected(&harness));
}

#[test]
fn a_save_holds_every_edit_made_in_its_frame() {
    let mut harness = editable_four_taps();
    let key = |key, pressed, modifiers| egui::Event::Key {
        key,
        pressed,
        repeat: false,
        modifiers,
        physical_key: None,
    };
    let command = egui::Modifiers::COMMAND;
    let events = &mut harness.input_mut().events;
    events.push(key(egui::Key::S, true, command));
    events.push(key(egui::Key::S, false, command));
    events.push(key(egui::Key::Num4, true, egui::Modifiers::NONE));
    events.push(key(egui::Key::Num4, false, egui::Modifiers::NONE));
    harness.run();
    let Request::Write { path, bytes } = request(&mut harness) else {
        panic!("expected a write");
    };
    assert!(holds_tap(&bytes, 0, 3));
    harness.state_mut().saved(Saved::To(path));
    assert_eq!(harness.state().title(), "four.json - charter");
}

#[test]
fn a_lane_key_held_across_a_chart_switch_leaves_the_other_chart_alone() {
    let mut harness = editable_demo();
    harness.key_down(egui::Key::Num1);
    harness.run();
    harness.get_by_label("Hard").click();
    harness.run();
    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    assert_eq!(note_at(&harness, 0, 0), Some(tap()));
}

/// Runs the window with its clock at `seconds`.
fn at_time(harness: &mut Harness<'_, Window>, seconds: f64) {
    harness.input_mut().time = Some(seconds);
    harness.run();
}

/// The four taps edited at one second in, and left long enough to be autosaved.
fn autosaved_four_taps() -> (Harness<'static, Window>, Vec<u8>) {
    let mut harness = editable_four_taps();
    at_time(&mut harness, 0.0);
    harness.key_press(egui::Key::Num4);
    at_time(&mut harness, 1.0);
    at_time(&mut harness, 31.5);
    let Request::Autosave { bytes } = request(&mut harness) else {
        panic!("expected an autosave");
    };
    (harness, bytes)
}

fn four_taps_with(settings: Settings) -> Harness<'static, Window> {
    let mut app = Window::new(PLUGINS).with_settings(settings);
    app.open(Path::new("four.json"), &four_taps());
    window(app)
}

fn snapshots(harness: &mut Harness<'_, Window>) -> Vec<(String, Vec<PathBuf>)> {
    let requests = harness.state_mut().take_requests();
    requests
        .into_iter()
        .filter_map(|request| {
            if let Request::Snapshot { name, folders, .. } = request {
                Some((name, folders))
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn a_changed_song_is_copied_each_wait_into_both_folders_and_not_again_unchanged() {
    let mut app = Window::new(PLUGINS)
        .with_settings(Settings {
            snapshots_in_folder: true,
            snapshots_beside_song: true,
            snapshot_minutes: 2,
            ..Settings::default()
        })
        .with_data_folder(PathBuf::from("/data"));
    app.open(Path::new("/songs/Four/Four.json"), &four_taps());
    let mut harness = window(app);
    at_time(&mut harness, 0.0);
    at_time(&mut harness, 500.0);
    assert_eq!(
        snapshots(&mut harness),
        [],
        "an unchanged song is not copied"
    );
    harness.key_press(egui::Key::Num4);
    at_time(&mut harness, 501.0);
    at_time(&mut harness, 600.0);
    assert_eq!(snapshots(&mut harness), [], "two minutes have not passed");
    at_time(&mut harness, 622.0);
    let folders = vec![
        PathBuf::from("/data/snapshots/Four"),
        PathBuf::from("/songs/Four/autosaves"),
    ];
    assert_eq!(snapshots(&mut harness), [("Four".to_owned(), folders)]);
    at_time(&mut harness, 900.0);
    assert_eq!(snapshots(&mut harness), [], "nothing changed since");
}

#[test]
fn a_chosen_snapshot_folder_is_used_and_an_unsaved_song_has_no_folder_beside_it() {
    let mut harness = created();
    harness
        .state_mut()
        .use_snapshot_folder(Path::new("/backups"));
    harness.state_mut().settings.snapshots_beside_song = true;
    at_time(&mut harness, 100.0);
    harness.key_press(egui::Key::Num1);
    at_time(&mut harness, 101.0);
    at_time(&mut harness, 500.0);
    let taken = snapshots(&mut harness);
    let [(name, folders)] = taken.as_slice() else {
        panic!("one snapshot, got {taken:?}");
    };
    assert_eq!(folders, &[PathBuf::from("/backups").join(name)]);
}

#[test]
fn an_unavailable_snapshot_folder_does_not_consume_the_songs_revision() {
    let mut harness = four_taps_with(Settings {
        snapshots_in_folder: true,
        snapshots_beside_song: false,
        snapshot_minutes: 1,
        ..Settings::default()
    });
    harness.key_press(egui::Key::Num4);
    at_time(&mut harness, 1.0);
    at_time(&mut harness, 70.0);
    assert!(snapshots(&mut harness).is_empty());

    harness.state_mut().autosave.data_folder = Some(PathBuf::from("/data"));
    at_time(&mut harness, 71.0);
    at_time(&mut harness, 133.0);
    assert_eq!(
        snapshots(&mut harness),
        [(
            "four".to_owned(),
            vec![PathBuf::from("/data/snapshots/four")]
        )]
    );
}

#[test]
fn a_failed_snapshot_is_retried_after_the_wait_without_another_edit() {
    let mut harness = four_taps_with(Settings {
        snapshots_in_folder: false,
        snapshots_beside_song: true,
        snapshot_minutes: 1,
        ..Settings::default()
    });
    harness.key_press(egui::Key::Num4);
    at_time(&mut harness, 1.0);
    at_time(&mut harness, 70.0);
    assert_eq!(snapshots(&mut harness).len(), 1);

    harness.state_mut().snapshot_failed("disk full");
    at_time(&mut harness, 71.0);
    at_time(&mut harness, 133.0);
    assert_eq!(snapshots(&mut harness).len(), 1);
}

#[test]
fn the_empty_window_lists_recent_songs_to_open() {
    let app = Window::new(PLUGINS).with_settings(Settings {
        recent: vec![PathBuf::from("/songs/Starlight/Starlight.json")],
        ..Settings::default()
    });
    let mut harness = window(app);
    harness.get_by_label("Starlight.json").click();
    harness.run();
    assert_eq!(
        harness.state_mut().take_requests(),
        [Request::OpenPath {
            path: PathBuf::from("/songs/Starlight/Starlight.json")
        }]
    );
}

#[test]
fn songs_opened_and_saved_are_remembered_latest_first_and_open_again() {
    let mut harness = four_taps_at("/songs/a.json");
    harness
        .state_mut()
        .open(Path::new("/songs/b.json"), &four_taps());
    harness.run();
    harness
        .state_mut()
        .saved(Saved::To(PathBuf::from("/songs/c.json")));
    harness.run();
    let recent = |view: &Harness<'_, Window>| view.state().settings().recent.clone();
    let paths = ["/songs/c.json", "/songs/b.json", "/songs/a.json"].map(PathBuf::from);
    assert_eq!(recent(&harness), paths);
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label_contains("Open recent").click();
    harness.run();
    harness.get_by_label_contains("b.json").click();
    harness.run();
    assert_eq!(
        harness.state_mut().take_requests(),
        [Request::OpenPath {
            path: PathBuf::from("/songs/b.json")
        }]
    );
    harness
        .state_mut()
        .refuse(Path::new("/songs/a.json"), &"it is not there");
    assert_eq!(recent(&harness).len(), 2);
    harness.state_mut().apply(Action::ClearRecent);
    assert_eq!(recent(&harness), Vec::<PathBuf>::new());
}

#[test]
fn unsaved_changes_wait_as_long_as_the_settings_say_before_they_are_autosaved() {
    let mut harness = four_taps_with(Settings {
        autosave_seconds: 10,
        ..Settings::default()
    });
    at_time(&mut harness, 0.0);
    harness.key_press(egui::Key::Num4);
    at_time(&mut harness, 1.0);
    at_time(&mut harness, 10.5);
    assert_eq!(harness.state_mut().take_requests(), []);
    at_time(&mut harness, 11.5);
    assert!(matches!(request(&mut harness), Request::Autosave { .. }));
}

#[test]
fn with_autosave_off_nothing_is_autosaved() {
    let mut harness = four_taps_with(Settings {
        autosave: false,
        ..Settings::default()
    });
    at_time(&mut harness, 0.0);
    harness.key_press(egui::Key::Num4);
    at_time(&mut harness, 1.0);
    at_time(&mut harness, 120.0);
    assert_eq!(harness.state_mut().take_requests(), []);
}

#[test]
fn the_settings_window_turns_autosave_off() {
    let mut harness = editable_demo();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    harness.run();
    settings_page(&mut harness, "Autosave");
    harness
        .get_by_role_and_label(Role::CheckBox, "Autosave unsaved changes")
        .click();
    harness.run();
    assert!(!harness.state().settings().autosave);
}

#[test]
fn unsaved_changes_are_autosaved_half_a_minute_after_they_are_made() {
    let mut harness = editable_four_taps();
    at_time(&mut harness, 0.0);
    harness.key_press(egui::Key::Num4);
    at_time(&mut harness, 1.0);
    at_time(&mut harness, 20.0);
    assert_eq!(harness.state_mut().take_requests(), []);
    at_time(&mut harness, 31.5);
    let Request::Autosave { bytes } = request(&mut harness) else {
        panic!("expected an autosave");
    };
    let kept = recovery::read(&bytes, PLUGINS).unwrap();
    assert_eq!(kept.path.as_deref(), Some(Path::new("four.json")));
    assert!(holds_tap(&kept.song.document().to_json().unwrap(), 0, 3));
    at_time(&mut harness, 90.0);
    assert_eq!(harness.state_mut().take_requests(), []);
}

#[test]
fn saving_removes_the_autosave() {
    let (mut harness, _) = autosaved_four_taps();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    harness.run();
    assert!(matches!(request(&mut harness), Request::Write { .. }));
    harness
        .state_mut()
        .saved(Saved::To(PathBuf::from("four.json")));
    harness.run();
    assert_eq!(request(&mut harness), Request::DropAutosave);
}

#[test]
fn undoing_back_to_the_saved_song_removes_the_autosave() {
    let (mut harness, _) = autosaved_four_taps();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(request(&mut harness), Request::DropAutosave);
}

#[test]
fn closing_without_saving_removes_the_autosave() {
    let (mut harness, _) = autosaved_four_taps();
    close(&mut harness);
    harness.run();
    harness.get_by_label("Don't save").click();
    harness.run();
    let requests = harness.state_mut().take_requests();
    assert_eq!(requests, [Request::DropAutosave]);
}

fn restore_prompt(harness: &Harness<'_, Window>) -> bool {
    harness
        .query_by_label("Restore unsaved changes to four.json?")
        .is_some()
}

#[test]
fn an_autosave_restored_remembers_the_audio_dropped_on_the_song() {
    let mut harness = created();
    drop_file(&mut harness, "/music/track.ogg");
    at_time(&mut harness, 100.0);
    at_time(&mut harness, 131.0);
    let Request::Autosave { bytes } = request(&mut harness) else {
        panic!("expected an autosave");
    };
    let mut restored = window(Window::new(GAMES));
    restored.state_mut().recover(&bytes);
    restored.run();
    restored.get_by_label("Restore").click();
    restored.run();
    assert_eq!(
        restored.state().audio_file(),
        Some(PathBuf::from("/music/track.ogg"))
    );
    let path = PathBuf::from("/songs/mine.json");
    assert_eq!(
        restored.state().audio_copy_for(&path),
        Some((
            PathBuf::from("/music/track.ogg"),
            PathBuf::from("/songs/track.ogg"),
        ))
    );
    restored.state_mut().saved(Saved::To(path));
    restored.run();
    let copies: Vec<_> = restored
        .state_mut()
        .take_requests()
        .into_iter()
        .filter(|request| matches!(request, Request::CopyAudio { .. }))
        .collect();
    assert_eq!(copies, []);
}

#[test]
fn an_autosave_restored_brings_the_changes_back_unsaved_for_their_file() {
    let (_, bytes) = autosaved_four_taps();
    let mut harness = window(Window::new(PLUGINS));
    harness.state_mut().recover(&bytes);
    harness.run();
    assert!(restore_prompt(&harness));
    harness.get_by_label("Restore").click();
    harness.run();
    assert_eq!(harness.state().title(), "*four.json - charter");
    assert_eq!(note_at(&harness, 0, 3), Some(tap()));
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    harness.run();
    let Request::Write { path, .. } = request(&mut harness) else {
        panic!("expected a write");
    };
    assert_eq!(path, Path::new("four.json"));
    harness.state_mut().saved(Saved::To(path));
    harness.run();
    assert_eq!(request(&mut harness), Request::DropAutosave);
}

#[test]
fn an_autosave_keeps_artwork_not_yet_beside_the_song_and_restores_it() {
    let mut harness = created();
    harness
        .state_mut()
        .use_artwork(Path::new("/pictures/art.png"), Some(picture(b"png")));
    at_time(&mut harness, 100.0);
    at_time(&mut harness, 131.0);
    let requests = harness.state_mut().take_requests();
    let Some(Request::Autosave { bytes }) = requests
        .into_iter()
        .find(|request| matches!(request, Request::Autosave { .. }))
    else {
        panic!("expected an autosave");
    };
    let mut restored = window(Window::new(GAMES));
    restored.state_mut().recover(&bytes);
    restored.run();
    restored.get_by_label("Restore").click();
    restored.run();
    let restored_requests = restored.state_mut().take_requests();
    assert!(restored_requests.contains(&Request::DecodeArtwork {
        bytes: b"png".to_vec()
    }));
    restored.state_mut().artwork_restored(Some(picture(b"png")));
    assert!(
        !restored
            .state()
            .may_replace(Path::new("/songs/New/art.png"))
    );
    let path = Path::new("/songs/New/New.json");
    assert_eq!(
        restored.state().artwork_write_for(path),
        Some((PathBuf::from("/songs/New/art.png"), b"png".to_vec()))
    );
}

#[test]
fn chosen_artwork_survives_being_undone_and_redone() {
    let mut harness = four_taps_at("/songs/Four/four.json");
    harness
        .state_mut()
        .use_artwork(Path::new("/pictures/art.png"), Some(picture(b"png")));
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(artwork_name(&harness), None);
    let redo = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);
    harness.key_press_modifiers(redo, egui::Key::Z);
    harness.run();
    assert_eq!(artwork_name(&harness).as_deref(), Some("art.png"));
    assert_eq!(
        harness
            .state()
            .artwork_write_for(Path::new("/songs/Four/four.json")),
        Some((PathBuf::from("/songs/Four/art.png"), b"png".to_vec()))
    );
}

#[test]
fn undoing_a_second_chosen_picture_brings_the_first_back() {
    let mut harness = four_taps_at("/songs/Four/four.json");
    let state = harness.state_mut();
    state.use_artwork(Path::new("/pictures/a.png"), Some(picture(b"a")));
    harness.run();
    let updated_state = harness.state_mut();
    updated_state.use_artwork(Path::new("/pictures/b.png"), Some(picture(b"b")));
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(artwork_name(&harness).as_deref(), Some("a.png"));
    assert_eq!(
        harness
            .state()
            .artwork_write_for(Path::new("/songs/Four/four.json")),
        Some((PathBuf::from("/songs/Four/a.png"), b"a".to_vec()))
    );
}

#[test]
fn undo_brings_back_each_picture_of_one_name_at_its_own_step() {
    let mut harness = four_taps_at("/songs/Four/four.json");
    for (file, bytes) in [
        ("/p/cover.png", b"A"),
        ("/p/other.png", b"B"),
        ("/p/cover.png", b"C"),
    ] {
        harness
            .state_mut()
            .use_artwork(Path::new(file), Some(picture(bytes)));
        harness.run();
    }
    let path = Path::new("/songs/Four/four.json");
    let written =
        |view: &Harness<'_, Window>| view.state().artwork_write_for(path).map(|(_, bytes)| bytes);
    let undo = |view: &mut Harness<'_, Window>| {
        view.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        view.run();
    };
    undo(&mut harness);
    assert_eq!(written(&harness), Some(b"B".to_vec()));
    undo(&mut harness);
    assert_eq!(written(&harness), Some(b"A".to_vec()));
    let redo = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);
    harness.key_press_modifiers(redo, egui::Key::Z);
    harness.key_press_modifiers(redo, egui::Key::Z);
    harness.run();
    assert_eq!(written(&harness), Some(b"C".to_vec()));
}

#[test]
fn a_save_in_the_frame_of_an_undo_writes_the_artwork_the_song_names() {
    let mut harness = four_taps_at("/songs/Four/four.json");
    for (file, bytes) in [("/p/a.png", b"A"), ("/p/b.png", b"B")] {
        harness
            .state_mut()
            .use_artwork(Path::new(file), Some(picture(bytes)));
        harness.run();
    }
    harness.state_mut().take_requests();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    harness.run();
    assert_eq!(artwork_name(&harness).as_deref(), Some("a.png"));
    let path = Path::new("/songs/Four/four.json");
    assert_eq!(
        harness.state().artwork_write_for(path),
        Some((PathBuf::from("/songs/Four/a.png"), b"A".to_vec()))
    );
}

#[test]
fn redo_brings_back_a_same_name_picture_chosen_after_another_edit() {
    let mut harness = four_taps_at("/songs/Four/four.json");
    harness
        .state_mut()
        .use_artwork(Path::new("/p/cover.png"), Some(picture(b"A")));
    harness.run();
    select_note(&mut harness, 3, 720);
    harness.key_press(egui::Key::Delete);
    harness.run();
    harness
        .state_mut()
        .use_artwork(Path::new("/q/cover.png"), Some(picture(b"B")));
    harness.run();
    let path = Path::new("/songs/Four/four.json");
    let written =
        |view: &Harness<'_, Window>| view.state().artwork_write_for(path).map(|(_, bytes)| bytes);
    for _ in 0..2 {
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
    }
    let redo = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);
    harness.key_press_modifiers(redo, egui::Key::Z);
    harness.run();
    assert_eq!(written(&harness), Some(b"A".to_vec()));
    harness.key_press_modifiers(redo, egui::Key::Z);
    harness.run();
    assert_eq!(written(&harness), Some(b"B".to_vec()));
}

#[test]
fn another_picture_under_the_same_name_is_autosaved_again() {
    let mut harness = created();
    let chosen = Path::new("/pictures/art.png");
    harness
        .state_mut()
        .use_artwork(chosen, Some(picture(b"one")));
    at_time(&mut harness, 100.0);
    at_time(&mut harness, 131.0);
    let autosaves = |view: &mut Harness<'_, Window>| {
        let requests = view.state_mut().take_requests();
        requests
            .iter()
            .filter(|request| matches!(request, Request::Autosave { .. }))
            .count()
    };
    assert_eq!(autosaves(&mut harness), 1);
    harness
        .state_mut()
        .use_artwork(chosen, Some(picture(b"two")));
    at_time(&mut harness, 140.0);
    at_time(&mut harness, 171.0);
    assert_eq!(autosaves(&mut harness), 1);
}

#[test]
fn a_picture_chosen_under_the_artwork_s_own_name_still_counts_as_unsaved() {
    let mut song: serde_json::Value = serde_json::from_slice(&four_taps()).unwrap();
    song["metadata"]["artwork"] = serde_json::json!("cover.jpg");
    let mut app = Window::new(PLUGINS);
    app.open(
        Path::new("/songs/four.json"),
        &serde_json::to_vec(&song).unwrap(),
    );
    let mut harness = window(app);
    assert_eq!(harness.state().title(), "four.json - charter");
    harness
        .state_mut()
        .use_artwork(Path::new("/downloads/cover.jpg"), Some(picture(b"new")));
    harness.run();
    assert_eq!(harness.state().title(), "*four.json - charter");
    let file = Path::new("/songs/cover.jpg");
    assert!(harness.state().may_replace(file));
    assert!(
        !harness
            .state()
            .may_replace(Path::new("/elsewhere/cover.jpg"))
    );
}

#[test]
fn an_autosave_discarded_is_removed() {
    let (_, bytes) = autosaved_four_taps();
    let mut harness = window(Window::new(PLUGINS));
    harness.state_mut().recover(&bytes);
    harness.run();
    harness.get_by_label("Discard").click();
    harness.run();
    assert!(!restore_prompt(&harness));
    assert_eq!(request(&mut harness), Request::DropAutosave);
    assert!(harness.state().song.is_none());
}

#[test]
fn a_damaged_autosave_says_so_and_is_removed() {
    let mut harness = window(Window::new(PLUGINS));
    harness.state_mut().recover(b"{ not json");
    harness.run();
    assert!(
        harness
            .query_by_label_contains("could not be restored")
            .is_some()
    );
    assert_eq!(request(&mut harness), Request::DropAutosave);
}

#[test]
fn a_failed_autosave_says_why_and_tries_again_later() {
    let (mut harness, _) = autosaved_four_taps();
    harness.state_mut().autosave_failed("the disk is full");
    harness.run();
    assert!(
        harness
            .query_by_label_contains("Autosave failed: the disk is full")
            .is_some()
    );
    at_time(&mut harness, 70.0);
    assert!(matches!(request(&mut harness), Request::Autosave { .. }));
}

#[test]
fn a_lane_key_on_a_note_removes_it_and_leaves_the_cursor_there() {
    let mut harness = editable_demo();
    harness.key_press(egui::Key::Num4);
    harness.run();
    assert_eq!(lanes_at(&harness, 0), Vec::<u16>::new());
    assert!(cursor_shows(&harness, "0:0:0"));
}

#[test]
fn lane_keys_leave_a_read_only_song_alone() {
    let mut harness = with_demo();
    harness.key_press(egui::Key::Num1);
    harness.run();
    assert_eq!(lanes_at(&harness, 0), [3]);
    assert!(harness.query_by_label_contains("Refused").is_none());
}

#[test]
fn a_key_past_the_last_lane_does_nothing() {
    let mut harness = editable_demo();
    harness.key_press(egui::Key::Num5);
    harness.run();
    assert_eq!(lanes_at(&harness, 0), [3]);
    assert!(cursor_shows(&harness, "0:0:0"));
    assert!(harness.query_by_label_contains("Refused").is_none());
}

#[test]
fn an_edit_the_game_refuses_says_why() {
    const PICKY: &[&dyn RulesetPlugin] = &[&NoFourthLane];
    let small = serde_json::json!({
        "format_version": 0,
        "ruleset": { "id": "sm", "version": "1" },
        "metadata": { "title": "Small", "artist": "a", "audio": "a.ogg" },
        "timing": { "offset_seconds": 0.0, "events": [ { "tick": 0, "kind": { "bpm": 120.0 } } ] },
        "charts": [ { "name": "Easy", "lanes": 4 } ]
    });
    let mut app = Window::new(PICKY);
    app.open(
        Path::new("small.json"),
        &serde_json::to_vec(&small).unwrap(),
    );
    let mut harness = window(app);
    harness.key_press(egui::Key::Num4);
    harness.run();
    assert!(
        harness
            .query_by_label("Refused: a note on lane 4")
            .is_some()
    );
    assert_eq!(lanes_at(&harness, 0), Vec::<u16>::new());
}

#[test]
fn ctrl_z_takes_an_edit_back_and_ctrl_shift_z_or_ctrl_y_makes_it_again() {
    let mut harness = editable_demo();
    let command = egui::Modifiers::COMMAND;
    harness.key_press(egui::Key::Num1);
    harness.run();
    harness.key_press_modifiers(command, egui::Key::Z);
    harness.run();
    assert_eq!(lanes_at(&harness, 0), [3]);
    assert!(cursor_shows(&harness, "0:0:0"));
    harness.key_press_modifiers(command.plus(egui::Modifiers::SHIFT), egui::Key::Z);
    harness.run();
    assert_eq!(lanes_at(&harness, 0), [0, 3]);
    harness.key_press_modifiers(command, egui::Key::Z);
    harness.run();
    harness.key_press_modifiers(command, egui::Key::Y);
    harness.run();
    assert_eq!(lanes_at(&harness, 0), [0, 3]);
}

#[test]
fn the_edit_menu_undoes_and_redoes_what_it_can() {
    let mut harness = editable_demo();
    harness.get_by_label("Edit").click();
    harness.run();
    let disabled = |view: &Harness<'_, Window>, item: &str| {
        view.get_by_label_contains(item)
            .accesskit_node()
            .is_disabled()
    };
    assert!(disabled(&harness, "Undo") && disabled(&harness, "Redo"));
    harness.key_press(egui::Key::Escape);
    harness.key_press(egui::Key::Num1);
    harness.run();
    harness.get_by_label("Edit").click();
    harness.run();
    assert!(!disabled(&harness, "Undo"));
    harness.get_by_label_contains("Undo").click();
    harness.run();
    assert_eq!(lanes_at(&harness, 0), [3]);
    harness.get_by_label("Edit").click();
    harness.run();
    harness.get_by_label_contains("Redo").click();
    harness.run();
    assert_eq!(lanes_at(&harness, 0), [0, 3]);
}

#[test]
fn an_empty_window_offers_the_demo_song() {
    let mut harness = window(Window::default());
    harness.get_by_label("Open the demo song").click();
    harness.run();
    assert!(harness.query_by_label("Demo").is_some());
}

#[test]
fn an_opened_song_lists_its_charts() {
    let mut app = Window::default();
    app.open(Path::new("example.json"), example());
    let harness = window(app);
    assert!(harness.query_all_by_label("Future").next().is_some());
    assert!(harness.query_all_by_label("Beyond").next().is_some());
}

#[test]
fn a_chart_says_what_it_cannot_draw_without_its_ruleset() {
    let mut app = Window::default();
    app.open(Path::new("example.json"), example());
    let harness = window(app);
    let note = "1 object on the plane needs the arcaea plugin to be drawn";
    assert!(harness.query_by_label(note).is_some());
}

#[test]
fn clicking_a_chart_opens_it() {
    let mut harness = with_demo();
    harness.get_by_label("Hard").click();
    harness.run();
    let song = harness.state().song.as_ref().unwrap();
    assert_eq!(song.chart_index(), 1);
}

#[test]
fn a_file_that_breaks_a_rule_says_what_and_where() {
    let broken = String::from_utf8(example().to_vec())
        .unwrap()
        .replace("\"Beyond\"", "\"Future\"");
    let mut app = Window::default();
    app.open(Path::new("broken.json"), broken.as_bytes());
    let harness = window(app);
    assert!(
        harness
            .query_by_label_contains("broken.json did not open")
            .is_some()
    );
    assert!(harness.query_by_label_contains("charts[1]").is_some());
}

#[test]
fn the_keyboard_moves_the_cursor_and_changes_the_snap() {
    let mut harness = with_demo();
    for _ in 0..4 {
        harness.key_press(egui::Key::ArrowDown);
    }
    harness.run();
    assert!(cursor_shows(&harness, "0:1:0"));
    harness.key_press(egui::Key::ArrowRight);
    harness.run();
    assert!(harness.query_by_label("Snap 24th").is_some());
    harness.key_press(egui::Key::PageDown);
    harness.run();
    assert!(cursor_shows(&harness, "1:0:0"));
}

#[test]
fn ctrl_plus_zooms_the_notefield_and_leaves_the_window_its_size() {
    let mut harness = with_demo();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Plus);
    harness.run();
    assert!(harness.query_by_label("Zoom 125%").is_some());
    assert!((harness.ctx.zoom_factor() - 1.0).abs() < f32::EPSILON);
}

#[test]
fn dropping_a_song_on_the_window_opens_it() {
    let mut harness = window(Window::default());
    let file = Arc::new(Dropped("example.json", example().to_vec()));
    harness.input_mut().dropped_files.push(file);
    harness.run();
    assert!(harness.query_all_by_label("Future").next().is_some());
}

/// The StepMania and Tap Tap Revenge plugins, which start songs.
const GAMES: &[&dyn RulesetPlugin] = &[&charter_stepmania::Plugin, &charter_ttr::Plugin];

fn new_song_dialog() -> Harness<'static, Window> {
    let mut harness = window(Window::new(GAMES));
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
    harness.run();
    harness
}

fn dialog_open(harness: &Harness<'_, Window>) -> bool {
    harness.query_by_label("Create").is_some()
}

fn charts(harness: &Harness<'_, Window>) -> Vec<(String, Option<LaneCount>)> {
    let song = harness.state().song.as_ref().unwrap();
    let charts = &song.document().document().charts;
    charts
        .iter()
        .map(|chart| (chart.name.clone(), chart.lanes))
        .collect()
}

fn lanes(n: u16) -> Option<LaneCount> {
    LaneCount::new(n)
}

#[test]
#[expect(
    clippy::non_ascii_literal,
    reason = "the test clicks the New song menu label with its ellipsis"
)]
fn the_file_menu_and_ctrl_n_start_a_new_song_and_cancel_leaves_it() {
    let mut harness = window(Window::new(GAMES));
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label_contains("New song…").click();
    harness.run();
    assert!(dialog_open(&harness));
    harness.get_by_label("Cancel").click();
    harness.run();
    assert!(!dialog_open(&harness));
    assert!(harness.state().song.is_none());
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
    harness.run();
    assert!(dialog_open(&harness));
}

#[test]
fn shortcuts_work_with_no_song_open() {
    let mut harness = window(Window::new(GAMES));
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::O);
    harness.run();
    assert_eq!(request(&mut harness), Request::Open);
}

#[test]
fn the_empty_window_offers_a_new_song() {
    let mut harness = window(Window::new(GAMES));
    harness.get_by_label("New song").click();
    harness.run();
    assert!(dialog_open(&harness));
}

#[test]
fn a_new_song_starts_as_one_empty_chart_of_the_first_games_usual_layout() {
    let mut harness = new_song_dialog();
    harness.get_by_label("Create").click();
    harness.run();
    assert!(!dialog_open(&harness));
    assert_eq!(charts(&harness), [("Easy".to_owned(), lanes(4))]);
    let song = harness.state().song.as_ref().unwrap();
    assert_eq!(song.document().document().ruleset.id.as_str(), "sm");
    assert_eq!(song.layers()[0].name, "Layer 0");
    assert!(!song.is_modified());
    assert!(!song.can_undo());
    assert!(song.read_only().is_none());
    assert_eq!(harness.state().title(), "Untitled.json - charter");
}

#[test]
fn a_new_tap_tap_revenge_song_takes_any_levels_named_as_its_game_names_them() {
    let mut harness = new_song_dialog();
    harness.get_by_label("Tap Tap Revenge").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::ComboBox, "Lanes")
        .click();
    harness.run();
    harness.get_by_label("5 lanes").click();
    harness.run();
    let title = harness.get_by_role_and_label(Role::TextInput, "Title");
    title.focus();
    title.type_text("Mine");
    harness.run();
    let preset = harness.get_by_role_and_label(Role::ComboBox, "Preset");
    assert_eq!(preset.value().as_deref(), Some("Custom, Extreme only"));
    preset.click();
    harness.run();
    harness.get_by_label("Tap Tap Reloaded").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::CheckBox, "Easy")
        .click();
    harness.run();
    let selected_preset = harness.get_by_role_and_label(Role::ComboBox, "Preset");
    assert_eq!(selected_preset.value().as_deref(), Some("Your own"));
    harness.get_by_label("Create").click();
    harness.run();
    let named = |name: &str| (name.to_owned(), lanes(5));
    assert_eq!(
        charts(&harness),
        [
            named("Easy"),
            named("Metallica"),
            named("Downloads"),
            named("Customs")
        ]
    );
    let song = harness.state().song.as_ref().unwrap();
    let levels: Vec<_> = song
        .document()
        .document()
        .charts
        .iter()
        .map(|chart| chart.settings["difficulty"].clone())
        .collect();
    assert_eq!(levels, ["easy", "medium", "hard", "extreme"]);
    assert_eq!(harness.state().title(), "Mine.json - charter");
}

#[test]
fn the_new_song_dialog_keeps_its_top_and_its_fields_one_edge() {
    let mut harness = new_song_dialog();
    let top = |view: &Harness<'_, Window>| view.get_by_label_contains("Pick the game").rect().top();
    let left = |view: &Harness<'_, Window>, label: &str| {
        view.get_by_role_and_label(Role::TextInput, label)
            .rect()
            .left()
    };
    let before = top(&harness);
    let edge = left(&harness, "Title");
    assert!((left(&harness, "Chart 1") - edge).abs() < 0.5);
    harness.get_by_label("Tap Tap Revenge").click();
    harness.run();
    harness.run();
    assert!((top(&harness) - before).abs() < 0.5);
    let extreme = left(&harness, "Extreme chart name");
    assert!((extreme - edge).abs() < 0.5, "{extreme} {edge}");
    let preset = harness.get_by_role_and_label(Role::ComboBox, "Preset");
    assert!((preset.rect().left() - edge).abs() < 0.5);
}

#[test]
fn a_new_song_needs_a_chart() {
    let mut harness = new_song_dialog();
    harness.get_by_label("Remove Easy").click();
    harness.run();
    assert!(harness.query_by_label("Remove Easy").is_none());
    harness.get_by_label("Create").click();
    harness.run();
    assert!(dialog_open(&harness));
    assert!(harness.state().song.is_none());
}

#[test]
fn a_rejected_new_song_keeps_its_fields_and_shows_why() {
    let mut harness = new_song_dialog();
    let title = harness.get_by_role_and_label(Role::TextInput, "Title");
    title.focus();
    title.type_text("Mine");
    harness.run();
    harness.get_by_label("Add a chart").click();
    harness.run();
    let second = harness.get_by_role_and_label(Role::TextInput, "Chart 2");
    second.focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_role_and_label(Role::TextInput, "Chart 2")
        .type_text("Easy");
    harness.run();

    harness.get_by_label("Create").click();
    harness.run();
    assert!(harness.state().song.is_none());
    assert!(dialog_open(&harness));
    assert_eq!(
        harness
            .get_by_role_and_label(Role::TextInput, "Title")
            .value()
            .as_deref(),
        Some("Mine")
    );
    assert_eq!(
        harness
            .get_by_role_and_label(Role::TextInput, "Chart 2")
            .value()
            .as_deref(),
        Some("Easy")
    );
    assert!(
        harness
            .query_all_by_label_contains("Could not start the song")
            .next()
            .is_some()
    );
}

#[test]
fn cancelling_the_unsaved_song_prompt_keeps_the_new_song_draft() {
    let mut app = Window::new(GAMES);
    app.open(Path::new("/songs/four.json"), &four_taps());
    app.song.as_mut().unwrap().mark_unsaved();
    let mut harness = window(app);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
    harness.run();
    let title = harness.get_by_role_and_label(Role::TextInput, "Title");
    title.focus();
    title.type_text("Mine");
    harness.run();

    harness.get_by_label("Create").click();
    harness.run();
    assert!(asks_to_save(&harness));
    harness.state_mut().answer(panels::Answer::Cancel);
    harness.run();

    assert!(dialog_open(&harness));
    assert_eq!(
        harness
            .get_by_role_and_label(Role::TextInput, "Title")
            .value()
            .as_deref(),
        Some("Mine")
    );
    assert_eq!(harness.state().title(), "*four.json - charter");
}

#[test]
fn lane_keys_wait_while_the_new_song_dialog_shows() {
    let mut harness = editable_demo();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
    harness.run();
    harness.key_press(egui::Key::Num1);
    harness.run();
    harness.get_by_label("Cancel").click();
    harness.run();
    assert_eq!(lanes_at(&harness, 0), [3]);
}

fn created() -> Harness<'static, Window> {
    let mut harness = new_song_dialog();
    harness.get_by_label("Create").click();
    harness.run();
    harness
}

/// A file dropped on the window that only a path stands for.
#[derive(Debug)]
struct DroppedPath(&'static str);

impl egui::DroppedFile for DroppedPath {
    fn path(&self) -> &Path {
        Path::new(self.0)
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        Err("not read in tests".to_owned())
    }
}

/// Drops the file at `path` on the window, and forgets the tags it asks the host for.
fn drop_file(harness: &mut Harness<'_, Window>, path: &'static str) {
    drop_keeping_requests(harness, path);
    let requests = &mut harness.state_mut().requests;
    requests.retain(|request| !matches!(request, Request::ReadTags { .. }));
}

fn drop_keeping_requests(harness: &mut Harness<'_, Window>, path: &'static str) {
    harness
        .input_mut()
        .dropped_files
        .push(Arc::new(DroppedPath(path)));
    harness.run();
}

fn read_tags(harness: &mut Harness<'_, Window>, path: &str, title: &str, artist: &str) {
    let file = Path::new(path);
    let (title, artist) = (Some(title.to_owned()), Some(artist.to_owned()));
    harness.state_mut().tags_read(file, title, artist);
    harness.run();
}

fn field_value(harness: &Harness<'_, Window>, label: &str) -> Option<String> {
    harness
        .get_by_role_and_label(Role::TextInput, label)
        .value()
}

fn facts(harness: &Harness<'_, Window>) -> (String, String) {
    let song = harness.state().song.as_ref().unwrap();
    let metadata = &song.document().document().metadata;
    (metadata.title.clone(), metadata.artist.clone())
}

const OFFER: &str = "Do you want to import song metadata from tags?";

#[test]
fn audio_dropped_on_the_new_song_dialog_fills_a_blank_title_and_artist_from_its_tags() {
    let mut harness = new_song_dialog();
    drop_keeping_requests(&mut harness, "/music/track.ogg");
    assert_eq!(
        request(&mut harness),
        Request::ReadTags {
            file: PathBuf::from("/music/track.ogg")
        }
    );
    assert!(harness.query_by_label("track.ogg").is_some());
    read_tags(&mut harness, "/music/track.ogg", "Starlight", "The Lanes");
    assert_eq!(field_value(&harness, "Title").as_deref(), Some("Starlight"));
    assert!(harness.query_by_label_contains(OFFER).is_none());
    harness.get_by_label("Create").click();
    harness.run();
    assert_eq!(
        facts(&harness),
        ("Starlight".to_owned(), "The Lanes".to_owned())
    );
    assert_eq!(audio(&harness), "track.ogg");
    assert_eq!(
        harness.state().audio_file(),
        Some(PathBuf::from("/music/track.ogg"))
    );
    assert!(!harness.state().song.as_ref().unwrap().is_modified());
}

#[test]
fn a_new_song_takes_the_cover_its_audio_holds_and_writes_it_into_its_folder() {
    let mut harness = new_song_dialog();
    drop_file(&mut harness, "/music/track.flac");
    let (bytes, image) = (
        b"jpeg".to_vec(),
        egui::ColorImage::filled([2, 2], egui::Color32::RED),
    );
    harness
        .state_mut()
        .artwork_embedded(Path::new("/music/track.flac"), "jpg", (bytes, image));
    harness.get_by_label("Create").click();
    harness.run();
    let song = harness.state().song.as_ref().unwrap();
    let artwork = song.document().document().metadata.artwork.clone();
    assert_eq!(
        artwork.map(|path| path.to_string()).as_deref(),
        Some("cover.jpg")
    );
    assert_eq!(
        harness
            .state()
            .artwork_write_for(Path::new("/s/Track/Track.json")),
        Some((PathBuf::from("/s/Track/cover.jpg"), b"jpeg".to_vec()))
    );
}

#[test]
fn tags_for_a_title_already_typed_are_offered_rather_than_taken() {
    let mut harness = new_song_dialog();
    let title = harness.get_by_role_and_label(Role::TextInput, "Title");
    title.focus();
    title.type_text("Mine");
    harness.run();
    drop_file(&mut harness, "/music/track.ogg");
    read_tags(&mut harness, "/music/track.ogg", "Starlight", "The Lanes");
    assert!(harness.query_by_label_contains(OFFER).is_some());
    harness.get_by_label("Keep mine").click();
    harness.run();
    assert_eq!(field_value(&harness, "Title").as_deref(), Some("Mine"));
    read_tags(&mut harness, "/music/track.ogg", "Starlight", "The Lanes");
    harness.get_by_label("Import").click();
    harness.run();
    assert_eq!(field_value(&harness, "Title").as_deref(), Some("Starlight"));
}

#[test]
#[expect(
    clippy::non_ascii_literal,
    reason = "the test clicks the Choose audio button with its ellipsis"
)]
fn the_new_song_dialog_chooses_its_audio_through_the_host() {
    let mut harness = new_song_dialog();
    harness.get_by_label("Choose audio…").click();
    harness.run();
    assert_eq!(request(&mut harness), Request::ChooseAudio);
    harness
        .state_mut()
        .use_audio(Path::new("/music/picked.flac"));
    harness.run();
    assert!(harness.query_by_label("picked.flac").is_some());
    assert!(matches!(request(&mut harness), Request::ReadTags { .. }));
}

#[test]
fn a_new_songs_title_and_artist_follow_the_tags_of_audio_dropped_on_it() {
    let mut harness = created();
    drop_file(&mut harness, "/music/track.ogg");
    read_tags(&mut harness, "/music/track.ogg", "Starlight", "The Lanes");
    assert_eq!(
        facts(&harness),
        ("Starlight".to_owned(), "The Lanes".to_owned())
    );
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    let defaults = ("Untitled".to_owned(), "Unknown artist".to_owned());
    assert_eq!(facts(&harness), defaults);
}

#[test]
fn a_song_with_a_title_of_its_own_asks_before_taking_the_tags() {
    let mut harness = editable_four_taps();
    drop_file(&mut harness, "/music/track.ogg");
    read_tags(&mut harness, "/music/track.ogg", "Starlight", "The Lanes");
    assert_eq!(facts(&harness), ("Four".to_owned(), "a".to_owned()));
    harness.get_by_label("Import").click();
    harness.run();
    assert_eq!(
        facts(&harness),
        ("Starlight".to_owned(), "The Lanes".to_owned())
    );
}

#[test]
fn artist_tags_are_offered_even_when_the_song_title_is_unchanged() {
    let mut harness = editable_four_taps();
    drop_file(&mut harness, "/music/track.ogg");
    read_tags(&mut harness, "/music/track.ogg", "Four", "New artist");
    assert!(harness.query_by_label_contains(OFFER).is_some());
    harness.get_by_label("Import").click();
    harness.run();
    assert_eq!(
        facts(&harness),
        ("Four".to_owned(), "New artist".to_owned())
    );
}

#[test]
fn fallback_fonts_are_installed_in_the_windows_text_families() {
    let bytes = include_bytes!("../../../charter-style/assets/fonts/Inter-Regular.ttf");
    let app = Window::default().with_fallback_fonts(vec![("fallback".to_owned(), bytes.to_vec())]);
    let harness = window(app);
    harness.ctx.fonts(|fonts| {
        let definitions = fonts.definitions();
        assert!(definitions.font_data.contains_key("fallback"));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            assert!(
                definitions.families[&family]
                    .iter()
                    .any(|name| name == "fallback")
            );
        }
    });
}

#[test]
fn the_notefield_identifies_an_empty_chart_without_lanes() {
    let mut document = serde_json::to_value(charter_editor::demo::song().document()).unwrap();
    document["charts"][0]["objects"] = serde_json::json!([]);
    document["charts"][0]["lanes"] = serde_json::Value::Null;
    let mut app = Window::default().with_settings(Settings {
        song_panel: false,
        ..Settings::default()
    });
    app.open(
        Path::new("/songs/without-lanes.json"),
        &serde_json::to_vec(&document).unwrap(),
    );
    let harness = window(app);
    assert!(harness.query_by_label("no lanes").is_some());
}

#[test]
fn tags_for_audio_the_song_no_longer_plays_are_ignored() {
    let mut harness = editable_four_taps();
    drop_file(&mut harness, "/music/track.ogg");
    read_tags(&mut harness, "/music/other.ogg", "Starlight", "The Lanes");
    assert!(harness.query_by_label_contains(OFFER).is_none());
    assert_eq!(facts(&harness), ("Four".to_owned(), "a".to_owned()));
}

fn offset_field<'a>(harness: &'a Harness<'_, Window>) -> egui_kittest::Node<'a> {
    harness.get_by_role_and_label(Role::TextInput, "Offset")
}

fn offset(harness: &Harness<'_, Window>) -> serde_json::Value {
    let song = harness.state().song.as_ref().unwrap();
    serde_json::to_value(
        song.document()
            .timing()
            .time_at(charter_core::Tick::new(0))
            .seconds(),
    )
    .unwrap()
}

fn enter_offset(harness: &mut Harness<'_, Window>, text: &str) {
    offset_field(harness).focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    offset_field(harness).type_text(text);
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
}

#[test]
fn the_offset_is_typed_in_milliseconds() {
    let mut harness = editable_demo();
    assert_eq!(offset_field(&harness).value().as_deref(), Some("0"));
    enter_offset(&mut harness, "1500.25");
    assert_eq!(offset(&harness), serde_json::json!(1.50025));
    assert_eq!(offset_field(&harness).value().as_deref(), Some("1500.25"));
}

#[test]
fn an_offset_that_is_not_a_time_says_so() {
    let mut harness = editable_demo();
    enter_offset(&mut harness, "soon");
    assert!(
        harness
            .query_by_label_contains("soon is not an offset in milliseconds")
            .is_some()
    );
}

#[test]
fn alt_arrows_and_the_offset_steppers_nudge_it_by_a_millisecond_or_ten_with_shift() {
    let mut harness = editable_demo();
    harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowRight);
    harness.run();
    assert_eq!(offset(&harness), serde_json::json!(0.001));
    let coarse = egui::Modifiers::ALT.plus(egui::Modifiers::SHIFT);
    harness.key_press_modifiers(coarse, egui::Key::ArrowLeft);
    harness.run();
    assert_eq!(offset(&harness), serde_json::json!(-0.009));
    harness.get_by_label("Raise the offset by 1 ms").click();
    harness.run();
    assert_eq!(offset(&harness), serde_json::json!(-0.008));
    harness.get_by_label("Lower the offset by 1 ms").click();
    harness.run();
    assert_eq!(offset(&harness), serde_json::json!(-0.009));
}

#[test]
fn the_tempo_section_opens_the_sync_window() {
    let mut harness = editable_demo();
    click_in_panel(&mut harness, "Sync to the audio");
    assert!(harness.query_by_label("Row at 0.0 s").is_some());
}

fn no_audio(harness: &Harness<'_, Window>) -> bool {
    harness.query_by_label_contains("No audio yet").is_some()
}

fn audio(harness: &Harness<'_, Window>) -> String {
    let song = harness.state().song.as_ref().unwrap();
    song.document()
        .document()
        .metadata
        .audio
        .as_str()
        .to_owned()
}

#[test]
fn a_new_song_asks_for_audio_until_a_file_is_dropped_on_it() {
    let mut harness = created();
    assert!(no_audio(&harness));
    drop_file(&mut harness, "/music/track.ogg");
    assert!(!no_audio(&harness));
    assert_eq!(audio(&harness), "track.ogg");
    assert_eq!(harness.state().title(), "*Untitled.json - charter");
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert!(no_audio(&harness));
}

#[test]
fn saving_a_song_plans_to_copy_dropped_audio_beside_it() {
    let mut harness = created();
    drop_file(&mut harness, "/music/track.ogg");
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    harness.run();
    assert!(matches!(request(&mut harness), Request::SaveAs { .. }));
    let path = PathBuf::from("/songs/mine.json");
    assert_eq!(
        harness.state().audio_copy_for(&path),
        Some((
            PathBuf::from("/music/track.ogg"),
            PathBuf::from("/songs/track.ogg"),
        ))
    );
    harness.state_mut().saved(Saved::To(path.clone()));
    harness.run();
    assert_eq!(harness.state_mut().take_requests(), []);
    assert_eq!(harness.state().audio_copy_for(&path), None);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    harness.run();
    assert!(matches!(request(&mut harness), Request::Write { .. }));
    harness.state_mut().saved(Saved::To(path));
    harness.run();
    assert_eq!(harness.state_mut().take_requests(), []);
}

#[test]
fn audio_already_beside_the_song_is_not_copied() {
    let mut harness = created();
    drop_file(&mut harness, "/songs/track.ogg");
    let path = PathBuf::from("/songs/mine.json");
    assert_eq!(harness.state().audio_copy_for(&path), None);
    harness.state_mut().saved(Saved::To(path));
    harness.run();
    assert_eq!(harness.state_mut().take_requests(), []);
}

#[test]
fn audio_that_could_not_be_copied_says_why() {
    let mut harness = created();
    harness.state_mut().audio_not_copied("the disk is full");
    harness.run();
    assert!(
        harness
            .query_by_label_contains("Audio not copied: the disk is full")
            .is_some()
    );
}

#[test]
fn audio_dropped_with_no_song_open_says_to_open_one() {
    let mut harness = window(Window::new(GAMES));
    drop_file(&mut harness, "/music/track.ogg");
    assert!(
        harness
            .query_by_label_contains("Open or start a song before adding its audio")
            .is_some()
    );
}

#[test]
fn audio_dropped_on_a_read_only_song_is_refused() {
    let mut harness = window(Window::new(&[]));
    harness.state_mut().open_demo();
    harness.run();
    drop_file(&mut harness, "/music/track.ogg");
    assert_eq!(audio(&harness), "demo.ogg");
    assert!(harness.query_by_label_contains("Refused").is_some());
}

#[test]
fn a_read_only_song_refuses_edits_from_actions_without_changing_its_document() {
    let mut app = Window::new(&[]);
    app.open(Path::new("four.json"), &four_taps());
    let before = app.song.as_ref().unwrap().document().to_json().unwrap();
    for action in [
        Action::Place {
            tick: Tick::new(1920),
            lane: LaneIndex::new(0),
            note: Note::Tap,
        },
        Action::SetSection("Verse".to_owned()),
        Action::ToggleBookmark,
        Action::NudgeBpm(1.0),
        Action::SetOffset(charter_core::Origin::try_from(0.1).unwrap()),
    ] {
        app.apply(action);
        assert!(
            app.message()
                .is_some_and(|message| message.starts_with("Refused:"))
        );
        assert_eq!(
            app.song.as_ref().unwrap().document().to_json().unwrap(),
            before
        );
    }
    assert!(!app.song.as_ref().unwrap().is_modified());
}

#[test]
fn editing_actions_without_a_song_neither_create_one_nor_request_a_save() {
    let mut app = Window::new(PLUGINS);
    for action in [
        Action::Place {
            tick: Tick::new(0),
            lane: LaneIndex::new(0),
            note: Note::Tap,
        },
        Action::Undo,
        Action::Redo,
        Action::Copy,
        Action::Paste(None),
        Action::FindTiming,
        Action::TogglePlay,
        Action::Save,
        Action::SaveAs,
    ] {
        app.apply(action);
    }
    let mut harness = window(app);
    assert!(harness.state().song.is_none());
    assert_eq!(harness.state().title(), "charter");
    assert_eq!(harness.state_mut().take_requests(), []);
    assert_eq!(
        harness.state().message(),
        Some("Finding the timing needs the song's audio")
    );
}

#[test]
fn a_loop_restarts_from_its_beginning_when_its_sound_changes() {
    let mut harness = listening(Settings {
        note_ticks: true,
        ..Settings::default()
    });
    harness.key_press(egui::Key::L);
    harness.run();
    played(&mut harness);
    harness.state_mut().playhead(1.0);
    harness.state_mut().settings.tuned_lanes = true;
    harness.run();
    let requests = harness.state_mut().take_requests();
    assert!(
        requests
            .iter()
            .any(|request| matches!(request, Request::Pause))
    );
    let Some(Request::Play {
        from,
        looping,
        clicks,
        ..
    }) = requests.last()
    else {
        panic!("expected restarted loop: {requests:?}");
    };
    assert!(from.abs() < 1e-9);
    assert_eq!(*looping, Some((0.0, 2.0)));
    assert!(
        clicks
            .iter()
            .any(|(_, tone)| *tone != charter_audio::Tick::Click.tone())
    );
    harness.run();
    assert_eq!(harness.state_mut().take_requests(), []);
}

#[test]
fn a_clipboard_reply_cancels_the_wait_for_the_last_copied_notes() {
    let mut harness = editable_four_taps();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.event(egui::Event::Copy);
    harness.run();
    let clipped = Clipped {
        charter_notes: harness
            .state()
            .clipboard
            .clip
            .iter()
            .take(1)
            .cloned()
            .collect(),
    };
    harness
        .state_mut()
        .apply(Action::PasteHere(Tick::new(3840)));
    harness.step();
    harness.event(egui::Event::Paste(serde_json::to_string(&clipped).unwrap()));
    harness.run();
    for _ in 0..PASTE_WAIT {
        harness.step();
    }
    assert_eq!(note_count(&harness), 5);
    assert_eq!(lanes_at(&harness, 3840), [0]);
    assert_eq!(harness.state().clipboard.pasting, None);
}

#[test]
fn a_failed_autosave_is_retried_after_the_delay_without_losing_the_edit() {
    let (mut harness, _) = autosaved_four_taps();
    harness.state_mut().autosave_failed("out of space");
    at_time(&mut harness, 32.0);
    assert!(
        harness
            .query_by_label_contains("Autosave failed: out of space")
            .is_some()
    );
    assert_eq!(harness.state_mut().take_requests(), []);
    at_time(&mut harness, 62.0);
    let Request::Autosave { bytes } = request(&mut harness) else {
        panic!("expected a retry");
    };
    let kept = recovery::read(&bytes, PLUGINS).unwrap();
    assert!(holds_tap(&kept.song.document().to_json().unwrap(), 0, 3));
}

#[test]
fn a_failed_timing_search_keeps_the_original_timing_and_stops_searching() {
    let mut harness = playable();
    let before = anchor_seconds(&harness);
    find_from_the_menu(&mut harness);
    harness.state_mut().take_requests();
    harness
        .state_mut()
        .timing_found(Err("no beats detected".to_owned()));
    harness.run();
    assert_eq!(anchor_seconds(&harness), before);
    assert!(
        harness
            .query_by_label_contains("no beats detected")
            .is_some()
    );
    assert!(
        harness
            .query_by_label_contains("Finding the timing")
            .is_none()
    );
}

#[test]
fn a_timing_result_for_a_song_that_cannot_be_edited_is_refused() {
    let mut app = Window::new(&[]);
    app.open(Path::new("four.json"), &four_taps());
    let before = app.song.as_ref().unwrap().document().to_json().unwrap();
    let found = serde_json::from_value(serde_json::json!({
        "anchors": [{ "tick": 0, "seconds": 1.0 }],
        "tempo": 120.0,
        "events": []
    }))
    .unwrap();
    app.timing_found(Ok(found));
    assert!(
        app.message()
            .is_some_and(|message| message.contains("timing found was refused"))
    );
    assert_eq!(
        app.song.as_ref().unwrap().document().to_json().unwrap(),
        before
    );

    let mut empty = Window::new(PLUGINS);
    empty.weights_needed();
    empty.timing_found(Ok(fresh_timing()));
    assert_eq!(empty.dialogs.finding, Finding::Idle);
    assert!(empty.song.is_none());
}

#[test]
fn playback_without_accessible_audio_explains_how_to_supply_it() {
    let mut unsaved = Window::new(PLUGINS);
    unsaved.open(Path::new("four.json"), &four_taps());
    unsaved.path = None;
    unsaved.apply(Action::TogglePlay);
    assert!(
        unsaved
            .message()
            .is_some_and(|message| message.contains("Save the song beside a.ogg"))
    );
    assert_eq!(unsaved.take_requests(), []);

    let mut harness = created();
    let new = harness.state_mut();
    new.apply(Action::TogglePlay);
    assert!(
        new.message()
            .is_some_and(|message| message.contains("drop an audio file"))
    );
    new.no_drops = true;
    new.apply(Action::TogglePlay);
    assert!(
        new.message()
            .is_some_and(|message| message.contains("choose one"))
    );
    assert!(!new.is_playing());
}

#[test]
fn stale_onsets_do_not_follow_a_new_audio_file() {
    let mut app = Window::new(PLUGINS);
    app.open(Path::new("/songs/four.json"), &four_taps());
    let onsets: Arc<[charter_audio::Onset]> = Arc::from([]);
    app.onsets_ready(Path::new("/songs/a.ogg"), onsets);
    assert_eq!(app.onsets().map(<[charter_audio::Onset]>::len), Some(0));
    app.use_audio(Path::new("/music/other.ogg"));
    assert!(app.onsets().is_none());
    assert_eq!(app.audio_file(), Some(PathBuf::from("/music/other.ogg")));
}

#[test]
fn a_timing_file_read_opens_the_import_and_a_read_failure_explains_it() {
    let mut app = Window::new(PLUGINS);
    app.timing_file_read("not timing");
    assert!(app.import.is_open());
    assert_eq!(app.import.text, "not timing");
    app.timing_file_failed("permission denied");
    assert_eq!(
        app.message(),
        Some("Could not read the timing: permission denied")
    );
}
