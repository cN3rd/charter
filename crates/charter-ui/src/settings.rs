//! What the user chooses about the editor, and the window where they choose it.

use std::num::NonZeroU16;

use serde::{Deserialize, Serialize};

use charter_editor::SNAPS;

use crate::flip::Direction;
use crate::panels::ordinal;
use crate::style::{ACCENT_FILL, PANEL, TEXT, WEAK_TEXT, glyph, icon, switch, titles};
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
pub struct Settings {
    /// Whether the cursor moves on a snap once the lane keys that placed a note are up.
    pub auto_jump: bool,
    /// The snap a song opens on, where its resolution holds it.
    pub snap: NonZeroU16,
    /// How close the notefield opens, as a percentage of its usual zoom.
    pub zoom: f32,
    /// How large the whole window draws, as a factor.
    pub scale: f32,
    /// Which way the notefield runs.
    pub direction: Direction,
    /// What sounds as each note passes the now line during playback.
    pub tick_sound: Sound,
    /// What sounds on each beat during playback.
    pub metronome: Sound,
    /// Whether unsaved changes are kept where the next run finds them.
    pub autosave: bool,
    /// How long unsaved changes wait before they are autosaved, in seconds.
    pub autosave_seconds: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_jump: true,
            snap: NonZeroU16::new(16).unwrap_or(NonZeroU16::MIN),
            zoom: 100.0,
            scale: 1.0,
            direction: Direction::Up,
            tick_sound: Sound::Off,
            metronome: Sound::Off,
            autosave: true,
            autosave_seconds: 30,
        }
    }
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
pub fn show(ctx: &egui::Context, settings: &mut Settings, open: &mut bool) {
    let page_id = egui::Id::new("settings page");
    let mut page: Page = ctx.data(|data| data.get_temp(page_id)).unwrap_or_default();
    egui::Window::new("Settings")
        .open(open)
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
                            ui.label(RichText::new(title).family(titles(ui)).size(16.0));
                            ui.weak(about);
                            ui.add_space(14.0);
                            match page {
                                Page::Editing => editing(ui, settings),
                                Page::Display => display(ui, settings),
                                Page::Sounds => sounds(ui, settings),
                                Page::Autosave => autosave(ui, settings),
                            }
                        });
                    });
            });
        });
    ctx.data_mut(|data| data.insert_temp(page_id, page));
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
}

fn sounds(ui: &mut egui::Ui, settings: &mut Settings) {
    for (name, about, id, sound) in [
        (
            "Note ticks",
            "A tick on each note as it passes. F4 turns it on and off.",
            "note ticks",
            &mut settings.tick_sound,
        ),
        (
            "Metronome",
            "A tick on each beat. F3 turns it on and off.",
            "metronome",
            &mut settings.metronome,
        ),
    ] {
        row(ui, name, about, |ui, caption| {
            egui::ComboBox::from_id_salt(id)
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
        assert_eq!(settings.metronome, Sound::Off);
    }

    #[test]
    fn stored_settings_come_back_and_missing_ones_take_their_defaults() {
        let settings = Settings {
            auto_jump: false,
            snap: NonZeroU16::new(24).unwrap(),
            zoom: 150.0,
            scale: 1.25,
            direction: Direction::Right,
            tick_sound: Sound::Click,
            metronome: Sound::Beep,
            autosave: false,
            autosave_seconds: 90,
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
