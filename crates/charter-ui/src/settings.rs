//! What the user chooses about the editor, and the window where they choose it.

use std::num::NonZeroU16;

use serde::{Deserialize, Serialize};

use charter_audio::{Gain, Kit, Volumes};
use charter_editor::SNAPS;

use crate::flip::Direction;
use crate::notefield::GRIDS;
use crate::panels::ordinal;
use crate::style::{ACCENT_FILL, PANEL, TEXT, WEAK_TEXT, glyph, icon, quiet_icon, switch, titles};
use egui::RichText;

/// A sound the editor can make while a song plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sound {
    #[default]
    Off,
    Click,
    Beep,
}

impl Sound {
    const ALL: [Self; 3] = [Self::Off, Self::Click, Self::Beep];

    /// The tone the sound plays, if any.
    pub(crate) fn tone(self) -> Option<charter_audio::Tone> {
        match self {
            Self::Off => None,
            Self::Click => Some(charter_audio::Tone::Click),
            Self::Beep => Some(charter_audio::Tone::Beep),
        }
    }

    /// Off when on, and a click when off.
    #[must_use]
    pub(crate) fn toggled(self) -> Self {
        match self {
            Self::Off => Self::Click,
            Self::Click | Self::Beep => Self::Off,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Click => "Click",
            Self::Beep => "Beep",
        }
    }
}

/// The user's choices, which the host keeps between runs. Choices missing from what the host
/// kept, such as ones newer than it, take their defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)] // independent choices, each its own switch
pub struct Settings {
    /// Whether the cursor moves on a snap once the lane keys that placed a note are up.
    pub auto_jump: bool,
    /// The snap a song opens on, where its resolution holds it.
    pub snap: NonZeroU16,
    /// Whether a BPM set on a timing point keeps the notes it times where they sound, rather
    /// than on their beats.
    pub notes_keep_time: bool,
    /// How close the notefield opens, as a percentage of its usual zoom.
    pub zoom: f32,
    /// How large the whole window draws, as a factor.
    pub scale: f32,
    /// Which way the notefield runs.
    pub direction: Direction,
    /// What sounds as each note passes the now line during playback.
    pub tick_sound: Sound,
    /// Whether the metronome sounds on each beat during playback.
    #[serde(deserialize_with = "on_or_sound")]
    pub metronome: bool,
    /// What the metronome sounds like.
    pub metronome_sound: Kit,
    /// How loud the music, the note ticks and the metronome play.
    pub volumes: Volumes,
    /// Whether unsaved changes are kept where the next run finds them.
    pub autosave: bool,
    /// How long unsaved changes wait before they are autosaved, in seconds.
    pub autosave_seconds: u32,
    /// Whether the song panel shows beside the notefield.
    pub song_panel: bool,
    /// Whether the notefield draws the audio's waveform beside the lanes.
    pub waveform: bool,
    /// The note value of the notefield grid's finest lines, one of [`GRIDS`]: 4 draws the beats
    /// alone. Older versions kept a sixteenths switch here.
    #[serde(alias = "sixteenths", deserialize_with = "grid_or_switch")]
    pub grid: NonZeroU16,
    /// Whether the metronome clicks on every line of the timing window's grid, not only on the
    /// beats.
    pub metronome_grid: bool,
    /// The alternate playback speed, as a factor, that R switches to in the timing window.
    pub rate_b: f64,
    /// The output and input latency the calibration measured, in milliseconds: what plays is
    /// heard this much later.
    pub latency_ms: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_jump: true,
            snap: NonZeroU16::new(16).unwrap_or(NonZeroU16::MIN),
            notes_keep_time: true,
            zoom: 100.0,
            scale: 1.0,
            direction: Direction::Up,
            tick_sound: Sound::Off,
            metronome: false,
            metronome_sound: Kit::default(),
            volumes: Volumes::default(),
            autosave: true,
            autosave_seconds: 30,
            song_panel: true,
            waveform: true,
            grid: BEATS,
            metronome_grid: false,
            rate_b: 0.6,
            latency_ms: 0.0,
        }
    }
}

/// What the settings window asks of the window around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    /// Play a measure of this metronome sound.
    Preview(Kit),
    /// Open the latency calibration.
    Calibrate,
}

/// The grid of beats alone.
const BEATS: NonZeroU16 = NonZeroU16::new(4).unwrap();

/// The grid, read from a note value or, as older versions kept it, a sixteenths switch. A value
/// the notefield does not offer draws the beats alone.
fn grid_or_switch<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<NonZeroU16, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Kept {
        Grid(u16),
        Sixteenths(bool),
    }
    let grid = match Kept::deserialize(deserializer)? {
        Kept::Grid(grid) => grid,
        Kept::Sixteenths(true) => 16,
        Kept::Sixteenths(false) => 4,
    };
    Ok(GRIDS
        .contains(&grid)
        .then(|| NonZeroU16::new(grid))
        .flatten()
        .unwrap_or(BEATS))
}

/// A grid's name in the settings and the toolbar.
pub(crate) fn grid_name(grid: NonZeroU16) -> String {
    if grid == BEATS {
        "Beats".to_owned()
    } else {
        format!("{} notes", ordinal(grid))
    }
}

/// Whether the metronome is on, read from a switch or, as older versions kept it, a sound.
fn on_or_sound<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Kept {
        On(bool),
        Sound(Sound),
    }
    Ok(match Kept::deserialize(deserializer)? {
        Kept::On(on) => on,
        Kept::Sound(sound) => sound != Sound::Off,
    })
}

/// The shortest and longest autosave waits, in seconds.
const AUTOSAVE_SECONDS: (u32, u32) = (5, 600);

/// The interface sizes on offer, as factors.
const SCALES: [f32; 6] = [0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

impl Settings {
    /// How long unsaved changes wait before they are autosaved, in seconds, kept to a range
    /// that neither writes constantly nor risks much; none when autosave is off.
    pub(crate) fn autosave_after(&self) -> Option<f64> {
        self.autosave.then(|| {
            f64::from(
                self.autosave_seconds
                    .clamp(AUTOSAVE_SECONDS.0, AUTOSAVE_SECONDS.1),
            )
        })
    }

    /// The interface size, kept to what the window can draw.
    pub(crate) fn scale(&self) -> f32 {
        if self.scale.is_finite() {
            self.scale.clamp(0.5, 3.0)
        } else {
            1.0
        }
    }
}

/// How wide the sidebar, a page and a setting's control are.
const SIDEBAR_W: f32 = 150.0;
const PAGE_W: f32 = 422.0;
const CONTROL_W: f32 = 200.0;

/// A page of the settings window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Editing,
    Display,
    Sounds,
    Autosave,
}

impl Page {
    const ALL: [Self; 4] = [Self::Editing, Self::Display, Self::Sounds, Self::Autosave];

    /// Its name in the sidebar, its icon, its title and what it covers.
    fn words(self) -> (&'static str, &'static str, &'static str, &'static str) {
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
pub fn show(ctx: &egui::Context, settings: &mut Settings, open: &mut bool) -> Option<Asked> {
    let page_id = egui::Id::new("settings page");
    let mut page: Page = ctx.data(|data| data.get_temp(page_id)).unwrap_or_default();
    if !*open {
        return None;
    }
    let mut preview = None;
    // Its own header, the title over the sidebar and a close button over the page, rather than
    // egui's title bar, which would sit flush against the edges the window draws to.
    egui::Window::new("Settings")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(0)))
        .fixed_size([620.0, 360.0])
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                sidebar(ui, &mut page);
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(24, 18))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
                            ui.set_width(PAGE_W);
                            let (_, _, title, about) = page.words();
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(title).family(titles(ui)).size(16.0));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let tip = "Close settings (Ctrl+Comma)";
                                        if quiet_icon(ui, icon::X, "Close settings", tip).clicked()
                                        {
                                            *open = false;
                                        }
                                    },
                                );
                            });
                            ui.weak(about);
                            ui.add_space(14.0);
                            match page {
                                Page::Editing => editing(ui, settings),
                                Page::Display => display(ui, settings),
                                Page::Sounds => preview = sounds(ui, settings),
                                Page::Autosave => autosave(ui, settings),
                            }
                        });
                    });
            });
        });
    ctx.data_mut(|data| data.insert_temp(page_id, page));
    preview
}

/// The pages, down the left of the window on a darker strip.
fn sidebar(ui: &mut egui::Ui, page: &mut Page) {
    egui::Frame::new()
        .fill(PANEL)
        .inner_margin(egui::Margin::symmetric(10, 14))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(SIDEBAR_W);
                ui.set_min_height(360.0);
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.add_space(4.0);
                ui.label(RichText::new("Settings").family(titles(ui)).size(16.0));
                ui.add_space(12.0);
                for choice in Page::ALL {
                    let (name, symbol, _, _) = choice.words();
                    let chosen = *page == choice;
                    let colour = if chosen { egui::Color32::WHITE } else { TEXT };
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
                    let name = name.to_owned();
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::SelectableLabel,
                            true,
                            chosen,
                            &name,
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
        let caption = ui
            .vertical(|ui| {
                ui.set_width(PAGE_W - CONTROL_W);
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
    let name = "Keep notes where they sound when a BPM changes";
    row(
        ui,
        name,
        "Off, the notes a changed BPM times stay on their beats and move with the grid.",
        |ui, _| {
            switch(ui, &mut settings.notes_keep_time, name);
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
                    for snap in SNAPS.into_iter().filter_map(NonZeroU16::new) {
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
            let zoom = egui::Slider::new(&mut settings.zoom, 25.0..=400.0).suffix("%");
            ui.add(zoom).labelled_by(caption);
        },
    );
}

fn display(ui: &mut egui::Ui, settings: &mut Settings) {
    row(
        ui,
        "Interface size",
        "How large the whole window draws.",
        |ui, caption| {
            egui::ComboBox::from_id_salt("scale")
                .selected_text(percent(settings.scale))
                .show_ui(ui, |ui| {
                    for scale in SCALES {
                        ui.selectable_value(&mut settings.scale, scale, percent(scale));
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

/// The sounds page. Says which metronome to preview, when its button was pressed.
fn sounds(ui: &mut egui::Ui, settings: &mut Settings) -> Option<Asked> {
    let about = "A tick on each note as it passes. F4 turns it on and off.";
    row(ui, "Note ticks", about, |ui, caption| {
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
    let about = "A tick on each beat, accented on each measure's first. F3 turns it on and off.";
    row(ui, "Metronome", about, |ui, _| {
        switch(ui, &mut settings.metronome, "Metronome");
    });
    let mut preview = None;
    let about = "Synthesised click pairs, public domain.";
    row(ui, "Metronome sound", about, |ui, caption| {
        let tip = "Play a measure of it";
        if quiet_icon(ui, icon::PLAY, "Preview the metronome", tip).clicked() {
            preview = Some(Asked::Preview(settings.metronome_sound));
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
    });
    let about = "How much later than it plays a sound is heard and answered. The playhead and \
        taps take it off.";
    row(ui, "Latency", about, |ui, _| {
        if ui.button("Calibrate…").clicked() {
            preview = Some(Asked::Calibrate);
        }
        ui.label(format!("{:+.0} ms", settings.latency_ms));
    });
    let name = "Metronome follows the grid";
    let about =
        "A click on every line of the timing window's grid, the ones between beats quieter.";
    row(ui, name, about, |ui, _| {
        switch(ui, &mut settings.metronome_grid, name);
    });
    let about = "The speed R switches to in the timing window, to hear flams slowed down.";
    row(ui, "Alternate speed", about, |ui, caption| {
        let mut percent = settings.rate_b * 100.0;
        let slider = egui::Slider::new(&mut percent, 25.0..=100.0)
            .suffix("%")
            .fixed_decimals(0);
        if ui.add(slider).labelled_by(caption).changed() {
            settings.rate_b = percent / 100.0;
        }
    });
    let volumes = &mut settings.volumes;
    for (name, about, gain) in [
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
        row(ui, name, about, |ui, caption| {
            volume(ui, gain).labelled_by(caption);
        });
    }
    preview
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

fn autosave(ui: &mut egui::Ui, settings: &mut Settings) {
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
            let wait = egui::DragValue::new(&mut settings.autosave_seconds)
                .range(AUTOSAVE_SECONDS.0..=AUTOSAVE_SECONDS.1)
                .suffix(" s");
            ui.add_enabled(settings.autosave, wait).labelled_by(caption);
        },
    );
}

/// A drop-down of the grids the notefield draws.
pub(crate) fn grid_choice(ui: &mut egui::Ui, grid: &mut NonZeroU16, id: &str) -> egui::Response {
    egui::ComboBox::from_id_salt(id)
        .selected_text(grid_name(*grid))
        .show_ui(ui, |ui| {
            for choice in GRIDS.into_iter().filter_map(NonZeroU16::new) {
                ui.selectable_value(grid, choice, grid_name(choice));
            }
        })
        .response
}

fn percent(factor: f32) -> String {
    format!("{:.0}%", factor * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_open_on_sixteenths_at_the_usual_zoom_moving_on_after_notes() {
        let settings = Settings::default();
        assert!(settings.auto_jump);
        assert!(settings.notes_keep_time);
        assert_eq!(settings.snap.get(), 16);
        assert!((settings.zoom - 100.0).abs() < f32::EPSILON);
        assert!((settings.scale - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn unsaved_changes_are_autosaved_after_half_a_minute_to_start_with() {
        let settings = Settings::default();
        assert!(settings.autosave);
        assert_eq!(settings.autosave_after(), Some(30.0));
        let off = Settings {
            autosave: false,
            ..Settings::default()
        };
        assert_eq!(off.autosave_after(), None);
    }

    #[test]
    fn an_autosave_wait_out_of_range_is_kept_within_it() {
        let settings = |seconds| Settings {
            autosave_seconds: seconds,
            ..Settings::default()
        };
        assert_eq!(settings(0).autosave_after(), Some(5.0));
        assert_eq!(settings(100_000).autosave_after(), Some(600.0));
    }

    #[test]
    fn a_stored_interface_size_is_kept_to_what_the_window_can_draw() {
        let size = |scale| {
            Settings {
                scale,
                ..Settings::default()
            }
            .scale()
        };
        assert!((size(f32::INFINITY) - 1.0).abs() < f32::EPSILON);
        assert!((size(0.1) - 0.5).abs() < f32::EPSILON);
        assert!((size(9.0) - 3.0).abs() < f32::EPSILON);
    }

    #[test]
    fn notes_travel_up_with_no_tick_or_metronome_sound_to_start_with() {
        let settings = Settings::default();
        assert_eq!(settings.direction, Direction::Up);
        assert_eq!(settings.tick_sound, Sound::Off);
        assert!(!settings.metronome);
        assert_eq!(settings.metronome_sound, Kit::ToneGate);
    }

    #[test]
    fn a_metronome_stored_as_a_sound_by_an_older_version_comes_back_on_or_off() {
        let on: Settings = serde_json::from_str(r#"{ "metronome": "beep" }"#).unwrap();
        assert!(on.metronome);
        let off: Settings = serde_json::from_str(r#"{ "metronome": "off" }"#).unwrap();
        assert!(!off.metronome);
    }

    #[test]
    fn a_sixteenths_switch_stored_by_an_older_version_comes_back_as_the_grid() {
        let on: Settings = serde_json::from_str(r#"{ "sixteenths": true }"#).unwrap();
        assert_eq!(on.grid.get(), 16);
        let off: Settings = serde_json::from_str(r#"{ "sixteenths": false }"#).unwrap();
        assert_eq!(off.grid.get(), 4);
    }

    #[test]
    fn the_grid_opens_on_beats_and_keeps_to_the_grids_it_offers() {
        assert_eq!(Settings::default().grid.get(), 4);
        let odd: Settings = serde_json::from_str(r#"{ "grid": 7 }"#).unwrap();
        assert_eq!(odd.grid.get(), 4);
    }

    #[test]
    fn stored_settings_come_back_and_missing_ones_take_their_defaults() {
        let settings = Settings {
            auto_jump: false,
            snap: NonZeroU16::new(24).unwrap(),
            notes_keep_time: false,
            zoom: 150.0,
            scale: 1.25,
            direction: Direction::Right,
            tick_sound: Sound::Click,
            metronome: true,
            metronome_sound: Kit::ShakerBurst,
            volumes: Volumes {
                music: Gain::from(0.5),
                ..Volumes::default()
            },
            autosave: false,
            autosave_seconds: 90,
            song_panel: false,
            waveform: false,
            grid: NonZeroU16::new(24).unwrap(),
            metronome_grid: true,
            rate_b: 0.5,
            latency_ms: 12.0,
        };
        let stored = serde_json::to_string(&settings).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&stored).unwrap(), settings);
        let older: Settings = serde_json::from_str(r#"{ "auto_jump": false }"#).unwrap();
        assert_eq!(
            older,
            Settings {
                auto_jump: false,
                ..Settings::default()
            }
        );
    }
}
