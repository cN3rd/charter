//! The rules relating values to each other. Each value was already checked on its own during
//! deserialization.

use std::collections::{BTreeSet, HashSet};
use std::mem::discriminant;

use crate::document::{
    Chart, Document, Extensions, Marker, Object, PathPoint, Position, Shape, Timing, TimingKind,
};
use crate::error::{Location, Rule, Stream, Violation};
use crate::ident::{Key, Kind, Namespace};
use crate::unit::{LaneSpan, Tick, TicksPerBeat};

pub(crate) fn validate(doc: &Document) -> Result<(), Violation> {
    let tpb = doc.ticks_per_beat;
    check_timing(&doc.timing, tpb).map_err(at(Location::Timing(Stream::Song)))?;
    check_markers(&doc.markers)?;
    let mut names = HashSet::with_capacity(doc.charts.len());
    for (c, chart) in doc.charts.iter().enumerate() {
        if !names.insert(chart.name.as_str()) {
            return Err(Violation {
                at: Location::Chart(c),
                rule: Rule::DuplicateChartName(chart.name.clone()),
            });
        }
        check_objects(doc, c, chart)?;
        if let Some(timing) = &chart.timing {
            check_timing(timing, tpb).map_err(|rule| Violation {
                at: Location::Timing(Stream::Chart(c)),
                rule,
            })?;
        }
    }
    check_namespaces(doc).map_err(at(Location::Plugins))
}

/// Markers are sorted by tick, and at most one of each kind sits on a tick.
fn check_markers(markers: &[Marker]) -> Result<(), Violation> {
    if !markers.is_sorted_by_key(|marker| marker.tick) {
        return Err(Violation {
            at: Location::Markers,
            rule: Rule::Unsorted,
        });
    }
    let mut seen = HashSet::with_capacity(markers.len());
    for (m, marker) in markers.iter().enumerate() {
        if !seen.insert((marker.tick, &marker.kind)) {
            return Err(Violation {
                at: Location::Marker(m),
                rule: Rule::DuplicateMarker { tick: marker.tick },
            });
        }
    }
    Ok(())
}

/// Checks a chart's objects.
fn check_objects(doc: &Document, c: usize, chart: &Chart) -> Result<(), Violation> {
    if !chart.objects.is_sorted_by_key(|o| o.tick) {
        return Err(Violation {
            at: Location::Objects(c),
            rule: Rule::Unsorted,
        });
    }
    let mut group: Option<(Tick, usize)> = None;
    let mut taken: Vec<LaneSpan> = Vec::new();
    for (j, object) in chart.objects.iter().enumerate() {
        check_object(doc, chart, object).map_err(|rule| Violation {
            at: Location::Object { chart: c, index: j },
            rule,
        })?;
        if group.is_none_or(|(tick, _)| tick != object.tick) {
            check_lane_group(c, group, &mut taken)?;
            taken.clear();
            group = Some((object.tick, j));
        }
        taken.extend(object.position.lanes());
    }
    check_lane_group(c, group, &mut taken)
}

/// Checks one object and its children against the rules that concern them alone.
fn check_object(doc: &Document, chart: &Chart, object: &Object) -> Result<(), Rule> {
    let samples =
        std::iter::once(&object.sample).chain(object.children.iter().map(|child| &child.sample));
    for sample in samples.flatten() {
        if !doc.samples.contains_key(sample) {
            return Err(Rule::UnknownSample(sample.clone()));
        }
    }
    for position in positions(object) {
        check_lanes(chart, position)?;
    }
    check_path(object.tick, &object.position, &object.shape)?;
    let end = object
        .shape
        .end(object.tick)
        .ok_or(Rule::SpanPastLastTick)?;
    for child in &object.children {
        check_path(child.tick, &child.position, &child.shape)?;
        let child_end = child.shape.end(child.tick).ok_or(Rule::SpanPastLastTick)?;
        if child.tick < object.tick || child_end > end {
            return Err(Rule::ChildOutsideParent);
        }
    }
    Ok(())
}

fn check_namespaces(doc: &Document) -> Result<(), Rule> {
    let ruleset = &doc.ruleset.id;
    if doc.plugins.contains_key(ruleset) {
        return Err(Rule::RulesetInPlugins(ruleset.clone()));
    }
    let used = used_namespaces(doc);
    if let Some(undeclared) = used
        .iter()
        .find(|namespace| **namespace != ruleset && !doc.plugins.contains_key(**namespace))
    {
        return Err(Rule::UndeclaredNamespace((*undeclared).clone()));
    }
    if let Some(unused) = doc
        .plugins
        .keys()
        .find(|namespace| !used.contains(namespace))
    {
        return Err(Rule::UnusedPlugin(unused.clone()));
    }
    Ok(())
}

fn at(location: Location) -> impl FnOnce(Rule) -> Violation {
    move |rule| Violation { at: location, rule }
}

fn check_timing(timing: &Timing, ticks_per_beat: TicksPerBeat) -> Result<(), Rule> {
    if timing.anchors.is_empty() {
        return Err(Rule::NoAnchor);
    }
    for pair in timing.anchors.windows(2) {
        let [previous, next] = pair else {
            unreachable!()
        };
        if previous.tick >= next.tick
            || crate::ResolvedTime::from(previous.seconds).seconds()
                >= crate::ResolvedTime::from(next.seconds).seconds()
        {
            return Err(Rule::AnchorsNotIncreasing);
        }
    }
    if !timing.events.is_sorted_by_key(|e| e.tick) {
        return Err(Rule::Unsorted);
    }
    let mut seen = HashSet::with_capacity(timing.events.len());
    let mut warp_end: Option<Tick> = None;
    for event in &timing.events {
        // One event per tick and kind, and per name among `other` events.
        let mut name = None;
        match &event.kind {
            TimingKind::Stop(_) | TimingKind::Delay(_) => {}
            TimingKind::Warp(ticks) => {
                if warp_end.is_some_and(|end| event.tick < end) {
                    return Err(Rule::OverlappingWarps);
                }
                warp_end = Some(
                    event
                        .tick
                        .checked_add(*ticks)
                        .ok_or(Rule::SpanPastLastTick)?,
                );
            }
            TimingKind::TimeSignature(meter) => {
                if !meter.measure_is_whole(ticks_per_beat) {
                    return Err(Rule::FractionalMeasure(*meter));
                }
            }
            TimingKind::Other { name: key, .. } => name = Some(key),
        }
        if !seen.insert((event.tick, discriminant(&event.kind), name)) {
            return Err(Rule::DuplicateEvent { tick: event.tick });
        }
    }
    let mut events = timing.events.iter().peekable();
    let mut warp_end = None;
    for anchor in &timing.anchors {
        while let Some(&event) = events.peek() {
            if event.tick > anchor.tick {
                break;
            }
            if let TimingKind::Warp(ticks) = event.kind {
                warp_end = Some(
                    event
                        .tick
                        .checked_add(ticks)
                        .ok_or(Rule::SpanPastLastTick)?,
                );
            }
            events.next();
        }
        if warp_end.is_some_and(|end| anchor.tick < end) {
            return Err(Rule::AnchorInWarp { tick: anchor.tick });
        }
    }
    for (_, next, rate) in timing.segment_rates() {
        if rate.is_none() {
            return Err(Rule::BadAnchorSegment { tick: next });
        }
    }
    Ok(())
}

/// Fails if two top-level objects in the group starting at `first` share a lane. Sorting keeps
/// hostile inputs at `n log n`.
fn check_lane_group(
    c: usize,
    group: Option<(Tick, usize)>,
    taken: &mut [LaneSpan],
) -> Result<(), Violation> {
    let Some((tick, first)) = group else {
        return Ok(());
    };
    taken.sort_unstable();
    if taken
        .windows(2)
        .any(|pair| matches!(pair, [a, b] if a.overlaps(*b)))
    {
        return Err(Violation {
            at: Location::Object {
                chart: c,
                index: first,
            },
            rule: Rule::LaneTaken { tick },
        });
    }
    Ok(())
}

/// Every position an object holds: its own, its path's, and its children's.
fn positions(object: &Object) -> impl Iterator<Item = &Position> {
    let own = std::iter::once(&object.position)
        .chain(path_points(&object.shape).iter().map(|p| &p.position));
    let children = object.children.iter().flat_map(|child| {
        std::iter::once(&child.position)
            .chain(path_points(&child.shape).iter().map(|p| &p.position))
    });
    own.chain(children)
}

fn path_points(shape: &Shape) -> &[PathPoint] {
    match shape {
        Shape::Path { points } => points.as_slice(),
        Shape::Instant | Shape::Sustain { .. } => &[],
    }
}

fn check_lanes(chart: &Chart, position: &Position) -> Result<(), Rule> {
    let Some(covered) = position.lanes() else {
        return Ok(());
    };
    let Some(lanes) = chart.lanes else {
        return Err(Rule::MissingLanes);
    };
    if !covered.fits(lanes) {
        return Err(Rule::LaneOutOfRange { lanes });
    }
    Ok(())
}

/// Path point ticks must strictly increase from the item's own tick, and every point stays in
/// the item's kind of position.
fn check_path(tick: Tick, position: &Position, shape: &Shape) -> Result<(), Rule> {
    let mut previous = tick;
    for point in path_points(shape) {
        if point.tick <= previous {
            return Err(Rule::PathNotIncreasing);
        }
        previous = point.tick;
        if discriminant(&point.position) != discriminant(position) {
            return Err(Rule::MixedPath);
        }
    }
    Ok(())
}

/// Every namespace the document uses: in extension keys, kinds, easings, marker kinds and
/// `other` names.
fn used_namespaces(doc: &Document) -> BTreeSet<&Namespace> {
    let mut used = BTreeSet::new();
    add_keys(&mut used, &doc.metadata.ext);
    used.extend(
        doc.markers
            .iter()
            .filter_map(|marker| marker.kind.namespace()),
    );
    let chart_timings = doc.charts.iter().filter_map(|chart| chart.timing.as_ref());
    for timing in std::iter::once(&doc.timing).chain(chart_timings) {
        for anchor in &timing.anchors {
            add_keys(&mut used, &anchor.ext);
        }
        for event in &timing.events {
            add_keys(&mut used, &event.ext);
            if let TimingKind::Other { name, .. } = &event.kind {
                used.insert(name.namespace());
            }
        }
    }
    for chart in &doc.charts {
        add_keys(&mut used, &chart.ext);
        for object in &chart.objects {
            add_item(&mut used, &object.kind, &object.shape, &object.ext);
            for child in &object.children {
                add_item(&mut used, &child.kind, &child.shape, &child.ext);
            }
        }
    }
    used
}

fn add_keys<'a>(used: &mut BTreeSet<&'a Namespace>, ext: &'a Extensions) {
    used.extend(ext.keys().map(Key::namespace));
}

fn add_item<'a>(
    used: &mut BTreeSet<&'a Namespace>,
    kind: &'a Kind,
    shape: &'a Shape,
    ext: &'a Extensions,
) {
    used.extend(kind.namespace());
    add_keys(used, ext);
    for point in path_points(shape) {
        used.extend(point.ease.namespace());
        add_keys(used, &point.ext);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::document::{Anchor, SampleId, TimingEvent};
    use crate::unit::{LaneCount, Origin, TimeSignature};

    fn minimal() -> Value {
        json!({
            "format_version": 1,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": {
                "anchors": [ { "tick": 0, "seconds": 0.0 } ],
                "tempo": 120.0,
                "events": [ { "tick": 0, "kind": { "time_signature": [4, 4] } } ]
            }
        })
    }

    fn with(edit: impl FnOnce(&mut Value)) -> Value {
        let mut value = minimal();
        edit(&mut value);
        value
    }

    fn push(value: &mut Value, pointer: &str, new: Value) {
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(new);
    }

    fn check(value: &Value) -> Result<(), Violation> {
        validate(&serde_json::from_value(value.clone()).unwrap())
    }

    fn plane(x: f64, y: f64) -> Value {
        json!({ "plane": { "x": x, "y": y } })
    }

    fn lane(tick: i32, index: u16) -> Value {
        json!({ "tick": tick, "position": { "lane": { "index": index } }, "kind": "tap", "shape": "instant" })
    }

    /// A minimal document with one four-lane chart holding `objects`.
    fn chart_of(objects: Value) -> Value {
        let mut chart = json!({ "name": "Easy", "lanes": 4 });
        chart["objects"] = objects;
        with(|v| v["charts"] = json!([chart]))
    }

    fn meter_of(numerator: u16, denominator: u16) -> TimeSignature {
        TimeSignature {
            numerator: std::num::NonZeroU16::new(numerator).unwrap(),
            denominator: std::num::NonZeroU16::new(denominator).unwrap(),
        }
    }

    fn broken_rule(value: &Value) -> Rule {
        check(value).unwrap_err().rule
    }

    #[test]
    fn objects_must_be_sorted_by_tick() {
        assert_eq!(
            broken_rule(&chart_of(json!([lane(10, 0), lane(0, 1)]))),
            Rule::Unsorted
        );
    }

    #[test]
    fn timing_events_must_be_sorted_by_tick() {
        let value = with(|v| {
            push(
                v,
                "/timing/events",
                json!({ "tick": -1, "kind": { "stop": 1.0 } }),
            );
        });
        assert_eq!(broken_rule(&value), Rule::Unsorted);
    }

    #[test]
    fn a_tick_holds_one_timing_event_per_kind_and_other_name() {
        let other =
            |name: &str| json!({ "tick": 0, "kind": { "other": { "name": name, "value": 1 } } });
        let two_names = with(|v| {
            push(v, "/timing/events", other("test.a"));
            push(v, "/timing/events", other("test.b"));
        });
        check(&two_names).unwrap();
        for duplicate in [
            json!({ "tick": 0, "kind": { "time_signature": [4, 4] } }),
            other("test.a"),
        ] {
            let value = with(|v| {
                push(v, "/timing/events", other("test.a"));
                push(v, "/timing/events", duplicate);
            });
            assert_eq!(
                broken_rule(&value),
                Rule::DuplicateEvent { tick: Tick::new(0) }
            );
        }
    }

    #[test]
    fn every_timing_needs_an_anchor_including_a_charts_own() {
        let meter_only = json!({ "anchors": [], "tempo": 120.0, "events": [ { "tick": 0, "kind": { "time_signature": [4, 4] } } ] });
        let song = with(|v| v["timing"] = meter_only.clone());
        let chart = with(|v| v["charts"] = json!([{ "name": "Easy", "timing": meter_only }]));
        for value in [song, chart] {
            assert_eq!(broken_rule(&value), Rule::NoAnchor);
        }
    }

    #[test]
    fn anchors_rise_in_ticks_and_seconds_stay_off_warps_and_leave_their_beats_time() {
        let timing = |anchors: Value, events: Value| {
            with(|v| v["timing"] = json!({ "anchors": anchors, "tempo": 120.0, "events": events }))
        };
        let backwards = timing(
            json!([{ "tick": 960, "seconds": 1.0 }, { "tick": 0, "seconds": 2.0 }]),
            json!([]),
        );
        assert_eq!(broken_rule(&backwards), Rule::AnchorsNotIncreasing);
        let earlier = timing(
            json!([{ "tick": 0, "seconds": 2.0 }, { "tick": 960, "seconds": 1.0 }]),
            json!([]),
        );
        assert_eq!(broken_rule(&earlier), Rule::AnchorsNotIncreasing);
        let warped = timing(
            json!([{ "tick": 0, "seconds": 0.0 }, { "tick": 1200, "seconds": 1.0 }]),
            json!([{ "tick": 960, "kind": { "warp": 960 } }]),
        );
        assert_eq!(
            broken_rule(&warped),
            Rule::AnchorInWarp {
                tick: Tick::new(1200)
            }
        );
        let outlasted = timing(
            json!([{ "tick": 0, "seconds": 0.0 }, { "tick": 1920, "seconds": 1.0 }]),
            json!([{ "tick": 960, "kind": { "stop": 1.5 } }]),
        );
        assert_eq!(
            broken_rule(&outlasted),
            Rule::BadAnchorSegment {
                tick: Tick::new(1920)
            }
        );
    }

    #[test]
    fn many_anchors_and_events_validate_without_quadratic_scans() {
        let mut document: Document = serde_json::from_value(minimal()).unwrap();
        let name: Key = "test.pulse".parse().unwrap();
        document.timing.anchors = (0..100_000)
            .map(|tick| Anchor {
                tick: Tick::new(tick),
                seconds: Origin::try_from(crate::number::from_millionths(i64::from(tick) * 1_000))
                    .unwrap(),
                ext: Extensions::new(),
            })
            .collect();
        document.timing.events = (0..100_000)
            .map(|tick| TimingEvent {
                tick: Tick::new(tick),
                kind: TimingKind::Other {
                    name: name.clone(),
                    value: Value::Null,
                },
                ext: Extensions::new(),
            })
            .collect();
        assert_eq!(validate(&document), Ok(()));
    }

    #[test]
    fn a_measure_must_be_a_whole_number_of_ticks() {
        let meter = |v: &mut Value, n: u16, d: u16| {
            push(
                v,
                "/timing/events",
                json!({ "tick": 0, "kind": { "time_signature": [n, d] } }),
            );
        };
        let septuple = with(|v| meter(v, 1, 7));
        assert_eq!(
            broken_rule(&septuple),
            Rule::FractionalMeasure(meter_of(1, 7))
        );
        let coarse = with(|v| {
            v["ticks_per_beat"] = json!(1);
            meter(v, 3, 8);
        });
        assert_eq!(
            broken_rule(&coarse),
            Rule::FractionalMeasure(meter_of(3, 8))
        );
    }

    #[test]
    fn warps_may_touch_but_not_overlap() {
        let warps = |second_start: i32| {
            with(|v| {
                push(
                    v,
                    "/timing/events",
                    json!({ "tick": 1, "kind": { "warp": 480 } }),
                );
                push(
                    v,
                    "/timing/events",
                    json!({ "tick": second_start, "kind": { "warp": 480 } }),
                );
            })
        };
        check(&warps(481)).unwrap();
        assert_eq!(broken_rule(&warps(480)), Rule::OverlappingWarps);
    }

    #[test]
    fn two_objects_on_one_tick_cannot_share_a_lane() {
        let wide = json!({ "tick": 0, "position": { "lane": { "index": 0, "width": 2 } }, "kind": "tap", "shape": "instant" });
        check(&chart_of(json!([wide, lane(0, 2)]))).unwrap();
        let dot =
            json!({ "tick": 0, "position": plane(0.5, 0.5), "kind": "tap", "shape": "instant" });
        check(&chart_of(json!([dot, dot]))).unwrap();
        for crowded in [json!([lane(0, 0), lane(0, 0)]), json!([wide, lane(0, 1)])] {
            assert_eq!(
                broken_rule(&chart_of(crowded)),
                Rule::LaneTaken { tick: Tick::new(0) }
            );
        }
    }

    #[test]
    fn chart_names_are_unique() {
        let value = with(|v| {
            v["charts"] = json!([{ "name": "Easy" }, { "name": "Hard" }, { "name": "Easy" }]);
        });
        assert_eq!(broken_rule(&value), Rule::DuplicateChartName("Easy".into()));
    }

    #[test]
    fn keysounds_must_be_in_the_bank_on_objects_and_children() {
        let mut tap = lane(0, 0);
        tap["sample"] = json!("snare");
        let mut parent = json!({ "tick": 0, "position": { "lane": { "index": 1 } }, "kind": "hold",
                                 "shape": { "sustain": { "duration": 960 } } });
        let mut child = lane(0, 1);
        child["sample"] = json!("snare");
        parent["children"] = json!([child]);
        for objects in [json!([tap]), json!([parent])] {
            let value = chart_of(objects);
            assert_eq!(
                broken_rule(&value),
                Rule::UnknownSample(SampleId::new("snare").unwrap())
            );
        }
    }

    #[test]
    fn violations_name_the_offending_item() {
        let mut tap = lane(0, 0);
        tap["sample"] = json!("snare");
        let err = check(&chart_of(json!([lane(0, 1), tap]))).unwrap_err();
        assert_eq!(
            err.to_string(),
            r#"charts[0].objects[1]: sample "snare" is not in the bank"#
        );
    }

    #[test]
    fn every_lane_a_position_covers_must_exist() {
        let wide = json!({ "tick": 0, "position": { "lane": { "index": 3, "width": 2 } }, "kind": "tap", "shape": "instant" });
        let slide = json!({ "tick": 0, "position": { "lane": { "index": 0 } }, "kind": "tap",
                            "shape": { "path": { "points": [ { "tick": 960, "position": { "lane": { "index": 4 } }, "ease": "linear" } ] } } });
        let mut parent = json!({ "tick": 0, "position": { "lane": { "index": 0 } }, "kind": "hold",
                                 "shape": { "sustain": { "duration": 960 } } });
        parent["children"] = json!([lane(960, 4)]);
        for objects in [
            json!([lane(0, 4)]),
            json!([wide]),
            json!([slide]),
            json!([parent]),
        ] {
            assert_eq!(
                broken_rule(&chart_of(objects)),
                Rule::LaneOutOfRange {
                    lanes: LaneCount::new(4).unwrap()
                }
            );
        }
    }

    #[test]
    fn lane_positions_need_a_chart_with_lanes() {
        let value = with(|v| v["charts"] = json!([{ "name": "Easy", "objects": [lane(0, 0)] }]));
        assert_eq!(broken_rule(&value), Rule::MissingLanes);
    }

    fn plane_path(tick: i32, point_ticks: &[i32]) -> Value {
        let points: Vec<Value> = point_ticks
            .iter()
            .map(|t| json!({ "tick": t, "position": plane(1.0, 1.0), "ease": "linear" }))
            .collect();
        json!({ "tick": tick, "position": plane(0.0, 0.0), "kind": "tap", "shape": { "path": { "points": points } } })
    }

    #[test]
    fn path_points_strictly_increase_from_the_objects_tick() {
        check(&chart_of(json!([plane_path(0, &[480, 960])]))).unwrap();
        let mut parent = plane_path(0, &[960]);
        parent["children"] = json!([plane_path(480, &[480])]);
        for objects in [
            json!([plane_path(0, &[0])]),
            json!([plane_path(0, &[960, 960])]),
            json!([parent]),
        ] {
            assert_eq!(broken_rule(&chart_of(objects)), Rule::PathNotIncreasing);
        }
    }

    #[test]
    fn a_path_stays_in_lanes_or_in_the_plane() {
        let mut lane_to_plane = plane_path(0, &[960]);
        lane_to_plane["position"] = json!({ "lane": { "index": 0 } });
        let mut plane_to_lane = plane_path(0, &[960]);
        plane_to_lane["shape"]["path"]["points"][0]["position"] = json!({ "lane": { "index": 0 } });
        for object in [lane_to_plane, plane_to_lane] {
            assert_eq!(broken_rule(&chart_of(json!([object]))), Rule::MixedPath);
        }
    }

    #[test]
    fn a_child_stays_within_its_parents_span_ends_included() {
        let parent_with = |child: Value| {
            let mut parent = json!({ "tick": 960, "position": plane(0.0, 0.0), "kind": "hold",
                                     "shape": { "sustain": { "duration": 960 } } });
            parent["children"] = json!([child]);
            chart_of(json!([parent]))
        };
        let dot = |tick: i32| json!({ "tick": tick, "position": plane(0.0, 0.0), "kind": "tap", "shape": "instant" });
        check(&parent_with(dot(960))).unwrap();
        check(&parent_with(dot(1920))).unwrap();
        let mut overlong = dot(1440);
        overlong["shape"] = json!({ "sustain": { "duration": 481 } });
        for child in [dot(959), dot(1921), overlong] {
            assert_eq!(broken_rule(&parent_with(child)), Rule::ChildOutsideParent);
        }
    }

    #[test]
    fn every_namespace_used_is_the_ruleset_or_a_declared_plugin() {
        let mut kind = lane(0, 0);
        kind["kind"] = json!("psk.flick");
        let mut ease = plane_path(0, &[960]);
        ease["shape"]["path"]["points"][0]["ease"] = json!("psk.curve");
        let mut child_ext = plane_path(0, &[960]);
        child_ext["children"] = json!([{ "tick": 0, "position": plane(0.0, 0.0), "kind": "tap",
                                        "shape": "instant", "ext": { "psk.x": 1 } }]);
        let mut point_ext = plane_path(0, &[960]);
        point_ext["shape"]["path"]["points"][0]["ext"] = json!({ "psk.x": 1 });
        let places = [
            chart_of(json!([kind])),
            chart_of(json!([ease])),
            chart_of(json!([child_ext])),
            with(|v| v["metadata"]["ext"] = json!({ "psk.x": 1 })),
            with(|v| {
                push(
                    v,
                    "/timing/events",
                    json!({ "tick": 0, "kind": { "other": { "name": "psk.y", "value": 1 } } }),
                );
            }),
            with(|v| v["timing"]["events"][0]["ext"] = json!({ "psk.x": 1 })),
            with(|v| v["charts"] = json!([{ "name": "Easy", "ext": { "psk.x": 1 } }])),
            with(|v| {
                let own = json!({
                    "anchors": [ { "tick": 0, "seconds": 0.0 } ],
                    "tempo": 120.0,
                    "events": [
                    { "tick": 0, "kind": { "other": { "name": "psk.y", "value": 1 } } }
                ] });
                v["charts"] = json!([{ "name": "Easy", "timing": own }]);
            }),
            chart_of(
                json!([{ "tick": 0, "position": plane(0.0, 0.0), "kind": "tap",
                              "shape": "instant", "ext": { "psk.x": 1 } }]),
            ),
            chart_of(json!([point_ext])),
        ];
        for value in places {
            assert_eq!(
                broken_rule(&value),
                Rule::UndeclaredNamespace(Namespace::new("psk").unwrap())
            );
            let mut declared = value;
            declared["plugins"] = json!({ "psk": { "version": "1" } });
            check(&declared).unwrap();
        }
    }

    #[test]
    fn anchor_extensions_require_and_use_their_plugin_namespace() {
        let song = with(|v| v["timing"]["anchors"][0]["ext"] = json!({ "swing.sync": 1 }));
        let chart = with(|v| {
            v["charts"] = json!([{
                "name": "Easy",
                "timing": {
                    "anchors": [{ "tick": 0, "seconds": 0.0, "ext": { "swing.sync": 1 } }],
                    "tempo": 120.0,
                    "events": []
                }
            }]);
        });
        for mut value in [song, chart] {
            assert_eq!(
                broken_rule(&value),
                Rule::UndeclaredNamespace(Namespace::new("swing").unwrap())
            );
            value["plugins"] = json!({ "swing": { "version": "1" } });
            check(&value).unwrap();
        }
    }

    #[test]
    fn a_declared_plugin_must_leave_data() {
        let value = with(|v| v["plugins"] = json!({ "swing": { "version": "0.2.0" } }));
        assert_eq!(
            broken_rule(&value),
            Rule::UnusedPlugin(Namespace::new("swing").unwrap())
        );
    }

    #[test]
    fn the_ruleset_is_not_declared_again_as_a_plugin() {
        let value = with(|v| {
            v["plugins"] = json!({ "test": { "version": "1" } });
            v["metadata"]["ext"] = json!({ "test.x": 1 });
        });
        assert_eq!(
            broken_rule(&value),
            Rule::RulesetInPlugins(Namespace::new("test").unwrap())
        );
    }

    #[test]
    fn every_span_ends_on_a_tick() {
        let last = i32::MAX - 47;
        let hold = |duration: u32| {
            json!({ "tick": last, "position": { "lane": { "index": 0 } },
                                           "kind": "hold", "shape": { "sustain": { "duration": duration } } })
        };
        check(&chart_of(json!([hold(47)]))).unwrap();
        assert_eq!(
            broken_rule(&chart_of(json!([hold(48)]))),
            Rule::SpanPastLastTick
        );
        let warp = with(|v| {
            push(
                v,
                "/timing/events",
                json!({ "tick": last, "kind": { "warp": 48 } }),
            );
        });
        assert_eq!(broken_rule(&warp), Rule::SpanPastLastTick);
    }

    #[test]
    fn markers_are_sorted_one_per_tick_and_kind_and_declare_their_namespace() {
        let marker = |tick: i32, kind: &str| json!({ "tick": tick, "kind": kind, "text": "" });
        let markers = |list: Value| with(|v| v["markers"] = list);
        check(&markers(json!([
            marker(0, "section"),
            marker(0, "bookmark"),
            marker(960, "section")
        ])))
        .unwrap();
        assert_eq!(
            check(&markers(json!([
                marker(960, "section"),
                marker(0, "section")
            ]))),
            Err(Violation {
                at: Location::Markers,
                rule: Rule::Unsorted
            })
        );
        assert_eq!(
            check(&markers(json!([
                marker(0, "section"),
                marker(0, "section")
            ]))),
            Err(Violation {
                at: Location::Marker(1),
                rule: Rule::DuplicateMarker { tick: Tick::new(0) }
            })
        );
        assert_eq!(
            broken_rule(&markers(json!([marker(0, "psk.fever")]))),
            Rule::UndeclaredNamespace(Namespace::new("psk").unwrap())
        );
    }
}
