//! Tap Tap Revenge's `.ttr2_track`: a keyed archive, as Apple's `NSKeyedArchiver` writes one, in a
//! binary or XML property list. Its root is a dictionary from each level's number, 1 for Easy to
//! 4 for Extreme, to a `KBMidiFile`: the level's tempo changes, and tracks of `KBMidiEvent`s, each
//! a MIDI note switched on or off at a moment of the audio or a line of text.
//!
//! A level's column count is not in the file but in the `info.plist` beside it, as
//! `columnCountPerLevel`, a dictionary from each level's number to its columns, 3 when a level
//! has none.
//!
//! [`plist`] reads and writes the property list; this module walks the archive's fixed shape
//! itself. The keyed-archive crates decode any object graph by recursion, which a hostile file
//! can make overflow the stack or grow without bound, and none writes custom classes.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::Cursor;

use plist::{Dictionary, Uid, Value};

use charter_core::{FileName, InvalidValue};

use crate::exchange::{
    Change, KEYS, Level, MAX_NOTES, Moment, Played, Report, Track, TrackBpm, TrackKey, beats_at,
};
use crate::{Difficulty, Lanes, Song};

/// The MIDI key of a level's lowest note.
const LOWEST: i64 = 60;

/// A `KBMidiEvent` switching a note on, or off with a velocity of zero.
const NOTE_ON: i64 = 0;
/// A `KBMidiEvent` switching a note off.
const NOTE_OFF: i64 = 1;
/// A `KBMidiEvent` holding text, such as `TITLE: Sing`.
const TEXT: i64 = 2;

/// The track in `bytes`, its levels' columns as `info`, the `info.plist` beside it, sets them,
/// and what of it the song leaves out in `report`.
///
/// # Errors
///
/// Returns why `bytes` holds no track: not a property list, not a keyed archive, or more notes
/// than any chart.
pub(crate) fn read(
    bytes: &[u8],
    info: Option<&[u8]>,
    report: &mut Report,
) -> Result<Track, String> {
    let archive = Value::from_reader(Cursor::new(bytes))
        .map_err(|why| format!("not a property list: {why}"))?;
    let not_archive = || "not a keyed archive of levels".to_owned();
    let mut archive = archive.into_dictionary().ok_or_else(not_archive)?;
    let Some(Value::Array(objects)) = archive.remove("$objects") else {
        return Err(not_archive());
    };
    let top = archive.remove("$top").ok_or_else(not_archive)?;
    let objects = Objects(&objects);
    let root = objects.field(&top, "root").ok_or_else(not_archive)?;
    let keys = objects.items(objects.field(root, "NS.keys").ok_or_else(not_archive)?);
    let files = objects.items(objects.field(root, "NS.objects").ok_or_else(not_archive)?);
    let info = info
        .map(|info| Info::read(info, report))
        .unwrap_or_default();
    let mut track = Track::default();
    let mut notes = 0_usize;
    let single = keys.len() == 1;
    for (key, file) in keys.into_iter().zip(files) {
        // A track of one level may number it apart from info.plist, as Muse's tracks do, under
        // 0 or 4 for a level info.plist offers as 3; the game plays it as info.plist says.
        let level = match (single, info.levels.as_slice()) {
            (true, &[offered]) => Some(offered),
            _ => key
                .as_signed_integer()
                .or_else(|| key.as_string()?.trim().parse().ok()),
        };
        let Some(difficulty) = level.and_then(difficulty_of) else {
            report.add(format!(
                "level {}, which is none of Easy, Medium, Hard and Extreme",
                level.map_or_else(|| "without a number".to_owned(), |level| level.to_string())
            ));
            continue;
        };
        // Keys such as "4" and "04" name one level; reading each again would multiply one
        // file's tempo list as many times as a hostile track lists it.
        if track
            .levels
            .iter()
            .any(|read| read.difficulty == difficulty)
        {
            report.add(format!(
                "another {} level, after the first",
                difficulty.name()
            ));
            continue;
        }
        let (read, title, artist) = read_file(&objects, file, difficulty, &mut notes, report)?;
        if track.levels.is_empty() {
            track.title = title;
            track.artist = artist;
        }
        track.levels.push(read);
    }
    for level in &mut track.levels {
        let number = i64::try_from(level.difficulty.index())
            .unwrap_or(0)
            .saturating_add(1);
        if let Some(&lanes) = info.columns.get(&number) {
            level.lanes = lanes;
        }
        // A level in the song's theme keeps none of its own.
        level.theme = info
            .themes
            .get(&number)
            .filter(|&theme| info.theme.as_ref() != Some(theme))
            .cloned();
    }
    // The game shows info.plist's title and artist; Muse's tracks title their archives "No File
    // Selected".
    track.title = info.title.unwrap_or(track.title);
    track.artist = info.artist.unwrap_or(track.artist);
    track.genre = info.genre;
    track.theme = info.theme;
    Ok(track)
}

/// What a track's `info.plist` says of its levels.
#[derive(Debug, Default)]
struct Info {
    /// The levels it offers, by number, from `availableDifficultyLevels`.
    levels: Vec<i64>,
    /// Each level's columns, by number, from `columnCountPerLevel`.
    columns: BTreeMap<i64, Lanes>,
    /// The title the game shows, from `kTTRDownloadableItemTitleKey`.
    title: Option<String>,
    /// The artist it shows, from `kTTRDownloadableItemSubtitleKey`.
    artist: Option<String>,
    /// The genre, from `kTTRGenre`.
    genre: Option<String>,
    /// The theme of every level that names none, from `themeName`.
    theme: Option<String>,
    /// Each level's theme, by number, from `levelThemes`.
    themes: BTreeMap<i64, String>,
}

impl Info {
    /// What `info`, the bytes of an `info.plist`, says, saying in `report` what does not read.
    fn read(info: &[u8], report: &mut Report) -> Self {
        let Ok(info) = Value::from_reader(Cursor::new(info)) else {
            report.add("the info.plist beside the track, which is not a property list");
            return Self::default();
        };
        let field = |key| info.as_dictionary()?.get(key);
        let levels = field("availableDifficultyLevels")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_signed_integer)
            .collect();
        let columns = field("columnCountPerLevel")
            .and_then(Value::as_dictionary)
            .into_iter()
            .flatten()
            .filter_map(|(level, count)| {
                let lanes = count
                    .as_signed_integer()
                    .and_then(|count| u16::try_from(count).ok())
                    .and_then(charter_core::LaneCount::new)
                    .and_then(Lanes::of);
                if lanes.is_none() {
                    report.add("a column count other than 3, 4 or 5 in info.plist");
                }
                Some((level.trim().parse().ok()?, lanes?))
            })
            .collect();
        let text = |key| {
            field(key)
                .and_then(Value::as_string)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        let themes = field("levelThemes")
            .and_then(Value::as_dictionary)
            .into_iter()
            .flatten()
            .filter_map(|(level, theme)| {
                Some((level.trim().parse().ok()?, theme.as_string()?.to_owned()))
            })
            .collect();
        Self {
            levels,
            columns,
            title: text("kTTRDownloadableItemTitleKey"),
            artist: text("kTTRDownloadableItemSubtitleKey"),
            genre: text("kTTRGenre"),
            theme: text("themeName"),
            themes,
        }
    }
}

/// The `info.plist` a track folder holding `track`, of `song`, keeps beside it, as the game's own
/// and custom tracks write theirs: its title and artist, levels, columns and themes, the files the
/// folder holds, and how long `audio` says the audio lasts.
///
/// # Errors
///
/// Returns why a theme cannot name a file in the fixed `Themes` folder.
#[expect(
    clippy::too_many_lines,
    reason = "the game's single info.plist holds all of these fixed fields"
)]
pub(crate) fn info(
    song: &Song,
    track: &Track,
    audio: Option<std::time::Duration>,
) -> Result<Vec<u8>, InvalidValue> {
    let number = |level: &Level| level.difficulty.index().saturating_add(1);
    let numbers: Vec<Value> = track
        .levels
        .iter()
        .map(|level| Value::Integer(u64::try_from(number(level)).unwrap_or(0).into()))
        .collect();
    let columns: Dictionary = track
        .levels
        .iter()
        .map(|level| {
            let lanes = u64::from(u16::from(level.lanes.width()));
            (number(level).to_string(), Value::Integer(lanes.into()))
        })
        .collect();
    let level_theme = |level: &Level| {
        song.charts()
            .iter()
            .find(|chart| chart.difficulty == Some(level.difficulty))
            .and_then(|chart| chart.theme.as_ref().map(charter_core::Name::as_str))
            .or_else(|| song.theme())
    };
    let themes: Dictionary = track
        .levels
        .iter()
        .filter_map(|level| Some((number(level).to_string(), Value::from(level_theme(level)?))))
        .collect();
    let theme = song
        .theme()
        .or_else(|| track.levels.iter().find_map(level_theme))
        .unwrap_or(crate::DEFAULT_THEME);
    let file = |name: &str| {
        let mut command = Dictionary::new();
        command.insert(
            "kTTRDownloadableItemPreferredFilenameKey".to_owned(),
            name.into(),
        );
        command.insert("kTTRDownloadableItemSourceURLKey".to_owned(), "null".into());
        Value::Dictionary(command)
    };
    let mut commands: Vec<_> = ["audio.m4a", "artwork.png", "taptrack.ttr2_track"]
        .map(file)
        .into();
    let mut needed: Vec<&str> = themes.values().filter_map(Value::as_string).collect();
    needed.push(theme);
    needed.sort_unstable();
    needed.dedup();
    for dependency in needed {
        let mut command = Dictionary::new();
        let theme_file = FileName::new(format!("{dependency}.ttrTheme"))?;
        let path = format!("Themes/{theme_file}");
        command.insert("kTTRDownloadableItemDependencyKey".to_owned(), path.into());
        command.insert("kTTRDownloadableItemSourceURLKey".to_owned(), "null".into());
        command.insert("kTTRDownloadableItemVersionKey".to_owned(), 0_u64.into());
        commands.push(Value::Dictionary(command));
    }
    let metadata = song.metadata();
    let mut info = Dictionary::new();
    if let Some(length) = audio {
        info.insert("audioFileDuration".to_owned(), length.as_secs_f64().into());
    }
    info.insert(
        "availableDifficultyLevels".to_owned(),
        Value::Array(numbers),
    );
    info.insert("bossLevels".to_owned(), Value::Array(Vec::new()));
    info.insert("columnCountPerLevel".to_owned(), Value::Dictionary(columns));
    info.insert(
        "kTTRDownloadableItemCommandsKey".to_owned(),
        Value::Array(commands),
    );
    info.insert(
        "kTTRDownloadableItemTitleKey".to_owned(),
        metadata.title.as_str().into(),
    );
    info.insert(
        "kTTRDownloadableItemSubtitleKey".to_owned(),
        metadata.artist.as_str().into(),
    );
    info.insert("kTTRDownloadableItemTypeKey".to_owned(), "track".into());
    let unique = format!("{}: {}", metadata.artist, metadata.title);
    info.insert(
        "kTTRDownloadableItemUniqueIdentifierKey".to_owned(),
        unique.into(),
    );
    info.insert("kTTRDownloadableItemVersionKey".to_owned(), 0_u64.into());
    if let Some(genre) = &metadata.genre {
        info.insert("kTTRGenre".to_owned(), genre.as_str().into());
    }
    info.insert("levelThemes".to_owned(), Value::Dictionary(themes));
    info.insert("lockedLevels".to_owned(), Value::Array(Vec::new()));
    info.insert("themeName".to_owned(), theme.into());
    let mut bytes = Vec::new();
    #[expect(
        clippy::expect_used,
        clippy::unwrap_in_result,
        reason = "a plist of strings, numbers and lists always writes into memory"
    )]
    Value::Dictionary(info)
        .to_writer_xml(&mut bytes)
        .expect("an info.plist writes into memory");
    Ok(bytes)
}

/// The difficulty the game numbers `level`.
fn difficulty_of(level: i64) -> Option<Difficulty> {
    let index = usize::try_from(level.checked_sub(1)?).ok()?;
    Difficulty::ALL.get(index).copied()
}

/// The finite tempo changes of one keyed file, or its default tempo.
fn read_tempo(
    objects: &Objects<'_>,
    file: &Value,
    report: &mut Report,
) -> Result<Vec<Change>, String> {
    let mut tempo: Vec<Change> = objects
        .field(file, "tempoChanges")
        .map(|changes| objects.items(changes))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|change| {
            let seconds = number(objects.field(change, "startTime")?)?;
            let bpm = number(objects.field(change, "tempo")?)?;
            let result = Change::new(
                seconds,
                bpm,
                charter_core::TimeSignature::default().numerator,
            );
            if result.is_none() {
                report.add("a tempo change that is not a tempo");
            }
            result
        })
        .collect();
    if tempo.is_empty() {
        let bpm = ["exactTempo", "tempo"]
            .into_iter()
            .filter_map(|key| objects.field(file, key).and_then(number))
            .find_map(|value| {
                let valid = TrackBpm::from_bpm(value);
                if valid.is_none() {
                    report.add("a tempo change that is not a tempo");
                }
                valid
            })
            .unwrap_or(TrackBpm::DEFAULT);
        let change = Change::new(
            0.0,
            bpm.bpm(),
            charter_core::TimeSignature::default().numerator,
        )
        .ok_or("a track has no finite tempo")?;
        tempo.push(change);
    }
    tempo.sort_by(|a, b| a.seconds.seconds().total_cmp(&b.seconds.seconds()));
    Ok(tempo)
}

/// The level in the `KBMidiFile` `file`, with the title and artist it names, counting its notes
/// into `notes`.
fn read_file(
    objects: &Objects<'_>,
    file: &Value,
    difficulty: Difficulty,
    notes: &mut usize,
    report: &mut Report,
) -> Result<(Level, String, String), String> {
    let text = |key| objects.field(file, key).and_then(Value::as_string);
    let mut title = text("title").unwrap_or_default().to_owned();
    let mut artist = text("copyrightInfo").unwrap_or_default().to_owned();
    let tempo = read_tempo(objects, file, report)?;
    let mut open: HashMap<TrackKey, VecDeque<Moment>> = HashMap::new();
    let mut played: Vec<Played> = Vec::new();
    let tracks = objects
        .field(file, "tracks")
        .map(|tracks| objects.items(tracks))
        .unwrap_or_default();
    for events in tracks
        .into_iter()
        .filter_map(|track| objects.field(track, "events"))
    {
        for event in objects.items(events) {
            *notes = notes.saturating_add(1);
            if *notes > MAX_NOTES {
                return Err("the file holds more notes than any chart".to_owned());
            }
            let integer = |key| objects.field(event, key).and_then(Value::as_signed_integer);
            let kind = integer("type");
            if kind == Some(TEXT) {
                let line = objects.field(event, "text").and_then(Value::as_string);
                if let Some(named) = line.and_then(|line| line.strip_prefix("TITLE:")) {
                    named.trim().clone_into(&mut title);
                } else if let Some(named) = line.and_then(|line| line.strip_prefix("ARTIST:")) {
                    named.trim().clone_into(&mut artist);
                }
                continue;
            }
            let Some(time) = objects.field(event, "time").and_then(number) else {
                continue;
            };
            let Some(time) = Moment::from_seconds(time) else {
                report.add("a note at a moment that is not a time");
                continue;
            };
            // Keys repeat each octave: one of Tap Tap Reloaded's tracks plays its level on 96 up.
            let note = integer("note").unwrap_or_default();
            let Some(key) = u8::try_from(note.saturating_sub(LOWEST).rem_euclid(i64::from(KEYS)))
                .ok()
                .and_then(TrackKey::new)
            else {
                report.add("a note on a key outside its level");
                continue;
            };
            match (kind, integer("velocity").unwrap_or_default()) {
                (Some(NOTE_ON), velocity) if velocity > 0 => {
                    open.entry(key).or_default().push_back(time);
                }
                (Some(NOTE_ON | NOTE_OFF), _) => {
                    if let Some(on) = open.get_mut(&key).and_then(VecDeque::pop_front) {
                        played.push(Played { key, on, off: time });
                    }
                }
                _ => {}
            }
        }
    }
    // A note never switched off is a tap.
    played.extend(
        open.into_iter()
            .flat_map(|(key, ons)| ons.into_iter().map(move |on| Played { key, on, off: on })),
    );
    played.sort_by(|a, b| a.on.seconds().total_cmp(&b.on.seconds()));
    let level = Level {
        difficulty,
        theme: None,
        lanes: Lanes::Three,
        tempo,
        notes: played,
    };
    Ok((level, title, artist))
}

/// A number stored as a real or an integer.
fn number(value: &Value) -> Option<f64> {
    value.as_real().or_else(|| {
        value
            .as_signed_integer()
            .and_then(|integer| i32::try_from(integer).ok())
            .map(f64::from)
    })
}

/// The objects of a keyed archive, which references index.
struct Objects<'a>(&'a [Value]);

impl<'a> Objects<'a> {
    /// `value`, or the object it references: a binary property list stores a reference as a
    /// [`Value::Uid`], an XML one as a dictionary holding only `CF$UID`.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "any other value is a value, not a reference"
    )]
    fn object(&self, value: &'a Value) -> Option<&'a Value> {
        let reference = match value {
            Value::Uid(uid) => Some(uid.get()),
            Value::Dictionary(fields) if fields.len() == 1 => {
                fields.get("CF$UID").and_then(Value::as_unsigned_integer)
            }
            _ => None,
        };
        match reference {
            Some(index) => self.0.get(usize::try_from(index).ok()?),
            None => Some(value),
        }
    }

    /// The field `key` of `object`, or the object it references.
    fn field(&self, object: &'a Value, key: &str) -> Option<&'a Value> {
        self.object(self.object(object)?.as_dictionary()?.get(key)?)
    }

    /// The items of the array `list`, an `NSArray` or a plain one, each followed to its object.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "any other value holds no items"
    )]
    fn items(&self, list: &'a Value) -> Vec<&'a Value> {
        let items = self.object(list).and_then(|list| match list {
            Value::Array(items) => Some(items),
            Value::Dictionary(fields) => fields.get("NS.objects").and_then(Value::as_array),
            _ => None,
        });
        items
            .into_iter()
            .flatten()
            .filter_map(|item| self.object(item))
            .collect()
    }
}

/// `track` as a binary `.ttr2_track`, the form the game's own tracks take.
pub(crate) fn write(track: &Track) -> Vec<u8> {
    let mut archive = Archive::default();
    let (keys, files): (Vec<Value>, Vec<Value>) = track
        .levels
        .iter()
        .map(|level| {
            let number = level.difficulty.index().saturating_add(1);
            let key = archive.add(Value::Integer(u64::try_from(number).unwrap_or(0).into()));
            (Value::Uid(key), Value::Uid(archive.file(track, level)))
        })
        .unzip();
    let root = archive.object(
        &["NSMutableDictionary", "NSDictionary"],
        [
            ("NS.keys", Value::Array(keys)),
            ("NS.objects", Value::Array(files)),
        ],
    );
    let mut top = Dictionary::new();
    top.insert("root".to_owned(), Value::Uid(root));
    let mut keyed = Dictionary::new();
    keyed.insert("$archiver".to_owned(), "NSKeyedArchiver".into());
    keyed.insert("$version".to_owned(), 100_000_u64.into());
    keyed.insert("$top".to_owned(), top.into());
    keyed.insert("$objects".to_owned(), Value::Array(archive.objects));
    let mut bytes = Vec::new();
    #[expect(
        clippy::expect_used,
        reason = "a plist of strings, numbers and references always writes into memory"
    )]
    Value::Dictionary(keyed)
        .to_writer_binary(&mut bytes)
        .expect("an archive writes into memory");
    bytes
}

/// A keyed archive being written: its objects, the first `$null`, and the classes among them.
struct Archive {
    /// Every object, which references index.
    objects: Vec<Value>,
    /// Where each class's entry is.
    classes: HashMap<&'static str, Uid>,
}

impl Default for Archive {
    fn default() -> Self {
        Self {
            objects: vec![Value::String("$null".to_owned())],
            classes: HashMap::new(),
        }
    }
}

impl Archive {
    /// Adds `value` as an object, and where it is.
    fn add(&mut self, value: Value) -> Uid {
        self.objects.push(value);
        Uid::new(u64::try_from(self.objects.len().saturating_sub(1)).unwrap_or(0))
    }

    /// The entry of the class `names` begins with, the class first and its ancestors after, added
    /// on first use.
    fn class(&mut self, names: &[&'static str]) -> Uid {
        let name = names.first().copied().unwrap_or("NSObject");
        if let Some(&uid) = self.classes.get(name) {
            return uid;
        }
        let mut entry = Dictionary::new();
        entry.insert("$classname".to_owned(), name.into());
        let lineage = names.iter().chain(&["NSObject"]).map(|&class| class.into());
        entry.insert("$classes".to_owned(), Value::Array(lineage.collect()));
        let uid = self.add(entry.into());
        self.classes.insert(name, uid);
        uid
    }

    /// Adds an object of the class `names` begins with, holding `fields`.
    fn object<const N: usize>(
        &mut self,
        names: &[&'static str],
        fields: [(&str, Value); N],
    ) -> Uid {
        let class = self.class(names);
        let mut object: Dictionary = fields
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect();
        object.insert("$class".to_owned(), Value::Uid(class));
        self.add(object.into())
    }

    /// Adds an `NSMutableArray` of `items`.
    fn array(&mut self, items: Vec<Uid>) -> Uid {
        let items = items.into_iter().map(Value::Uid).collect();
        self.object(
            &["NSMutableArray", "NSArray"],
            [("NS.objects", Value::Array(items))],
        )
    }

    /// Adds `text` as a string object.
    fn text(&mut self, text: &str) -> Value {
        Value::Uid(self.add(text.into()))
    }

    /// Adds a `KBMidiEvent`.
    fn event(&mut self, kind: i64, key: u8, seconds: f64, beats: f64, text: Option<&str>) -> Uid {
        let text = text.map_or(Value::Uid(Uid::new(0)), |text| self.text(text));
        let on = kind == NOTE_ON;
        self.object(
            &["KBMidiEvent"],
            [
                ("type", kind.into()),
                ("time", seconds.into()),
                ("timeInQuarterNotes", beats.into()),
                ("note", u64::from(key).into()),
                ("channel", 0_u64.into()),
                ("velocity", if on { 100_u64 } else { 0 }.into()),
                ("text", text),
            ],
        )
    }

    /// Adds `level` of `track` as a `KBMidiFile`: a track of its title and artist, and one of its
    /// notes.
    fn file(&mut self, track: &Track, level: &Level) -> Uid {
        let tempo = &level.tempo;
        let changes = tempo
            .iter()
            .map(|change| {
                self.object(
                    &["KBMidiTempoMap"],
                    [
                        ("startTime", change.seconds.seconds().into()),
                        ("tempo", change.bpm.bpm().into()),
                    ],
                )
            })
            .collect();
        let changes = self.array(changes);
        let title = format!("TITLE: {}", track.title);
        let artist = format!("ARTIST: {}", track.artist);
        let lines = [title, artist]
            .iter()
            .map(|line| self.event(TEXT, 0, 0.0, 0.0, Some(line)))
            .collect();
        let lines = self.array(lines);
        let lines = self.object(&["KBMidiTrack"], [("events", Value::Uid(lines))]);
        let mut switches: Vec<(Moment, i64, u8)> = level
            .notes
            .iter()
            .flat_map(|note| {
                let key = note.key.get().saturating_add(60);
                [(note.on, NOTE_ON, key), (note.off, NOTE_OFF, key)]
            })
            .collect();
        // A note ending where the next on its key starts switches off first.
        switches.sort_by(|a, b| a.0.seconds().total_cmp(&b.0.seconds()).then(b.1.cmp(&a.1)));
        let notes = switches
            .into_iter()
            .map(|(seconds, kind, key)| {
                self.event(kind, key, seconds.seconds(), beats_at(tempo, seconds), None)
            })
            .collect();
        let notes = self.array(notes);
        let notes = self.object(&["KBMidiTrack"], [("events", Value::Uid(notes))]);
        let tracks = self.array(vec![lines, notes]);
        let named = self.text(&track.title);
        let performer = self.text(&track.artist);
        let raw = Value::Uid(self.add(Value::Data(Vec::new())));
        let first = tempo
            .first()
            .map_or(TrackBpm::DEFAULT.bpm(), |change| change.bpm.bpm());
        self.object(
            &["KBMidiFile"],
            [
                ("title", named),
                ("copyrightInfo", performer),
                ("trackID", 0_u64.into()),
                ("timeDivision", 480.0.into()),
                ("exactTempo", first.into()),
                ("rawData", raw),
                ("tempoChanges", Value::Uid(changes)),
                ("tracks", Value::Uid(tracks)),
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use proptest::prelude::*;

    use super::*;

    fn four() -> NonZeroU16 {
        NonZeroU16::new(4).unwrap()
    }

    fn track() -> Track {
        let played = |key, on: f64, off: f64| Played::new(key, on, off).unwrap();
        Track {
            title: "House Music".to_owned(),
            artist: "Benny Benassi".to_owned(),
            genre: None,
            theme: None,
            levels: vec![
                Level {
                    difficulty: Difficulty::Easy,
                    theme: None,
                    lanes: Lanes::Three,
                    tempo: vec![Change::new(0.0, 130.000_13, four()).unwrap()],
                    notes: vec![played(2, 3.0, 3.115), played(2, 3.115, 3.5)],
                },
                Level {
                    difficulty: Difficulty::Extreme,
                    theme: None,
                    lanes: Lanes::Three,
                    tempo: vec![
                        Change::new(0.5, 100.0, four()).unwrap(),
                        Change::new(10.0, 150.0, four()).unwrap(),
                    ],
                    notes: vec![played(5, 3.0, 3.1), played(0, 12.0, 13.0)],
                },
            ],
        }
    }

    #[test]
    fn a_written_track_reads_back_as_it_was() {
        let mut report = Report::default();
        let bytes = write(&track());
        assert!(bytes.starts_with(b"bplist00"));
        assert_eq!(read(&bytes, None, &mut report).unwrap(), track());
        assert!(report.into_lines().is_empty());
    }

    #[test]
    fn a_written_note_carries_the_beat_the_game_counts_it_on() {
        let bytes = write(&track());
        let archive = Value::from_reader(Cursor::new(bytes)).unwrap();
        let objects = archive.as_dictionary().unwrap()["$objects"]
            .as_array()
            .unwrap();
        let beats: Vec<f64> = objects
            .iter()
            .filter_map(Value::as_dictionary)
            .filter(|event| event.get("note").and_then(Value::as_signed_integer) == Some(60))
            .filter_map(|event| event["timeInQuarterNotes"].as_real())
            .collect();
        // 12 s at 100 BPM from 0.5 s, then 150 BPM from 10 s: 20.83 beats, then 5 more.
        assert!((beats[0] - (10.0 * 100.0 / 60.0 + 2.0 * 150.0 / 60.0)).abs() < 1e-9);
    }

    /// An XML archive as `MidiToTTR` writes one: the level's key a string, references `CF$UID`
    /// dictionaries, and `events` holding what is given.
    fn xml(level: &str, events: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>$archiver</key><string>NSKeyedArchiver</string>
<key>$version</key><integer>100000</integer>
<key>$top</key><dict><key>root</key><dict><key>CF$UID</key><integer>1</integer></dict></dict>
<key>$objects</key><array>
<string>$null</string>
<dict><key>NS.keys</key><array><dict><key>CF$UID</key><integer>2</integer></dict></array>
<key>NS.objects</key><array><dict><key>CF$UID</key><integer>3</integer></dict></array></dict>
<string>{level}</string>
<dict><key>title</key><dict><key>CF$UID</key><integer>4</integer></dict>
<key>exactTempo</key><real>90</real>
<key>tracks</key><dict><key>CF$UID</key><integer>5</integer></dict></dict>
<string>Untitled</string>
<dict><key>NS.objects</key><array><dict><key>CF$UID</key><integer>6</integer></dict></array></dict>
<dict><key>events</key><array>{events}</array></dict>
<string>TITLE:sudo</string>
</array></dict></plist>"#
        )
    }

    /// An inline `KBMidiEvent`.
    fn event(kind: i64, note: i64, velocity: i64, time: f64) -> String {
        format!(
            "<dict><key>type</key><integer>{kind}</integer><key>note</key><integer>{note}</integer>\
             <key>velocity</key><integer>{velocity}</integer><key>time</key><real>{time}</real></dict>"
        )
    }

    #[test]
    fn an_xml_archive_as_midi_to_ttr_writes_it_reads() {
        let title = "<dict><key>type</key><integer>2</integer>\
                     <key>text</key><dict><key>CF$UID</key><integer>7</integer></dict></dict>";
        let events = [
            title.to_owned(),
            event(0, 62, 100, 1.0),
            event(0, 62, 0, 2.0),
            event(0, 60, 100, 3.0),
            event(0, 70, 100, 4.0),
        ]
        .concat();
        let mut report = Report::default();
        let track = read(xml("4", &events).as_bytes(), None, &mut report).unwrap();
        assert_eq!(track.title, "sudo");
        let [extreme] = track.levels.as_slice() else {
            panic!("one level: {track:?}");
        };
        assert_eq!(extreme.difficulty, Difficulty::Extreme);
        assert!((extreme.tempo[0].bpm.bpm() - 90.0).abs() < f64::EPSILON);
        assert_eq!(
            extreme.notes,
            [
                Played::new(2, 1.0, 2.0).unwrap(),
                Played::new(0, 3.0, 3.0).unwrap(),
                // Key 70 is the level's eleventh, which no preset plays in three lanes.
                Played::new(10, 4.0, 4.0).unwrap(),
            ]
        );
        assert!(report.into_lines().is_empty());
    }

    #[test]
    fn a_kids_level_is_left_out() {
        let mut report = Report::default();
        let track = read(xml("0", "").as_bytes(), None, &mut report).unwrap();
        assert!(track.levels.is_empty());
        assert_eq!(
            report.into_lines(),
            ["level 0, which is none of Easy, Medium, Hard and Extreme"]
        );
    }

    #[test]
    fn what_is_not_a_keyed_archive_is_refused() {
        let mut report = Report::default();
        assert!(
            read(b"MThd", None, &mut report)
                .unwrap_err()
                .starts_with("not a property list")
        );
        let mut empty = Vec::new();
        Value::Dictionary(Dictionary::new())
            .to_writer_binary(&mut empty)
            .unwrap();
        assert_eq!(
            read(&empty, None, &mut report).unwrap_err(),
            "not a keyed archive of levels"
        );
        let dangling = xml("4", "").replace("<integer>3</integer>", "<integer>99</integer>");
        assert!(
            read(dangling.as_bytes(), None, &mut report)
                .unwrap()
                .levels
                .is_empty()
        );
    }

    #[test]
    fn a_level_listed_again_is_read_once() {
        let key = "<dict><key>CF$UID</key><integer>2</integer></dict>";
        let file = "<dict><key>CF$UID</key><integer>3</integer></dict>";
        let listed = xml("4", "")
            .replacen(
                &format!("<array>{key}</array>"),
                &format!("<array>{}</array>", key.repeat(3)),
                1,
            )
            .replacen(
                &format!("<array>{file}</array>"),
                &format!("<array>{}</array>", file.repeat(3)),
                1,
            );
        let mut report = Report::default();
        let track = read(listed.as_bytes(), None, &mut report).unwrap();
        assert_eq!(track.levels.len(), 1);
        let lines = report.into_lines();
        assert!(
            lines.iter().any(|line| line.contains("Extreme")),
            "{lines:?}"
        );
    }

    #[test]
    fn lists_sharing_their_entries_cannot_make_reading_unbounded() {
        let events = "<dict><key>CF$UID</key><integer>6</integer></dict>".repeat(2048);
        let tracks = "<dict><key>CF$UID</key><integer>6</integer></dict>".repeat(1024);
        let shared = xml("4", &events).replace(
            "<dict><key>NS.objects</key><array><dict><key>CF$UID</key><integer>6</integer></dict></array></dict>",
            &format!("<dict><key>NS.objects</key><array>{tracks}</array></dict>"),
        );
        let mut report = Report::default();
        assert_eq!(
            read(shared.as_bytes(), None, &mut report).unwrap_err(),
            "the file holds more notes than any chart"
        );
    }

    #[test]
    fn invalid_tempo_and_note_moments_are_not_kept_in_a_track() {
        let change = |seconds: f64, bpm: f64| {
            let mut fields = Dictionary::new();
            fields.insert("startTime".to_owned(), Value::Real(seconds));
            fields.insert("tempo".to_owned(), Value::Real(bpm));
            Value::Dictionary(fields)
        };
        let event = |kind: i64, time: f64| {
            let mut fields = Dictionary::new();
            fields.insert("type".to_owned(), kind.into());
            fields.insert("note".to_owned(), LOWEST.into());
            fields.insert("velocity".to_owned(), 100.into());
            fields.insert("time".to_owned(), Value::Real(time));
            Value::Dictionary(fields)
        };
        let mut notes = Dictionary::new();
        notes.insert(
            "events".to_owned(),
            Value::Array(vec![
                event(NOTE_ON, f64::NAN),
                event(NOTE_ON, 1.0),
                event(NOTE_OFF, 2.0),
            ]),
        );
        let mut file = Dictionary::new();
        file.insert(
            "tempoChanges".to_owned(),
            Value::Array(vec![
                change(f64::NAN, 120.0),
                change(0.0, 0.0),
                change(0.0, f64::INFINITY),
                change(0.0, 90.0),
            ]),
        );
        file.insert(
            "tracks".to_owned(),
            Value::Array(vec![Value::Dictionary(notes)]),
        );
        let file = Value::Dictionary(file);
        let mut report = Report::default();
        let (level, _, _) =
            read_file(&Objects(&[]), &file, Difficulty::Easy, &mut 0, &mut report).unwrap();
        assert_eq!(level.tempo.len(), 1);
        assert_eq!(level.notes.len(), 1);
        assert_eq!(level.tempo[0].seconds.seconds(), 0.0);
        assert_eq!(level.tempo[0].bpm.bpm(), 90.0);
        assert_eq!(level.notes[0].key.get(), 0);
        assert_eq!(level.notes[0].on.seconds(), 1.0);
        assert_eq!(level.notes[0].off.seconds(), 2.0);
        assert_eq!(
            report.into_lines(),
            [
                "a note at a moment that is not a time",
                "a tempo change that is not a tempo (3 times)"
            ]
        );
    }

    proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            drop(read(&bytes, None, &mut Report::default()));
        }

        #[test]
        fn a_damaged_track_never_panics(at in 0_usize..4096, byte in any::<u8>()) {
            let mut bytes = write(&track());
            if let Some(slot) = bytes.get_mut(at) {
                *slot = byte;
            }
            drop(read(&bytes, None, &mut Report::default()));
        }
    }
}
