//! Row painting for the timing window.

use egui::{Sense, vec2};

use crate::pointer::{Gesture, anchor_near, ghost, reach, release, row_input};
use crate::rows::{self, Anchor, Row, Scene};

use super::{
    Bpm, Command, Frame, Inputs, Issue, Layout, PULSE, Pos2, ROW, Tick, TimingWindow, style,
};

/// The rows, and every press and drag on them. Says whether the pointer is over an anchor.
#[expect(clippy::too_many_lines, reason = "draws rows then their drag state")]
pub(crate) fn rows(
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
        let position = frame.bars.position(rows::float_from_i64(i64::from(tick)));
        spans
            .iter()
            .position(|span| position >= span.start && position < span.to)
    };
    let target = window
        .scroll_to
        .take()
        .or_else(|| (window.follow && inputs.playing).then_some(frame.cursor));
    let mut area = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource {
            drag: egui::scroll_area::DragScroll::Never,
            ..egui::scroll_area::ScrollSource::ALL
        });
    if let Some(index) = target.and_then(row_of) {
        let offset = rows::float_from_usize(index.saturating_sub(1)) * pitch;
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
                    let through = rows::float_to_f32((frame.now - at) / PULSE);
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
            let key = rows::float_to_i64((span.start * 100.0).round());
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
                Issue::Bar { .. }
                | Issue::Short { .. }
                | Issue::Jump { .. }
                | Issue::Anchor { .. }
                | Issue::Metre { .. } => None,
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

/// The tempo of the BPM change on `tick`, as the song holds it: stops and delays between
/// anchors take time the tempo does not.
fn tempo_at(frame: &Frame<'_>, tick: Tick) -> Option<Bpm> {
    frame.timing.bpm_at(tick)
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
                keyboard::help(ui);
                used = ui.min_rect();
                below = ui.cursor().top();
            });
        });
        output.textures_delta.clear();
        assert!(used.width() <= 420.0, "help is too wide: {used:?}");
        assert!(below <= 248.0, "help hides the rows: {below}");
    }
}
