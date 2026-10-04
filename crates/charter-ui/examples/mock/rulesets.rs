//! A made-up song for each ruleset, and how each ruleset draws its own column. The editor draws
//! the other columns the same way for every game.

use eframe::egui::{
    Align2, Color32, FontId, Painter, Pos2, Rangef, Rect, Shape, Stroke, StrokeKind, pos2, vec2,
};

use std::f32::consts::FRAC_PI_2;

use super::{Beat, Direction, Lanes};

/// A made-up song in one ruleset, and how that ruleset draws its column.
pub struct Mock {
    /// The game, as the mock switcher names it.
    pub game: &'static str,
    /// Name shown in the song panel, independent of the ruleset.
    pub title: &'static str,
    /// Each chart's name and lane count, and whether it is the open one.
    pub charts: &'static [(&'static str, &'static str, bool)],
    /// What the chart header says after the open chart's name.
    pub layout: &'static str,
    /// Label/value pairs shown in the chart settings panel.
    pub settings: &'static [(&'static str, &'static str)],
    /// Named audio samples shown in the song panel.
    pub samples: &'static [(&'static str, &'static str)],
    /// The key that places a note in each lane, one per lane.
    pub keys: &'static [&'static str],
    /// Screen width of each lane, in points.
    pub lane_w: f32,
    /// Playfield beyond the lanes on each side, such as Arcaea's sky.
    pub sky_w: f32,
    /// A strip beside the playfield where the ruleset draws a second view, as its caption and
    /// width, such as Arcaea's heights seen from the side.
    pub side: Option<(&'static str, f32)>,
    /// Whether later beats scroll up or down the playfield.
    pub direction: Direction,
    /// Event beats and labels drawn beside the chart.
    pub timing: &'static [(Beat, &'static str)],
    /// What is selected, which the inspector describes.
    pub selected: &'static str,
    /// Label/value pairs describing the selected object.
    pub inspector: &'static [(&'static str, &'static str)],
    /// Song validation message, if the mock shows one.
    pub problem: Option<&'static str>,
    /// Draws the ruleset's own kinds in its column.
    pub draw: fn(&Painter, &Lanes),
}

impl Mock {
    /// Number of playable lanes, capped to the lane count's `u16` range.
    pub fn lanes(&self) -> u16 {
        u16::try_from(self.keys.len()).unwrap_or(u16::MAX)
    }

    /// How wide the lanes and any playfield beyond them are.
    pub fn playfield_w(&self) -> f32 {
        f32::from(self.lanes()) * self.lane_w + 2.0 * self.sky_w
    }

    /// How wide the ruleset's column is, side view included.
    pub fn width(&self) -> f32 {
        self.playfield_w() + self.side.map_or(0.0, |(_, width)| width)
    }
}

/// Songs offered by the ruleset switcher, in display order.
pub const ALL: [&Mock; 5] = [
    &STEPMANIA,
    &TAP_TAP_REVENGE,
    &OSU_MANIA,
    &PROJECT_SEKAI,
    &ARCAEA,
];

/// Light note heads and outlines on dark playfields.
const WHITE: Color32 = Color32::from_gray(230);
/// Blue tap heads, arcs and osu!mania side keys.
const BLUE: Color32 = Color32::from_rgb(110, 170, 255);
/// Dimmer fill for StepMania hold bodies.
const DARK_BLUE: Color32 = Color32::from_rgb(60, 100, 170);
/// Mine outlines and the first Tap Tap Revenge lane.
const RED: Color32 = Color32::from_rgb(235, 90, 90);
/// Slide heads and the middle Tap Tap Revenge lane.
const GREEN: Color32 = Color32::from_rgb(100, 210, 130);
/// Critical notes, mega taps and the middle osu!mania key.
const YELLOW: Color32 = Color32::from_rgb(250, 205, 80);
/// Flicks and Arcaea's second arc colour.
const PINK: Color32 = Color32::from_rgb(255, 110, 170);
/// Regular Project Sekai tap heads.
const CYAN: Color32 = Color32::from_rgb(90, 215, 255);
/// Tap Tap Revenge shake gestures.
const ORANGE: Color32 = Color32::from_rgb(255, 160, 60);
/// Tap Tap Revenge slice gestures.
const VIOLET: Color32 = Color32::from_rgb(190, 120, 255);
/// Muted detail lines in the Arcaea side view.
const GREY: Color32 = Color32::from_gray(150);
/// Translucent fill behind Arcaea's sky area.
const SKY: Color32 = Color32::from_rgba_unmultiplied_const(70, 90, 180, 28);

/// Mock dance chart with stops, a tempo change and a warp warning.
pub const STEPMANIA: Mock = Mock {
    game: "StepMania",
    title: "Anthem",
    charts: &[
        ("Easy", "4 lanes", false),
        ("Hard", "4 lanes", true),
        ("Challenge", "4 lanes", false),
        ("Doubles", "8 lanes", false),
    ],
    layout: "dance-single, 4 lanes",
    settings: &[
        ("Style", "dance-single"),
        ("Difficulty", "Hard"),
        ("Meter", "9"),
    ],
    samples: &[("kick", "kick.wav"), ("clap", "clap.wav")],
    keys: &["1", "2", "3", "4"],
    lane_w: 56.0,
    sky_w: 0.0,
    side: None,
    direction: Direction::Down,
    timing: &[
        (Beat(0.0), "bpm 180"),
        (Beat(3.0), "stop 0.500"),
        (Beat(8.0), "bpm 190"),
        (Beat(10.0), "sm.scroll 0.5"),
    ],
    selected: "2 taps",
    inspector: &[
        ("At", "13:2:0"),
        ("Lane", "mixed"),
        ("Kind", "tap"),
        ("Shape", "instant"),
        ("Sample", "none"),
    ],
    problem: Some("3 notes inside the warp at 32:0:0 are unreachable"),
    draw: stepmania,
};

/// Mock three-lane chart with gesture notes and upward scrolling.
const TAP_TAP_REVENGE: Mock = Mock {
    game: "Tap Tap Revenge",
    title: "Neon Riff",
    charts: &[
        ("Easy", "3 lanes", false),
        ("Medium", "3 lanes", false),
        ("Hard", "3 lanes", true),
        ("Extreme", "3 lanes", false),
    ],
    layout: "3 lanes",
    settings: &[],
    samples: &[],
    keys: &["1", "2", "3"],
    lane_w: 72.0,
    sky_w: 0.0,
    side: None,
    direction: Direction::Up,
    timing: &[(Beat(0.0), "bpm 128"), (Beat(8.0), "bpm 132")],
    selected: "1 shake",
    inspector: &[
        ("At", "13:3:0"),
        ("Lane", "0"),
        ("Kind", "ttr.shake"),
        ("Shape", "instant"),
        ("Direction", "left"),
    ],
    problem: None,
    draw: tap_tap_revenge,
};

/// Mock seven-key chart with per-lane colours and holds.
const OSU_MANIA: Mock = Mock {
    game: "osu!mania",
    title: "Sidechain",
    charts: &[
        ("4K Easy", "4 lanes", false),
        ("7K Normal", "7 lanes", false),
        ("7K Hard", "7 lanes", true),
        ("7K Insane", "7 lanes", false),
    ],
    layout: "7 keys",
    settings: &[("HP", "7"), ("OD", "8")],
    samples: &[
        ("clap", "soft-hitclap.wav"),
        ("whistle", "drum-whistle.wav"),
    ],
    keys: &["S", "D", "F", "Spc", "J", "K", "L"],
    lane_w: 40.0,
    sky_w: 0.0,
    side: None,
    direction: Direction::Up,
    timing: &[
        (Beat(0.0), "bpm 174"),
        (Beat(8.0), "osu_mania.scroll_speed\n0.8"),
    ],
    selected: "1 hold",
    inspector: &[
        ("At", "13:0:0"),
        ("Lane", "3"),
        ("Kind", "hold"),
        ("Shape", "sustain, 2 beats"),
        ("Sample", "clap"),
    ],
    problem: None,
    draw: osu_mania,
};

/// Mock twelve-lane chart with flicks and multi-point slides.
const PROJECT_SEKAI: Mock = Mock {
    game: "Project Sekai",
    title: "Hologram",
    charts: &[
        ("Easy", "12 lanes", false),
        ("Normal", "12 lanes", false),
        ("Hard", "12 lanes", false),
        ("Expert", "12 lanes", false),
        ("Master", "12 lanes", true),
    ],
    layout: "12 lanes",
    settings: &[("Level", "31")],
    samples: &[],
    keys: &["1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "-", "="],
    lane_w: 26.0,
    sky_w: 0.0,
    side: None,
    direction: Direction::Up,
    timing: &[(Beat(0.0), "bpm 160")],
    selected: "1 flick",
    inspector: &[
        ("At", "14:0:0"),
        ("Lane", "4, width 4"),
        ("Kind", "psk.flick"),
        ("Shape", "instant"),
        ("Direction", "up"),
        ("Critical", "yes"),
    ],
    problem: None,
    draw: project_sekai,
};

/// Mock four-lane floor with sky arcs and a side-on height view.
const ARCAEA: Mock = Mock {
    game: "Arcaea",
    title: "Skyline",
    charts: &[
        ("Past", "4 lanes", false),
        ("Present", "4 lanes", false),
        ("Future", "4 lanes", true),
        ("Beyond", "4 lanes", false),
    ],
    layout: "4 floor lanes and the sky",
    settings: &[("Rating", "9+")],
    samples: &[],
    keys: &["1", "2", "3", "4"],
    lane_w: 44.0,
    sky_w: 88.0,
    side: Some(("height", 96.0)),
    direction: Direction::Up,
    timing: &[(Beat(0.0), "bpm 200")],
    selected: "1 arc",
    inspector: &[
        ("At", "13:0:0"),
        ("Position", "plane 0.00, 1.00"),
        ("Kind", "arcaea.arc"),
        ("Shape", "path, 1 point"),
        ("Ease", "arcaea.b"),
        ("Colour", "blue"),
    ],
    problem: None,
    draw: arcaea,
};

/// A note head across `across`, centred on `y`.
fn head(across: Rangef, y: f32, half_height: f32) -> Rect {
    Rect::from_x_y_ranges(across, y - half_height..=y + half_height)
}

/// A hold's body from `from` to `to`, whichever way the chart scrolls.
fn body(across: Rangef, from: f32, to: f32) -> Rect {
    Rect::from_two_pos(pos2(across.min, from), pos2(across.max, to))
}

/// Marks what the inspector describes.
fn outline(painter: &Painter, rect: Rect) {
    painter.rect_stroke(
        rect.expand(3.0),
        4.0,
        Stroke::new(1.5, Color32::WHITE),
        StrokeKind::Outside,
    );
}

/// Kinds shown in the StepMania mock sequence.
enum Step {
    /// Single instant note.
    Tap,
    /// Sustained note ending at the contained beat.
    Hold(Beat),
    /// Hazard drawn as a ring instead of a filled head.
    Mine,
}

/// Beat, lane and kind of each note in the StepMania mock.
const STEPS: [(Beat, u16, Step); 38] = [
    (Beat(-1.0), 1, Step::Tap),
    (Beat(0.0), 0, Step::Tap),
    (Beat(1.0), 1, Step::Tap),
    (Beat(2.0), 2, Step::Tap),
    (Beat(3.0), 0, Step::Tap),
    (Beat(3.0), 3, Step::Hold(Beat(4.5))),
    (Beat(5.0), 2, Step::Tap),
    (Beat(6.0), 0, Step::Tap),
    (Beat(6.0), 1, Step::Tap),
    (Beat(7.0), 2, Step::Mine),
    (Beat(7.5), 3, Step::Tap),
    (Beat(8.0), 0, Step::Tap),
    (Beat(8.25), 1, Step::Tap),
    (Beat(8.5), 2, Step::Tap),
    (Beat(8.75), 3, Step::Tap),
    (Beat(9.0), 2, Step::Tap),
    (Beat(9.25), 1, Step::Tap),
    (Beat(9.5), 0, Step::Tap),
    (Beat(9.75), 1, Step::Tap),
    (Beat(10.0), 0, Step::Tap),
    (Beat(10.0), 3, Step::Tap),
    (Beat(11.0), 1, Step::Hold(Beat(13.0))),
    (Beat(11.0), 2, Step::Tap),
    (Beat(12.0), 3, Step::Tap),
    (Beat(12.5), 2, Step::Tap),
    (Beat(13.0), 0, Step::Tap),
    (Beat(14.0), 0, Step::Tap),
    (Beat(14.0), 3, Step::Mine),
    (Beat(15.0), 2, Step::Tap),
    (Beat(15.5), 1, Step::Tap),
    (Beat(16.0), 0, Step::Tap),
    (Beat(16.0), 3, Step::Tap),
    (Beat(17.0), 1, Step::Tap),
    (Beat(17.5), 2, Step::Tap),
    (Beat(18.0), 0, Step::Hold(Beat(20.0))),
    (Beat(18.0), 3, Step::Tap),
    (Beat(19.0), 2, Step::Tap),
    (Beat(20.0), 1, Step::Tap),
];

/// Draws the StepMania notes and outlines the selected pair of taps.
fn stepmania(painter: &Painter, lanes: &Lanes) {
    for (at, lane, step) in STEPS {
        let across = lanes.span(lane, 1).shrink(8.0);
        let y = lanes.y(at);
        match step {
            Step::Tap => {
                painter.rect_filled(head(across, y, 5.0), 3.0, BLUE);
            }
            Step::Hold(end) => {
                let tail = body(across.shrink(8.0), y, lanes.y(end));
                painter.rect_filled(tail, 2.0, DARK_BLUE);
                painter.rect_filled(head(across, y, 5.0), 3.0, BLUE);
            }
            Step::Mine => {
                painter.circle_stroke(pos2(across.center(), y), 6.0, Stroke::new(2.0, RED));
            }
        }
    }
    for lane in [0, 1] {
        outline(
            painter,
            head(lanes.span(lane, 1).shrink(8.0), lanes.y(Beat(6.0)), 5.0),
        );
    }
}

/// Kinds shown in the Tap Tap Revenge mock sequence.
enum Gem {
    /// Single coloured orb.
    Tap,
    /// Orb with a coloured body ending at the contained beat.
    Hold(Beat),
    /// Directional gesture with the shake label.
    Shake,
    /// Directional gesture with the slice label.
    Slice,
    /// Covers every lane, whichever it names.
    Mega,
}

/// Each lane's colour, left to right.
const GEM_COLOURS: [Color32; 3] = [RED, GREEN, BLUE];

/// Beat, lane and kind of each Tap Tap Revenge note.
const GEMS: [(Beat, u16, Gem); 21] = [
    (Beat(-1.0), 2, Gem::Tap),
    (Beat(0.0), 0, Gem::Tap),
    (Beat(0.5), 1, Gem::Tap),
    (Beat(1.0), 2, Gem::Tap),
    (Beat(2.0), 1, Gem::Hold(Beat(3.5))),
    (Beat(4.0), 0, Gem::Tap),
    (Beat(4.0), 2, Gem::Tap),
    (Beat(5.0), 0, Gem::Slice),
    (Beat(6.0), 2, Gem::Slice),
    (Beat(7.0), 0, Gem::Shake),
    (Beat(8.0), 0, Gem::Mega),
    (Beat(9.0), 1, Gem::Tap),
    (Beat(9.5), 0, Gem::Tap),
    (Beat(10.0), 2, Gem::Tap),
    (Beat(11.0), 1, Gem::Shake),
    (Beat(12.0), 0, Gem::Hold(Beat(14.0))),
    (Beat(12.0), 2, Gem::Tap),
    (Beat(13.0), 2, Gem::Tap),
    (Beat(14.0), 2, Gem::Shake),
    (Beat(15.0), 1, Gem::Tap),
    (Beat(16.0), 0, Gem::Mega),
];

/// Draws orbs, gesture boxes and full-width mega taps.
fn tap_tap_revenge(painter: &Painter, lanes: &Lanes) {
    for (at, lane, gem) in GEMS {
        let across = lanes.span(lane, 1);
        let y = lanes.y(at);
        let centre = pos2(across.center(), y);
        let colour = GEM_COLOURS.get(usize::from(lane)).copied().unwrap_or(WHITE);
        let gesture_box = head(across.shrink(6.0), y, 11.0);
        match gem {
            Gem::Tap => orb(painter, centre, colour),
            Gem::Hold(end) => {
                let tail = Rangef::new(centre.x - 5.0, centre.x + 5.0);
                painter.rect_filled(body(tail, y, lanes.y(end)), 5.0, colour.gamma_multiply(0.5));
                orb(painter, centre, colour);
            }
            Gem::Shake => gesture(
                painter,
                gesture_box,
                aim(lane, lanes.count),
                "shake",
                ORANGE,
            ),
            Gem::Slice => gesture(
                painter,
                gesture_box,
                aim(lane, lanes.count),
                "slice",
                VIOLET,
            ),
            Gem::Mega => {
                let bar = head(lanes.span(0, lanes.count).shrink(6.0), y, 9.0);
                painter.rect_filled(bar, 9.0, YELLOW);
                painter.text(
                    bar.center(),
                    Align2::CENTER_CENTER,
                    "mega tap",
                    FontId::proportional(12.0),
                    Color32::BLACK,
                );
            }
        }
    }
    outline(
        painter,
        head(lanes.span(0, 1).shrink(6.0), lanes.y(Beat(7.0)), 11.0),
    );
}

/// Which way a shake or slice points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Aim {
    /// Gesture arrow points towards the first lane's left edge.
    Left,
    /// Gesture arrow points towards the bottom of the screen.
    Down,
    /// Gesture arrow points towards the last lane's right edge.
    Right,
}

/// Where a shake or slice points, which its lane decides: left in the first lane, right in the
/// last, and down in between.
const fn aim(lane: u16, lanes: u16) -> Aim {
    if lane == 0 {
        Aim::Left
    } else if lane.saturating_add(1) == lanes {
        Aim::Right
    } else {
        Aim::Down
    }
}

/// Draws a coloured Tap Tap Revenge note with a light outline.
fn orb(painter: &Painter, centre: Pos2, colour: Color32) {
    painter.circle_filled(centre, 11.0, colour);
    painter.circle_stroke(centre, 11.0, Stroke::new(2.0, WHITE));
}

/// A shake or slice: a box across its lane, with an arrow the way it points and its name.
fn gesture(painter: &Painter, rect: Rect, aim: Aim, name: &str, colour: Color32) {
    painter.rect_stroke(rect, 6.0, Stroke::new(2.0, colour), StrokeKind::Inside);
    let tip = pos2(rect.left() + 15.0, rect.center().y);
    let corners = match aim {
        Aim::Left => [vec2(-5.0, 0.0), vec2(4.0, -5.0), vec2(4.0, 5.0)],
        Aim::Down => [vec2(0.0, 5.0), vec2(-5.0, -4.0), vec2(5.0, -4.0)],
        Aim::Right => [vec2(5.0, 0.0), vec2(-4.0, 5.0), vec2(-4.0, -5.0)],
    };
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "egui position offsets use vector addition"
    )]
    let arrow = corners.into_iter().map(|corner| tip + corner).collect();
    painter.add(Shape::convex_polygon(arrow, colour, Stroke::NONE));
    painter.text(
        pos2(rect.left() + 25.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(12.0),
        colour,
    );
}

/// A tap, or a hold when it has an end.
type Press = (Beat, u16, Option<Beat>);

/// osu!'s default seven-key colours, with the middle key set apart.
const SEVEN_KEY: [Color32; 7] = [WHITE, BLUE, WHITE, YELLOW, WHITE, BLUE, WHITE];

/// Beat, lane and optional hold end of the seven-key osu!mania chart.
const PRESSES: [Press; 40] = [
    (Beat(-1.0), 2, None),
    (Beat(-0.5), 4, None),
    (Beat(0.0), 0, None),
    (Beat(0.0), 6, None),
    (Beat(0.5), 2, None),
    (Beat(1.0), 4, None),
    (Beat(1.5), 1, None),
    (Beat(2.0), 5, None),
    (Beat(2.5), 3, None),
    (Beat(3.0), 0, None),
    (Beat(3.0), 2, None),
    (Beat(3.5), 6, None),
    (Beat(4.0), 3, Some(Beat(6.0))),
    (Beat(4.0), 1, None),
    (Beat(4.5), 5, None),
    (Beat(5.0), 0, None),
    (Beat(5.0), 6, None),
    (Beat(5.5), 2, None),
    (Beat(6.0), 4, None),
    (Beat(6.5), 1, None),
    (Beat(7.0), 5, Some(Beat(8.0))),
    (Beat(7.0), 0, None),
    (Beat(8.0), 2, None),
    (Beat(8.0), 3, None),
    (Beat(8.0), 4, None),
    (Beat(8.5), 1, None),
    (Beat(9.0), 6, None),
    (Beat(9.5), 0, None),
    (Beat(10.0), 3, Some(Beat(11.0))),
    (Beat(10.0), 5, None),
    (Beat(10.5), 1, None),
    (Beat(11.0), 6, None),
    (Beat(11.5), 2, None),
    (Beat(12.0), 0, None),
    (Beat(12.0), 4, None),
    (Beat(12.5), 3, None),
    (Beat(13.0), 1, None),
    (Beat(13.0), 5, None),
    (Beat(13.5), 6, None),
    (Beat(14.0), 2, Some(Beat(15.0))),
];

/// Draws the osu!mania presses and marks the chosen hold.
fn osu_mania(painter: &Painter, lanes: &Lanes) {
    presses(painter, lanes, &PRESSES, &SEVEN_KEY);
    let hold = lanes.span(3, 1).shrink(3.0);
    outline(painter, body(hold, lanes.y(Beat(4.0)), lanes.y(Beat(6.0))));
}

/// Taps and holds coloured by lane: every osu!mania note, and Arcaea's floor.
fn presses(painter: &Painter, lanes: &Lanes, presses: &[Press], colours: &[Color32]) {
    for &(at, lane, end) in presses {
        let across = lanes.span(lane, 1).shrink(3.0);
        let y = lanes.y(at);
        let colour = colours.get(usize::from(lane)).copied().unwrap_or(WHITE);
        if let Some(end) = end {
            let tail = body(across.shrink(4.0), y, lanes.y(end));
            painter.rect_filled(tail, 0.0, colour.gamma_multiply(0.45));
        }
        painter.rect_filled(head(across, y, 5.0), 2.0, colour);
    }
}

/// Kinds shown in the Project Sekai mock sequence.
enum Sekai {
    /// Instant note spanning one or more lanes.
    Tap,
    /// Instant note requiring an upward flick.
    Flick,
    /// The points after its head, each a beat and the lanes it covers there as first lane and
    /// width, and whether it ends in a flick.
    Slide(&'static [(Beat, u16, u16)], bool),
}

/// Beat, first lane, width in lanes, whether the note is critical, and the note.
const SEKAI_NOTES: [(Beat, u16, u16, bool, Sekai); 18] = [
    (Beat(-1.0), 5, 3, false, Sekai::Tap),
    (Beat(0.0), 2, 3, false, Sekai::Tap),
    (Beat(0.0), 7, 3, false, Sekai::Tap),
    (Beat(1.0), 4, 4, true, Sekai::Tap),
    (Beat(2.0), 0, 3, false, Sekai::Tap),
    (Beat(2.5), 9, 3, false, Sekai::Tap),
    (
        Beat(3.0),
        3,
        3,
        false,
        Sekai::Slide(
            &[(Beat(4.5), 6, 3), (Beat(5.5), 6, 3), (Beat(7.0), 1, 4)],
            true,
        ),
    ),
    (Beat(4.0), 10, 2, false, Sekai::Tap),
    (Beat(5.0), 0, 2, false, Sekai::Flick),
    (Beat(6.5), 9, 3, false, Sekai::Tap),
    (Beat(8.0), 4, 4, true, Sekai::Flick),
    (Beat(9.0), 1, 3, false, Sekai::Tap),
    (Beat(9.0), 8, 3, false, Sekai::Tap),
    (
        Beat(10.0),
        5,
        2,
        true,
        Sekai::Slide(&[(Beat(12.0), 5, 2)], false),
    ),
    (Beat(11.0), 0, 3, false, Sekai::Tap),
    (Beat(12.5), 8, 3, false, Sekai::Tap),
    (Beat(13.0), 2, 4, false, Sekai::Flick),
    (Beat(14.0), 6, 3, true, Sekai::Tap),
];

/// Draws wide notes, flicks and linked slide segments.
fn project_sekai(painter: &Painter, lanes: &Lanes) {
    for (at, lane, width, critical, note) in SEKAI_NOTES {
        let across = lanes.span(lane, width).shrink(2.0);
        let y = lanes.y(at);
        match note {
            Sekai::Tap => {
                let colour = if critical { YELLOW } else { CYAN };
                painter.rect_filled(head(across, y, 6.0), 4.0, colour);
            }
            Sekai::Flick => flick(painter, across, y, if critical { YELLOW } else { PINK }),
            Sekai::Slide(points, ends_in_flick) => {
                let colour = if critical { YELLOW } else { GREEN };
                let mut from = (across, y);
                for &(to_at, to_lane, to_width) in points {
                    let to = (lanes.span(to_lane, to_width).shrink(2.0), lanes.y(to_at));
                    ribbon(painter, from, to, colour.gamma_multiply(0.35));
                    from = to;
                }
                let relays = points.len().saturating_sub(1);
                for &(relay_at, relay_lane, relay_width) in points.iter().take(relays) {
                    let middle = lanes.span(relay_lane, relay_width).center();
                    diamond(painter, pos2(middle, lanes.y(relay_at)), colour);
                }
                painter.rect_filled(head(across, y, 6.0), 4.0, colour);
                if ends_in_flick {
                    flick(painter, from.0, from.1, PINK);
                } else {
                    painter.rect_filled(head(from.0, from.1, 6.0), 4.0, colour);
                }
            }
        }
    }
    outline(
        painter,
        head(lanes.span(4, 4).shrink(2.0), lanes.y(Beat(8.0)), 6.0),
    );
}

/// One stretch of a slide, from the lanes it covers at one height to those at another.
fn ribbon(painter: &Painter, from: (Rangef, f32), to: (Rangef, f32), fill: Color32) {
    let (upper, lower) = if from.1 < to.1 {
        (from, to)
    } else {
        (to, from)
    };
    let corners = vec![
        pos2(upper.0.min, upper.1),
        pos2(upper.0.max, upper.1),
        pos2(lower.0.max, lower.1),
        pos2(lower.0.min, lower.1),
    ];
    painter.add(Shape::convex_polygon(corners, fill, Stroke::NONE));
}

/// A flick's bar, with an arrow pointing up the screen.
fn flick(painter: &Painter, across: Rangef, y: f32, colour: Color32) {
    painter.rect_filled(head(across, y, 6.0), 4.0, colour);
    let tip = pos2(across.center(), y - 20.0);
    let arrow = vec![tip, pos2(tip.x + 9.0, y - 8.0), pos2(tip.x - 9.0, y - 8.0)];
    painter.add(Shape::convex_polygon(arrow, colour, Stroke::NONE));
}

/// A relay point partway along a slide.
fn diamond(painter: &Painter, centre: Pos2, colour: Color32) {
    let corners = [
        vec2(0.0, -6.0),
        vec2(6.0, 0.0),
        vec2(0.0, 6.0),
        vec2(-6.0, 0.0),
    ];
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "egui position offsets use vector addition"
    )]
    let points = corners.into_iter().map(|corner| centre + corner).collect();
    painter.add(Shape::convex_polygon(points, colour, Stroke::NONE));
}

/// Arcaea's easings, as arcfutil reads them. Each eases x and height apart: `b` eases both, `si`
/// and `so` ease x alone, and a four-letter ease eases x by its first half and height by its
/// second.
#[derive(Clone, Copy)]
enum Ease {
    /// Linear x and height.
    S,
    /// Smoothed x and height.
    B,
    /// Sine-in x and linear height.
    Si,
    /// Sine-out x and linear height.
    So,
    /// Sine-in x and height.
    SiSi,
    /// Sine-in x and sine-out height.
    SiSo,
    /// Sine-out x and sine-in height.
    SoSi,
    /// Sine-out x and height.
    SoSo,
}

impl Ease {
    /// How far across and how far up an arc has gone, each from 0 to 1, a fraction `t` of the way
    /// through it.
    fn at(self, t: f32) -> (f32, f32) {
        let bezier = t * t * (3.0 - 2.0 * t);
        let si = (t * FRAC_PI_2).sin();
        let so = 1.0 - (t * FRAC_PI_2).cos();
        match self {
            Self::S => (t, t),
            Self::B => (bezier, bezier),
            Self::Si => (si, t),
            Self::So => (so, t),
            Self::SiSi => (si, si),
            Self::SiSo => (si, so),
            Self::SoSi => (so, si),
            Self::SoSo => (so, so),
        }
    }
}

/// An arc or trace across Arcaea's playfield: its start and end beat, where it starts and ends as
/// x and height, and its ease. An x of 0 is the floor's left edge and 1 its right, with the sky
/// half a floor beyond each, and a height of 0 is the floor and 1 the top of the sky.
type Curve = (Beat, Beat, (f32, f32), (f32, f32), Ease);

/// The first two meet from above but not from the side, which the height view shows.
const ARCS: [(Curve, Color32); 8] = [
    (
        (Beat(4.0), Beat(6.0), (0.0, 1.0), (0.5, 0.25), Ease::B),
        BLUE,
    ),
    (
        (Beat(4.0), Beat(6.0), (1.0, 0.0), (0.5, 0.75), Ease::B),
        PINK,
    ),
    (
        (Beat(6.0), Beat(8.0), (0.5, 0.25), (-0.25, 0.25), Ease::Si),
        BLUE,
    ),
    (
        (Beat(6.0), Beat(8.0), (0.5, 0.75), (1.25, 1.0), Ease::So),
        PINK,
    ),
    (
        (Beat(12.0), Beat(14.0), (-0.5, 0.0), (0.25, 1.0), Ease::SiSi),
        BLUE,
    ),
    (
        (Beat(12.0), Beat(14.0), (1.5, 0.0), (0.75, 0.5), Ease::SoSo),
        PINK,
    ),
    (
        (Beat(14.0), Beat(16.0), (0.25, 1.0), (0.75, 0.5), Ease::SiSo),
        BLUE,
    ),
    (
        (Beat(14.0), Beat(16.0), (0.75, 0.5), (0.25, 0.0), Ease::SoSi),
        PINK,
    ),
];

/// A trace, which nobody holds, carrying arctaps.
const TRACE: Curve = (Beat(9.0), Beat(11.0), (0.25, 0.5), (0.75, 1.0), Ease::S);
/// Beats where taps attach to the unheld trace.
const ARCTAPS: [Beat; 3] = [Beat(9.0), Beat(10.0), Beat(11.0)];

/// How many straight pieces draw one curve.
const PIECES: u8 = 24;

/// Beat, lane and optional hold end of the Arcaea floor notes.
const FLOOR: [Press; 12] = [
    (Beat(-1.0), 3, None),
    (Beat(0.0), 0, None),
    (Beat(0.5), 1, None),
    (Beat(1.0), 2, None),
    (Beat(1.5), 3, None),
    (Beat(2.0), 1, Some(Beat(3.0))),
    (Beat(3.0), 2, None),
    (Beat(8.0), 0, None),
    (Beat(8.0), 3, None),
    (Beat(12.0), 1, None),
    (Beat(12.5), 2, None),
    (Beat(13.0), 1, Some(Beat(15.0))),
];

/// The floor and sky seen from above in the playfield, and the sky seen from the side in the
/// height view, the floor at its left edge and the top of the sky at its right.
fn arcaea(painter: &Painter, lanes: &Lanes) {
    let floor = lanes.span(0, lanes.count);
    let margins = [
        Rangef::new(lanes.playfield.min, floor.min),
        Rangef::new(floor.max, lanes.playfield.max),
    ];
    for margin in margins {
        painter.rect_filled(Rect::from_x_y_ranges(margin, lanes.rows), 0.0, SKY);
    }
    presses(painter, lanes, &FLOOR, &[WHITE; 4]);
    sky(painter, lanes, |x, _| floor.min + x * floor.span());
    if let Some(side) = lanes.side {
        let heights = side.shrink(12.0);
        painter.rect_filled(Rect::from_x_y_ranges(heights, lanes.rows), 0.0, SKY);
        painter.vline(heights.min, lanes.rows, Stroke::new(2.0, GREY));
        sky(painter, lanes, |_, height| {
            heights.min + height * heights.span()
        });
    }
}

/// The trace, its arctaps and the arcs in one view, where `across` puts an x and height on
/// screen.
fn sky(painter: &Painter, lanes: &Lanes, across: impl Fn(f32, f32) -> f32) {
    painter.add(Shape::line(
        points(lanes, TRACE, &across),
        Stroke::new(2.0, GREY),
    ));
    let (start, end, ..) = TRACE;
    for tap in ARCTAPS {
        let (x, height) = along(TRACE, (tap.0 - start.0) / (end.0 - start.0));
        let centre = pos2(across(x, height), lanes.y(tap));
        let arctap = Rect::from_center_size(centre, vec2(22.0, 8.0));
        painter.rect_filled(arctap, 2.0, Color32::from_gray(30));
        painter.rect_stroke(arctap, 2.0, Stroke::new(1.5, WHITE), StrokeKind::Inside);
    }
    if let Some(&(selected, _)) = ARCS.first() {
        let halo = Stroke::new(10.0, Color32::WHITE);
        painter.add(Shape::line(points(lanes, selected, &across), halo));
    }
    for (curve, colour) in ARCS {
        painter.add(Shape::line(
            points(lanes, curve, &across),
            Stroke::new(6.0, colour),
        ));
    }
}

/// Where a curve is, as x and height, a fraction `t` of the way through it.
fn along((_, _, from, to, ease): Curve, t: f32) -> (f32, f32) {
    let (x, height) = ease.at(t);
    (
        from.0 + (to.0 - from.0) * x,
        from.1 + (to.1 - from.1) * height,
    )
}

/// A curve as points on screen in one view, close enough together to join with straight lines.
fn points(lanes: &Lanes, curve: Curve, across: impl Fn(f32, f32) -> f32) -> Vec<Pos2> {
    let (start, end, ..) = curve;
    (0..=PIECES)
        .map(|piece| {
            let t = f32::from(piece) / f32::from(PIECES);
            let (x, height) = along(curve, t);
            let beat = Beat(start.0 + (end.0 - start.0) * t);
            pos2(across(x, height), lanes.y(beat))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gesture_points_the_way_its_lane_says() {
        assert_eq!(
            [0, 1, 2].map(|lane| aim(lane, 3)),
            [Aim::Left, Aim::Down, Aim::Right]
        );
        assert_eq!([1, 2].map(|lane| aim(lane, 4)), [Aim::Down, Aim::Down]);
    }

    #[test]
    fn arcaea_eases_across_and_up_apart() {
        let near = |(x, height): (f32, f32), (want_x, want_height): (f32, f32)| {
            (x - want_x).abs() < 1e-5 && (height - want_height).abs() < 1e-5
        };
        let sine = std::f32::consts::FRAC_1_SQRT_2;
        assert!(near(Ease::B.at(0.25), (0.156_25, 0.156_25)));
        assert!(near(Ease::Si.at(0.5), (sine, 0.5)));
        assert!(near(Ease::So.at(0.5), (1.0 - sine, 0.5)));
        assert!(near(Ease::SiSo.at(0.5), (sine, 1.0 - sine)));
        assert!(near(Ease::SoSi.at(0.5), (1.0 - sine, sine)));
    }
}
