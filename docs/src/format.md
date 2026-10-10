---
type: Format Specification
title: Chart format draft
description: Working v0 draft of charter's chart representation, kept small by interpreting as little as possible and pushing type safety to ruleset plugins.
tags: [format, data-model, draft]
status: draft
---

# Chart format draft

**v0, not stable.** An *internal* representation first; whether it is also an exchange format is
[open](research/topics/primary-format.md).

## Design rule

**The core interprets as little as possible.** Something is a core concept only if charter itself
must compute with it. Everything else is opaque to the core and belongs to a plugin.

The corollary matters as much as the rule: **the format types only what the core computes with,
and the plugin boundary is where everything else gets its type safety**. The core stores an
extension value as opaque data; the plugin that owns it deserializes it into its own types and is
free to be as strict as it likes. Pushing the strictness outward is what lets the core stay small
without the whole system becoming loose.

## What this commits to

| Decision | Position | Confidence |
|---|---|---|
| [Row resolution](research/topics/row-resolution.md) | 960 ticks/beat, configurable | High, derived arithmetically |
| [Musical position](research/topics/musical-position.md) | Integer ticks in time, six-place decimals in space | High |
| [Numbers](#numbers) | No stored floats: integers and six-place decimals | Medium |
| [Note type model](research/topics/note-type-model.md) | Structure in core, semantics in plugin | Medium |
| Ruleset scope | Exactly one per file, structurally; other plugins unrestricted | Medium |
| Keysounds | Core: a file-level bank objects reference | Medium |
| [Markers](#markers) | Core: song-wide sections and bookmarks on ticks | Medium |
| Multi-player | Modelled as extra lanes; exporters fold them back | Medium |
| Grouping | Value-keyed, so objects need no identity | Medium |
| [Internal model richness](research/topics/internal-model-richness.md) | Anchors and four interpreted timing event kinds, rest carried | Medium |
| [Gimmick coverage](research/topics/gimmick-coverage.md) | Everything representable, little interpreted | Medium |
| [Round-trip fidelity](research/topics/round-trip-fidelity.md) | Plugin data preserved by value | High |
| [Domain model purity](research/topics/domain-model-purity.md) | Transient editor state separate; layer visibility and locks saved | High |
| [Meter changes](research/topics/meter-changes.md) | Meter is timing data, policy left to the editor | Medium |
| Schema evolution | Closed core schema, open plugin data | Medium |
| Serialization | JSON | **Low, most likely to change** |

Deferred: [target audience](research/topics/target-audience.md),
[primary format](research/topics/primary-format.md),
[guitar format support](research/topics/guitar-format-support.md), and the
[plugin](research/topics/plugins.md) loading mechanism. Plugin *identity* is defined here; loading
is not.

---

## Versioning

### Format version

```
format_version: u32
```

One integer, incremented on **every** change to the core schema. A reader seeing a higher version
than it knows must refuse rather than guess.

The plugin boundary separates closed from open. Everything charter defines is closed: a reader
rejects any key it does not recognise, because such a key is either from a newer format, which the
version already reports, or a mistake. Everything a plugin writes sits in an opaque slot,
`settings`, `ext` or an `other` timing value, which the core carries without inspecting. A plugin
changes its own data without touching `format_version` and records the change in its
[own version](#plugin-versions).

The cost is that an older charter refuses a file from a newer one even after a purely additive
change. Carrying unknown core keys instead would need a catch-all field on every struct, and it
would turn a misspelled key into silently kept data rather than an error.

### Ownership

```
ruleset: { id: Namespace, version: String }     // exactly one
plugins: Map<Namespace, { version: String }>    // every other contributor
```

```json
{
  "ruleset": { "id": "arcaea", "version": "1.2.0" },
  "plugins": {
    "swing": { "version": "0.2.0" },
    "osu":   { "version": "0.1.0" }
  }
}
```

The ruleset sits in its own field rather than in the map, so the shape alone stops a file naming
two rulesets. Every other contributor, such as a timing plugin or an import adapter that left data
behind, goes in `plugins`, where any number may appear.

A namespace identifies its owner everywhere a plugin can leave data, always as
`<namespace>.<name>`: extension keys, note kinds, easings, marker kinds, and timing `other` names.
Unprefixed
names belong to the shared vocabulary and to no plugin. The grammar is under
[identifiers](#identifiers).

The declarations and the data must agree:

- Every namespace used anywhere in the file is either `ruleset.id` or a key in `plugins`
- Every key in `plugins` is **actually used** somewhere in the file
- `plugins` does not contain `ruleset.id`

Declarations track contributors, so a plugin that left nothing behind has no business being
listed, and a stale entry is a lie about what the file depends on. `ruleset` needs no such use: it
defines the chart's meaning whether or not it wrote a single extension key. Listing it in
`plugins` as well would give it a second version that could disagree with the first.

Together this is what makes data mappable back to its owner. Without it, `ext` is a bag of strings
whose meaning depends on where the file came from, which is unrecoverable once the file moves.

### Plugin versions

A bare namespace is not enough, because a plugin that changed the meaning or shape of its own data
between `1.2` and `2.0` needs to know which one it is reading before deserializing. Recording the
version lets the plugin migrate or refuse, and lets it do so **per namespace** rather than
failing the whole file.

The core never compares versions. The string is opaque to it and belongs to the owning plugin,
which may use semver or anything else.

A reader missing a plugin entirely, or holding an incompatible version, can still parse and
round-trip the file. It cannot render that ruleset or meaningfully edit that plugin's data, and it
reports a missing plugin on a different error path from a corrupt file.

---

## One ruleset per file

Every chart in a file is a difficulty of the file's one [ruleset](#ownership). Other plugins are
unrestricted: a file may pull in a timing plugin, an analysis plugin and an import adapter at
once, because those extend a chart rather than define what it is.

**A ruleset is a game, and layout belongs to the chart.** `sm` is a ruleset; dance-single and
dance-double are layouts. A real `.sm` file routinely holds both a single and a double chart, so
making layout the ruleset would make ordinary files unrepresentable under this rule.

Layout therefore stays per chart:

```
Chart {
    name:     String,               // "Challenge", "Future", "Expert+"
    lanes:    Option<LaneCount>,
    settings: Map<String, Value>,   // opaque, owned by the ruleset
    timing:   Option<Timing>,       // replaces the song's when present
    layers:   Vec<Layer>,           // optional, chart-local
    objects:  Vec<Object>,
    ext:      Extensions,
}
```

`name` is core only because the editor must list charts and tell them apart; ArrowVortex's chart
list is the reference. The numeric **rating is not core** and lives in `settings`, because rating
scales are ruleset-specific: Arcaea, StepMania and osu! do not agree on range or meaning, and a
shared `meter: u8` would be a number the core cannot interpret, compare or validate.

`lanes` is structural rather than semantic, so it is a field. The plugin knows dance-single has
four lanes, but **an editor missing that plugin does not**, and lane count is exactly what it needs
to draw a generic notefield and to check that every `Lane` position fits. It is optional because a
purely `Plane`-based ruleset has no lanes. The style *name* is the plugin's business and lives in
`settings`.

`charts` may be empty. Timing, metadata and samples alone make a sync-only document, which is what
Tempora produces and what charter would export to hand timing to another tool.

### Ruleset settings

`settings` is configuration the ruleset owns, opaque to the core and deserialized by the plugin
into its own types.

It appears on the file, for configuration that applies to the whole ruleset, and on each chart,
for configuration that differs per difficulty. The per-chart case is common: osu!'s difficulty
settings and Quaver's per-map scroll behaviour both live there. The two do not merge, for the same
reason per-chart timing [replaces](#per-chart-timing) the song's. The plugin reads two independent
maps and decides what they mean.

**`settings` needs no namespace prefix**, and that is a direct payoff of one ruleset per file.
Ownership is unambiguous, so the keys inside it are the plugin's own flat vocabulary. The moment a
file can carry several rulesets, this either needs prefixes or needs to move.

`settings` and `ext` are both opaque maps, split by scope and owner:

| | Holds | Keys | Written by |
|---|---|---|---|
| `settings` | Configuration for the file or one chart | Unprefixed | The ruleset |
| `ext` | Data attached to one item | `<namespace>.<name>` | Any plugin, the ruleset included |

Multiple rulesets per file is the obvious later extension: move `ruleset` from the file down to the
chart, under a new `format_version`.

---

## Time base

Musical position is an `i32` **tick** (`Tick`). Resolution is `ticks_per_beat`, a `NonZeroU16`
(`TicksPerBeat`) defaulting to **960**. A beat is a quarter note whatever the meter. StepMania,
`.chart` and MIDI all treat it that way, so `bpm` counts quarter notes too.

An "Nth note" spans `4/N` beats, so resolution `T` represents it exactly when `4T mod N == 0`:

| `ticks_per_beat` | StepMania 48 rows | `.chart` 192 ticks | ArrowVortex menu snaps | Custom snaps 4-192 exact |
|---|---|---|---|---|
| 48 | yes | **no** | yes | 11 / 189 |
| 192 | yes | yes | yes | 12 / 189 |
| 480 | yes | **no** | yes | 22 / 189 |
| **960** | **yes** | **yes** | **yes** | **22 / 189** |
| 2880 | yes | yes | yes | 30 / 189 |
| 3840 | yes | yes | yes | 22 / 189 |

960 is the smallest multiple of 192 that also represents quintuplets exactly, while covering
StepMania's 48 rows. Conversions are exact integer scaling: StepMania row × 20, and `.chart` tick
× 5 at its usual resolution of 192. Other resolutions cover more custom snaps: 2880 holds 30 of
the 189 values from 4 to 192; 3840 holds the same 22 as 960.

It stays configurable because **no finite resolution covers every snap**: 960 handles quintuplets
(a 20th note is 192 ticks) but not septuplets.

At 960, an `i32` tick reaches about 2.2 million beats either side of zero, over 600 hours at 60
BPM, and even the finest `u16` resolution leaves over 32,000 beats. Spans (`Ticks`) are `u32` and
[end on a tick](#well-formedness), so a tick plus a span is always a tick.

Negative ticks are legal, since charts have lead-ins and objects can precede beat 0. Source
positions that do not land on the grid are rounded to the nearest tick on import, ties to even,
with the original preserved in `ext`. The loss is then visible and reversible. This applies to any
source, not only millisecond-based ones.

Time is integers because two musically identical positions must compare equal, and computed floats
do not. Every other number is an exact [decimal](#numbers).

---

## Timing

```
Timing {
    anchors: Vec<Anchor>,               // at least one, rising in tick and seconds
    tempo:   Bpm,                       // tempo after the last anchor
    events:         Vec<TimingEvent>,
}

Anchor { tick: Tick, seconds: Origin }
TimingEvent { tick: Tick, kind: TimingKind, ext: Extensions }

TimingKind =
    Stop(Seconds)
  | Delay(Seconds)
  | Warp(Ticks)
  | TimeSignature(TimeSignature)            // written [numerator, denominator]
  | Other { name: Key, value: Value }

TimeSignature { numerator: NonZeroU16, denominator: NonZeroU16 }
```

### Anchors

Tempo is stored as authored beat-to-audio anchors. An anchor says that one tick falls at one exact
audio time in seconds. The tempo of a segment between two anchors is derived from the distance
between them after subtracting stops, delays and warped ticks. The `tempo` field is the explicit
tail tempo after the last anchor.

An anchor sits on a whole tick, so a typed tempo lands the next anchor up to half a tick from where
it would put it, and the derived rate differs from the typed one past the sixth place or so. Charter
shows a segment's tempo as the shortest decimal that lands its next anchor on the same tick, which
is the tempo typed, and keeps the exact rate for resolving time, so every anchor falls on its own
`seconds`.

Objects, events and markers sit on ticks and take their times from the tempo map, so changing the
tempo moves them in the audio. Keeping them where they sound is the editor's job. When a BPM is
set, charter stretches the timed ticks up to the next anchor by the change of tempo, then moves
everything after it by the ticks that anchor moved. Asked to keep notes on their beats, it does
only the second.

The editor still shows an "offset": it is the resolved time of tick 0. Changing it shifts every
anchor's `seconds` by the same amount.

`seconds` is an `Origin` rather than the `AudioTime` a preview uses, because chart time may fall
before the start of the file.

### Kinds

A kind is **interpreted** only if charter must compute with it:

| Kind | Effect |
|---|---|
| `stop` | Pauses time after the objects on its tick |
| `delay` | Pauses time before the objects on its tick |
| `warp` | Skips ticks without advancing time |
| `time_signature` | Sets the [measure grid](#measures) |

`stop` and `delay` differ only in where the objects on their own tick fall. That is a difference
in time, so charter needs both even though it never judges anything.

Everything else, such as scroll, speed, tick count, combo multipliers and fake regions, is
`other { name, value }`, carried faithfully and never interpreted. Its name is always prefixed by
the plugin that owns it. This is [gimmick coverage](research/topics/gimmick-coverage.md) resolved
by preserving everything and interpreting almost nothing.

### The starting state

- Every `Timing` has at least one anchor. With one anchor, ticks before it extrapolate at the tail
  `tempo`; with several anchors, ticks before the first extrapolate at the first segment's derived
  tempo.
- The meter is 4/4 until the first `time_signature`. A meter default is safe in a way a tempo
  default is not: it affects only the measure grid, and every surveyed format assumes the same
  value.

### Resolving time

The timing map is a non-decreasing function from tick to audio time. Anchors fix it at exact
beats:

- Between two anchors, time advances at the derived tempo for the timed ticks in that segment,
  kept exact rather than rounded to a stored BPM, so every tick before the next anchor falls before
  it and the anchor lands on its own time.
- After the last anchor, time advances at `tempo`.
- A `stop` pauses **after** the objects on its tick and a `delay` **before** them, so an object on
  a `stop` tick is hit as the pause begins and one on a `delay` tick as it ends. The time of a tick
  includes its own `delay` and excludes its own `stop`.
- A `warp` at tick `t` of length `w` covers ticks `[t, t + w)`. They take no time, and an object on
  one is skipped in play. A warped tick carrying a `stop` or `delay` is the exception: it still
  pauses and its objects are playable, which lets gimmick charts alternate stops and warps.
- Warps do not overlap. Importers merge overlapping ones the way StepMania does, keeping the
  furthest end.

This is StepMania's model, including the order of events on one tick, as implemented in
[TimingData.cpp](https://raw.githubusercontent.com/stepmania/stepmania/5_1-new/src/TimingData.cpp).

Tick to time is a function; time to tick is not. A `stop` or `delay` maps one tick to a range of
times, and a warp maps a range of ticks to one time. Converting time back returns a resolved
position rather than a tick, and the caller handles the paused and warped cases.

A `Sustain` records its span in ticks, so its length in seconds is whatever the map makes it: a
hold containing a two-second stop lasts two seconds longer, and one containing a warp lasts
shorter. An object on a warped tick is unreachable in play, and so is a sustain that ends on one.
Real files contain both, so they stay legal and are reported as warnings.

### Per-chart timing

`.sm` has one timing stream per song, shared by every difficulty. **`.ssc` lets an individual chart
carry its own**, overriding the song's.

It exists because difficulties disagree. A Challenge chart may add warps and stops as gimmicks its
Easy chart does not have, and a single difficulty is sometimes re-synced without touching the
others.

`timing` is therefore required on the song and optional on a chart, where it **replaces** the
song's entirely, offset included. The offset travels with the timing it belongs to, so a difficulty
re-synced on its own carries its own origin rather than inheriting one that no longer fits.

It replaces rather than merges because `.ssc` semantics are replacement, and merge rules would be
one more thing to specify and get wrong.

### Measures

A measure lasts `numerator * 4 / denominator` beats and must be a whole number of ticks, so
`numerator * 4 * ticks_per_beat` must divide evenly by `denominator`. Before the first
`time_signature`, a measure starts at every multiple of `4 * ticks_per_beat`, negative ones
included. Each `time_signature` restarts the grid at its own tick, cutting short any measure in
progress.

The format takes no position on what editing a meter change preserves, whether measure labels,
beat count or audio offsets. Each is expressible, so the choice belongs to the editor.

---

## Metadata

```
Metadata {
    title:   String,
    artist:  String,
    audio:   ResourcePath,
    preview: Option<Preview>,
    artwork: Option<ResourcePath>,
    genre:   Option<String>,
    ext:     Extensions,
}

Preview { start_seconds: AudioTime, length_seconds: Option<Seconds> }
```

Each field is one the core uses: `audio` to load the file, `title` and `artist` to identify it in a
browser, and `preview` to play an excerpt of it.

`preview` earns a field on the same evidence as `title`: it appears in `.sm` as `#SAMPLESTART` and
`#SAMPLELENGTH`, in `.osu` as `PreviewTime`, and in `.qua` as a preview time. Something present in
nearly every surveyed format is not format-specific. `length_seconds` is optional because some
formats give only a start point. Both fields are audio time, like anchor `seconds`, so a chart's
own timing does not move the preview.

`artwork` and `genre` earn theirs the same way: a song's picture is StepMania's `#JACKET`,
osu!'s background, Arcaea's jacket and Tap Tap Revenge's artwork, and a genre is StepMania's
`#GENRE` and Tap Tap Revenge's `kTTRGenre`. The artwork is a resource path, so it is held to the
same rules as the audio. Banners, backgrounds beyond the one picture, and credits stay in `ext`.
An open-ended metadata block is where formats accumulate twenty optional strings nobody reads.

---

## Samples

```
samples: Map<SampleId, ResourcePath>     // file-level bank
```

A `SampleId` is any non-empty text, and `Object.sample` names one. Keysounds could live in `ext`,
preserved on disk but unplayable, because the editor cannot audition a sound it has no generic way
to find.

So this is core by the same test as everything else: **charter must compute with it.** Loading and
scheduling audio is editor work. The ruleset decides what a keysound *means*; the editor still has
to play it.

A bank plus a reference matches the formats: `.sm` `#KEYSOUNDS` is a file-level list that notes
index into, and `.qua` carries per-object keysounds alongside custom audio samples. The bank also
deduplicates the common case of one sample used a thousand times.

Named sample *sets*, such as osu!'s normal, soft and drum banks, are not file paths and stay
ruleset semantics in `ext`.

Unlike `plugins`, the bank is **not** declared-if-and-only-if-used. An unreferenced sample is a
sound the author loaded but has not placed yet, which is an ordinary authoring state rather than a
stale dependency claim.

Multiple stems are **not** covered. Editor on Fire supports them, but no surveyed chart *format*
carries them, so they are project state rather than chart data.

---

## Markers

```
markers: Vec<Marker>     // song-wide, sorted by tick

Marker { tick: Tick, kind: MarkerKind, text: String }

MarkerKind = Section | Bookmark | Other(Key)   // "section", "bookmark", "psk.fever"
```

A marker names a point in the song. A `section` starts a part such as a verse or a drop, and a
`bookmark` is a place the author wants to find again. `text` is what the editor shows, and may be
empty. Any other kind is a [key](#identifiers) owned by the plugin of its namespace, as a note kind
is.

Markers are core because the editor must find them without the ruleset: it draws them on the
timeline and jumps between them. The surveyed formats carry them. `.chart` puts sections in its
`[Events]` track, StepMania writes `#LABELS`, and osu! keeps bookmarks in the beatmap's `[Editor]`
section.

A marker sits on a tick, like everything musical, so it keeps its place in the music when the tempo
changes. Markers are song-wide. `.ssc` and osu! can keep a different set per difficulty, and an
importer merges them into the song's list.

---

## Layers

A chart may list `layers` after its timing, ordered top to bottom. Each has a unique `name` (1–64
characters, without surrounding whitespace or control characters) and a `colour`: `red`,
`orange`, `yellow`, `green`, `teal`, `blue`, `purple` or `pink`. `view` is `shown` (the default),
`ghosted` or `hidden`; `locked` defaults to false. Shown, unlocked layers are editable; ghosted
layers are reference notes, and hidden layers are not drawn. Default `view` and `locked` are
omitted when writing.

An object may name its layer with `layer`. A note without `layer` is on the first layer.
Files without layers remain valid for other tools. When charter opens one, it gives each such
chart a shown, unlocked blue *Layer 0* and names that layer on every note. Opening is not an
edit; saving writes the new layer and note names.

A named layer not listed in the chart raises `UnknownLayer` at that object. Repeating a layer
name raises `DuplicateLayer` at the second layer. Children belong to their parent's layer and do
not name their own.

Layers change only the editor's view and editing permissions. The format and ruleset validate all
notes together, whatever their layer. Exporters merge the layers into the game's one chart.
See [the layers design](layers.md) for the editor behaviour.

---

## Objects

One type covers taps, holds, slides and arcs.

```
Object {
    tick:     Tick,
    position: Position,
    kind:     Kind,
    shape:    Shape,
    layer:    Option<LayerName>,   // first layer when absent
    sample:   Option<SampleId>,   // keysound, from the bank
    children: Vec<Child>,
    ext:      Extensions,
}
```

### Positions

```
Position =
    Lane  { index: LaneIndex, width: LaneCount }   // discrete columns, width defaults to 1
  | Plane { x: Decimal, y: Decimal }               // continuous playfield
```

`Lane` covers every VSRG. Lanes are numbered from 0, and a `Lane` position covers `width` of them
starting at `index`. `width` exists because lane-based games such as Project Sekai give notes a
span across several columns; games without widths leave it at 1.

`Plane` covers continuous playfields, where position is a point rather than a column. Its
coordinates are [decimals](#numbers) in units the ruleset defines. A continuous playfield has **no
snapping convention**, so nothing needs to prove two x values are the same position, but an exact
decimal still keeps an edited point from drifting.

`index` is an index, nothing more. There is no ten-lane ceiling; StepMania's is an artifact of
having ten digit keys, not of its data model.

**Multi-player charts are multi-lane charts.** StepMania's couple and routine modes put two players
on shared panels, with each note assigned to a player. Charter gives each player its own lanes and
lets the exporter fold them back onto shared panels, because player assignment is a ruleset
concern. It also keeps [lane uniqueness](#well-formedness) intact, which a shared-panel model would
break the moment both players hit the same panel together.

### Shapes

```
Shape =
    Instant
  | Sustain { duration: Ticks }
  | Path    { points: Vec<PathPoint> }   // non-empty

PathPoint { tick: Tick, position: Position, ease: Ease, ext: Extensions }
```

A path starts at the object's own `tick` and `position`, and `points`, which cannot be empty, are
the ones after it. Each point's `ease` shapes the segment arriving at it.

An enum rather than a duration field plus an optional point list, because that pairing lets an
object carry both and disagree with itself about where it ends. The enum makes that a compile
error instead of a validation rule.

`duration` is unsigned and non-zero, so `Sustain { 0 }` cannot restate `Instant` and a sustain
cannot end before it starts. Two spellings of one state is the same defect as a duration
contradicting a path, and the fix is the same: remove the spelling rather than validate against
it.

`Instant` and `Sustain` allocate nothing; only real paths pay for a vector.

A hold is `Sustain`, following ArrowVortex, which treats a `NOTE_STEP_OR_HOLD` with
`endrow == row` as a tap and `endrow > row` as a hold. Unterminated heads, orphan tails and
inverted spans stop being representable.

### Identifiers

```
Kind = Tap | Hold | Roll | Mine | Lift | Fake | Other(Key)   // "tap", "psk.flick"
Ease = Linear | Step | In | Out | InOut | Other(Key)         // "in_out", "arcaea.si"
Key  { namespace: Namespace, name: Name }                    // "psk.flick"
```

Note kinds, easings and [marker kinds](#markers) each have a closed shared vocabulary, and every
other value is a `Key` owned by the plugin of its namespace:

- kinds: `tap`, `hold`, `roll`, `mine`, `lift`, `fake`
- easings: `linear`, `step`, `in`, `out`, `in_out`
- marker kinds: `section`, `bookmark`

The vocabulary grows only with `format_version`. Rulesets should reuse it rather than reinvent it.

A `Key` is written `<namespace>.<name>` and split at the first `.` when read: the namespace
matches `[a-z][a-z0-9_]*` and the name is the non-empty rest, so a name may contain dots and a
namespace never does. Extension keys and `other` names are always keys.

Text exists only in the file. Code matches `Kind::Hold` instead of comparing strings, so a
misspelled `Kind::Hodl` fails to compile where `kind == "hodl"` compiled and never matched, and a
key's namespace is a field rather than something found again by splitting. `Other` holds a parsed
`Key`, so a plugin turns the keys in its own namespace into its own closed enum at its boundary and
fails loudly on anything else.

The enums fix spelling, not meaning, which stays with the ruleset. The core never evaluates a
curve, for instance: drawing a path and hit-testing it both require knowing how the playfield
projects, which is the plugin's job.

Kind and shape are independent. A roll and a hold share a shape and differ in kind.

### Children

An object may contain children. A `Child` has every field of an `Object` except `children` and
`layer`, so containment is one level deep by type and children inherit their parent's layer.
Children have no independent existence: deleting the parent deletes them.

Arcaea's `.aff` does the same, nesting arctaps inside the arc they sit on rather than giving them
IDs that point at it. Containment cannot dangle and needs no identity.

No surveyed game needs more than one level, the deepest being Arcaea's arctaps on an arc, and most
need none.

---

## Well-formedness

Deserialization checks each value on its own: the types, the [identifier grammar](#identifiers),
[resource paths](#resource-paths) and [numeric bounds](#numbers). One validation pass then checks
every rule relating values to each other: the rules below, the [namespace rules](#ownership), the
anchor requirement and ordering, [warp overlap](#resolving-time), anchor segments that must leave
time for at least one unwarped tick, and [measure lengths](#measures). No other code sees a
document that failed either.

Types remove most illegal states: a `Shape` cannot hold a duration and a path at once, a nonzero
`Ticks` stops `Sustain` restating `Instant`, and containment cannot dangle. What is left is stated
here rather than left to each implementation to guess, because an unstated rule is two
implementations disagreeing.

**Ordering and uniqueness**

- `objects`, timing `events` and `markers` are sorted by `tick`. Order among items on one tick
  carries no meaning, and writers preserve it. Sorted order makes diffs stable and lets range
  queries be a binary search, which is how Moonscraper queries the visible window.
- No two top-level objects of shared kinds on one tick cover a common lane. Two taps in one lane at
  one instant is universally invalid, and it is cheap to check. The rule stops there: two `Plane`
  points may legitimately coincide, and where children and objects of a ruleset's own kinds may sit
  is the ruleset's judgement. A Tap Tap Revenge shake, for one, is a gesture drawn in the column of
  its direction, which a tap in that column may share an instant with.
- At most one timing event per `tick` and kind; `other` events are keyed by `tick` and name. Import
  adapters resolve duplicates rather than passing them on.
- At most one marker per `tick` and kind, since two sections starting on one tick name one part
  twice.
- Chart names are unique, since the editor tells charts apart by name.
- Timing anchors strictly increase in both tick and audio time.

**References**

- Every `sample` is a key in `samples`.
- Every lane a position covers exists: `index + width <= lanes`. A chart with any `Lane` position
  therefore sets `lanes`.

**Spans**

- Every span ends on a tick: a sustain or warp at tick `t` with length `w` has
  `t + w <= i32::MAX`, so adding a span to a tick never leaves the type.
- Anchors do not sit inside warps, and the pauses and warps between two anchors leave positive
  time for the unwarped ticks between them.
- `Path` points are strictly increasing in `tick`, and the first is later than the object's own
  tick.
- All points of a `Path`, including the object's own position, are the **same** `Position` variant.
  An object cannot begin in a lane and end in the plane.
- A child's span lies within its parent's, both ends included. An arctap belongs to the arc it sits
  on, so one outside it is meaningless, and a Project Sekai slide can end in a flick on its final
  tick.
- **Overlapping sustains are not a core rule.** A sustain and another object overlapping in one lane
  is invalid in every VSRG surveyed, but that is a ruleset judgement, and a ruleset that wants it is
  not wrong. Core rules cover what is universally impossible, not what is usually wrong.

---

## Untrusted input

Chart files arrive from strangers, usually inside downloaded archives.

### Resource paths

`ResourcePath` values, `metadata.audio` and every entry in `samples`, are relative to the directory
containing the chart. An unconstrained path in a downloaded chart is an arbitrary-file-read
primitive, so the grammar is strict: `/` is the only separator, and every component is non-empty,
is neither `.` nor `..`, and contains no `\`, `:` or NUL. That rules out absolute paths, drive
letters, UNC prefixes and traversal by grammar alone, and gives each path one spelling on every
platform.

Opening a resource must not follow a link out of the chart's directory. The host must check this
each time a resource is opened, since links are filesystem state that can change after the
document is parsed.

**Implementation gap:** the current host does not enforce this filesystem containment. The path
grammar rejects textual traversal, but a symlink can still lead outside the song folder.
Validation of a chart file does not make its resource opens sandboxed.

### Limits

Readers cap document size and nesting depth, and reject a document over either rather than
allocate for it or recurse into it. Every count is bounded by the size. Core structure has a fixed
depth, since a `Child` has no children, so only opaque values can reach the depth cap. serde_json's
default recursion limit of 128 is one such cap.

### Numbers

The core schema stores no floats. A number is an integer or a **decimal of at most six places**,
held as a whole number of millionths, so `120.5` is `120_500_000`. Six places hold the sources
exactly apart from derived tempos: StepMania 5.1 writes timing segments with three decimals and
offsets with six, `.chart` stores tempo in thousandths of a BPM and anchors in microseconds, and
osu! and MIDI count time in whole milliseconds and microseconds.

A decimal is an ordinary JSON number within ±999,999,999.999999. That is at most fifteen significant
digits, which survive conversion to a double and back, so any [I-JSON](#serialization) reader keeps
the value. A reader rejects a seventh decimal place, since rounding on read would change a value on
a round trip. An importer rounds instead, to the nearest millionth with ties to even, and keeps the
original in `ext`, as it does for [off-grid ticks](#time-base).

Decimals keep edits exact: `0.1 + 0.2` is `0.3`, where a float gives `0.30000000000000004` and a
writer would store it.

osu! and MIDI store tempo as the length of a beat, in milliseconds and microseconds, and a BPM
derived from that rarely ends as a decimal. A fraction would hold it exactly, but a fraction is not
a JSON number, and the beat lengths are rounded already: osu!'s `342.857142857143` is 175 BPM,
which six places hold as `175.000000` and write back as the same beat length. One that does not
come back, such as `300.5`, keeps its original in `ext`.

Audio calculations use finite floats, but chart files store integer millionths. `ResolvedTime` is
a moment in the audio, `TimeShift` is the signed span between moments, and `BeatLength` is the
positive duration of one beat. None is stored in a chart file. Raw audio positions are checked
before entering editing commands or playback requests.

`tempo` must be positive. Negative BPM is a StepMania time-skipping gimmick predating explicit warps,
and charter has `warp` for that. Zero BPM would stop position advancing while time ran on, an
unbounded `stop`, and `stop` already expresses a pause. `stop` and `delay` must be positive too,
since a zero-length one is a second spelling of no event at all.

Importers handle all of these the way the source game does, which is what makes a converted chart
play the same. StepMania's
[loader](https://raw.githubusercontent.com/stepmania/stepmania/5_1-new/src/NotesLoaderSM.cpp)
turns negative BPMs and negative stops into warps and discards zero BPMs, zero stops and
non-positive delays, so the `.sm` adapter does the same and keeps the original values in `ext`.

### Units

Numbers that mean different things get different types, so a tempo cannot be passed where a pause
is expected. Each type keeps its number private and carries the operations the model needs, so
raw numbers appear only at the edges: where a document is read or written, and where a value is
shown.

| Type | Holds | Meaning | Operations |
|---|---|---|---|
| `Tick` | `i32` | A musical position | Adding `Ticks` gives a `Tick`; a later `Tick` is `Ticks` away; steps to the nearest line of a grid either way |
| `Ticks` | `NonZeroU32` | A length in ticks | |
| `TicksPerBeat` | `NonZeroU16` | The resolution of the time base | Gives the length of an Nth note when it is whole |
| `TimeSignature` | Two `NonZeroU16` | A meter | Tells whether its measure is a whole number of ticks |
| `Bpm` | `i64` millionths above 0 | Quarter notes per minute | Resolves `Ticks` at a `TicksPerBeat` to seconds |
| `BeatLength` | Finite `f64` above 0 | Duration of one quarter-note beat, never stored | Converts from BPM or milliseconds; gives seconds or BPM |
| `Seconds` | `i64` millionths above 0 | A length of time | Moves a `ResolvedTime` later |
| `AudioTime` | `i64` millionths, 0 or more | A point in the audio file, from its start | Converts to a `ResolvedTime` |
| `Origin` | `i64` millionths | The audio time of tick 0, negative before the file starts | Converts to a `ResolvedTime` |
| `ResolvedTime` | `f64` | An audio time resolved from a timing map, never stored | Compares; gives seconds for an audio API or a display |
| `TimeShift` | `f64` | A signed span between audio moments, never stored | Moves a `ResolvedTime` earlier or later |
| `LaneIndex` | `u16` | A lane, counting from 0 | Gives its number to a view |
| `LaneCount` | `NonZeroU16` | A number of lanes | Names its last and middle lanes; gives its number to a view |
| `LaneSpan` | Two `u32` | The lanes a lane position covers | Tells whether it fits a chart or overlaps another span |
| `Decimal` | `i64` millionths | A quantity the ruleset defines, such as a plane coordinate | |

---

## Extensibility

The test is whether games structurally unlike StepMania fit without a core change.

### The dividing line

**The core models structure. The plugin models semantics.**

Structure is what generic editing operates on: when an object happens, where it is, its geometry,
what it contains. Semantics is what a plugin renders and judges.

This matters because select, move, copy, paste, resnap, stretch and delete need only structure.
They work on a Project Sekai slide without knowing what a slide is.

### Transforms that need the plugin

A horizontal mirror must turn a left-flick into a right-flick, and nothing in the core knows that.

**Semantically meaningful transforms need plugin participation**, wherever the attribute is
stored. Promoting flick direction to a core field would not help, because the core still would not
know that mirroring inverts it. The format's job is to make structure addressable so a plugin hook
has something to operate on. That hook is the [plugin](research/topics/plugins.md) surface, still
undesigned.

### Project Sekai

| Feature | Representation |
|---|---|
| Tap | `Lane`, `Instant`, kind `tap` |
| Note spanning lanes | `Lane { width }` |
| Hold | `Lane`, `Sustain` |
| Flick | `Instant`, kind `psk.flick`, direction in `ext` |
| Trace / slide | `Path`, one point per waypoint |
| Slide changing lane | each `PathPoint` carries its own `Lane` |
| Visible or invisible relay point | `PathPoint.ext` |
| Slide ending in a flick | child `Instant`, kind `psk.flick`, on the end tick |
| Curved segment | `PathPoint.ease` |
| Critical (gold) | `ext` |

Critical is the instructive one. Any note type can be critical, so modelling it as a kind would
multiply the vocabulary.

### Arcaea

| Feature | Representation |
|---|---|
| Floor note | `Lane`, index 0 to 3 for `.aff` lanes 1 to 4, `Instant` |
| Floor hold | `Lane`, `Sustain` |
| Arc | `Plane`, `Path`, kind `arcaea.arc` |
| Arc easing (`s`, `b`, `si`, `so`, ...) | `PathPoint.ease`: `linear` for `s`, `arcaea.<name>` otherwise |
| Arctap on an arc | child object, `Instant` |
| Colour, trace-only, timing group | `ext` |

Both playfield geometries appear **in the same chart**, which Arcaea requires: floor notes are
lanes and arcs are not.

Timing groups, which give a set of notes their own scroll timing in both Arcaea and Quaver, stay in
`ext`, and so do Quaver's layers. Two games wanting grouping would normally argue for core
structure, but grouping here needs none, because it is **keyed by value**: objects sharing
`arcaea.timing_group: 1` are in a group because they carry the same value, and nothing points at
anything.

That is why no object carries an identity. Identity would only become necessary if something had
to reference **one specific object** rather than share a value with it, and nothing in the surveyed
games does.

---

## Extensions

Metadata, charts, timing events, objects, children and path points each carry:

```
Extensions = Map<Key, Value>
```

Keys are `<namespace>.<name>`, where the namespace is `ruleset.id` or a key in
[`plugins`](#ownership). The core preserves every entry [by value](#serialization).

`Value` is opaque to the core. The owning plugin deserializes it into its own types, which is
where validation belongs: the core cannot know that `arcaea.color` must be blue or red, and the
arcaea plugin cannot avoid knowing it.

The failure this prevents is real and shipped: **ArrowVortex** ignores keysounds on SM load while
its saver writes `#KEYSOUNDS`, so opening and saving a keysounded file silently loses them.
**Editor on Fire** does the opposite, retaining unknown MIDI tracks in a raw list and writing them
back. EOF's approach costs a map.

For import/export adapters, the intended loss-reporting contract is testable:

> A same-format round trip should preserve the format's data by value and produce an empty loss report.

Core JSON read/write preserves extensions by value. Game adapters do not yet satisfy the full
reporting requirement: TTR export, for example, drops layers without listing that loss.
See [export loss reporting](research/topics/export-loss-reporting.md).

Extensions do not make `.osu` able to hold a warp;
[structural incompatibility](research/formats/interoperability.md) is unaffected by anything
charter does.

`ext` holds data from outside charter or semantics owned by a plugin. Charter's own structural
concepts get fields.

---

## Serialization

**JSON for v0.** The weakest choice here.

For it: serde-native, universal tooling, unambiguous unlike YAML. Against it: no comments, and a
10,000-note chart runs to roughly a megabyte.

The file is [I-JSON](https://www.rfc-editor.org/rfc/rfc7493): UTF-8, no duplicate keys, and no
number beyond the range or precision of an IEEE 754 double, so an integer lies within
±(2^53 - 1). A reader rejects a duplicate key instead of picking a winner, because parsers disagree
on which one wins. The number rule reaches into opaque values too, so a plugin needing an exact
64-bit integer stores it as a string.

Readers parse each number to the nearest double. serde_json does so only with its
`float_roundtrip` feature; without it, about one full-precision value in ten comes back changed.

A round trip preserves the document by value: key order, whitespace and number spelling may
change, and values may not. Writers omit every field holding its default, such as an empty
collection, `None`, `width: 1` or `ticks_per_beat: 960`, so re-saving an unedited file adds
nothing to it.

| Alternative | For | Against |
|---|---|---|
| TOML | Excellent headers | Poor for ten thousand array elements |
| YAML | What `.qua` uses | Footguns; `serde_yaml` unmaintained |
| MessagePack | Compact, fast | Not human-readable or diffable |
| Header plus compact object lines | What `.sm` and `.chart` do | A hand-written parser |

The last is where this likely ends up if file size or diff noise becomes a real complaint, and is
not where a draft should start.

```json
{
  "format_version": 1,
  "ruleset": { "id": "arcaea", "version": "1.2.0" },
  "settings": { "sky_input": true },
  "plugins": {
    "swing": { "version": "0.2.0" },
    "osu":   { "version": "0.1.0" }
  },
  "metadata": {
    "title": "...", "artist": "...", "audio": "song.ogg",
    "preview": { "start_seconds": 42.5, "length_seconds": 15.0 }
  },
  "samples": {
    "clap": "keysounds/clap.wav",
    "kick": "keysounds/kick.wav"
  },
  "timing": {
    "anchors": [ { "tick": 0, "seconds": -0.014 } ],
    "tempo": 174.0,
    "events": [
      { "tick": 30720, "kind": { "stop": 0.5 } },
      { "tick": 61440, "kind": { "time_signature": [7, 8] } },
      { "tick": 61440, "kind": { "other": { "name": "swing.groove", "value": 0.62 } } }
    ]
  },
  "markers": [
    { "tick": 0,     "kind": "section", "text": "Intro" },
    { "tick": 61440, "kind": "section", "text": "Drop" }
  ],
  "charts": [
    {
      "name": "Future",
      "lanes": 4,
      "settings": { "rating": 10, "style": "touch" },
      "ext": { "osu.approach_rate": 9.2 },
      "objects": [
        { "tick": 0, "position": { "lane": { "index": 0 } },
          "kind": "tap", "shape": "instant", "sample": "kick" },
        { "tick": 240, "position": { "lane": { "index": 3 } },
          "kind": "hold", "shape": { "sustain": { "duration": 480 } } },
        { "tick": 960, "position": { "plane": { "x": 0.5, "y": 0.0 } },
          "kind": "arcaea.arc",
          "shape": { "path": { "points": [
            { "tick": 1920, "position": { "plane": { "x": 0.75, "y": 1.0 } },
              "ease": "arcaea.si" }
          ] } },
          "children": [
            { "tick": 1440, "position": { "plane": { "x": 0.62, "y": 0.5 } },
              "kind": "arcaea.arctap", "shape": "instant" }
          ],
          "ext": { "arcaea.color": "blue", "arcaea.timing_group": 1 }
        }
      ]
    },
    {
      "name": "Beyond",
      "lanes": 4,
      "settings": { "rating": 12 },
      "timing": {
        "anchors": [ { "tick": 0, "seconds": -0.014 } ],
        "tempo": 174.0,
        "events": [ { "tick": 15360, "kind": { "warp": 1920 } } ]
      }
    }
  ]
}
```

`ticks_per_beat` is absent, so it is 960: the `stop` sits on beat 32 and the 7/8 signature begins
on beat 64.

The first anchor's `seconds` of `-0.014` means tick 0 falls 14 milliseconds **before** the audio begins.

The file shows the things easiest to get wrong: both playfield geometries in one chart; a second
difficulty overriding the song timing, anchor included, with a warp the first does not have; a
keysound referencing the file-level bank; and a non-ruleset `swing` plugin contributing a timing
event. An `osu.` key also survives an import, declared because its data is present and for no
other reason.

---

## Known gaps

- osu! places objects in milliseconds, which do not land on ticks under variable tempo. Import
  [rounds them](#time-base), which is acceptable while osu! is an export target rather than a
  first-class one. Revisit only if osu! becomes primary.
- An object takes roughly 150 bytes, about 1.5 MB for a 10,000-note chart. Acceptable. If it stops
  being acceptable the fix is interning kinds and arena-allocating children, not a different model.
- Anchors are now the tempo model rather than editor-only project data. They keep authored times
  exact and avoid drift from cumulative BPM rounding.
