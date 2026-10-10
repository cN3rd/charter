//! What the user chooses about the editor, and the window where they choose it.

use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use charter_audio::{Gain, Kit, Volumes};
use charter_core::NoteValue;
use charter_editor::SNAPS;

use crate::flip::Direction;
use crate::notefield::{GRIDS, Zoom};
use crate::panels::ordinal;
use crate::style::{ACCENT_FILL, PANEL, TEXT, WEAK_TEXT, glyph, icon, quiet_icon, switch, titles};
use crate::window::Rate;
use egui::RichText;

/// A sound the editor can make while a song plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sound {
    // Older versions kept the note ticks off as a sound of their own.
    /// A short, dry note click.
    #[default]
    #[serde(alias = "off")]
    Click,
    /// A pitched electronic note.
    Beep,
    /// A sharp impact.
    Hit,
    /// A handclap.
    Clap,
    /// A quieter handclap.
    SoftClap,
    /// A bright finishing accent.
    Finish,
}

/// A latency calibration value, in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(from = "f64", into = "f64")]
pub struct LatencyMs(f64);

impl LatencyMs {
    /// Smallest latency, in milliseconds.
    pub(crate) const MIN: f64 = -500.0;
    /// Largest latency, in milliseconds.
    pub(crate) const MAX: f64 = 500.0;

    /// This latency in milliseconds.
    #[must_use]
    pub const fn ms(self) -> f64 {
        self.0
    }

    /// This latency as seconds at `rate`.
    #[must_use]
    pub fn seconds_at(self, rate: f64) -> f64 {
        self.0 * rate / 1000.0
    }
}

impl From<f64> for LatencyMs {
    fn from(ms: f64) -> Self {
        if ms.is_finite() {
            Self(ms.clamp(Self::MIN, Self::MAX))
        } else {
            Self::default()
        }
    }
}

impl From<LatencyMs> for f64 {
    fn from(latency: LatencyMs) -> Self {
        latency.ms()
    }
}

/// A notefield zoom as a percentage of its usual size.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "f32", into = "f32")]
pub struct ZoomPercent(f32);

impl ZoomPercent {
    /// Smallest zoom percentage shared with the notefield.
    pub(crate) const MIN: f32 = Zoom::PERCENT_RANGE.0;
    /// Largest zoom percentage shared with the notefield.
    pub(crate) const MAX: f32 = Zoom::PERCENT_RANGE.1;

    /// Zoom as a percentage.
    #[must_use]
    pub const fn percent(self) -> f32 {
        self.0
    }
}

impl Default for ZoomPercent {
    fn default() -> Self {
        Self(100.0)
    }
}

impl From<f32> for ZoomPercent {
    fn from(percent: f32) -> Self {
        if percent.is_finite() {
            Self(percent.clamp(Self::MIN, Self::MAX))
        } else {
            Self::default()
        }
    }
}

impl From<ZoomPercent> for f32 {
    fn from(zoom: ZoomPercent) -> Self {
        zoom.percent()
    }
}

/// The size of the editor window's interface, as a factor.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "f32", into = "f32")]
pub struct InterfaceScale(f32);

impl InterfaceScale {
    /// Smallest interface size the window can draw.
    const MIN: f32 = 0.5;
    /// Largest interface size the window can draw.
    const MAX: f32 = 3.0;

    /// Interface size as a factor.
    #[must_use]
    pub const fn factor(self) -> f32 {
        self.0
    }
}

impl Default for InterfaceScale {
    fn default() -> Self {
        Self(1.0)
    }
}

impl From<f32> for InterfaceScale {
    fn from(factor: f32) -> Self {
        if factor.is_finite() {
            Self(factor.clamp(Self::MIN, Self::MAX))
        } else {
            Self::default()
        }
    }
}

impl From<InterfaceScale> for f32 {
    fn from(scale: InterfaceScale) -> Self {
        scale.factor()
    }
}

/// The alternate playback speed, as a factor of normal speed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "f64", into = "f64")]
pub struct AlternateRate(f64);

impl AlternateRate {
    /// Slowest playback speed shared with the window.
    pub(crate) const MIN: f64 = Rate::SLOWEST;
    /// Fastest playback speed shared with the window.
    pub(crate) const MAX: f64 = Rate::FASTEST;

    /// Playback speed as a factor.
    #[must_use]
    pub const fn factor(self) -> f64 {
        self.0
    }
}

impl Default for AlternateRate {
    fn default() -> Self {
        Self(0.6)
    }
}

impl From<f64> for AlternateRate {
    fn from(factor: f64) -> Self {
        if factor.is_finite() {
            Self(factor.clamp(Self::MIN, Self::MAX))
        } else {
            Self::default()
        }
    }
}

impl From<AlternateRate> for f64 {
    fn from(rate: AlternateRate) -> Self {
        rate.factor()
    }
}

/// An integer setting kept between `MIN` and `MAX`, starting at `START`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "i128", into = "u32")]
pub struct BoundedU32<const MIN: u32, const MAX: u32, const START: u32>(u32);

impl<const MIN: u32, const MAX: u32, const START: u32> BoundedU32<MIN, MAX, START> {
    /// The lower bound.
    pub const MIN: u32 = MIN;
    /// The upper bound.
    pub const MAX: u32 = MAX;

    /// The bounded number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<const MIN: u32, const MAX: u32, const START: u32> Default for BoundedU32<MIN, MAX, START> {
    fn default() -> Self {
        Self::from(START)
    }
}

impl<const MIN: u32, const MAX: u32, const START: u32> From<u32> for BoundedU32<MIN, MAX, START> {
    fn from(value: u32) -> Self {
        Self(value.clamp(MIN, MAX))
    }
}

impl<const MIN: u32, const MAX: u32, const START: u32> From<i128> for BoundedU32<MIN, MAX, START> {
    fn from(value: i128) -> Self {
        let value = u32::try_from(value).unwrap_or(if value < 0 { MIN } else { MAX });
        Self::from(value)
    }
}

impl<const MIN: u32, const MAX: u32, const START: u32> From<BoundedU32<MIN, MAX, START>> for u32 {
    fn from(value: BoundedU32<MIN, MAX, START>) -> Self {
        value.get()
    }
}

/// Seconds between autosaves, from 5 to 600.
pub type AutosaveSeconds = BoundedU32<5, 600, 30>;
/// Minutes between snapshots, from 1 to 120.
pub type SnapshotMinutes = BoundedU32<1, 120, 5>;
/// Copies of a song to keep per folder, from 1 to 500.
pub type SnapshotsKept = BoundedU32<1, 500, 20>;
/// Milliseconds of audio before and after the cursor, from 50 to 1000.
pub type SnippetMs = BoundedU32<50, 1000, 200>;

impl From<SnapshotsKept> for NonZeroU32 {
    fn from(kept: SnapshotsKept) -> Self {
        Self::new(kept.get()).unwrap_or(Self::MIN)
    }
}

impl Sound {
    /// The note tick sounds offered in settings, in menu order.
    const ALL: [Self; 6] = [
        Self::Click,
        Self::Beep,
        Self::Hit,
        Self::Clap,
        Self::SoftClap,
        Self::Finish,
    ];

    /// What each note ticks with.
    pub(crate) const fn tick(self) -> charter_audio::Tick {
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

    /// The label used for this tick sound in the settings window.
    const fn name(self) -> &'static str {
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
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent choices, each its own switch"
)]
pub struct Settings {
    /// Whether the cursor moves on a snap once the lane keys that placed a note are up.
    pub auto_jump: bool,
    /// The snap a song opens on, where its resolution holds it.
    pub snap: NoteValue,
    /// Whether a BPM set on a timing point keeps the notes it times where they sound, rather
    /// than on their beats.
    pub notes_keep_time: bool,
    /// How close the notefield opens, as a percentage of its usual zoom.
    pub zoom: ZoomPercent,
    /// How large the whole window draws, as a factor.
    pub scale: InterfaceScale,
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
    pub autosave_seconds: AutosaveSeconds,
    /// Whether a timestamped copy of a changed song is kept in the snapshot folder.
    pub snapshots_in_folder: bool,
    /// Where those copies go, each song in a folder of its own; none for `snapshots` in charter's
    /// data folder.
    pub snapshot_folder: Option<std::path::PathBuf>,
    /// Whether a timestamped copy of a changed song is kept in an `autosaves` folder beside it,
    /// once it has been saved.
    pub snapshots_beside_song: bool,
    /// How often a changed song is copied, in minutes.
    pub snapshot_minutes: SnapshotMinutes,
    /// How many copies of a song each folder keeps, the oldest going first.
    pub snapshots_kept: SnapshotsKept,
    /// The songs opened or saved last, the latest first.
    pub recent: Vec<std::path::PathBuf>,
    /// Whether the song panel shows beside the notefield.
    pub song_panel: bool,
    /// Whether the notefield draws the audio's waveform beside the lanes.
    pub waveform: bool,
    /// Whether that strip shows how loud each pitch is instead of the waveform.
    pub frequencies: bool,
    /// Whether the whole chart shows in a strip beside the notefield, to jump across it.
    pub minimap: bool,
    /// Tint whole notes with their layer's colour instead of marking their leading edge.
    pub colour_notes_by_layer: bool,
    /// The note value of the notefield grid's finest lines, one of [`GRIDS`]: 4 draws the beats
    /// alone. Older versions kept a sixteenths switch here.
    #[serde(alias = "sixteenths", deserialize_with = "grid_or_switch")]
    pub grid: NoteValue,
    /// Whether the metronome clicks on every line of the timing window's grid, not only on the
    /// beats.
    pub metronome_grid: bool,
    /// The alternate playback speed, as a factor, that R switches to in the timing window.
    pub rate_b: AlternateRate,
    /// How much audio Q and W play before and after the cursor, in milliseconds.
    pub snippet_ms: SnippetMs,
    /// Whether audio played slower or faster than its speed keeps its pitch, from a tenth to
    /// twice its speed.
    pub keep_pitch: bool,
    /// How much later than it plays a sound is heard, in milliseconds, as the calibration's flash
    /// set it: the cursor takes it off the playhead to show what is heard.
    pub audio_latency_ms: LatencyMs,
    /// How much later than a sound is heard a tap along to it comes, in milliseconds: notes placed
    /// while playing go this much earlier than the cursor. Older versions kept the two latencies
    /// together here.
    #[serde(alias = "latency_ms")]
    pub input_latency_ms: LatencyMs,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_jump: true,
            snap: NoteValue::new(16).unwrap_or(NoteValue::QUARTER),
            notes_keep_time: true,
            zoom: ZoomPercent::default(),
            scale: InterfaceScale::default(),
            direction: Direction::Up,
            note_ticks: false,
            tick_sound: Sound::Click,
            tuned_lanes: false,
            metronome: false,
            metronome_sound: Kit::default(),
            volumes: Volumes::default(),
            autosave: true,
            autosave_seconds: AutosaveSeconds::default(),
            snapshots_in_folder: false,
            snapshot_folder: None,
            snapshots_beside_song: false,
            snapshot_minutes: SnapshotMinutes::default(),
            snapshots_kept: SnapshotsKept::default(),
            recent: Vec::new(),
            song_panel: true,
            waveform: true,
            frequencies: false,
            minimap: true,
            colour_notes_by_layer: false,
            grid: BEATS,
            metronome_grid: false,
            rate_b: AlternateRate::default(),
            snippet_ms: SnippetMs::default(),
            keep_pitch: true,
            audio_latency_ms: LatencyMs::default(),
            input_latency_ms: LatencyMs::default(),
        }
    }
}

/// What the settings window asks of the window around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// Play a measure of this metronome sound.
    Preview(Kit),
    /// Open the latency calibration.
    Calibrate,
    /// Ask the user for a folder to keep snapshots in.
    ChooseSnapshotFolder,
}

/// The grid of beats alone.
const BEATS: NoteValue = NoteValue::QUARTER;

/// The grid, read from a note value or, as older versions kept it, a sixteenths switch. A value
/// the notefield does not offer draws the beats alone.
fn grid_or_switch<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<NoteValue, D::Error> {
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
    Ok(NoteValue::new(grid)
        .filter(|value| GRIDS.contains(value))
        .unwrap_or(BEATS))
}

/// A grid's name in the settings and the toolbar.
pub(crate) fn grid_name(grid: NoteValue) -> String {
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

/// The interface sizes on offer, as factors.
/// Interface size factors offered in display settings.
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
        let note_ticks = mine.get("note_ticks").cloned();
        let mut changed: serde_json::Map<String, Value> = mine
            .into_iter()
            .filter(|(key, value)| defaults.get(key) != Some(value))
            .collect();
        // A tick sound kept alone reads as the ticks on, as older versions kept them, so the
        // switch is kept with it.
        if changed.contains_key("tick_sound")
            && let Some(on) = note_ticks
        {
            changed.insert("note_ticks".to_owned(), on);
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

    /// How long a changed song waits between snapshots, in seconds; none when snapshots are off.
    pub(crate) fn snapshot_after(&self) -> Option<f64> {
        (self.snapshots_in_folder || self.snapshots_beside_song)
            .then(|| f64::from(self.snapshot_minutes.get()) * 60.0)
    }

    /// How long unsaved changes wait before they are autosaved; none when autosave is off.
    pub(crate) fn autosave_after(&self) -> Option<f64> {
        self.autosave
            .then(|| f64::from(self.autosave_seconds.get()))
    }

    /// How much audio Q and W play, in seconds.
    pub(crate) fn snippet_seconds(&self) -> f64 {
        f64::from(self.snippet_ms.get()) / 1000.0
    }
}

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
                                    Page::Sounds => *asked_preview = sounds(ui, settings),
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
fn sounds(ui: &mut egui::Ui, settings: &mut Settings) -> Option<Asked> {
    note_ticks(ui, settings);
    sound_switches(ui, settings);
    let mut asked = None;
    metronome_sound(ui, settings, &mut asked);
    latency(ui, settings, &mut asked);
    playback_speed(ui, settings);
    snippet_length(ui, settings);
    volume_rows(ui, settings);
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
    use proptest::prelude::*;

    #[test]
    fn dragging_the_settings_windows_corner_resizes_it() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1000.0, 700.0))
            .build_ui_state(
                |ui, (settings, open): &mut (Settings, bool)| {
                    show(ui.ctx(), settings, open, None);
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
                    show(ui.ctx(), settings, open, None);
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
        assert!((settings.zoom.percent() - 100.0).abs() < f32::EPSILON);
        assert!((settings.scale.factor() - 1.0).abs() < f32::EPSILON);
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
        let settings = |seconds: u32| Settings {
            autosave_seconds: AutosaveSeconds::from(seconds),
            ..Settings::default()
        };
        assert_eq!(settings(0).autosave_after(), Some(5.0));
        assert_eq!(settings(100_000).autosave_after(), Some(600.0));
    }

    #[test]
    fn a_stored_interface_size_is_kept_to_what_the_window_can_draw() {
        let size = |scale: f32| {
            Settings {
                scale: scale.into(),
                ..Settings::default()
            }
            .scale
            .factor()
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
    fn snap_and_grid_note_values_keep_their_numeric_settings() {
        let settings = Settings {
            snap: NoteValue::new(24).unwrap(),
            grid: NoteValue::new(16).unwrap(),
            ..Settings::default()
        };
        let stored: Value = serde_json::from_str(&settings.to_kept()).unwrap();
        assert_eq!(stored, serde_json::json!({ "snap": 24, "grid": 16 }));
        let restored = Settings::from_kept(r#"{ "snap": 24, "grid": 16 }"#);
        assert_eq!(
            (restored.snap, restored.grid),
            (settings.snap, settings.grid)
        );
    }

    #[test]
    fn stored_settings_come_back_and_missing_ones_take_their_defaults() {
        let settings = Settings {
            auto_jump: false,
            snap: NoteValue::new(24).unwrap(),
            notes_keep_time: false,
            zoom: 150.0.into(),
            scale: 1.25.into(),
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
            autosave_seconds: 90_u32.into(),
            snapshots_in_folder: true,
            snapshot_folder: Some("/snapshots".into()),
            snapshots_beside_song: true,
            snapshot_minutes: 10_u32.into(),
            snapshots_kept: 5_u32.into(),
            recent: vec!["/songs/a.json".into()],
            song_panel: false,
            waveform: false,
            frequencies: true,
            minimap: false,
            colour_notes_by_layer: true,
            grid: NoteValue::new(24).unwrap(),
            metronome_grid: true,
            rate_b: 0.5.into(),
            snippet_ms: 120_u32.into(),
            keep_pitch: false,
            audio_latency_ms: 12.0.into(),
            input_latency_ms: 20.0.into(),
        };
        let stored = serde_json::to_string(&settings).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&stored).unwrap(), settings);
        let together: Settings = serde_json::from_str(r#"{ "latency_ms": 30.0 }"#).unwrap();
        assert!((together.input_latency_ms.ms() - 30.0).abs() < f64::EPSILON);
        assert!(together.audio_latency_ms.ms().abs() < f64::EPSILON);
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
    fn latency_newtypes_keep_the_old_settings_json_shape() {
        let kept = r#"{ "audio_latency_ms": 12.5, "input_latency_ms": -8.25 }"#;
        let settings = Settings::from_kept(kept);
        assert_eq!(settings.audio_latency_ms, LatencyMs::from(12.5));
        assert_eq!(settings.input_latency_ms, LatencyMs::from(-8.25));

        let stored: Value = serde_json::from_str(&settings.to_kept()).unwrap();
        assert_eq!(stored["audio_latency_ms"], 12.5);
        assert_eq!(stored["input_latency_ms"], -8.25);
    }

    #[test]
    fn stored_numbers_are_bounded_as_they_are_read() {
        let kept = r#"{
            "audio_latency_ms": 1000000000,
            "input_latency_ms": -1000000000,
            "zoom": 1000000000,
            "scale": -5,
            "rate_b": -5,
            "autosave_seconds": 1000000000,
            "snapshot_minutes": 0,
            "snapshots_kept": 0,
            "snippet_ms": 0
        }"#;
        for settings in [
            Settings::from_kept(kept),
            serde_json::from_str::<Settings>(kept).unwrap(),
        ] {
            let stored = serde_json::to_value(settings).unwrap();
            assert_eq!(stored["audio_latency_ms"], 500.0);
            assert_eq!(stored["input_latency_ms"], -500.0);
            assert_eq!(stored["zoom"], 1250.0);
            assert_eq!(stored["scale"], 0.5);
            assert_eq!(stored["rate_b"], 0.1);
            assert_eq!(stored["autosave_seconds"], 600);
            assert_eq!(stored["snapshot_minutes"], 1);
            assert_eq!(stored["snapshots_kept"], 1);
            assert_eq!(stored["snippet_ms"], 50);
        }

        let opposite = r#"{
            "zoom": -5, "rate_b": 1000000000, "autosave_seconds": -5,
            "snapshot_minutes": 1000000000, "snapshots_kept": -5,
            "snippet_ms": 1000000000
        }"#;
        let settings = Settings::from_kept(opposite);
        let stored = serde_json::to_value(settings).unwrap();
        assert_eq!(stored["zoom"], 12.5);
        assert_eq!(stored["rate_b"], 4.0);
        assert_eq!(stored["autosave_seconds"], 5);
        assert_eq!(stored["snapshot_minutes"], 120);
        assert_eq!(stored["snapshots_kept"], 1);
        assert_eq!(stored["snippet_ms"], 1000);
        let direct = serde_json::from_str::<Settings>(opposite).unwrap();
        assert_eq!(serde_json::to_value(direct).unwrap(), stored);
    }

    #[test]
    fn large_saved_integer_settings_reach_their_upper_bounds() {
        let kept = r#"{
            "autosave_seconds": 18446744073709551615,
            "snapshot_minutes": 18446744073709551615,
            "snapshots_kept": 18446744073709551615,
            "snippet_ms": 18446744073709551615
        }"#;
        let settings = Settings::from_kept(kept);
        let stored = serde_json::to_value(settings).unwrap();
        assert_eq!(stored["autosave_seconds"], 600);
        assert_eq!(stored["snapshot_minutes"], 120);
        assert_eq!(stored["snapshots_kept"], 500);
        assert_eq!(stored["snippet_ms"], 1000);
        let direct = serde_json::from_str::<Settings>(kept).unwrap();
        assert_eq!(serde_json::to_value(direct).unwrap(), stored);
    }

    #[test]
    fn nonfinite_numeric_choices_take_their_defaults() {
        let settings = Settings {
            zoom: f32::NAN.into(),
            scale: f32::INFINITY.into(),
            rate_b: f64::NEG_INFINITY.into(),
            ..Settings::default()
        };
        let stored = serde_json::to_value(settings).unwrap();
        assert_eq!(stored["zoom"], 100.0);
        assert_eq!(stored["scale"], 1.0);
        assert_eq!(stored["rate_b"], 0.6);
    }

    #[test]
    fn only_settings_changed_from_the_defaults_are_kept() {
        assert_eq!(Settings::default().to_kept(), "{}");
        let changed = Settings {
            metronome: true,
            zoom: 150.0.into(),
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
        assert!(
            (settings.zoom.percent() - Settings::default().zoom.percent()).abs() < f32::EPSILON
        );
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
        assert!((mixed.zoom.percent() - 100.0).abs() < f32::EPSILON);
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
                scale: value.into(),
                ..Settings::default()
            };
            assert!((settings.scale.factor() - expected).abs() < f32::EPSILON);
        }
        for (seconds, expected) in [(4_u32, 5.0), (5, 5.0), (600, 600.0), (601, 600.0)] {
            let settings = Settings {
                autosave_seconds: seconds.into(),
                ..Settings::default()
            };
            assert_eq!(settings.autosave_after(), Some(expected));
        }
    }

    fn settings_window() -> Harness<'static, (Settings, bool)> {
        let mut harness = Harness::builder().build_ui_state(
            |ui, (settings, open): &mut (Settings, bool)| {
                show(ui.ctx(), settings, open, None);
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
    fn snapshots_show_the_chosen_folder_and_default_restores_the_data_folder() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, (settings, open, asked): &mut (Settings, bool, Option<Asked>)| {
                *asked = show(
                    ui.ctx(),
                    settings,
                    open,
                    Some(std::path::Path::new("/data/charter")),
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
                *asked = show(ui.ctx(), settings, open, None).or(*asked);
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
                show(ui.ctx(), settings, open, None);
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
    fn the_snippet_length_is_kept_between_a_twentieth_and_a_second() {
        let length = |snippet_ms: u32| {
            Settings {
                snippet_ms: SnippetMs::from(snippet_ms),
                ..Settings::default()
            }
            .snippet_seconds()
        };
        assert!((length(200) - 0.2).abs() < f64::EPSILON);
        assert!((length(0) - 0.05).abs() < f64::EPSILON);
        assert!((length(u32::MAX) - 1.0).abs() < f64::EPSILON);
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

    proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn arbitrary_kept_settings_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let kept = String::from_utf8_lossy(&bytes);
            let settings = Settings::from_kept(&kept);
            prop_assert_eq!(Settings::from_kept(&settings.to_kept()), settings);
        }
    }
}
