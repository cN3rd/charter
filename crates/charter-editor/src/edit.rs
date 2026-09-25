//! Changes to a chart's objects, each holding what it takes to undo it.

use charter_core::Object;

/// One change to one object of a chart.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Op {
    Insert {
        index: usize,
        object: Object,
    },
    Remove {
        index: usize,
        object: Object,
    },
    Replace {
        index: usize,
        old: Object,
        new: Object,
    },
}

impl Op {
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

    /// Makes the edit to `objects`, which must be the objects it was made against.
    pub(crate) fn apply(&self, objects: &mut Vec<Object>) {
        // Each index was found in the objects its change is made to, so it is in range.
        for op in &self.0 {
            match op {
                Op::Insert { index, object } => objects.insert(*index, object.clone()),
                Op::Remove { index, .. } => {
                    objects.remove(*index);
                }
                Op::Replace { index, new, .. } => {
                    objects.splice(*index..=*index, [new.clone()]);
                }
            }
        }
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
        let mut objects = before.clone();
        edit.apply(&mut objects);
        assert_eq!(objects, [hold(240, 240), tap(720)]);
        edit.inverse().apply(&mut objects);
        assert_eq!(objects, before);
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
