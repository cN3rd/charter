//! Tap Tap Revenge songs as MIDI files, the way Editor on Fire charts them for `MidiToTTR`: a
//! track named `PART GUITAR` holds every level, Easy on keys 60 to 71, Medium twelve keys up,
//! then Hard and Extreme. The keys of a level are laid out as [`Layout`] says. A MIDI file holds
//! no column count, so its levels have three columns. The time of a tick counts from the start of
//! the audio, whatever MIDI delay the project has, since the game plays notes at their moments.
//!
//! [`midly`] reads and writes the file; this module maps its notes and tempo map.

use midly::num::{u7, u15, u24, u28};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};

use crate::exchange::{
    Change, Level, MAX_NOTES, Moment, Report, Switches, Track, TrackKey, beats_at,
};
use crate::{Difficulty, Lanes};

/// How a MIDI file lays out a level's keys, from the level's lowest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Layout {
    /// As the track's own: the left column, left side, middle column, right side, right column
    /// and the middle note, keys 0 to 5.
    Standard,
    /// Tap Tap Reloaded's: the left, middle and right columns on keys 0, 1 and 2, and the middle
    /// note on 4.
    Reloaded,
}

impl Layout {
    /// A track's key, from the level's lowest, for this layout's `key`, if it has one.
    const fn track_key(self, key: u8) -> Option<u8> {
        match (self, key) {
            (Self::Standard, key) => Some(key),
            (Self::Reloaded, 0) => Some(0),
            (Self::Reloaded, 1) => Some(2),
            (Self::Reloaded, 2) => Some(4),
            (Self::Reloaded, 4) => Some(5),
            (Self::Reloaded, _) => None,
        }
    }

    /// This layout's key for a track's `key`, if it has one.
    const fn midi_key(self, key: u8) -> Option<u8> {
        match (self, key) {
            (Self::Standard, key) => Some(key),
            (Self::Reloaded, 0) => Some(0),
            (Self::Reloaded, 2) => Some(1),
            (Self::Reloaded, 4) => Some(2),
            (Self::Reloaded, 5) => Some(4),
            (Self::Reloaded, _) => None,
        }
    }
}

/// The track EOF charts guitar in, which holds every level.
const GUITAR: &[u8] = b"PART GUITAR";

/// The lowest key of the Easy level.
const LOWEST: u8 = 60;

/// Keys from one level's lowest to the next's.
const OCTAVE: u8 = 12;

/// Microseconds a beat takes before the file sets a tempo, 120 BPM.
const DEFAULT_BEAT: u32 = 500_000;

/// Ticks to a beat in a file this writes, as EOF writes them.
const PER_BEAT: u16 = 480;

/// A MIDI key turning off or on, in that order at the same tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Switch {
    /// Release the key.
    Off,
    /// Press the key.
    On,
}

/// What of a MIDI file a track is read from.
#[derive(Clone, Copy, Debug)]
enum Event {
    /// A beat takes this many microseconds from here.
    Tempo(u32),
    /// This many beats to a bar from here, each two to this power in a whole note.
    Meter(u8, u8),
    /// A key switched on or off.
    Note {
        /// The MIDI key.
        key: u8,
        /// Whether it switched off or on.
        switch: Switch,
    },
}

/// The track in `bytes`, its keys laid out as `layout`, and what of it the song leaves out in
/// `report`.
///
/// # Errors
///
/// Returns why `bytes` holds no track: not a MIDI file, one timed in frames, or more notes than
/// any chart.
pub(crate) fn read(bytes: &[u8], layout: Layout, report: &mut Report) -> Result<Track, String> {
    let smf = Smf::parse(bytes).map_err(|why| format!("not a MIDI file: {why}"))?;
    let Timing::Metrical(per_beat) = smf.header.timing else {
        return Err("a MIDI file timed in frames rather than beats".to_owned());
    };
    let per_beat = f64::from(per_beat.as_int().max(1));
    let title = smf
        .tracks
        .first()
        .and_then(|track| name(track))
        .filter(|&title| title != GUITAR)
        .map(|title| String::from_utf8_lossy(title).into_owned())
        .unwrap_or_default();
    let mut tempo: Vec<Change> = Vec::new();
    let mut tempo_tick = None;
    let mut beat = DEFAULT_BEAT;
    let mut meter = charter_core::TimeSignature::default().numerator;
    let (mut seconds, mut at) = (0.0, 0_u64);
    let mut open: Switches<usize> = Switches::default();
    let mut levels: Vec<Level> = Vec::new();
    for (tick, event) in events(&smf, report)? {
        seconds += beats(tick.saturating_sub(at), per_beat) * f64::from(beat) / 1e6;
        at = tick;
        let moment = Moment::from_seconds(seconds).ok_or("a MIDI event has a non-finite time")?;
        match event {
            Event::Tempo(micros) => beat = micros,
            Event::Meter(beats, unit) => {
                if unit != 2 {
                    report.add("a time signature over other than a quarter note");
                }
                meter = std::num::NonZeroU16::new(u16::from(beats)).unwrap_or(meter);
            }
            Event::Note { key, switch } => {
                let Some((difficulty, key)) = level_key(key, layout, switch, report) else {
                    continue;
                };
                let place = difficulty.index();
                match switch {
                    Switch::On => open.on(place, key, moment),
                    Switch::Off => {
                        if let Some(note) = open.off(place, key, moment) {
                            level(&mut levels, difficulty).notes.push(note);
                        }
                    }
                }
                continue;
            }
        }
        let change = Change::new(moment.seconds(), 60e6 / f64::from(beat), meter)
            .ok_or("a MIDI tempo is out of range")?;
        match tempo.last_mut() {
            Some(last) if tempo_tick == Some(tick) => *last = change,
            _ => tempo.push(change),
        }
        tempo_tick = Some(tick);
    }
    for (index, note) in open.unfinished() {
        let Some(&difficulty) = Difficulty::ALL.get(index) else {
            continue;
        };
        level(&mut levels, difficulty).notes.push(note);
    }
    if tempo
        .first()
        .is_none_or(|first| first.seconds.seconds() > 0.0)
    {
        tempo.insert(
            0,
            Change::new(
                0.0,
                60e6 / f64::from(DEFAULT_BEAT),
                charter_core::TimeSignature::default().numerator,
            )
            .ok_or("the default MIDI tempo is out of range")?,
        );
    }
    for level in &mut levels {
        level.tempo.clone_from(&tempo);
        level
            .notes
            .sort_by(|a, b| a.on.seconds().total_cmp(&b.on.seconds()));
    }
    levels.sort_by_key(|level| level.difficulty.index());
    Ok(Track {
        title,
        artist: String::new(),
        genre: None,
        theme: None,
        levels,
    })
}

/// Every event of `smf` that matters, at its tick in tick order: tempo and meter from any track,
/// notes from `PART GUITAR` when there is one, and from every track when not.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "every other kind of MIDI event is nothing to the game"
)]
fn events(smf: &Smf<'_>, report: &mut Report) -> Result<Vec<(u64, Event)>, String> {
    let guitar = smf.tracks.iter().any(|track| name(track) == Some(GUITAR));
    let mut events: Vec<(u64, Event)> = Vec::new();
    for track in &smf.tracks {
        let notes = !guitar || name(track) == Some(GUITAR);
        let mut tick = 0_u64;
        for event in track {
            tick = tick.saturating_add(u64::from(event.delta.as_int()));
            let read = match event.kind {
                TrackEventKind::Meta(MetaMessage::Tempo(micros)) if micros.as_int() > 0 => {
                    Event::Tempo(micros.as_int())
                }
                TrackEventKind::Meta(MetaMessage::TimeSignature(beats, unit, ..)) => {
                    Event::Meter(beats, unit)
                }
                TrackEventKind::Midi { message, .. } => {
                    let (key, switch) = match message {
                        MidiMessage::NoteOn { key, vel } => (
                            key.as_int(),
                            if vel.as_int() > 0 {
                                Switch::On
                            } else {
                                Switch::Off
                            },
                        ),
                        MidiMessage::NoteOff { key, .. } => (key.as_int(), Switch::Off),
                        _ => continue,
                    };
                    if !notes {
                        if switch == Switch::On {
                            let track = name(track).map_or_else(
                                || "without a name".to_owned(),
                                |name| String::from_utf8_lossy(name).into_owned(),
                            );
                            report.add(format!(
                                "the notes of the track {track}, as the song is PART GUITAR's"
                            ));
                        }
                        continue;
                    }
                    if events.len() >= MAX_NOTES {
                        return Err("the file holds more notes than any chart".to_owned());
                    }
                    Event::Note { key, switch }
                }
                _ => continue,
            };
            events.push((tick, read));
        }
    }
    events.sort_by_key(|&(tick, _)| tick);
    Ok(events)
}

/// The name of `track`, if it has one.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "a track's name is one kind of event among many"
)]
fn name<'a>(track: &[TrackEvent<'a>]) -> Option<&'a [u8]> {
    track.iter().find_map(|event| match event.kind {
        TrackEventKind::Meta(MetaMessage::TrackName(name)) => Some(name),
        _ => None,
    })
}

/// `ticks` as beats of `per_beat` ticks.
fn beats(ticks: u64, per_beat: f64) -> f64 {
    // ponytail: a tick count past 2^53 loses precision here; no MIDI file's ticks come near.
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "a MIDI file's ticks stay far below 2^53"
    )]
    let ticks = ticks as f64;
    ticks / per_beat
}

/// The level and track key MIDI key `key` of `layout` plays, or none, saying so in `report` when
/// it switches on.
fn level_key(
    key: u8,
    layout: Layout,
    switch: Switch,
    report: &mut Report,
) -> Option<(Difficulty, TrackKey)> {
    let above = key.checked_sub(LOWEST);
    let difficulty =
        above.and_then(|above| Difficulty::ALL.get(usize::from(above.div_euclid(OCTAVE))));
    let Some(&difficulty) = difficulty else {
        if switch == Switch::On {
            report.add("notes outside keys 60 to 107, which hold no level");
        }
        return None;
    };
    let found = above
        .and_then(|above| layout.track_key(above % OCTAVE))
        .and_then(TrackKey::new);
    if found.is_none() && switch == Switch::On {
        report.add(format!(
            "notes on MIDI key {key}, which the layout gives no note in a {} level",
            difficulty.name()
        ));
    }
    Some(difficulty).zip(found)
}

/// The level of `difficulty` among `levels`, added when it is not there yet.
fn level(levels: &mut Vec<Level>, difficulty: Difficulty) -> &mut Level {
    let index = levels
        .iter()
        .position(|level| level.difficulty == difficulty)
        .unwrap_or_else(|| {
            levels.push(Level {
                difficulty,
                theme: None,
                lanes: Lanes::Three,
                tempo: Vec::new(),
                notes: Vec::new(),
            });
            levels.len().saturating_sub(1)
        });
    #[expect(
        clippy::indexing_slicing,
        reason = "the index was just found or pushed"
    )]
    &mut levels[index]
}

/// `track` as a MIDI file EOF opens: a tempo track named after the song, and `PART GUITAR`
/// holding every level. A MIDI file has one tempo, the first level's; a level whose tempo differs
/// keeps its notes' moments but not its beats, as `report` says.
pub(crate) fn write(track: &Track, layout: Layout, report: &mut Report) -> Vec<u8> {
    let first = track.levels.first();
    let tempo = first.map(|level| level.tempo.clone()).unwrap_or_default();
    for level in track.levels.iter().skip(1) {
        if level.tempo != tempo {
            report.add(format!(
                "the {} level's own tempo, as a MIDI file has one",
                level.difficulty.name()
            ));
        }
    }
    let tempo = led_in(tempo);
    let per_beat = f64::from(PER_BEAT);
    let tick = |seconds: Moment| {
        let tick = (beats_at(&tempo, seconds) * per_beat).round();
        (tick >= 0.0 && tick < f64::from(u28::max_value().as_int())).then(|| {
            #[expect(
                clippy::as_conversions,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "the tick was just checked to be a whole number a MIDI delta holds"
            )]
            let tick = tick as u32;
            tick
        })
    };
    let mut timing: Vec<(u32, TrackEventKind<'static>)> = Vec::new();
    let mut meter = None;
    for change in &tempo {
        let at = tick(change.seconds).unwrap_or(0);
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the microseconds a beat takes are clamped to what a MIDI tempo holds"
        )]
        let micros = (60e6 / change.bpm.bpm())
            .round()
            .clamp(1.0, f64::from(u24::max_value().as_int())) as u32;
        timing.push((
            at,
            TrackEventKind::Meta(MetaMessage::Tempo(u24::new(micros))),
        ));
        if meter != Some(change.meter) {
            meter = Some(change.meter);
            let beats = u8::try_from(change.meter.get()).unwrap_or_else(|_| {
                report.add("a meter of more than 255 beats, which MIDI holds as 255");
                u8::MAX
            });
            let signature = MetaMessage::TimeSignature(beats, 2, 24, 8);
            timing.push((at, TrackEventKind::Meta(signature)));
        }
    }
    let mut switches = switches(track, layout, &tick, report);
    // A note ending where the next on its key starts switches off first.
    switches.sort_by_key(|&(at, switch, _)| (at, switch));
    // Every switch is a note-on, off at velocity zero, as TTRToMidi writes them.
    let notes = switches.into_iter().map(|(at, switch, key)| {
        let vel = u7::new(match switch {
            Switch::Off => 0,
            Switch::On => 100,
        });
        let message = MidiMessage::NoteOn {
            key: u7::new(key),
            vel,
        };
        let channel = midly::num::u4::new(0);
        (at, TrackEventKind::Midi { channel, message })
    });
    let named = |name: &'static [u8]| (0, TrackEventKind::Meta(MetaMessage::TrackName(name)));
    let title = meta_text(&track.title, META_TEXT);
    if title.len() < track.title.len() {
        report.add("the title past its first 256 MiB, which a MIDI track name cannot hold");
    }
    let title = title.as_bytes();
    let tracks = [
        std::iter::once((0, TrackEventKind::Meta(MetaMessage::TrackName(title))))
            .chain(timing)
            .collect(),
        std::iter::once(named(GUITAR)).chain(notes).collect(),
    ]
    .map(delta_track);
    let smf = Smf {
        header: Header::new(Format::Parallel, Timing::Metrical(u15::new(PER_BEAT))),
        tracks: tracks.to_vec(),
    };
    let mut bytes = Vec::new();
    #[expect(
        clippy::expect_used,
        reason = "a MIDI file of valid events, its text cut to what a meta event holds, always \
                  writes into memory"
    )]
    smf.write_std(&mut bytes)
        .expect("a MIDI file writes into memory");
    bytes
}

/// Most bytes of text a meta event holds, the largest length its variable-length count writes.
const META_TEXT: usize = 0x0FFF_FFFF;

/// `text` cut to at most `most` bytes, on a character.
fn meta_text(text: &str, most: usize) -> &str {
    text.get(..text.floor_char_boundary(most))
        .unwrap_or_default()
}

/// Each note of `track`'s three-lane levels switched on and off, as its tick, switch and MIDI
/// key in `layout`, with `tick` placing a moment; what has no place is said in `report`.
fn switches(
    track: &Track,
    layout: Layout,
    tick: &impl Fn(Moment) -> Option<u32>,
    report: &mut Report,
) -> Vec<(u32, Switch, u8)> {
    let mut switches: Vec<(u32, Switch, u8)> = Vec::new();
    for level in &track.levels {
        if level.lanes != Lanes::Three {
            report.add(format!(
                "the {}-lane {} level, as a MIDI file holds no column count",
                level.lanes.width(),
                level.difficulty.name()
            ));
            continue;
        }
        let octave = u8::try_from(level.difficulty.index())
            .unwrap_or(0)
            .saturating_mul(OCTAVE);
        for note in &level.notes {
            let (Some(on), Some(off)) = (tick(note.on), tick(note.off)) else {
                report.add("a note before the audio starts, which a MIDI file cannot hold");
                continue;
            };
            let Some(key) = layout.midi_key(note.key.get()) else {
                report.add("left and right shakes and slices, which Tap Tap Reloaded's layout has no key for");
                continue;
            };
            let key = LOWEST.saturating_add(octave).saturating_add(key);
            switches.push((on, Switch::On, key));
            switches.push((off.max(on.saturating_add(1)), Switch::Off, key));
        }
    }
    switches
}

/// `tempo`, with a tempo from the start of the audio to its first change that puts that change
/// on a whole beat, so EOF's beats line up with the song's.
fn led_in(tempo: Vec<Change>) -> Vec<Change> {
    let Some(&first) = tempo.first() else {
        return tempo;
    };
    if first.seconds.seconds() <= 0.0 {
        return tempo;
    }
    let lead = (first.seconds.seconds() * first.bpm.bpm() / 60.0)
        .round()
        .max(1.0);
    let Some(lead) = Change::new(0.0, lead * 60.0 / first.seconds.seconds(), first.meter) else {
        return tempo;
    };
    std::iter::once(lead).chain(tempo).collect()
}

/// `events` at their ticks, sorted, as a MIDI track of deltas ending the track.
fn delta_track(events: Vec<(u32, TrackEventKind<'_>)>) -> Vec<TrackEvent<'_>> {
    let mut at = 0_u32;
    events
        .into_iter()
        .chain(std::iter::once((
            u32::MAX,
            TrackEventKind::Meta(MetaMessage::EndOfTrack),
        )))
        .map(|(tick, kind)| {
            let tick = if tick == u32::MAX { at } else { tick };
            let delta = tick.saturating_sub(at);
            at = at.max(tick);
            TrackEvent {
                delta: u28::new(delta),
                kind,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use proptest::prelude::*;

    use super::*;
    use crate::exchange::Played;

    fn meter(beats: u16) -> NonZeroU16 {
        NonZeroU16::new(beats).unwrap()
    }

    fn played(key: u8, on: f64, off: f64) -> Played {
        Played::new(key, on, off).unwrap()
    }

    fn change(seconds: f64, bpm: f64) -> Change {
        Change::new(seconds, bpm, meter(4)).unwrap()
    }

    #[test]
    fn text_past_what_a_meta_event_holds_is_cut_on_a_character() {
        assert_eq!(meta_text("a\u{f1}b", 2), "a");
        assert_eq!(meta_text("a\u{f1}b", 3), "a\u{f1}");
        assert_eq!(meta_text("a\u{f1}b", 99), "a\u{f1}b");
    }

    #[test]
    fn a_written_track_reads_back_at_the_same_moments() {
        let tempo = vec![change(0.0, 120.0), change(4.0, 150.0)];
        let track = Track {
            title: "sudo".to_owned(),
            artist: String::new(),
            genre: None,
            theme: None,
            levels: vec![
                Level {
                    difficulty: Difficulty::Medium,
                    theme: None,
                    lanes: Lanes::Three,
                    tempo: tempo.clone(),
                    notes: vec![played(0, 0.5, 0.625), played(5, 4.4, 4.5)],
                },
                Level {
                    difficulty: Difficulty::Extreme,
                    theme: None,
                    lanes: Lanes::Three,
                    tempo,
                    notes: vec![played(4, 1.0, 3.0), played(4, 3.0, 3.1)],
                },
            ],
        };
        let mut report = Report::default();
        let back = read(
            &write(&track, Layout::Standard, &mut report),
            Layout::Standard,
            &mut report,
        )
        .unwrap();
        assert!(report.into_lines().is_empty());
        assert_eq!(back.title, "sudo");
        assert_eq!(back.levels.len(), 2);
        for (level, was) in back.levels.iter().zip(&track.levels) {
            assert_eq!(level.difficulty, was.difficulty);
            assert_eq!(level.tempo, was.tempo);
            assert_eq!(level.notes.len(), was.notes.len());
            for (note, then) in level.notes.iter().zip(&was.notes) {
                assert_eq!(note.key, then.key);
                assert!(
                    (note.on.seconds() - then.on.seconds()).abs() < 1e-9,
                    "{note:?} {then:?}"
                );
                assert!(
                    (note.off.seconds() - then.off.seconds()).abs() < 1e-9,
                    "{note:?} {then:?}"
                );
            }
        }
    }

    #[test]
    fn note_off_precedes_note_on_when_two_notes_share_a_tick() {
        let track = Track {
            title: String::new(),
            artist: String::new(),
            genre: None,
            theme: None,
            levels: vec![Level {
                difficulty: Difficulty::Easy,
                theme: None,
                lanes: Lanes::Three,
                tempo: vec![change(0.0, 120.0)],
                notes: vec![played(0, 0.5, 1.0), played(0, 1.0, 1.5)],
            }],
        };
        let tick = |moment: Moment| match moment.seconds() {
            0.5 => Some(10),
            1.0 => Some(20),
            1.5 => Some(30),
            _ => None,
        };
        let mut events = switches(&track, Layout::Standard, &tick, &mut Report::default());
        events.sort_by_key(|&(at, switch, _)| (at, switch));
        assert_eq!(
            events,
            [
                (10, Switch::On, LOWEST),
                (20, Switch::Off, LOWEST),
                (20, Switch::On, LOWEST),
                (30, Switch::Off, LOWEST),
            ]
        );
    }

    #[test]
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "the test picks the tempos out of every kind of event"
    )]
    fn a_first_tempo_after_the_start_is_led_into_on_a_whole_beat() {
        let track = Track {
            title: String::new(),
            artist: String::new(),
            genre: None,
            theme: None,
            levels: vec![Level {
                difficulty: Difficulty::Easy,
                theme: None,
                lanes: Lanes::Three,
                tempo: vec![change(0.75, 120.0)],
                notes: vec![played(2, 0.75, 0.8)],
            }],
        };
        let bytes = write(&track, Layout::Standard, &mut Report::default());
        let smf = Smf::parse(&bytes).unwrap();
        let tempos: Vec<u32> = smf.tracks[0]
            .iter()
            .filter_map(|event| match event.kind {
                TrackEventKind::Meta(MetaMessage::Tempo(micros)) => Some(micros.as_int()),
                _ => None,
            })
            .collect();
        // 0.75 s is a beat and a half at 120 BPM, so two beats of 0.375 s lead in.
        assert_eq!(tempos, [375_000, 500_000]);
        let first_note = smf.tracks[1]
            .iter()
            .scan(0, |tick, event| {
                *tick += event.delta.as_int();
                Some((*tick, event.kind))
            })
            .find(|(_, kind)| matches!(kind, TrackEventKind::Midi { .. }));
        assert_eq!(
            first_note.map(|(tick, _)| tick),
            Some(2 * u32::from(PER_BEAT))
        );
        let back = read(&bytes, Layout::Standard, &mut Report::default()).unwrap();
        assert!((back.levels[0].notes[0].on.seconds() - 0.75).abs() < 1e-9);
    }

    /// A MIDI event `delta` ticks after the one before.
    fn at(delta: u32, kind: TrackEventKind<'static>) -> TrackEvent<'static> {
        TrackEvent {
            delta: u28::new(delta),
            kind,
        }
    }

    fn note(delta: u32, key: u8, vel: u8) -> TrackEvent<'static> {
        let message = MidiMessage::NoteOn {
            key: u7::new(key),
            vel: u7::new(vel),
        };
        at(
            delta,
            TrackEventKind::Midi {
                channel: 0.into(),
                message,
            },
        )
    }

    fn meta(delta: u32, message: MetaMessage<'static>) -> TrackEvent<'static> {
        at(delta, TrackEventKind::Meta(message))
    }

    /// A MIDI file as EOF writes one for `MidiToTTR`, with a bass track beside the guitar's.
    fn eof() -> Vec<u8> {
        let off = MidiMessage::NoteOff {
            key: u7::new(96),
            vel: u7::new(64),
        };
        let tracks = vec![
            vec![
                meta(0, MetaMessage::TrackName(b"sudo")),
                meta(0, MetaMessage::Tempo(u24::new(500_000))),
                meta(0, MetaMessage::TimeSignature(3, 2, 24, 8)),
                meta(960, MetaMessage::Tempo(u24::new(250_000))),
                meta(0, MetaMessage::EndOfTrack),
            ],
            vec![
                meta(0, MetaMessage::TrackName(b"PART GUITAR")),
                note(480, 96, 100),
                at(
                    240,
                    TrackEventKind::Midi {
                        channel: 0.into(),
                        message: off,
                    },
                ),
                note(0, 101, 100),
                note(0, 116, 100),
                note(0, 103, 100),
                note(240, 101, 0),
                note(480, 62, 100),
                note(960, 62, 0),
                meta(0, MetaMessage::EndOfTrack),
            ],
            vec![
                meta(0, MetaMessage::TrackName(b"PART BASS")),
                note(0, 60, 100),
                note(10, 60, 0),
                meta(0, MetaMessage::EndOfTrack),
            ],
        ];
        let smf = Smf {
            header: Header::new(Format::Parallel, Timing::Metrical(u15::new(480))),
            tracks,
        };
        let mut bytes = Vec::new();
        smf.write_std(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn an_eof_guitar_track_reads_as_levels_by_octave() {
        let mut report = Report::default();
        let track = read(&eof(), Layout::Standard, &mut report).unwrap();
        assert_eq!(track.title, "sudo");
        let levels: Vec<Difficulty> = track.levels.iter().map(|level| level.difficulty).collect();
        assert_eq!(levels, [Difficulty::Easy, Difficulty::Extreme]);
        let extreme = &track.levels[1];
        assert_eq!(
            extreme.notes,
            [
                played(0, 0.5, 0.75),
                played(5, 0.75, 1.0),
                // Key 103, Extreme's seventh, is a level's note for the layout to place.
                played(7, 0.75, 0.75)
            ]
        );
        // Tick 1440 is two beats at 120 BPM, then one at 240: 1.25 s.
        assert_eq!(track.levels[0].notes, [played(2, 1.25, 1.75)]);
        assert_eq!(
            extreme.tempo,
            [
                Change::new(0.0, 120.0, meter(3)).unwrap(),
                Change::new(1.0, 240.0, meter(3)).unwrap(),
            ]
        );
        assert_eq!(
            report.into_lines(),
            [
                "notes outside keys 60 to 107, which hold no level",
                "the notes of the track PART BASS, as the song is PART GUITAR's",
            ]
        );
    }

    #[test]
    fn a_zero_tempo_is_ignored_and_time_runs_on_at_the_last_one() {
        let tracks = vec![vec![
            meta(0, MetaMessage::Tempo(u24::new(500_000))),
            meta(480, MetaMessage::Tempo(u24::new(0))),
            note(480, 60, 100),
            note(480, 60, 0),
            meta(0, MetaMessage::EndOfTrack),
        ]];
        let smf = Smf {
            header: Header::new(Format::Parallel, Timing::Metrical(u15::new(480))),
            tracks,
        };
        let mut bytes = Vec::new();
        smf.write_std(&mut bytes).unwrap();
        let track = read(&bytes, Layout::Standard, &mut Report::default()).unwrap();
        assert_eq!(
            track.levels[0].tempo,
            [Change::new(0.0, 120.0, meter(4)).unwrap()]
        );
        assert_eq!(track.levels[0].notes, [played(0, 1.0, 1.5)]);
    }

    #[test]
    fn what_is_not_a_beat_timed_midi_file_is_refused() {
        let mut report = Report::default();
        assert!(
            read(b"bplist00", Layout::Standard, &mut report)
                .unwrap_err()
                .starts_with("not a MIDI file")
        );
        let smf = Smf {
            header: Header::new(Format::SingleTrack, Timing::Timecode(midly::Fps::Fps25, 40)),
            tracks: vec![vec![meta(0, MetaMessage::EndOfTrack)]],
        };
        let mut bytes = Vec::new();
        smf.write_std(&mut bytes).unwrap();
        assert_eq!(
            read(&bytes, Layout::Standard, &mut report).unwrap_err(),
            "a MIDI file timed in frames rather than beats"
        );
    }

    proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            drop(read(&bytes, Layout::Standard, &mut Report::default()));
        }

        #[test]
        fn a_damaged_eof_file_never_panics(at in 0_usize..256, byte in any::<u8>()) {
            let mut bytes = eof();
            if let Some(slot) = bytes.get_mut(at) {
                *slot = byte;
            }
            drop(read(&bytes, Layout::Standard, &mut Report::default()));
        }
    }
}
