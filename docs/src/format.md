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
must compute with it. Everything else is carried verbatim and belongs to a ruleset plugin.

The corollary matters as much as the rule: **the format is deliberately weakly typed, and the
plugin boundary is where type safety lives.** The core stores an extension value as opaque data;
the plugin that owns it deserializes it into its own types and is free to be as strict as it
likes. Pushing the strictness outward is what lets the core stay small without the whole system
becoming loose.

## What this commits to

| Decision | Position | Confidence |
|---|---|---|
| [Row resolution](research/topics/row-resolution.md) | 960 ticks/beat, configurable | High, derived arithmetically |
| [Musical position](research/topics/musical-position.md) | Integer ticks in time, floats only in space | High |
| [Note type model](research/topics/note-type-model.md) | Structure in core, semantics in plugin | Medium |
| Ruleset scope | Exactly one per file, structurally; other plugins unrestricted | Medium |
| [Internal model richness](research/topics/internal-model-richness.md) | Five interpreted timing kinds, rest carried | Medium |
| [Gimmick coverage](research/topics/gimmick-coverage.md) | Everything representable, little interpreted | Medium |
| [Round-trip fidelity](research/topics/round-trip-fidelity.md) | Unknown data preserved verbatim | High |
| [Domain model purity](research/topics/domain-model-purity.md) | No editor state in chart data | High |
| [Meter changes](research/topics/meter-changes.md) | Meter is timing data, policy left to the editor | Medium |
| Serialization | JSON | **Low, most likely to change** |

Deferred: [target audience](research/topics/target-audience.md),
[primary format](research/topics/primary-format.md),
[guitar format support](research/topics/guitar-format-support.md), and the
[plugin](research/topics/plugins.md) loading mechanism. Plugin *identity* is defined here; loading
is not.

---

## Versioning

Three independent questions, deliberately kept apart because they have different answers and
different error paths.

### Can I parse this?

```
format_version: u32
```

One integer, incremented **only on a breaking change**. A reader seeing a higher version than it
knows must refuse rather than guess.

Additive changes do not bump it, which is safe only because of one rule:

> **Parsers must preserve keys they do not recognise**, round-tripping them untouched.

Without that rule an old reader silently drops new data, so every addition would have to be
breaking. With it, an old reader opening a newer file keeps everything it cannot use.

### Who wrote this?

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

The ruleset sits in its own field rather than in the map, so **a file cannot name two rulesets**.
That is a shape, not a rule a validator has to enforce. Everything else — a timing plugin, an
import adapter that left data behind — goes in `plugins`, where any number may appear.

A namespace identifies its owner everywhere a plugin can leave data, always as
`<namespace>.<name>`: extension keys, note kinds, easings, and timing `other` names. Unprefixed
names belong to the shared vocabulary and to no plugin.

Two rules bind them together:

- Every namespace used anywhere in the file is either `ruleset.id` or a key in `plugins`
- Every key in `plugins` is **actually used** somewhere in the file

The second direction matters as much as the first. Declarations track contributors, so a plugin
that left nothing behind has no business being listed, and a stale entry is a lie about what the
file depends on. `ruleset` is exempt: it defines the chart's meaning whether or not it wrote a
single extension key.

Together this is what makes data mappable back to its owner. Without it, `ext` is a bag of strings
whose meaning depends on where the file came from, which is unrecoverable once the file moves.

### Can I interpret it correctly?

The recorded version answers this, and it is the reason a bare namespace is not enough. A plugin
that changed the meaning or shape of its own extension data between `1.2` and `2.0` needs to know
which one it is reading before deserializing. Recording the version lets the plugin migrate, warn,
or refuse, and lets it do so **per namespace** rather than failing the whole file.

A reader missing a plugin entirely, or holding an incompatible version, can still parse and
round-trip the file. It simply cannot render or meaningfully edit that ruleset. That is a missing
plugin, not a corrupt file, and the two should not share an error path.

---

## One ruleset per file

```
ruleset:  { id: Namespace, version: String }
settings: Value        // opaque, owned by that ruleset
```

A file declares **one** ruleset, at the top level, in a field that cannot hold two. Its `charts`
are difficulties of that same ruleset, not independent charts that happen to share an audio file.

Non-ruleset plugins are unrestricted. A file may pull in a timing plugin, an analysis plugin and an
import adapter at once, because those extend a chart rather than define what it is. Only the
ruleset is exclusive.

**Ruleset means the game, not the layout.** `sm` is a ruleset; dance-single and dance-double are
not. A real `.sm` file routinely holds both a single and a double chart, so making layout the
ruleset would make ordinary files unrepresentable under this rule.

Layout therefore stays per chart:

```
Chart {
    name:     String,           // "Challenge", "Future", "Expert+"
    lanes:    Option<u16>,
    settings: Value,            // opaque, owned by the ruleset
    timing:   Option<Timing>,   // replaces the song's when present
    objects:  Vec<Object>,
    ext:      Extensions,
}
```

`name` is core only because the editor must list charts and tell them apart; ArrowVortex's chart
list is the reference. The numeric **rating is not core** and lives in `settings`, because rating
scales are ruleset-specific — Arcaea, StepMania and osu! do not agree on range or meaning, and a
shared `meter: u8` would be a number the core cannot interpret, compare or validate.

`lanes` is structural rather than semantic, so it is a field. The plugin knows dance-single has
four lanes, but **an editor missing that plugin does not**, and lane count is exactly what it needs
to draw a generic notefield and to check that `Lane.index` is in range. It is optional because a
purely `Plane`-based ruleset has no lanes. The style *name* is the plugin's business and lives in
`ext`.

### Ruleset settings

`settings` is configuration the ruleset owns: opaque to the core, deserialized by the plugin into
its own types, and validated there. The core cannot know that an approach rate is bounded or that
a scroll speed must be positive; the plugin cannot avoid knowing.

It appears in two scopes, and they do **not** merge:

- On the file, for configuration that applies to the whole ruleset
- On a chart, for configuration that differs per difficulty, which is common — osu!'s difficulty
  settings and Quaver's per-map scroll behaviour both live at this level

No merging, for the same reason per-chart timing replaces rather than merges: merge rules are one
more thing to specify and get wrong. The plugin reads two independent objects and decides what
they mean.

**`settings` needs no namespace prefix**, and that is a direct payoff of one ruleset per file.
Ownership is unambiguous, so the keys inside it are the plugin's own flat vocabulary. The moment a
file can carry several rulesets, this either needs prefixes or needs to move.

The distinction from `ext` is worth stating, since both are opaque bags:

| | Purpose | Written by |
|---|---|---|
| `settings` | Deliberate configuration the ruleset owns | The ruleset plugin |
| `ext` | Data charter could not map, kept so it is not lost | Import adapters, other plugins |

Multiple rulesets per file is the obvious later extension: move `ruleset` from the file down to
the chart. That is a breaking change, hence the version field.

---

## Time base

Musical position is a signed integer **tick**. Resolution is `ticks_per_beat`, defaulting to
**960**.

An "Nth note" spans `4/N` beats, so resolution `T` represents it exactly when `4T mod N == 0`:

| `ticks_per_beat` | StepMania 48 rows | `.chart` 192 ticks | ArrowVortex menu snaps | Custom snaps 4-192 exact |
|---|---|---|---|---|
| 48 | yes | **no** | yes | 11 / 189 |
| 192 | yes | yes | yes | 12 / 189 |
| 480 | yes | **no** | yes | 22 / 189 |
| **960** | **yes** | **yes** | **yes** | **22 / 189** |
| 3840 | yes | yes | yes | 22 / 189 |

960 is the smallest value representing both source formats exactly while reaching the practical
ceiling on snap coverage; 3840 buys nothing. Conversions are exact integer scaling: StepMania row
× 20, `.chart` tick × 5.

It stays configurable because **no finite resolution covers every snap** — 960 handles quintuplets
(a 20th note is 192 ticks) but not septuplets. A knob, not indecision.

Time is integers because two musically identical positions must compare equal, and computed floats
do not. Space is a different axis; see [positions](#positions).

---

## Timing

```
TimingEvent { tick: i64, kind: TimingKind, ext: Extensions }
```

A kind is **interpreted** only if charter must compute with it. That leaves five:

| Kind | Payload | Why it is core |
|---|---|---|
| `bpm` | beats per minute | Sets the tick to time rate |
| `stop` | seconds | Time advances, position does not |
| `delay` | seconds | Same as `stop` for time, differs only in judgement |
| `warp` | ticks | Position jumps, time does not advance |
| `time_signature` | numerator, denominator | Measure structure, which drives the grid |

The first four define the tick to time mapping. The fifth defines the measure grid. Nothing else
does either.

`delay` earns its place despite charter never judging anything: it pauses time exactly as `stop`
does, so omitting it would corrupt the mapping.

Everything else — scroll, speed, tick count, combo multipliers, labels, fake regions — is
`other { name, value }`. Carried faithfully, never interpreted. This is
[gimmick coverage](research/topics/gimmick-coverage.md) resolved by preserving everything and
interpreting almost nothing.

### Time is not invertible

- **stop** and **delay** map one tick to a range of times.
- **warp** maps a range of ticks to one time. Ticks inside a warp have **no** time at all.

Tick to time is a function; time to tick is not. Conversion returns a resolved position, not a
tick, and the caller handles the warped case. Notes inside warps stay representable — real files
contain them, so a validator should report them rather than the format forbidding them.

### Per-chart timing

`.sm` has one timing stream per song, shared by every difficulty. **`.ssc` lets an individual chart
carry its own**, overriding the song's.

It exists because difficulties genuinely disagree. A Challenge chart may add warps and stops as
gimmicks its Easy chart does not have, and a single difficulty is sometimes re-synced without
touching the others.

So timing appears in two places:

- `timing` on the song, required
- `timing` on a chart, optional — when present it **replaces** the song's entirely

Replacement rather than merge, because `.ssc` semantics are replacement, and merge rules would be
one more thing to specify and get wrong.

### Meter changes

`time_signature` is just another event. The format takes **no position** on what a meter change
preserves — measure labels, beat count, or audio offsets — because all three are expressible and
the choice is editing behaviour, not representation.

---

## Metadata

```
Metadata { title: String, artist: String, audio: Path, ext: Extensions }
```

Three fields, because three are what the core actually uses: `audio` to load the file, `title` and
`artist` to identify it in a chart browser. Banners, backgrounds, credits, preview windows and
genre tags are all real, all format-specific, and all go in `ext`.

An open-ended metadata block is where formats accumulate twenty optional strings nobody reads.

---

## Objects

One type covers taps, holds, slides and arcs.

```
Object {
    tick:     i64,
    position: Position,
    kind:     Kind,
    shape:    Shape,
    children: Vec<Object>,
    ext:      Extensions,
}
```

### Positions

```
Position =
    Lane  { index: u16, width: u16 }   // discrete columns, width defaults to 1
  | Plane { x: f64, y: f64 }           // continuous playfield
```

`Lane` covers every VSRG. `width` exists because lane-based games such as Project Sekai give notes
a span across several columns; games without widths leave it at 1.

`Plane` covers continuous playfields, where position is a point rather than a column.

Floats are right here and wrong in time. A continuous playfield has **no snapping convention**, so
nothing ever needs to prove two x values are the same position. Ticks do.

`index` is an index, nothing more. There is no ten-lane ceiling; StepMania's is an artifact of
having ten digit keys, not of its data model.

### Shapes

```
Shape =
    Instant
  | Sustain { duration: NonZeroI64 }
  | Path    { points: Vec<PathPoint> }   // non-empty

PathPoint { tick: i64, position: Position, ease: Ident }
```

An enum rather than a duration field plus an optional point list, because that pairing lets an
object carry both and disagree with itself about where it ends. Three variants make that a compile
error instead of a validation rule.

`duration` is non-zero so that `Sustain { 0 }` cannot restate `Instant`. Two spellings of one
state is the same defect as a duration contradicting a path, and the fix is the same: remove the
spelling rather than validate against it.

One invariant resists this treatment: a `Path` must be non-empty, and Rust has no cheap non-empty
vector. It stays a validation rule, and it is the only one in the object model.

`Instant` and `Sustain` allocate nothing; only real paths pay for a vector.

A hold is `Sustain`, following ArrowVortex, which treats a `NOTE_STEP_OR_HOLD` with
`endrow == row` as a tap and `endrow > row` as a hold. Unterminated heads, orphan tails and
inverted spans stop being representable.

### Identifiers

```
Ident(String)    // "tap", "linear", "psk.flick", "arcaea.si"
```

One type covers both note kinds and easings, **because the core never branches on either**. An
enum would privilege one game's vocabulary — StepMania's — over every other, then hand everything
else to a catch-all variant anyway. An enum whose last variant is `Other(String)` is a string with
extra steps.

Easing was the clearest case. The core never needs to evaluate a curve: drawing a path and
hit-testing it both require knowing how the playfield projects, which is the plugin's job.

Unprefixed names are shared vocabulary that rulesets should reuse rather than reinvent:

- kinds: `tap`, `hold`, `roll`, `mine`, `lift`, `fake`
- easings: `linear`, `step`, `in`, `out`, `in_out`

A prefixed name belongs to the plugin of that namespace, exactly as extension keys do.

The plugin is where this becomes type-safe again. Its own vocabulary *is* closed, so it parses an
`Ident` into a real enum at its boundary and fails loudly on anything else. The looseness is
confined to the wire.

Kind and shape are independent. A roll and a hold share a shape and differ in kind.

### Children

An object may contain objects. Structural containment, not a reference: children have no
independent existence, and deleting the parent deletes them.

Arcaea validates this — its `.aff` nests arctaps **inside** the arc they sit on rather than giving
them IDs pointing at one. Containment cannot dangle and needs no identity.

---

## Extensibility

The test is whether games structurally unlike StepMania fit without a core change.

### The dividing line

**The core models structure. The plugin models semantics.**

Structure is what generic editing operates on: when an object happens, where it is, its geometry,
what it contains. Semantics is what a plugin renders and judges.

This is load-bearing because select, move, copy, paste, resnap, stretch and delete need only
structure. They work on a Project Sekai slide without knowing what a slide is.

### Where it stops working

A horizontal mirror must turn a left-flick into a right-flick, and nothing in the core knows that.

**Semantically meaningful transforms need plugin participation**, regardless of where the attribute
is stored — promoting flick direction to a core field would not help, because the core still would
not know that mirroring inverts it. The format's job is to make structure addressable so a plugin
hook has something to operate on. That hook is the [plugin](research/topics/plugins.md) surface,
still undesigned.

### Project Sekai

| Feature | Representation |
|---|---|
| Tap | `Lane`, `Instant`, kind `tap` |
| Note spanning lanes | `Lane { width }` |
| Hold | `Lane`, `Sustain` |
| Flick | `Instant`, kind `psk.flick`, direction in `ext` |
| Trace / slide | `Path`, one point per waypoint |
| Slide changing lane | each `PathPoint` carries its own `Lane` |
| Slide ending in a flick | last `PathPoint`, or a child at the end tick |
| Curved segment | `PathPoint.ease` |
| Critical (gold) | `ext` |

Critical is the instructive one: it is orthogonal to note type — any type can be critical — so
modelling it as a kind would multiply the vocabulary.

### Arcaea

| Feature | Representation |
|---|---|
| Floor note | `Lane { index: 1..4 }`, `Instant` |
| Floor hold | `Lane`, `Sustain` |
| Arc | `Plane`, `Path`, kind `arcaea.arc` |
| Arc easing (`s`, `b`, `si`, `so`) | `PathPoint.ease`, `Named` where needed |
| Arctap on an arc | child object, `Instant` |
| Colour, trace-only, arc group | `ext` |

Both playfield geometries appear **in the same chart**, which Arcaea requires: floor notes are
lanes and arcs are not.

Arc *groups* — sibling arcs traced as one motion — stay in `ext`. They are a sibling relation,
which containment does not express, and one game wanting them is not enough to justify core
structure.

---

## Extensions

Every object, chart and timing event carries:

```
Extensions = Map<String, Value>
```

Keys are `<namespace>.<key>`, where the namespace is a plugin declared in
[`plugins`](#who-wrote-these-extensions). Unknown keys are preserved verbatim.

`Value` is opaque to the core. The owning plugin deserializes it into its own types, which is
where validation belongs: the core cannot know that `arcaea.color` must be blue or red, and the
arcaea plugin cannot avoid knowing it.

The failure this prevents is real and shipped: **ArrowVortex** ignores keysounds on SM load while
its saver writes `#KEYSOUNDS`, so opening and saving a keysounded file silently loses them.
**Editor on Fire** does the opposite, retaining unknown MIDI tracks in a raw list and writing them
back. EOF's approach costs a map.

It also makes loss reporting mechanical, which gives a testable property:

> A same-format round trip produces an empty loss report.

That is a test, not a policy. See [export loss reporting](research/topics/export-loss-reporting.md).

Extensions do not make `.osu` able to hold a warp;
[structural incompatibility](research/formats/interoperability.md) is unaffected by anything
charter does.

The rule that stops `ext` becoming a dumping ground: **`ext` is for data originating outside
charter, or semantics belonging to a plugin. Charter's own structural concepts get fields.**

---

## Serialization

**JSON for v0.** The weakest choice here.

For it: serde-native, universal tooling, unambiguous unlike YAML. Against it: no comments, and a
10,000-note chart runs to roughly a megabyte.

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
  "format_version": 0,
  "ticks_per_beat": 960,
  "ruleset": { "id": "arcaea", "version": "1.2.0" },
  "settings": { "base_bpm": 174.0, "audio_offset_ms": -14 },
  "plugins": {
    "swing": { "version": "0.2.0" },
    "osu":   { "version": "0.1.0" }
  },
  "metadata": { "title": "...", "artist": "...", "audio": "song.ogg" },
  "timing": [
    { "tick": 0,     "kind": { "bpm": 174.0 } },
    { "tick": 30720, "kind": { "stop": 0.5 } },
    { "tick": 61440, "kind": { "time_signature": [7, 8] } },
    { "tick": 61440, "kind": { "other": { "name": "swing.groove", "value": 0.62 } } }
  ],
  "charts": [
    {
      "name": "Future",
      "lanes": 4,
      "settings": { "rating": 10, "style": "touch", "sky_input": true },
      "ext": { "osu.approach_rate": 9.2 },
      "objects": [
        { "tick": 0, "position": { "lane": { "index": 0 } },
          "kind": "tap", "shape": "instant" },
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
          "ext": { "arcaea.color": "blue", "arcaea.group": 1 }
        }
      ]
    },
    {
      "name": "Beyond",
      "lanes": 4,
      "settings": { "rating": 12 },
      "timing": [ { "tick": 0, "kind": { "bpm": 174.0 } },
                  { "tick": 15360, "kind": { "warp": 1920 } } ],
      "objects": []
    }
  ]
}
```

At 960 ticks per beat the `stop` sits on beat 32 and the 7/8 signature begins on beat 64.

The file shows the four things easiest to get wrong: both playfield geometries in one chart, which
Arcaea requires; a second difficulty overriding the song timing with a warp the first does not
have; a non-ruleset `swing` plugin contributing a timing event alongside the ruleset; and an `osu.`
key surviving an import, declared because its data is present and for no other reason.

---

## Known gaps

- **Time-native positions.** osu! places objects in milliseconds, which do not land on ticks under
  variable tempo. Not a problem while osu! is an export target rather than a first-class one:
  import rounds to the nearest tick and keeps the original in `ext`, making the loss visible and
  reversible. Revisit only if osu! becomes primary.
- **Object size.** Roughly 120-150 bytes per object, about 1.5 MB for a 10,000-note chart.
  Acceptable. If it stops being acceptable the fix is interning kinds and arena-allocating
  children, not a different model.
- **Sync anchors are not in v0.** Tempora pairs an audio offset with a musical position and derives
  tempo from it, which is a better *authoring* model than typing BPM values. Excluded because it
  duplicates information already in `bpm` events and raises questions this draft cannot answer —
  what an anchor inside a warp means, what editing a derived BPM does to the anchor that produced
  it. If it returns it should be editor project data, not chart data. See
  [sync anchors](research/topics/sync-anchors.md).
- **Plugin version semantics are unspecified.** The field records a version string; nothing yet
  says whether comparison is semver, what a plugin must do on a mismatch, or how migrations are
  expressed.
