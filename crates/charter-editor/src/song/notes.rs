//! Note placement and transformations.

#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "song submodules share one private implementation vocabulary"
    )
)]
use super::*;

impl Song {
    /// Places an instant note of `kind` on `lane` at `tick`, or removes the object that starts there.
    ///
    /// # Errors
    ///
    /// Refuses on read-only songs, missing charts, format errors or game rejections.
    pub fn toggle(&mut self, tick: Tick, lane: LaneIndex, kind: Kind) -> Result<Toggled, Refusal> {
        let active = self.editable_layer(self.active_layer())?.cloned();
        let chart = self.chart().ok_or(Refusal::NoChart)?;
        let (edit, toggled) = if let Some((index, object)) = self.starting(tick, lane) {
            if !self.is_editable(object)
                || chart.layer_index(object).unwrap_or(0) != self.active_layer()
            {
                return Err(Refusal::OnLayer(self.object_layer_name(object)));
            }
            let object = object.clone();
            (Edit(vec![Op::Remove { index, object }]), Toggled::Removed)
        } else {
            let index = chart.objects.partition_point(|object| object.tick <= tick);
            let mut object = note(tick, lane, kind);
            object.layer = active;
            (Edit(vec![Op::Insert { index, object }]), Toggled::Placed)
        };
        self.document = self.edited(self.chart, &edit)?;
        self.record(edit);
        Ok(toggled)
    }

    /// Makes the tap or hold that starts on `lane` at `from` end at `to`: a hold when `to` is
    /// later, a tap otherwise. When the edit before placed that note, the two undo as one step.
    /// Does nothing when no tap or hold starts there.
    ///
    /// # Errors
    ///
    /// Refuses when the note's new length would break a rule of the format or of the song's
    /// game.
    pub fn stretch(&mut self, lane: LaneIndex, from: Tick, to: Tick) -> Result<(), Refusal> {
        let Some((index, old)) = self.starting(from, lane) else {
            return Ok(());
        };
        if !self.is_editable(old) {
            return Err(Refusal::OnLayer(self.object_layer_name(old)));
        }
        if !matches!(old.kind, Kind::Tap | Kind::Hold) {
            return Ok(());
        }
        let mut new = old.clone();
        (new.kind, new.shape) = match from.ticks_until(to) {
            Some(duration) => (Kind::Hold, Shape::Sustain { duration }),
            None => (Kind::Tap, Shape::Instant),
        };
        if new == *old {
            return Ok(());
        }
        let old = old.clone();
        let edit = Edit(vec![Op::Replace { index, old, new }]);
        self.document = self.edited(self.chart, &edit)?;
        self.record(edit);
        Ok(())
    }
}

impl Song {
    /// Whether a note of the open chart starts at `tick` on `lane`.
    #[must_use]
    pub fn starts_at(&self, tick: Tick, lane: LaneIndex) -> bool {
        self.starting(tick, lane).is_some()
    }

    /// The object of the open chart that starts at `tick` and covers `lane`, and where it is.
    pub(super) fn starting(&self, tick: Tick, lane: LaneIndex) -> Option<(usize, &Object)> {
        let objects = &self.document.document().charts.get(self.chart)?.objects;
        let from = objects.partition_point(|object| object.tick < tick);
        let to = objects.partition_point(|object| object.tick <= tick);
        let key = LaneSpan::new(lane, LaneCount::ONE);
        let here = objects.get(from..to).unwrap_or_default();
        here.iter().enumerate().find_map(|(offset, object)| {
            let covers = object
                .position
                .lanes()
                .is_some_and(|span| span.overlaps(key));
            covers.then_some((from.saturating_add(offset), object))
        })
    }
}

impl Song {
    /// Removes every selected note, as one step.
    ///
    /// # Errors
    ///
    /// Refuses on read-only songs, format errors or game rejections.
    pub fn delete_selection(&mut self) -> Result<(), Refusal> {
        let Some(chart) = self.chart() else {
            return Ok(());
        };
        // Removing from the back keeps the index of every note still to remove.
        let removals = chart.objects.iter().enumerate().rev();
        let ops: Vec<_> = removals
            .filter(|(_, object)| self.is_selected(object))
            .map(|(index, object)| Op::Remove {
                index,
                object: object.clone(),
            })
            .collect();
        if ops.is_empty() {
            return Ok(());
        }
        let edit = Edit(ops);
        self.document = self.edited(self.chart, &edit)?;
        self.record(edit);
        Ok(())
    }

    /// Moves the end of each selected tap and hold, or of those starting at the cursor when none
    /// is selected, to the snap's next line later or earlier, as one step: a tap lengthens to a
    /// hold, and a hold shortened to its own tick or before becomes a tap.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, and when the new lengths break a rule of the format or of the
    /// song's game; the notes stay as they were.
    pub fn resize_notes(&mut self, step: Step) -> Result<(), Refusal> {
        let (Some(chart), Some(length)) = (self.chart(), self.length_of(self.snap)) else {
            return Ok(());
        };
        let chosen = |object: &Object| {
            if self.selection.is_empty() {
                object.tick == self.cursor && note_at(object).is_some() && self.is_editable(object)
            } else {
                self.is_selected(object)
            }
        };
        let mut ops = Vec::new();
        for (index, old) in chart.objects.iter().enumerate() {
            if !matches!(old.kind, Kind::Tap | Kind::Hold) || !chosen(old) {
                continue;
            }
            let Some(end) = old.shape.end(old.tick) else {
                continue;
            };
            let moved = match step {
                Step::Later => end.next_on(length),
                Step::Earlier => end.previous_on(length),
            };
            let mut new = old.clone();
            (new.kind, new.shape) =
                match moved.and_then(|moved_end| old.tick.ticks_until(moved_end)) {
                    Some(duration) => (Kind::Hold, Shape::Sustain { duration }),
                    None => (Kind::Tap, Shape::Instant),
                };
            if new != *old {
                let old = old.clone();
                ops.push(Op::Replace { index, old, new });
            }
        }
        if ops.is_empty() {
            return Ok(());
        }
        let edit = Edit(ops);
        self.document = self.edited(self.chart, &edit)?;
        self.record(edit);
        Ok(())
    }

    /// Moves every selected note `ticks` later, earlier when negative, and `lanes` to the right,
    /// left when negative, with its children and path, as one step that keeps them selected.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, when a note would leave the lanes or the ticks, when one
    /// would start where a note that is not moving starts on a lane it covers, and when the move
    /// breaks a rule of the format or of the song's game. The notes stay as they were.
    pub fn move_selection(&mut self, ticks: i32, lanes: i32) -> Result<(), Refusal> {
        if (ticks, lanes) == (0, 0) {
            return Ok(());
        }
        let width = self.lane_count();
        self.replace_selection(|object| {
            let moved = object
                .shifted_by(ticks, lanes)
                .ok_or(Refusal::OffTheChart)?;
            moved
                .position
                .fits_in(width)
                .then_some(moved)
                .ok_or(Refusal::OffTheChart)
        })
    }

    /// Moves every selected note to the snap's line nearest it, holds keeping their length, as one
    /// step that keeps them selected: for notes placed by ear or while playing.
    ///
    /// # Errors
    ///
    /// Refuses as [`Song::move_selection`] does.
    pub fn quantize_selection(&mut self) -> Result<(), Refusal> {
        let Some(step) = self.length_of(self.snap) else {
            return Ok(());
        };
        let step = i64::from(u32::from(step));
        self.replace_selection(|object| {
            let tick = i64::from(object.tick);
            let nearest = tick
                .saturating_add(step.div_euclid(2))
                .div_euclid(step)
                .saturating_mul(step);
            let by = i32::try_from(nearest.saturating_sub(tick))
                .map_err(|_out_of_range| Refusal::OffTheChart)?;
            object.shifted_by(by, 0).ok_or(Refusal::OffTheChart)
        })
    }

    /// Flips every selected note across the chart, its first lane becoming its last, as one step
    /// that keeps them selected.
    ///
    /// # Errors
    ///
    /// Refuses as [`Song::move_selection`] does.
    pub fn mirror_selection(&mut self) -> Result<(), Refusal> {
        let width = self.lane_count();
        let flip = |position: Position| match position {
            Position::Lane { index, width: span } => {
                let right = u16::from(index).saturating_add(u16::from(span));
                let width = width?;
                Some(Position::Lane {
                    index: LaneIndex::new(u16::from(width).checked_sub(right)?),
                    width: span,
                })
            }
            Position::Plane { .. } => Some(position),
        };
        self.replace_selection(|object| {
            object
                .map_ticks_positions(Some, flip)
                .ok_or(Refusal::OffTheChart)
        })
    }

    /// Makes every selected note what the song's game makes it by its change `change`, such as
    /// a tap into another kind of note, as one step that keeps them selected. Notes the change
    /// does not apply to stay as they are, and notes it makes the same merge into one.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, and as [`Song::move_selection`] does.
    pub fn change_selection(&mut self, change: charter_core::ChangeIndex) -> Result<(), Refusal> {
        let plugin = self.plugin().ok_or(Refusal::ReadOnly)?;
        let change = plugin
            .note_changes()
            .get(change.get())
            .ok_or(Refusal::NoChart)?
            .apply;
        let lanes = self
            .chart()
            .and_then(|chart| chart.lanes)
            .ok_or(Refusal::NoChart)?;
        self.replace_selection(|object| {
            let mut changed = change(object, lanes).unwrap_or_else(|| object.clone());
            changed.layer.clone_from(&object.layer);
            Ok(changed)
        })
    }

    /// The selected notes, earliest first, as a clipboard holds them: their ticks counted from
    /// the earliest's.
    #[must_use]
    pub fn copied(&self) -> Vec<Object> {
        let Some(chart) = self.chart() else {
            return Vec::new();
        };
        let mut selected = chart
            .objects
            .iter()
            .filter(|object| self.is_selected(object));
        let Some(first) = selected.next() else {
            return Vec::new();
        };
        let origin = i64::from(first.tick);
        let back = |tick: Tick| {
            i32::try_from(i64::from(tick).saturating_sub(origin))
                .map(Tick::new)
                .map_err(|_out_of_range| Refusal::NotWhole)
        };
        std::iter::once(first)
            .chain(selected)
            .filter_map(|object| shifted_at(object, back).ok())
            .collect()
    }

    /// Adds the notes of `clip`, as [`Song::copied`] gives them, starting at `at`, as one step,
    /// and selects them. Notes past the chart's lanes, or where a note starts on a lane they
    /// cover, are left out. Says how many were added.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, and when the notes break a rule of the format or of the
    /// song's game.
    pub fn paste(&mut self, clip: &[Object], at: Tick) -> Result<usize, Refusal> {
        self.paste_with_layers(clip, at, false)
    }

    /// Duplicate selected notes within the open chart, keeping their original layers.
    ///
    /// # Errors
    /// Refuses read-only songs, invalid placements and plugin rejections.
    pub fn duplicate_selection(&mut self, at: Tick) -> Result<usize, Refusal> {
        let clip = self.copied();
        self.paste_with_layers(&clip, at, true)
    }

    /// Paste clipboard notes, optionally keeping the source layer for a same-chart duplicate.
    fn paste_with_layers(
        &mut self,
        clip: &[Object],
        at: Tick,
        preserve_layers: bool,
    ) -> Result<usize, Refusal> {
        let offset = i64::from(at);
        let on = |tick: Tick| {
            i32::try_from(i64::from(tick).saturating_add(offset))
                .map(Tick::new)
                .map_err(|_out_of_range| Refusal::OffTheChart)
        };
        let placed: Vec<Object> = clip
            .iter()
            .filter_map(|object| shifted_at(object, on).ok())
            .collect();
        self.add(placed, preserve_layers)
    }

    /// Adds a tap on each lane at each tick of `taps`, as one step, and selects them, leaving out
    /// those where a note already starts. Says how many were added.
    ///
    /// # Errors
    ///
    /// Refuses as [`Song::paste`] does.
    pub fn add_taps(
        &mut self,
        taps: impl IntoIterator<Item = (Tick, LaneIndex)>,
    ) -> Result<usize, Refusal> {
        let taps = taps
            .into_iter()
            .map(|(tick, lane)| note(tick, lane, Kind::Tap))
            .collect();
        self.add(taps, false)
    }

    /// The open chart's lanes, or none without a chart.
    fn lane_count(&self) -> Option<LaneCount> {
        self.chart()?.lanes
    }

    /// Adds `objects` as one step and selects them, leaving out those past the lanes, where a
    /// note already starts on a lane they cover, or the same as one added before them.
    fn add(&mut self, mut objects: Vec<Object>, preserve_layers: bool) -> Result<usize, Refusal> {
        let active = if preserve_layers {
            None
        } else {
            self.editable_layer(self.active_layer())?.cloned()
        };
        let Some(chart) = self.chart() else {
            return Err(Refusal::NoChart);
        };
        let width = self.lane_count();
        if !preserve_layers {
            for object in &mut objects {
                object.layer.clone_from(&active);
            }
        }
        objects.sort_by_key(|object| object.tick);
        let mut all: Vec<&Object> = chart.objects.iter().collect();
        let mut ops = Vec::new();
        let mut selection = BTreeSet::new();
        for object in &objects {
            if !object.position.fits_in(width) {
                continue;
            }
            if let Some(blocker) = blocking(&all, object) {
                if !self.same_layer(blocker, object) {
                    return Err(Refusal::OnLayer(self.object_layer_name(blocker)));
                }
                continue;
            }
            let index = all.partition_point(|other| other.tick <= object.tick);
            selection.extend(note_at(object));
            all.insert(index, object);
            ops.push(Op::Insert {
                index,
                object: object.clone(),
            });
        }
        let added = ops.len();
        if added == 0 {
            return Ok(0);
        }
        let edit = Edit(ops);
        self.document = self.edited(self.chart, &edit)?;
        self.selection = selection;
        self.record(edit);
        Ok(added)
    }

    /// Puts each selected note where `change` puts it, as one step that keeps them selected.
    /// A changed note the same as one changed before it merges into it.
    fn replace_selection(
        &mut self,
        change: impl Fn(&Object) -> Result<Object, Refusal>,
    ) -> Result<(), Refusal> {
        let Some(chart) = self.chart() else {
            return Ok(());
        };
        if self.selection.is_empty() {
            return Ok(());
        }
        // Removing from the back keeps the index of every note still to remove.
        let mut ops = Vec::new();
        let mut changed = Vec::new();
        for (index, object) in chart.objects.iter().enumerate().rev() {
            if self.is_selected(object) {
                ops.push(Op::Remove {
                    index,
                    object: object.clone(),
                });
                changed.push(change(object)?);
            }
        }
        // A change that leaves every note as it was is no edit, and no step to undo.
        let same = chart
            .objects
            .iter()
            .rev()
            .filter(|object| self.is_selected(object))
            .eq(changed.iter());
        if same {
            return Ok(());
        }
        let mut objects: Vec<&Object> = chart
            .objects
            .iter()
            .filter(|object| !self.is_selected(object))
            .collect();
        changed.reverse();
        let mut selection = BTreeSet::new();
        for object in &changed {
            if ops
                .iter()
                .any(|op| matches!(op, Op::Insert { object: placed, .. } if placed == object))
            {
                continue;
            }
            if let Some(blocker) = blocking(&objects, object) {
                return Err(if self.same_layer(blocker, object) {
                    Refusal::Occupied
                } else {
                    Refusal::OnLayer(self.object_layer_name(blocker))
                });
            }
            let index = objects.partition_point(|other| other.tick <= object.tick);
            selection.extend(note_at(object));
            objects.insert(index, object);
            ops.push(Op::Insert {
                index,
                object: object.clone(),
            });
        }
        let edit = Edit(ops);
        self.document = self.edited(self.chart, &edit)?;
        self.selection = selection;
        self.record(edit);
        Ok(())
    }
}

/// Whether another object starts at `object`'s tick on one of its lanes.
pub(super) fn taken<T: std::borrow::Borrow<Object>>(objects: &[T], object: &Object) -> bool {
    blocking(objects, object).is_some()
}

/// First note occupying the same starting tick and lane span.
pub(super) fn blocking<'a, T: std::borrow::Borrow<Object>>(
    objects: &'a [T],
    object: &Object,
) -> Option<&'a Object> {
    let from = objects.partition_point(|other| other.borrow().tick < object.tick);
    let to = objects.partition_point(|other| other.borrow().tick <= object.tick);
    let span = object.position.lanes();
    objects
        .get(from..to)
        .unwrap_or_default()
        .iter()
        .find_map(|other| {
            other
                .borrow()
                .position
                .lanes()
                .zip(span)
                .is_some_and(|(other, span)| other.overlaps(span))
                .then_some(other.borrow())
        })
}

/// The note an object is, when it sits on lanes.
pub(super) const fn note_at(object: &Object) -> Option<NoteAt> {
    match object.position {
        Position::Lane { index, .. } => Some(NoteAt {
            tick: object.tick,
            lane: index,
        }),
        Position::Plane { .. } => None,
    }
}

/// The floating-point tick span used in tempo interpolation.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "tempo math uses floating-point tick spans"
)]
pub(super) fn tick_delta_as_f64(to: Tick, from: Tick) -> f64 {
    i64::from(to).saturating_sub(i64::from(from)) as f64
}

/// Rounded tempo interpolation, checked as a tick by the caller.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "the caller checks the resulting tick range"
)]
pub(super) const fn rounded_tick_delta(value: f64) -> i64 {
    value.round() as i64
}

/// `tick` moved `by` ticks, if that is still a tick.
pub(super) fn moved(tick: Tick, by: i64) -> Result<Tick, Refusal> {
    i32::try_from(i64::from(tick).saturating_add(by))
        .map(Tick::new)
        .map_err(|_out_of_range| Refusal::NotWhole)
}

/// `object` with every tick it holds, its children's and its path's, and its end, moved by
/// `shift`. A hold whose end would no longer come after its start is refused.
pub(super) fn shifted_at(
    object: &Object,
    shift: impl Fn(Tick) -> Result<Tick, Refusal>,
) -> Result<Object, Refusal> {
    let mut moved = object.clone();
    moved.tick = shift(object.tick)?;
    moved.shape = shifted_shape(&object.shape, (object.tick, moved.tick), &shift)?;
    for (child, was) in moved.children.iter_mut().zip(&object.children) {
        child.tick = shift(was.tick)?;
        child.shape = shifted_shape(&was.shape, (was.tick, child.tick), &shift)?;
    }
    Ok(moved)
}

/// `shape`, of a note that started at `from` and starts at `to` now, with its end or its path's
/// ticks through `shift`.
pub(super) fn shifted_shape(
    shape: &Shape,
    (from, to): (Tick, Tick),
    shift: impl Fn(Tick) -> Result<Tick, Refusal>,
) -> Result<Shape, Refusal> {
    Ok(match shape {
        Shape::Instant => Shape::Instant,
        Shape::Sustain { duration } => {
            let end = shift(from.checked_add(*duration).ok_or(Refusal::NotWhole)?)?;
            Shape::Sustain {
                duration: to.ticks_until(end).ok_or(Refusal::Crosses(from))?,
            }
        }
        Shape::Path { points } => {
            let moved_points = points
                .as_slice()
                .iter()
                .map(|point| {
                    let mut point = point.clone();
                    point.tick = shift(point.tick)?;
                    Ok(point)
                })
                .collect::<Result<Vec<_>, Refusal>>()?;
            Shape::Path {
                points: charter_core::Points::new(moved_points)
                    .map_err(|_invalid_points| Refusal::NotWhole)?,
            }
        }
    })
}

/// An instant note of `kind` on `lane` at `tick`.
pub(super) const fn note(tick: Tick, lane: LaneIndex, kind: Kind) -> Object {
    Object {
        tick,
        position: Position::Lane {
            index: lane,
            width: LaneCount::ONE,
        },
        kind,
        shape: Shape::Instant,
        layer: None,
        sample: None,
        children: Vec::new(),
        ext: Extensions::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::song::tests::*;
    use serde_json::{Value, json};

    fn objects(song: &Song) -> Vec<Value> {
        let chart = song.chart().unwrap();
        chart
            .objects
            .iter()
            .map(|o| serde_json::to_value(o).unwrap())
            .collect()
    }

    fn lane(index: u16) -> LaneIndex {
        LaneIndex::new(index)
    }

    #[test]
    fn a_lane_key_places_a_tap_at_the_cursor_among_the_notes_around_it() {
        let mut song = Song::new(
            document(json!([object(0, 0, "tap"), object(480, 1, "tap")])),
            EDITABLE,
        );
        song.step(Step::Later);
        assert_eq!(
            song.toggle(song.cursor(), lane(2), Kind::Tap).unwrap(),
            Toggled::Placed
        );
        assert_eq!(
            objects(&song),
            [
                object(0, 0, "tap"),
                object(240, 2, "tap"),
                object(480, 1, "tap")
            ]
        );
    }

    #[test]
    fn a_mine_goes_where_a_tap_would() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        assert_eq!(
            song.toggle(at(0), lane(1), Kind::Mine).unwrap(),
            Toggled::Placed
        );
        assert_eq!(objects(&song), [object(0, 0, "tap"), object(0, 1, "mine")]);
    }

    #[test]
    fn a_note_placed_away_from_the_cursor_leaves_the_cursor_and_undoes_back_to_it() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.toggle(at(480), lane(2), Kind::Tap).unwrap();
        assert_eq!(objects(&song), [object(480, 2, "tap")]);
        assert_eq!(song.cursor(), at(0));
        song.step(Step::Later);
        song.undo().unwrap();
        assert_eq!(song.cursor(), at(0));
    }

    fn hold(tick: i32, lane: u16, duration: u32) -> Value {
        json!({ "tick": tick, "position": { "lane": { "index": lane } }, "kind": "hold",
                "shape": { "sustain": { "duration": duration } }, "layer": "Layer 0" })
    }

    #[test]
    fn stretching_a_tap_past_its_tick_makes_it_a_hold_to_there() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        song.stretch(lane(0), at(0), at(480)).unwrap();
        assert_eq!(objects(&song), [hold(0, 0, 480)]);
    }

    #[test]
    fn stretching_a_hold_back_to_its_own_tick_makes_it_a_tap_again() {
        let mut song = Song::new(document(json!([hold(0, 0, 480)])), EDITABLE);
        song.stretch(lane(0), at(0), at(0)).unwrap();
        assert_eq!(objects(&song), [object(0, 0, "tap")]);
    }

    #[test]
    fn stretching_an_older_note_is_a_step_of_its_own() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        song.toggle(at(240), lane(1), Kind::Tap).unwrap();
        song.stretch(lane(0), at(0), at(480)).unwrap();
        song.undo().unwrap();
        assert_eq!(objects(&song), [object(0, 0, "tap"), object(240, 1, "tap")]);
    }

    #[test]
    fn lengthening_makes_a_selected_tap_a_hold_to_the_next_line_and_a_hold_longer() {
        let mut song = Song::new(
            document(json!([
                object(0, 0, "tap"),
                hold(0, 1, 100),
                object(0, 2, "tap")
            ])),
            EDITABLE,
        );
        let notes = [0, 1].map(|index| NoteAt {
            tick: at(0),
            lane: lane(index),
        });
        song.select(notes, Selecting::Replace);
        song.resize_notes(Step::Later).unwrap();
        assert_eq!(
            objects(&song),
            [hold(0, 0, 240), hold(0, 1, 240), object(0, 2, "tap")]
        );
        song.resize_notes(Step::Later).unwrap();
        assert_eq!(
            objects(&song),
            [hold(0, 0, 480), hold(0, 1, 480), object(0, 2, "tap")]
        );
        assert_eq!(song.selection().count(), 2);
        song.undo().unwrap();
        assert_eq!(
            objects(&song),
            [hold(0, 0, 240), hold(0, 1, 240), object(0, 2, "tap")]
        );
    }

    #[test]
    fn shortening_takes_a_hold_back_a_line_and_one_line_long_to_a_tap() {
        let mut song = Song::new(
            document(json!([hold(0, 0, 300), object(240, 1, "tap")])),
            EDITABLE,
        );
        song.resize_notes(Step::Earlier).unwrap();
        assert_eq!(objects(&song), [hold(0, 0, 240), object(240, 1, "tap")]);
        song.resize_notes(Step::Earlier).unwrap();
        assert_eq!(objects(&song), [object(0, 0, "tap"), object(240, 1, "tap")]);
        let steps = song.history_step();
        song.resize_notes(Step::Earlier).unwrap();
        assert_eq!(song.history_step(), steps, "a tap cannot shorten");
    }

    #[test]
    fn without_a_selection_the_notes_at_the_cursor_resize() {
        let mut song = Song::new(
            document(json!([
                object(0, 0, "tap"),
                object(240, 1, "tap"),
                object(240, 2, "mine")
            ])),
            EDITABLE,
        );
        song.step(Step::Later);
        song.resize_notes(Step::Later).unwrap();
        assert_eq!(
            objects(&song),
            [
                object(0, 0, "tap"),
                hold(240, 1, 240),
                object(240, 2, "mine")
            ]
        );
    }

    /// Offers one change, "across", which makes a note cover every lane.
    #[derive(Debug)]
    struct Across;

    #[expect(
        clippy::missing_trait_methods,
        reason = "a test plugin: the trait's defaults for every optional method"
    )]
    impl RulesetPlugin for Across {
        fn id(&self) -> &'static str {
            "test"
        }

        fn check(&self, _: &ValidDocument) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        fn note_changes(&self) -> Vec<charter_core::NoteChange> {
            vec![charter_core::NoteChange {
                name: "Across".to_owned(),
                about: "Widen to every lane".to_owned(),
                apply: across,
            }]
        }
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "the signature is fixed by NoteChange::apply"
    )]
    fn across(object: &Object, lanes: LaneCount) -> Option<Object> {
        Some(Object {
            position: Position::Lane {
                index: LaneIndex::new(0),
                width: lanes,
            },
            ..object.clone()
        })
    }

    fn select_everything(song: &mut Song) {
        song.select_all();
    }

    #[test]
    fn copied_notes_paste_at_another_tick_as_one_step_and_come_out_selected() {
        let mut song = Song::new(
            document(json!([object(240, 0, "tap"), hold(480, 1, 240)])),
            EDITABLE,
        );
        select_everything(&mut song);
        let clip = song.copied();
        assert_eq!(clip.first().unwrap().tick, at(0));
        assert_eq!(song.paste(&clip, at(1920)).unwrap(), 2);
        assert_eq!(
            objects(&song),
            [
                object(240, 0, "tap"),
                hold(480, 1, 240),
                object(1920, 0, "tap"),
                hold(2160, 1, 240)
            ]
        );
        assert_eq!(song.selection().count(), 2);
        song.undo().unwrap();
        assert_eq!(objects(&song).len(), 2);
    }

    #[test]
    fn a_paste_or_taps_leave_out_places_already_taken() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        let added = song
            .add_taps([(at(0), lane(0)), (at(0), lane(1)), (at(240), lane(9))])
            .unwrap();
        assert_eq!(added, 1);
        assert_eq!(objects(&song), [object(0, 0, "tap"), object(0, 1, "tap")]);
    }

    #[test]
    fn quantizing_moves_the_selection_to_the_nearest_snap_line() {
        let mut song = Song::new(
            document(json!([
                object(130, 2, "tap"),
                object(250, 0, "tap"),
                hold(470, 1, 100)
            ])),
            EDITABLE,
        );
        select_everything(&mut song);
        song.quantize_selection().unwrap();
        assert_eq!(
            objects(&song),
            [
                object(240, 2, "tap"),
                object(240, 0, "tap"),
                hold(480, 1, 100)
            ]
        );
    }

    #[test]
    fn quantizing_notes_already_on_the_grid_is_no_step() {
        let mut song = Song::new(document(json!([object(240, 0, "tap")])), EDITABLE);
        select_everything(&mut song);
        song.quantize_selection().unwrap();
        assert!(!song.can_undo());
        assert!(!song.is_modified());
    }

    #[test]
    fn a_pasted_note_takes_its_child_s_path_along() {
        let parent = json!({ "tick": 0, "position": { "lane": { "index": 0 } }, "kind": "hold",
            "shape": { "sustain": { "duration": 480 } }, "children": [ { "tick": 0,
            "position": { "lane": { "index": 0 } }, "kind": "tap", "shape": { "path": {
            "points": [ { "tick": 240, "position": { "lane": { "index": 1 } },
            "ease": "linear" } ] } } } ] });
        let mut song = Song::new(document(json!([parent])), EDITABLE);
        select_everything(&mut song);
        let clip = song.copied();
        assert_eq!(song.paste(&clip, at(960)).unwrap(), 1);
        let pasted = objects(&song);
        let child = &pasted[1]["children"][0];
        assert_eq!(child["shape"]["path"]["points"][0]["tick"], 1200);
    }

    #[test]
    fn mirroring_flips_the_selection_across_the_lanes() {
        let mut song = Song::new(
            document(json!([object(0, 0, "tap"), object(240, 2, "tap")])),
            EDITABLE,
        );
        select_everything(&mut song);
        song.mirror_selection().unwrap();
        assert_eq!(objects(&song), [object(0, 3, "tap"), object(240, 1, "tap")]);
        assert_eq!(song.selection().count(), 2);
    }

    #[test]
    fn mirroring_flips_a_path_and_its_children_too() {
        let path = json!({ "tick": 0, "position": { "lane": { "index": 0 } }, "kind": "tap",
            "shape": { "path": { "points": [
                { "tick": 240, "position": { "lane": { "index": 1 } }, "ease": "linear" } ] } } });
        let mut song = Song::new(document(json!([path])), EDITABLE);
        select_everything(&mut song);
        song.mirror_selection().unwrap();
        let mirrored = objects(&song);
        let mirrored = mirrored.first().unwrap();
        assert_eq!(mirrored["position"]["lane"]["index"], 3);
        assert_eq!(
            mirrored["shape"]["path"]["points"][0]["position"]["lane"]["index"],
            2
        );
    }

    #[test]
    fn mirroring_flips_a_child_s_path_too() {
        let parent = json!({ "tick": 0, "position": { "lane": { "index": 0 } }, "kind": "hold",
            "shape": { "sustain": { "duration": 480 } }, "children": [ { "tick": 0, "position": { "lane": { "index": 0 } },
            "kind": "tap", "shape": { "path": { "points": [
                { "tick": 240, "position": { "lane": { "index": 1 } }, "ease": "linear" } ] } } } ] });
        let mut song = Song::new(document(json!([parent])), EDITABLE);
        select_everything(&mut song);
        song.mirror_selection().unwrap();
        let mirrored = objects(&song);
        let child = &mirrored.first().unwrap()["children"][0];
        assert_eq!(child["position"]["lane"]["index"], 3);
        assert_eq!(
            child["shape"]["path"]["points"][0]["position"]["lane"]["index"],
            2
        );
    }

    #[test]
    fn moving_a_lane_past_the_last_is_refused() {
        let mut song = Song::new(document(json!([object(0, 3, "tap")])), EDITABLE);
        select_everything(&mut song);
        assert!(matches!(
            song.move_selection(0, 1),
            Err(Refusal::OffTheChart)
        ));
        song.move_selection(0, -2).unwrap();
        assert_eq!(objects(&song), [object(0, 1, "tap")]);
    }

    #[test]
    fn a_game_change_applies_to_the_selection_and_merges_notes_it_makes_the_same() {
        let mut song = Song::new(
            document(json!([object(0, 0, "tap"), object(0, 2, "tap")])),
            &[&Across],
        );
        select_everything(&mut song);
        song.change_selection(charter_core::ChangeIndex::new(0))
            .unwrap();
        let across = json!({ "tick": 0, "position": { "lane": { "index": 0, "width": 4 } },
                             "kind": "tap", "shape": "instant", "layer": "Layer 0" });
        assert_eq!(objects(&song), [across]);
        // A change the plugin does not offer is now unrepresentable: there is no id to get wrong.
        assert_eq!(objects(&song).len(), 1);
    }

    #[test]
    fn a_stretch_the_game_refuses_leaves_the_note_as_it_was() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), &[&NoHolds]);
        let refusal = song.stretch(lane(0), at(0), at(480)).unwrap_err();
        assert_eq!(refusal.to_string(), "a hold");
        assert_eq!(objects(&song), [object(0, 0, "tap")]);
    }

    #[test]
    fn only_taps_and_holds_stretch() {
        let mut song = Song::new(document(json!([object(0, 0, "mine")])), EDITABLE);
        song.stretch(lane(0), at(0), at(480)).unwrap();
        assert_eq!(objects(&song), [object(0, 0, "mine")]);
        assert!(!song.can_undo());
    }

    #[test]
    fn a_lane_key_removes_whatever_starts_at_the_cursor_on_its_lane() {
        let wide = json!({ "tick": 0, "position": { "lane": { "index": 2, "width": 2 } },
                           "kind": "hold", "shape": { "sustain": { "duration": 480 } } });
        let mut song = Song::new(document(json!([object(0, 0, "mine"), wide])), EDITABLE);
        assert_eq!(
            song.toggle(song.cursor(), lane(0), Kind::Tap).unwrap(),
            Toggled::Removed
        );
        assert_eq!(
            song.toggle(song.cursor(), lane(3), Kind::Tap).unwrap(),
            Toggled::Removed
        );
        assert_eq!(objects(&song), Vec::<Value>::new());
    }

    #[test]
    fn a_hold_that_started_earlier_leaves_the_lane_free_at_the_cursor() {
        let hold = json!({ "tick": 0, "position": { "lane": { "index": 0 } },
                           "kind": "hold", "shape": { "sustain": { "duration": 960 } } });
        let mut song = Song::new(document(json!([hold])), EDITABLE);
        song.step(Step::Later);
        assert_eq!(
            song.toggle(song.cursor(), lane(0), Kind::Tap).unwrap(),
            Toggled::Placed
        );
        assert_eq!(objects(&song), [hold, object(240, 0, "tap")]);
    }

    #[test]
    fn an_edit_that_breaks_a_format_rule_is_refused_and_changes_nothing() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        let refusal = song.toggle(song.cursor(), lane(4), Kind::Tap).unwrap_err();
        assert!(matches!(refusal, Refusal::Document(_)), "{refusal:?}");
        assert_eq!(
            refusal.to_string(),
            "charts[0].objects[1]: position covers a lane past the last of 4"
        );
        assert_eq!(objects(&song), [object(0, 0, "tap")]);
    }

    #[test]
    fn an_edit_the_game_refuses_is_refused_and_changes_nothing() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), &[&NoLastLane]);
        let refusal = song.toggle(song.cursor(), lane(3), Kind::Tap).unwrap_err();
        assert!(matches!(refusal, Refusal::Game(_)), "{refusal:?}");
        assert_eq!(refusal.to_string(), "a note on lane 4");
        assert_eq!(objects(&song), [object(0, 0, "tap")]);
    }

    #[test]
    fn a_read_only_song_refuses_every_edit() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), &[]);
        let refusal = song.toggle(song.cursor(), lane(1), Kind::Tap).unwrap_err();
        assert!(matches!(refusal, Refusal::ReadOnly), "{refusal:?}");
        assert_eq!(objects(&song), [object(0, 0, "tap")]);
    }

    #[test]
    fn a_song_without_charts_has_nothing_to_edit() {
        let empty = json!({
            "format_version": 1,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": { "anchors": [ { "tick": 0, "seconds": 0.0 } ], "tempo": 120.0, "events": [] }
        });
        let mut song = Song::open(&serde_json::to_vec(&empty).unwrap(), EDITABLE).unwrap();
        assert!(matches!(
            song.toggle(song.cursor(), lane(0), Kind::Tap),
            Err(Refusal::NoChart)
        ));
    }

    #[test]
    fn a_note_starts_where_its_head_is_on_any_lane_it_covers() {
        let wide = json!({ "tick": 240, "position": { "lane": { "index": 1, "width": 2 } },
                           "kind": "tap", "shape": "instant" });
        let song = Song::new(document(json!([wide])), EDITABLE);
        assert!(song.starts_at(at(240), lane(1)));
        assert!(song.starts_at(at(240), lane(2)));
        assert!(!song.starts_at(at(240), lane(3)));
        assert!(!song.starts_at(at(0), lane(1)));
    }
}

#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    fn moving_a_tick_succeeds_exactly_when_the_destination_is_in_range() {
        let tick = Tick::new(kani::any());
        let by: i64 = kani::any();
        let destination = i128::from(i64::from(tick)) + i128::from(by);
        match moved(tick, by) {
            Ok(to) => {
                assert!(i128::from(i64::from(to)) == destination);
            }
            Err(Refusal::NotWhole) => {
                assert!(destination < i128::from(i32::MIN) || destination > i128::from(i32::MAX));
            }
            Err(_) => unreachable!(),
        }
    }

    #[kani::proof]
    fn selection_width_covers_both_end_lanes_when_representable() {
        let first: u16 = kani::any();
        let last: u16 = kani::any();
        let width = last
            .checked_sub(first)
            .and_then(|span| LaneCount::new(span.saturating_add(1)));
        if first <= last && (first != 0 || last != u16::MAX) {
            let area = LaneSpan::new(LaneIndex::new(first), width.expect("range fits"));
            let last_lane = LaneSpan::new(LaneIndex::new(last), LaneCount::ONE);
            assert!(area.overlaps(last_lane));
        } else if last < first {
            assert!(width.is_none());
        }
    }

    #[kani::proof]
    fn a_lane_note_fits_exactly_when_its_last_lane_exists() {
        let index: u16 = kani::any();
        let width: std::num::NonZeroU16 = kani::any();
        let lanes: u16 = kani::any();
        let object = note(Tick::new(0), LaneIndex::new(index), Kind::Tap);
        let object = Object {
            position: Position::Lane {
                index: LaneIndex::new(index),
                width: LaneCount::new(width.get()).expect("nonzero width"),
            },
            ..object
        };
        assert!(
            object.position.fits_in(LaneCount::new(lanes))
                == (u32::from(index) + u32::from(width.get()) <= u32::from(lanes))
        );
    }
}
