use super::{
    Action, Align, LayerIndex, Layout, NoteValue, Rate, RichText, Song, Tick, Ui, Zoom, egui, icon,
    style,
};
use egui::AtomExt as _;

/// Draws the playback and editing controls, sending their choices as `actions`.
#[expect(clippy::too_many_lines, reason = "toolbar groups in display order")]
pub(crate) fn transport_bar(
    ui: &mut Ui,
    song: Option<&Song>,
    (zoom, rate, playing, panel, grid): (Zoom, Rate, bool, bool, NoteValue),
    (metronome, (ticks, tuned), volumes): (bool, (bool, bool), charter_audio::Volumes),
    actions: &mut Vec<Action>,
) {
    let width = ui.available_width();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        // One name in either state, which the toggle reports, and a tip saying what a click does.
        let panel_tip = if panel {
            "Hide the song panel (Ctrl+\\)"
        } else {
            "Show the song panel (Ctrl+\\)"
        };
        if style::tool(
            ui,
            icon::SIDEBAR_SIMPLE,
            "Show the song panel",
            panel_tip,
            Some(panel),
        )
        .clicked()
        {
            actions.push(Action::ToggleSongPanel);
        }
        ui.add_enabled_ui(song.is_some(), |ui| {
            style::chip(ui, |ui| {
                let start = Action::JumpTo(Tick::new(0));
                if style::tool(ui, icon::SKIP_BACK, "Jump to the start", "Home", None).clicked() {
                    actions.push(start);
                }
                let (symbol, playback_tip) = if playing {
                    (icon::PAUSE, "Pause (Space)")
                } else {
                    (icon::PLAY, "Play from the cursor (Space)")
                };
                if style::tool(ui, symbol, "Play", playback_tip, Some(playing)).clicked() {
                    actions.push(Action::TogglePlay);
                }
                if style::tool(ui, icon::SKIP_FORWARD, "Jump to the end", "End", None).clicked() {
                    actions.push(Action::JumpEnd);
                }
            });
        });
        if let Some(song) = song {
            readout(ui, song);
            if width >= 800.0 {
                style::chip(ui, |ui| {
                    ui.label(style::glyph(ui, icon::METRONOME).color(style::WEAK_TEXT));
                    let tempo = song
                        .bpm()
                        .map_or_else(|| "no BPM".to_owned(), |(_, bpm)| format!("{bpm} BPM"));
                    ui.label(tempo);
                });
                style::chip(ui, |ui| {
                    ui.label(style::glyph(ui, icon::GRID_FOUR).color(style::WEAK_TEXT));
                    if style::quiet_icon(
                        ui,
                        icon::CARET_LEFT,
                        "Coarser snap",
                        "Coarser snap (Left)",
                    )
                    .clicked()
                    {
                        actions.push(Action::CoarserSnap);
                    }
                    ui.label(format!("Snap {}", ordinal(song.snap())));
                    if style::quiet_icon(ui, icon::CARET_RIGHT, "Finer snap", "Finer snap (Right)")
                        .clicked()
                    {
                        actions.push(Action::FinerSnap);
                    }
                    if width >= 1200.0 {
                        let mut chosen = grid;
                        let caption = ui.weak("Grid");
                        crate::settings::grid_choice(ui, &mut chosen, "toolbar grid lines")
                            .on_hover_text("Lines between the beats down to this note")
                            .labelled_by(caption.id)
                            .widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::ComboBox,
                                    true,
                                    "Grid lines",
                                )
                            });
                        if chosen != grid {
                            actions.push(Action::SetGrid(chosen));
                        }
                        // The notefield draws the snap's lines when the grid leaves some out.
                        if !grid.get().is_multiple_of(song.snap().get()) {
                            ui.weak("+ snap").on_hover_text(format!(
                                "The notefield draws the {} lines too, since the snap lands on \
                                 them",
                                ordinal(song.snap())
                            ));
                        }
                    }
                });
            }
            if width >= 950.0 {
                style::chip(ui, |ui| {
                    ui.label(style::glyph(ui, icon::GAUGE).color(style::WEAK_TEXT));
                    if style::quiet_icon(ui, icon::MINUS, "Slower", "Play slower (Shift+Left)")
                        .clicked()
                    {
                        actions.push(Action::NudgeRate(-0.1));
                    }
                    ui.label(RichText::new(format!("Speed {:.0}%", rate.percent())).monospace());
                    if style::quiet_icon(ui, icon::PLUS, "Faster", "Play faster (Shift+Right)")
                        .clicked()
                    {
                        actions.push(Action::NudgeRate(0.1));
                    }
                });
            }
            if width >= 1200.0 {
                style::chip(ui, |ui| {
                    if style::quiet_icon(
                        ui,
                        icon::MAGNIFYING_GLASS_MINUS,
                        "Zoom out",
                        "Zoom out (Ctrl+Minus)",
                    )
                    .clicked()
                    {
                        actions.push(Action::ZoomBy(Zoom::STEP.recip()));
                    }
                    ui.label(RichText::new(format!("Zoom {:.0}%", zoom.percent())).monospace());
                    if style::quiet_icon(
                        ui,
                        icon::MAGNIFYING_GLASS_PLUS,
                        "Zoom in",
                        "Zoom in (Ctrl+Plus)",
                    )
                    .clicked()
                    {
                        actions.push(Action::ZoomBy(Zoom::STEP));
                    }
                });
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if style::tool(
                ui,
                icon::GEAR_SIX,
                "Open settings",
                "Settings (Ctrl+Comma)",
                None,
            )
            .clicked()
            {
                actions.push(Action::Settings);
            }
            style::chip(ui, |ui| {
                if style::tool(
                    ui,
                    icon::MUSIC_NOTE_SIMPLE,
                    "Note ticks",
                    "Note ticks (F4)",
                    Some(ticks),
                )
                .clicked()
                {
                    actions.push(Action::ToggleTicks);
                }
                if style::tool(
                    ui,
                    icon::PIANO_KEYS,
                    "Tuned lanes",
                    "Tuned lanes: each lane ticks in its own pitch (Shift+F4)",
                    Some(tuned),
                )
                .clicked()
                {
                    actions.push(Action::ToggleTunedLanes);
                }
                if style::tool(
                    ui,
                    icon::METRONOME,
                    "Metronome",
                    "Metronome (F3)",
                    Some(metronome),
                )
                .clicked()
                {
                    actions.push(Action::ToggleMetronome);
                }
                let button = style::tool(ui, icon::SPEAKER_HIGH, "Volume", "Volume", None);
                let popup = egui::Popup::menu(&button)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
                popup.show(|popup_ui| {
                    let mut changed = volumes;
                    style::table(popup_ui, "volumes", |volume_ui| {
                        for (volume_name, gain) in [
                            ("Music volume", &mut changed.music),
                            ("Note tick volume", &mut changed.ticks),
                            ("Metronome volume", &mut changed.metronome),
                        ] {
                            let caption = volume_ui.weak(volume_name);
                            crate::settings::volume(volume_ui, gain).labelled_by(caption.id);
                            volume_ui.end_row();
                        }
                    });
                    if changed != volumes {
                        actions.push(Action::SetVolumes(changed));
                    }
                });
            });
        });
    });
}

/// Where the cursor is, large, as a transport's display shows it: the grid position, and the
/// time under it.
fn readout(ui: &mut Ui, song: &Song) {
    egui::Frame::new()
        .fill(style::WELL)
        .corner_radius(7)
        .inner_margin(egui::Margin::symmetric(12, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let timing = song.timing();
                let at = timing.grid_position(song.cursor()).to_string();
                let big = RichText::new(&at)
                    .monospace()
                    .size(17.0)
                    .color(style::ACCENT);
                let cursor = ui.label(big);
                cursor.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Label,
                        true,
                        format!("Cursor at {at}"),
                    )
                });
                let time = style::clock(timing.time_at(song.cursor()).seconds());
                ui.label(
                    RichText::new(time)
                        .monospace()
                        .size(13.0)
                        .color(style::WEAK_TEXT),
                );
            });
        });
}

/// What changes rarely: whether the audio plays and how it stands, and whether the song can be
/// edited.
#[expect(
    clippy::non_ascii_literal,
    reason = "the status bar displays coloured circle and square markers"
)]
pub(crate) fn status_bar(
    ui: &mut Ui,
    song: Option<&Song>,
    (audio, playing): (&str, bool),
    empty_status: Option<&str>,
    actions: &mut Vec<Action>,
) {
    ui.spacing_mut().item_spacing.x = 10.0;
    let Some(song) = song else {
        ui.weak(empty_status.unwrap_or("No song open"));
        return;
    };
    egui::Sides::new().shrink_left().truncate().show(
        ui,
        |left| {
            let (colour, state) = if playing {
                (style::ACCENT, "Playing")
            } else {
                (style::WEAK_TEXT, "Paused")
            };
            left.label(RichText::new("●").color(colour).size(9.0));
            left.label(state);
            let layers = song.layers();
            let audio_rect = left.available_rect_before_wrap();
            // ponytail: split spare width with the layer; measure both names if this split
            // leaves too little of either visible in narrower windows.
            let audio_width = if layers.get(song.active_layer().get()).is_some() {
                audio_rect.width() * 0.5
            } else {
                audio_rect.width()
            };
            left.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(audio_rect.with_max_x(audio_rect.min.x + audio_width)),
                |audio_ui| audio_ui.add(egui::Label::new(RichText::new(audio).weak()).truncate()),
            )
            .inner
            .on_hover_text(audio);
            if let Some(row) = layers.get(song.active_layer().get()) {
                left.separator();
                let menu = left.menu_button(
                    (
                        egui::RichText::new("■").color(style::layer_colour(row.colour)),
                        row.name.as_str().atom_shrink(true),
                    ),
                    |menu_ui| {
                        for (index, layer) in layers.iter().enumerate() {
                            if menu_ui.button(&layer.name).clicked() {
                                actions.push(Action::SetActiveLayer(LayerIndex::new(index)));
                                menu_ui.close();
                            }
                        }
                    },
                );
                menu.response
                    .on_hover_text(format!("Active layer: {}; click to switch", row.name))
                    .widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, row.name.as_str())
                    });
            }
            let selected = song.selection().count();
            if selected > 0 {
                left.separator();
                let noun = if selected == 1 { "note" } else { "notes" };
                left.label(RichText::new(format!("{selected} {noun} selected")).monospace());
            }
        },
        |right| {
            if song.read_only().is_some() {
                right.weak("Read only");
                right.label(style::glyph(right, icon::LOCK_SIMPLE).color(style::WEAK_TEXT));
            } else {
                right.label("Editing");
                right.label(style::glyph(right, icon::PENCIL_SIMPLE).color(style::ACCENT));
            }
        },
    );
}

/// A snap's name, such as `16th` or `32nd`.
pub(crate) fn ordinal(n: NoteValue) -> String {
    let n = n.get();
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;

    #[test]
    fn long_audio_and_active_layer_keep_editing_status_on_screen() {
        let layer_name = "A very long active layer for percussion and extra overdubs";
        let audio_name = "audio-with-a-very-long-description-of-every-track-and-bonus-version.ogg";
        let mut song = Song::open(
            &charter_editor::demo::song().to_json().unwrap(),
            &[&charter_stepmania::Plugin],
        )
        .unwrap();
        song.rename_layer(LayerIndex::new(0), layer_name).unwrap();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(427.0, 180.0))
            .build_ui_state(
                |ui, song: &mut Song| {
                    status_bar(ui, Some(song), (audio_name, false), None, &mut Vec::new());
                },
                song,
            );
        harness.run();

        let editing = harness.query_by_label("Editing").map(|node| node.rect());
        assert!(
            editing.is_some_and(|rect| rect.left() >= 0.0 && rect.right() <= 427.0),
            "Editing must be painted in the 427-point window: {editing:?}"
        );
        let layer = harness.query_by_label(layer_name).map(|node| node.rect());
        assert!(
            layer.is_some_and(|rect| rect.right() <= editing.unwrap().left()),
            "Layer button should fit before Editing: {layer:?}, {editing:?}"
        );
        let audio = harness.query_by_label(audio_name).map(|node| node.rect());
        assert!(
            audio.is_some_and(|rect| rect.right() <= editing.unwrap().left()),
            "Audio label should fit before Editing: {audio:?}, {editing:?}"
        );
    }
}
