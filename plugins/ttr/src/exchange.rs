//! A song as Tap Tap Revenge's files hold it, the model `.ttr2_track` files and Editor on Fire's
//! MIDI share: each level's notes switched on and off at moments of the audio, and the tempo from
//! moments of it. The game plays a note at its moment, so moments, not beats, carry over, and a
//! note held longer than [`HOLD_SECONDS`] is a hold.
//!
//! Only three-column levels are known: their notes are MIDI keys 60 to 65, a tap in each column
//! and a shake left, right and down between them. How the game's files hold four or five columns,
//! slices and mega taps is not known, so those are left out and said so.

use std::collections::{BTreeMap, HashSet};
use std::num::NonZeroU16;

use charter_core::{
    LaneIndex, Metadata, Position, ResourcePath, Section, SectionError, Stream, Tick, Ticks,
    TicksPerBeat, TimeSignature, Timing, TimingKind, Valid, ValidDocument, sectioned,
};

use crate::{Chart, Difficulty, Lanes, Note, NoteKind, Shake, Song, Tempo, read_tempo};

/// Seconds a note must last to be a hold; a shorter one is a tap.
// ponytail: MidiToTTR's figure; the game keeps its own as `tapAndHoldThresholdDuration`, which no
// shipped file or theme states. Measure it in the game if holds come out as taps.
pub(crate) const HOLD_SECONDS: f64 = 0.3;

/// The most notes a file may hold across its levels, well past any chart, so a hostile file
/// whose lists share their entries cannot make reading it take unbounded time.
pub(crate) const MAX_NOTES: usize = 1 << 20;

/// A song as the game's files hold it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Track {
    /// The song's title.
    pub(crate) title: String,
    /// Who performs the song.
    pub(crate) artist: String,
    /// The levels, easiest first.
    pub(crate) levels: Vec<Level>,
}

/// One level of a song.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Level {
    /// Which level it is.
    pub(crate) difficulty: Difficulty,
    /// The tempo from each moment it changes, earliest first.
    pub(crate) tempo: Vec<Change>,
    /// The notes, earliest first.
    pub(crate) notes: Vec<Played>,
}

/// A tempo from a moment of the audio on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Change {
    /// Seconds into the audio.
    pub(crate) seconds: f64,
    /// Quarter notes a minute.
    pub(crate) bpm: f64,
    /// Quarter-note beats to a bar from here, which only MIDI holds.
    pub(crate) meter: NonZeroU16,
}

/// One note of a level, switched on and off at moments of the audio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Played {
    /// What the player does.
    pub(crate) key: Key,
    /// Seconds into the audio it starts.
    pub(crate) on: f64,
    /// Seconds into the audio it ends.
    pub(crate) off: f64,
}

/// The MIDI keys of a three-column level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Key {
    /// Key 60, the left column.
    Left,
    /// Key 61, a shake left.
    ShakeLeft,
    /// Key 62, the middle column.
    Middle,
    /// Key 63, a shake right.
    ShakeRight,
    /// Key 64, the right column.
    Right,
    /// Key 65, a shake down.
    ShakeDown,
}

impl Key {
    /// Every key, lowest first.
    const ALL: [Self; 6] = [
        Self::Left,
        Self::ShakeLeft,
        Self::Middle,
        Self::ShakeRight,
        Self::Right,
        Self::ShakeDown,
    ];

    /// The key's MIDI note number.
    pub(crate) const fn midi(self) -> u8 {
        match self {
            Self::Left => 60,
            Self::ShakeLeft => 61,
            Self::Middle => 62,
            Self::ShakeRight => 63,
            Self::Right => 64,
            Self::ShakeDown => 65,
        }
    }

    /// The key with MIDI note number `note`, if it is one.
    pub(crate) fn of(note: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.midi() == note)
    }

    /// The column a tap or hold on this key takes, or none for a shake.
    const fn column(self) -> Option<u16> {
        match self {
            Self::Left => Some(0),
            Self::Middle => Some(1),
            Self::Right => Some(2),
            Self::ShakeLeft | Self::ShakeRight | Self::ShakeDown => None,
        }
    }

    /// The note this key plays when held for `held`, or a tap, or a shake.
    const fn kind(self, held: Option<Ticks>) -> NoteKind {
        match (self.column(), held, self) {
            (Some(lane), Some(duration), _) => NoteKind::Hold {
                lane: LaneIndex::new(lane),
                duration,
            },
            (Some(lane), None, _) => NoteKind::Tap {
                lane: LaneIndex::new(lane),
            },
            (None, _, Self::ShakeLeft) => NoteKind::Shake(Shake::Left),
            (None, _, Self::ShakeRight) => NoteKind::Shake(Shake::Right),
            (None, _, _) => NoteKind::Shake(Shake::Down),
        }
    }
}

/// What a conversion left out, counted by what it was, in words for the user.
#[derive(Debug, Default)]
pub(crate) struct Report(BTreeMap<String, usize>);

impl Report {
    /// Counts one more of `what`.
    pub(crate) fn add(&mut self, what: impl Into<String>) {
        let count = self.0.entry(what.into()).or_default();
        *count = count.saturating_add(1);
    }

    /// Each thing left out, with how many there were when more than one.
    pub(crate) fn into_lines(self) -> Vec<String> {
        self.0
            .into_iter()
            .map(|(what, count)| match count {
                1 => what,
                _ => format!("{what} ({count} times)"),
            })
            .collect()
    }
}

/// The song `track` makes, its audio named `audio`, and what it left out.
///
/// # Errors
///
/// Returns why the track makes no song: no level of the four, or a tempo past what the format
/// holds.
pub(crate) fn to_song(
    track: Track,
    audio: &str,
    report: &mut Report,
) -> Result<ValidDocument, String> {
    let mut levels: Vec<Level> = Vec::new();
    for level in track.levels {
        if levels
            .iter()
            .any(|kept| kept.difficulty == level.difficulty)
        {
            report.add(format!("a second {} level", level.difficulty.name()));
        } else {
            levels.push(level);
        }
    }
    levels.sort_by_key(|level| level.difficulty.index());
    let first = levels
        .first()
        .ok_or("the file has no Easy, Medium, Hard or Extreme level")?;
    let ticks_per_beat = TicksPerBeat::default();
    let tempo = tempo_of(&first.tempo, ticks_per_beat, report)?;
    let own_tempo = |level: &Level, said: &mut Report| {
        (level.tempo != first.tempo)
            .then(|| tempo_of(&level.tempo, ticks_per_beat, said))
            .transpose()
    };
    let charts = levels
        .iter()
        .map(|level| {
            Ok(Chart {
                lanes: Lanes::Three,
                name: level.difficulty.name().to_owned(),
                difficulty: Some(level.difficulty),
                theme: None,
                tempo: own_tempo(level, report)?,
                notes: Vec::new(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let metadata = Metadata {
        title: track.title,
        artist: track.artist,
        audio: ResourcePath::new(audio).map_err(|why| why.to_string())?,
        preview: None,
        artwork: None,
        genre: None,
        ext: charter_core::Extensions::new(),
    };
    let unplaced = Song::new(
        metadata.clone(),
        ticks_per_beat,
        tempo.clone(),
        Vec::new(),
        charts.clone(),
    )
    .map_err(|why| why.to_string())?
    .to_document();
    let charts = charts
        .into_iter()
        .zip(unplaced.charts())
        .zip(&levels)
        .map(|((chart, placed), level)| Chart {
            notes: notes_of(&level.notes, placed.timing(), report),
            ..chart
        })
        .collect();
    Ok(
        Song::new(metadata, ticks_per_beat, tempo, Vec::new(), charts)
            .map_err(|why| why.to_string())?
            .to_document(),
    )
}

/// The tempo map `changes` make, or 120 BPM from the start of the audio without any.
fn tempo_of(
    changes: &[Change],
    ticks_per_beat: TicksPerBeat,
    report: &mut Report,
) -> Result<Tempo, String> {
    let mut sections: Vec<Section> = changes
        .iter()
        .filter(|change| {
            let usable = change.seconds.is_finite() && change.bpm.is_finite() && change.bpm > 0.0;
            if !usable {
                report.add("a tempo change that is not a tempo");
            }
            usable
        })
        .map(|change| Section {
            seconds: change.seconds,
            beat_seconds: 60.0 / change.bpm,
            meter: change.meter,
        })
        .collect();
    if sections.is_empty() {
        sections.push(Section {
            seconds: 0.0,
            beat_seconds: 0.5,
            meter: TimeSignature::default().numerator,
        });
    }
    let made = sectioned(&sections, ticks_per_beat).map_err(|why| match why {
        SectionError::Empty => "the file has no tempo".to_owned(),
        SectionError::OutOfRange(what) => what,
    })?;
    for _ in 0..made.merged {
        report.add("a tempo change on the tick of a later one");
    }
    read_tempo(&made.timing, Stream::Song).map_err(|why| why.to_string())
}

/// `played` as notes on `timing`'s ticks, without those on a lane and tick an earlier one took.
fn notes_of(played: &[Played], timing: Valid<'_, Timing>, report: &mut Report) -> Vec<Note> {
    let mut taken: HashSet<(i64, u16)> = HashSet::new();
    let mut notes = Vec::with_capacity(played.len());
    for note in played {
        if !note.on.is_finite() || !note.off.is_finite() {
            report.add("a note at a moment that is not a time");
            continue;
        }
        let tick = nearest(timing, note.on);
        let held = (note.off - note.on > HOLD_SECONDS)
            .then(|| tick.ticks_until(nearest(timing, note.off)))
            .flatten();
        let kind = note.key.kind(held);
        let Position::Lane { index, width } = Lanes::Three.position(kind) else {
            continue;
        };
        let first = u16::from(index);
        let lanes = first..first.saturating_add(u16::from(width));
        if lanes
            .clone()
            .any(|lane| taken.contains(&(i64::from(tick), lane)))
        {
            report.add("a note on a column and tick another note already takes");
            continue;
        }
        taken.extend(lanes.map(|lane| (i64::from(tick), lane)));
        notes.push(Note { tick, kind });
    }
    notes.sort_by_key(|note| i64::from(note.tick));
    notes
}

/// The tick `timing` puts nearest `seconds` into the audio.
fn nearest(timing: Valid<'_, Timing>, seconds: f64) -> Tick {
    let before = timing.latest_tick_by(seconds);
    let after = Ticks::new(1).and_then(|one| before.checked_add(one));
    let off = |tick: Tick| (timing.time_at(tick).seconds() - seconds).abs();
    match after {
        Some(after) if off(after) < off(before) => after,
        _ => before,
    }
}

/// `song`, a document this ruleset accepts, as the game's files hold it, leaving out what they
/// cannot and saying so in `report`.
///
/// # Errors
///
/// Returns why nothing of the song can be written: no chart with three lanes is one of the four
/// levels.
pub(crate) fn from_song(valid: &ValidDocument, report: &mut Report) -> Result<Track, String> {
    let song = Song::try_from(valid).map_err(|why| why.to_string())?;
    let ticks_per_beat = song.ticks_per_beat();
    for _ in song.markers() {
        report.add("a section or bookmark marker");
    }
    let metadata = song.metadata();
    let kept_elsewhere = [
        (
            "the theme",
            song.theme().is_some() || song.charts().iter().any(|chart| chart.theme.is_some()),
        ),
        ("the artwork", metadata.artwork.is_some()),
        ("the genre", metadata.genre.is_some()),
        ("the preview", metadata.preview.is_some()),
    ];
    for (what, set) in kept_elsewhere {
        if set {
            report.add(format!(
                "{what}, which a track's info.plist holds, not this file"
            ));
        }
    }
    let mut levels: Vec<Level> = Vec::new();
    for (chart, placed) in song.charts().iter().zip(valid.charts()) {
        let Some(difficulty) = chart.difficulty else {
            report.add(format!(
                "the chart {:?}, which is none of the four levels",
                chart.name
            ));
            continue;
        };
        if chart.lanes != Lanes::Three {
            report.add(format!(
                "the chart {:?}, which has {} lanes where the game's files hold 3",
                chart.name,
                chart.lanes.width()
            ));
            continue;
        }
        if levels.iter().any(|level| level.difficulty == difficulty) {
            report.add(format!(
                "the chart {:?}, a second {}",
                chart.name,
                difficulty.name()
            ));
            continue;
        }
        let timing = placed.timing();
        levels.push(Level {
            difficulty,
            tempo: changes_of(timing, report),
            notes: played_of(&chart.notes, timing, ticks_per_beat, report),
        });
    }
    if levels.is_empty() {
        return Err("no chart with three lanes is Easy, Medium, Hard or Extreme".to_owned());
    }
    levels.sort_by_key(|level| level.difficulty.index());
    Ok(Track {
        title: song.metadata().title.clone(),
        artist: song.metadata().artist.clone(),
        levels,
    })
}

/// `timing`'s tempo, with a change wherever the tempo or the meter changes.
fn changes_of(timing: Valid<'_, Timing>, report: &mut Report) -> Vec<Change> {
    let mut meters: BTreeMap<i64, NonZeroU16> = BTreeMap::new();
    for event in &timing.events {
        if let TimingKind::TimeSignature(meter) = event.kind {
            if meter.denominator.get() == 4 {
                meters.insert(i64::from(event.tick), meter.numerator);
            } else {
                report.add("a time signature over other than a quarter note");
            }
        }
    }
    let mut ticks: Vec<(Tick, Option<f64>)> = timing
        .tempos()
        .into_iter()
        .map(|(tick, bpm)| (tick, Some(bpm)))
        .collect();
    ticks.extend(
        timing
            .events
            .iter()
            .filter(|event| matches!(event.kind, TimingKind::TimeSignature(_)))
            .map(|event| (event.tick, None)),
    );
    ticks.sort_by_key(|&(tick, _)| i64::from(tick));
    // A tempo and a meter on one tick make one change, the tempo's anchor sorted first.
    ticks.dedup_by_key(|&mut (tick, _)| i64::from(tick));
    let mut meter = TimeSignature::default().numerator;
    ticks
        .into_iter()
        .map(|(tick, bpm)| {
            let at = i64::from(tick);
            meter = meters.range(..=at).next_back().map_or(meter, |(_, &m)| m);
            Change {
                seconds: timing.time_at(tick).seconds(),
                bpm: bpm.unwrap_or_else(|| timing.tempo_at(tick)),
                meter,
            }
        })
        .collect()
}

/// `notes` at their moments on `timing`.
fn played_of(
    notes: &[Note],
    timing: Valid<'_, Timing>,
    ticks_per_beat: TicksPerBeat,
    report: &mut Report,
) -> Vec<Played> {
    let seconds = |tick: Tick| timing.time_at(tick).seconds();
    // The game's own tracks end a tap a sixteenth after it starts.
    let sixteenth = Ticks::new(u32::from(u16::from(ticks_per_beat)).div_euclid(4));
    notes
        .iter()
        .filter_map(|note| {
            let on = seconds(note.tick);
            let (key, end) = match note.kind {
                NoteKind::Tap { lane } => (column_key(lane)?, None),
                NoteKind::Hold { lane, duration } => (column_key(lane)?, Some(duration)),
                NoteKind::Shake(Shake::Left) => (Key::ShakeLeft, None),
                NoteKind::Shake(Shake::Right) => (Key::ShakeRight, None),
                NoteKind::Shake(Shake::Down) => (Key::ShakeDown, None),
                NoteKind::Slice(_) | NoteKind::MegaTap => {
                    report.add("a slice or mega tap, which the game's files are not known to hold");
                    return None;
                }
            };
            let off = if let Some(duration) = end {
                let off = note.tick.checked_add(duration).map_or(on, seconds);
                if off - on <= HOLD_SECONDS {
                    report.add("a hold of 0.3 seconds or less, which the game plays as a tap");
                }
                off
            } else {
                let sixteenth = sixteenth
                    .and_then(|span| note.tick.checked_add(span))
                    .map_or(on, seconds);
                on + (sixteenth - on).clamp(0.0, HOLD_SECONDS / 2.0)
            };
            Some(Played { key, on, off })
        })
        .collect()
}

/// The key of a tap or hold in column `lane` of three.
fn column_key(lane: LaneIndex) -> Option<Key> {
    Key::ALL
        .into_iter()
        .find(|key| key.column() == Some(u16::from(lane)))
}

/// The quarter notes from the start of the audio to `seconds` on `tempo`, the tempo before the
/// first change reaching back to the start.
pub(crate) fn beats_at(tempo: &[Change], seconds: f64) -> f64 {
    let Some(first) = tempo.first() else {
        return seconds * 2.0;
    };
    let mut beats = first.seconds * first.bpm / 60.0;
    let mut from = first;
    for change in tempo.iter().skip(1) {
        if change.seconds > seconds {
            break;
        }
        beats += (change.seconds - from.seconds) * from.bpm / 60.0;
        from = change;
    }
    beats + (seconds - from.seconds) * from.bpm / 60.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Side;

    /// Four beats to the bar.
    fn four() -> NonZeroU16 {
        NonZeroU16::new(4).unwrap()
    }

    /// A level of `difficulty` at 120 BPM from the start holding `notes`.
    fn level(difficulty: Difficulty, notes: Vec<Played>) -> Level {
        Level {
            difficulty,
            tempo: vec![Change {
                seconds: 0.0,
                bpm: 120.0,
                meter: four(),
            }],
            notes,
        }
    }

    fn played(key: Key, on: f64, off: f64) -> Played {
        Played { key, on, off }
    }

    fn song(levels: Vec<Level>, report: &mut Report) -> Song {
        let track = Track {
            title: "Sing".to_owned(),
            artist: "My Chemical Romance".to_owned(),
            levels,
        };
        Song::try_from(&to_song(track, "audio.m4a", report).unwrap()).unwrap()
    }

    #[test]
    fn each_key_is_its_midi_note_and_back() {
        for (key, note) in Key::ALL.into_iter().zip(60..) {
            assert_eq!(key.midi(), note);
            assert_eq!(Key::of(note), Some(key));
        }
        assert_eq!(Key::of(59), None);
        assert_eq!(Key::of(66), None);
    }

    #[test]
    fn keys_become_taps_holds_and_shakes_in_three_lanes() {
        let mut report = Report::default();
        let notes = vec![
            played(Key::Left, 0.5, 0.6),
            played(Key::Middle, 1.0, 2.0),
            played(Key::Right, 1.5, 1.5),
            played(Key::ShakeLeft, 2.0, 2.1),
            played(Key::ShakeRight, 2.5, 2.6),
            played(Key::ShakeDown, 3.0, 4.0),
        ];
        let song = song(vec![level(Difficulty::Hard, notes)], &mut report);
        assert!(report.into_lines().is_empty());
        let chart = &song.charts()[0];
        assert_eq!(chart.name, "Hard");
        assert_eq!(chart.difficulty, Some(Difficulty::Hard));
        assert_eq!(chart.lanes, Lanes::Three);
        let beat = 960;
        let kinds: Vec<(i64, NoteKind)> = chart
            .notes
            .iter()
            .map(|note| (i64::from(note.tick), note.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                (
                    beat,
                    NoteKind::Tap {
                        lane: LaneIndex::new(0)
                    }
                ),
                (
                    2 * beat,
                    NoteKind::Hold {
                        lane: LaneIndex::new(1),
                        duration: Ticks::new(1920).unwrap(),
                    }
                ),
                (
                    3 * beat,
                    NoteKind::Tap {
                        lane: LaneIndex::new(2)
                    }
                ),
                (4 * beat, NoteKind::Shake(Shake::Left)),
                (5 * beat, NoteKind::Shake(Shake::Right)),
                (6 * beat, NoteKind::Shake(Shake::Down)),
            ]
        );
        assert_eq!(song.metadata().title, "Sing");
        assert_eq!(song.metadata().audio.as_str(), "audio.m4a");
    }

    #[test]
    fn a_note_on_a_taken_column_and_tick_and_a_second_level_are_left_out() {
        let mut report = Report::default();
        let notes = vec![
            played(Key::Left, 1.0, 1.1),
            played(Key::Left, 1.0, 1.1),
            played(Key::ShakeLeft, 1.0, 1.1),
            played(Key::Middle, f64::NAN, 1.1),
        ];
        let levels = vec![
            level(Difficulty::Easy, notes),
            level(Difficulty::Easy, Vec::new()),
        ];
        let song = song(levels, &mut report);
        assert_eq!(song.charts().len(), 1);
        assert_eq!(song.charts()[0].notes.len(), 1);
        assert_eq!(
            report.into_lines(),
            [
                "a note at a moment that is not a time",
                "a note on a column and tick another note already takes (2 times)",
                "a second Easy level",
            ]
        );
    }

    #[test]
    fn a_level_with_its_own_tempo_keeps_it_and_levels_come_easiest_first() {
        let mut report = Report::default();
        let mut hard = level(Difficulty::Hard, vec![played(Key::Left, 1.0, 1.0)]);
        hard.tempo[0].bpm = 150.0;
        let song = song(vec![hard, level(Difficulty::Easy, Vec::new())], &mut report);
        let names: Vec<&str> = song.charts().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Easy", "Hard"]);
        assert!(song.charts()[0].tempo.is_none());
        let own = song.charts()[1].tempo.as_ref().unwrap();
        assert_eq!(own.tempo, charter_core::Bpm::try_from(150.0).unwrap());
    }

    #[test]
    fn a_track_without_a_level_or_a_tempo_is_refused_or_given_120_bpm() {
        let mut report = Report::default();
        let none = to_song(Track::default(), "audio.m4a", &mut report).unwrap_err();
        assert!(none.contains("no Easy, Medium, Hard or Extreme level"));
        let mut untimed = level(Difficulty::Medium, Vec::new());
        untimed.tempo.clear();
        let song = song(vec![untimed], &mut report);
        assert_eq!(
            song.tempo().tempo,
            charter_core::Bpm::try_from(120.0).unwrap()
        );
    }

    #[test]
    fn a_song_exports_as_moments_with_what_the_files_cannot_hold_left_out() {
        let mut report = Report::default();
        let notes = vec![
            played(Key::Left, 0.5, 0.6),
            played(Key::Middle, 1.0, 2.0),
            played(Key::ShakeDown, 3.0, 3.1),
        ];
        let song = song(vec![level(Difficulty::Extreme, notes)], &mut report);
        let mut charts = song.charts().to_vec();
        charts[0].notes.push(Note {
            tick: Tick::new(7 * 960),
            kind: NoteKind::Slice(Side::Left),
        });
        charts.push(Chart {
            lanes: Lanes::Four,
            name: "Wide".to_owned(),
            difficulty: Some(Difficulty::Easy),
            theme: None,
            tempo: None,
            notes: Vec::new(),
        });
        charts.push(Chart {
            name: "Unlevelled".to_owned(),
            difficulty: None,
            ..charts[0].clone()
        });
        let song = Song::new(
            song.metadata().clone(),
            song.ticks_per_beat(),
            song.tempo().clone(),
            Vec::new(),
            charts,
        )
        .unwrap()
        .with_theme(Some(charter_core::Name::new("TTRDJ").unwrap()));
        let track = from_song(&song.to_document(), &mut report).unwrap();
        assert_eq!(track.title, "Sing");
        let [extreme] = track.levels.as_slice() else {
            panic!("one level: {:?}", track.levels);
        };
        assert_eq!(extreme.difficulty, Difficulty::Extreme);
        let keys: Vec<Key> = extreme.notes.iter().map(|note| note.key).collect();
        assert_eq!(keys, [Key::Left, Key::Middle, Key::ShakeDown]);
        let hold = extreme.notes[1];
        assert!((hold.on - 1.0).abs() < 1e-9 && (hold.off - 2.0).abs() < 1e-9);
        let tap = extreme.notes[0];
        assert!(
            (tap.off - tap.on - 0.125).abs() < 1e-9,
            "a sixteenth at 120 BPM"
        );
        assert_eq!(extreme.tempo.len(), 1);
        assert_eq!(
            report.into_lines(),
            [
                "a slice or mega tap, which the game's files are not known to hold",
                "the chart \"Unlevelled\", which is none of the four levels",
                "the chart \"Wide\", which has 4 lanes where the game's files hold 3",
                "the theme, which a track's info.plist holds, not this file",
            ]
        );
    }

    #[test]
    fn beats_count_from_the_start_of_the_audio_across_tempo_changes() {
        let tempo = [
            Change {
                seconds: 1.0,
                bpm: 120.0,
                meter: four(),
            },
            Change {
                seconds: 3.0,
                bpm: 60.0,
                meter: four(),
            },
        ];
        assert!((beats_at(&tempo, 0.0)).abs() < 1e-12);
        assert!((beats_at(&tempo, 3.0) - 6.0).abs() < 1e-12);
        assert!((beats_at(&tempo, 5.0) - 8.0).abs() < 1e-12);
        assert!((beats_at(&[], 1.5) - 3.0).abs() < 1e-12);
    }
}
