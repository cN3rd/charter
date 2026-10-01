use super::gesture::{hit_on_layers, snap_line};
use super::{Action, Field, Flip, LaneIndex, Selecting, Song, Step, Tick, Ui, egui};

/// The patterns the right-click menu places, by the lanes each step counts from the lane
/// clicked, and the spacings it places them at, as how many to a beat.
const PATTERNS: [(&str, &[u16]); 3] = [
    ("1-1-1-1", &[0, 0, 0, 0]),
    ("1-2-3-1-2-3", &[0, 1, 2, 0, 1, 2]),
    ("1-2-1-2", &[0, 1, 0, 1]),
];
/// Note values offered for the spacing between pattern steps.
const SPACINGS: [u16; 7] = [1, 2, 4, 8, 16, 32, 64];

/// The right-click menu. A right click selects the note under the pointer unless it is selected
/// already, and leaves the cursor alone; what the menu places goes on the snap line clicked, from
/// the lane clicked.
pub(super) fn menu(
    ui: &Ui,
    response: &egui::Response,
    song: &Song,
    (field, flip, visible): (&Field, Flip, (Tick, Tick)),
    actions: &mut Vec<Action>,
) {
    let spot_id = ui.id().with("menu spot");
    if response.secondary_clicked()
        && let Some(pos) = ui.input(|input| input.pointer.latest_pos().map(|at| flip.back(at)))
    {
        let tick = snap_line(song, field.view, pos);
        if let Some(chart) = song.chart()
            && let Some(note) = hit_on_layers(song, field, chart.timing(), pos, visible)
            && !song.selection().any(|selected| selected == note)
        {
            actions.push(Action::Select(Some(note), Selecting::Replace));
        }
        let lane = field.lane_at(pos.x).unwrap_or(LaneIndex::new(0));
        let tick = tick.unwrap_or_else(|| song.cursor());
        ui.data_mut(|data| data.insert_temp(spot_id, (lane, tick)));
    }
    // Where it was clicked, since playback moves the cursor on while the menu is open.
    let spot: (LaneIndex, Tick) = ui
        .data(|data| data.get_temp(spot_id))
        .unwrap_or_else(|| (LaneIndex::new(0), song.cursor()));
    response.context_menu(|menu_ui| menu_items(menu_ui, song, spot, actions));
}

/// The right-click menu's items for `song`, with `lane` and `at` the lane and snap line clicked,
/// where what it places goes.
#[expect(clippy::too_many_lines, reason = "menu items in display order")]
pub(super) fn menu_items(
    ui: &mut Ui,
    song: &Song,
    (lane, at): (LaneIndex, Tick),
    actions: &mut Vec<Action>,
) {
    let editable = song.read_only().is_none();
    let selected = song.selection().next().is_some();
    let (edits, chosen) = (editable, editable && selected);
    item(ui, actions, ("Cut", "Ctrl+X", chosen), Action::Cut);
    item(ui, actions, ("Copy", "Ctrl+C", selected), Action::Copy);
    item(
        ui,
        actions,
        ("Paste here", "Ctrl+V", edits),
        Action::PasteHere(at),
    );
    item(
        ui,
        actions,
        ("Duplicate", "Ctrl+D", chosen),
        Action::Duplicate,
    );
    item(
        ui,
        actions,
        ("Delete", "Delete", chosen),
        Action::DeleteSelection,
    );
    if song.layers().len() > 1 {
        ui.add_enabled_ui(chosen, |ui| {
            ui.menu_button("Move to layer", |ui| {
                for (index, row) in song.layers().iter().enumerate() {
                    let already_there = song.chart().is_some_and(|chart| {
                        chart
                            .objects()
                            .filter(|object| song.is_selected(object))
                            .all(|object| chart.layer_index(&object) == Some(index))
                    });
                    if !already_there && ui.button(&row.name).clicked() {
                        actions.push(Action::MoveSelectionToLayer(index));
                        ui.close();
                    }
                }
            });
        });
    }
    ui.separator();
    let move_left = Action::ShiftSelection {
        snaps: 0,
        lanes: -1,
    };
    let move_right = Action::ShiftSelection { snaps: 0, lanes: 1 };
    item(
        ui,
        actions,
        ("Move a lane left", "Ctrl+Left", chosen),
        move_left,
    );
    item(
        ui,
        actions,
        ("Move a lane right", "Ctrl+Right", chosen),
        move_right,
    );
    item(
        ui,
        actions,
        ("Mirror across the lanes", "Ctrl+M", chosen),
        Action::Mirror,
    );
    item(
        ui,
        actions,
        ("Snap to the grid", "G", chosen),
        Action::Quantize,
    );
    item(
        ui,
        actions,
        ("Lengthen by the snap", "]", edits),
        Action::ResizeNotes(Step::Later),
    );
    item(
        ui,
        actions,
        ("Shorten by the snap", "[", edits),
        Action::ResizeNotes(Step::Earlier),
    );
    let changes = song
        .plugin()
        .map(charter_core::RulesetPlugin::note_changes)
        .unwrap_or_default();
    if !changes.is_empty() {
        ui.add_enabled_ui(chosen, |ui| {
            ui.menu_button("Change to", |ui| {
                for change in changes {
                    let button = ui.button(&change.name).on_hover_text(&change.about);
                    if button.clicked() {
                        actions.push(Action::ChangeNotes(change.id));
                    }
                }
            });
        });
    }
    ui.separator();
    ui.add_enabled_ui(edits, |ui| patterns(ui, (lane, at), actions));
    let lane_name = format!("Select lane {}", u16::from(lane).saturating_add(1));
    item(
        ui,
        actions,
        (&lane_name, "", true),
        Action::SelectLane(lane),
    );
    item(
        ui,
        actions,
        ("Select all", "Ctrl+A", true),
        Action::SelectAll,
    );
    item(
        ui,
        actions,
        ("Select none", "Escape", selected),
        Action::ClearSelection,
    );
    ui.separator();
    item(
        ui,
        actions,
        ("Add a timing point here", "", edits),
        Action::Here(at, Box::new(Action::AddBpm)),
    );
    item(
        ui,
        actions,
        ("Bookmark here", "Ctrl+B", edits),
        Action::Here(at, Box::new(Action::ToggleBookmark)),
    );
}

/// The right-click menu's patterns, each at every spacing, placed from `lane` on.
pub(super) fn patterns(ui: &mut Ui, (lane, at): (LaneIndex, Tick), actions: &mut Vec<Action>) {
    ui.menu_button("Insert pattern here", |ui| {
        for (name, steps) in PATTERNS {
            ui.menu_button(name, |ui| {
                for per_beat in SPACINGS {
                    let label = if per_beat == 1 {
                        "Every beat".to_owned()
                    } else {
                        format!("Every 1/{per_beat} beat")
                    };
                    if ui.button(label).clicked() {
                        let pattern = Action::Pattern {
                            steps,
                            per_beat,
                            lane,
                        };
                        actions.push(Action::Here(at, Box::new(pattern)));
                    }
                }
            });
        }
    });
}

/// A menu item named `name`, showing `keys`, that asks for `action` when clicked.
fn item(
    ui: &mut Ui,
    actions: &mut Vec<Action>,
    (name, keys, enabled): (&str, &str, bool),
    action: Action,
) {
    let button = egui::Button::new(name);
    let button = if keys.is_empty() {
        button
    } else {
        button.shortcut_text(keys)
    };
    if ui.add_enabled(enabled, button).clicked() {
        actions.push(action);
    }
}
