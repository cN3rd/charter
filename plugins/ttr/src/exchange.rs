//! A song as Tap Tap Revenge's files hold it, the model `.ttr2_track` files and Editor on Fire's
//! MIDI share: each level's notes switched on and off at moments of the audio, and the tempo from
//! moments of it. The game plays a note at its moment, so moments, not beats, carry over, and a
//! note held longer than [`HOLD_SECONDS`] is a hold.
//!
//! A note is a key, counted from the lowest of its level, 60 in a track. A level of four or five
//! columns plays keys 0 up, left to right. A level of three plays keys 0, 2 and 4 as its columns,
//! and 1, 3 and 5 as whatever its theme makes them, which depends on the game the track is for:
//! see [`Preset`].

use std::collections::{BTreeMap, HashSet};
use std::num::NonZeroU16;

use charter_core::{
    LaneIndex, Metadata, Position, ResolvedTime, ResourcePath, Section, SectionError, Stream, Tick,
    Ticks, TicksPerBeat, TimeSignature, Timing, TimingKind, Valid, ValidDocument, sectioned,
};

use crate::{Chart, Difficulty, Lanes, Note, NoteKind, Shake, Side, Song, Tempo, read_tempo};

/// Seconds a note must last to be a hold; a shorter one is a tap.
// ponytail: the themes' `tapAndHoldThresholdDuration` with the game's short-holds option on; off,
// most themes hold from 1 second, and TTRDJ and Tour themes set none.
pub(crate) const HOLD_SECONDS: f64 = 0.15;

/// The most notes a file may hold across its levels, well past any chart, so a hostile file
/// whose lists share their entries cannot make reading it take unbounded time.
pub(crate) const MAX_NOTES: usize = 1 << 20;

/// What keys 1, 3 and 5 of a three-column level are, as the themes of each game make them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Preset {
    /// Tap Tap Revenge's own themes and custom themes like them: shakes left, right and down.
    Revenge,
    /// Tap Tap Revenge: Tour: slices left and right, and the big note across every column.
    Tour,
    /// Custom themes that give the keys columns of their own, such as MCR and Warmen: taps in the
    /// left, right and middle columns that cannot be held.
    TapThemes,
}

/// A song as the game's files hold it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Track {
    /// The song's title.
    pub(crate) title: String,
    /// Who performs the song.
    pub(crate) artist: String,
    /// The song's genre, which a track's `info.plist` holds.
    pub(crate) genre: Option<String>,
    /// The theme every level plays in unless it names its own, which `info.plist` holds.
    pub(crate) theme: Option<String>,
    /// The levels, easiest first.
    pub(crate) levels: Vec<Level>,
}

/// One level of a song.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Level {
    /// Which level it is.
    pub(crate) difficulty: Difficulty,
    /// The theme it plays in, over the song's, which `info.plist` holds.
    pub(crate) theme: Option<String>,
    /// Its columns, which `info.plist` sets beside a track.
    pub(crate) lanes: Lanes,
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
    /// Its key, counted from the level's lowest, below [`KEYS`].
    pub(crate) key: u8,
    /// Seconds into the audio it starts.
    pub(crate) on: f64,
    /// Seconds into the audio it ends.
    pub(crate) off: f64,
}

/// Keys a level has, an octave.
pub(crate) const KEYS: u8 = 12;

/// The columns of a three-column level's keys 0, 2 and 4.
const COLUMN_KEYS: [u8; 3] = [0, 2, 4];

impl Preset {
    /// The note `key` plays in a level of `lanes`, held for `held` when it can be, or none when
    /// the level has no such key.
    fn kind(self, key: u8, lanes: Lanes, held: Option<Ticks>) -> Option<NoteKind> {
        let column = |lane: u16, holds: bool| {
            let lane = LaneIndex::new(lane);
            Some(match held.filter(|_| holds) {
                Some(duration) => NoteKind::Hold { lane, duration },
                None => NoteKind::Tap { lane },
            })
        };
        if lanes != Lanes::Three {
            return (u16::from(key) < u16::from(lanes.width()))
                .then(|| column(key.into(), true))?;
        }
        match (key, self) {
            (0, _) => column(0, true),
            (2, _) => column(1, true),
            (4, _) => column(2, true),
            (1, Self::Revenge) => Some(NoteKind::Shake(Shake::Left)),
            (3, Self::Revenge) => Some(NoteKind::Shake(Shake::Right)),
            (5, Self::Revenge) => Some(NoteKind::Shake(Shake::Down)),
            (1, Self::Tour) => Some(NoteKind::Slice(Side::Left)),
            (3, Self::Tour) => Some(NoteKind::Slice(Side::Right)),
            (5, Self::Tour) => Some(NoteKind::MegaTap),
            (1, Self::TapThemes) => column(0, false),
            (3, Self::TapThemes) => column(2, false),
            (5, Self::TapThemes) => column(1, false),
            _ => None,
        }
    }

    /// The key `kind` is written on in a level of `lanes`, and whether it holds; none, said in
    /// `report`, for a note such a level has no key for. A kind this game plays as another, such
    /// as a slice in Tap Tap Revenge, is written as that one and said so.
    fn key(self, kind: NoteKind, lanes: Lanes, report: &mut Report) -> Option<(u8, bool)> {
        let lane = match kind {
            NoteKind::Tap { lane } | NoteKind::Hold { lane, .. } => Some(u16::from(lane)),
            NoteKind::Shake(_) | NoteKind::Slice(_) | NoteKind::MegaTap => None,
        };
        let holds = matches!(kind, NoteKind::Hold { .. });
        if lanes != Lanes::Three {
            if lane.is_none() {
                report.add(format!(
                    "a shake, slice or mega tap in a {}-lane level, which has keys for its columns alone",
                    lanes.width()
                ));
            }
            return lane
                .and_then(|lane| u8::try_from(lane).ok())
                .map(|key| (key, holds));
        }
        if let Some(lane) = lane {
            return COLUMN_KEYS.get(usize::from(lane)).map(|&key| (key, holds));
        }
        let (key, plays_as) = match (kind, self) {
            (NoteKind::Shake(Shake::Left), Self::Revenge)
            | (NoteKind::Slice(Side::Left), Self::Tour) => (1, None),
            (NoteKind::Shake(Shake::Right), Self::Revenge)
            | (NoteKind::Slice(Side::Right), Self::Tour) => (3, None),
            (NoteKind::Shake(Shake::Down), Self::Revenge) | (NoteKind::MegaTap, Self::Tour) => {
                (5, None)
            }
            (NoteKind::Slice(Side::Left) | NoteKind::Shake(Shake::Left), _) => {
                (1, Some(self.name(1)))
            }
            (NoteKind::Slice(Side::Right) | NoteKind::Shake(Shake::Right), _) => {
                (3, Some(self.name(3)))
            }
            _ => (5, Some(self.name(5))),
        };
        if let Some(plays_as) = plays_as {
            report.add(format!(
                "{}, which {} plays as {plays_as}",
                kind_name(kind),
                self.game()
            ));
        }
        Some((key, false))
    }

    /// What this game plays key 1, 3 or 5 of a three-column level as, in words.
    const fn name(self, key: u8) -> &'static str {
        match (self, key) {
            (Self::Revenge, 1) => "a left shake",
            (Self::Revenge, 3) => "a right shake",
            (Self::Revenge, _) => "a down shake",
            (Self::Tour, 1) => "a left slice",
            (Self::Tour, 3) => "a right slice",
            (Self::Tour, _) => "a mega tap",
            (Self::TapThemes, 1) => "a tap in the left column",
            (Self::TapThemes, 3) => "a tap in the right column",
            (Self::TapThemes, _) => "a tap in the middle column",
        }
    }

    /// The game this preset is for, in words.
    const fn game(self) -> &'static str {
        match self {
            Self::Revenge => "Tap Tap Revenge",
            Self::Tour => "Tap Tap Revenge: Tour",
            Self::TapThemes => "a custom tap theme",
        }
    }
}

/// A shake, slice or mega tap, in words.
const fn kind_name(kind: NoteKind) -> &'static str {
    match kind {
        NoteKind::Shake(Shake::Left) => "a left shake",
        NoteKind::Shake(Shake::Right) => "a right shake",
        NoteKind::Shake(Shake::Down) => "a down shake",
        NoteKind::Slice(Side::Left) => "a left slice",
        NoteKind::Slice(Side::Right) => "a right slice",
        NoteKind::MegaTap => "a mega tap",
        NoteKind::Tap { .. } | NoteKind::Hold { .. } => "a tap",
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
    preset: Preset,
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
    // A tempo change after every note, such as one the Glam As You Club Mix of Poker Face places
    // a thousand hours in, changes nothing the game plays.
    let end = levels
        .iter()
        .flat_map(|level| &level.notes)
        .map(|note| note.off.max(note.on))
        .filter(|end| end.is_finite())
        .fold(0.0_f64, f64::max);
    for level in &mut levels {
        let first = level.tempo.first().copied();
        let before = level.tempo.len();
        level
            .tempo
            .retain(|change| change.seconds <= end || Some(*change) == first);
        for _ in level.tempo.len()..before {
            report.add("a tempo change after the last note");
        }
    }
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
                lanes: level.lanes,
                name: level.difficulty.name().to_owned(),
                difficulty: Some(level.difficulty),
                theme: named(level.theme.as_deref(), report),
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
        genre: track.genre,
        ext: charter_core::Extensions::new(),
    };
    let theme = named(track.theme.as_deref(), report);
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
            notes: notes_of(level, preset, placed.timing(), report),
            ..chart
        })
        .collect();
    Ok(
        Song::new(metadata, ticks_per_beat, tempo, Vec::new(), charts)
            .map_err(|why| why.to_string())?
            .with_theme(theme)
            .to_document(),
    )
}

/// `theme` as a theme name, or none, said in `report`, when it cannot be one.
fn named(theme: Option<&str>, report: &mut Report) -> Option<charter_core::Name> {
    let theme = theme?;
    let name = charter_core::Name::new(theme).ok();
    if name.is_none() {
        report.add(format!("the theme {theme:?}, which is not a theme name"));
    }
    name
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

/// `level`'s notes as `preset` plays them, on `timing`'s ticks, without those on a lane and tick
/// an earlier one took.
fn notes_of(
    level: &Level,
    preset: Preset,
    timing: Valid<'_, Timing>,
    report: &mut Report,
) -> Vec<Note> {
    // Taps and holds take their columns, and shakes, slices and mega taps theirs, apart: a tap and
    // a shake may share a column at once, as the game plays both.
    let mut taken: HashSet<(i64, u16, bool)> = HashSet::new();
    let mut notes = Vec::with_capacity(level.notes.len());
    for note in &level.notes {
        let (Some(on), Some(off)) = (
            ResolvedTime::from_seconds(note.on),
            ResolvedTime::from_seconds(note.off),
        ) else {
            report.add("a note at a moment that is not a time");
            continue;
        };
        let tick = nearest(timing, on);
        let held = ((off - on).seconds() > HOLD_SECONDS)
            .then(|| tick.ticks_until(nearest(timing, off)))
            .flatten();
        let Some(kind) = preset.kind(note.key, level.lanes, held) else {
            report.add(format!(
                "notes on a key a {}-lane {} level has no column for",
                level.lanes.width(),
                level.difficulty.name()
            ));
            continue;
        };
        let Position::Lane { index, width } = level.lanes.position(kind) else {
            continue;
        };
        let first = u16::from(index);
        let lanes = first..first.saturating_add(u16::from(width));
        let column = matches!(kind, NoteKind::Tap { .. } | NoteKind::Hold { .. });
        if lanes
            .clone()
            .any(|lane| taken.contains(&(i64::from(tick), lane, column)))
        {
            report.add("a note on a column and tick another note already takes");
            continue;
        }
        taken.extend(lanes.map(|lane| (i64::from(tick), lane, column)));
        notes.push(Note { tick, kind });
    }
    notes.sort_by_key(|note| i64::from(note.tick));
    notes
}

/// The tick `timing` puts nearest `at` in the audio.
fn nearest(timing: Valid<'_, Timing>, at: ResolvedTime) -> Tick {
    let before = timing.latest_tick_by(at);
    let after = Ticks::new(1).and_then(|one| before.checked_add(one));
    let off = |tick: Tick| (timing.time_at(tick) - at).seconds().abs();
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
pub(crate) fn from_song(
    valid: &ValidDocument,
    preset: Preset,
    report: &mut Report,
) -> Result<Track, String> {
    let song = Song::try_from(valid).map_err(|why| why.to_string())?;
    let ticks_per_beat = song.ticks_per_beat();
    for _ in song.markers() {
        report.add("a section or bookmark marker");
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
            theme: None,
            lanes: chart.lanes,
            tempo: changes_of(timing, report),
            notes: played_of(chart, preset, timing, ticks_per_beat, report),
        });
    }
    if levels.is_empty() {
        return Err("no chart is Easy, Medium, Hard or Extreme".to_owned());
    }
    levels.sort_by_key(|level| level.difficulty.index());
    Ok(Track {
        title: song.metadata().title.clone(),
        artist: song.metadata().artist.clone(),
        genre: None,
        theme: None,
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

/// `chart`'s notes on the keys `preset` plays them on, at their moments on `timing`.
fn played_of(
    chart: &Chart,
    preset: Preset,
    timing: Valid<'_, Timing>,
    ticks_per_beat: TicksPerBeat,
    report: &mut Report,
) -> Vec<Played> {
    let seconds = |tick: Tick| timing.time_at(tick).seconds();
    // The game's own tracks end a tap a sixteenth after it starts.
    let sixteenth = Ticks::new(u32::from(u16::from(ticks_per_beat)).div_euclid(4));
    chart
        .notes
        .iter()
        .filter_map(|note| {
            let on = seconds(note.tick);
            let (key, _) = preset.key(note.kind, chart.lanes, report)?;
            let off = if let NoteKind::Hold { duration, .. } = note.kind {
                let off = note.tick.checked_add(duration).map_or(on, seconds);
                if off - on <= HOLD_SECONDS {
                    report.add("a hold of 0.15 seconds or less, which the game plays as a tap");
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
            theme: None,
            lanes: Lanes::Three,
            tempo: vec![Change {
                seconds: 0.0,
                bpm: 120.0,
                meter: four(),
            }],
            notes,
        }
    }

    fn played(key: u8, on: f64, off: f64) -> Played {
        Played { key, on, off }
    }

    fn song(levels: Vec<Level>, report: &mut Report) -> Song {
        song_as(levels, Preset::Revenge, report)
    }

    fn song_as(levels: Vec<Level>, preset: Preset, report: &mut Report) -> Song {
        let track = Track {
            title: "Sing".to_owned(),
            artist: "My Chemical Romance".to_owned(),
            genre: None,
            theme: None,
            levels,
        };
        Song::try_from(&to_song(track, "audio.m4a", preset, report).unwrap()).unwrap()
    }

    #[test]
    fn each_game_plays_the_side_and_middle_keys_of_three_lanes_its_own_way() {
        let tap = |lane| {
            Some(NoteKind::Tap {
                lane: LaneIndex::new(lane),
            })
        };
        let beat = Ticks::new(960);
        let read = |preset: Preset| -> Vec<Option<NoteKind>> {
            (0..7)
                .map(|key| preset.kind(key, Lanes::Three, beat))
                .collect()
        };
        let hold = |lane| {
            Some(NoteKind::Hold {
                lane: LaneIndex::new(lane),
                duration: beat.unwrap(),
            })
        };
        let columns = [hold(0), hold(1), hold(2)];
        assert_eq!(
            read(Preset::Revenge),
            [
                columns[0],
                Some(NoteKind::Shake(Shake::Left)),
                columns[1],
                Some(NoteKind::Shake(Shake::Right)),
                columns[2],
                Some(NoteKind::Shake(Shake::Down)),
                None,
            ]
        );
        assert_eq!(
            read(Preset::Tour),
            [
                columns[0],
                Some(NoteKind::Slice(Side::Left)),
                columns[1],
                Some(NoteKind::Slice(Side::Right)),
                columns[2],
                Some(NoteKind::MegaTap),
                None,
            ]
        );
        // A tap theme's own columns for the side and middle keys cannot hold.
        assert_eq!(
            read(Preset::TapThemes),
            [
                columns[0],
                tap(0),
                columns[1],
                tap(2),
                columns[2],
                tap(1),
                None
            ]
        );
    }

    #[test]
    fn four_and_five_lanes_play_their_keys_left_to_right_whatever_the_game() {
        for preset in [Preset::Revenge, Preset::Tour, Preset::TapThemes] {
            for lanes in [Lanes::Four, Lanes::Five] {
                let width = u8::try_from(u16::from(lanes.width())).unwrap();
                for key in 0..KEYS {
                    let expected = (key < width).then(|| NoteKind::Tap {
                        lane: LaneIndex::new(key.into()),
                    });
                    assert_eq!(preset.kind(key, lanes, None), expected);
                }
            }
        }
    }

    #[test]
    fn a_kind_a_game_plays_otherwise_is_written_on_that_key_and_said_so() {
        let mut report = Report::default();
        assert_eq!(
            Preset::Revenge.key(NoteKind::Slice(Side::Right), Lanes::Three, &mut report),
            Some((3, false))
        );
        assert_eq!(
            Preset::Tour.key(NoteKind::Shake(Shake::Down), Lanes::Three, &mut report),
            Some((5, false))
        );
        assert_eq!(
            Preset::Tour.key(NoteKind::MegaTap, Lanes::Three, &mut report),
            Some((5, false))
        );
        let hold = NoteKind::Hold {
            lane: LaneIndex::new(3),
            duration: Ticks::new(1).unwrap(),
        };
        assert_eq!(
            Preset::Tour.key(hold, Lanes::Five, &mut report),
            Some((3, true))
        );
        assert_eq!(
            Preset::Revenge.key(NoteKind::MegaTap, Lanes::Four, &mut report),
            None
        );
        assert_eq!(
            report.into_lines(),
            [
                "a down shake, which Tap Tap Revenge: Tour plays as a mega tap",
                "a right slice, which Tap Tap Revenge plays as a right shake",
                "a shake, slice or mega tap in a 4-lane level, which has keys for its columns alone",
            ]
        );
    }

    #[test]
    fn a_four_lane_level_imports_as_a_four_lane_chart() {
        let mut report = Report::default();
        let mut wide = level(
            Difficulty::Hard,
            vec![played(3, 1.0, 2.0), played(5, 1.5, 1.6)],
        );
        wide.lanes = Lanes::Four;
        let song = song_as(vec![wide], Preset::Tour, &mut report);
        let chart = &song.charts()[0];
        assert_eq!(chart.lanes, Lanes::Four);
        assert_eq!(
            chart.notes.iter().map(|note| note.kind).collect::<Vec<_>>(),
            [NoteKind::Hold {
                lane: LaneIndex::new(3),
                duration: Ticks::new(1920).unwrap()
            }]
        );
        assert_eq!(
            report.into_lines(),
            ["notes on a key a 4-lane Hard level has no column for"]
        );
    }

    #[test]
    fn keys_become_taps_holds_and_shakes_in_three_lanes() {
        let mut report = Report::default();
        let notes = vec![
            played(0, 0.5, 0.6),
            played(2, 1.0, 2.0),
            played(4, 1.5, 1.5),
            played(1, 2.0, 2.1),
            played(3, 2.5, 2.6),
            played(5, 3.0, 4.0),
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
            played(0, 1.0, 1.1),
            played(0, 1.0, 1.1),
            played(1, 1.0, 1.1),
            played(2, f64::NAN, 1.1),
        ];
        let levels = vec![
            level(Difficulty::Easy, notes),
            level(Difficulty::Easy, Vec::new()),
        ];
        let song = song(levels, &mut report);
        assert_eq!(song.charts().len(), 1);
        // The shake shares the left column with the tap, as the game plays both.
        assert_eq!(song.charts()[0].notes.len(), 2);
        assert_eq!(
            report.into_lines(),
            [
                "a note at a moment that is not a time",
                "a note on a column and tick another note already takes",
                "a second Easy level",
            ]
        );
    }

    #[test]
    fn a_level_with_its_own_tempo_keeps_it_and_levels_come_easiest_first() {
        let mut report = Report::default();
        let mut hard = level(Difficulty::Hard, vec![played(0, 1.0, 1.0)]);
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
        let none =
            to_song(Track::default(), "audio.m4a", Preset::Revenge, &mut report).unwrap_err();
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
            played(0, 0.5, 0.6),
            played(2, 1.0, 2.0),
            played(5, 3.0, 3.1),
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
        let track = from_song(&song.to_document(), Preset::Revenge, &mut report).unwrap();
        assert_eq!(track.title, "Sing");
        let [easy, extreme] = track.levels.as_slice() else {
            panic!("two levels: {:?}", track.levels);
        };
        assert_eq!(
            (easy.difficulty, easy.lanes),
            (Difficulty::Easy, Lanes::Four)
        );
        assert_eq!(extreme.difficulty, Difficulty::Extreme);
        let keys: Vec<u8> = extreme.notes.iter().map(|note| note.key).collect();
        assert_eq!(keys, [0, 2, 5, 1]);
        let hold = extreme.notes[1];
        assert!((hold.on - 1.0).abs() < 1e-9 && (hold.off - 2.0).abs() < 1e-9);
        let tap = extreme.notes[0];
        assert!(
            (tap.off - tap.on - HOLD_SECONDS / 2.0).abs() < 1e-9,
            "a sixteenth at 120 BPM, kept within half the hold threshold"
        );
        assert_eq!(extreme.tempo.len(), 1);
        assert_eq!(
            report.into_lines(),
            [
                "a left slice, which Tap Tap Revenge plays as a left shake",
                "the chart \"Unlevelled\", which is none of the four levels",
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
