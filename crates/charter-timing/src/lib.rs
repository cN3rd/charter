//! The timing window: the song's audio a bar or a few a row, laid out by bar position rather
//! than by time, so a correct map lines its beats up down the rows and a wrong one shows. Every
//! BPM change is an anchor, pinning a beat to a moment of the audio; the tempo between anchors
//! follows from where they are. The window measures the map against the onsets its host found,
//! and the interaction follows the Beatline specification: the verb is visible, Shift means
//! finer, Ctrl switches from the beat to the audio.
//!
//! The window reads a song and draws it; what the user asks of it comes back as [`Command`]s,
//! which its host carries out, since the host owns the song, its history and its playback. The
//! host says through [`TimingWindow::refused`] when an edit could not be made.

pub mod accuracy;
pub mod grid;
mod panel;
pub mod rows;

use std::num::NonZeroU16;

use charter_audio::{Onset, Peaks};
use charter_core::{Bpm, Tick, Timing, Valid};
use charter_editor::{Pin, Refusal, Song};
use charter_style as style;
use egui::{Key, Modifiers, Pos2, Sense, vec2};

use crate::accuracy::{Fitted, Issue, Residual};
use crate::grid::{Grid, RESOLUTIONS};
use crate::rows::{Anchor, Bars, BarsPerRow, GRAB, Layout, ROW, Row, Scene, SecondsPerRow};

/// What the timing window asks its host to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Make an edit to the song's timing, as one step.
    Edit(Edit),
    /// Put the cursor on the tick and play from there, unless the song is playing already.
    PlayFrom(Tick),
    /// Play from this many seconds into the audio with the metronome on, to hear an edit.
    Audition(f64),
    /// Play, or pause.
    TogglePlay,
    /// Loop the ticks from the first to the second with the metronome, or stop looping them
    /// when they loop already.
    Loop { from: Tick, to: Tick },
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
    /// Makes the beat on `tick` fall `seconds` into the audio, anchoring it there.
    Sync { tick: Tick, seconds: f64 },
    /// Sets the tempo starting at this anchor, accounting for stops, delays and warps.
    SetBpmAt { tick: Tick, bpm: Bpm },
    /// Removes the anchors on the ticks.
    Unsync(Vec<Tick>),
    /// Moves each anchor on a first tick to the beat on its second, keeping their moments.
    MoveAnchors(Vec<(Tick, Tick)>),
    /// Moves the anchors on the ticks `by` seconds, merged into the step before when `merge`.
    Nudge {
        ticks: Vec<Tick>,
        by: f64,
        merge: bool,
    },
    /// Sets the tempo after the last anchor.
    TailBpm { bpm: Bpm, merge: bool },
    /// Doubles, or halves, the tempo after each anchor on the ticks.
    Scale { ticks: Vec<Tick>, double: bool },
    /// Makes these the anchors, with this tempo after the last, as a fit does.
    Repin { pins: Vec<Pin>, tail: Bpm },
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
#[allow(clippy::struct_excessive_bools)] // settings the user turns on and off one by one
pub struct TimingWindow {
    pub open: bool,
    pub grid: Grid,
    /// Whether the rows lay the song out by bar or by audio time.
    pub layout: Layout,
    pub bars_per_row: BarsPerRow,
    /// How many seconds a row holds in the time layout.
    pub seconds_per_row: SecondsPerRow,
    /// Whether added and moved beats snap to the grid.
    pub snap_grid: bool,
    /// Whether moments snap to an onset within 25 ms.
    pub snap_onset: bool,
    /// How strong an onset has to be, from 0 to 1, to count.
    pub sensitivity: f32,
    /// Whether adding an anchor plays from a beat before it.
    pub audition: bool,
    /// Whether the rows follow the playhead while the song plays.
    pub follow: bool,
    focus: Option<Tick>,
    /// The selected anchors, a run of neighbours, by tick.
    selection: Vec<Tick>,
    gesture: Gesture,
    /// The anchor an edit ran into, and when, to pulse it.
    pulse: Option<(Tick, f64)>,
    /// Why the last edit did not happen.
    status: Option<String>,
    /// A fit waiting on Enter or Esc, and what it does in words.
    preview: Option<(Fitted, String)>,
    /// The anchors last nudged, and when, so a run of nudges joins one step.
    last_nudge: Option<(Vec<Tick>, f64)>,
    /// The issue last gone to.
    issue: Option<usize>,
    /// Whether the window has the keyboard: after a click in it, until a click elsewhere.
    keys: bool,
    /// Whether the keys are listed.
    help: bool,
    /// Whether the metronome is muted while Shift+C is held.
    muting: bool,
    drafts: panel::Drafts,
    /// Where the open context menu started, while the pointer moves over its buttons.
    menu_target: Option<(Option<Tick>, Pos2)>,
    /// A tick whose row to scroll to, as the second row.
    scroll_to: Option<Tick>,
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
            gesture: Gesture::Idle,
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

/// What a press or drag in the rows is doing, as the specification's state machine has it.
#[derive(Clone, Debug, PartialEq)]
enum Gesture {
    Idle,
    /// Moving an anchor's beat, `from` where it was to `to`, with the others selected alike.
    DragBeat {
        from: Tick,
        to: Tick,
        origin: f64,
    },
    /// Moving an anchor in the audio, from `from` seconds to `to`.
    DragAudio {
        tick: Tick,
        from: f64,
        to: f64,
        origin: f64,
    },
    /// Selecting the anchors between two points on screen.
    Band {
        origin: Pos2,
        to: Pos2,
    },
}

/// What the rows are measured against, worked out again only when the timing, the grid or the
/// onsets change.
#[derive(Debug)]
struct Cache {
    timing: Timing,
    grid: Grid,
    sensitivity: f32,
    onsets: Vec<Onset>,
    residuals: Vec<Residual>,
    issues: Vec<Issue>,
}

/// Everything a frame of the window works from.
pub(crate) struct Frame<'a> {
    timing: Valid<'a, Timing>,
    pins: Vec<Pin>,
    tail: Bpm,
    bars: Bars<'a>,
    onsets: &'a [Onset],
    residuals: &'a [Residual],
    issues: &'a [Issue],
    editable: bool,
    cursor: Tick,
    now: f64,
}

/// What the host gives the window each frame.
#[derive(Clone, Copy, Debug)]
pub struct Inputs<'a> {
    pub song: Option<&'a Song>,
    pub peaks: Option<&'a Peaks>,
    pub onsets: &'a [Onset],
    pub playing: bool,
    /// The calibrated audio latency, in milliseconds, shown in the status line.
    pub latency_ms: f64,
}

impl TimingWindow {
    /// Whether the window takes the keyboard's commands, so the editor window leaves them.
    #[must_use]
    pub fn has_keyboard(&self) -> bool {
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
            other => {
                let first = other.to_string();
                let mut letters = first.chars();
                letters.next().map_or(first.clone(), |letter| {
                    letter.to_uppercase().chain(letters).collect()
                })
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
        let fitted = accuracy::fit(
            frame.timing,
            (&frame.pins, frame.tail),
            range.clone(),
            frame.residuals,
            half_bpm,
        );
        match fitted {
            Some(fitted) => {
                let before = frame.tail;
                let about = if *range.end() + 1 == frame.pins.len() {
                    format!("Tail {before} → {} BPM", fitted.tail)
                } else {
                    format!("{} anchors fitted", range.count())
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

    fn focus_on(&mut self, tick: Tick) {
        self.focus = Some(tick);
        self.selection = vec![tick];
        self.scroll_to = Some(tick);
    }

    /// Nudges the targets `by` seconds, joining the step before when they were nudged a moment
    /// ago.
    fn nudge(&mut self, by: f64, now: f64, commands: &mut Vec<Command>) {
        let ticks = self.targets();
        if ticks.is_empty() {
            return;
        }
        let merge = self
            .last_nudge
            .as_ref()
            .is_some_and(|(last, at)| *last == ticks && now - at < NUDGE_MERGE);
        self.last_nudge = Some((ticks.clone(), now));
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
        let tick = frame.timing.latest_tick_by(seconds);
        let tick = if self.snap_grid {
            self.grid.nearest(frame.timing, tick)
        } else {
            tick
        };
        let half = self
            .grid
            .step(frame.timing.ticks_per_beat())
            .map_or(0, |step| i64::from(u32::from(step)) / 2);
        let close = frame
            .pins
            .iter()
            .find(|pin| (i64::from(pin.tick) - i64::from(tick)).abs() < half.max(1));
        if let Some(close) = close {
            self.pulse = Some((close.tick, frame.now));
            self.status = Some(format!(
                "Too close to the anchor at {}",
                style::clock(frame.timing.time_at(close.tick).seconds())
            ));
            return;
        }
        commands.push(Command::Edit(Edit::Sync { tick, seconds }));
        self.focus_on(tick);
        self.status = None;
        if self.audition {
            commands.push(Command::Audition(seconds - lead(frame, tick)));
        }
    }
}

/// How long before `tick` an audition starts: a beat there, kept between 150 and 600 ms.
fn lead(frame: &Frame<'_>, tick: Tick) -> f64 {
    let bpm = frame.timing.tempo_at(tick);
    (60.0 / bpm).clamp(AUDITION.0, AUDITION.1)
}

/// What a key asks of the timing window.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Intent {
    Play,
    Enter,
    Escape,
    AddAtPlayhead,
    /// Focus the anchor before, or after.
    Focus(bool),
    /// Go to the issue before, or after.
    Issue(bool),
    /// Nudge the targets this many seconds.
    Nudge(f64),
    /// Move the targets this many grid steps.
    Move(i32),
    /// Change the tail tempo by this many BPM.
    Tail(f64),
    /// Use the grid resolution at this place among [`RESOLUTIONS`].
    Grid(usize),
    Fit {
        half_bpm: bool,
    },
    Scale {
        double: bool,
    },
    Follow,
    /// Switch the rows between the bar layout and the time layout.
    Layout,
    Delete,
    SelectAll,
    Help,
    /// Loop the bar under the cursor, or the selection.
    Loop {
        selection: bool,
    },
    ToggleRate,
    ToggleMetronome,
    Mute,
}

/// What `key` pressed with `modifiers` asks of the timing window, if anything.
fn intent(key: Key, modifiers: Modifiers) -> Option<Intent> {
    let (shift, ctrl) = (modifiers.shift, modifiers.command || modifiers.ctrl);
    let nudge = if ctrl {
        0.010
    } else if shift {
        0.0001
    } else {
        0.001
    };
    Some(match key {
        Key::Space => Intent::Play,
        Key::Enter => Intent::Enter,
        Key::Escape => Intent::Escape,
        Key::A if ctrl => Intent::SelectAll,
        Key::A => Intent::AddAtPlayhead,
        Key::J => Intent::Focus(false),
        Key::K => Intent::Focus(true),
        Key::N => Intent::Issue(!shift),
        Key::ArrowLeft => Intent::Nudge(-nudge),
        Key::ArrowRight => Intent::Nudge(nudge),
        Key::ArrowUp => Intent::Move(-1),
        Key::ArrowDown => Intent::Move(1),
        Key::Plus | Key::Equals => Intent::Tail(if shift { 1.0 } else { 0.1 }),
        Key::Minus => Intent::Tail(if shift { -1.0 } else { -0.1 }),
        Key::F => Intent::Fit { half_bpm: shift },
        Key::D => Intent::Scale { double: !shift },
        Key::O => Intent::Follow,
        Key::V => Intent::Layout,
        Key::L => Intent::Loop { selection: shift },
        Key::R => Intent::ToggleRate,
        Key::C if shift => Intent::Mute,
        Key::C => Intent::ToggleMetronome,
        Key::Delete | Key::Backspace => Intent::Delete,
        Key::Questionmark | Key::Slash => Intent::Help,
        _ => {
            let numbers = [
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
                Key::Num7,
                Key::Num8,
                Key::Num9,
                Key::Num0,
            ];
            Intent::Grid(numbers.iter().position(|number| *number == key)?)
        }
    })
}

/// The keys of the timing window, for its shortcut sheet.
const KEYS: &[(&str, &str)] = &[
    ("Space", "Play or pause"),
    (
        "Enter",
        "Play from a beat before the focused anchor; apply a fit",
    ),
    ("Esc", "Cancel a drag or fit, then clear the selection"),
    ("A", "Add an anchor at the playhead"),
    ("J, K", "Focus the anchor before, after"),
    ("N, Shift+N", "Next, previous issue"),
    (
        "←, →",
        "Nudge in the audio by 1 ms · Shift 0.1 ms · Ctrl 10 ms",
    ),
    ("↑, ↓", "Move on the grid by one step"),
    ("+, −", "Tail tempo by 0.1 BPM · Shift 1 BPM"),
    ("1 … 0", "Grid 1/1 … 1/32"),
    ("F, Shift+F", "Fit tempo, fit to half BPMs"),
    ("D, Shift+D", "Double, halve the tempo"),
    ("O", "Follow the playhead"),
    ("V", "Rows of bars or rows of seconds"),
    ("L, Shift+L", "Loop the bar under the cursor, the selection"),
    ("R", "Switch to the alternate speed and back"),
    ("C, Shift+C held", "Metronome on or off; muted while held"),
    ("?", "Show or hide these keys"),
    ("Del", "Remove the focused or selected anchors"),
    ("Ctrl+A", "Select every anchor"),
    (
        "Ctrl+wheel",
        "Nudge the focused anchor by 1 ms · Shift 0.1 ms",
    ),
];

/// Draws the timing window over `inputs` while it is open, and answers its input with
/// `commands`.
#[allow(clippy::too_many_lines)] // the window's panels in the order they draw
pub fn show(
    ctx: &egui::Context,
    inputs: Inputs<'_>,
    window: &mut TimingWindow,
    commands: &mut Vec<Command>,
) {
    if !window.open {
        window.gesture = Gesture::Idle;
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
            settle(window, song);
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
                    help(ui);
                }
                hovering = rows(ui, window, &frame, inputs, commands);
            });
            if window.keys && !ui.ctx().egui_wants_keyboard_input() {
                keyboard(ui.ctx(), window, &frame, commands);
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
        cache.timing == *timing
            && cache.grid == window.grid
            && (cache.sensitivity - window.sensitivity).abs() <= f32::EPSILON
            && cache.onsets.as_slice() == onsets
    });
    fresh.unwrap_or_else(|| {
        let pins = song.anchors();
        let residuals = accuracy::residuals(timing, window.grid, onsets, window.sensitivity);
        let issues = accuracy::issues(timing, &pins, &residuals, (onsets, window.sensitivity));
        Cache {
            timing: (*timing).clone(),
            grid: window.grid,
            sensitivity: window.sensitivity,
            onsets: onsets.to_vec(),
            residuals,
            issues,
        }
    })
}

/// Keeps the focus and selection on anchors that are there, after edits moved or removed some.
fn settle(window: &mut TimingWindow, song: &Song) {
    let pins = song.anchors();
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
    let first = timing.latest_tick_by(0.0).min(Tick::new(0));
    let mut last = song.end().max(song.cursor());
    if let Some(anchor) = timing.anchors.last() {
        last = last.max(anchor.tick);
    }
    if let Some(event) = timing.events.last() {
        last = last.max(event.tick);
    }
    Frame {
        timing,
        tail: song
            .tail_bpm()
            .unwrap_or_else(|| Bpm::try_from(120.0).unwrap_or_else(|_| unreachable!())),
        pins: song.anchors(),
        bars: Bars::new(timing, first, last, 4).laid_out(layout),
        onsets,
        residuals: &cache.residuals,
        issues: &cache.issues,
        editable: song.read_only().is_none(),
        cursor: song.cursor(),
        now,
    }
}

fn help(ui: &mut egui::Ui) {
    let narrow = ui.available_width() < 600.0;
    let height = (ui.available_height() - ROW - 20.0).max(120.0);
    style::solid_scroll_bars(ui);
    egui::ScrollArea::vertical()
        .id_salt("timing help")
        .max_height(height)
        .show(ui, |ui| {
            egui::Frame::new()
                .fill(style::RAISED)
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    if narrow {
                        for (keys, does) in KEYS {
                            ui.monospace(*keys);
                            ui.label(*does);
                            ui.add_space(6.0);
                        }
                    } else {
                        style::table(ui, "timing keys", |ui| {
                            for (keys, does) in KEYS {
                                ui.monospace(*keys);
                                ui.label(*does);
                                ui.end_row();
                            }
                        });
                    }
                });
        });
}

/// The rows, and every press and drag on them. Says whether the pointer is over an anchor.
#[allow(clippy::too_many_lines)] // one row after another, then the drag across them
fn rows(
    ui: &mut egui::Ui,
    window: &mut TimingWindow,
    frame: &Frame<'_>,
    inputs: Inputs<'_>,
    commands: &mut Vec<Command>,
) -> bool {
    let spans = frame.bars.rows(window.bars_per_row, window.seconds_per_row);
    let spacing = 4.0;
    let pitch = ROW + spacing;
    let row_of = |tick: Tick| {
        #[allow(clippy::cast_precision_loss)]
        let position = frame.bars.position(i64::from(tick) as f64);
        spans
            .iter()
            .position(|span| position >= span.start && position < span.to)
    };
    let target = window
        .scroll_to
        .take()
        .or((window.follow && inputs.playing).then_some(frame.cursor));
    let mut area = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource {
            drag: egui::scroll_area::DragScroll::Never,
            ..egui::scroll_area::ScrollSource::ALL
        });
    if let Some(index) = target.and_then(row_of) {
        #[allow(clippy::cast_precision_loss)] // rows of a song
        let offset = (index.saturating_sub(1)) as f32 * pitch;
        area = area.vertical_scroll_offset(offset);
    }
    style::solid_scroll_bars(ui);
    let anchors: Vec<Anchor> = frame
        .pins
        .iter()
        .map(|pin| Anchor {
            tick: pin.tick,
            bpm: tempo_at(frame, pin.tick)
                .map_or_else(|| frame.tail.to_string(), |bpm| bpm.to_string()),
            focused: window.focus == Some(pin.tick),
            selected: window.selection.contains(&pin.tick),
            pulse: window
                .pulse
                .filter(|(tick, _)| *tick == pin.tick)
                .map_or(0.0, |(_, at)| {
                    #[allow(clippy::cast_possible_truncation)] // a share of a second
                    let through = ((frame.now - at) / PULSE) as f32;
                    (through * std::f32::consts::TAU * 2.0).sin().abs() * (1.0 - through)
                }),
        })
        .collect();
    let mut laid: Vec<Row> = Vec::new();
    let mut hovering = false;
    let mut released: Option<Pos2> = None;
    let latest = ui.input(|input| input.pointer.latest_pos());
    let shift = ui.input(|input| input.modifiers.shift);
    let reach = reach(window, frame);
    area.show_rows(ui, ROW, spans.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = spacing;
        for span in spans.get(range).unwrap_or_default() {
            let (whole, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
            let mut rect = whole;
            rect.min.x += rows::GUTTER;
            let row = Row {
                from: span.from,
                start: span.start,
                to: span.to,
                rect,
            };
            // The measure the row's own part starts in, which names it in the bar layout.
            let measure = frame
                .bars
                .measure_at(frame.bars.tick_at_position(span.start).floor());
            let number = measure.map_or(0, |measure| measure.number);
            let (title, label) = match window.layout {
                Layout::Bars => (number.to_string(), format!("Measure {number}")),
                Layout::Time => (
                    format!("{:.1}s", span.start),
                    format!("Row at {:.1} s", span.start),
                ),
            };
            #[allow(clippy::cast_possible_truncation)] // hundredths of a row's start
            let key = (span.start * 100.0).round() as i64;
            let id = ui.id().with(("timing row", key));
            let response = ui.interact(rect, id, Sense::click_and_drag());
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &label));
            let painter = ui.painter_at(whole);
            let off = frame.issues.iter().find_map(|issue| match *issue {
                Issue::Bar {
                    number: at,
                    median_ms,
                    ..
                } if at == number => Some(median_ms),
                _ => None,
            });
            rows::header(&painter, &row, (&title, measure), off);
            let ghost = ghost(window, frame, &row, response.hover_pos(), shift);
            let scene = Scene {
                timing: frame.timing,
                bars: &frame.bars,
                grid: window.grid,
                peaks: inputs.peaks,
                onsets: frame.onsets,
                sensitivity: window.sensitivity,
                residuals: frame.residuals,
                anchors: &anchors,
                cursor: frame.cursor,
                ghost,
                reach,
            };
            rows::draw(&painter, &row, &scene);
            let over = response
                .hover_pos()
                .and_then(|pos| anchor_near(frame, &row, pos.x));
            hovering |= over.is_some();
            if frame.editable {
                row_input(&response, &row, frame, window, commands);
                if response.drag_stopped() {
                    released = ui.input(|input| input.pointer.latest_pos());
                }
            } else if response.secondary_clicked()
                && let Some(pos) = response.interact_pointer_pos()
            {
                commands.push(Command::PlayFrom(row.tick_at(&frame.bars, pos.x)));
            }
            laid.push(row);
        }
    });
    if let Gesture::Band { origin, .. } = window.gesture
        && let Some(to) = latest
    {
        window.gesture = Gesture::Band { origin, to };
        let band = egui::Rect::from_two_pos(origin, to);
        ui.painter().rect_filled(band, 0.0, style::MARQUEE_FILL);
        ui.painter().rect_stroke(
            band,
            0.0,
            egui::Stroke::new(1.0, style::MARQUEE),
            egui::StrokeKind::Inside,
        );
    }
    if let Some(at) = released {
        release(window, frame, &laid, at, commands);
    }
    hovering
}

/// The bar positions the anchor being dragged on the grid may reach, if one is; the rest is
/// dimmed.
fn reach(window: &TimingWindow, frame: &Frame<'_>) -> Option<(f64, f64)> {
    let Gesture::DragBeat { from, .. } = window.gesture else {
        return None;
    };
    let bar = |tick: Tick| {
        #[allow(clippy::cast_precision_loss)]
        let at = i64::from(tick) as f64;
        frame.bars.position(at)
    };
    let moving = moving(window, from);
    let (first, last) = (moving.first().copied()?, moving.last().copied()?);
    let before = frame
        .pins
        .iter()
        .rev()
        .find(|pin| pin.tick < first)
        .map(|pin| pin.tick);
    let after = frame
        .pins
        .iter()
        .find(|pin| pin.tick > last)
        .map(|pin| pin.tick);
    Some((before.map_or(f64::MIN, bar), after.map_or(f64::MAX, bar)))
}

/// Where in `row` what is under way would land, and whether that is on a snap target: a
/// dragged anchor's target, or where a click at `hover` would add one.
fn ghost(
    window: &TimingWindow,
    frame: &Frame<'_>,
    row: &Row,
    hover: Option<Pos2>,
    shift: bool,
) -> Option<(f32, bool)> {
    match window.gesture {
        Gesture::DragBeat { to, .. } if row.holds(&frame.bars, to) => {
            Some((row.x(&frame.bars, to), window.snap_grid && !shift))
        }
        Gesture::DragAudio { to, .. } => {
            let snapped = window.snap_onset
                && !shift
                && frame
                    .onsets
                    .iter()
                    .any(|onset| (onset.seconds - to).abs() < 1e-9);
            Some((seconds_x(frame, row, to)?, snapped))
        }
        Gesture::Idle if frame.editable => {
            let pos = hover?;
            if anchor_near(frame, row, pos.x).is_some() {
                return None;
            }
            let seconds = row.seconds_at(&frame.bars, frame.timing, pos.x);
            let snapped = (window.snap_onset && !shift)
                .then(|| accuracy::snap(frame.onsets, window.sensitivity, seconds))
                .flatten();
            let target = snapped.unwrap_or(seconds);
            Some((seconds_x(frame, row, target)?, snapped.is_some()))
        }
        _ => None,
    }
}

/// Where the moment `seconds` lies across `row`, if it shows there.
fn seconds_x(frame: &Frame<'_>, row: &Row, seconds: f64) -> Option<f32> {
    let tick = frame.timing.latest_tick_by(seconds);
    row.holds(&frame.bars, tick).then(|| {
        let at = frame.timing.time_at(tick).seconds();
        let next = frame
            .timing
            .time_at(Tick::new(
                i32::try_from(i64::from(tick) + 1).unwrap_or(i32::MAX),
            ))
            .seconds();
        let within = if next > at {
            (seconds - at) / (next - at)
        } else {
            0.0
        };
        #[allow(clippy::cast_precision_loss)]
        let tick = i64::from(tick) as f64 + within.clamp(0.0, 1.0);
        row.x_of(frame.bars.position(tick))
    })
}

/// The anchors a drag of the one on `tick` moves: the selection when it is in it.
fn moving(window: &TimingWindow, tick: Tick) -> Vec<Tick> {
    if window.selection.contains(&tick) {
        window.selection.clone()
    } else {
        vec![tick]
    }
}

/// The anchor in `row` within grabbing distance of `x`, the nearest if several.
fn anchor_near(frame: &Frame<'_>, row: &Row, x: f32) -> Option<Tick> {
    frame
        .pins
        .iter()
        .filter(|pin| row.holds(&frame.bars, pin.tick))
        .map(|pin| (pin.tick, (row.x(&frame.bars, pin.tick) - x).abs()))
        .filter(|(_, off)| *off <= GRAB)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(tick, _)| tick)
}

/// A row's presses, clicks and the start and run of drags on it.
#[allow(clippy::too_many_lines)] // this handles the row's pointer gestures and context menu
fn row_input(
    response: &egui::Response,
    row: &Row,
    frame: &Frame<'_>,
    window: &mut TimingWindow,
    commands: &mut Vec<Command>,
) {
    let modifiers = response.ctx.input(|input| input.modifiers);
    let (shift, ctrl) = (modifiers.shift, modifiers.command || modifiers.ctrl);
    let pointer = response.interact_pointer_pos().or(response.hover_pos());
    let anchor = pointer.and_then(|pos| anchor_near(frame, row, pos.x));
    // Where the button went down, not where the drag was first noticed a few points on.
    if response.drag_started()
        && let Some(origin) = response.ctx.input(|input| input.pointer.press_origin())
    {
        let pressed = anchor_near(frame, row, origin.x);
        let seconds = row.seconds_at(&frame.bars, frame.timing, origin.x);
        window.gesture = match pressed {
            Some(tick) if ctrl => {
                let at = frame
                    .pins
                    .iter()
                    .find(|pin| pin.tick == tick)
                    .map_or(seconds, |pin| pin.seconds);
                Gesture::DragAudio {
                    tick,
                    from: at,
                    to: at,
                    origin: seconds,
                }
            }
            Some(tick) => Gesture::DragBeat {
                from: tick,
                to: tick,
                origin: row.position_at(origin.x),
            },
            None => Gesture::Band { origin, to: origin },
        };
        if let Some(tick) = pressed
            && !window.selection.contains(&tick)
        {
            window.focus = Some(tick);
            window.selection = vec![tick];
        }
    }
    if response.dragged()
        && let Some(pos) = response.ctx.input(|input| input.pointer.latest_pos())
    {
        drag(window, frame, row, pos, shift);
    }
    if response.double_clicked() {
        if let Some(tick) = anchor {
            window.focus = Some(tick);
            window.selection = vec![tick];
            window.drafts.focus_seconds = true;
        }
    } else if response.clicked()
        && let Some(pos) = pointer
    {
        match anchor {
            Some(tick) if shift => {
                let from = window.focus.unwrap_or(tick);
                let (low, high) = if from <= tick {
                    (from, tick)
                } else {
                    (tick, from)
                };
                window.selection = frame
                    .pins
                    .iter()
                    .map(|pin| pin.tick)
                    .filter(|at| *at >= low && *at <= high)
                    .collect();
                window.focus = Some(tick);
            }
            Some(tick) => {
                window.focus = Some(tick);
                window.selection = vec![tick];
            }
            None => {
                let seconds = row.seconds_at(&frame.bars, frame.timing, pos.x);
                window.add(frame, seconds, shift, commands);
            }
        }
    }
    if response.middle_clicked()
        && let Some(pos) = pointer
    {
        match anchor.and_then(|tick| frame.pins.iter().find(|pin| pin.tick == tick)) {
            Some(pin) => commands.push(Command::Audition(pin.seconds - lead(frame, pin.tick))),
            None => commands.push(Command::PlayFrom(row.tick_at(&frame.bars, pos.x))),
        }
    }
    if response.secondary_clicked()
        && let Some(pos) = pointer
    {
        window.menu_target = Some((anchor, pos));
    }
    response.context_menu(|ui| {
        let (anchor, pointer) = window
            .menu_target
            .map_or((anchor, pointer), |(tick, pos)| (tick, Some(pos)));
        menu(ui, window, frame, row, anchor, pointer, commands);
    });
    if response.hovered()
        && let Some((notches, fine)) = response.ctx.input(|input| nudge_notches(&input.events))
    {
        let target = window.focus.or(anchor);
        if let Some(tick) = target {
            window.focus = Some(tick);
            if window.selection.is_empty() {
                window.selection = vec![tick];
            }
            let by = if fine { 0.0001 } else { 0.001 };
            window.nudge(by * f64::from(notches), frame.now, commands);
        }
    }
}

/// Moves the drag under way to `pos`, over `row`: a beat snaps to the grid, a moment to an onset,
/// both finely and unsnapped with Shift, and neither past the anchors beside what moves; running
/// into one pulses it and stops there.
fn drag(window: &mut TimingWindow, frame: &Frame<'_>, row: &Row, pos: Pos2, shift: bool) {
    match window.gesture.clone() {
        Gesture::DragBeat { from, origin, .. } => {
            let pointer = row.position_at(pos.x);
            let bar = if shift {
                origin + (pointer - origin) / FINE
            } else {
                pointer
            };
            let tick = frame.bars.tick_at_position(bar).round();
            #[allow(clippy::cast_possible_truncation)] // clamped to a document's ticks
            let tick = Tick::new(tick.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32);
            let snapped = if window.snap_grid && !shift {
                window.grid.nearest(frame.timing, tick)
            } else {
                tick
            };
            let to = clamp_beat(window, frame, from, snapped);
            window.gesture = Gesture::DragBeat { from, to, origin };
        }
        Gesture::DragAudio {
            tick, from, origin, ..
        } => {
            let pointer = row.seconds_at(&frame.bars, frame.timing, pos.x);
            let moment = if shift {
                origin + (pointer - origin) / FINE
            } else {
                pointer
            };
            let target = from + (moment - origin);
            let target = if window.snap_onset && !shift {
                accuracy::snap(frame.onsets, window.sensitivity, target).unwrap_or(target)
            } else {
                target
            };
            let to = clamp_audio(window, frame, tick, from, target);
            window.gesture = Gesture::DragAudio {
                tick,
                from,
                to,
                origin,
            };
        }
        Gesture::Band { .. } | Gesture::Idle => {}
    }
}

/// `to`, or the nearest beat to it that the anchors moving with the one on `from` can reach
/// without meeting a neighbour, which then pulses.
fn clamp_beat(window: &mut TimingWindow, frame: &Frame<'_>, from: Tick, to: Tick) -> Tick {
    let moving = moving(window, from);
    let (Some(&first), Some(&last)) = (moving.first(), moving.last()) else {
        return to;
    };
    let delta = i64::from(to) - i64::from(from);
    let before = frame.pins.iter().rev().find(|pin| pin.tick < first);
    let after = frame.pins.iter().find(|pin| pin.tick > last);
    let mut clamped = delta;
    if let Some(before) = before {
        let lowest = i64::from(before.tick) + 1 - i64::from(first);
        if clamped < lowest {
            clamped = lowest;
            window.pulse = Some((before.tick, frame.now));
        }
    }
    if let Some(after) = after {
        let highest = i64::from(after.tick) - 1 - i64::from(last);
        if clamped > highest {
            clamped = highest;
            window.pulse = Some((after.tick, frame.now));
        }
    }
    Tick::new(i32::try_from(i64::from(from) + clamped).unwrap_or(i32::MAX))
}

/// `to` seconds, or the nearest moment the anchors moving with the one on `tick` can reach
/// without meeting a neighbour in the audio, which then pulses.
fn clamp_audio(
    window: &mut TimingWindow,
    frame: &Frame<'_>,
    tick: Tick,
    from: f64,
    to: f64,
) -> f64 {
    const MARGIN: f64 = 0.001;
    let moving = moving(window, tick);
    let times: Vec<(Tick, f64)> = frame
        .pins
        .iter()
        .map(|pin| (pin.tick, pin.seconds))
        .collect();
    let (Some(&first), Some(&last)) = (moving.first(), moving.last()) else {
        return to;
    };
    let seconds = |at: Tick| times.iter().find(|(tick, _)| *tick == at).map(|(_, s)| *s);
    let (Some(first_at), Some(last_at)) = (seconds(first), seconds(last)) else {
        return to;
    };
    let mut delta = to - from;
    if let Some(&(before, at)) = times.iter().rev().find(|(t, _)| *t < first)
        && first_at + delta < at + MARGIN
    {
        delta = at + MARGIN - first_at;
        window.pulse = Some((before, frame.now));
    }
    if let Some(&(after, at)) = times.iter().find(|(t, _)| *t > last)
        && last_at + delta > at - MARGIN
    {
        delta = at - MARGIN - last_at;
        window.pulse = Some((after, frame.now));
    }
    from + delta
}

/// Ends the drag under way where the pointer let go, at `at` among the `laid` rows.
fn release(
    window: &mut TimingWindow,
    frame: &Frame<'_>,
    laid: &[Row],
    at: Pos2,
    commands: &mut Vec<Command>,
) {
    match std::mem::replace(&mut window.gesture, Gesture::Idle) {
        Gesture::DragBeat { from, to, .. } if from != to => {
            let delta = i64::from(to) - i64::from(from);
            let moved: Vec<(Tick, Tick)> = moving(window, from)
                .into_iter()
                .filter_map(|tick| {
                    let to = i32::try_from(i64::from(tick) + delta).ok()?;
                    Some((tick, Tick::new(to)))
                })
                .collect();
            window.selection = moved.iter().map(|(_, to)| *to).collect();
            window.focus = Some(to);
            commands.push(Command::Edit(Edit::MoveAnchors(moved)));
        }
        Gesture::DragAudio { tick, from, to, .. } if (to - from).abs() > 1e-7 => {
            let ticks = moving(window, tick);
            commands.push(Command::Edit(Edit::Nudge {
                ticks,
                by: to - from,
                merge: false,
            }));
        }
        Gesture::Band { origin, .. } => {
            let tick_of =
                |pos: Pos2| rows::row_at(laid, pos).map(|row| row.tick_at(&frame.bars, pos.x));
            let (Some(a), Some(b)) = (tick_of(origin), tick_of(at)) else {
                return;
            };
            let (low, high) = if a <= b { (a, b) } else { (b, a) };
            window.selection = frame
                .pins
                .iter()
                .map(|pin| pin.tick)
                .filter(|tick| *tick >= low && *tick <= high)
                .collect();
            window.focus = window.selection.first().copied();
        }
        Gesture::DragBeat { .. } | Gesture::DragAudio { .. } | Gesture::Idle => {}
    }
}

/// The right-click menu of a row, on an anchor or off one.
fn menu(
    ui: &mut egui::Ui,
    window: &mut TimingWindow,
    frame: &Frame<'_>,
    row: &Row,
    anchor: Option<Tick>,
    pointer: Option<Pos2>,
    commands: &mut Vec<Command>,
) {
    if let Some(tick) = anchor {
        if window.focus != Some(tick) && !window.selection.contains(&tick) {
            window.focus = Some(tick);
            window.selection = vec![tick];
        }
        if ui.button("Play from a beat before").clicked()
            && let Some(pin) = frame.pins.iter().find(|pin| pin.tick == tick)
        {
            commands.push(Command::Audition(pin.seconds - lead(frame, tick)));
            ui.close();
        }
        if ui.button("Fit tempo").clicked() {
            window.fit(frame, false);
            ui.close();
        }
        if ui.button("Double tempo").clicked() {
            commands.push(Command::Edit(Edit::Scale {
                ticks: window.targets(),
                double: true,
            }));
            ui.close();
        }
        if ui.button("Halve tempo").clicked() {
            commands.push(Command::Edit(Edit::Scale {
                ticks: window.targets(),
                double: false,
            }));
            ui.close();
        }
        if ui.button("Select to the start").clicked() {
            window.selection = frame
                .pins
                .iter()
                .map(|pin| pin.tick)
                .filter(|at| *at <= tick)
                .collect();
            ui.close();
        }
        if ui.button("Select to the end").clicked() {
            window.selection = frame
                .pins
                .iter()
                .map(|pin| pin.tick)
                .filter(|at| *at >= tick)
                .collect();
            ui.close();
        }
        ui.separator();
        if ui.button("Remove").clicked() {
            commands.push(Command::Edit(Edit::Unsync(window.targets())));
            ui.close();
        }
    } else if let Some(pos) = pointer {
        if ui.button("Play from here").clicked() {
            commands.push(Command::PlayFrom(row.tick_at(&frame.bars, pos.x)));
            ui.close();
        }
        if ui.button("Add an anchor here").clicked() {
            let seconds = row.seconds_at(&frame.bars, frame.timing, pos.x);
            window.add(frame, seconds, false, commands);
            ui.close();
        }
    }
}

/// The window's keys, while it has the keyboard.
fn keyboard(
    ctx: &egui::Context,
    window: &mut TimingWindow,
    frame: &Frame<'_>,
    commands: &mut Vec<Command>,
) {
    let pressed: Vec<(Key, Modifiers)> = ctx.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
            .collect()
    });
    // Shift+C mutes only while held.
    let let_go = ctx.input(|input| {
        input.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key: Key::C,
                    pressed: false,
                    ..
                }
            )
        })
    });
    if window.muting && let_go {
        window.muting = false;
        commands.push(Command::MuteMetronome(false));
    }
    let dragging = !matches!(window.gesture, Gesture::Idle);
    for (key, modifiers) in pressed {
        let Some(intent) = intent(key, modifiers) else {
            continue;
        };
        // During a drag only Esc and Space count.
        if dragging && !matches!(intent, Intent::Escape | Intent::Play) {
            continue;
        }
        if frame.editable
            || matches!(
                intent,
                Intent::Play
                    | Intent::Focus(_)
                    | Intent::Issue(_)
                    | Intent::Grid(_)
                    | Intent::Follow
                    | Intent::Layout
                    | Intent::Help
                    | Intent::Escape
                    | Intent::Loop { .. }
                    | Intent::ToggleRate
                    | Intent::ToggleMetronome
                    | Intent::Mute
            )
        {
            act(window, frame, intent, commands);
        }
    }
}

/// Does what `intent` asks.
#[allow(clippy::too_many_lines)] // one arm for every key the window has
fn act(window: &mut TimingWindow, frame: &Frame<'_>, intent: Intent, commands: &mut Vec<Command>) {
    let index = |tick: Tick| frame.pins.iter().position(|pin| pin.tick == tick);
    match intent {
        Intent::Play => commands.push(Command::TogglePlay),
        Intent::Enter => {
            if let Some((fitted, _)) = window.preview.take() {
                commands.push(Command::Edit(Edit::Repin {
                    pins: fitted.pins,
                    tail: fitted.tail,
                }));
            } else if let Some(pin) = window
                .focus
                .and_then(index)
                .and_then(|at| frame.pins.get(at))
            {
                commands.push(Command::Audition(pin.seconds - lead(frame, pin.tick)));
            }
        }
        Intent::Escape => {
            if !matches!(window.gesture, Gesture::Idle) {
                window.gesture = Gesture::Idle;
            } else if window.preview.is_some() {
                window.preview = None;
            } else if window.help {
                window.help = false;
            } else {
                window.selection.clear();
                window.focus = None;
            }
        }
        Intent::AddAtPlayhead => {
            let seconds = frame.timing.time_at(frame.cursor).seconds();
            window.add(frame, seconds, false, commands);
        }
        Intent::Focus(later) => {
            let current = window.focus.and_then(index);
            let next = match (current, later) {
                (Some(at), true) => at + 1,
                (Some(at), false) => at.saturating_sub(1),
                (None, _) => frame
                    .pins
                    .iter()
                    .position(|pin| pin.tick >= frame.cursor)
                    .unwrap_or(0),
            };
            if let Some(pin) = frame.pins.get(next.min(frame.pins.len().saturating_sub(1))) {
                window.focus_on(pin.tick);
            }
        }
        Intent::Issue(later) => {
            let count = frame.issues.len();
            if count > 0 {
                let at = match (window.issue, later) {
                    (Some(at), true) => (at + 1) % count,
                    (Some(at), false) => (at + count - 1) % count,
                    (None, _) => 0,
                };
                window.go_to_issue(frame, at);
            }
        }
        Intent::Nudge(by) => window.nudge(by, frame.now, commands),
        Intent::Move(steps) => {
            let moved: Vec<(Tick, Tick)> = window
                .targets()
                .into_iter()
                .map(|tick| (tick, window.grid.stepped(frame.timing, tick, steps)))
                .collect();
            if let Some(&(_, to)) = moved.iter().find(|(from, _)| Some(*from) == window.focus) {
                window.focus = Some(to);
            }
            window.selection = moved.iter().map(|(_, to)| *to).collect();
            commands.push(Command::Edit(Edit::MoveAnchors(moved)));
        }
        Intent::Tail(by) => {
            let tempo = f64::from(frame.tail) + by;
            match Bpm::try_from((tempo * 1e6).round() / 1e6) {
                Ok(bpm) => {
                    let merge = window
                        .last_nudge
                        .as_ref()
                        .is_some_and(|(last, at)| last.is_empty() && frame.now - at < NUDGE_MERGE);
                    window.last_nudge = Some((Vec::new(), frame.now));
                    commands.push(Command::Edit(Edit::TailBpm { bpm, merge }));
                }
                Err(_) => window.status = Some(format!("{tempo} is not a tempo")),
            }
        }
        Intent::Grid(at) => {
            if let Some(resolution) = RESOLUTIONS
                .get(at)
                .and_then(|value| NonZeroU16::new(*value))
            {
                window.grid = Grid { resolution };
            }
        }
        Intent::Fit { half_bpm } => window.fit(frame, half_bpm),
        Intent::Scale { double } => {
            commands.push(Command::Edit(Edit::Scale {
                ticks: window.targets(),
                double,
            }));
        }
        Intent::Follow => window.follow = !window.follow,
        Intent::Layout => {
            window.layout = match window.layout {
                Layout::Bars => Layout::Time,
                Layout::Time => Layout::Bars,
            };
            // Keep what was in view there.
            window.scroll_to = window.focus.or(Some(frame.cursor));
        }
        Intent::Delete => {
            let targets = window.targets();
            if !targets.is_empty() {
                commands.push(Command::Edit(Edit::Unsync(targets)));
                window.selection.clear();
            }
        }
        Intent::SelectAll => {
            window.selection = frame.pins.iter().map(|pin| pin.tick).collect();
            window.focus = window.selection.first().copied();
        }
        Intent::Help => window.help = !window.help,
        Intent::Loop { selection } => {
            let span = if selection {
                window.selected_span(frame)
            } else {
                None
            };
            let span = span.or_else(|| {
                let measure = frame.timing.measures_from(frame.cursor).next()?;
                Some((measure.start, measure.start.checked_add(measure.length)?))
            });
            if let Some((from, to)) = span {
                commands.push(Command::Loop { from, to });
            }
        }
        Intent::ToggleRate => commands.push(Command::ToggleRate),
        Intent::ToggleMetronome => commands.push(Command::ToggleMetronome),
        Intent::Mute => {
            if !window.muting {
                window.muting = true;
                commands.push(Command::MuteMetronome(true));
            }
        }
    }
}

/// The tempo of the BPM change on `tick`, as the song holds it: stops and delays between
/// anchors take time the tempo does not.
fn tempo_at(frame: &Frame<'_>, tick: Tick) -> Option<Bpm> {
    frame.timing.bpm_at(tick)
}

/// Ctrl+wheel notches among `events`, later first, and whether Shift made them coarse.
fn nudge_notches(events: &[egui::Event]) -> Option<(i32, bool)> {
    let mut notches = 0;
    let mut coarse = false;
    for event in events {
        if let egui::Event::MouseWheel {
            delta, modifiers, ..
        } = event
            && (modifiers.command || modifiers.ctrl)
            && delta.y != 0.0
        {
            notches += if delta.y > 0.0 { 1 } else { -1 };
            coarse |= modifiers.shift;
        }
    }
    (notches != 0).then_some((notches, coarse))
}

#[cfg(test)]
mod tests {
    use charter_core::{RulesetPlugin, ValidDocument};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use serde_json::json;

    use super::*;

    #[derive(Debug)]
    struct TestRuleset;

    impl RulesetPlugin for TestRuleset {
        fn id(&self) -> &'static str {
            "test"
        }

        fn check(&self, _: &ValidDocument) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    pub(super) struct WindowTest {
        pub(super) song: Song,
        pub(super) window: TimingWindow,
        pub(super) commands: Vec<Command>,
        pub(super) onsets: Vec<Onset>,
    }

    impl WindowTest {
        fn ui(&mut self, ui: &mut egui::Ui) {
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

    pub(super) fn timing_harness() -> Harness<'static, WindowTest> {
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

    pub(super) fn row_point(harness: &Harness<'_, WindowTest>, tick: i32) -> Pos2 {
        let rect = harness.get_by_label("Measure 0").rect();
        let fraction = f64::from(tick) / 3840.0;
        #[allow(clippy::cast_possible_truncation)] // a fraction of one row
        let across = ((fraction + rows::LEAD_IN) / (1.0 + rows::LEAD_IN)) as f32;
        egui::pos2(rect.left() + across * rect.width(), rect.top() + 30.0)
    }

    fn pointer(harness: &Harness<'_, WindowTest>, pos: Pos2, pressed: bool, modifiers: Modifiers) {
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

    fn drag(harness: &mut Harness<'_, WindowTest>, from: Pos2, to: Pos2, modifiers: Modifiers) {
        harness.hover_at(from);
        harness.event(egui::Event::ModifiersChanged(modifiers));
        pointer(harness, from, true, modifiers);
        for step in 1..=4 {
            #[allow(clippy::cast_precision_loss)] // four pointer steps
            harness.hover_at(from.lerp(to, step as f32 / 4.0));
        }
        pointer(harness, to, false, modifiers);
        harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
        harness.step();
    }

    #[test]
    fn a_row_click_emits_a_sync_and_an_audition_before_the_new_anchor() {
        let mut harness = timing_harness();
        assert_eq!(harness.state().song.read_only(), None);
        let pos = row_point(&harness, 2880);
        click(&mut harness, pos, Modifiers::NONE);
        let [
            Command::Edit(Edit::Sync { tick, seconds }),
            Command::Audition(from),
        ] = harness.state().commands.as_slice()
        else {
            panic!("expected sync and audition: {:?}", harness.state().commands);
        };
        assert_eq!(*tick, Tick::new(2880));
        assert!((seconds - 1.5).abs() < 1e-4);
        assert!((from - 1.0).abs() < 1e-4);
        assert_eq!(harness.state().window.gesture, Gesture::Idle);
    }

    #[test]
    fn dragging_a_beat_emits_one_move_and_focuses_its_destination() {
        let mut harness = timing_harness();
        harness.state_mut().window.grid = Grid {
            resolution: NonZeroU16::new(8).unwrap(),
        };
        harness.run();
        let from = row_point(&harness, 960);
        let to = row_point(&harness, 1440);
        drag(&mut harness, from, to, Modifiers::NONE);
        assert_eq!(
            harness.state().commands,
            [Command::Edit(Edit::MoveAnchors(vec![(
                Tick::new(960),
                Tick::new(1440)
            )]))]
        );
        assert_eq!(harness.state().window.gesture, Gesture::Idle);
    }

    #[test]
    fn ctrl_dragging_an_anchor_emits_an_audio_nudge_without_changing_its_beat() {
        let mut harness = timing_harness();
        let from = row_point(&harness, 960);
        let to = row_point(&harness, 1200);
        drag(&mut harness, from, to, Modifiers::COMMAND);
        let [Command::Edit(Edit::Nudge { ticks, by, merge })] = harness.state().commands.as_slice()
        else {
            panic!("expected audio nudge: {:?}", harness.state().commands);
        };
        assert_eq!(ticks, &[Tick::new(960)]);
        assert!((by - 0.125).abs() < 1e-4);
        assert!(!merge);
        assert_eq!(harness.state().window.focus, Some(Tick::new(960)));
    }

    #[test]
    fn shift_click_selects_the_run_and_delete_emits_one_unsync() {
        let mut harness = timing_harness();
        let first = row_point(&harness, 0);
        let last = row_point(&harness, 1920);
        click(&mut harness, first, Modifiers::NONE);
        click(&mut harness, last, Modifiers::SHIFT);
        assert_eq!(
            harness.state().window.selection,
            [Tick::new(0), Tick::new(960), Tick::new(1920)]
        );
        harness.key_press(Key::Delete);
        harness.run();
        assert_eq!(
            harness.state().commands,
            [Command::Edit(Edit::Unsync(vec![
                Tick::new(0),
                Tick::new(960),
                Tick::new(1920)
            ]))]
        );
    }

    #[test]
    fn a_band_selects_only_the_anchors_between_its_endpoints() {
        let mut harness = timing_harness();
        let from = row_point(&harness, 400);
        let to = row_point(&harness, 1500);
        drag(&mut harness, from, to, Modifiers::NONE);
        assert_eq!(harness.state().window.selection, [Tick::new(960)]);
        assert_eq!(harness.state().commands, []);
        assert_eq!(harness.state().window.gesture, Gesture::Idle);
    }

    #[test]
    fn nudges_merge_for_the_same_selection_until_a_refusal_resets_them() {
        let mut harness = timing_harness();
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
                    by: 0.001,
                    merge: false
                }),
                Command::Edit(Edit::Nudge {
                    ticks: vec![Tick::new(960)],
                    by: 0.001,
                    merge: true
                })
            ]
        );
        let edit = Edit::Nudge {
            ticks: vec![Tick::new(960)],
            by: 0.001,
            merge: true,
        };
        let state = harness.state_mut();
        let timing = state.song.timing();
        let now = state.window.last_nudge.as_ref().unwrap().1;
        state
            .window
            .refused(&edit, &Refusal::Crosses(Tick::new(1920)), timing, now);
        harness.key_press(Key::ArrowRight);
        harness.step();
        assert_eq!(
            harness.state().commands.last(),
            Some(&Command::Edit(Edit::Nudge {
                ticks: vec![Tick::new(960)],
                by: 0.001,
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
    fn dragging_into_a_neighbour_stops_short_and_pulses_that_anchor() {
        let mut harness = timing_harness();
        let from = row_point(&harness, 960);
        let to = row_point(&harness, 2400);
        drag(&mut harness, from, to, Modifiers::NONE);
        assert_eq!(
            harness.state().commands,
            [Command::Edit(Edit::MoveAnchors(vec![(
                Tick::new(960),
                Tick::new(1919)
            )]))]
        );
        assert!(matches!(
            harness.state().window.pulse,
            Some((tick, _)) if tick == Tick::new(1920)
        ));
    }

    #[test]
    fn escape_cancels_a_drag_before_clearing_the_selection() {
        let mut harness = timing_harness();
        let at = row_point(&harness, 960);
        click(&mut harness, at, Modifiers::NONE);
        harness.state_mut().window.gesture = Gesture::DragBeat {
            from: Tick::new(960),
            to: Tick::new(1440),
            origin: 0.25,
        };
        harness.key_press(Key::Delete);
        harness.run();
        assert_eq!(harness.state().commands, []);
        harness.key_press(Key::Escape);
        harness.run();
        assert_eq!(harness.state().window.gesture, Gesture::Idle);
        assert_eq!(harness.state().window.selection, [Tick::new(960)]);
        harness.key_press(Key::Escape);
        harness.run();
        assert_eq!(harness.state().window.selection, []);
    }

    #[test]
    fn adding_an_anchor_snaps_to_an_onset_unless_shift_is_held() {
        let mut snapped = timing_harness();
        snapped.state_mut().onsets = vec![Onset {
            seconds: 1.512,
            strength: 1.0,
        }];
        snapped.run();
        let at = row_point(&snapped, 2880);
        click(&mut snapped, at, Modifiers::NONE);
        let [Command::Edit(Edit::Sync { seconds, .. }), ..] = snapped.state().commands.as_slice()
        else {
            panic!("expected sync: {:?}", snapped.state().commands);
        };
        assert!((seconds - 1.512).abs() < 1e-9);

        let mut plain = timing_harness();
        plain.state_mut().onsets = vec![Onset {
            seconds: 1.512,
            strength: 1.0,
        }];
        plain.run();
        let at = row_point(&plain, 2880);
        click(&mut plain, at, Modifiers::SHIFT);
        let [Command::Edit(Edit::Sync { seconds, .. }), ..] = plain.state().commands.as_slice()
        else {
            panic!("expected sync: {:?}", plain.state().commands);
        };
        assert!((seconds - 1.5).abs() < 1e-4);
    }

    #[test]
    fn adding_too_near_an_anchor_refuses_and_identifies_the_blocker() {
        let mut harness = timing_harness();
        let at = row_point(&harness, 1030);
        click(&mut harness, at, Modifiers::NONE);
        assert_eq!(harness.state().commands, []);
        assert!(matches!(
            harness.state().window.pulse,
            Some((tick, _)) if tick == Tick::new(960)
        ));
        assert!(
            harness
                .query_by_label_contains("Too close to the anchor")
                .is_some()
        );
    }

    #[test]
    fn ctrl_dragging_past_a_neighbour_keeps_a_millisecond_between_them() {
        let mut harness = timing_harness();
        let from = row_point(&harness, 960);
        let to = row_point(&harness, 2400);
        drag(&mut harness, from, to, Modifiers::COMMAND);
        let [Command::Edit(Edit::Nudge { ticks, by, merge })] = harness.state().commands.as_slice()
        else {
            panic!("expected constrained nudge: {:?}", harness.state().commands);
        };
        assert_eq!(ticks, &[Tick::new(960)]);
        assert!((by - 0.499).abs() < 1e-5);
        assert!(!merge);
        assert!(matches!(
            harness.state().window.pulse,
            Some((tick, _)) if tick == Tick::new(1920)
        ));
    }

    #[test]
    fn selecting_two_anchors_loops_their_span_and_layout_switch_keeps_the_focus() {
        let mut harness = timing_harness();
        let first = row_point(&harness, 0);
        let last = row_point(&harness, 1920);
        click(&mut harness, first, Modifiers::NONE);
        click(&mut harness, last, Modifiers::SHIFT);
        harness.key_press_modifiers(Modifiers::SHIFT, Key::L);
        harness.step();
        assert_eq!(
            harness.state().commands,
            [Command::Loop {
                from: Tick::new(0),
                to: Tick::new(1920)
            }]
        );
        harness.key_press(Key::V);
        harness.step();
        assert_eq!(harness.state().window.layout, Layout::Time);
        assert_eq!(harness.state().window.scroll_to, None);
        assert!(harness.query_by_label("Row at 0.0 s").is_some());
    }

    #[test]
    fn an_anchor_context_menu_keeps_its_target_while_the_pointer_moves_to_a_button() {
        let mut harness = timing_harness();
        let at = row_point(&harness, 960);
        harness.hover_at(at);
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        harness.step();
        harness.get_by_label("Double tempo").click();
        harness.step();
        assert_eq!(
            harness.state().commands,
            [Command::Edit(Edit::Scale {
                ticks: vec![Tick::new(960)],
                double: true
            })]
        );
    }

    #[test]
    fn dragging_one_of_several_selected_anchors_moves_the_whole_run() {
        let mut harness = timing_harness();
        harness.state_mut().window.grid = Grid {
            resolution: NonZeroU16::new(8).unwrap(),
        };
        harness.run();
        let first = row_point(&harness, 0);
        let second = row_point(&harness, 960);
        click(&mut harness, first, Modifiers::NONE);
        click(&mut harness, second, Modifiers::SHIFT);
        let to = row_point(&harness, 1440);
        drag(&mut harness, second, to, Modifiers::NONE);
        assert_eq!(
            harness.state().commands,
            [Command::Edit(Edit::MoveAnchors(vec![
                (Tick::new(0), Tick::new(480)),
                (Tick::new(960), Tick::new(1440))
            ]))]
        );
    }

    #[test]
    fn a_key_after_clicking_outside_the_window_does_not_edit_the_song() {
        let mut harness = timing_harness();
        let at = row_point(&harness, 960);
        click(&mut harness, at, Modifiers::NONE);
        assert!(harness.state().window.has_keyboard());
        click(&mut harness, egui::pos2(20.0, 20.0), Modifiers::NONE);
        assert!(!harness.state().window.has_keyboard());
        harness.key_press(Key::Delete);
        harness.step();
        assert_eq!(harness.state().commands, []);
    }

    #[test]
    fn only_a_focused_existing_anchor_can_be_auditioned_with_enter() {
        let mut harness = timing_harness();
        harness.key_press(Key::Enter);
        harness.step();
        assert_eq!(harness.state().commands, []);
        let at = row_point(&harness, 960);
        click(&mut harness, at, Modifiers::NONE);
        harness.key_press(Key::Enter);
        harness.step();
        assert_eq!(harness.state().commands, [Command::Audition(0.0)]);
    }

    #[test]
    fn nudge_merge_expires_at_the_window_and_is_scoped_to_the_same_targets() {
        let mut window = TimingWindow::default();
        let mut commands = Vec::new();
        window.focus = Some(Tick::new(960));
        window.nudge(0.001, 1.0, &mut commands);
        window.nudge(0.001, 1.8, &mut commands);
        window.selection = vec![Tick::new(0), Tick::new(960)];
        window.nudge(0.001, 1.9, &mut commands);
        assert_eq!(
            commands,
            [
                Command::Edit(Edit::Nudge {
                    ticks: vec![Tick::new(960)],
                    by: 0.001,
                    merge: false
                }),
                Command::Edit(Edit::Nudge {
                    ticks: vec![Tick::new(960)],
                    by: 0.001,
                    merge: false
                }),
                Command::Edit(Edit::Nudge {
                    ticks: vec![Tick::new(0), Tick::new(960)],
                    by: 0.001,
                    merge: false
                })
            ]
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
    fn timing_keys_fit_beside_the_inspector_and_leave_room_for_rows() {
        let ctx = egui::Context::default();
        let mut used = egui::Rect::NOTHING;
        let mut below = 0.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(420.0, 320.0),
            )),
            ..egui::RawInput::default()
        };
        let mut output = ctx.run_ui(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                help(ui);
                used = ui.min_rect();
                below = ui.cursor().top();
            });
        });
        output.textures_delta.clear();
        assert!(used.width() <= 420.0, "help is too wide: {used:?}");
        assert!(below <= 248.0, "help hides the rows: {below}");
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
    fn keys_ask_for_what_the_specification_gives_them() {
        let none = Modifiers::NONE;
        assert_eq!(intent(Key::ArrowRight, none), Some(Intent::Nudge(0.001)));
        assert_eq!(
            intent(Key::ArrowLeft, Modifiers::SHIFT),
            Some(Intent::Nudge(-0.0001))
        );
        assert_eq!(
            intent(Key::ArrowRight, Modifiers::COMMAND),
            Some(Intent::Nudge(0.010))
        );
        assert_eq!(intent(Key::ArrowUp, none), Some(Intent::Move(-1)));
        assert_eq!(intent(Key::Num1, none), Some(Intent::Grid(0)));
        assert_eq!(intent(Key::Num0, none), Some(Intent::Grid(9)));
        assert_eq!(
            intent(Key::F, Modifiers::SHIFT),
            Some(Intent::Fit { half_bpm: true })
        );
        assert_eq!(
            intent(Key::D, Modifiers::SHIFT),
            Some(Intent::Scale { double: false })
        );
        assert_eq!(intent(Key::A, Modifiers::COMMAND), Some(Intent::SelectAll));
        assert_eq!(intent(Key::A, none), Some(Intent::AddAtPlayhead));
        assert_eq!(intent(Key::N, Modifiers::SHIFT), Some(Intent::Issue(false)));
        assert_eq!(
            intent(Key::Minus, Modifiers::SHIFT),
            Some(Intent::Tail(-1.0))
        );
        assert_eq!(intent(Key::Q, none), None);
    }

    #[test]
    fn ctrl_wheel_notches_add_up_and_shift_makes_them_fine() {
        let wheel = |up: f32, modifiers| egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(0.0, up),
            modifiers,
            phase: egui::TouchPhase::Move,
        };
        let ctrl = Modifiers::COMMAND;
        assert_eq!(nudge_notches(&[wheel(1.0, Modifiers::NONE)]), None);
        let fine = [wheel(1.0, ctrl), wheel(1.0, ctrl), wheel(-1.0, ctrl)];
        assert_eq!(nudge_notches(&fine), Some((1, false)));
        let coarse = [wheel(-1.0, ctrl.plus(Modifiers::SHIFT))];
        assert_eq!(nudge_notches(&coarse), Some((-1, true)));
    }

    #[test]
    fn issue_keys_wrap_in_both_directions_and_focus_the_named_anchor() {
        let mut harness = timing_harness();
        harness.state_mut().onsets = [0.012, 0.512, 1.012]
            .map(|seconds| Onset {
                seconds,
                strength: 1.0,
            })
            .to_vec();
        harness.run();
        let count = harness.state().window.cache.as_ref().unwrap().issues.len();
        assert!(count >= 3, "expected displaced anchors to be issues");
        harness.key_press(Key::N);
        harness.run();
        assert_eq!(harness.state().window.issue, Some(0));
        assert_eq!(harness.state().window.focus, Some(Tick::new(0)));
        harness.key_press_modifiers(Modifiers::SHIFT, Key::N);
        harness.run();
        assert_eq!(harness.state().window.issue, Some(count - 1));
        assert_eq!(harness.state().window.focus, Some(Tick::new(1920)));
        harness.key_press(Key::N);
        harness.run();
        assert_eq!(harness.state().window.issue, Some(0));
    }

    #[test]
    fn playback_and_metronome_keys_work_on_a_read_only_song_but_edits_do_not() {
        let mut harness = timing_harness();
        let document = harness.state().song.document().clone();
        harness.state_mut().song = Song::new(document, &[]);
        harness.run();
        let anchor = row_point(&harness, 960);
        click(&mut harness, anchor, Modifiers::NONE);
        harness.key_press(Key::ArrowRight);
        harness.key_press(Key::Delete);
        harness.key_press(Key::Space);
        harness.key_press(Key::R);
        harness.key_press(Key::C);
        harness.step();
        assert_eq!(
            harness.state().commands,
            [
                Command::TogglePlay,
                Command::ToggleRate,
                Command::ToggleMetronome
            ]
        );
        harness.key_press_modifiers(Modifiers::SHIFT, Key::C);
        harness.step();
        assert!(
            harness
                .state()
                .commands
                .ends_with(&[Command::MuteMetronome(true), Command::MuteMetronome(false)])
        );
        harness.event(egui::Event::Key {
            key: Key::C,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        harness.step();
        assert_eq!(
            harness.state().commands.last(),
            Some(&Command::MuteMetronome(false))
        );
    }

    #[test]
    fn the_anchor_menu_can_audition_select_a_run_and_remove_that_run() {
        let mut harness = timing_harness();
        let at = row_point(&harness, 960);
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        harness.step();
        harness.get_by_label("Select to the end").click();
        harness.step();
        assert_eq!(
            harness.state().window.selection,
            [Tick::new(960), Tick::new(1920)]
        );
        harness.hover_at(at);
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        harness.step();
        harness.get_by_label("Remove").click();
        harness.step();
        assert_eq!(
            harness.state().commands,
            [Command::Edit(Edit::Unsync(vec![
                Tick::new(960),
                Tick::new(1920)
            ]))]
        );
    }

    #[test]
    fn a_fit_stays_a_preview_until_enter_and_escape_discards_it() {
        let mut harness = timing_harness();
        harness.state_mut().onsets = [0.0, 0.5, 1.0, 1.5, 2.0]
            .map(|seconds| Onset {
                seconds,
                strength: 1.0,
            })
            .to_vec();
        harness.run();
        let at = row_point(&harness, 960);
        click(&mut harness, at, Modifiers::NONE);
        harness.key_press(Key::F);
        harness.step();
        assert!(harness.state().window.preview.is_some());
        assert_eq!(harness.state().commands, []);
        harness.key_press(Key::Escape);
        harness.step();
        assert!(harness.state().window.preview.is_none());
        assert_eq!(harness.state().commands, []);
        harness.key_press(Key::F);
        harness.step();
        harness.key_press(Key::Enter);
        harness.step();
        assert!(matches!(
            harness.state().commands.as_slice(),
            [Command::Edit(Edit::Repin { .. })]
        ));
        assert!(harness.state().window.preview.is_none());
    }
}
