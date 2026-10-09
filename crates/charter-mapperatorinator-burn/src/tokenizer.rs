//! The fixed vocabulary of a Mapperatorinator checkpoint, read from its `tokenizer.json`.
//!
//! Ported from OliBomby/Mapperatorinator (MIT), `osuT5/osuT5/tokenizer.py` and `event.py`: the
//! state-dict loading, `encode`, `decode` and the encoders of the conditioning tokens a timing
//! prompt carries.

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use std::collections::HashMap;

/// An event of the model's vocabulary: a type, named as upstream's `EventType` values are, and
/// a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Event {
    pub(crate) kind: String,
    pub(crate) value: i32,
}

impl Event {
    pub(crate) fn new(kind: &str, value: i32) -> Self {
        Self {
            kind: kind.to_owned(),
            value,
        }
    }
}

pub(crate) const TIME_SHIFT: &str = "t";
pub(crate) const BEAT: &str = "beat";
pub(crate) const MEASURE: &str = "measure";
pub(crate) const TIMING_POINT: &str = "timing_point";

/// Event types that end a group, as `TYPE_EVENTS` in upstream's `data_utils.py`.
pub(crate) const TYPE_EVENTS: [&str; 21] = [
    "circle",
    "spinner",
    "spinner_end",
    "slider_head",
    "bezier_anchor",
    "perfect_anchor",
    "catmull_anchor",
    "red_anchor",
    "last_anchor",
    "slider_end",
    BEAT,
    MEASURE,
    TIMING_POINT,
    "kiai",
    "hold_note",
    "hold_note_end",
    "drumroll",
    "drumroll_end",
    "denden",
    "denden_end",
    "scroll_speed_change",
];

#[derive(Deserialize)]
struct RangeJson {
    #[serde(rename = "type")]
    kind: String,
    min_value: i32,
    max_value: i32,
}

#[derive(Deserialize)]
struct TokenizerJson {
    offset: u32,
    context_sos: HashMap<String, u32>,
    context_eos: HashMap<String, u32>,
    event_ranges: Vec<RangeJson>,
    input_event_ranges: Vec<RangeJson>,
    num_diff_classes: i32,
    num_mapper_classes: i32,
    num_descriptor_classes: i32,
    num_cs_classes: i32,
}

#[derive(Clone, Debug)]
struct Range {
    kind: String,
    min: i32,
    max: i32,
    start: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct Tokenizer {
    ranges: Vec<Range>,
    by_kind: HashMap<String, usize>,
    context_sos: HashMap<String, u32>,
    context_eos: HashMap<String, u32>,
    num_diff_classes: i32,
    num_mapper_classes: i32,
    num_descriptor_classes: i32,
    num_cs_classes: i32,
}

pub(crate) const PAD: u32 = 0;
pub(crate) const SOS: u32 = 1;
pub(crate) const EOS: u32 = 2;

impl Tokenizer {
    pub(crate) fn from_json(text: &str) -> Result<Self> {
        let json: TokenizerJson = serde_json::from_str(text)
            .context("tokenizer.json is not a Mapperatorinator tokenizer")?;
        let mut ranges = Vec::new();
        let mut start = json.offset;
        for r in json
            .event_ranges
            .iter()
            .chain(json.input_event_ranges.iter())
        {
            if r.max_value < r.min_value {
                bail!("event range {} is empty", r.kind);
            }
            ranges.push(Range {
                kind: r.kind.clone(),
                min: r.min_value,
                max: r.max_value,
                start,
            });
            start += (r.max_value - r.min_value + 1) as u32;
        }
        let by_kind = ranges
            .iter()
            .enumerate()
            .map(|(i, r)| (r.kind.clone(), i))
            .collect();
        Ok(Self {
            ranges,
            by_kind,
            context_sos: json.context_sos,
            context_eos: json.context_eos,
            num_diff_classes: json.num_diff_classes,
            num_mapper_classes: json.num_mapper_classes,
            num_descriptor_classes: json.num_descriptor_classes,
            num_cs_classes: json.num_cs_classes,
        })
    }

    pub(crate) fn encode(&self, kind: &str, value: i32) -> Result<u32> {
        let r = &self.ranges[*self
            .by_kind
            .get(kind)
            .ok_or_else(|| anyhow!("unknown event type: {kind}"))?];
        if !(r.min..=r.max).contains(&value) {
            bail!(
                "event value {value} is not within range [{}, {}] for event type {kind}",
                r.min,
                r.max
            );
        }
        Ok(r.start + (value - r.min) as u32)
    }

    /// The event of `token`, or `None` for control tokens outside every range.
    pub(crate) fn decode(&self, token: u32) -> Option<Event> {
        self.ranges
            .iter()
            .find(|r| token >= r.start && token <= r.start + (r.max - r.min) as u32)
            .map(|r| Event {
                kind: r.kind.clone(),
                value: r.min + (token - r.start) as i32,
            })
    }

    /// The first token of `kind`'s range and one past its last.
    pub(crate) fn range(&self, kind: &str) -> Result<(u32, u32)> {
        let r = &self.ranges[*self
            .by_kind
            .get(kind)
            .ok_or_else(|| anyhow!("unknown event type: {kind}"))?];
        Ok((r.start, r.start + (r.max - r.min + 1) as u32))
    }

    pub(crate) fn context_sos(&self, context: &str) -> Result<u32> {
        self.context_sos
            .get(context)
            .copied()
            .ok_or_else(|| anyhow!("no start token for context {context}"))
    }

    pub(crate) fn context_eos(&self, context: &str) -> Result<u32> {
        self.context_eos
            .get(context)
            .copied()
            .ok_or_else(|| anyhow!("no end token for context {context}"))
    }

    /// Whether `token` starts a sequence or a context, which resets the monotonic time shift rule.
    pub(crate) fn is_sos(&self, token: u32) -> bool {
        token == SOS || self.context_sos.values().any(|&t| t == token)
    }

    pub(crate) fn diff_unk(&self) -> Result<u32> {
        self.encode("difficulty", self.num_diff_classes)
    }

    pub(crate) fn mapper_unk(&self) -> Result<u32> {
        self.encode("mapper", self.num_mapper_classes)
    }

    pub(crate) fn year_unk(&self) -> Result<u32> {
        self.encode("year", 2077)
    }

    pub(crate) fn descriptor_unk(&self) -> Result<u32> {
        self.encode("descriptor", self.num_descriptor_classes)
    }

    pub(crate) fn hold_note_ratio_unk(&self) -> Result<u32> {
        self.encode("hold_note_ratio", 12)
    }

    pub(crate) fn scroll_speed_ratio_unk(&self) -> Result<u32> {
        self.encode("scroll_speed_ratio", 12)
    }

    pub(crate) fn encode_cs(&self, cs: f64) -> Result<u32> {
        let value = ((cs * f64::from(self.num_cs_classes - 1) / 10.0) as i32)
            .clamp(0, self.num_cs_classes - 1);
        self.encode("cs", value)
    }

    /// `song_length` in milliseconds, in ten-second steps up to ten minutes.
    pub(crate) fn encode_song_length(&self, song_length: f64) -> Result<u32> {
        let value = (song_length / 10000.0).floor().clamp(0.0, 60.0) as i32;
        self.encode("song_length", value)
    }

    pub(crate) fn encode_global_sv(&self, global_sv: f64) -> Result<u32> {
        self.encode(
            "global_sv",
            round_half_even(global_sv.clamp(0.4, 3.6) * 100.0) as i32,
        )
    }

    pub(crate) fn encode_song_position(&self, song_position: f64, song_length: f64) -> Result<u32> {
        self.encode(
            "song_position",
            ratio_to_value(song_position / song_length, 100),
        )
    }
}

/// Upstream's `Tokenizer.ratio_to_value`.
fn ratio_to_value(ratio: f64, resolution: i32) -> i32 {
    if ratio <= 0.0 {
        -1
    } else if ratio >= 1.0 {
        resolution + 1
    } else {
        (round_half_even(ratio * f64::from(resolution)) as i32).clamp(0, resolution)
    }
}

/// Python's `round`, which rounds halves to even.
pub(crate) fn round_half_even(x: f64) -> f64 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 {
        2.0 * (x / 2.0).round()
    } else {
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenizer() -> Tokenizer {
        Tokenizer::from_json(
            r#"{"offset": 9, "context_sos": {"timing": 3}, "context_eos": {"timing": 4},
            "event_ranges": [{"type": "t", "min_value": 0, "max_value": 1637}, {"type": "beat", "min_value": 0, "max_value": 0}],
            "input_event_ranges": [{"type": "song_position", "min_value": -1, "max_value": 101}],
            "num_diff_classes": 24, "num_mapper_classes": 7605, "num_descriptor_classes": 146, "num_cs_classes": 21}"#,
        )
        .unwrap()
    }

    #[test]
    fn tokens_count_up_from_the_offset_through_the_output_then_input_ranges() {
        let t = tokenizer();
        assert_eq!(t.encode("t", 0).unwrap(), 9);
        assert_eq!(t.encode("beat", 0).unwrap(), 1647);
        assert_eq!(t.encode("song_position", -1).unwrap(), 1648);
        assert_eq!(t.decode(1646), Some(Event::new("t", 1637)));
        assert_eq!(t.decode(2), None);
        assert!(t.encode("t", 1638).is_err());
    }

    #[test]
    fn song_position_is_a_percentage_with_ends_of_its_own() {
        let t = tokenizer();
        assert_eq!(
            t.encode_song_position(0.0, 100.0).unwrap(),
            t.encode("song_position", -1).unwrap()
        );
        assert_eq!(
            t.encode_song_position(25.0, 100.0).unwrap(),
            t.encode("song_position", 25).unwrap()
        );
        assert_eq!(
            t.encode_song_position(100.0, 100.0).unwrap(),
            t.encode("song_position", 101).unwrap()
        );
    }

    #[test]
    fn rounding_matches_python() {
        assert_eq!(round_half_even(2.5), 2.0);
        assert_eq!(round_half_even(3.5), 4.0);
        assert_eq!(round_half_even(-2.5), -2.0);
        assert_eq!(round_half_even(2.4), 2.0);
    }
}
