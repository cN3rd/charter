use super::{egui, heading, style, table};

/// The keyboard shortcuts, grouped, while `open` is set, showing those that match `filter`.
pub(crate) fn shortcuts(ctx: &egui::Context, open: &mut bool, filter: &mut String) {
    let space = ctx.content_rect().size();
    let compact = space.x < 760.0;
    let width = (space.x - 24.0).max(260.0);
    let height = (space.y - 24.0).max(180.0);
    let window = egui::Window::new("Keyboard shortcuts")
        .open(open)
        .collapsible(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .min_size([260.0, 180.0]);
    let window = if compact {
        window.default_size([width, height])
    } else {
        window.default_size([640.0, 560.0])
    };
    window.show(ctx, |ui| {
        ui.horizontal(|ui| {
            let caption = ui.weak("Filter");
            let field = egui::TextEdit::singleline(filter)
                .hint_text("a key or what it does")
                .desired_width(if compact { width - 100.0 } else { 280.0 });
            ui.add(field).labelled_by(caption.id);
        });
        let wanted = filter.trim().to_lowercase();
        let shown = |entry: &&(&str, &str)| {
            wanted.is_empty()
                || entry.0.to_lowercase().contains(&wanted)
                || entry.1.to_lowercase().contains(&wanted)
        };
        style::solid_scroll_bars(ui);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (group, keys) in KEYS {
                    let keys: Vec<_> = keys.iter().filter(shown).collect();
                    if keys.is_empty() {
                        continue;
                    }
                    heading(ui, group);
                    if compact {
                        for (shortcut, does) in keys {
                            ui.monospace(*shortcut);
                            ui.label(*does);
                            ui.add_space(8.0);
                        }
                    } else {
                        table(ui, group, |ui| {
                            for (shortcut, does) in keys {
                                ui.monospace(*shortcut);
                                ui.label(*does);
                                ui.end_row();
                            }
                        });
                    }
                }
            });
    });
}

/// The keys and gestures the editor answers to, grouped, and what each does.
const KEYS: [(&str, &[(&str, &str)]); 7] = [
    (
        "Note entry",
        &[
            (
                "1 to 9, 0",
                "Place/remove taps; while playing, record on the nearest snap \
                 (input latency adjusted)",
            ),
            (
                "Hold 1 to 0, move",
                "Draw a hold; while playing, hold past 200 ms to grow it",
            ),
            ("Shift+1 to 0", "Place or remove a mine"),
            ("Ctrl+Click", "Place or remove a tap"),
            ("Ctrl+Shift+Click", "Place or remove a mine"),
            ("Ctrl+Drag", "Draw a hold"),
            (
                "[, ]",
                "Shorten or lengthen the selection, or the notes at the cursor, by the snap",
            ),
        ],
    ),
    (
        "Selection",
        &[
            ("Click, Drag", "Select notes"),
            ("Drag a note", "Move it, or the selection it is in"),
            (
                "Shift+Drag a note",
                "Move it across lanes, keeping its time",
            ),
            (
                "Right-click",
                "Menu: edits, patterns, the game's note changes",
            ),
            (
                "Ctrl+X, Ctrl+C, Ctrl+V",
                "Cut, copy, or paste at the cursor",
            ),
            ("Ctrl+D", "Duplicate the selection after itself"),
            ("Ctrl+Left, Ctrl+Right", "Move the selection a lane"),
            ("Ctrl+Up, Ctrl+Down", "Move the selection by the snap"),
            ("Ctrl+M", "Mirror the selection across the lanes"),
            ("G", "Snap the selection to the nearest snap lines"),
            ("Shift+Click, Shift+Drag", "Add to the selection"),
            ("Alt+Click, Alt+Drag", "Take from the selection"),
            ("Ctrl+A, Escape", "Select every note, or none"),
            ("Delete, Backspace", "Delete the selection"),
            ("Ctrl+Z", "Undo"),
            ("Ctrl+Shift+Z, Ctrl+Y", "Redo"),
        ],
    ),
    (
        "Layers",
        &[
            ("Ctrl+Shift+N", "Add a layer above the active one"),
            ("Alt+1 to 9", "Make that layer active"),
            ("Alt+Shift+1 to 9", "Move selected notes to that layer"),
            (
                "Ctrl+Shift+Up, Ctrl+Shift+Down",
                "Choose the adjacent layer",
            ),
            ("Ctrl+E", "Merge the active layer down"),
            ("Ctrl+Shift+H", "Show or hide the active layer"),
        ],
    ),
    (
        "Moving around",
        &[
            ("Up, Down, Wheel", "Move the cursor by the snap"),
            (
                "Page Up, Page Down, Shift+Wheel",
                "Move the cursor by a measure",
            ),
            ("Middle-drag", "Pan the chart"),
            (
                "Home, End",
                "Jump to the start, or to where the last note ends",
            ),
            ("Alt+Up, Alt+Down", "Jump to the previous or next section"),
            ("Left, Right", "Coarser or finer snap"),
            ("Ctrl+Wheel, Ctrl+Plus, Ctrl+Minus", "Zoom"),
        ],
    ),
    (
        "Timing",
        &[
            ("Ctrl+T", "Show or hide the timing window"),
            ("Ctrl+B", "Add or remove a bookmark at the cursor"),
            ("F7, F8", "Lower or raise the tempo at the cursor by 1 BPM"),
            (
                "Alt+Left, Alt+Right",
                "Lower or raise the offset by 1 ms, or by 10 ms with Shift",
            ),
        ],
    ),
    (
        "Playback",
        &[
            ("Space", "Play from the cursor, or pause"),
            ("Q", "Play a moment before the cursor"),
            ("W", "Play a moment after the cursor"),
            ("Shift+Left, Shift+Right", "Play slower or faster by 10%"),
            (
                "Ctrl+Shift+Left, Ctrl+Shift+Right",
                "Play slower or faster by 1%",
            ),
            ("F3, F4", "Turn the metronome or the note ticks on or off"),
            (
                "Shift+F4",
                "Tune each lane's note ticks to a pitch of its own, or back",
            ),
        ],
    ),
    (
        "Files and windows",
        &[
            ("Ctrl+N", "New song"),
            ("Ctrl+O, Ctrl+S, Ctrl+Shift+S", "Open, save, save as"),
            ("Ctrl+Comma", "Settings"),
            ("Ctrl+\\", "Show or hide the song panel"),
            ("F1", "Keyboard shortcuts"),
        ],
    ),
];
