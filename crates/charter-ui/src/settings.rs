//! What the user chooses about the editor, and the window where they choose it.

use std::num::NonZeroU16;

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    // Older versions kept the note ticks off as a sound of their own.
    #[default]
    #[serde(alias = "off")]
    Click,
    Beep,
    Hit,
    Clap,
    SoftClap,
    Finish,
}

impl Sound {
    const ALL: [Self; 6] = [
        Self::Click,
        Self::Beep,
        Self::Hit,
        Self::Clap,
        Self::SoftClap,
        Self::Finish,
    ];

    /// What each note ticks with.
    pub(crate) fn tick(self) -> charter_audio::Tick {
        use charter_audio::Tick;
        match self {
            Self::Click => Tick::Click,
            Self::Beep => Tick::Beep,
            Self::Hit => Tick::Hit,
            Self::Clap => Tick::Clap,
            Self::SoftClap => Tick::SoftClap,
            Self::Finish => Tick::Finish,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Click => "Click",
            Self::Beep => "Beep",
            Self::Hit => "Hit",
            Self::Clap => "Clap",
            Self::SoftClap => "Soft clap",
            Self::Finish => "Finish",
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
    /// Whether a tick sounds as each note passes the now line during playback.
    #[serde(deserialize_with = "on_or_sound")]
    pub note_ticks: bool,
    /// What that tick sounds like, kept while the ticks are off.
    pub tick_sound: Sound,
    /// Whether each lane's note ticks in a pitch of its own, lowest on the left.
    pub tuned_lanes: bool,
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
    /// Whether that strip shows how loud each pitch is instead of the waveform.
    pub frequencies: bool,
    /// Whether the whole chart shows in a strip beside the notefield, to jump across it.
    pub minimap: bool,
    /// The note value of the notefield grid's finest lines, one of [`GRIDS`]: 4 draws the beats
    /// alone. Older versions kept a sixteenths switch here.
    #[serde(alias = "sixteenths", deserialize_with = "grid_or_switch")]
    pub grid: NonZeroU16,
    /// Whether the metronome clicks on every line of the timing window's grid, not only on the
    /// beats.
    pub metronome_grid: bool,
    /// The alternate playback speed, as a factor, that R switches to in the timing window.
    pub rate_b: f64,
    /// Whether audio played slower or faster than its speed keeps its pitch, from a quarter to
    /// twice its speed.
    pub keep_pitch: bool,
    /// How much later than it plays a sound is heard, in milliseconds, as the calibration's flash
    /// set it: the cursor takes it off the playhead to show what is heard.
    pub audio_latency_ms: f64,
    /// How much later than a sound is heard a tap along to it comes, in milliseconds: notes placed
    /// while playing go this much earlier than the cursor. Older versions kept the two latencies
    /// together here.
    #[serde(alias = "latency_ms")]
    pub input_latency_ms: f64,
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
            note_ticks: false,
            tick_sound: Sound::Click,
            tuned_lanes: false,
            metronome: false,
            metronome_sound: Kit::default(),
            volumes: Volumes::default(),
            autosave: true,
            autosave_seconds: 30,
            song_panel: true,
            waveform: true,
            frequencies: false,
            minimap: true,
            grid: BEATS,
            metronome_grid: false,
            rate_b: 0.6,
            keep_pitch: true,
            audio_latency_ms: 0.0,
            input_latency_ms: 0.0,
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

/// Whether a sound is on, read from a switch or, as older versions kept it, the name of a
/// sound, `off` for none.
fn on_or_sound<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Kept {
        On(bool),
        Sound(String),
    }
    Ok(match Kept::deserialize(deserializer)? {
        Kept::On(on) => on,
        Kept::Sound(sound) => sound != "off",
    })
}

/// The shortest and longest autosave waits, in seconds.
const AUTOSAVE_SECONDS: (u32, u32) = (5, 600);

/// The interface sizes on offer, as factors.
const SCALES: [f32; 6] = [0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

impl Settings {
    /// The settings as the host keeps them between runs, as JSON: only those that differ from
    /// the defaults, so a setting left at its default follows the default when a later version
    /// changes it.
    #[must_use]
    pub fn to_kept(&self) -> String {
        let (Ok(Value::Object(mine)), Ok(Value::Object(defaults))) = (
            serde_json::to_value(self),
            serde_json::to_value(Self::default()),
        ) else {
            return "{}".to_owned();
        };
        let mut changed: serde_json::Map<String, Value> = mine
            .clone()
            .into_iter()
            .filter(|(key, value)| defaults.get(key) != Some(value))
            .collect();
        // A tick sound kept alone reads as the ticks on, as older versions kept them, so the
        // switch is kept with it.
        if changed.contains_key("tick_sound")
            && let Some(on) = mine.get("note_ticks")
        {
            changed.insert("note_ticks".to_owned(), on.clone());
        }
        Value::Object(changed).to_string()
    }

    /// The settings the host kept, from [`Settings::to_kept`] or an older whole copy: each kept
    /// setting that still reads, and the default for every other, so one setting a version can
    /// no longer read costs only itself.
    #[must_use]
    pub fn from_kept(kept: &str) -> Self {
        let Ok(Value::Object(mut kept)) = serde_json::from_str(kept) else {
            return Self::default();
        };
        // Older versions kept the ticks on as the sound they made.
        if !kept.contains_key("note_ticks")
            && let Some(Value::String(sound)) = kept.get("tick_sound")
        {
            let on = Value::Bool(sound != "off");
            kept.insert("note_ticks".to_owned(), on);
        }
        let mut taken = serde_json::Map::new();
        for (key, value) in kept {
            let mut trial = taken.clone();
            trial.insert(key.clone(), value.clone());
            if serde_json::from_value::<Self>(Value::Object(trial)).is_ok() {
                taken.insert(key, value);
            }
        }
        serde_json::from_value(Value::Object(taken)).unwrap_or_default()
    }

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
        360.0
    };
    let page_height = if compact {
        (height - 125.0).max(60.0)
    } else {
        (space.y - 150.0).max(120.0)
    };
    // Its own header, the title over the sidebar and a close button over the page, rather than
    // egui's title bar, which would sit flush against the edges the window draws to.
    egui::Window::new("Settings")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(0)))
        .fixed_size([width, height])
        .show(ctx, |ui| {
            let mut contents = |ui: &mut egui::Ui,
                                page: &mut Page,
                                preview: &mut Option<Asked>,
                                open: &mut bool| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(if compact { 16 } else { 24 }, 18))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
                            ui.set_width(if compact { width - 32.0 } else { PAGE_W });
                            if compact {
                                let caption = ui.weak("Page");
                                egui::ComboBox::from_id_salt("settings page")
                                    .selected_text(page.words().0)
                                    .show_ui(ui, |ui| {
                                        for choice in Page::ALL {
                                            ui.selectable_value(page, choice, choice.words().0);
                                        }
                                    })
                                    .response
                                    .labelled_by(caption.id);
                            }
                            let (_, _, title, about) = page.words();
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(title).family(titles(ui)).size(16.0));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| header_buttons(ui, settings, open),
                                );
                            });
                            ui.weak(about);
                            ui.add_space(14.0);
                            crate::style::solid_scroll_bars(ui);
                            egui::ScrollArea::vertical()
                                .max_height(page_height)
                                .show(ui, |ui| match page {
                                    Page::Editing => editing(ui, settings),
                                    Page::Display => display(ui, settings),
                                    Page::Sounds => *preview = sounds(ui, settings),
                                    Page::Autosave => autosave(ui, settings),
                                });
                        });
                    });
            };
            if compact {
                contents(ui, &mut page, &mut preview, open);
            } else {
                ui.horizontal_top(|ui| {
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
    let tip = "Every setting on every page back to how charter starts, but the latency \
        calibration, which measures this computer";
    if quiet_icon(ui, icon::ARROW_COUNTER_CLOCKWISE, "Restore defaults", tip).clicked() {
        *settings = Settings {
            audio_latency_ms: settings.audio_latency_ms,
            input_latency_ms: settings.input_latency_ms,
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
    let name = "Frequencies";
    row(
        ui,
        name,
        "The strip shows how loud each pitch is, low to high, instead of the waveform.",
        |ui, _| {
            switch(ui, &mut settings.frequencies, name);
        },
    );
    let name = "Minimap";
    row(
        ui,
        name,
        "The whole chart in a strip beside the notefield; click or drag it to jump there.",
        |ui, _| {
            switch(ui, &mut settings.minimap, name);
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
    let about = "The click, the beep, or a synthesised hit sound, public domain.";
    row(ui, "Note tick sound", about, |ui, caption| {
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
fn sounds(ui: &mut egui::Ui, settings: &mut Settings) -> Option<Asked> {
    note_ticks(ui, settings);
    let name = "Tuned lanes";
    let about = "Each lane ticks in its own pitch, lowest on the left, a whole tone apart and an \
        octave at most across. Shift+F4 turns it on and off.";
    row(ui, name, about, |ui, _| {
        switch(ui, &mut settings.tuned_lanes, name);
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
    let about = format!(
        "Audio {:+.0} ms, input {:+.0} ms. Align the flash to the click, then tap along.",
        settings.audio_latency_ms, settings.input_latency_ms
    );
    row(ui, "Latency", &about, |ui, _| {
        if ui.button("Calibrate…").clicked() {
            preview = Some(Asked::Calibrate);
        }
    });
    let name = "Metronome follows the grid";
    let about =
        "A click on every line of the timing window's grid, the ones between beats quieter.";
    row(ui, name, about, |ui, _| {
        switch(ui, &mut settings.metronome_grid, name);
    });
    let name = "Keep pitch at other speeds";
    let about = "Slowed or sped-up audio keeps its pitch from 25% to 200%; outside that, and with \
        this off, its pitch follows its speed.";
    row(ui, name, about, |ui, _| {
        switch(ui, &mut settings.keep_pitch, name);
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
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn settings_pages_and_controls_fit_a_narrow_window() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build_ui_state(
                |ui, (settings, open): &mut (Settings, bool)| {
                    show(ui.ctx(), settings, open);
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
        assert!(!settings.note_ticks);
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
            note_ticks: true,
            tick_sound: Sound::Clap,
            tuned_lanes: true,
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
            frequencies: true,
            minimap: false,
            grid: NonZeroU16::new(24).unwrap(),
            metronome_grid: true,
            rate_b: 0.5,
            keep_pitch: false,
            audio_latency_ms: 12.0,
            input_latency_ms: 20.0,
        };
        let stored = serde_json::to_string(&settings).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&stored).unwrap(), settings);
        let together: Settings = serde_json::from_str(r#"{ "latency_ms": 30.0 }"#).unwrap();
        assert!((together.input_latency_ms - 30.0).abs() < f64::EPSILON);
        assert!(together.audio_latency_ms.abs() < f64::EPSILON);
        let older: Settings = serde_json::from_str(r#"{ "auto_jump": false }"#).unwrap();
        assert_eq!(
            older,
            Settings {
                auto_jump: false,
                ..Settings::default()
            }
        );
    }

    #[test]
    fn only_settings_changed_from_the_defaults_are_kept() {
        assert_eq!(Settings::default().to_kept(), "{}");
        let changed = Settings {
            metronome: true,
            zoom: 150.0,
            ..Settings::default()
        };
        let kept = changed.to_kept();
        let value: Value = serde_json::from_str(&kept).unwrap();
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["metronome", "zoom"]);
        assert_eq!(Settings::from_kept(&kept), changed);
    }

    #[test]
    fn a_kept_setting_that_no_longer_reads_takes_its_default_and_leaves_the_rest() {
        let kept = r#"{ "metronome": true, "tick_sound": "trombone", "zoom": "wide" }"#;
        let settings = Settings::from_kept(kept);
        assert!(settings.metronome);
        assert_eq!(settings.tick_sound, Settings::default().tick_sound);
        assert!((settings.zoom - Settings::default().zoom).abs() < f32::EPSILON);
        assert_eq!(Settings::from_kept("not json"), Settings::default());
    }

    #[test]
    fn ticks_kept_as_a_sound_by_older_versions_read_as_on_or_off() {
        let on = Settings::from_kept(r#"{ "tick_sound": "beep" }"#);
        assert!(on.note_ticks);
        assert_eq!(on.tick_sound, Sound::Beep);
        let off = Settings::from_kept(r#"{ "tick_sound": "off" }"#);
        assert!(!off.note_ticks);
        assert_eq!(off.tick_sound, Sound::Click);
    }

    #[test]
    fn a_tick_sound_chosen_with_the_ticks_off_stays_off() {
        let chosen = Settings {
            tick_sound: Sound::Beep,
            ..Settings::default()
        };
        assert_eq!(Settings::from_kept(&chosen.to_kept()), chosen);
    }

    #[test]
    fn older_grid_and_sound_settings_migrate_without_losing_other_choices() {
        let cases = [
            (
                r#"{"sixteenths":true,"note_ticks":"beep","metronome":"off"}"#,
                16,
                true,
                false,
            ),
            (
                r#"{"sixteenths":false,"note_ticks":"off","metronome":"click"}"#,
                4,
                false,
                true,
            ),
            (
                r#"{"grid":0,"tick_sound":"off","zoom":175}"#,
                4,
                false,
                false,
            ),
            (
                r#"{"grid":7,"tick_sound":"hit","latency_ms":18}"#,
                4,
                true,
                false,
            ),
        ];
        for (kept, grid, ticks, metronome) in cases {
            let settings = Settings::from_kept(kept);
            assert_eq!(settings.grid.get(), grid, "{kept}");
            assert_eq!(settings.note_ticks, ticks, "{kept}");
            assert_eq!(settings.metronome, metronome, "{kept}");
        }
        let mixed = Settings::from_kept(r#"{"sixteenths":true,"zoom":"bad","auto_jump":false}"#);
        assert_eq!(mixed.grid.get(), 16);
        assert!(!mixed.auto_jump);
        assert!((mixed.zoom - 100.0).abs() < f32::EPSILON);
        assert_eq!(Settings::from_kept("[]"), Settings::default());
    }

    #[test]
    fn invalid_scale_and_autosave_waits_are_bounded_at_both_ends() {
        for (value, expected) in [
            (f32::NAN, 1.0),
            (f32::NEG_INFINITY, 1.0),
            (0.5, 0.5),
            (3.0, 3.0),
        ] {
            let settings = Settings {
                scale: value,
                ..Settings::default()
            };
            assert!((settings.scale() - expected).abs() < f32::EPSILON);
        }
        for (seconds, expected) in [(4, 5.0), (5, 5.0), (600, 600.0), (601, 600.0)] {
            let settings = Settings {
                autosave_seconds: seconds,
                ..Settings::default()
            };
            assert_eq!(settings.autosave_after(), Some(expected));
        }
    }

    fn settings_window() -> Harness<'static, (Settings, bool)> {
        let mut harness = Harness::builder().build_ui_state(
            |ui, (settings, open): &mut (Settings, bool)| {
                show(ui.ctx(), settings, open);
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
        assert!((harness.state().0.scale - 1.25).abs() < f32::EPSILON);
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
    fn sound_preview_and_calibration_buttons_ask_for_their_distinct_actions() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, (settings, open, asked): &mut (Settings, bool, Option<Asked>)| {
                *asked = show(ui.ctx(), settings, open).or(*asked);
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
                show(ui.ctx(), settings, open);
            },
            (
                Settings {
                    zoom: 175.0,
                    audio_latency_ms: 12.0,
                    input_latency_ms: -8.0,
                    ..Settings::default()
                },
                true,
            ),
        );
        harness.run();
        harness.get_by_label("Restore defaults").click();
        harness.run();
        assert!((harness.state().0.zoom - 100.0).abs() < f32::EPSILON);
        assert!((harness.state().0.audio_latency_ms - 12.0).abs() < f64::EPSILON);
        assert!((harness.state().0.input_latency_ms + 8.0).abs() < f64::EPSILON);
    }
}
