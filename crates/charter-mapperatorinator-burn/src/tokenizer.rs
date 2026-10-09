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
    /// The event's type.
    pub(crate) kind: String,
    /// The event's value, within its type's range.
    pub(crate) value: i32,
}

impl Event {
    /// An event of type `kind` with `value`.
    pub(crate) fn new(kind: &str, value: i32) -> Self {
        Self {
            kind: kind.to_owned(),
            value,
        }
    }
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "a song's event times in milliseconds fit the tokenizer's i32 event values"
)]
/// A time in milliseconds as an event's value.
pub(crate) const fn event_time(ms: i64) -> i32 {
    ms as i32
}

/// The type of time shift events, which say when the events after them happen.
pub(crate) const TIME_SHIFT: &str = "t";
/// The type of beat events.
pub(crate) const BEAT: &str = "beat";
/// The type of measure events, the beats that start a bar.
pub(crate) const MEASURE: &str = "measure";
/// The type of timing point events, the beats that start a tempo or meter.
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

/// An event type's values in `tokenizer.json`.
#[derive(Deserialize)]
struct RangeJson {
    /// The event type.
    #[serde(rename = "type")]
    kind: String,
    /// The type's lowest value.
    min_value: i32,
    /// The type's highest value.
    max_value: i32,
}

/// The parts of `tokenizer.json` timing reads.
#[derive(Deserialize)]
struct TokenizerJson {
    /// The token of the first event range's lowest value; control tokens come before it.
    offset: u32,
    /// The token that starts each context.
    context_sos: HashMap<String, u32>,
    /// The token that ends each context.
    context_eos: HashMap<String, u32>,
    /// The event types the model writes, in token order.
    event_ranges: Vec<RangeJson>,
    /// The event types only prompts carry, in token order after `event_ranges`.
    input_event_ranges: Vec<RangeJson>,
    /// Difficulty classes; the value past the last means unknown.
    num_diff_classes: i32,
    /// Mapper classes; the value past the last means unknown.
    num_mapper_classes: i32,
    /// Descriptor classes; the value past the last means unknown.
    num_descriptor_classes: i32,
    /// Circle size classes.
    num_cs_classes: i32,
}

/// An event type's values and their tokens, which count up from `start`.
#[derive(Clone, Debug)]
struct Range {
    /// The event type.
    kind: String,
    /// The type's lowest value.
    min: i32,
    /// The type's highest value.
    max: i32,
    /// The token of the lowest value.
    start: u32,
}

impl Range {
    /// How far the highest value is above the lowest, and so the last token above the first.
    const fn span(&self) -> u32 {
        self.max.abs_diff(self.min)
    }
}

/// A checkpoint's vocabulary: its event types' tokens, its context tokens, and the class counts
/// its unknown conditioning tokens come after.
#[derive(Clone, Debug)]
pub(crate) struct Tokenizer {
    /// Every event type's range, in token order.
    ranges: Vec<Range>,
    /// Each event type's index in `ranges`.
    by_kind: HashMap<String, usize>,
    /// The token that starts each context.
    context_sos: HashMap<String, u32>,
    /// The token that ends each context.
    context_eos: HashMap<String, u32>,
    /// Difficulty classes; the value past the last means unknown.
    num_diff_classes: i32,
    /// Mapper classes; the value past the last means unknown.
    num_mapper_classes: i32,
    /// Descriptor classes; the value past the last means unknown.
    num_descriptor_classes: i32,
    /// Circle size classes.
    num_cs_classes: i32,
}

/// The padding token, fed to rows that have finished decoding.
pub(crate) const PAD: u32 = 0;
/// The token that starts a sequence.
pub(crate) const SOS: u32 = 1;
/// The token that ends a sequence.
pub(crate) const EOS: u32 = 2;

impl Tokenizer {
    /// The tokenizer described by `text`, a checkpoint's `tokenizer.json`.
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
            start = start
                .saturating_add(r.max_value.abs_diff(r.min_value))
                .saturating_add(1);
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

    /// The range of event type `kind`.
    fn kind_range(&self, kind: &str) -> Result<&Range> {
        self.by_kind
            .get(kind)
            .and_then(|&index| self.ranges.get(index))
            .ok_or_else(|| anyhow!("unknown event type: {kind}"))
    }

    /// The token of the event of type `kind` with `value`.
    pub(crate) fn encode(&self, kind: &str, value: i32) -> Result<u32> {
        let r = self.kind_range(kind)?;
        if !(r.min..=r.max).contains(&value) {
            bail!(
                "event value {value} is not within range [{}, {}] for event type {kind}",
                r.min,
                r.max
            );
        }
        Ok(r.start.saturating_add(value.abs_diff(r.min)))
    }

    /// The event of `token`, or `None` for control tokens outside every range.
    pub(crate) fn decode(&self, token: u32) -> Option<Event> {
        self.ranges
            .iter()
            .find(|r| token >= r.start && token <= r.start.saturating_add(r.span()))
            .map(|r| Event {
                kind: r.kind.clone(),
                value: r.min.saturating_add_unsigned(token.saturating_sub(r.start)),
            })
    }

    /// The first token of `kind`'s range and one past its last.
    pub(crate) fn range(&self, kind: &str) -> Result<(u32, u32)> {
        let r = self.kind_range(kind)?;
        Ok((r.start, r.start.saturating_add(r.span()).saturating_add(1)))
    }

    /// The token that starts `context`.
    pub(crate) fn context_sos(&self, context: &str) -> Result<u32> {
        self.context_sos
            .get(context)
            .copied()
            .ok_or_else(|| anyhow!("no start token for context {context}"))
    }

    /// The token that ends `context`.
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

    /// The token of an unknown difficulty.
    pub(crate) fn diff_unk(&self) -> Result<u32> {
        self.encode("difficulty", self.num_diff_classes)
    }

    /// The token of an unknown mapper.
    pub(crate) fn mapper_unk(&self) -> Result<u32> {
        self.encode("mapper", self.num_mapper_classes)
    }

    /// The token of an unknown year.
    pub(crate) fn year_unk(&self) -> Result<u32> {
        self.encode("year", 2077)
    }

    /// The token of an unknown descriptor.
    pub(crate) fn descriptor_unk(&self) -> Result<u32> {
        self.encode("descriptor", self.num_descriptor_classes)
    }

    /// The token of an unknown hold note ratio.
    pub(crate) fn hold_note_ratio_unk(&self) -> Result<u32> {
        self.encode("hold_note_ratio", 12)
    }

    /// The token of an unknown scroll speed ratio.
    pub(crate) fn scroll_speed_ratio_unk(&self) -> Result<u32> {
        self.encode("scroll_speed_ratio", 12)
    }

    /// The token of circle size `cs`.
    pub(crate) fn encode_cs(&self, cs: f64) -> Result<u32> {
        let last = self.num_cs_classes.saturating_sub(1);
        let value = truncated(cs * f64::from(last) / 10.0).clamp(0, last);
        self.encode("cs", value)
    }

    /// `song_length` in milliseconds, in ten-second steps up to ten minutes.
    pub(crate) fn encode_song_length(&self, song_length: f64) -> Result<u32> {
        let value = truncated((song_length / 10000.0).floor().clamp(0.0, 60.0));
        self.encode("song_length", value)
    }

    /// The token of the base slider velocity `global_sv`.
    pub(crate) fn encode_global_sv(&self, global_sv: f64) -> Result<u32> {
        self.encode(
            "global_sv",
            truncated(round_half_even(global_sv.clamp(0.4, 3.6) * 100.0)),
        )
    }

    /// The token of `song_position` as a share of `song_length`.
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
        resolution.saturating_add(1)
    } else {
        truncated(round_half_even(ratio * f64::from(resolution))).clamp(0, resolution)
    }
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "conditioning values are small, and truncating toward zero is the rounding wanted"
)]
/// `value` truncated toward zero to a token's value.
const fn truncated(value: f64) -> i32 {
    value as i32
}

/// Python's `round`, which rounds halves to even.
#[expect(
    clippy::float_cmp,
    reason = "Python rounds to even only an exact half, which a binary float holds exactly"
)]
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
        t.encode("t", 1638).unwrap_err();
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
