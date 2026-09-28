//! Which way the notefield runs on screen. The notefield lays itself out one way, lanes across
//! and later notes lower, and [`Flip`] turns that onto the screen for each [`Direction`].

use egui::{Align, Align2, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};
use serde::{Deserialize, Serialize};

/// Which way notes travel past the now line as the song plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Later notes below, as StepMania scrolls.
    #[default]
    Up,
    /// Later notes above, as guitar games scroll.
    Down,
    /// Later notes to the right, as Taiko scrolls.
    Left,
    /// Later notes to the left.
    Right,
}

impl Direction {
    pub const ALL: [Self; 4] = [Self::Up, Self::Down, Self::Left, Self::Right];

    /// What the settings call it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Up => "Vertical, notes travel up",
            Self::Down => "Vertical, notes travel down",
            Self::Left => "Horizontal, notes travel left",
            Self::Right => "Horizontal, notes travel right",
        }
    }

    /// Whether time runs across the screen rather than down it.
    fn across(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

/// Turns the notefield's own layout, lanes across and later notes lower, onto `area` of the
/// screen, and back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flip {
    pub area: Rect,
    pub direction: Direction,
}

impl Flip {
    /// The area the notefield lays itself out in: `area`, turned a quarter when time runs
    /// across.
    #[must_use]
    pub fn field(self) -> Rect {
        let size = self.area.size();
        let size = if self.direction.across() {
            vec2(size.y, size.x)
        } else {
            size
        };
        Rect::from_min_size(self.area.min, size)
    }

    /// Where `at`, in the notefield's layout, is on screen.
    #[must_use]
    pub fn pos(self, at: Pos2) -> Pos2 {
        let (min, max) = (self.area.min, self.area.max);
        let (across, along) = (at.x - min.x, at.y - min.y);
        match self.direction {
            Direction::Up => at,
            Direction::Down => pos2(at.x, max.y - along),
            Direction::Left => pos2(min.x + along, min.y + across),
            Direction::Right => pos2(max.x - along, min.y + across),
        }
    }

    /// Where the point on screen at `at` is in the notefield's layout.
    #[must_use]
    pub fn back(self, at: Pos2) -> Pos2 {
        let (min, max) = (self.area.min, self.area.max);
        match self.direction {
            Direction::Up => at,
            Direction::Down => pos2(at.x, min.y + (max.y - at.y)),
            Direction::Left => pos2(min.x + (at.y - min.y), min.y + (at.x - min.x)),
            Direction::Right => pos2(min.x + (at.y - min.y), min.y + (max.x - at.x)),
        }
    }

    #[must_use]
    pub fn rect(self, rect: Rect) -> Rect {
        Rect::from_two_pos(self.pos(rect.min), self.pos(rect.max))
    }

    /// How text anchored `align` in the notefield's layout is anchored on screen, so it keeps
    /// the side of its point it had.
    #[must_use]
    pub fn align(self, align: Align2) -> Align2 {
        let [x, y] = align.0;
        let flip = |align: Align| match align {
            Align::Min => Align::Max,
            Align::Center => Align::Center,
            Align::Max => Align::Min,
        };
        Align2(match self.direction {
            Direction::Up => [x, y],
            Direction::Down => [x, flip(y)],
            Direction::Left => [y, x],
            Direction::Right => [flip(y), x],
        })
    }
}

/// A painter that takes the notefield's layout, and draws it turned onto the screen.
pub struct Canvas {
    painter: Painter,
    flip: Flip,
}

impl Canvas {
    #[must_use]
    pub fn new(painter: Painter, flip: Flip) -> Self {
        Self { painter, flip }
    }

    /// This canvas, drawing only inside `rect`.
    #[must_use]
    pub fn clipped(&self, rect: Rect) -> Self {
        Self {
            painter: self.painter.with_clip_rect(self.flip.rect(rect)),
            flip: self.flip,
        }
    }

    /// Draws `texture` over `rect` of the layout, its left edge on the rect's left and its top
    /// on the rect's top, turned with the rest of the notefield.
    pub fn image(&self, texture: egui::TextureId, rect: Rect) {
        let corners = [
            (rect.left_top(), pos2(0.0, 0.0)),
            (rect.right_top(), pos2(1.0, 0.0)),
            (rect.right_bottom(), pos2(1.0, 1.0)),
            (rect.left_bottom(), pos2(0.0, 1.0)),
        ];
        let mut mesh = egui::Mesh::with_texture(texture);
        for (at, uv) in corners {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: self.flip.pos(at),
                uv,
                color: egui::Color32::WHITE,
            });
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        self.painter.add(Shape::mesh(mesh));
    }

    pub fn rect_filled(&self, rect: Rect, rounding: f32, fill: egui::Color32) {
        self.painter
            .rect_filled(self.flip.rect(rect), rounding, fill);
    }

    pub fn rect_stroke(&self, rect: Rect, rounding: f32, stroke: Stroke, kind: StrokeKind) {
        self.painter
            .rect_stroke(self.flip.rect(rect), rounding, stroke, kind);
    }

    pub fn rect(
        &self,
        rect: Rect,
        rounding: f32,
        fill: egui::Color32,
        stroke: Stroke,
        kind: StrokeKind,
    ) {
        self.painter
            .rect(self.flip.rect(rect), rounding, fill, stroke, kind);
    }

    /// A line across the lanes at `y`, from `x.min` to `x.max`.
    pub fn hline(&self, x: egui::Rangef, y: f32, stroke: Stroke) {
        self.line(vec![pos2(x.min, y), pos2(x.max, y)], stroke);
    }

    /// A line along time at `x`, from `y.min` to `y.max`.
    pub fn vline(&self, x: f32, y: egui::Rangef, stroke: Stroke) {
        self.line(vec![pos2(x, y.min), pos2(x, y.max)], stroke);
    }

    pub fn line(&self, points: Vec<Pos2>, stroke: Stroke) {
        let points = points
            .into_iter()
            .map(|point| self.flip.pos(point))
            .collect();
        self.painter.add(Shape::line(points, stroke));
    }

    pub fn convex_polygon(&self, points: Vec<Pos2>, fill: egui::Color32, stroke: Stroke) {
        let points = points
            .into_iter()
            .map(|point| self.flip.pos(point))
            .collect();
        self.painter
            .add(Shape::convex_polygon(points, fill, stroke));
    }

    pub fn circle_filled(&self, centre: Pos2, radius: f32, fill: egui::Color32) {
        self.painter
            .circle_filled(self.flip.pos(centre), radius, fill);
    }

    pub fn circle_stroke(&self, centre: Pos2, radius: f32, stroke: Stroke) {
        self.painter
            .circle_stroke(self.flip.pos(centre), radius, stroke);
    }

    /// Text upright on screen, at the point `at` of the layout and on the side of it `align`
    /// says.
    pub fn text(
        &self,
        at: Pos2,
        align: Align2,
        text: impl ToString,
        font: egui::FontId,
        colour: egui::Color32,
    ) {
        self.painter.text(
            self.flip.pos(at),
            self.flip.align(align),
            text,
            font,
            colour,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flip(direction: Direction) -> Flip {
        Flip {
            area: Rect::from_min_size(pos2(100.0, 50.0), vec2(400.0, 300.0)),
            direction,
        }
    }

    #[test]
    fn every_direction_turns_back_to_where_it_started() {
        let at = pos2(130.0, 90.0);
        for direction in Direction::ALL {
            let flip = flip(direction);
            let back = flip.back(flip.pos(at));
            assert!((back - at).length() < 1e-3, "{direction:?}: {back:?}");
        }
    }

    #[test]
    fn later_notes_sit_where_the_direction_sends_them_from() {
        let (now, later) = (pos2(110.0, 60.0), pos2(110.0, 160.0));
        let (up, down, left, right) = (
            flip(Direction::Up),
            flip(Direction::Down),
            flip(Direction::Left),
            flip(Direction::Right),
        );
        assert!(up.pos(later).y > up.pos(now).y);
        assert!(down.pos(later).y < down.pos(now).y);
        assert!(left.pos(later).x > left.pos(now).x);
        assert!(right.pos(later).x < right.pos(now).x);
    }

    #[test]
    fn lanes_run_down_the_screen_when_time_runs_across() {
        let (first, second) = (pos2(110.0, 60.0), pos2(170.0, 60.0));
        for direction in [Direction::Left, Direction::Right] {
            let flip = flip(direction);
            assert!(flip.pos(second).y > flip.pos(first).y, "{direction:?}");
        }
    }

    #[test]
    fn the_layout_fills_the_area_turned_when_time_runs_across() {
        assert_eq!(flip(Direction::Down).field().size(), vec2(400.0, 300.0));
        assert_eq!(flip(Direction::Right).field().size(), vec2(300.0, 400.0));
        let field = flip(Direction::Right).field();
        assert_eq!(
            flip(Direction::Right).rect(field),
            flip(Direction::Right).area
        );
    }

    #[test]
    fn text_keeps_its_side_of_its_point() {
        let below_left = Align2::LEFT_TOP;
        assert_eq!(flip(Direction::Up).align(below_left), Align2::LEFT_TOP);
        assert_eq!(flip(Direction::Down).align(below_left), Align2::LEFT_BOTTOM);
        assert_eq!(flip(Direction::Left).align(below_left), Align2::LEFT_TOP);
        assert_eq!(flip(Direction::Right).align(below_left), Align2::RIGHT_TOP);
    }
}
