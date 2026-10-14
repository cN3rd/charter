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
    /// Directions offered in the display settings, in menu order.
    pub(crate) const ALL: [Self; 4] = [Self::Up, Self::Down, Self::Left, Self::Right];

    /// What the settings call it.
    #[must_use]
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Up => "Vertical, notes travel up",
            Self::Down => "Vertical, notes travel down",
            Self::Left => "Horizontal, notes travel left",
            Self::Right => "Horizontal, notes travel right",
        }
    }

    /// Whether time runs across the screen rather than down it.
    const fn across(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

/// Turns the notefield's own layout, lanes across and later notes lower, onto `area` of the
/// screen, and back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Flip {
    /// The screen area receiving the turned notefield.
    pub(crate) area: Rect,
    /// The direction in which notes travel on screen.
    pub(crate) direction: Direction,
}

impl Flip {
    /// The area the notefield lays itself out in: `area`, turned a quarter when time runs
    /// across.
    #[must_use]
    pub(crate) fn field(self) -> Rect {
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
    pub(crate) fn pos(self, at: Pos2) -> Pos2 {
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
    pub(crate) fn back(self, at: Pos2) -> Pos2 {
        let (min, max) = (self.area.min, self.area.max);
        match self.direction {
            Direction::Up => at,
            Direction::Down => pos2(at.x, min.y + (max.y - at.y)),
            Direction::Left => pos2(min.x + (at.y - min.y), min.y + (at.x - min.x)),
            Direction::Right => pos2(min.x + (at.y - min.y), min.y + (max.x - at.x)),
        }
    }

    /// The screen rectangle covered by `rect` of the notefield's layout.
    #[must_use]
    pub(crate) fn rect(self, rect: Rect) -> Rect {
        Rect::from_two_pos(self.pos(rect.min), self.pos(rect.max))
    }

    /// How text anchored `align` in the notefield's layout is anchored on screen, so it keeps
    /// the side of its point it had.
    #[must_use]
    pub(crate) fn align(self, align: Align2) -> Align2 {
        let [x, y] = align.0;
        let flip = |edge: Align| match edge {
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
pub(crate) struct Canvas {
    /// The painter whose clip rectangle is in screen coordinates.
    painter: Painter,
    /// The transform from notefield coordinates to screen coordinates.
    flip: Flip,
}

impl Canvas {
    /// A painter for drawing through `flip` into the screen.
    #[must_use]
    pub(crate) const fn new(painter: Painter, flip: Flip) -> Self {
        Self { painter, flip }
    }

    /// This canvas, drawing only inside `rect`.
    #[must_use]
    pub(crate) fn clipped(&self, rect: Rect) -> Self {
        Self {
            painter: self.painter.with_clip_rect(self.flip.rect(rect)),
            flip: self.flip,
        }
    }

    /// Draws `texture` over `rect` of the layout, its left edge on the rect's left and its top
    /// on the rect's top, turned with the rest of the notefield.
    pub(crate) fn image(&self, texture: egui::TextureId, rect: Rect) {
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

    /// Fills `rect` of the notefield's layout, after turning it onto the screen.
    pub(crate) fn rect_filled(&self, rect: Rect, rounding: f32, fill: egui::Color32) {
        self.painter
            .rect_filled(self.flip.rect(rect), rounding, fill);
    }

    /// A four-pixel stripe at the head's screen-left edge, independent of the scroll direction.
    pub(crate) fn left_stripe(&self, rect: Rect, fill: egui::Color32) {
        let screen = self.flip.rect(rect);
        // A lane-dark gutter behind the stripe, so a layer colour close to the note's own kind
        // colour still reads as a bar of its own instead of merging into the head.
        let gutter = Rect::from_min_size(screen.min, vec2(5.0, screen.height()));
        self.painter
            .rect_filled(gutter, 2.0, crate::style::LANES_BG);
        let stripe = Rect::from_min_size(screen.min, vec2(4.0, screen.height()));
        self.painter.rect_filled(stripe, 2.0, fill);
    }

    /// Strokes the transformed outline of `rect` in notefield coordinates.
    pub(crate) fn rect_stroke(&self, rect: Rect, rounding: f32, stroke: Stroke, kind: StrokeKind) {
        self.painter
            .rect_stroke(self.flip.rect(rect), rounding, stroke, kind);
    }

    /// Fills and strokes `rect` in notefield coordinates, turned onto the screen.
    pub(crate) fn rect(
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
    pub(crate) fn hline(&self, x: egui::Rangef, y: f32, stroke: Stroke) {
        self.line(vec![pos2(x.min, y), pos2(x.max, y)], stroke);
    }

    /// A line along time at `x`, from `y.min` to `y.max`.
    pub(crate) fn vline(&self, x: f32, y: egui::Rangef, stroke: Stroke) {
        self.line(vec![pos2(x, y.min), pos2(x, y.max)], stroke);
    }

    /// Draws a line through `points` of the notefield's layout.
    pub(crate) fn line(&self, points: Vec<Pos2>, stroke: Stroke) {
        let points = points
            .into_iter()
            .map(|point| self.flip.pos(point))
            .collect();
        self.painter.add(Shape::line(points, stroke));
    }

    /// Draws a filled polygon through `points` of the notefield's layout.
    pub(crate) fn convex_polygon(&self, points: Vec<Pos2>, fill: egui::Color32, stroke: Stroke) {
        let points = points
            .into_iter()
            .map(|point| self.flip.pos(point))
            .collect();
        self.painter
            .add(Shape::convex_polygon(points, fill, stroke));
    }

    /// Fills a circle at `centre` in the notefield's layout, in screen pixels.
    pub(crate) fn circle_filled(&self, centre: Pos2, radius: f32, fill: egui::Color32) {
        self.painter
            .circle_filled(self.flip.pos(centre), radius, fill);
    }

    /// Strokes a circle at `centre` in the notefield's layout, in screen pixels.
    pub(crate) fn circle_stroke(&self, centre: Pos2, radius: f32, stroke: Stroke) {
        self.painter
            .circle_stroke(self.flip.pos(centre), radius, stroke);
    }

    /// Text upright on screen, at the point `at` of the layout and on the side of it `align`
    /// says.
    pub(crate) fn text(
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

    /// Draws text only if its screen rectangle does not cover an earlier label in this column,
    /// returning the rectangle of text drawn.
    pub(crate) fn text_if_clear(
        &self,
        at: Pos2,
        align: Align2,
        text: &impl ToString,
        font: egui::FontId,
        colour: egui::Color32,
        drawn: &[Rect],
    ) -> Option<Rect> {
        let galley = self.painter.layout_no_wrap(text.to_string(), font, colour);
        let rect = self
            .flip
            .align(align)
            .anchor_size(self.flip.pos(at), galley.size());
        // ponytail: this quadratic scan is bounded by visible labels; bucket by screen position
        // if dense timing charts make the label pass slow.
        if !rect.intersects(self.painter.clip_rect())
            || drawn.iter().any(|previous| rect.intersects(*previous))
        {
            return None;
        }
        self.painter.galley(rect.min, galley, colour);
        Some(rect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Color32;

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

    #[test]
    fn points_across_the_field_and_just_outside_it_round_trip_in_every_direction() {
        for direction in Direction::ALL {
            let flip = flip(direction);
            let field = flip.field();
            for x in [-10.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.1] {
                for y in [-0.1, 0.0, 0.25, 0.5, 0.75, 1.0, 1.1] {
                    let at = field.min + vec2(x * field.width(), y * field.height());
                    let back = flip.back(flip.pos(at));
                    assert!(
                        (back - at).length() < 0.001,
                        "{direction:?}: {at:?} → {back:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn turned_rectangles_keep_their_sides_and_their_centres() {
        let rect = Rect::from_min_size(pos2(120.0, 90.0), vec2(45.0, 70.0));
        for direction in Direction::ALL {
            let flip = flip(direction);
            let turned = flip.rect(rect);
            let size = if direction.across() {
                vec2(70.0, 45.0)
            } else {
                vec2(45.0, 70.0)
            };
            assert_eq!(turned.size(), size, "{direction:?}");
            assert_eq!(turned.center(), flip.pos(rect.center()), "{direction:?}");
        }
    }

    #[test]
    fn text_alignment_turns_each_side_and_keeps_centred_axes_centred() {
        let anchors = [Align::Min, Align::Center, Align::Max];
        for direction in Direction::ALL {
            let flip = flip(direction);
            for x in anchors {
                for y in anchors {
                    let anchor = flip.align(Align2([x, y]));
                    let offset = |align| match align {
                        Align::Min => -1,
                        Align::Center => 0,
                        Align::Max => 1,
                    };
                    let (x, y) = (offset(x), offset(y));
                    let expected = match direction {
                        Direction::Up => (x, y),
                        Direction::Down => (x, -y),
                        Direction::Left => (y, x),
                        Direction::Right => (-y, x),
                    };
                    assert_eq!(
                        (offset(anchor.0[0]), offset(anchor.0[1])),
                        expected,
                        "{direction:?}: ({x}, {y})"
                    );
                }
            }
        }
    }

    #[test]
    fn image_corners_keep_their_texture_coordinates_when_the_field_turns() {
        let rect = Rect::from_min_max(pos2(110.0, 60.0), pos2(130.0, 80.0));
        let cases = [
            (
                Direction::Up,
                [
                    pos2(110.0, 60.0),
                    pos2(130.0, 60.0),
                    pos2(130.0, 80.0),
                    pos2(110.0, 80.0),
                ],
            ),
            (
                Direction::Down,
                [
                    pos2(110.0, 340.0),
                    pos2(130.0, 340.0),
                    pos2(130.0, 320.0),
                    pos2(110.0, 320.0),
                ],
            ),
            (
                Direction::Left,
                [
                    pos2(110.0, 60.0),
                    pos2(110.0, 80.0),
                    pos2(130.0, 80.0),
                    pos2(130.0, 60.0),
                ],
            ),
            (
                Direction::Right,
                [
                    pos2(490.0, 60.0),
                    pos2(490.0, 80.0),
                    pos2(470.0, 80.0),
                    pos2(470.0, 60.0),
                ],
            ),
        ];
        for (direction, positions) in cases {
            let ctx = egui::Context::default();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                Canvas::new(ui.painter().clone(), flip(direction))
                    .image(egui::TextureId::User(1), rect);
            });
            output.textures_delta.clear();
            let mesh = output
                .shapes
                .iter()
                .find_map(|shape| {
                    if let Shape::Mesh(mesh) = &shape.shape {
                        Some(mesh)
                    } else {
                        None
                    }
                })
                .expect("the image produces a textured mesh");
            let uv = [
                pos2(0.0, 0.0),
                pos2(1.0, 0.0),
                pos2(1.0, 1.0),
                pos2(0.0, 1.0),
            ];
            for ((vertex, position), texel) in mesh.vertices.iter().zip(positions).zip(uv) {
                assert_eq!(
                    (vertex.pos, vertex.uv, vertex.color),
                    (position, texel, Color32::WHITE),
                    "{direction:?}"
                );
            }
            assert_eq!(mesh.vertices.len(), 4);
            assert_eq!(mesh.indices, [0, 1, 2, 0, 2, 3]);
        }
    }

    #[test]
    fn a_circle_moves_with_the_field_but_keeps_its_radius() {
        let positions = [
            (Direction::Up, pos2(130.0, 90.0)),
            (Direction::Down, pos2(130.0, 310.0)),
            (Direction::Left, pos2(140.0, 80.0)),
            (Direction::Right, pos2(460.0, 80.0)),
        ];
        for (direction, expected) in positions {
            let ctx = egui::Context::default();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                Canvas::new(ui.painter().clone(), flip(direction)).circle_filled(
                    pos2(130.0, 90.0),
                    7.0,
                    Color32::RED,
                );
            });
            output.textures_delta.clear();
            let circle = output.shapes.iter().find_map(|shape| match &shape.shape {
                Shape::Circle(circle) => Some(circle),
                Shape::Noop
                | Shape::Vec(_)
                | Shape::Ellipse(_)
                | Shape::LineSegment { .. }
                | Shape::Path(_)
                | Shape::Rect(_)
                | Shape::Text(_)
                | Shape::Mesh(_)
                | Shape::QuadraticBezier(_)
                | Shape::CubicBezier(_)
                | Shape::Callback(_) => None,
            });
            let circle = circle.expect("the canvas paints a circle");
            assert_eq!(
                (circle.center, circle.radius, circle.fill),
                (expected, 7.0, Color32::RED)
            );
        }
    }
}
