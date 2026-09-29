//! Timing import and automatic timing discovery.

use super::{Finding, Request, Window, found_message};

impl Window {
    /// Handles this window operation.
    pub fn timing_file_read(&mut self, text: &str) {
        text.clone_into(&mut self.import.text);
        self.import.set_open(true);
    }
    /// Handles this window operation.
    pub fn timing_file_failed(&mut self, why: &str) {
        self.failure = Some(format!("Could not read the timing: {why}"));
    }
    /// Handles this private window operation.
    pub(super) fn find_timing(&mut self) {
        let (Some(song), Some(file)) = (&self.song, self.audio_file()) else {
            self.failure = Some("Finding the timing needs the song's audio".to_owned());
            return;
        };
        let ticks_per_beat = song.document().document().ticks_per_beat;
        self.requests.push(Request::FindTiming {
            file,
            ticks_per_beat,
        });
        self.dialogs.finding = Finding::Running;
    }
    /// Handles this window operation.
    pub fn timing_found(&mut self, found: Result<charter_core::Timing, String>) {
        self.dialogs.finding = Finding::Idle;
        let Some(song) = &mut self.song else {
            return;
        };
        self.failure = Some(match found.map(|timing| song.replace_timing(timing)) {
            Ok(Ok(())) => found_message(song),
            Ok(Err(refusal)) => format!("The timing found was refused: {refusal}"),
            Err(why) => format!("No timing found: {why}"),
        });
    }
}
