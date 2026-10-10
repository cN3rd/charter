//! The timing window: the song's audio laid out either by bars or by fixed spans of time, so a
//! correct map lines its beats up down the rows and a wrong one shows. Every BPM change is an
//! anchor, pinning a beat to a moment of the audio; the tempo between anchors follows from where
//! they are. The window measures the map against the onsets its host found, and the interaction
//! follows the Beatline specification: the verb is visible, Shift means finer, Ctrl switches from
//! the beat to the audio.
//!
//! The window reads a song and draws it; what the user asks of it comes back as [`Command`]s,
//! which its host carries out, since the host owns the song, its history and its playback. The
//! host says through [`TimingWindow::refused`] when an edit could not be made. Pointer,
//! keyboard and row painting internals live in private modules; the crate root keeps the public
//! surface and per-frame shared state.

mod accuracy;
mod draw;
pub mod grid;
mod keyboard;
mod panel;
mod pointer;
pub mod rows;

use charter_audio::{Onset, Peaks};
use charter_core::{Bpm, ResolvedTime, Tick, TimeShift, Timing, Valid};
use charter_editor::{Pin, Refusal, Song};
use charter_style as style;
use egui::Pos2;

use crate::accuracy::{Fitted, Issue, Residual};
use crate::grid::{Grid, RESOLUTIONS};
use crate::rows::{Bars, BarsPerRow, Layout, ROW, SecondsPerRow};

pub(crate) use crate::pointer::Gesture;

/// What the timing window asks its host to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Make an edit to the song's timing, as one step.
    Edit(Edit),
    /// Put the cursor on the tick and play from there, unless the song is playing already.
    PlayFrom(Tick),
    /// Play from this audio moment with the metronome on, to hear an edit.
    Audition(ResolvedTime),
    /// Play, or pause.
    TogglePlay,
    /// Loop the ticks from the first to the second with the metronome, or stop looping them
    /// when they loop already.
    Loop {
        /// First tick of the loop, inclusive.
        from: Tick,
        /// Tick where playback returns to the start.
        to: Tick,
    },
    /// Switch between the usual playback speed and the alternate one.
    ToggleRate,
    /// Turn the metronome on or off.
    ToggleMetronome,
    /// Mute the metronome while a key is held, or unmute it.
    MuteMetronome(bool),
}

/// An edit the timing window makes to a song's timing.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// Makes the beat on `tick` fall at this audio moment, anchoring it there.
    Sync {
        /// Beat to pin.
        tick: Tick,
        /// Moment in the audio.
        seconds: ResolvedTime,
    },
    /// Sets the tempo starting at this anchor, accounting for stops, delays and warps.
    SetBpmAt {
        /// Anchor beginning the segment.
        tick: Tick,
        /// Tempo to set on that segment.
        bpm: Bpm,
    },
    /// Removes the anchors on the ticks.
    Unsync(Vec<Tick>),
    /// Moves each anchor on a first tick to the beat on its second, keeping their moments.
    MoveAnchors(Vec<(Tick, Tick)>),
    /// Moves the anchors on the ticks by an audio span, merged into the step before when `merge`.
    Nudge {
        /// Anchors to shift in the audio.
        ticks: Vec<Tick>,
        /// Signed audio span.
        by: TimeShift,
        /// Whether to join the preceding nudge in undo history.
        merge: bool,
    },
    /// Sets the tempo after the last anchor.
    TailBpm {
        /// Tempo beyond the last anchor.
        bpm: Bpm,
        /// Whether to join the preceding adjustment in undo history.
        merge: bool,
    },
    /// Doubles, or halves, the tempo after each anchor on the ticks.
    Scale {
        /// Anchors whose following segments change tempo.
        ticks: Vec<Tick>,
        /// Double when true, halve otherwise.
        double: bool,
    },
    /// Makes these the anchors, with this tempo after the last, as a fit does.
    Repin {
        /// Replacement anchors, in tick order.
        pins: Vec<Pin>,
        /// Tempo beyond the last replacement anchor.
        tail: Bpm,
    },
}

impl Edit {
    /// Makes the edit to `song`, as one step.
    ///
    /// # Errors
    ///
    /// Whatever the song refuses the edit for, such as an anchor passing its neighbour.
    pub fn apply(&self, song: &mut Song) -> Result<(), Refusal> {
        match self {
            Self::Sync { tick, seconds } => song.sync(*tick, *seconds),
            Self::SetBpmAt { tick, bpm } => song.set_bpm_at(*tick, *bpm),
            Self::Unsync(ticks) => song.unsync_anchors(ticks),
            Self::MoveAnchors(moves) => song.move_anchors(moves),
            Self::Nudge { ticks, by, merge } => song.nudge_anchors(ticks, *by, *merge),
            Self::TailBpm { bpm, merge } => song.set_tail_bpm(*bpm, *merge),
            Self::Scale { ticks, double } => song.scale_tempo(ticks, *double),
            Self::Repin { pins, tail } => song.repin(pins, *tail, false),
        }
    }
}

/// How long nudges of the same anchors keep joining one step, in seconds.
const NUDGE_MERGE: f64 = 0.8;
/// How long a refused edit's blocking anchor pulses, in seconds: twice.
const PULSE: f64 = 0.8;
/// The shortest and longest lead-in an audition plays before the anchor, in seconds.
const AUDITION: (f64, f64) = (0.150, 0.600);
/// How much slower than the pointer a Shift-drag moves.
const FINE: f64 = 10.0;

/// The timing window: whether it shows, how it lays the song out, and what the user is doing in
/// it.
#[derive(Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "settings the user turns on and off individually"
)]
pub struct TimingWindow {
    /// Whether the timing window is visible.
    open: bool,
    /// Subdivision used for beat lines and snapping.
    grid: Grid,
    /// Whether the rows lay the song out by bar or by audio time.
    layout: Layout,
    /// Number of bars displayed per row in the bar layout.
    bars_per_row: BarsPerRow,
    /// How many seconds a row holds in the time layout.
    seconds_per_row: SecondsPerRow,
    /// Whether added and moved beats snap to the grid.
    snap_grid: bool,
    /// Whether moments snap to an onset within 25 ms.
    snap_onset: bool,
    /// How strong an onset has to be, from 0 to 1, to count.
    sensitivity: f32,
    /// Whether adding an anchor plays from a beat before it.
    audition: bool,
    /// Whether the rows follow the playhead while the song plays.
    follow: bool,
    /// Anchor under keyboard focus, by its tick.
    focus: Option<Tick>,
    /// The selected anchors, a run of neighbours, by tick.
    selection: Vec<Tick>,
    /// Active pointer operation in the timing rows.
    gesture: pointer::Gesture,
    /// The anchor an edit ran into, and when, to pulse it.
    pulse: Option<(Tick, f64)>,
    /// Why the last edit did not happen.
    status: Option<String>,
    /// A fit waiting on Enter or Esc, and what it does in words.
    preview: Option<(Fitted, String)>,
    /// The anchors last nudged, and when, so a run of nudges joins one step.
    last_nudge: Option<LastNudge>,
    /// The issue last gone to.
    issue: Option<usize>,
    /// Whether the window has the keyboard: after a click in it, until a click elsewhere.
    keys: bool,
    /// Whether the keys are listed.
    help: bool,
    /// Whether the metronome is muted while Shift+C is held.
    muting: bool,
    /// Inspector values being typed but not yet committed.
    drafts: panel::Drafts,
    /// Where the open context menu started, while the pointer moves over its buttons.
    menu_target: Option<(Option<Tick>, Pos2)>,
    /// A tick whose row to scroll to, as the second row.
    scroll_to: Option<Tick>,
    /// Measurements reused while timing, grid and onsets stay unchanged.
    cache: Option<Cache>,
}

impl Default for TimingWindow {
    fn default() -> Self {
        Self {
            open: false,
            grid: Grid::default(),
            layout: Layout::default(),
            bars_per_row: BarsPerRow::default(),
            seconds_per_row: SecondsPerRow::default(),
            snap_grid: true,
            snap_onset: true,
            sensitivity: 0.1,
            audition: true,
            follow: false,
            focus: None,
            selection: Vec::new(),
            gesture: pointer::Gesture::Idle,
            pulse: None,
            status: None,
            preview: None,
            last_nudge: None,
            issue: None,
            keys: true,
            help: false,
            muting: false,
            drafts: panel::Drafts::default(),
            menu_target: None,
            scroll_to: None,
            cache: None,
        }
    }
}

/// A run of related audio nudges that can merge into one undo step.
#[derive(Clone, Debug, PartialEq)]
struct LastNudge {
    /// Anchors shifted by the last nudge; empty means the tail tempo changed.
    ticks: Vec<Tick>,
    /// Frame timestamp when the nudge happened.
    at: f64,
}

/// Inputs that make cached timing measurements stale when any of them changes.
#[derive(Debug, PartialEq)]
struct CacheKey {
    /// Timing map the measurements were computed for.
    timing: Timing,
    /// Grid resolution the measurements were computed for.
    grid: Grid,
    /// Minimum onset strength the measurements were computed for.
    sensitivity: f32,
    /// Onsets the measurements were computed for.
    onsets: Box<[Onset]>,
}

/// What the rows are measured against, worked out again only when the timing, grid, onset
/// sensitivity or onsets change.
#[derive(Debug)]
struct Cache {
    /// Values that decide whether these measurements are still current.
    key: CacheKey,
    /// Anchors measured with this timing map.
    pins: Vec<Pin>,
    /// Audio deviations from the timing grid, in milliseconds.
    residuals: Vec<Residual>,
    /// Timing problems inferred from the residuals and anchors.
    issues: Vec<Issue>,
}

/// Everything a frame of the window works from.
pub(crate) struct Frame<'a> {
    /// Validated timing map for this frame.
    timing: Valid<'a, Timing>,
    /// Anchors of the current map, in tick order.
    pins: &'a [Pin],
    /// Tempo after the last anchor.
    tail: Bpm,
    /// Mapping between ticks and visible row positions.
    bars: Bars<'a>,
    /// Audio onsets to draw and snap against.
    onsets: &'a [Onset],
    /// Onset deviations from the grid, in milliseconds.
    residuals: &'a [Residual],
    /// Detected timing problems for the issue list.
    issues: &'a [Issue],
    /// Whether the song's plugin allows edits.
    editable: bool,
    /// Editor playhead position, in ticks.
    cursor: Tick,
    /// Frame timestamp, in seconds, for feedback and merged edits.
    now: f64,
}

impl Frame<'_> {
    /// Tempo in effect at `tick`, falling back to the tail after the last anchor.
    fn bpm_at(&self, tick: Tick) -> Bpm {
        self.timing.bpm_at(tick).unwrap_or(self.tail)
    }
}

/// What the host gives the window each frame.
#[derive(Clone, Copy, Debug)]
pub struct Inputs<'a> {
    /// Song open in the editor, if any.
    pub song: Option<&'a Song>,
    /// Audio envelope, when decoding has completed.
    pub peaks: Option<&'a Peaks>,
    /// Detected audio transients, sorted by time.
    pub onsets: &'a [Onset],
    /// Whether the audio is currently playing.
    pub playing: bool,
    /// The calibrated audio latency, in milliseconds, shown in the status line.
    pub latency_ms: f64,
}

impl TimingWindow {
    /// Whether the timing window is visible.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// Shows or hides the timing window.
    pub const fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    /// Subdivision used for beat lines and snapping.
    #[must_use]
    pub const fn grid(&self) -> Grid {
        self.grid
    }

    /// Sets the row layout.
    pub const fn set_layout(&mut self, layout: Layout) {
        self.layout = layout;
    }

    /// Whether the window takes the keyboard's commands, so the editor window leaves them.
    #[must_use]
    pub const fn has_keyboard(&self) -> bool {
        self.open && self.keys
    }

    /// Takes that `edit` was refused for `why`: the anchor it ran into pulses, and the status
    /// line says so.
    pub fn refused(&mut self, edit: &Edit, why: &Refusal, timing: Valid<'_, Timing>, now: f64) {
        let reason = match why {
            Refusal::Crosses(tick) => {
                self.pulse = Some((*tick, now));
                format!(
                    "Would cross the anchor at {}",
                    style::clock(timing.time_at(*tick).seconds())
                )
            }
            other @ (Refusal::ReadOnly
            | Refusal::NoChart
            | Refusal::Document(_)
            | Refusal::Game(_)
            | Refusal::OnlyTempo
            | Refusal::NotWhole
            | Refusal::FirstTempo
            | Refusal::Occupied
            | Refusal::OffTheChart
            | Refusal::LayerHidden(_)
            | Refusal::LayerGhosted(_)
            | Refusal::LayerLocked(_)
            | Refusal::OnLayer(_)
            | Refusal::LayerName(_)
            | Refusal::LastLayer) => {
                let first = other.to_string();
                let mut letters = first.chars();
                if let Some(letter) = letters.next() {
                    letter.to_uppercase().chain(letters).collect()
                } else {
                    first
                }
            }
        };
        if matches!(edit, Edit::Nudge { .. }) {
            self.last_nudge = None;
        }
        self.status = Some(reason);
    }

    /// Fits the focused anchor, or the selection, to the onsets, as a preview to apply or
    /// discard.
    fn fit(&mut self, frame: &Frame<'_>, half_bpm: bool) {
        let range = self.range(frame);
        let Some(range) = range else {
            self.status = Some("Focus or select anchors to fit".to_owned());
            return;
        };
        let at_tail = range.end().saturating_add(1) == frame.pins.len();
        let count = range.end().saturating_sub(*range.start()).saturating_add(1);
        let fitted = accuracy::fit(
            frame.timing,
            (frame.pins, frame.tail),
            range,
            frame.residuals,
            half_bpm,
        );
        match fitted {
            Some(fitted) => {
                let before = frame.tail;
                let about = if at_tail {
                    format!("Tail {before} → {} BPM", fitted.tail)
                } else {
                    format!("{count} anchors fitted")
                };
                self.preview = Some((fitted, about));
                self.status = None;
            }
            None => self.status = Some("No onsets to fit to there".to_owned()),
        }
    }

    /// The indexes among the anchors of the selection, or of the focused anchor alone.
    fn range(&self, frame: &Frame<'_>) -> Option<std::ops::RangeInclusive<usize>> {
        let index = |tick: Tick| frame.pins.iter().position(|pin| pin.tick == tick);
        match (self.selection.first(), self.selection.last()) {
            (Some(&first), Some(&last)) if self.selection.len() > 1 => {
                Some(index(first)?..=index(last)?)
            }
            _ => {
                let at = index(self.focus?)?;
                Some(at..=at)
            }
        }
    }

    /// The anchors a key or command acts on: the selection, or the focused one.
    fn targets(&self) -> Vec<Tick> {
        if self.selection.is_empty() {
            self.focus.into_iter().collect()
        } else {
            self.selection.clone()
        }
    }

    /// The ticks from the first selected anchor to the last, or from a lone selected anchor to the
    /// next.
    fn selected_span(&self, frame: &Frame<'_>) -> Option<(Tick, Tick)> {
        let (first, last) = (*self.selection.first()?, *self.selection.last()?);
        let to = if first == last {
            frame.pins.iter().find(|pin| pin.tick > first)?.tick
        } else {
            last
        };
        Some((first, to))
    }

    /// Focuses the issue at `at` and scrolls its anchor into view.
    fn go_to_issue(&mut self, frame: &Frame<'_>, at: usize) {
        let Some(issue) = frame.issues.get(at) else {
            return;
        };
        self.issue = Some(at);
        let tick = panel::issue_tick(issue);
        self.scroll_to = Some(tick);
        if frame.pins.iter().any(|pin| pin.tick == tick) {
            self.focus = Some(tick);
            self.selection = vec![tick];
        }
    }

    /// Selects and scrolls to the anchor at `tick`.
    fn focus_on(&mut self, tick: Tick) {
        self.focus = Some(tick);
        self.selection = vec![tick];
        self.scroll_to = Some(tick);
    }

    /// Selects every anchor between `a` and `b`, inclusive.
    fn select_between(&mut self, frame: &Frame<'_>, a: Tick, b: Tick) {
        let (low, high) = if a <= b { (a, b) } else { (b, a) };
        self.selection = frame
            .pins
            .iter()
            .map(|pin| pin.tick)
            .filter(|tick| *tick >= low && *tick <= high)
            .collect();
    }

    /// Nudges the targets `by` seconds, joining the step before when they were nudged a moment
    /// ago.
    fn nudge(&mut self, by: f64, now: f64, commands: &mut Vec<Command>) {
        let ticks = self.targets();
        if ticks.is_empty() {
            return;
        }
        let Some(by) = TimeShift::from_seconds(by) else {
            return;
        };
        let merge = self
            .last_nudge
            .as_ref()
            .is_some_and(|last| last.ticks == ticks && now - last.at < NUDGE_MERGE);
        self.last_nudge = Some(LastNudge {
            ticks: ticks.clone(),
            at: now,
        });
        commands.push(Command::Edit(Edit::Nudge { ticks, by, merge }));
    }

    /// An anchor at `seconds`, snapped to an onset and its beat to the grid as the toolbar says,
    /// unless `plain`, refused too close to another; played from a beat before when auditioning.
    fn add(&mut self, frame: &Frame<'_>, seconds: f64, plain: bool, commands: &mut Vec<Command>) {
        let seconds = if self.snap_onset && !plain {
            accuracy::snap(frame.onsets, self.sensitivity, seconds).unwrap_or(seconds)
        } else {
            seconds
        };
        let Some(at) = ResolvedTime::from_seconds(seconds) else {
            return;
        };
        let tick = frame.timing.latest_tick_by(at);
        let tick = if self.snap_grid {
            self.grid.nearest(frame.timing, tick)
        } else {
            tick
        };
        let half = self
            .grid
            .step(frame.timing.ticks_per_beat())
            .map_or(0, |step| i64::from(u32::from(step)).div_euclid(2));
        let close = frame
            .pins
            .iter()
            .find(|pin| tick.offset_to(pin.tick).get().abs() < half.max(1));
        if let Some(close) = close {
            self.pulse = Some((close.tick, frame.now));
            self.status = Some(format!(
                "Too close to the anchor at {}",
                style::clock(frame.timing.time_at(close.tick).seconds())
            ));
            return;
        }
        commands.push(Command::Edit(Edit::Sync { tick, seconds: at }));
        self.focus_on(tick);
        self.status = None;
        if self.audition
            && let Some(from) = audition_from(frame, at, tick)
        {
            commands.push(Command::Audition(from));
        }
    }
}

/// Audio moment up to a beat before `tick`, when it can be represented.
fn audition_from(frame: &Frame<'_>, at: ResolvedTime, tick: Tick) -> Option<ResolvedTime> {
    let beat = frame.timing.tempo_at(tick).seconds();
    ResolvedTime::from_seconds(at.seconds() - beat.clamp(AUDITION.0, AUDITION.1))
}

/// Draws the timing window over `inputs` while it is open, and answers its input with
/// `commands`.
pub fn show(
    ctx: &egui::Context,
    inputs: Inputs<'_>,
    window: &mut TimingWindow,
    commands: &mut Vec<Command>,
) {
    if !window.open {
        window.gesture = pointer::Gesture::Idle;
        return;
    }
    let now = ctx.input(|input| input.time);
    if window.pulse.is_some_and(|(_, at)| now - at > PULSE) {
        window.pulse = None;
    } else if window.pulse.is_some() {
        ctx.request_repaint();
    }
    let mut open = window.open;
    let shown = egui::Window::new("Timing")
        .open(&mut open)
        .default_size([1040.0, 640.0])
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .min_size([560.0, 300.0])
        .collapsible(false)
        .show(ctx, |ui| {
            let Some(song) = inputs.song else {
                ui.weak("Open a song to map its timing.");
                return;
            };
            let cache = refreshed(window.cache.take(), song, inputs.onsets, window);
            settle(window, &cache.pins);
            let frame = frame(song, inputs.onsets, (&cache, window.layout), now);
            egui::Panel::top("timing toolbar")
                .show(ui, |ui| panel::toolbar(ui, window, &frame, commands));
            let mut hovering = false;
            egui::Panel::bottom("timing status").show(ui, |ui| {
                panel::status(ui, window, &frame, (hovering, inputs.latency_ms));
            });
            // A width of its own, so the rows keep theirs as the inspector's numbers come and go.
            egui::Panel::right("timing inspector")
                .resizable(false)
                .exact_size(260.0)
                .show(ui, |ui| panel::inspector(ui, window, &frame, commands));
            egui::CentralPanel::default().show(ui, |ui| {
                if inputs.peaks.is_none() {
                    ui.colored_label(
                        style::WARN,
                        "The waveform shows once the song's audio loads.",
                    );
                }
                if window.help {
                    keyboard::help(ui);
                }
                hovering = draw::rows(ui, window, &frame, inputs, commands);
            });
            if window.keys && !ui.ctx().egui_wants_keyboard_input() {
                keyboard::keyboard(ui.ctx(), window, &frame, commands);
            }
            window.cache = Some(cache);
        });
    window.open = open;
    if let Some(shown) = shown {
        let rect = shown.response.rect;
        let pressed = ctx.input(|input| {
            input
                .pointer
                .any_pressed()
                .then(|| input.pointer.press_origin())
                .flatten()
        });
        if let Some(at) = pressed {
            window.keys = rect.contains(at);
        }
    }
}

/// `cache`, or what the rows are measured against worked out again when it is of another
/// timing, grid or set of onsets.
fn refreshed(cache: Option<Cache>, song: &Song, onsets: &[Onset], window: &TimingWindow) -> Cache {
    let timing = song.timing();
    let fresh = cache.filter(|cache| {
        cache.key.timing == *timing
            && cache.key.grid == window.grid
            && (cache.key.sensitivity - window.sensitivity).abs() <= f32::EPSILON
            && cache.key.onsets.as_ref() == onsets
    });
    fresh.unwrap_or_else(|| {
        let pins = song.anchors();
        let residuals = accuracy::residuals(timing, window.grid, onsets, window.sensitivity);
        let issues = accuracy::issues(timing, &pins, &residuals, (onsets, window.sensitivity));
        Cache {
            key: CacheKey {
                timing: (*timing).clone(),
                grid: window.grid,
                sensitivity: window.sensitivity,
                onsets: onsets.into(),
            },
            pins,
            residuals,
            issues,
        }
    })
}

/// Keeps the focus and selection on anchors that are there, after edits moved or removed some.
fn settle(window: &mut TimingWindow, pins: &[Pin]) {
    window
        .selection
        .retain(|tick| pins.iter().any(|pin| pin.tick == *tick));
    if window
        .focus
        .is_some_and(|tick| !pins.iter().any(|pin| pin.tick == tick))
    {
        window.focus = window.selection.first().copied();
    }
}

/// The frame's context: the timing as it stands, its anchors, and what it is measured against.
fn frame<'a>(
    song: &'a Song,
    onsets: &'a [Onset],
    (cache, layout): (&'a Cache, Layout),
    now: f64,
) -> Frame<'a> {
    let timing = song.timing();
    let first = ResolvedTime::from_seconds(0.0)
        .map_or(Tick::new(0), |zero| timing.latest_tick_by(zero))
        .min(Tick::new(0));
    let mut last = song.end().max(song.cursor());
    if let Some(anchor) = timing.anchors.last() {
        last = last.max(anchor.tick);
    }
    if let Some(event) = timing.events.last() {
        last = last.max(event.tick);
    }
    Frame {
        timing,
        tail: song.tail_bpm(),
        pins: &cache.pins,
        bars: Bars::new(timing, first, last, 4).laid_out(layout),
        onsets,
        residuals: &cache.residuals,
        issues: &cache.issues,
        editable: song.read_only().is_none(),
        cursor: song.cursor(),
        now,
    }
}

/// Deterministic property tests with no regression files.
#[cfg(test)]
fn proptest_config() -> proptest::test_runner::Config {
    proptest::test_runner::Config {
        failure_persistence: None,
        rng_seed: proptest::test_runner::RngSeed::Fixed(0),
        ..proptest::test_runner::Config::default()
    }
}

#[cfg(test)]
pub(crate) mod test_support {

    use charter_core::{RulesetPlugin, ValidDocument};
    use egui::{Modifiers, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use serde_json::json;

    use crate::*;

    #[derive(Debug)]
    struct TestRuleset;

    #[expect(
        clippy::missing_trait_methods,
        reason = "a test plugin: the trait's defaults for every optional method"
    )]
    impl RulesetPlugin for TestRuleset {
        fn id(&self) -> &'static str {
            "test"
        }

        fn check(&self, _: &ValidDocument) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    pub(crate) struct WindowTest {
        pub(super) song: Song,
        pub(super) window: TimingWindow,
        pub(super) commands: Vec<Command>,
        pub(super) onsets: Vec<Onset>,
    }

    impl WindowTest {
        pub(crate) fn ui(&mut self, ui: &egui::Ui) {
            show(
                ui.ctx(),
                Inputs {
                    song: Some(&self.song),
                    peaks: None,
                    onsets: &self.onsets,
                    playing: false,
                    latency_ms: 0.0,
                },
                &mut self.window,
                &mut self.commands,
            );
        }
    }

    pub(crate) fn timing_harness() -> Harness<'static, WindowTest> {
        let value = json!({
            "format_version": 1,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "a.ogg" },
            "timing": {
                "anchors": [
                    { "tick": 0, "seconds": 0.0 },
                    { "tick": 960, "seconds": 0.5 },
                    { "tick": 1920, "seconds": 1.0 }
                ],
                "tempo": 120.0,
                "events": []
            }
        });
        let document = ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        let state = WindowTest {
            song: Song::new(document, &[&TestRuleset]),
            window: TimingWindow {
                open: true,
                layout: Layout::Bars,
                ..TimingWindow::default()
            },
            commands: Vec::new(),
            onsets: Vec::new(),
        };
        let mut harness = Harness::builder()
            .with_size(vec2(1280.0, 800.0))
            .build_ui_state(|ui, state: &mut WindowTest| state.ui(ui), state);
        harness.run();
        harness
    }

    pub(crate) fn row_point(harness: &Harness<'_, WindowTest>, tick: i32) -> Pos2 {
        let rect = harness.get_by_label("Measure 0").rect();
        let fraction = f64::from(tick) / 3840.0;
        let across = rows::float_to_f32((fraction + rows::LEAD_IN) / (1.0 + rows::LEAD_IN));
        egui::pos2(rect.left() + across * rect.width(), rect.top() + 30.0)
    }

    pub(crate) fn pointer(
        harness: &Harness<'_, WindowTest>,
        pos: Pos2,
        pressed: bool,
        modifiers: Modifiers,
    ) {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        });
    }

    pub(super) fn click(harness: &mut Harness<'_, WindowTest>, pos: Pos2, modifiers: Modifiers) {
        harness.hover_at(pos);
        harness.event(egui::Event::ModifiersChanged(modifiers));
        pointer(harness, pos, true, modifiers);
        pointer(harness, pos, false, modifiers);
        harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
        harness.step();
    }

    pub(crate) fn drag(
        harness: &mut Harness<'_, WindowTest>,
        from: Pos2,
        to: Pos2,
        modifiers: Modifiers,
    ) {
        harness.hover_at(from);
        harness.event(egui::Event::ModifiersChanged(modifiers));
        pointer(harness, from, true, modifiers);
        for step in 1..=4 {
            harness.hover_at(from.lerp(to, rows::float_to_f32(f64::from(step)) / 4.0));
        }
        pointer(harness, to, false, modifiers);
        harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
        harness.step();
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        unused_imports,
        reason = "shared imports keep moved tests close to their original form"
    )]

    use charter_audio::Onset;
    use charter_core::{Bpm, Tick, ValidDocument};
    use charter_editor::{Pin, Song};
    use egui::{Key, Modifiers, Pos2, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use serde_json::json;

    use super::*;
    use crate::test_support::*;
    use crate::*;

    #[test]
    fn nudges_merge_for_the_same_selection_until_a_refusal_resets_them() {
        let mut harness = timing_harness();
        let by = TimeShift::from_seconds(0.001).unwrap();
        let at = row_point(&harness, 960);
        click(&mut harness, at, Modifiers::NONE);
        harness.key_press(Key::ArrowRight);
        harness.step();
        harness.key_press(Key::ArrowRight);
        harness.step();
        assert_eq!(
            harness.state().commands,
            [
                Command::Edit(Edit::Nudge {
                    ticks: vec![Tick::new(960)],
                    by,
                    merge: false
                }),
                Command::Edit(Edit::Nudge {
                    ticks: vec![Tick::new(960)],
                    by,
                    merge: true
                })
            ]
        );
        let edit = Edit::Nudge {
            ticks: vec![Tick::new(960)],
            by,
            merge: true,
        };
        let state = harness.state_mut();
        let timing = state.song.timing();
        let now = state.window.last_nudge.as_ref().unwrap().at;
        state
            .window
            .refused(&edit, &Refusal::Crosses(Tick::new(1920)), timing, now);
        harness.key_press(Key::ArrowRight);
        harness.step();
        assert_eq!(
            harness.state().commands.last(),
            Some(&Command::Edit(Edit::Nudge {
                ticks: vec![Tick::new(960)],
                by,
                merge: false
            }))
        );
        assert_eq!(harness.state().window.pulse, Some((Tick::new(1920), now)));
        assert!(
            harness
                .query_by_label_contains("Would cross the anchor")
                .is_some()
        );
    }

    #[test]
    fn a_missing_onset_for_a_fit_explains_why_no_edit_was_made() {
        let mut harness = timing_harness();
        let at = row_point(&harness, 960);
        click(&mut harness, at, Modifiers::NONE);
        harness.key_press(Key::F);
        harness.step();
        assert_eq!(harness.state().commands, []);
        assert!(
            harness
                .query_by_label_contains("No onsets to fit")
                .is_some()
        );
    }

    #[test]
    fn empty_chart_rows_reach_late_anchors_and_timing_events() {
        let value = json!({
            "format_version": 1,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "a.ogg" },
            "timing": {
                "anchors": [
                    { "tick": 0, "seconds": 0.0 },
                    { "tick": 38400, "seconds": 20.0 }
                ],
                "tempo": 120.0,
                "events": [
                    { "tick": 76800, "kind": { "time_signature": [3, 4] } }
                ]
            },
            "charts": [{ "name": "Empty", "lanes": 4 }]
        });
        let document = ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        let song = Song::new(document, &[]);
        let window = TimingWindow::default();
        let cache = refreshed(None, &song, &[], &window);
        let frame = frame(&song, &[], (&cache, Layout::Bars), 0.0);

        for tick in [Tick::new(38400), Tick::new(76800)] {
            assert!(
                frame
                    .bars
                    .measures()
                    .iter()
                    .any(|measure| measure.start == tick),
                "no row for {tick:?}"
            );
        }
        assert!(
            frame
                .bars
                .rows(BarsPerRow::One, SecondsPerRow::default())
                .len()
                >= 21
        );
    }

    #[test]
    fn changed_onsets_with_the_same_count_refresh_residuals() {
        let value = json!({
            "format_version": 1,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "a.ogg" },
            "timing": {
                "anchors": [{ "tick": 0, "seconds": 0.0 }],
                "tempo": 120.0,
                "events": []
            }
        });
        let document = ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        let song = Song::new(document, &[]);
        let window = TimingWindow::default();
        let onset = |seconds| Onset {
            seconds,
            strength: 1.0,
        };
        let previous = refreshed(None, &song, &[onset(0.5)], &window);
        assert_eq!(previous.residuals.len(), 1);

        let current = refreshed(Some(previous), &song, &[onset(0.51)], &window);
        assert_eq!(current.residuals.len(), 1);
        assert!((current.residuals[0].ms - 10.0).abs() < 1e-6);
    }

    #[test]
    fn a_refused_edit_capitalises_its_reason_and_accepts_an_empty_plugin_error() {
        let harness = timing_harness();
        let mut window = TimingWindow::default();
        let edit = Edit::TailBpm {
            bpm: Bpm::try_from(120.0).unwrap(),
            merge: false,
        };
        window.refused(
            &edit,
            &Refusal::ReadOnly,
            harness.state().song.timing(),
            0.0,
        );
        assert_eq!(window.status.as_deref(), Some("The song is read only"));
        let blank = Refusal::Game(Box::new(std::io::Error::other("")));
        window.refused(&edit, &blank, harness.state().song.timing(), 0.0);
        assert_eq!(window.status.as_deref(), Some(""));
    }
}
/// Proofs for Kani, run with `cargo kani -p charter-timing`.
#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    fn an_empty_selection_targets_the_focused_anchor_if_there_is_one() {
        let mut window = TimingWindow::default();
        let tick = Tick::new(kani::any());
        window.focus = Some(tick);
        assert_eq!(window.targets(), vec![tick]);
        window.focus = None;
        assert!(window.targets().is_empty());
    }

    #[kani::proof]
    fn a_selection_targets_its_members_instead_of_the_focused_anchor() {
        let mut window = TimingWindow::default();
        let selected = Tick::new(kani::any());
        window.focus = Some(Tick::new(kani::any()));
        window.selection = vec![selected];
        assert_eq!(window.targets(), vec![selected]);
    }
}
