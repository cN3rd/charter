//! Changes to a song, each holding what it takes to undo it.

use charter_core::{Document, Layer, Marker, Metadata, Object, Settings, Timing};

/// One change: to one object of the chart an edit is made to, or to the song as a whole.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Op {
    /// Adds a note at its sorted position in the open chart.
    Insert {
        /// Position in the chart's object list.
        index: usize,
        /// Note to insert.
        object: Object,
    },
    /// Removes a note from the open chart.
    Remove {
        /// Position in the chart's object list.
        index: usize,
        /// Note to remove, retained for undo.
        object: Object,
    },
    /// Replaces one note without changing its sorted position.
    Replace {
        /// Position in the chart's object list.
        index: usize,
        /// Note before the edit, retained for undo.
        old: Object,
        /// Note after the edit.
        new: Object,
    },
    /// Changes the song's metadata.
    Metadata {
        /// Metadata before the edit.
        old: Box<Metadata>,
        /// Metadata after the edit.
        new: Box<Metadata>,
    },
    /// Changes the song's markers.
    Markers {
        /// Markers before the edit.
        old: Vec<Marker>,
        /// Markers after the edit.
        new: Vec<Marker>,
    },
    /// The song's settings, or chart `chart`'s when there is one.
    Settings {
        /// Chart index when these are chart settings, or `None` for song settings.
        chart: Option<usize>,
        /// Settings before the edit.
        old: Settings,
        /// Settings after the edit.
        new: Settings,
    },
    /// Every object of chart `chart`, whichever chart the edit is made to.
    Objects {
        /// Index of the chart whose objects change.
        chart: usize,
        /// Objects before the edit.
        old: Vec<Object>,
        /// Objects after the edit.
        new: Vec<Object>,
    },
    /// Ordered layers of chart `chart`.
    Layers {
        /// Chart index.
        chart: usize,
        /// Previous layers.
        old: Vec<Layer>,
        /// New layers.
        new: Vec<Layer>,
    },
    /// A change to the timing of the chart the edit is made to when `own`, or else the song's.
    Timing {
        /// Whether the change belongs to the chart instead of the song.
        own: bool,
        /// Timing before the edit.
        old: Box<Timing>,
        /// Timing after the edit.
        new: Box<Timing>,
    },
}

impl Op {
    /// The reverse operation, swapping the saved old and new values.
    fn inverse(&self) -> Self {
        match self {
            Self::Insert { index, object } => Self::Remove {
                index: *index,
                object: object.clone(),
            },
            Self::Remove { index, object } => Self::Insert {
                index: *index,
                object: object.clone(),
            },
            Self::Replace { index, old, new } => Self::Replace {
                index: *index,
                old: new.clone(),
                new: old.clone(),
            },
            Self::Metadata { old, new } => Self::Metadata {
                old: new.clone(),
                new: old.clone(),
            },
            Self::Markers { old, new } => Self::Markers {
                old: new.clone(),
                new: old.clone(),
            },
            Self::Settings { chart, old, new } => Self::Settings {
                chart: *chart,
                old: new.clone(),
                new: old.clone(),
            },
            Self::Objects { chart, old, new } => Self::Objects {
                chart: *chart,
                old: new.clone(),
                new: old.clone(),
            },
            Self::Layers { chart, old, new } => Self::Layers {
                chart: *chart,
                old: new.clone(),
                new: old.clone(),
            },
            Self::Timing { own, old, new } => Self::Timing {
                own: *own,
                old: new.clone(),
                new: old.clone(),
            },
        }
    }
}

/// Changes that make one step of the history, made in order, each against the objects the
/// ones before it left.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Edit(pub(crate) Vec<Op>);

impl Edit {
    /// The edit that takes this one back: each change undone, newest first.
    pub(crate) fn inverse(&self) -> Self {
        Self(self.0.iter().rev().map(Op::inverse).collect())
    }

    /// Makes the edit to `document`, whose chart `chart` its object changes go to. It must be
    /// the document the edit was made against. Says whether the chart was there.
    pub(crate) fn apply(&self, document: &mut Document, chart: usize) -> bool {
        for op in &self.0 {
            match op {
                Op::Metadata { new, .. } => document.metadata = (**new).clone(),
                Op::Markers { new, .. } => document.markers.clone_from(new),
                Op::Settings {
                    chart: None, new, ..
                } => document.settings.clone_from(new),
                Op::Settings {
                    chart: Some(chart_index),
                    new,
                    ..
                } => {
                    let Some(edited) = document.charts.get_mut(*chart_index) else {
                        return false;
                    };
                    edited.settings.clone_from(new);
                }
                Op::Objects {
                    chart: chart_index,
                    new,
                    ..
                } => {
                    let Some(edited) = document.charts.get_mut(*chart_index) else {
                        return false;
                    };
                    edited.objects.clone_from(new);
                }
                Op::Layers {
                    chart: chart_index,
                    new,
                    ..
                } => {
                    let Some(edited) = document.charts.get_mut(*chart_index) else {
                        return false;
                    };
                    edited.layers.clone_from(new);
                }
                Op::Timing {
                    own: false, new, ..
                } => document.timing = (**new).clone(),
                Op::Timing { new, .. } => {
                    let Some(edited) = document.charts.get_mut(chart) else {
                        return false;
                    };
                    edited.timing = Some((**new).clone());
                }
                Op::Insert { .. } | Op::Remove { .. } | Op::Replace { .. } => {
                    let Some(edited) = document.charts.get_mut(chart) else {
                        return false;
                    };
                    let objects = &mut edited.objects;
                    // Each index was found in the objects its change is made to, so it is in
                    // range.
                    match op {
                        Op::Insert { index, object } => objects.insert(*index, object.clone()),
                        Op::Remove { index, .. } => {
                            objects.remove(*index);
                        }
                        Op::Replace { index, new, .. } => {
                            objects.splice(*index..=*index, [new.clone()]);
                        }
                        Op::Metadata { .. }
                        | Op::Markers { .. }
                        | Op::Objects { .. }
                        | Op::Layers { .. }
                        | Op::Settings { .. }
                        | Op::Timing { .. } => {}
                    }
                }
            }
        }
        true
    }

    /// Folds `next` into this edit when all `next` does is change the one object this edit
    /// inserted, so the two undo as one step. Says whether it did.
    pub(crate) fn absorb(&mut self, next: &Self) -> bool {
        if let (
            [Op::Insert { index, object }],
            [
                Op::Replace {
                    index: at,
                    old,
                    new,
                },
            ],
        ) = (self.0.as_mut_slice(), next.0.as_slice())
            && index == at
            && object == old
        {
            *object = new.clone();
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use charter_core::{Extensions, Kind, LaneCount, LaneIndex, Position, Shape, Tick, Ticks};

    use super::*;

    fn object(tick: i32, kind: Kind, shape: Shape) -> Object {
        Object {
            tick: Tick::new(tick),
            position: Position::Lane {
                index: LaneIndex::new(0),
                width: LaneCount::ONE,
            },
            kind,
            shape,
            layer: None,
            sample: None,
            children: Vec::new(),
            ext: Extensions::new(),
        }
    }

    fn tap(tick: i32) -> Object {
        object(tick, Kind::Tap, Shape::Instant)
    }

    fn hold(tick: i32, duration: u32) -> Object {
        let duration = Ticks::new(duration).unwrap();
        object(tick, Kind::Hold, Shape::Sustain { duration })
    }

    #[test]
    fn an_edit_then_its_inverse_leave_the_objects_as_they_were() {
        let before = vec![tap(0), tap(240), tap(480)];
        let edit = Edit(vec![
            Op::Remove {
                index: 2,
                object: tap(480),
            },
            Op::Remove {
                index: 0,
                object: tap(0),
            },
            Op::Replace {
                index: 0,
                old: tap(240),
                new: hold(240, 240),
            },
            Op::Insert {
                index: 1,
                object: tap(720),
            },
        ]);
        let mut document = song(before.clone());
        assert!(edit.apply(&mut document, 0));
        assert_eq!(document.charts[0].objects, [hold(240, 240), tap(720)]);
        assert!(edit.inverse().apply(&mut document, 0));
        assert_eq!(document.charts[0].objects, before);
    }

    /// A song of one chart holding `objects`.
    fn song(objects: Vec<Object>) -> Document {
        let value = serde_json::json!({
            "format_version": 1,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": { "anchors": [ { "tick": 0, "seconds": 0.0 } ], "tempo": 120.0, "events": [] },
            "charts": [ { "name": "Easy", "lanes": 4 } ]
        });
        let mut document: Document = serde_json::from_value(value).unwrap();
        document.charts[0].objects = objects;
        document
    }

    #[test]
    fn a_metadata_change_and_its_inverse_swap_the_metadata() {
        let mut document = song(Vec::new());
        let old = document.metadata.clone();
        let new = Metadata {
            title: "u".to_owned(),
            ..old.clone()
        };
        let edit = Edit(vec![Op::Metadata {
            old: Box::new(old.clone()),
            new: Box::new(new.clone()),
        }]);
        assert!(edit.apply(&mut document, 0));
        assert_eq!(document.metadata, new);
        assert!(edit.inverse().apply(&mut document, 0));
        assert_eq!(document.metadata, old);
    }

    #[test]
    fn a_timing_change_goes_to_the_chart_when_its_own_and_else_to_the_song() {
        let mut document = song(Vec::new());
        let old = document.timing.clone();
        let mut new = old.clone();
        new.events.clear();
        let timing = |own| {
            Edit(vec![Op::Timing {
                own,
                old: Box::new(old.clone()),
                new: Box::new(new.clone()),
            }])
        };
        assert!(timing(true).apply(&mut document, 0));
        assert_eq!(document.charts[0].timing.as_ref(), Some(&new));
        assert_eq!(document.timing, old);
        assert!(timing(false).apply(&mut document, 0));
        assert_eq!(document.timing, new);
        assert!(!timing(true).apply(&mut document, 1));
    }

    #[test]
    fn an_object_change_to_a_chart_that_is_not_there_does_not_apply() {
        let mut document = song(Vec::new());
        let edit = Edit(vec![Op::Insert {
            index: 0,
            object: tap(0),
        }]);
        assert!(!edit.apply(&mut document, 1));
    }

    #[test]
    fn changing_what_an_insert_placed_folds_into_the_insert() {
        let mut placed = Edit(vec![Op::Insert {
            index: 1,
            object: tap(240),
        }]);
        let stretched = Edit(vec![Op::Replace {
            index: 1,
            old: tap(240),
            new: hold(240, 480),
        }]);
        assert!(placed.absorb(&stretched));
        assert_eq!(
            placed,
            Edit(vec![Op::Insert {
                index: 1,
                object: hold(240, 480)
            }])
        );
    }

    #[test]
    fn changing_anything_else_is_not_folded_in() {
        let placed = Edit(vec![Op::Insert {
            index: 1,
            object: tap(240),
        }]);
        let elsewhere = Edit(vec![Op::Replace {
            index: 0,
            old: tap(0),
            new: hold(0, 480),
        }]);
        let mut unchanged = placed.clone();
        assert!(!unchanged.absorb(&elsewhere));
        assert_eq!(unchanged, placed);
    }
}
