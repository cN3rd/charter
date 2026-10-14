//! The settings window: the pages, rows and controls that edit the stored values.

#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "the settings page and the values it edits share one vocabulary"
    )
)]
use super::*;

/// How wide the sidebar is.
const SIDEBAR_W: f32 = 150.0;
/// Width of a settings row's control, in pixels.
const CONTROL_W: f32 = 200.0;

/// A page of the settings window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Page {
    /// Note placement and opening defaults.
    #[default]
    Editing,
    /// Interface size and notefield orientation.
    Display,
    /// Playback clicks and sound levels.
    Sounds,
    /// Autosave and snapshot intervals.
    Autosave,
}

impl Page {
    /// The pages in sidebar order.
    const ALL: [Self; 4] = [Self::Editing, Self::Display, Self::Sounds, Self::Autosave];

    /// Its name in the sidebar, its icon, its title and what it covers.
    const fn words(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Self::Editing => (
                "Editing",
                icon::PENCIL_SIMPLE,
                "Note entry and defaults",
                "How placing notes moves the cursor, and how songs open.",
            ),
            Self::Display => (
                "Display",
                icon::MONITOR,
                "Display and notefield",
                "How large the window draws, and which way the chart runs.",
            ),
            Self::Sounds => (
                "Sounds",
                icon::SPEAKER_HIGH,
                "Sounds while playing",
                "What plays over the song's audio.",
            ),
            Self::Autosave => (
                "Autosave",
                icon::FLOPPY_DISK,
                "Autosave and recovery",
                "Keeping unsaved changes safe from a crash.",
            ),
        }
    }
}

/// The settings window, while `open` is set: a sidebar of pages and the page chosen. Choices
/// take effect as they are made.
/// Draws the settings window while `open`, and says which metronome to preview, if one.
pub(crate) fn show(
    ctx: &egui::Context,
    settings: &mut Settings,
    open: &mut bool,
    data_folder: Option<&std::path::Path>,
    ffmpeg_status: &FfmpegStatus,
) -> Option<Asked> {
    let page_id = egui::Id::new("settings page");
    let mut page: Page = ctx.data(|data| data.get_temp(page_id)).unwrap_or_default();
    if !*open {
        return None;
    }
    let mut preview = None;
    let space = ctx.content_rect().size();
    let compact = space.x < 650.0;
    let width = if compact {
        (space.x - 24.0).max(260.0)
    } else {
        620.0
    };
    let height = if compact {
        (space.y - 24.0).max(180.0)
    } else {
        (space.y - 48.0).clamp(300.0, 640.0)
    };
    // Its own header, the title over the sidebar and a close button over the page, rather than
    // egui's title bar, which would sit flush against the edges the window draws to.
    egui::Window::new("Settings")
        .title_bar(false)
        .collapsible(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(0)))
        .default_size([width, height])
        .min_size(if compact {
            [260.0, 180.0]
        } else {
            [SIDEBAR_W + 300.0, 300.0]
        })
        .show(ctx, |window_ui| {
            let mut contents = |content_ui: &mut egui::Ui,
                                shown_page: &mut Page,
                                asked_preview: &mut Option<Asked>,
                                is_open: &mut bool| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(if compact { 16 } else { 24 }, 18))
                    .show(content_ui, |page_ui| {
                        page_ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
                            ui.set_width(ui.available_width());
                            if compact {
                                let caption = ui.weak("Page");
                                egui::ComboBox::from_id_salt("settings page")
                                    .selected_text(shown_page.words().0)
                                    .show_ui(ui, |ui| {
                                        for choice in Page::ALL {
                                            ui.selectable_value(
                                                shown_page,
                                                choice,
                                                choice.words().0,
                                            );
                                        }
                                    })
                                    .response
                                    .labelled_by(caption.id);
                            }
                            let (_, _, title, about) = shown_page.words();
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(title).family(titles(ui)).size(16.0));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| header_buttons(ui, settings, is_open),
                                );
                            });
                            ui.weak(about);
                            ui.add_space(14.0);
                            crate::style::solid_scroll_bars(ui);
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| match shown_page {
                                    Page::Editing => editing(ui, settings),
                                    Page::Display => display(ui, settings),
                                    Page::Sounds => {
                                        *asked_preview = sounds(ui, settings, ffmpeg_status);
                                    }
                                    Page::Autosave => {
                                        *asked_preview = autosave(ui, settings, data_folder);
                                    }
                                });
                        });
                    });
            };
            if compact {
                contents(window_ui, &mut page, &mut preview, open);
            } else {
                window_ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                    sidebar(ui, &mut page);
                    contents(ui, &mut page, &mut preview, open);
                });
            }
        });
    ctx.data_mut(|data| data.insert_temp(page_id, page));
    preview
}

/// The page header's buttons, right to left: close, then restore the defaults.
fn header_buttons(ui: &mut egui::Ui, settings: &mut Settings, open: &mut bool) {
    let tip = "Close settings (Ctrl+Comma)";
    if quiet_icon(ui, icon::X, "Close settings", tip).clicked() {
        *open = false;
    }
    let defaults_tip = "Every setting on every page back to how charter starts, but the latency \
        calibration, which measures this computer";
    if quiet_icon(
        ui,
        icon::ARROW_COUNTER_CLOCKWISE,
        "Restore defaults",
        defaults_tip,
    )
    .clicked()
    {
        // The calibration measures this computer, and the recent songs are history, not choices.
        *settings = Settings {
            audio_latency_ms: settings.audio_latency_ms,
            input_latency_ms: settings.input_latency_ms,
            recent: std::mem::take(&mut settings.recent),
            ffmpeg_offered: settings.ffmpeg_offered,
            ..Settings::default()
        };
    }
}

/// The pages, down the left of the window on a darker strip.
fn sidebar(ui: &mut egui::Ui, page: &mut Page) {
    egui::Frame::new()
        .fill(PANEL)
        .inner_margin(egui::Margin::symmetric(10, 14))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(SIDEBAR_W);
                ui.set_min_height(ui.available_height());
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.add_space(4.0);
                ui.label(RichText::new("Settings").family(titles(ui)).size(16.0));
                ui.add_space(12.0);
                for choice in Page::ALL {
                    let (name, symbol, _, _) = choice.words();
                    let chosen = *page == choice;
                    let colour = if chosen { ON_ACCENT } else { TEXT };
                    let item = egui::Button::new((
                        glyph(ui, symbol).color(colour),
                        RichText::new(name).color(colour),
                    ))
                    .fill(if chosen {
                        ACCENT_FILL
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .frame_when_inactive(chosen)
                    .min_size(egui::vec2(130.0, 30.0));
                    let response = ui.add(item);
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::SelectableLabel,
                            true,
                            chosen,
                            name,
                        )
                    });
                    if response.clicked() {
                        *page = choice;
                    }
                }
            });
        });
}

/// One setting: its name and what it does on the left, its control on the right, labelled by
/// the name.
fn row(ui: &mut egui::Ui, name: &str, about: &str, control: impl FnOnce(&mut egui::Ui, egui::Id)) {
    ui.horizontal(|ui| {
        let caption_width = (ui.available_width() - CONTROL_W).max(80.0);
        let caption = ui
            .vertical(|ui| {
                ui.set_width(caption_width);
                let caption = ui.label(name).id;
                ui.label(RichText::new(about).small().color(WEAK_TEXT));
                caption
            })
            .inner;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            control(ui, caption);
        });
    });
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(6.0);
}

/// Controls for placing notes and for opening a chart.
fn editing(ui: &mut egui::Ui, settings: &mut Settings) {
    let name = "Move on a snap after placing a note";
    row(
        ui,
        name,
        "Once the lane keys that placed it come up.",
        |ui, _| {
            switch(ui, &mut settings.auto_jump, name);
        },
    );
    let keep_time_name = "Keep notes where they sound when a BPM changes";
    row(
        ui,
        keep_time_name,
        "Off, the notes a changed BPM times stay on their beats and move with the grid.",
        |ui, _| {
            switch(ui, &mut settings.notes_keep_time, keep_time_name);
        },
    );
    row(
        ui,
        "Default snap",
        "The snap a song opens on.",
        |ui, caption| {
            egui::ComboBox::from_id_salt("snap")
                .selected_text(format!("{} snap", ordinal(settings.snap)))
                .show_ui(ui, |ui| {
                    for snap in SNAPS {
                        let text = format!("{} snap", ordinal(snap));
                        ui.selectable_value(&mut settings.snap, snap, text);
                    }
                })
                .response
                .labelled_by(caption);
        },
    );
    row(
        ui,
        "Default zoom",
        "How close the notefield opens.",
        |ui, caption| {
            let mut percent = settings.zoom.percent();
            let zoom =
                egui::Slider::new(&mut percent, ZoomPercent::MIN..=ZoomPercent::MAX).suffix("%");
            if ui.add(zoom).labelled_by(caption).changed() {
                settings.zoom = percent.into();
            }
        },
    );
}

/// Controls for the interface size and the notefield's appearance.
fn display(ui: &mut egui::Ui, settings: &mut Settings) {
    row(
        ui,
        "Interface size",
        "How large the whole window draws.",
        |ui, caption| {
            egui::ComboBox::from_id_salt("scale")
                .selected_text(percent(settings.scale.factor()))
                .show_ui(ui, |ui| {
                    for scale in SCALES {
                        ui.selectable_value(&mut settings.scale, scale.into(), percent(scale));
                    }
                })
                .response
                .labelled_by(caption);
        },
    );
    row(
        ui,
        "Notefield",
        "Which way notes travel past the now line.",
        |ui, caption| {
            egui::ComboBox::from_id_salt("direction")
                .selected_text(settings.direction.name())
                .width(210.0)
                .show_ui(ui, |ui| {
                    for direction in Direction::ALL {
                        ui.selectable_value(&mut settings.direction, direction, direction.name());
                    }
                })
                .response
                .labelled_by(caption);
        },
    );
    let name = "Waveform";
    row(
        ui,
        name,
        "The song's audio in a strip beside the lanes.",
        |ui, _| {
            switch(ui, &mut settings.waveform, name);
        },
    );
    let frequencies_name = "Frequencies";
    row(
        ui,
        frequencies_name,
        "The strip shows how loud each pitch is, low to high, instead of the waveform.",
        |ui, _| {
            switch(ui, &mut settings.frequencies, frequencies_name);
        },
    );
    let minimap_name = "Minimap";
    row(
        ui,
        minimap_name,
        "The whole chart in a strip beside the notefield; click or drag it to jump there.",
        |ui, _| {
            switch(ui, &mut settings.minimap, minimap_name);
        },
    );
    row(
        ui,
        "Grid lines",
        "Lines between the beats down to this note, each beat's brighter. The metronome keeps to \
        the beats.",
        |ui, caption| {
            grid_choice(ui, &mut settings.grid, "grid lines").labelled_by(caption);
        },
    );
}

/// The sounds page's note tick rows: whether they sound, and what with.
fn note_ticks(ui: &mut egui::Ui, settings: &mut Settings) {
    let about = "A tick on each note as it passes. F4 turns it on and off.";
    row(ui, "Note ticks", about, |ui, _| {
        switch(ui, &mut settings.note_ticks, "Note ticks");
    });
    let sound_about = "The click, the beep, or a synthesised hit sound, public domain.";
    row(ui, "Note tick sound", sound_about, |ui, caption| {
        let sound = &mut settings.tick_sound;
        egui::ComboBox::from_id_salt("note ticks")
            .selected_text(sound.name())
            .show_ui(ui, |ui| {
                for choice in Sound::ALL {
                    ui.selectable_value(sound, choice, choice.name());
                }
            })
            .response
            .labelled_by(caption);
    });
}

/// The sounds page. Says which metronome to preview, when its button was pressed.
fn sounds(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    ffmpeg_status: &FfmpegStatus,
) -> Option<Asked> {
    note_ticks(ui, settings);
    sound_switches(ui, settings);
    let mut asked = None;
    metronome_sound(ui, settings, &mut asked);
    latency(ui, settings, &mut asked);
    playback_speed(ui, settings);
    snippet_length(ui, settings);
    volume_rows(ui, settings);
    ui.add_space(12.0);
    let caption = ui.label("FFmpeg binary");
    ui.horizontal_wrapped(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut settings.ffmpeg_path)
                .hint_text("Use ffmpeg on the PATH")
                .desired_width(150.0),
        )
        .labelled_by(caption.id);
        if ui.button("Choose FFmpeg").clicked() {
            asked = Some(Asked::ChooseFfmpeg);
        }
        if ui
            .add_enabled(
                matches!(ffmpeg_status, FfmpegStatus::Missing),
                egui::Button::new("Install help"),
            )
            .clicked()
        {
            asked = Some(Asked::InstallFfmpeg);
        }
    });
    match ffmpeg_status {
        FfmpegStatus::Unknown => {
            ui.weak("Checking for FFmpeg.");
        }
        FfmpegStatus::Missing => {
            ui.weak("FFmpeg not found at the configured path or on the PATH.");
        }
        FfmpegStatus::Downloading(share) => {
            ui.weak(format!("Downloading ffmpeg, {:.0}%", share * 100.0));
        }
        FfmpegStatus::Found(path) => {
            ui.weak(format!("Found: {}", path.display()));
        }
    }
    asked
}

/// The sounds page's boolean rows.
fn sound_switches(ui: &mut egui::Ui, settings: &mut Settings) {
    let name = "Tuned lanes";
    let about = "Each lane ticks in its own pitch, lowest on the left, a whole tone apart and an \
        octave at most across. Shift+F4 turns it on and off.";
    row(ui, name, about, |ui, _| {
        switch(ui, &mut settings.tuned_lanes, name);
    });
    let metronome_about =
        "A tick on each beat, accented on each measure's first. F3 turns it on and off.";
    row(ui, "Metronome", metronome_about, |ui, _| {
        switch(ui, &mut settings.metronome, "Metronome");
    });
    let grid_name = "Metronome follows the grid";
    let grid_about =
        "A click on every line of the timing window's grid, the ones between beats quieter.";
    row(ui, grid_name, grid_about, |ui, _| {
        switch(ui, &mut settings.metronome_grid, grid_name);
    });
    let pitch_name = "Keep pitch at other speeds";
    let pitch_about = "Slowed or sped-up audio keeps its pitch from 10% to 200%; outside that, and with \
        this off, its pitch follows its speed.";
    row(ui, pitch_name, pitch_about, |ui, _| {
        switch(ui, &mut settings.keep_pitch, pitch_name);
    });
}

/// The sounds page's metronome sound row.
fn metronome_sound(ui: &mut egui::Ui, settings: &mut Settings, asked: &mut Option<Asked>) {
    let metronome_sound_about = "Synthesised click pairs, public domain.";
    row(
        ui,
        "Metronome sound",
        metronome_sound_about,
        |ui, caption| {
            let tip = "Play a measure of it";
            if quiet_icon(ui, icon::PLAY, "Preview the metronome", tip).clicked() {
                *asked = Some(Asked::Preview(settings.metronome_sound));
            }
            let kit = &mut settings.metronome_sound;
            egui::ComboBox::from_id_salt("metronome sound")
                .selected_text(kit.name())
                .height(f32::INFINITY)
                .show_ui(ui, |ui| {
                    for choice in Kit::ALL {
                        ui.selectable_value(kit, choice, choice.name());
                    }
                })
                .response
                .labelled_by(caption);
        },
    );
}

/// The sounds page's latency-calibration row.
#[expect(
    clippy::non_ascii_literal,
    reason = "the calibration button displays an ellipsis"
)]
fn latency(ui: &mut egui::Ui, settings: &Settings, asked: &mut Option<Asked>) {
    let latency_about = format!(
        "Audio {:+.0} ms, input {:+.0} ms. Align the flash to the click, then tap along.",
        settings.audio_latency_ms.ms(),
        settings.input_latency_ms.ms()
    );
    row(ui, "Latency", &latency_about, |ui, _| {
        if ui.button("Calibrate…").clicked() {
            *asked = Some(Asked::Calibrate);
        }
    });
}

/// The sounds page's alternate-rate row.
fn playback_speed(ui: &mut egui::Ui, settings: &mut Settings) {
    let speed_about = "The speed R switches to in the timing window, to hear flams slowed down.";
    row(ui, "Alternate speed", speed_about, |ui, caption| {
        let mut percent = settings.rate_b.factor() * 100.0;
        let slider = egui::Slider::new(
            &mut percent,
            (AlternateRate::MIN * 100.0)..=(AlternateRate::MAX * 100.0),
        )
        .suffix("%")
        .fixed_decimals(0);
        if ui.add(slider).labelled_by(caption).changed() {
            settings.rate_b = (percent / 100.0).into();
        }
    });
}

/// The sounds page's row for how much Q and W play.
fn snippet_length(ui: &mut egui::Ui, settings: &mut Settings) {
    let about = "How much Q and W play before and after the cursor.";
    row(ui, "Snippet length", about, |ui, caption| {
        let mut ms = settings.snippet_ms.get();
        let slider = egui::Slider::new(&mut ms, SnippetMs::MIN..=SnippetMs::MAX)
            .suffix(" ms")
            .step_by(10.0);
        if ui.add(slider).labelled_by(caption).changed() {
            settings.snippet_ms = ms.into();
        }
    });
}

/// The sounds page's volume rows.
fn volume_rows(ui: &mut egui::Ui, settings: &mut Settings) {
    let volumes = &mut settings.volumes;
    for (volume_name, volume_about, gain) in [
        ("Music volume", "The song's audio.", &mut volumes.music),
        (
            "Note tick volume",
            "The tick on each note.",
            &mut volumes.ticks,
        ),
        (
            "Metronome volume",
            "The metronome's clicks.",
            &mut volumes.metronome,
        ),
        (
            "Subdivision volume",
            "Clicks between beats, as a share of the metronome's.",
            &mut volumes.subdivision,
        ),
    ] {
        row(ui, volume_name, volume_about, |ui, caption| {
            volume(ui, gain).labelled_by(caption);
        });
    }
}

/// A slider setting `gain` in percent.
pub(crate) fn volume(ui: &mut egui::Ui, gain: &mut Gain) -> egui::Response {
    let mut percent = gain.amplitude() * 100.0;
    let slider = egui::Slider::new(&mut percent, 0.0..=100.0)
        .trailing_fill(true)
        .suffix("%")
        .fixed_decimals(0);
    let response = ui.add(slider);
    if response.changed() {
        *gain = Gain::from(percent / 100.0);
    }
    response
}

/// The autosave page: the crash-recovery file, then snapshots. Says when the user asked to choose
/// the snapshot folder. `data_folder` is charter's own, when the host has one.
fn autosave(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    data_folder: Option<&std::path::Path>,
) -> Option<Asked> {
    let name = "Autosave unsaved changes";
    row(
        ui,
        name,
        "The next start offers them back after a crash.",
        |ui, _| {
            switch(ui, &mut settings.autosave, name);
        },
    );
    row(
        ui,
        "Wait before saving",
        "How long changes sit before they are kept.",
        |ui, caption| {
            let mut seconds = settings.autosave_seconds.get();
            let wait = egui::DragValue::new(&mut seconds)
                .range(AutosaveSeconds::MIN..=AutosaveSeconds::MAX)
                .suffix(" s");
            if ui
                .add_enabled(settings.autosave, wait)
                .labelled_by(caption)
                .changed()
            {
                settings.autosave_seconds = seconds.into();
            }
        },
    );
    if let Some(folder) = data_folder {
        ui.weak(format!(
            "The recovery file and these settings are kept in {}.",
            folder.display()
        ));
    }
    ui.add_space(10.0);
    snapshots(ui, settings, data_folder)
}

/// The autosave page's snapshot rows: where timestamped copies of a changed song go, how often,
/// and how many are kept. Says when the user asked to choose the folder.
#[expect(
    clippy::non_ascii_literal,
    reason = "the snapshot-folder chooser displays an ellipsis"
)]
fn snapshots(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    data_folder: Option<&std::path::Path>,
) -> Option<Asked> {
    let mut asked = None;
    let name = "Snapshots in a folder";
    let about =
        "Every few minutes, a dated copy of a changed song, each song in a folder of its own.";
    row(ui, name, about, |ui, _| {
        switch(ui, &mut settings.snapshots_in_folder, name);
    });
    let shown = settings.snapshot_folder.as_deref().map_or_else(
        || {
            data_folder.map_or_else(
                || "charter's data folder".to_owned(),
                |data| data.join("snapshots").display().to_string(),
            )
        },
        |folder| folder.display().to_string(),
    );
    row(ui, "Snapshot folder", &shown, |ui, _| {
        let chooser = ui.add_enabled(settings.snapshots_in_folder, egui::Button::new("Choose…"));
        if chooser.clicked() {
            asked = Some(Asked::ChooseSnapshotFolder);
        }
        let custom = settings.snapshot_folder.is_some();
        if ui
            .add_enabled(custom, egui::Button::new("Default"))
            .clicked()
        {
            settings.snapshot_folder = None;
        }
    });
    let beside_name = "Snapshots beside the song";
    let beside_about =
        "The same copies in an autosaves folder beside the song's file, once it is saved.";
    row(ui, beside_name, beside_about, |ui, _| {
        switch(ui, &mut settings.snapshots_beside_song, beside_name);
    });
    let on = settings.snapshots_in_folder || settings.snapshots_beside_song;
    row(
        ui,
        "Snapshot every",
        "A song unchanged since its last copy is not copied again.",
        |ui, caption| {
            let mut minutes = settings.snapshot_minutes.get();
            let every = egui::DragValue::new(&mut minutes)
                .range(SnapshotMinutes::MIN..=SnapshotMinutes::MAX)
                .suffix(" min");
            if ui.add_enabled(on, every).labelled_by(caption).changed() {
                settings.snapshot_minutes = minutes.into();
            }
        },
    );
    row(
        ui,
        "Snapshots kept",
        "Each folder keeps this many of a song, the oldest going first.",
        |ui, caption| {
            let mut kept = settings.snapshots_kept.get();
            let control =
                egui::DragValue::new(&mut kept).range(SnapshotsKept::MIN..=SnapshotsKept::MAX);
            if ui.add_enabled(on, control).labelled_by(caption).changed() {
                settings.snapshots_kept = kept.into();
            }
        },
    );
    asked
}

/// A drop-down of the grids the notefield draws.
pub(crate) fn grid_choice(ui: &mut egui::Ui, grid: &mut NoteValue, id: &str) -> egui::Response {
    egui::ComboBox::from_id_salt(id)
        .selected_text(grid_name(*grid))
        .show_ui(ui, |ui| {
            for choice in GRIDS {
                ui.selectable_value(grid, choice, grid_name(choice));
            }
        })
        .response
}

/// `factor` displayed as a percentage rounded to a whole number.
fn percent(factor: f32) -> String {
    format!("{:.0}%", factor * 100.0)
}

#[cfg(test)]
mod tests {

    use super::*;
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn dragging_the_settings_windows_corner_resizes_it() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1000.0, 700.0))
            .build_ui_state(
                |ui, (settings, open): &mut (Settings, bool)| {
                    show(ui.ctx(), settings, open, None, &FfmpegStatus::default());
                },
                (Settings::default(), true),
            );
        harness.run();
        let area = |shown: &Harness<'_, (Settings, bool)>| {
            shown
                .ctx
                .memory(|memory| {
                    let window = memory
                        .layer_ids()
                        .find(|layer| layer.order == egui::Order::Middle)?;
                    memory.area_rect(window.id)
                })
                .unwrap()
        };
        let before = area(&harness);
        let corner = before.max - egui::vec2(2.0, 2.0);
        harness.hover_at(corner);
        harness.event(egui::Event::PointerButton {
            pos: corner,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run();
        let to = corner + egui::vec2(-120.0, -80.0);
        harness.hover_at(to);
        harness.event(egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run();
        let after = area(&harness);
        assert!(
            after.width() < before.width() - 60.0,
            "{before:?} {after:?}"
        );
        assert!(
            after.height() < before.height() - 40.0,
            "{before:?} {after:?}"
        );
    }

    #[test]
    fn settings_pages_and_controls_fit_a_narrow_window() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build_ui_state(
                |ui, (settings, open): &mut (Settings, bool)| {
                    show(ui.ctx(), settings, open, None, &FfmpegStatus::default());
                },
                (Settings::default(), true),
            );
        harness.run();
        harness.get_by_label("Page").click();
        harness.run();
        let display = harness.get_by_label("Display");
        assert!(display.rect().left() >= 0.0, "{:?}", display.rect());
        assert!(display.rect().right() <= 400.0, "{:?}", display.rect());
        display.click();
        harness.run();
        let size = harness.get_by_label("Interface size");
        assert!(size.rect().left() >= 0.0, "{:?}", size.rect());
        assert!(size.rect().right() <= 400.0, "{:?}", size.rect());
    }

    fn settings_window() -> Harness<'static, (Settings, bool)> {
        let mut harness = Harness::builder().build_ui_state(
            |ui, (settings, open): &mut (Settings, bool)| {
                show(ui.ctx(), settings, open, None, &FfmpegStatus::default());
            },
            (Settings::default(), true),
        );
        harness.run();
        harness
    }

    fn page(harness: &mut Harness<'_, (Settings, bool)>, name: &str) {
        harness.get_by_label(name).click();
        harness.run();
    }

    #[test]
    fn editing_and_display_controls_change_the_settings_they_name() {
        let mut harness = settings_window();
        harness
            .get_by_role_and_label(Role::CheckBox, "Move on a snap after placing a note")
            .click();
        harness.run();
        assert!(!harness.state().0.auto_jump);
        harness
            .get_by_role_and_label(
                Role::CheckBox,
                "Keep notes where they sound when a BPM changes",
            )
            .click();
        harness.run();
        assert!(!harness.state().0.notes_keep_time);
        harness.get_by_label("Default snap").click();
        harness.run();
        harness.get_by_label("24th snap").click();
        harness.run();
        assert_eq!(harness.state().0.snap.get(), 24);

        page(&mut harness, "Display");
        harness.get_by_label("Interface size").click();
        harness.run();
        harness.get_by_label("125%").click();
        harness.run();
        assert!((harness.state().0.scale.factor() - 1.25).abs() < f32::EPSILON);
        harness.get_by_label("Notefield").click();
        harness.run();
        harness
            .get_by_label("Horizontal, notes travel left")
            .click();
        harness.run();
        assert_eq!(harness.state().0.direction, Direction::Left);
        for (name, initial) in [
            ("Waveform", true),
            ("Frequencies", false),
            ("Minimap", true),
        ] {
            harness.get_by_role_and_label(Role::CheckBox, name).click();
            harness.run();
            let value = match name {
                "Waveform" => harness.state().0.waveform,
                "Frequencies" => harness.state().0.frequencies,
                _ => harness.state().0.minimap,
            };
            assert_ne!(value, initial, "{name}");
        }
        harness.get_by_label("Grid lines").scroll_to_me();
        harness.run();
        harness.get_by_label("Grid lines").click();
        harness.run();
        harness.get_by_label("16th notes").click();
        harness.run();
        assert_eq!(harness.state().0.grid.get(), 16);
    }

    #[test]
    fn sounds_and_autosave_pages_apply_their_switches_and_sound_choices() {
        let mut harness = settings_window();
        page(&mut harness, "Sounds");
        for (name, before) in [
            ("Note ticks", false),
            ("Tuned lanes", false),
            ("Metronome", false),
            ("Metronome follows the grid", false),
            ("Keep pitch at other speeds", true),
        ] {
            harness
                .get_by_role_and_label(Role::CheckBox, name)
                .scroll_to_me();
            harness.run();
            harness.get_by_role_and_label(Role::CheckBox, name).click();
            harness.run();
            let after = match name {
                "Note ticks" => harness.state().0.note_ticks,
                "Tuned lanes" => harness.state().0.tuned_lanes,
                "Metronome" => harness.state().0.metronome,
                "Metronome follows the grid" => harness.state().0.metronome_grid,
                _ => harness.state().0.keep_pitch,
            };
            assert_ne!(after, before, "{name}");
        }
        harness.get_by_label("Note tick sound").scroll_to_me();
        harness.run();
        harness.get_by_label("Note tick sound").click();
        harness.run();
        harness.get_by_label("Clap").click();
        harness.run();
        assert_eq!(harness.state().0.tick_sound, Sound::Clap);
        page(&mut harness, "Autosave");
        harness
            .get_by_role_and_label(Role::CheckBox, "Autosave unsaved changes")
            .click();
        harness.run();
        assert_eq!(harness.state().0.autosave_after(), None);
    }

    #[test]
    fn the_ffmpeg_binary_path_can_be_typed_in_settings() {
        let mut harness = settings_window();
        page(&mut harness, "Sounds");
        harness.get_by_label("FFmpeg binary").scroll_to_me();
        harness.run();
        let path = harness.get_by_role_and_label(Role::TextInput, "FFmpeg binary");
        path.focus();
        path.type_text("/tools/ffmpeg");
        harness.run();
        assert_eq!(harness.state().0.ffmpeg_path, "/tools/ffmpeg");
    }

    #[test]
    fn choosing_an_ffmpeg_binary_asks_the_host_for_a_file() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, (settings, open, asked): &mut (Settings, bool, Option<Asked>)| {
                *asked = show(ui.ctx(), settings, open, None, &FfmpegStatus::default()).or(*asked);
            },
            (Settings::default(), true, None),
        );
        harness.run();
        harness.get_by_label("Sounds").click();
        harness.run();
        harness.get_by_label("Choose FFmpeg").scroll_to_me();
        harness.run();
        harness.get_by_label("Choose FFmpeg").click();
        harness.run();
        assert_eq!(harness.state().2, Some(Asked::ChooseFfmpeg));
    }

    #[test]
    fn snapshots_show_the_chosen_folder_and_default_restores_the_data_folder() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, (settings, open, asked): &mut (Settings, bool, Option<Asked>)| {
                *asked = show(
                    ui.ctx(),
                    settings,
                    open,
                    Some(std::path::Path::new("/data/charter")),
                    &FfmpegStatus::default(),
                );
            },
            (Settings::default(), true, None),
        );
        harness.run();
        harness.get_by_label("Autosave").click();
        harness.run();
        assert!(harness.query_by_label("/data/charter/snapshots").is_some());
        harness.state_mut().0.snapshot_folder = Some("/music/copies".into());
        harness.run();
        assert!(harness.query_by_label("/music/copies").is_some());
        harness.get_by_label("Default").click();
        harness.run();
        assert_eq!(harness.state().0.snapshot_folder, None);
        assert!(harness.query_by_label("/data/charter/snapshots").is_some());
    }

    #[test]
    #[expect(
        clippy::non_ascii_literal,
        reason = "the test finds the calibration button by its displayed ellipsis"
    )]
    fn sound_preview_and_calibration_buttons_ask_for_their_distinct_actions() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, (settings, open, asked): &mut (Settings, bool, Option<Asked>)| {
                *asked = show(ui.ctx(), settings, open, None, &FfmpegStatus::default()).or(*asked);
            },
            (Settings::default(), true, None),
        );
        harness.run();
        harness.get_by_label("Sounds").click();
        harness.run();
        harness.get_by_label("Metronome sound").click();
        harness.run();
        harness.get_by_label("Shaker burst").click();
        harness.run();
        assert_eq!(harness.state().0.metronome_sound, Kit::ShakerBurst);
        harness.get_by_label("Preview the metronome").click();
        harness.run();
        assert_eq!(harness.state().2, Some(Asked::Preview(Kit::ShakerBurst)));
        harness.state_mut().2 = None;
        harness.get_by_label("Calibrate…").scroll_to_me();
        harness.run();
        harness.get_by_label("Calibrate…").click();
        harness.run();
        assert_eq!(harness.state().2, Some(Asked::Calibrate));
    }

    #[test]
    fn restoring_defaults_keeps_the_two_calibrations() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, (settings, open): &mut (Settings, bool)| {
                show(ui.ctx(), settings, open, None, &FfmpegStatus::default());
            },
            (
                Settings {
                    zoom: 175.0.into(),
                    audio_latency_ms: 12.0.into(),
                    input_latency_ms: (-8.0).into(),
                    ..Settings::default()
                },
                true,
            ),
        );
        harness.run();
        harness.get_by_label("Restore defaults").click();
        harness.run();
        assert!((harness.state().0.zoom.percent() - 100.0).abs() < f32::EPSILON);
        assert!((harness.state().0.audio_latency_ms.ms() - 12.0).abs() < f64::EPSILON);
        assert!((harness.state().0.input_latency_ms.ms() + 8.0).abs() < f64::EPSILON);
    }

    #[test]
    fn restoring_defaults_does_not_repeat_the_first_startup_ffmpeg_offer() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, (settings, open): &mut (Settings, bool)| {
                show(ui.ctx(), settings, open, None, &FfmpegStatus::default());
            },
            (
                Settings {
                    ffmpeg_path: "/tools/ffmpeg".into(),
                    ffmpeg_offered: true,
                    ..Settings::default()
                },
                true,
            ),
        );
        harness.run();
        harness.get_by_label("Restore defaults").click();
        harness.run();
        assert!(harness.state().0.ffmpeg_offered);
        assert!(harness.state().0.ffmpeg_path.is_empty());
    }

    #[test]
    fn zoom_speed_and_each_volume_slider_change_their_own_setting() {
        let mut harness = settings_window();
        harness.get_by_label("Default zoom").click();
        harness.run();
        assert!((harness.state().0.zoom.percent() - 100.0).abs() > 1.0);

        page(&mut harness, "Sounds");
        harness.get_by_label("Alternate speed").scroll_to_me();
        harness.run();
        harness.get_by_label("Alternate speed").click();
        harness.run();
        assert!((harness.state().0.rate_b.factor() - 0.6).abs() > 0.01);
        harness.get_by_label("Snippet length").scroll_to_me();
        harness.run();
        harness.get_by_label("Snippet length").click();
        harness.run();
        assert_ne!(harness.state().0.snippet_ms.get(), 200);
        harness.state_mut().0.volumes.subdivision = Gain::from(0.0);
        harness.run();
        for label in [
            "Music volume",
            "Note tick volume",
            "Metronome volume",
            "Subdivision volume",
        ] {
            let before = match label {
                "Music volume" => harness.state().0.volumes.music,
                "Note tick volume" => harness.state().0.volumes.ticks,
                "Metronome volume" => harness.state().0.volumes.metronome,
                _ => harness.state().0.volumes.subdivision,
            };
            harness.get_by_label(label).scroll_to_me();
            harness.run();
            harness.get_by_label(label).click();
            harness.run();
            let after = match label {
                "Music volume" => harness.state().0.volumes.music,
                "Note tick volume" => harness.state().0.volumes.ticks,
                "Metronome volume" => harness.state().0.volumes.metronome,
                _ => harness.state().0.volumes.subdivision,
            };
            assert_ne!(after, before, "{label}");
        }
    }

    #[test]
    fn autosave_wait_can_change_only_while_autosave_is_on() {
        let mut harness = settings_window();
        page(&mut harness, "Autosave");
        let wait = harness.get_by_label("Wait before saving");
        wait.focus();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        wait.type_text("12");
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert_eq!(harness.state().0.autosave_after(), Some(12.0));
        harness
            .get_by_role_and_label(Role::CheckBox, "Autosave unsaved changes")
            .click();
        harness.run();
        assert_eq!(harness.state().0.autosave_after(), None);
        assert_eq!(harness.state().0.autosave_seconds.get(), 12);
    }
}
