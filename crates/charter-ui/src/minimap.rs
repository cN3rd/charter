//! The minimap: the whole open chart squeezed into a strip beside the notefield, its notes by lane
//! and its sections across, with a box over the stretch the notefield shows. A click or a drag on
//! it moves the cursor there.

use charter_core::{Kind, MarkerKind, Position, ResolvedTime};
use charter_editor::Song;
use egui::{Rect, Sense, Stroke, StrokeKind, Ui};

use crate::Action;
use crate::notefield::{kind_colours, nearest_line};
use crate::style;

/// How wide the minimap is.
pub(crate) const WIDTH: f32 = 44.0;
/// The shortest a note draws, so a tap still shows.
const NOTE_H: f32 = 1.5;

/// Where times sit along the minimap: all of the song, `from` to `to` seconds, top to bottom, or
/// bottom to top when `reversed`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Map {
    /// The screen rectangle receiving the whole-song strip.
    rect: Rect,
    /// The earliest time drawn in the strip, in seconds.
    from: f64,
    /// The latest time drawn in the strip, in seconds.
    to: f64,
    /// Whether later times sit above earlier times.
    reversed: bool,
}

impl Map {
    /// The map of `song` in `rect`: from its start, or its earliest note if that comes before,
    /// to the end of its audio, `length` seconds, or its last note or marker if later.
    fn of(song: &Song, rect: Rect, length: Option<f64>, reversed: bool) -> Self {
        let timing = song.timing();
        let chart = song.chart();
        let objects = chart.iter().flat_map(|chart| chart.objects.iter());
        let starts = objects.clone().map(|object| object.tick);
        let ends = objects.filter_map(|object| object.shape.end(object.tick));
        let markers = song.document().document().markers.iter().map(|m| m.tick);
        let seconds = |tick| timing.time_at(tick).seconds();
        let from = starts.clone().map(seconds).fold(0.0, f64::min);
        let to = ends
            .chain(starts)
            .chain(markers)
            .map(seconds)
            .fold(length.unwrap_or(0.0), f64::max)
            .max(from + 1.0);
        Self {
            rect,
            from,
            to,
            reversed,
        }
    }

    /// Where audio `seconds` into the song sits.
    fn y_of(self, seconds: f64) -> f32 {
        let share = (seconds - self.from) / (self.to - self.from);
        let share = if self.reversed { 1.0 - share } else { share };
        let share = crate::cast::f64_f32(share);
        self.rect.top() + share * self.rect.height()
    }

    /// The seconds into the song at `y`, kept within the song.
    fn seconds_at(self, y: f32) -> f64 {
        let share = f64::from(((y - self.rect.top()) / self.rect.height()).clamp(0.0, 1.0));
        let share = if self.reversed { 1.0 - share } else { share };
        self.from + share * (self.to - self.from)
    }
}

/// Draws the minimap of `song` in `rect`, with a box over `visible`, the seconds the notefield
/// shows, and asks to move the cursor to where it is clicked or dragged.
pub(crate) fn show(
    ui: &Ui,
    rect: Rect,
    song: &Song,
    (visible, length, reversed): ((f64, f64), Option<f64>, bool),
    actions: &mut Vec<Action>,
) {
    let response = ui
        .interact(rect, ui.id().with("minimap"), Sense::click_and_drag())
        .on_hover_text("The whole chart. Click or drag to move the cursor there.");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Minimap"));
    let map = Map::of(song, rect, length, reversed);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, style::LANES_BG);
    let timing = song.timing();
    let seconds = |tick| timing.time_at(tick).seconds();
    for marker in &song.document().document().markers {
        if marker.kind == MarkerKind::Section {
            let y = map.y_of(seconds(marker.tick));
            painter.hline(rect.x_range(), y, Stroke::new(1.0, style::SECTION));
        }
    }
    if let Some(chart) = song.chart() {
        let lanes = chart.lanes.map_or(1, u16::from).max(1);
        let lane_w = (rect.width() - 4.0) / f32::from(lanes);
        for object in &chart.objects {
            let Position::Lane { index, width } = object.position else {
                continue;
            };
            let left = rect.left() + 2.0 + f32::from(u16::from(index)) * lane_w;
            let right = left + f32::from(u16::from(width)) * lane_w;
            let end = object.shape.end(object.tick).unwrap_or(object.tick);
            let (a, b) = (map.y_of(seconds(object.tick)), map.y_of(seconds(end)));
            let (top, bottom) = (a.min(b), a.max(b).max(a.min(b) + NOTE_H));
            let colour = if song.is_selected(object) {
                style::SELECTED
            } else if object.kind == Kind::Hold {
                style::HOLD
            } else {
                kind_colours(&object.kind).0
            };
            let note = Rect::from_x_y_ranges(left + 0.5..=right - 0.5, top..=bottom);
            painter.rect_filled(note, 0.0, colour);
        }
    }
    let (a, b) = (map.y_of(visible.0), map.y_of(visible.1));
    let shown = Rect::from_x_y_ranges(rect.x_range(), a.min(b)..=a.max(b).max(a.min(b) + 2.0));
    painter.rect(
        shown,
        2.0,
        style::NOW_BAND,
        Stroke::new(1.0, style::MEASURE_LINE),
        StrokeKind::Inside,
    );
    if response.is_pointer_button_down_on()
        && let Some(at) = response.interact_pointer_pos()
        && let Some(hovered) = ResolvedTime::from_seconds(map.seconds_at(at.y))
    {
        let step = song
            .document()
            .document()
            .ticks_per_beat
            .note_length(song.snap());
        let tick = step.map_or_else(
            || timing.latest_tick_by(hovered),
            |step| nearest_line(timing, hovered, step),
        );
        if tick != song.cursor() {
            actions.push(Action::JumpTo(tick));
        }
    }
}

#[cfg(test)]
mod tests {
    use charter_core::{Tick, ValidDocument};
    use egui::pos2;
    use egui_kittest::Harness;
    use serde_json::json;

    use super::*;

    fn song_with(objects: &[serde_json::Value]) -> Song {
        let document = json!({
            "format_version": 1,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "a.ogg" },
            "timing": { "anchors": [ { "tick": 0, "seconds": 0.0 } ], "tempo": 120.0, "events": [] },
            "charts": [ { "name": "Easy", "lanes": 4, "objects": objects } ]
        });
        let document = ValidDocument::from_json(&serde_json::to_vec(&document).unwrap()).unwrap();
        Song::new(document, &[])
    }

    fn tap(tick: i32) -> serde_json::Value {
        json!({ "tick": tick, "position": { "lane": { "index": 0 } },
                "kind": "tap", "shape": "instant" })
    }

    fn song() -> Song {
        song_with(&[tap(0), tap(19_200)])
    }

    fn rect() -> Rect {
        Rect::from_min_max(pos2(0.0, 100.0), pos2(40.0, 300.0))
    }

    #[test]
    fn the_map_spans_the_song_to_its_audio_or_its_last_note_whichever_is_later() {
        // The last tap is 20 beats in at 120 BPM: 10 s.
        let short = Map::of(&song(), rect(), Some(4.0), false);
        assert!((short.to - 10.0).abs() < 1e-9);
        let long = Map::of(&song(), rect(), Some(20.0), false);
        assert!((long.to - 20.0).abs() < 1e-9);
        assert!((long.y_of(10.0) - 200.0).abs() < 1e-3);
        assert!((long.seconds_at(250.0) - 15.0).abs() < 1e-3);
    }

    #[test]
    fn a_reversed_map_runs_bottom_to_top_and_clamps_to_the_song() {
        let map = Map::of(&song(), rect(), Some(20.0), true);
        assert!((map.y_of(0.0) - 300.0).abs() < 1e-3);
        assert!((map.seconds_at(100.0) - 20.0).abs() < 1e-3);
        assert!((map.seconds_at(-50.0) - 20.0).abs() < 1e-3);
        assert!(map.seconds_at(900.0).abs() < 1e-3);
    }

    #[test]
    fn an_empty_chart_still_maps_one_second_without_dividing_by_zero() {
        let map = Map::of(&song_with(&[]), rect(), None, false);
        assert!((map.y_of(0.0) - 100.0).abs() < 1e-3);
        assert!((map.y_of(1.0) - 300.0).abs() < 1e-3);
        assert!((map.seconds_at(200.0) - 0.5).abs() < 1e-9);
        assert!((map.seconds_at(900.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_note_before_tick_zero_is_visible_and_can_be_reached_in_both_directions() {
        let song = song_with(&[tap(-1_920), tap(19_200)]);
        let normal = Map::of(&song, rect(), None, false);
        let reversed = Map::of(&song, rect(), None, true);
        assert!((normal.y_of(-1.0) - 100.0).abs() < 1e-3);
        assert!((normal.seconds_at(100.0) + 1.0).abs() < 1e-9);
        assert!((reversed.y_of(-1.0) - 300.0).abs() < 1e-3);
        assert!((reversed.seconds_at(300.0) + 1.0).abs() < 1e-9);
        assert!((normal.y_of(10.0) - 300.0).abs() < 1e-3);
    }

    #[test]
    fn dragging_outside_the_strip_jumps_only_as_far_as_the_chart_ends() {
        let mut song = song();
        song.jump_to(Tick::new(960));
        let mut harness = Harness::builder()
            .with_size(egui::vec2(200.0, 400.0))
            .build_ui_state(
                |ui, (song, actions): &mut (Song, Vec<Action>)| {
                    show(ui, rect(), song, ((0.0, 2.0), None, false), actions);
                },
                (song, Vec::new()),
            );
        harness.run();
        let at = pos2(20.0, 150.0);
        harness.hover_at(at);
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run();
        harness.hover_at(pos2(20.0, 350.0));
        harness.run();
        assert!(
            harness
                .state()
                .1
                .contains(&Action::JumpTo(Tick::new(19_200)))
        );
        harness.hover_at(pos2(20.0, 50.0));
        harness.run();
        assert_eq!(
            harness.state().1.last(),
            Some(&Action::JumpTo(Tick::new(0)))
        );
    }
}
