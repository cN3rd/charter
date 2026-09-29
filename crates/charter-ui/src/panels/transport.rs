use super::{
    Action, Align, Layout, NonZeroU16, Rate, RichText, Song, Tick, Ui, Zoom, egui, icon, style,
};

/// Draws the playback and editing controls, sending their choices as `actions`.
#[expect(clippy::too_many_lines, reason = "toolbar groups in display order")]
pub fn transport_bar(
    ui: &mut Ui,
    song: Option<&Song>,
    (zoom, rate, playing, panel, grid): (Zoom, Rate, bool, bool, NonZeroU16),
    (metronome, (ticks, tuned), volumes): (bool, (bool, bool), charter_audio::Volumes),
    actions: &mut Vec<Action>,
) {
    let width = ui.available_width();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (name, tip) = if panel {
            ("Hide the song panel", "Hide the song panel (Ctrl+\\)")
        } else {
            ("Show the song panel", "Show the song panel (Ctrl+\\)")
        };
        if style::tool(ui, icon::SIDEBAR_SIMPLE, name, tip, panel).clicked() {
            actions.push(Action::ToggleSongPanel);
        }
        ui.add_enabled_ui(song.is_some(), |ui| {
            style::chip(ui, |ui| {
                let start = Action::JumpTo(Tick::new(0));
                if style::tool(ui, icon::SKIP_BACK, "Jump to the start", "Home", false).clicked() {
                    actions.push(start);
                }
                let (symbol, playback_name) = if playing {
                    (icon::PAUSE, "Pause")
                } else {
                    (icon::PLAY, "Play")
                };
                let playback_tip = format!("{playback_name} from the cursor (Space)");
                if style::tool(ui, symbol, playback_name, &playback_tip, playing).clicked() {
                    actions.push(Action::TogglePlay);
                }
                if style::tool(ui, icon::SKIP_FORWARD, "Jump to the end", "End", false).clicked() {
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
                    ui.label(format!("Speed {:.0}%", rate.percent()));
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
                    ui.label(format!("Zoom {:.0}%", zoom.percent()));
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
                false,
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
                    ticks,
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
                    tuned,
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
                    metronome,
                )
                .clicked()
                {
                    actions.push(Action::ToggleMetronome);
                }
                let button = style::tool(ui, icon::SPEAKER_HIGH, "Volume", "Volume", false);
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
pub fn status_bar(
    ui: &mut Ui,
    song: Option<&Song>,
    (audio, playing): (&str, bool),
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let Some(song) = song else {
            ui.weak("No song open");
            return;
        };
        let (colour, state) = if playing {
            (style::ACCENT, "Playing")
        } else {
            (style::WEAK_TEXT, "Paused")
        };
        ui.label(RichText::new("●").color(colour).size(9.0));
        ui.label(state);
        ui.weak(audio);
        let layers = song.layers();
        if let Some(row) = layers.get(song.active_layer()) {
            ui.separator();
            let menu = ui.menu_button(
                (
                    egui::RichText::new("■").color(style::layer_colour(row.colour)),
                    row.name.as_str(),
                ),
                |ui| {
                    for (index, layer) in layers.iter().enumerate() {
                        if ui.button(&layer.name).clicked() {
                            actions.push(Action::SetActiveLayer(index));
                            ui.close();
                        }
                    }
                },
            );
            menu.response
                .on_hover_text("Active layer; click to switch")
                .widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, row.name.as_str())
                });
        }
        let selected = song.selection().count();
        if selected > 0 {
            ui.separator();
            let noun = if selected == 1 { "note" } else { "notes" };
            ui.label(format!("{selected} {noun} selected"));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if song.read_only().is_some() {
                ui.weak("Read only");
                ui.label(style::glyph(ui, icon::LOCK_SIMPLE).color(style::WEAK_TEXT));
            } else {
                ui.label("Editing");
                ui.label(style::glyph(ui, icon::PENCIL_SIMPLE).color(style::ACCENT));
            }
        });
    });
}

/// A snap's name, such as `16th` or `32nd`.
pub fn ordinal(n: NonZeroU16) -> String {
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
