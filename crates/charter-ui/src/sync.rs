//! The sync window, after Tempora: the song's audio one measure a row, laid out by beat rather
//! than by time, so hits that fall on the grid line up down the rows and hits that do not stand
//! out. Syncing is dragging a hit onto the grid line it belongs on. Every BPM change is an
//! anchor there, and moving one keeps its neighbours where they are in the audio.

use charter_audio::Peaks;
use charter_core::{Measure, Tick, Ticks, Timing, TimingKind, Valid};
use charter_editor::Song;
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};

use crate::notefield::latest_tick_by;
use crate::style;
use crate::window::Action;

/// How tall a measure's row is.
const ROW: f32 = 64.0;
/// How wide the measure numbers down the left are.
const GUTTER: f32 = 40.0;
/// How far from an anchor's line, in points, a click still takes it.
const GRAB: f32 = 6.0;
/// How far one Ctrl+wheel notch moves an anchor in the audio, and with Shift too: Tempora's.
const NUDGE: f64 = 0.002;
const COARSE_NUDGE: f64 = 0.010;
/// A beat's note: grid lines of it draw brighter than the snap's.
const QUARTER: std::num::NonZeroU16 = std::num::NonZeroU16::new(4).unwrap();
/// Measures shown past the song's last object, room to sync an outro.
const TAIL: usize = 4;

/// Whether the sync window shows, and the hit being dragged in it.
#[derive(Debug, Default)]
pub struct Sync {
    pub open: bool,
    /// Where in the audio the hit being dragged was picked up, in seconds.
    grabbed: Option<f64>,
}

/// A measure's row: which measure, and where its beats lie across the screen.
#[derive(Clone, Copy, Debug)]
struct Row {
    measure: Measure,
    rect: Rect,
}

impl Row {
    fn x(&self, tick: Tick) -> f32 {
        #[allow(clippy::cast_possible_truncation)] // a fraction of a row
        let across = self.measure.fraction(tick) as f32;
        self.rect.left() + across * self.rect.width()
    }

    fn tick_at(&self, x: f32) -> Tick {
        let across = (x - self.rect.left()) / self.rect.width();
        self.measure.tick_at(f64::from(across))
    }

    /// The grid line `step` ticks apart nearest `x`, which may start the next measure.
    fn snapped(&self, x: f32, step: Ticks) -> Tick {
        let near = self.tick_at(x);
        let below = near
            .next_on(step)
            .and_then(|next| next.previous_on(step))
            .unwrap_or(near);
        let above = below.next_on(step).unwrap_or(below);
        if (self.x(above) - x).abs() < (self.x(below) - x).abs() {
            above
        } else {
            below
        }
    }
}

/// Draws the sync window for `song`, over the audio in `peaks` when it has loaded, and answers
/// its input with `actions`.
pub(crate) fn show(
    ctx: &egui::Context,
    song: Option<&Song>,
    peaks: Option<&Peaks>,
    sync: &mut Sync,
    playing: bool,
    actions: &mut Vec<Action>,
) {
    let mut open = sync.open;
    egui::Window::new("Sync")
        .open(&mut open)
        .default_size([880.0, 560.0])
        .pivot(Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .min_size([420.0, 240.0])
        .collapsible(false)
        .show(ctx, |ui| {
            let Some(song) = song else {
                ui.weak("Open a song to sync it to its audio.");
                return;
            };
            header(ui, song, peaks);
            ui.separator();
            rows(ui, song, peaks, sync, playing, actions);
        });
    sync.open = open;
    if !sync.open {
        sync.grabbed = None;
    }
}

/// What the window does, and how.
fn header(ui: &mut egui::Ui, song: &Song, peaks: Option<&Peaks>) {
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("Snap 1/{}", song.snap()));
        ui.weak("·");
        ui.weak(
            "Drag a hit onto its grid line. Click adds an anchor, double-click removes one, \
             Ctrl+wheel nudges the nearest by 2 ms (Shift: 10 ms), right-click plays from there.",
        );
    });
    if peaks.is_none() {
        ui.colored_label(
            style::WARN,
            "The waveform shows once the song's audio loads.",
        );
    }
}

/// The measures with audio or objects in them, one a row.
fn measures(song: &Song) -> Vec<Measure> {
    let timing = song.timing();
    let first = latest_tick_by(timing, 0.0).min(Tick::new(0));
    let last = song.end().max(song.cursor());
    let mut shown: Vec<Measure> = timing
        .measures_from(first)
        .take_while(|measure| measure.start <= last)
        .collect();
    shown.extend(timing.measures_from(last).skip(1).take(TAIL));
    shown
}

fn rows(
    ui: &mut egui::Ui,
    song: &Song,
    peaks: Option<&Peaks>,
    sync: &mut Sync,
    playing: bool,
    actions: &mut Vec<Action>,
) {
    let timing = song.timing();
    let step = song
        .document()
        .document()
        .ticks_per_beat
        .note_length(song.snap());
    let beat = song
        .document()
        .document()
        .ticks_per_beat
        .note_length(QUARTER);
    let measures = measures(song);
    let editable = song.read_only().is_none();
    let mut laid = Vec::new();
    let mut released = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource {
            drag: egui::scroll_area::DragScroll::Never,
            ..egui::scroll_area::ScrollSource::ALL
        })
        .show_rows(ui, ROW, measures.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for measure in measures.get(range).unwrap_or_default() {
                let (whole, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
                let mut rect = whole;
                rect.min.x += GUTTER;
                let row = Row {
                    measure: *measure,
                    rect,
                };
                let id = ui.id().with(("sync row", measure.number));
                let response = ui.interact(rect, id, Sense::click_and_drag());
                let label = format!("Measure {}", measure.number);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &label)
                });
                let painter = ui.painter_at(whole);
                painter.text(
                    pos2(whole.left() + GUTTER - 8.0, whole.top() + 4.0),
                    Align2::RIGHT_TOP,
                    measure.number.to_string(),
                    FontId::monospace(12.0),
                    style::WEAK_TEXT,
                );
                draw_row(&painter, &row, song, peaks, (step, beat));
                if editable {
                    answer(&response, &row, timing, step, sync, playing, actions);
                    if response.drag_stopped() {
                        released = ui.input(|input| input.pointer.latest_pos());
                    }
                }
                laid.push(row);
            }
        });
    if let (Some(grabbed), Some(step)) = (sync.grabbed, step) {
        let hovered = ui.input(|input| input.pointer.latest_pos());
        if let Some(pos) = released {
            sync.grabbed = None;
            if let Some(row) = laid.iter().find(|row| row.rect.y_range().contains(pos.y)) {
                let tick = row.snapped(pos.x, step);
                actions.push(Action::Sync {
                    tick,
                    seconds: grabbed,
                });
            }
        } else if let Some(pos) = hovered
            && let Some(row) = laid.iter().find(|row| row.rect.contains(pos))
        {
            let x = row.x(row.snapped(pos.x, step));
            let painter = ui.painter();
            painter.vline(x, row.rect.y_range(), Stroke::new(2.0, style::ACCENT));
        }
    }
}

/// The row's input: a drag picks a hit up, a click adds an anchor, a double-click removes one,
/// Ctrl+wheel nudges one and a right-click plays.
fn answer(
    response: &egui::Response,
    row: &Row,
    timing: Valid<'_, Timing>,
    step: Option<Ticks>,
    sync: &mut Sync,
    playing: bool,
    actions: &mut Vec<Action>,
) {
    let pointer = response.interact_pointer_pos().or(response.hover_pos());
    let anchor = pointer.and_then(|pos| {
        anchors_in(row, timing)
            .into_iter()
            .map(|(tick, _)| (tick, (row.x(tick) - pos.x).abs()))
            .filter(|(_, off)| *off <= GRAB)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(tick, _)| tick)
    });
    let now = |tick: Tick| timing.time_at(tick).seconds();
    // Where the button went down, not where the drag was first noticed a few points on.
    if response.drag_started()
        && let Some(pos) = response.ctx.input(|input| input.pointer.press_origin())
    {
        sync.grabbed = Some(now(row.tick_at(pos.x)));
    }
    if response.double_clicked() {
        if let Some(tick) = anchor {
            actions.push(Action::Unsync(tick));
        }
    } else if response.clicked()
        && anchor.is_none()
        && let (Some(pos), Some(step)) = (pointer, step)
    {
        let tick = row.snapped(pos.x, step);
        actions.push(Action::Sync {
            tick,
            seconds: now(tick),
        });
    }
    if response.secondary_clicked()
        && let (Some(pos), Some(step)) = (pointer, step)
    {
        actions.push(Action::JumpTo(row.snapped(pos.x, step)));
        if !playing {
            actions.push(Action::TogglePlay);
        }
    }
    if response.hovered()
        && let Some(pos) = pointer
    {
        let notches = response.ctx.input(|input| nudge_notches(&input.events));
        let nearest = anchors_in(row, timing).into_iter().min_by(|a, b| {
            (row.x(a.0) - pos.x)
                .abs()
                .total_cmp(&(row.x(b.0) - pos.x).abs())
        });
        if let (Some((tick, _)), Some((notches, coarse))) = (nearest, notches) {
            let by = if coarse { COARSE_NUDGE } else { NUDGE };
            actions.push(Action::Sync {
                tick,
                seconds: now(tick) + by * f64::from(notches),
            });
        }
    }
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

/// The BPM changes in the row's measure, by tick and tempo.
fn anchors_in(row: &Row, timing: Valid<'_, Timing>) -> Vec<(Tick, charter_core::Bpm)> {
    let end = row.measure.start.checked_add(row.measure.length);
    timing
        .events
        .iter()
        .filter(|event| event.tick >= row.measure.start && end.is_none_or(|end| event.tick < end))
        .filter_map(|event| match event.kind {
            TimingKind::Bpm(bpm) => Some((event.tick, bpm)),
            _ => None,
        })
        .collect()
}

/// A row's waveform, grid, anchors and cursor.
fn draw_row(
    painter: &egui::Painter,
    row: &Row,
    song: &Song,
    peaks: Option<&Peaks>,
    (step, beat): (Option<Ticks>, Option<Ticks>),
) {
    let timing = song.timing();
    let rect = row.rect;
    painter.rect_filled(rect, 2.0, style::WAVE_BG);
    if let Some(peaks) = peaks {
        waveform(painter, row, timing, peaks);
    }
    let end = row.measure.start.checked_add(row.measure.length);
    let lines = |step: Ticks| {
        std::iter::successors(Some(row.measure.start), move |tick| tick.next_on(step))
            .take_while(move |tick| end.is_none_or(|end| *tick < end))
    };
    if let Some(step) = step {
        for tick in lines(step) {
            painter.vline(
                row.x(tick),
                rect.y_range(),
                Stroke::new(1.0, style::BEAT_LINE),
            );
        }
    }
    if let Some(beat) = beat {
        for tick in lines(beat) {
            painter.vline(
                row.x(tick),
                rect.y_range(),
                Stroke::new(1.0, style::DIVIDER),
            );
        }
    }
    painter.vline(
        rect.left(),
        rect.y_range(),
        Stroke::new(1.5, style::MEASURE_LINE),
    );
    for (tick, bpm) in anchors_in(row, timing) {
        let x = row.x(tick);
        painter.vline(x, rect.y_range(), Stroke::new(2.0, style::TIMING_MARK));
        // On a chip of its own, since the waveform runs under it.
        let galley = painter.layout_no_wrap(
            format!("{bpm}"),
            FontId::monospace(11.0),
            style::TIMING_MARK,
        );
        let at = pos2(x + 3.0, rect.top() + 3.0);
        let chip = Rect::from_min_size(at, galley.size()).expand2(vec2(3.0, 1.0));
        painter.rect_filled(chip.translate(vec2(3.0, 1.0)), 2.0, style::WAVE_BG);
        painter.galley(at + vec2(3.0, 1.0), galley, style::TIMING_MARK);
    }
    let cursor = song.cursor();
    if cursor >= row.measure.start && end.is_none_or(|end| cursor < end) {
        painter.vline(
            row.x(cursor),
            rect.y_range(),
            Stroke::new(1.5, style::NOW_LINE),
        );
    }
}

/// The audio across the row, one column a point, each spanning the audio its beats cover.
fn waveform(painter: &egui::Painter, row: &Row, timing: Valid<'_, Timing>, peaks: &Peaks) {
    let rect = row.rect;
    let (middle, half) = (rect.center().y, rect.height() / 2.0 - 4.0);
    let mut x = rect.left().floor();
    let mut from = timing.time_at(row.tick_at(x)).seconds();
    while x < rect.right() {
        let to = timing.time_at(row.tick_at(x + 1.0)).seconds();
        let extent = peaks.between(from.min(to), from.max(to));
        from = to;
        if extent.high - extent.low >= 0.004 {
            let top = middle - extent.high.clamp(-1.0, 1.0) * half;
            let bottom = middle - extent.low.clamp(-1.0, 1.0) * half;
            painter.vline(
                x + 0.5,
                egui::Rangef::new(top, bottom),
                Stroke::new(1.0, WAVE),
            );
        }
        x += 1.0;
    }
}

/// The waveform a touch brighter than the notefield's, since it is all this window shows.
const WAVE: Color32 = Color32::from_rgb(118, 158, 210);

#[cfg(test)]
mod tests {
    use charter_core::{TicksPerBeat, TimeSignature};

    use super::*;

    fn row() -> Row {
        let measure = Measure {
            number: 1,
            start: Tick::new(3840),
            length: Ticks::new(3840).unwrap(),
            meter: TimeSignature::COMMON,
        };
        let rect = Rect::from_min_size(pos2(100.0, 0.0), vec2(400.0, ROW));
        Row { measure, rect }
    }

    fn sixteenth() -> Ticks {
        let resolution = TicksPerBeat::new(960).unwrap();
        resolution
            .note_length(std::num::NonZeroU16::new(16).unwrap())
            .unwrap()
    }

    #[test]
    fn a_row_lays_its_measure_out_by_beat() {
        let row = row();
        assert!((row.x(Tick::new(4800)) - 200.0).abs() < 1e-3);
        assert_eq!(row.tick_at(300.0), Tick::new(5760));
    }

    #[test]
    fn a_drop_snaps_to_the_nearest_grid_line_even_the_next_measures() {
        let row = row();
        // A sixteenth is 25 points across this row.
        assert_eq!(row.snapped(112.0, sixteenth()), Tick::new(3840));
        assert_eq!(row.snapped(113.0, sixteenth()), Tick::new(4080));
        assert_eq!(row.snapped(499.0, sixteenth()), Tick::new(7680));
    }

    #[test]
    fn ctrl_wheel_notches_add_up_and_shift_makes_them_coarse() {
        let wheel = |up: f32, modifiers| egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(0.0, up),
            modifiers,
            phase: egui::TouchPhase::Move,
        };
        let ctrl = egui::Modifiers::COMMAND;
        let plain = [wheel(1.0, egui::Modifiers::NONE)];
        assert_eq!(nudge_notches(&plain), None);
        let fine = [wheel(1.0, ctrl), wheel(1.0, ctrl), wheel(-1.0, ctrl)];
        assert_eq!(nudge_notches(&fine), Some((1, false)));
        let coarse = [wheel(-1.0, ctrl.plus(egui::Modifiers::SHIFT))];
        assert_eq!(nudge_notches(&coarse), Some((-1, true)));
    }
}
