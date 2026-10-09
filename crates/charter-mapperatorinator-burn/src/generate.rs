//! Timing events for windows of audio: each window's prompt, greedy decoding of a batch of
//! windows at once, and the events between the timing context's start and end tokens.
//!
//! Ported from OliBomby/Mapperatorinator (MIT), `osuT5/osuT5/inference/processor.py`
//! (`get_class_vector`, `get_prompt`, `generate_parallel`, `_decode`) and `logit_processors.py`
//! (`MonotonicTimeShiftLogitsProcessor`). Super timing decodes greedily, so the temperature and
//! top-k it also passes cannot change a token and are left out. Upstream's batched path decodes
//! each window on through the map context after the timing context ends; decoding is causal, so
//! stopping at the timing context's end token yields the same timing tokens, much sooner.

use anyhow::{Result, bail};
use burn::prelude::*;
use burn::tensor::TensorData;

use crate::mel::{self, BINS, MEL_FRAMES};
use crate::model::Model;
use crate::tokenizer::{EOS, Event, PAD, SOS, TIME_SHIFT, Tokenizer, event_time};

/// Milliseconds a time shift token steps by.
pub(crate) const MS_PER_STEP: i64 = 10;

/// What the prompt tells the model about the beatmap the timing is for. Unknown values go in as
/// the tokenizer's unknown tokens, as upstream's CLI leaves them.
#[derive(Clone, Debug)]
pub(crate) struct Conditions {
    /// The osu! game mode: 0 standard, 1 taiko, 2 catch, 3 mania.
    pub(crate) gamemode: i32,
    /// Whether the beatmap is hitsounded.
    pub(crate) hitsounded: bool,
    /// The beatmap's circle size.
    pub(crate) circle_size: f64,
    /// The beatmap's base slider velocity.
    pub(crate) slider_multiplier: f64,
    /// Keys of a mania beatmap.
    pub(crate) keycount: i32,
}

impl Default for Conditions {
    /// Upstream's `compile_default_args`.
    fn default() -> Self {
        Self {
            gamemode: 0,
            hitsounded: true,
            circle_size: 4.0,
            slider_multiplier: 1.4,
            keycount: 4,
        }
    }
}

/// One window to decode.
pub(crate) struct Window<'a> {
    /// The window's samples.
    pub(crate) samples: &'a [f32],
    /// Where the window starts, in milliseconds.
    pub(crate) frame_time: i64,
    /// The song's length in milliseconds.
    pub(crate) song_length: f64,
}

/// The prompt of a window: conditioning tokens, the last scroll speed and the song position,
/// then the sequence start and the timing context's start.
pub(crate) fn prompt(
    t: &Tokenizer,
    c: &Conditions,
    frame_time: i64,
    song_length: f64,
) -> Result<Vec<u32>> {
    let mut p = vec![
        t.encode("gamemode", c.gamemode)?,
        t.diff_unk()?,
        t.mapper_unk()?,
        t.year_unk()?,
        t.encode("hitsounded", i32::from(c.hitsounded))?,
        t.encode_song_length(song_length)?,
    ];
    if matches!(c.gamemode, 0 | 2) {
        p.push(t.encode_global_sv(c.slider_multiplier)?);
        p.push(t.encode_cs(c.circle_size)?);
    }
    if c.gamemode == 3 {
        p.push(t.encode("keycount", c.keycount)?);
        p.push(t.hold_note_ratio_unk()?);
    }
    if matches!(c.gamemode, 1 | 3) {
        p.push(t.scroll_speed_ratio_unk()?);
    }
    p.push(t.descriptor_unk()?);
    p.push(t.encode("scroll_speed", 100)?);
    p.push(t.encode_song_position(ms_float(frame_time), song_length)?);
    p.push(SOS);
    p.push(t.context_sos("timing")?);
    Ok(p)
}

/// The next token of each row: the largest logit, first on ties, among those the monotonic rule
/// allows, which forbids a time shift before the row's last one since its last start token.
fn pick(logits: &[f32], vocab: usize, last_shift: &[Option<u32>], shifts: (u32, u32)) -> Vec<u32> {
    logits
        .chunks_exact(vocab)
        .zip(last_shift)
        .map(|(row, last)| {
            let forbidden = last.map_or(0..0, |v| shifts.0..shifts.0.saturating_add(v));
            let mut best: Option<(u32, f32)> = None;
            for (i, &x) in (0_u32..).zip(row) {
                if !forbidden.contains(&i) && best.is_none_or(|(_, b)| x > b) {
                    best = Some((i, x));
                }
            }
            best.map_or(0, |(i, _)| i)
        })
        .collect()
}

/// The tokens each window's decoding wrote after its prompt, through the timing context's end
/// token, or `None` when it stopped without one.
#[expect(
    clippy::indexing_slicing,
    reason = "a batch has one prompt per window, and each row is below the batch size"
)]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "a prompt is a few dozen tokens, and a time shift is at or past its range's start"
)]
pub(crate) fn decode(
    model: &Model,
    t: &Tokenizer,
    c: &Conditions,
    windows: &[Window<'_>],
    encoder_batch: usize,
) -> Result<Vec<Option<Vec<u32>>>> {
    if windows.is_empty() {
        return Ok(Vec::new());
    }
    let batch = windows.len();
    let device = model.device();
    let timing_eos = t.context_eos("timing")?;
    let shifts = t.range(TIME_SHIFT)?;

    let mut encoded = Vec::new();
    for chunk in windows.chunks(encoder_batch.max(1)) {
        let samples: Vec<&[f32]> = chunk.iter().map(|w| w.samples).collect();
        let power = mel::power_spectrograms(&samples, &model.hann);
        let power = Tensor::<3>::from_data(
            TensorData::new(power, [chunk.len(), MEL_FRAMES, BINS]),
            device,
        );
        encoded.push(model.encode(power));
    }
    let encoded = Tensor::cat(encoded, 0);

    let prompts = windows
        .iter()
        .map(|w| prompt(t, c, w.frame_time, w.song_length))
        .collect::<Result<Vec<_>>>()?;
    let len = prompts[0].len();
    if prompts.iter().any(|p| p.len() != len) {
        bail!("prompts of one batch differ in length");
    }
    let flat: Vec<u32> = prompts.concat();
    let max_new = model.max_target_positions.saturating_sub(len);

    let (mut logits, mut cache) = model.prefill(encoded, &flat, batch, len + 256);
    let mut out: Vec<Vec<u32>> = vec![Vec::new(); batch];
    let mut done = vec![None::<bool>; batch];
    let mut last_shift: Vec<Option<u32>> = vec![None; batch];
    for _ in 0..max_new {
        let values: Vec<f32> = logits
            .try_into_vec_as::<f32>()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let next = pick(&values, model.vocab_size, &last_shift, shifts);
        let mut feed = Vec::with_capacity(batch);
        for (row, &token) in next.iter().enumerate() {
            if done[row].is_some() {
                feed.push(PAD);
                continue;
            }
            out[row].push(token);
            if (shifts.0..shifts.1).contains(&token) {
                last_shift[row] = Some(token - shifts.0);
            } else if t.is_sos(token) {
                last_shift[row] = None;
            }
            if token == timing_eos {
                done[row] = Some(true);
            } else if token == EOS {
                done[row] = Some(false);
            }
            feed.push(token);
        }
        if done.iter().all(Option::is_some) {
            break;
        }
        logits = model.step(&mut cache, &feed);
    }
    Ok(out
        .into_iter()
        .zip(done)
        .map(|(tokens, d)| (d == Some(true)).then_some(tokens))
        .collect())
}

/// The events of a window's timing tokens, time shifts made absolute: a step's time plus half a
/// step, as upstream de-biases the truncation of times to steps.
pub(crate) fn events(t: &Tokenizer, tokens: &[u32], frame_time: i64) -> Vec<Event> {
    let mut events = Vec::new();
    for &token in tokens {
        if token == EOS {
            break;
        }
        let Some(mut event) = t.decode(token) else {
            continue;
        };
        if event.kind == TIME_SHIFT {
            let half_step = if event.value >= 0 {
                MS_PER_STEP.div_euclid(2)
            } else {
                0
            };
            let time = frame_time
                .saturating_add(i64::from(event.value).saturating_mul(MS_PER_STEP))
                .saturating_add(half_step);
            event.value = event_time(time);
        }
        events.push(event);
    }
    events
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "window start times in milliseconds are far below 2^52, where f64 is exact"
)]
/// A time in milliseconds as a float.
const fn ms_float(ms: i64) -> f64 {
    ms as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_shift_before_the_last_is_never_picked() {
        let vocab = 6;
        // Tokens 2..5 are time shifts; the row's last was shift 2, so shifts 0 and 1 are out.
        let logits = [0.0, 0.0, 9.0, 8.0, 1.0, 0.0];
        assert_eq!(pick(&logits, vocab, &[Some(2)], (2, 6)), vec![4]);
        assert_eq!(pick(&logits, vocab, &[None], (2, 6)), vec![2]);
    }

    #[test]
    fn ties_go_to_the_first_token() {
        assert_eq!(pick(&[1.0, 3.0, 3.0], 3, &[None], (0, 0)), vec![1]);
    }

    #[test]
    fn times_are_absolute_and_centred_on_their_step() {
        let t = Tokenizer::from_json(
            r#"{"offset": 9, "context_sos": {"timing": 3}, "context_eos": {"timing": 4},
            "event_ranges": [{"type": "t", "min_value": 0, "max_value": 1637}, {"type": "beat", "min_value": 0, "max_value": 0}],
            "input_event_ranges": [], "num_diff_classes": 24, "num_mapper_classes": 7605, "num_descriptor_classes": 146, "num_cs_classes": 21}"#,
        )
        .unwrap();
        let tokens = [9 + 12, 1647, 3, 9 + 50, 1647, EOS, 9 + 60];
        assert_eq!(
            events(&t, &tokens, 1000),
            vec![
                Event::new("t", 1125),
                Event::new("beat", 0),
                Event::new("t", 1505),
                Event::new("beat", 0)
            ]
        );
    }
}
