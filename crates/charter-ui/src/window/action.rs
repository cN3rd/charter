//! What a panel, key or gesture asks the editor to do, and the notes a lane key places.

#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "window submodules share one private implementation vocabulary"
    )
)]
use super::*;

/// A note a lane key places.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Note {
    /// A note that starts and ends at its tick.
    Tap,
    /// A mine placed at its tick rather than a playable note.
    Mine,
}

impl Note {
    /// The chart's note kind to store for this lane-key choice.
    pub(super) const fn kind(self) -> Kind {
        match self {
            Self::Tap => Kind::Tap,
            Self::Mine => Kind::Mine,
        }
    }
}

/// Something a panel, key or gesture asks the editor to do.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Action {
    /// Make a layer above the active one.
    AddLayer,
    /// Choose the layer at this row.
    SetActiveLayer(LayerIndex),
    /// Choose the adjacent row.
    StepLayer(Step),
    /// Move the selected notes onto this row.
    MoveSelectionToLayer(LayerIndex),
    /// Select the notes on this row.
    SelectLayer(LayerIndex),
    /// Set the view of this row.
    SetLayerView(LayerIndex, charter_core::LayerView),
    /// Solo this row, or restore the preceding views.
    SoloLayer(LayerIndex),
    /// Set the lock on this row.
    SetLayerLocked(LayerIndex, bool),
    /// Set the palette colour of this row.
    RecolourLayer(LayerIndex, charter_core::LayerColour),
    /// Give this row a new name.
    RenameLayer(LayerIndex, String),
    /// Drag a row to a new position.
    MoveLayer {
        /// Row moved.
        from: LayerIndex,
        /// Row it moves to.
        to: LayerIndex,
    },
    /// Merge the active row into the next row.
    MergeActiveLayer,
    /// Merge this row into the next row.
    MergeLayer(LayerIndex),
    /// Remove the row, either discarding or moving its notes.
    DeleteLayer(LayerIndex, charter_editor::LayerNotes),
    /// Copy this row into another chart.
    CopyLayerToChart {
        /// Row whose notes are copied.
        layer: LayerIndex,
        /// Chart receiving the copied notes.
        chart: ChartIndex,
    },
    /// Show or hide the active row.
    ToggleActiveLayerView,
    /// Switch between note-kind and layer-colour tinting.
    ToggleLayerColours,
    /// Replace the open song with the built-in demo.
    OpenDemo,
    /// Show the chart at this index in the open song.
    OpenChart(ChartIndex),
    /// Move the cursor by a snap in the given direction.
    Move(Step),
    /// Move the cursor by a measure in the given direction.
    MoveMeasure(Step),
    /// Increase the snap's denominator to reach finer lines.
    FinerSnap,
    /// Decrease the snap's denominator to reach coarser lines.
    CoarserSnap,
    /// Multiply the notefield zoom by this factor.
    ZoomBy(f32),
    /// Places a note on the lane at the cursor, or removes what starts there.
    Toggle(LaneIndex, Note),
    /// The lane key of a lane that toggled came up.
    LaneUp(LaneIndex),
    /// Every lane key that toggled is up again.
    LanesReleased,
    /// Places a note at `tick` on `lane`, or removes what starts there.
    Place {
        /// Position along the chart where the note starts.
        tick: Tick,
        /// Lane on which the note is placed.
        lane: LaneIndex,
        /// Kind of note placed on that lane.
        note: Note,
    },
    /// Makes the tap or hold starting on `lane` at `from` end at `to`.
    Stretch {
        /// Lane whose tap or hold is stretched.
        lane: LaneIndex,
        /// Tick where the note starts.
        from: Tick,
        /// Tick where the note ends.
        to: Tick,
    },
    /// Meets the selection with a note, or with none: nothing was under the pointer.
    Select(Option<NoteAt>, Selecting),
    /// Moves the selected notes this many ticks later and lanes right, earlier and left when
    /// negative.
    MoveSelection {
        /// Signed distance to move along the chart, in ticks.
        ticks: TickOffset,
        /// Signed distance to move across lanes.
        lanes: LaneOffset,
    },
    /// Meets the selection with the notes starting inside this tick and lane rectangle.
    SelectBetween {
        /// First tick covered by the selection.
        from_tick: Tick,
        /// Last tick covered by the selection.
        to_tick: Tick,
        /// First lane covered by the selection.
        from_lane: LaneIndex,
        /// Last lane covered by the selection.
        to_lane: LaneIndex,
        /// Whether to replace, add to or remove from the existing selection.
        how: Selecting,
    },
    /// Revert the latest editing step.
    Undo,
    /// Reapply the latest reverted editing step.
    Redo,
    /// Select every note in the open chart.
    SelectAll,
    /// Leave no notes selected.
    ClearSelection,
    /// Remove the selected notes from the open chart.
    DeleteSelection,
    /// Moves the end of the selected notes, or those at the cursor, a snap later or earlier.
    ResizeNotes(Step),
    /// Copies the selected notes.
    Copy,
    /// Copies the selected notes and removes them.
    Cut,
    /// Pastes the copied notes at the cursor: those the clipboard's text holds, when it holds
    /// some, and otherwise those copied last.
    Paste(Option<String>),
    /// Copies the selected notes to just after themselves.
    Duplicate,
    /// Moves the selected notes this many snap lines later and lanes right, back and left when
    /// negative.
    ShiftSelection {
        /// Signed number of snap lines to move the notes.
        snaps: i32,
        /// Signed number of lanes to move the notes.
        lanes: LaneOffset,
    },
    /// Flips the selected notes across the lanes.
    Mirror,
    /// Moves the selected notes to the snap's nearest lines.
    Quantize,
    /// Makes the selected notes what the song's game makes them by the change named.
    ChangeNotes(charter_core::ChangeIndex),
    /// Places taps from the cursor on, one `per_beat`th of a beat apart, on the lanes `steps`
    /// counts from `lane`, wrapping past the last.
    Pattern {
        /// Lane offsets of the pattern, wrapping at the last lane.
        steps: &'static [u16],
        /// Number of pattern positions per beat.
        per_beat: u16,
        /// First lane of the pattern.
        lane: LaneIndex,
    },
    /// Selects every note on the lane.
    SelectLane(LaneIndex),
    /// Moves the cursor to the tick and then does the action there, for what a menu opened on
    /// that tick places.
    Here(Tick, Box<Self>),
    /// Pastes what the system clipboard holds at the tick, or with no text there the notes
    /// copied last.
    PasteHere(Tick),
    /// Asks the host for a song to open.
    Open,
    /// Opens a song opened before.
    OpenRecent(PathBuf),
    /// Forgets the songs opened before.
    ClearRecent,
    /// Saves the song to its file, or asks where when it has none.
    Save,
    /// Asks where to save the song.
    SaveAs,
    /// Shows or hides the settings window.
    Settings,
    /// Opens the latency calibration.
    Calibrate,
    /// Shows the dialog that starts a song.
    NewSong,
    /// Plays the audio from the cursor, or pauses it.
    TogglePlay,
    /// Asks the host to let the user choose the song's audio.
    ChooseAudio,
    /// Asks the host to let the user choose a picture for the song's artwork.
    ChooseArtwork,
    /// Makes the audio play this much faster, or slower when negative.
    NudgeRate(f64),
    /// Makes the audio play at this factor of its speed.
    SetRate(f64),
    /// Apply the user's playback volume settings.
    SetVolumes(charter_audio::Volumes),
    /// Gives the timing point on the tick this tempo.
    SetBpmAt(Tick, charter_core::Bpm),
    /// Gives the song this title, artist, audio and preview.
    SetMetadata(Box<charter_core::Metadata>),
    /// Sets a text setting of the song or the open chart, or takes it away without a value.
    SetSetting {
        /// Whether the setting belongs to the song or its open chart.
        scope: charter_core::SettingScope,
        /// The game's key for the text setting.
        key: String,
        /// Replacement text, or none to remove the setting.
        value: Option<String>,
    },
    /// Moves the tempo point on the tick to this audio moment, the first one taking the grid along.
    MovePointTo(Tick, ResolvedTime),
    /// Gives the timing point on the tick this meter.
    SetMeterAt(Tick, charter_core::TimeSignature),
    /// Removes the timing point on the tick.
    RemoveTimingAt(Tick),
    /// Puts tick 0 of the open timing this far into the audio.
    SetOffset(charter_core::Origin),
    /// Starts a BPM change at the cursor, at the tempo in effect there.
    AddBpm,
    /// Moves the offset by this many seconds, later in the audio when positive.
    NudgeOffset(f64),
    /// Show the timing window for editing audio anchors.
    ShowSync,
    /// Show the dialog for reading another program's timing.
    ShowImport,
    /// Ask the host to choose a file to import timing from.
    ChooseTimingFile,
    /// Puts this timing in place of the one tempo edits go to.
    ImportTiming(Box<charter_core::Timing>),
    /// Puts the cursor on this tick.
    JumpTo(Tick),
    /// Names the section at the cursor this, starting one there if none does.
    SetSection(String),
    /// Removes the song's marker with this key.
    RemoveMarker(MarkerKey),
    /// Puts the cursor where the open chart's last note ends.
    JumpEnd,
    /// An edit the timing window asks for.
    Timing(charter_timing::Edit),
    /// Play from this audio moment with the metronome on, to hear an edit.
    Audition(ResolvedTime),
    /// Play the settings' snippet of audio before the cursor, or after it, leaving the cursor be.
    PlaySnippet(Step),
    /// Ask whether to reset the open chart's timing.
    AskResetTiming,
    /// Put 120 BPM in 4/4 from 0 ms in place of the open chart's timing.
    ResetTiming,
    /// Loop the ticks from `from` to `to` with the metronome, or stop looping them.
    Loop {
        /// Tick at which the playback loop starts.
        from: Tick,
        /// Tick at which the playback loop ends.
        to: Tick,
    },
    /// Switch between the usual playback speed and the alternate one.
    ToggleRate,
    /// Mute the metronome, or unmute it, while a key is held.
    MuteMetronome(bool),
    /// Show or hide the timing window.
    ToggleSync,
    /// Moves every timing event at `from` to `to`, as one timing point.
    MoveTiming {
        /// Tick holding the events to move.
        from: Tick,
        /// Tick to which those events move.
        to: Tick,
    },
    /// Moves the song's marker with `key` to `to`.
    MoveMarker {
        /// Marker to move.
        key: MarkerKey,
        /// Tick at which to put the marker.
        to: Tick,
    },
    /// Puts the cursor on the nearest section that way.
    JumpSection(Step),
    /// Adds a bookmark at the cursor, or removes the one there.
    ToggleBookmark,
    /// Raises the tempo at the cursor by this many BPM, or lowers it when negative.
    NudgeBpm(f64),
    /// Shows or hides the keyboard shortcuts.
    Help,
    /// Shows or hides the About window.
    About,
    /// Shows or hides the song panel.
    ToggleSongPanel,
    /// Turns the metronome on or off.
    ToggleMetronome,
    /// Turns the tick on each note on or off.
    ToggleTicks,
    /// Tunes each lane's note ticks to a pitch of its own, or back.
    ToggleTunedLanes,
    /// Finds the song's timing from its audio with this model.
    FindTiming(Tracker),
    /// Asks for a song in another program's format to open.
    ImportSong,
    /// Asks where to write the open song in another program's format.
    ExportSong,
    /// Draws the notefield's grid down to this note value.
    SetGrid(NoteValue),
}
