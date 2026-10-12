//! The protocol between the window and the program hosting it: what the window asks for, and what the host answers.

#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "window submodules share one private implementation vocabulary"
    )
)]
use super::*;

/// Something only the program hosting the window can do. The host answers a save through
/// [`Answer::Saved`], and an open by passing the file to [`Answer::Opened`].
#[derive(Debug, PartialEq)]
pub enum Request {
    /// Write `bytes`, the song as it stands, to `path`.
    Write {
        /// Destination file of the open song.
        path: PathBuf,
        /// Serialised chart document to write.
        bytes: Vec<u8>,
    },
    /// Ask where to save the song, suggesting the file name `name`, then write `bytes` there.
    SaveAs {
        /// Suggested file name for the save dialog.
        name: String,
        /// Serialised chart document to write at the chosen location.
        bytes: Vec<u8>,
    },
    /// Ask for a song to open.
    Open,
    /// Read the song at `path` and pass it to [`Answer::Opened`], or say why with
    /// [`Answer::Refused`]: a song opened before.
    OpenPath {
        /// File last used to open this song.
        path: PathBuf,
    },
    /// Write `bytes`, the song as its file holds it, as a dated copy named after `name` into each
    /// of `folders`, keeping the newest `kept` copies of it there; say what fails with
    /// [`Answer::SnapshotFailed`].
    ///
    /// A request must keep at least one copy:
    /// ```
    /// # use std::num::NonZeroU32;
    /// # use charter_ui::Request;
    /// let _ = Request::Snapshot {
    ///     name: String::new(), bytes: Vec::new(), folders: Vec::new(),
    ///     kept: NonZeroU32::MIN,
    /// };
    /// ```
    /// ```compile_fail
    /// # use charter_ui::Request;
    /// let _ = Request::Snapshot {
    ///     name: String::new(), bytes: Vec::new(), folders: Vec::new(),
    ///     kept: 0,
    /// };
    /// ```
    Snapshot {
        /// Name used for the dated copies.
        name: String,
        /// Serialised song to preserve.
        bytes: Vec<u8>,
        /// Destinations for the copies.
        folders: Vec<PathBuf>,
        /// Nonzero maximum number of copies to retain per destination.
        kept: NonZeroU32,
    },
    /// Ask the user for a folder to keep snapshots in, and pass it to
    /// [`Answer::UsedSnapshotFolder`].
    ChooseSnapshotFolder,
    /// Ask the user to choose an `FFmpeg` binary, and pass it to [`Answer::UsedFfmpegPath`].
    ChooseFfmpeg,
    /// Install a verified `FFmpeg` release after the user agrees.
    InstallFfmpeg,
    /// Keep `bytes`, the song's unsaved changes, where the next run finds them, in place of any
    /// kept before. The host passes them to [`Answer::Recovered`] when it starts, and says through
    /// [`Answer::AutosaveFailed`] if they could not be kept.
    Autosave {
        /// Serialised autosave and its original file path.
        bytes: Vec<u8>,
    },
    /// Remove the kept changes: they were saved or thrown away.
    DropAutosave,
    /// Copy the song's audio from `from` to `to`, beside the song, unless a file is there. The
    /// host says through [`Answer::AudioNotCopied`] if it could not.
    CopyAudio {
        /// Original audio file.
        from: PathBuf,
        /// Destination beside the chart file.
        to: PathBuf,
    },
    /// Play the song's audio from `from` at `rate` times its speed. The host reports where it is
    /// through [`Answer::Playhead`], and says when it ends through [`Answer::PlaybackEnded`].
    /// `clicks` are the tones to mix over it, each at its time in seconds into the audio.
    /// `volumes` say how loud the audio and each kind of click play, and `looping` the interval
    /// to play over and over, if any.
    Play {
        /// Moment in the audio where playback starts.
        from: ResolvedTime,
        /// Playback speed relative to the audio's usual speed.
        rate: f64,
        /// Tones timed from the start of the audio, in seconds.
        clicks: Vec<(f64, charter_audio::Tone)>,
        /// Levels for the music and each kind of click.
        volumes: charter_audio::Volumes,
        /// Nonempty audio interval to repeat.
        looping: Option<LoopRegion>,
        /// Whether audio played faster or slower than its speed keeps its pitch.
        keep_pitch: bool,
    },
    /// Carry on playing with the audio at this volume.
    Volume(charter_audio::Gain),
    /// Pause the song's audio at its current position.
    Pause,
    /// Carry on playing from this audio moment.
    Seek(ResolvedTime),
    /// Carry on playing at this many times the audio's speed.
    Rate(f64),
    /// Play `clicks` over silence, each at its time in seconds from now, alongside anything
    /// playing: a taste of a metronome sound.
    Preview {
        /// Tones timed from now, in seconds.
        clicks: Vec<(f64, charter_audio::Tone)>,
        /// Levels for the preview's tones.
        volumes: charter_audio::Volumes,
    },
    /// Ask the user to choose an audio file for the open song, and pass it to
    /// [`Answer::UsedAudio`].
    ChooseAudio,
    /// Ask the user to choose a picture for the open song's artwork, and pass it to
    /// [`Answer::UsedArtwork`].
    ChooseArtwork,
    /// Decode `bytes`, the artwork an autosave kept, and pass the picture to
    /// [`Answer::ArtworkRestored`].
    DecodeArtwork {
        /// Picture bytes kept in the autosave.
        bytes: Vec<u8>,
    },
    /// Read and decode the picture at `file`, the song's artwork, and pass it to
    /// [`Answer::ArtworkRead`].
    ReadArtwork {
        /// Artwork file named by the song.
        file: PathBuf,
    },
    /// Ask the user to choose a file holding timing in another program's format, such as a
    /// `.osu` file, and pass its text to [`Answer::TimingFileRead`].
    ChooseTimingFile,
    /// Read the tags of the audio file at `file`, and pass them to [`Answer::TagsRead`].
    ReadTags {
        /// Audio file whose title and artist tags should be read.
        file: PathBuf,
    },
    /// Find the timing of the audio in `file` at `ticks_per_beat` and pass it to
    /// [`Answer::TimingFound`], or call [`Answer::WeightsNeeded`] when the beat tracker's
    /// weights are not there yet.
    FindTiming {
        /// Audio to analyse for beats.
        file: PathBuf,
        /// Resolution of the song receiving the timing.
        ticks_per_beat: charter_core::TicksPerBeat,
        /// The model to find it with.
        tracker: Tracker,
    },
    /// Ask the user for a song in one of `formats`, each a name and the extensions its files end
    /// in, and pass its path and bytes to [`Answer::ImportedSong`] with those of the `companions`
    /// found beside it, by name.
    ChooseImport {
        /// Formats a plugin reads songs from, with their extensions.
        formats: Vec<(String, Vec<String>)>,
        /// Files a format reads beside the song's, by name, such as `info.plist`.
        companions: Vec<FileName>,
    },
    /// Ask where to write the open song in one of `formats`, suggesting the name `name`, and pass
    /// the path to [`Answer::ExportedSong`].
    ChooseExport {
        /// Suggested file name, without an extension.
        name: String,
        /// Formats the song's plugin writes, with their extensions.
        formats: Vec<(String, Vec<String>)>,
    },
    /// Write `bytes`, the open song in another program's format, to `path`, the files `beside` it
    /// and the song's own resources, each from its file to its place, saying through
    /// [`Answer::ExportFailed`] what could not be.
    WriteExport {
        /// Destination of the main file.
        path: PathBuf,
        /// The exported file.
        bytes: Vec<u8>,
        /// Files made beside it, each with its place.
        beside: Vec<(PathBuf, Vec<u8>)>,
        /// The song's files written beside it: source, destination and handling the plugin asks
        /// of the host.
        resources: Vec<(PathBuf, PathBuf, SongResource)>,
    },
    /// Download the beat tracker's weights, the user having agreed, telling
    /// [`Answer::WeightsProgress`] how far it has got, then find the timing asked for.
    DownloadWeights,
    /// Find the timing asked for on the CPU, the user having agreed after
    /// [`Answer::GpuMissing`].
    FindTimingOnCpu,
}

/// What the host did about a [`Request`], or noticed without being asked. The host hands each one
/// to [`Window::answer`], the one way anything reaches the window from outside it.
///
/// A variant carries its data owned, since the window keeps most of it. Several name the file they
/// concern, so the window can tell an answer about the open song from one about a song it has since
/// closed.
#[derive(Debug)]
pub enum Answer {
    /// The song in `path` was read, holding `bytes`.
    Opened {
        /// File the song was read from.
        path: PathBuf,
        /// The song as it was stored.
        bytes: Vec<u8>,
    },
    /// No song was named to open at startup, so the demo song stands in.
    OpenedDemo,
    /// A save finished, for better or worse.
    Saved(Saved),
    /// An autosave or snapshot holding `bytes` was found to offer the user.
    Recovered {
        /// The recovered song as it was stored.
        bytes: Vec<u8>,
    },
    /// The file at `path` could not be opened, because `why`.
    Refused {
        /// File that would not open.
        path: PathBuf,
        /// What stopped it, in the user's words.
        why: String,
    },
    /// The song's audio is `file` from now on.
    UsedAudio {
        /// Audio file the song now plays.
        file: PathBuf,
    },
    /// `file` was decoded and runs for `seconds`.
    AudioReady {
        /// Audio file that was decoded.
        file: PathBuf,
        /// How long it runs.
        seconds: f64,
    },
    /// `file` would not decode, because `why`.
    AudioFailed {
        /// Audio file that would not decode.
        file: PathBuf,
        /// What stopped it, in the user's words.
        why: String,
    },
    /// The audio could not be copied beside the saved song, because `why`.
    AudioNotCopied {
        /// What stopped it, in the user's words.
        why: String,
    },
    /// `file`'s tags named its `title` and `artist`, where it carried them.
    TagsRead {
        /// Audio file the tags were read from.
        file: PathBuf,
        /// Title the tags gave, if any.
        title: Option<String>,
        /// Artist the tags gave, if any.
        artist: Option<String>,
    },
    /// `file`'s waveform `peaks` were measured.
    WaveformReady {
        /// Audio file the peaks were measured from.
        file: PathBuf,
        /// Peaks to draw.
        peaks: Arc<Peaks>,
    },
    /// `file`'s `spectrum` was measured.
    SpectrumReady {
        /// Audio file the spectrum was measured from.
        file: PathBuf,
        /// Spectrum to draw.
        spectrum: Arc<Spectrum>,
    },
    /// `file`'s `onsets` were found.
    OnsetsReady {
        /// Audio file the onsets were found in.
        file: PathBuf,
        /// Onsets to draw and to time against.
        onsets: Arc<[charter_audio::Onset]>,
    },
    /// Playback reached the end of the audio, or was stopped.
    PlaybackEnded,
    /// Playback is this many seconds into the audio.
    Playhead(f64),
    /// Snapshots go in `folder` from now on.
    UsedSnapshotFolder {
        /// Folder the user chose for snapshots.
        folder: PathBuf,
    },
    /// A snapshot could not be written, because `why`.
    SnapshotFailed {
        /// What stopped it, in the user's words.
        why: String,
    },
    /// An autosave could not be written, because `why`.
    AutosaveFailed {
        /// What stopped it, in the user's words.
        why: String,
    },
    /// A song in another program's format was read from `path`, holding `bytes`, with the
    /// `companions` found beside it.
    ImportedSong {
        /// File the song was read from.
        path: PathBuf,
        /// The song as that program stored it.
        bytes: Vec<u8>,
        /// Files read beside it, by name.
        companions: BTreeMap<FileName, Vec<u8>>,
    },
    /// The user chose `path` to export the song to.
    ExportedSong {
        /// Destination the user chose.
        path: PathBuf,
    },
    /// The song could not be exported to `path`, because `why`.
    ExportFailed {
        /// Destination that would not take it.
        path: PathBuf,
        /// What stopped it, in the user's words.
        why: String,
    },
    /// `file` held `picture`, or no artwork the host could read.
    ArtworkRead {
        /// Artwork file that was read.
        file: PathBuf,
        /// The picture, where it decoded.
        picture: Option<Picture>,
    },
    /// `file`'s audio held `cover`, its extension and picture, where it carried one.
    ArtworkRipped {
        /// Audio file the cover was taken from.
        file: PathBuf,
        /// The cover's extension and picture, where it had one.
        cover: Option<(String, Picture)>,
    },
    /// `file`'s audio held `picture`, to be saved with the `extension` given.
    ArtworkEmbedded {
        /// Audio file the picture was taken from.
        file: PathBuf,
        /// Extension to save the picture under.
        extension: String,
        /// The picture itself.
        picture: Picture,
    },
    /// An undone artwork change put `picture` back, where it decoded.
    ArtworkRestored {
        /// The picture, where it decoded.
        picture: Option<Picture>,
    },
    /// The song's artwork is `file` from now on, holding `picture` where it decoded.
    UsedArtwork {
        /// Artwork file the song now shows.
        file: PathBuf,
        /// The picture, where it decoded.
        picture: Option<Picture>,
    },
    /// Finding the timing from the audio ended, with the timing or with why it could not.
    TimingFound(Result<charter_core::Timing, String>),
    /// The timing was not looked for, because `why`.
    TimingRefused {
        /// What stopped it, in the user's words.
        why: String,
    },
    /// A timing file was read, holding `text`.
    TimingFileRead {
        /// The timing file's contents.
        text: String,
    },
    /// A timing file would not be read, because `why`.
    TimingFileFailed {
        /// What stopped it, in the user's words.
        why: String,
    },
    /// Finding the timing is this far along, from zero to one.
    FindingProgress(f32),
    /// The beat tracker's weights are missing, so the user is asked to download them.
    WeightsNeeded,
    /// Downloading the weights is this far along, from zero to one.
    WeightsProgress(f32),
    /// No graphics card the tracker can use was found, so the user is asked about the CPU.
    GpuMissing,
    /// `FFmpeg` was looked for and found at `binary`, or not found at all.
    FfmpegFound {
        /// Where `FFmpeg` is, if anywhere.
        binary: Option<PathBuf>,
    },
    /// `FFmpeg` could not be installed, because `why`.
    FfmpegFailed {
        /// What stopped it, in the user's words.
        why: String,
    },
    /// `FFmpeg` was installed to `path`.
    FfmpegInstalled {
        /// Where the installed binary went.
        path: PathBuf,
    },
    /// Installing `FFmpeg` is this far along, from zero to one.
    FfmpegProgress(f32),
    /// `FFmpeg` is the binary at `path` from now on.
    UsedFfmpegPath {
        /// Binary the user chose.
        path: PathBuf,
    },
}
