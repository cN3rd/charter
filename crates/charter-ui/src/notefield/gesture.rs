use super::draw::reaching;
use super::{
    Action, Chart, Field, Flip, LaneIndex, Note, NoteAt, Object, Pos2, Position, Rangef,
    ResolvedTime, Selecting, Song, Tick, Ticks, Timing, Ui, Valid, View, egui, nearest_line,
};
use charter_core::TimeShift;

/// A drag across the notefield in progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Gesture {
    /// A hold being drawn out of the tap Ctrl placed where the drag began.
    Hold {
        /// Lane where the initial tap was placed.
        lane: LaneIndex,
        /// Tick where the hold begins.
        from: Tick,
    },
    /// A box selecting the notes whose heads it covers, from a corner kept in audio time so the
    /// box stays put if the chart scrolls under it.
    Marquee {
        /// Whether covered notes replace or extend the current selection.
        how: Selecting,
        /// Screen x of the fixed corner.
        x: f32,
        /// Audio time at the fixed corner.
        seconds: ResolvedTime,
    },
    /// The timing point being dragged to another line.
    MoveTiming {
        /// Tick shared by the events at this timing point.
        from: Tick,
    },
    /// The song's marker being dragged to another line.
    MoveMarker {
        /// Index into the song's markers.
        index: usize,
    },
    /// The cursor's handle, dragged from `y` on screen while the cursor was at `seconds`.
    Scrub {
        /// Cursor's audio time when the drag began.
        seconds: ResolvedTime,
        /// Screen height at the start of the drag, in points.
        y: f32,
    },
    /// The chart, grabbed with the middle button from `y` on screen while the cursor was at
    /// `seconds`, and dragged along as a page is.
    Pan {
        /// Cursor's audio time when the drag began.
        seconds: ResolvedTime,
        /// Screen height where the chart was grabbed, in points.
        y: f32,
    },
    /// The selected notes, grabbed on the snap line `tick` over `lane`. They can move `room.0`
    /// lanes left, a negative count, to `room.1` lanes right and stay on the lanes.
    MoveNotes {
        /// Snap line where the notes were grabbed.
        tick: Tick,
        /// Lane where the notes were grabbed.
        lane: LaneIndex,
        /// Permitted lane offsets to the left and right.
        room: (i32, i32),
    },
}

impl Gesture {
    /// How far the selection grabbed in a [`Gesture::MoveNotes`] has been dragged to `pos`, in
    /// ticks along the snap and lanes across: the snap line nearest `pos` from the one it was
    /// grabbed on, and the lane under `pos`, kept within the lanes, from the one it was grabbed
    /// over. With `keep_time`, as while Shift is held, the notes keep their ticks and only
    /// change lanes.
    pub(super) fn moved_by(
        self,
        field: &Field,
        to: Tick,
        pos: Pos2,
        keep_time: bool,
    ) -> Option<(i32, i32)> {
        let Self::MoveNotes { tick, lane, room } = self else {
            return None;
        };
        let ticks = if keep_time {
            0
        } else {
            i32::try_from(i64::from(to).checked_sub(i64::from(tick))?).ok()?
        };
        let inside = field.columns.lanes;
        let over = field.lane_at(pos.x.clamp(inside.min, inside.max - 0.5))?;
        let lanes = i32::from(u16::from(over)).saturating_sub(i32::from(u16::from(lane)));
        Some((ticks, lanes.clamp(room.0, room.1.max(room.0))))
    }
}

/// Which notes a drag moves and how far they can move sideways.
struct MoveRoom {
    /// Whether the grabbed note is already selected, so the whole selection moves.
    selected: bool,
    /// Permitted lane offsets to the left and right.
    lanes: (i32, i32),
}

/// The notes a drag from `note` moves, the selection when `note` is in it and otherwise `note`
/// alone, and how many lanes they have room to move left, a negative count, and right.
fn room(chart: Valid<'_, Chart>, song: &Song, note: NoteAt, lanes: u16) -> MoveRoom {
    let grabbed = |object: &Object| {
        object.tick == note.tick
            && matches!(object.position, Position::Lane { index, .. } if index == note.lane)
    };
    let selected = chart
        .objects
        .iter()
        .any(|object| grabbed(object) && song.is_selected(object));
    let spans = chart
        .objects
        .iter()
        .filter(|object| {
            if selected {
                song.is_selected(object)
            } else {
                grabbed(object)
            }
        })
        .filter_map(|object| match object.position {
            Position::Lane { index, width } => Some((index, width)),
            Position::Plane { .. } => None,
        });
    let (mut first, mut last) = (i32::MAX, i32::MIN);
    for (index, width) in spans {
        let from = i32::from(u16::from(index));
        first = first.min(from);
        last = last.max(
            from.saturating_add(i32::from(u16::from(width)))
                .saturating_sub(1),
        );
    }
    MoveRoom {
        selected,
        lanes: (
            first.saturating_neg(),
            i32::from(lanes).saturating_sub(1).saturating_sub(last),
        ),
    }
}

/// The handle under `pos`, if one is: the cursor's on the ruler, a timing point (every event at one
/// tick) in the gutter or a marker on the ruler, each within grabbing distance of its line. The song's first BPM
/// change stays put, so the song keeps a tempo to start from. Only the cursor's handle moves in a
/// song that cannot be edited.
pub(super) fn handle_at(song: &Song, field: &Field, pos: Pos2) -> Option<Gesture> {
    let (columns, view) = (&field.columns, field.view);
    let near = |y: f32| (y - pos.y).abs() <= GRAB;
    let on_handle = Rangef::new(columns.handle_x() - 9.0, columns.handle_x() + 7.0);
    if on_handle.contains(pos.x) && near(view.now_y) {
        let seconds = view.seconds_at(view.now_y);
        return Some(Gesture::Scrub { seconds, y: pos.y });
    }
    if song.read_only().is_some() {
        return None;
    }
    let timing = song.timing();
    if columns.timing.contains(pos.x) {
        let points = timing
            .events
            .chunk_by(|a, b| a.tick == b.tick)
            .filter_map(|point| point.first().map(|event| event.tick))
            .chain(timing.anchors.iter().map(|anchor| anchor.tick))
            .enumerate();
        return nearest(points, timing, view, pos.y).map(|(_, from)| Gesture::MoveTiming { from });
    }
    if columns.ruler.contains(pos.x) {
        let markers = &song.document().document().markers;
        let ticks = markers
            .iter()
            .enumerate()
            .map(|(index, marker)| (index, marker.tick));
        return nearest(ticks, timing, view, pos.y).map(|(index, _)| Gesture::MoveMarker { index });
    }
    None
}

/// Whichever of `ticks` draws nearest `y` on screen, with its index, when one is within grabbing
/// distance of it.
fn nearest(
    ticks: impl Iterator<Item = (usize, Tick)>,
    timing: Valid<'_, Timing>,
    view: View,
    y: f32,
) -> Option<(usize, Tick)> {
    ticks
        .map(|(index, tick)| ((index, tick), (view.y(timing.time_at(tick)) - y).abs()))
        .filter(|(_, off)| *off <= GRAB)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(position, _)| position)
}

/// Where a pan the middle button grabbed at `y` with the cursor at `seconds` puts the cursor with
/// the pointer at `pos`: the chart follows the pointer, so dragging it down goes back, onto the
/// snap line there.
pub(super) fn panned(
    field: &Field,
    timing: Valid<'_, Timing>,
    step: Option<Ticks>,
    (seconds, y): (ResolvedTime, f32),
    pos: Pos2,
) -> Option<Tick> {
    let earlier =
        TimeShift::from_seconds(f64::from((pos.y - y) / field.view.zoom.points_per_second()))?;
    let at = seconds - earlier;
    Some(match step {
        Some(step) => nearest_line(timing, at, step),
        None => timing.latest_tick_by(at),
    })
}

/// What clicks and drags over the notefield ask for. With Ctrl they place notes, a tap or under
/// Shift a mine, and a drag draws a hold; otherwise a click selects the note under the pointer,
/// Shift adding it and Alt taking it away, and a drag selects with a box.
#[expect(clippy::too_many_lines, reason = "one branch per gesture")]
pub(super) fn pointer(
    ui: &Ui,
    response: &egui::Response,
    song: &Song,
    field: &Field,
    flip: Flip,
    visible: (Tick, Tick),
    gesture: &mut Option<Gesture>,
) -> Vec<Action> {
    let mut actions = Vec::new();
    let Some(chart) = song.chart() else {
        return actions;
    };
    let timing = chart.timing();
    let (modifiers, origin, latest, escaped) = ui.input(|input| {
        let pointer = &input.pointer;
        let escaped = input.key_pressed(egui::Key::Escape);
        (
            input.modifiers,
            pointer.press_origin().map(|at| flip.back(at)),
            pointer.latest_pos().map(|at| flip.back(at)),
            escaped,
        )
    });
    // A gesture lasts as long as its drag, and Escape drops it.
    if escaped || !(response.dragged() || response.drag_stopped()) {
        *gesture = None;
    }
    let step = song
        .document()
        .document()
        .ticks_per_beat
        .note_length(song.snap());
    let line =
        |pos: Pos2| step.map(|step| nearest_line(timing, field.view.seconds_at(pos.y), step));
    let editable = song.read_only().is_none();
    let spot = |pos: Pos2| {
        if !editable || !field.rows.contains(pos.y) {
            return None;
        }
        Some((field.lane_at(pos.x)?, line(pos)?))
    };
    let how = selecting(modifiers);
    if response.clicked()
        && let Some(pos) = latest
    {
        if !modifiers.command {
            actions.push(Action::Select(
                hit_on_layers(song, field, timing, pos, visible),
                how,
            ));
        } else if let Some((lane, tick)) = spot(pos) {
            let note = if modifiers.shift {
                Note::Mine
            } else {
                Note::Tap
            };
            actions.push(Action::Place { tick, lane, note });
        }
    }
    let (middle, secondary) = ui.input(|input| {
        let pointer = &input.pointer;
        (
            pointer.button_down(egui::PointerButton::Middle),
            pointer.secondary_down(),
        )
    });
    if response.drag_started()
        && !escaped
        && let Some(origin) = origin
    {
        *gesture = if middle {
            let seconds = field.view.seconds_at(field.view.now_y);
            Some(Gesture::Pan {
                seconds,
                y: origin.y,
            })
        } else if secondary {
            None
        } else {
            started(song, field, (origin, modifiers), visible, &mut actions)
        };
    }
    if response.dragged()
        && let (Some(Gesture::Pan { seconds, y }), Some(pos)) = (*gesture, latest)
        && let Some(to) = panned(field, timing, step, (seconds, y), pos)
    {
        actions.push(Action::JumpTo(to));
    }
    if response.dragged()
        && let (Some(Gesture::Scrub { seconds, y }), Some(pos)) = (*gesture, latest)
        && let Some(step) = step
        && let Some(later) =
            TimeShift::from_seconds(f64::from((pos.y - y) / field.view.zoom.points_per_second()))
    {
        let to = nearest_line(timing, seconds + later, step);
        actions.push(Action::JumpTo(to));
    }
    if response.dragged()
        && let (Some(Gesture::Hold { lane, from }), Some(pos)) = (*gesture, latest)
        && let Some(to) = line(pos)
    {
        actions.push(Action::Stretch { lane, from, to });
    }
    if response.drag_stopped() {
        let keep_time = modifiers.shift;
        actions.extend(let_go(
            gesture.take(),
            (latest, keep_time),
            field,
            timing,
            line,
        ));
    }
    actions
}

/// The gesture a drag from `origin` starts, and what it asks for at once: a handle's move, the
/// selection's move from a note, a hold drawn with Ctrl, or else a box.
pub(super) fn started(
    song: &Song,
    field: &Field,
    (origin, modifiers): (Pos2, egui::Modifiers),
    visible: (Tick, Tick),
    actions: &mut Vec<Action>,
) -> Option<Gesture> {
    let snapped = |pos: Pos2| snap_line(song, field.view, pos);
    if let Some(handle) = handle_at(song, field, origin) {
        Some(handle)
    } else if let Some((unselected, moving)) =
        grab(song, field, origin, visible, modifiers, snapped)
    {
        if let Some(note) = unselected {
            actions.push(Action::Select(Some(note), selecting(modifiers)));
        }
        Some(moving)
    } else if modifiers.command {
        if song.read_only().is_some() || !field.rows.contains(origin.y) {
            return None;
        }
        let (lane, from) = (field.lane_at(origin.x)?, snapped(origin)?);
        // A note already there is stretched as it is, rather than toggled away.
        if !song.starts_at(from, lane) {
            let note = Note::Tap;
            actions.push(Action::Place {
                tick: from,
                lane,
                note,
            });
        }
        Some(Gesture::Hold { lane, from })
    } else {
        Some(Gesture::Marquee {
            how: selecting(modifiers),
            x: origin.x,
            seconds: field.view.seconds_at(origin.y),
        })
    }
}

/// The gesture a plain drag, or one with Shift, from `origin` over a note starts in an editable
/// song: moving the selection when the note is in it, and otherwise the note alone, returned to
/// be selected first, or added to the selection under Shift.
pub(super) fn grab(
    song: &Song,
    field: &Field,
    origin: Pos2,
    visible: (Tick, Tick),
    modifiers: egui::Modifiers,
    line: impl Fn(Pos2) -> Option<Tick>,
) -> Option<(Option<NoteAt>, Gesture)> {
    // Shift keeps the notes' ticks as they move.
    let plain = modifiers.is_none() || modifiers.shift_only();
    if song.read_only().is_some() || !plain || !field.rows.contains(origin.y) {
        return None;
    }
    let chart = song.chart()?;
    let note = hit_on_layers(song, field, chart.timing(), origin, visible)?;
    let room = room(chart, song, note, field.lanes);
    let gesture = Gesture::MoveNotes {
        tick: line(origin)?,
        lane: field.lane_at(origin.x)?,
        room: room.lanes,
    };
    Some(((!room.selected).then_some(note), gesture))
}

/// Whether the timing point on `tick` is a tempo point, an anchor of the audio.
pub(super) fn anchored(timing: Valid<'_, Timing>, tick: Tick) -> bool {
    timing.anchors.iter().any(|anchor| anchor.tick == tick)
}

/// What a drag asks for when it lets go at `latest`: the hold drawn to there, the handle
/// dropped on the line there, or the notes the box covers selected.
pub(super) fn let_go(
    gesture: Option<Gesture>,
    (latest, keep_time): (Option<Pos2>, bool),
    field: &Field,
    timing: Valid<'_, Timing>,
    line: impl Fn(Pos2) -> Option<Tick>,
) -> Option<Action> {
    let (pos, gesture) = (latest?, gesture?);
    match gesture {
        Gesture::Hold { lane, from } => line(pos).map(|to| Action::Stretch { lane, from, to }),
        // A tempo point is a moment of the audio, so it goes where it is dropped; a point of
        // meters and stops alone is a beat, so it goes to the line there.
        Gesture::MoveTiming { from } if anchored(timing, from) => Some(Action::MovePointTo(
            from,
            field.view.seconds_at(pos.y).seconds(),
        )),
        Gesture::MoveTiming { from } => line(pos).map(|to| Action::MoveTiming { from, to }),
        Gesture::MoveMarker { index } => line(pos).map(|to| Action::MoveMarker { index, to }),
        Gesture::Marquee { how, x, seconds } => {
            field
                .boxed(timing, (x, seconds), pos)
                .map(|(ticks, lanes)| Action::SelectBetween {
                    from_tick: ticks.0,
                    to_tick: ticks.1,
                    from_lane: lanes.0,
                    to_lane: lanes.1,
                    how,
                })
        }
        Gesture::MoveNotes { .. } => {
            let (ticks, lanes) = gesture.moved_by(field, line(pos)?, pos, keep_time)?;
            Some(Action::MoveSelection { ticks, lanes })
        }
        Gesture::Scrub { .. } | Gesture::Pan { .. } => None,
    }
}

/// How a click or a box meets the selection: Shift adds to it and Alt takes from it.
const fn selecting(modifiers: egui::Modifiers) -> Selecting {
    if modifiers.alt {
        Selecting::Subtract
    } else if modifiers.shift {
        Selecting::Add
    } else {
        Selecting::Replace
    }
}

/// How far from a note, in points, a click still takes it.
const GRAB: f32 = 6.0;

/// The note drawn under `pos`, the one whose head is nearest when several are.
#[cfg(test)]
pub(super) fn hit(
    objects: &[Object],
    field: &Field,
    timing: Valid<'_, Timing>,
    pos: Pos2,
    (first, last): (Tick, Tick),
) -> Option<NoteAt> {
    hit_filtered(objects, field, timing, pos, (first, last), |_| true)
}

/// Only shown notes participate in pointer gestures or context-menu selection.
pub(super) fn hit_on_layers(
    song: &Song,
    field: &Field,
    timing: Valid<'_, Timing>,
    pos: Pos2,
    visible: (Tick, Tick),
) -> Option<NoteAt> {
    let chart = song.chart()?;
    hit_filtered(&chart.objects, field, timing, pos, visible, |object| {
        song.layer_look(object).0 == charter_core::LayerView::Shown
    })
}

/// The nearest note whose layer is eligible for selection.
pub(super) fn hit_filtered(
    objects: &[Object],
    field: &Field,
    timing: Valid<'_, Timing>,
    pos: Pos2,
    (first, last): (Tick, Tick),
    eligible: impl Fn(&Object) -> bool,
) -> Option<NoteAt> {
    let under = reaching(objects, first, last).filter_map(|index| {
        let object = objects.get(index)?;
        if !eligible(object) {
            return None;
        }
        let Position::Lane {
            index: lane_index,
            width,
        } = object.position
        else {
            return None;
        };
        let head = field.view.y(timing.time_at(object.tick));
        let tail = field.view.y(timing.time_at(object.shape.end(object.tick)?));
        let reach = Rangef::new(head.min(tail) - GRAB, head.max(tail) + GRAB);
        let over = field.columns.span(lane_index, width).contains(pos.x) && reach.contains(pos.y);
        let note = NoteAt {
            tick: object.tick,
            lane: lane_index,
        };
        over.then_some((note, (pos.y - head).abs()))
    });
    under
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(note, _)| note)
}

/// The snap line nearest `pos` on screen.
pub(super) fn snap_line(song: &Song, view: View, pos: Pos2) -> Option<Tick> {
    let step = song
        .document()
        .document()
        .ticks_per_beat
        .note_length(song.snap())?;
    Some(nearest_line(song.timing(), view.seconds_at(pos.y), step))
}
