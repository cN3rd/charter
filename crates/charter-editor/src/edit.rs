//! Changes to a chart's objects, each holding what it takes to undo it.

use charter_core::Object;

/// One change to one object of a chart.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Op {
    Insert { index: usize, object: Object },
    Remove { index: usize, object: Object },
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
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use charter_core::{Extensions, Kind, LaneCount, LaneIndex, Position, Shape, Tick};

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
            Op::Insert {
                index: 1,
                object: tap(720),
            },
        ]);
        let mut objects = before.clone();
        edit.apply(&mut objects);
        assert_eq!(objects, [tap(240), tap(720)]);
        edit.inverse().apply(&mut objects);
        assert_eq!(objects, before);
    }
}
