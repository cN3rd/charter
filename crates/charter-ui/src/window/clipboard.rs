//! Copy, cut, paste and duplicate editing.

use super::{Clipped, Window, refused};

impl Window {
    /// Handles this private window operation.
    pub(super) fn copy(&mut self) {
        let Some(song) = &self.song else {
            return;
        };
        let copied = song.copied();
        if copied.is_empty() {
            return;
        }
        let clipped = Clipped {
            charter_notes: copied,
        };
        self.clipboard.out = serde_json::to_string(&clipped).ok();
        self.clipboard.clip = clipped.charter_notes;
    }
    /// Handles this private window operation.
    pub(super) fn paste(&mut self, text: Option<&str>) {
        let external = if let Some(text) = text {
            let Ok(clipped) = serde_json::from_str::<Clipped>(text) else {
                self.failure =
                    Some("The clipboard holds no notes: copy some first (Ctrl+C).".to_owned());
                return;
            };
            Some(clipped.charter_notes)
        } else {
            None
        };
        let clip = external.as_deref().unwrap_or(&self.clipboard.clip);
        let Some(song) = &mut self.song else {
            return;
        };
        if clip.is_empty() {
            self.failure = Some("Nothing to paste: copy some notes first (Ctrl+C).".to_owned());
            return;
        }
        match song.paste(clip, song.cursor()) {
            Ok(0) => self.failure = Some("Nothing pasted: every place is taken.".to_owned()),
            Ok(_) => {}
            Err(why) => self.failure = Some(refused(&why)),
        }
    }
    /// Handles this private window operation.
    pub(super) fn duplicate(&mut self) {
        let Some(song) = &mut self.song else {
            return;
        };
        // After the selection's last end, so a hold is not copied over itself.
        let last = song.chart().and_then(|chart| {
            chart
                .objects
                .iter()
                .filter(|object| song.is_selected(object))
                .filter_map(|object| object.shape.end(object.tick))
                .max()
        });
        let step = song
            .document()
            .document()
            .ticks_per_beat
            .note_length(song.snap());
        let (Some(last), Some(step)) = (last, step) else {
            return;
        };
        let Some(at) = last.next_on(step) else {
            return;
        };
        self.failure = match song.duplicate_selection(at) {
            Ok(0) => Some("Nothing duplicated: every place is taken.".to_owned()),
            Ok(_) => None,
            Err(why) => Some(refused(&why)),
        };
    }
}
