//! A made-up song the editor can open without a file, built through the same validation as any
//! other document.

use charter_core::ValidDocument;
use serde_json::{Value, json};

/// Ticks in a sixteenth note and in a 4/4 measure, at the default resolution the demo uses.
const SIXTEENTH: i64 = 240;
const MEASURE: i64 = 16 * SIXTEENTH;
/// Ticks in a 7/8 measure.
const SEVEN_EIGHT_MEASURE: i64 = 14 * SIXTEENTH;

const VERSE: i64 = 8 * MEASURE;
const CHORUS: i64 = 16 * MEASURE;
const BRIDGE: i64 = 24 * MEASURE;
const OUTRO: i64 = BRIDGE + 4 * SEVEN_EIGHT_MEASURE;
const END: i64 = OUTRO + 4 * MEASURE;

/// The demo song: two charts over a tempo change, a stop and a spell of 7/8.
///
/// # Panics
///
/// Never: the demo is fixed data, and a test checks that it validates.
#[must_use]
#[allow(clippy::expect_used)] // see Panics
pub fn song() -> ValidDocument {
    let document = json!({
        "format_version": 0,
        "ruleset": { "id": "sm", "version": "0.1.0" },
        "metadata": { "title": "Demo", "artist": "charter", "audio": "demo.ogg" },
        "samples": { "clap": "clap.wav" },
        "timing": { "offset_seconds": 0.0, "events": [
            { "tick": 0, "kind": { "bpm": 150.0 } },
            { "tick": VERSE, "kind": { "stop": 0.5 } },
            { "tick": CHORUS, "kind": { "bpm": 180.0 } },
            { "tick": BRIDGE, "kind": { "time_signature": [7, 8] } },
            { "tick": OUTRO, "kind": { "time_signature": [4, 4] } },
        ] },
        "markers": [
            { "tick": 0, "kind": "section", "text": "Intro" },
            { "tick": VERSE, "kind": "section", "text": "Verse" },
            { "tick": CHORUS, "kind": "section", "text": "Chorus" },
            { "tick": CHORUS + 4 * MEASURE, "kind": "bookmark", "text": "Check the stream" },
            { "tick": BRIDGE, "kind": "section", "text": "Bridge" },
            { "tick": OUTRO, "kind": "section", "text": "Outro" },
        ],
        "charts": [
            { "name": "Easy", "lanes": 4, "settings": { "difficulty": "Easy", "meter": 3 },
              "objects": easy() },
            { "name": "Hard", "lanes": 4, "settings": { "difficulty": "Hard", "meter": 9 },
              "objects": hard() },
        ],
    });
    ValidDocument::from_json(&serde_json::to_vec(&document).expect("json values serialize"))
        .expect("the demo song is valid")
}

/// Quarter notes walking across the lanes, with a hold at each section.
fn easy() -> Vec<Value> {
    let mut chart = Placer::default();
    for tick in (0..END).filter(|tick| tick % (4 * SIXTEENTH) == 0) {
        let beat = tick / (4 * SIXTEENTH);
        if [0, VERSE, CHORUS, BRIDGE, OUTRO].contains(&tick) {
            chart.place(tick, 3, "hold", 8 * SIXTEENTH, false);
        } else {
            chart.place(tick, walk(beat, &[0, 1, 2, 3, 2, 1]), "tap", 0, false);
        }
    }
    chart.objects
}

/// Eighth notes with holds and rolls, a sixteenth stream through the chorus, and mines through
/// the bridge.
fn hard() -> Vec<Value> {
    let mut chart = Placer::default();
    for tick in (0..END).filter(|tick| tick % SIXTEENTH == 0) {
        let step = tick / SIXTEENTH;
        if (CHORUS..BRIDGE).contains(&tick) {
            let lane = walk(step * 3 + step / 4, &[0, 1, 2, 3]);
            chart.place(tick, lane, "tap", 0, step % 16 == 0);
        } else if (BRIDGE..OUTRO).contains(&tick) {
            if step % 2 == 0 {
                chart.place(tick, walk(step / 2, &[0, 3, 1, 2]), "tap", 0, false);
            } else if step % 4 == 3 {
                chart.place(tick, walk(step / 4, &[0, 1, 2, 3]), "mine", 0, false);
            }
        } else if step % 2 == 0 {
            let lane = walk(step / 2, &[0, 2, 1, 3]);
            chart.place(tick, lane, "tap", 0, false);
            if (VERSE..CHORUS).contains(&tick) && step % 16 == 0 {
                chart.place(tick, 3 - lane, "tap", 0, false);
            }
            if step % 32 == 8 {
                chart.place(tick, 3, "hold", 2 * SIXTEENTH, false);
            }
            if step % 64 == 40 {
                chart.place(tick, 1, "roll", 3 * SIXTEENTH, false);
            }
        }
    }
    chart.objects
}

/// The lane `step` lands on when walking `lanes` over and over.
fn walk(step: i64, lanes: &[usize]) -> usize {
    let at = usize::try_from(step).unwrap_or(0) % lanes.len().max(1);
    lanes.get(at).copied().unwrap_or(0)
}

/// Notes in tick order, skipping any lane that a sustain still covers so no two share a lane.
#[derive(Default)]
struct Placer {
    objects: Vec<Value>,
    free_from: [i64; 4],
}

impl Placer {
    /// Places a note, sustained for `length` ticks when that is not 0, unless its lane is busy.
    fn place(&mut self, tick: i64, lane: usize, kind: &str, length: i64, clap: bool) {
        let Some(free_from) = self.free_from.get_mut(lane) else {
            return;
        };
        if tick < *free_from {
            return;
        }
        let shape = if length > 0 {
            json!({ "sustain": { "duration": length } })
        } else {
            json!("instant")
        };
        let mut object = serde_json::Map::new();
        object.insert("tick".into(), json!(tick));
        object.insert("position".into(), json!({ "lane": { "index": lane } }));
        object.insert("kind".into(), json!(kind));
        object.insert("shape".into(), shape);
        if clap {
            object.insert("sample".into(), json!("clap"));
        }
        self.objects.push(Value::Object(object));
        *free_from = tick + length + 1;
    }
}

#[cfg(test)]
mod tests {
    use charter_core::{Kind, Shape, TimingKind};

    use super::*;

    #[test]
    fn the_demo_song_has_an_easy_and_a_hard_chart_over_varied_timing() {
        let song = song();
        let names: Vec<_> = song.charts().map(|chart| chart.name.clone()).collect();
        assert_eq!(names, ["Easy", "Hard"]);
        let timing = &song.document().timing.events;
        for kind in ["stop", "bpm change", "time signature"] {
            let found = match kind {
                "stop" => timing.iter().any(|e| matches!(e.kind, TimingKind::Stop(_))),
                "bpm change" => {
                    timing
                        .iter()
                        .filter(|e| matches!(e.kind, TimingKind::Bpm(_)))
                        .count()
                        > 1
                }
                _ => timing
                    .iter()
                    .any(|e| matches!(e.kind, TimingKind::TimeSignature(_))),
            };
            assert!(found, "the demo timing has a {kind}");
        }
        let hard: Vec<_> = song.charts().nth(1).unwrap().objects().collect();
        assert!(hard.len() > 200, "{} objects", hard.len());
        for kind in [Kind::Tap, Kind::Hold, Kind::Roll, Kind::Mine] {
            assert!(
                hard.iter().any(|object| object.kind == kind),
                "the hard chart has a {kind}"
            );
        }
        assert!(
            hard.iter()
                .any(|object| matches!(object.shape, Shape::Sustain { .. }))
        );
    }
}
