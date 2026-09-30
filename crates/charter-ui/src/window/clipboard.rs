//! Copy, cut, paste and duplicate editing.

use super::{Clipped, Window, refused};

impl Window {
    /// Copies selected notes to the app-local clipboard and system clipboard request.
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
    /// Pastes notes from text when present, otherwise from the app-local clipboard.
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
    /// Duplicates selected notes just after the selection.
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

#[cfg(test)]
mod tests {
    use crate::window::test_support::*;
    use proptest::prelude::*;

    #[test]
    fn add_layer_button_and_shortcut_both_create_undoable_layers() {
        let mut harness = editable_demo();
        harness.get_by_label("Add layer").click();
        harness.run();
        assert_eq!(harness.state().song.as_ref().unwrap().layers().len(), 2);
        harness.key_press_modifiers(
            egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
            egui::Key::N,
        );
        harness.run();
        assert_eq!(harness.state().song.as_ref().unwrap().layers().len(), 3);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(harness.state().song.as_ref().unwrap().layers().len(), 2);
    }
    #[test]
    fn copying_a_layer_to_another_chart_is_one_undo_step() {
        let mut app = Window::new(PLUGINS);
        app.open_demo();
        app.apply(Action::AddLayer);
        let before = app.song.as_ref().unwrap().document().to_json().unwrap();
        app.apply(Action::CopyLayerToChart { layer: 1, chart: 1 });
        assert_ne!(
            app.song.as_ref().unwrap().document().to_json().unwrap(),
            before
        );
        app.apply(Action::Undo);
        assert_eq!(
            app.song.as_ref().unwrap().document().to_json().unwrap(),
            before
        );
    }
    #[test]
    fn copied_notes_paste_at_the_cursor_from_the_clipboard_or_from_memory() {
        let mut harness = editable_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.event(egui::Event::Copy);
        harness.run();
        let clipped = Clipped {
            charter_notes: harness.state().clipboard.clip.clone(),
        };
        assert_eq!(clipped.charter_notes.len(), 4);
        let text = serde_json::to_string(&clipped).unwrap();
        for _ in 0..8 {
            harness.key_press(egui::Key::ArrowDown);
        }
        harness.event(egui::Event::Paste(text));
        harness.run();
        assert_eq!(note_count(&harness), 8);
        assert_eq!(lanes_at(&harness, 1920), [0]);
        harness.key_press(egui::Key::PageDown);
        harness.event(egui::Event::Paste("some other text".to_owned()));
        harness.run();
        assert_eq!(note_count(&harness), 8, "other text is not notes");
        assert!(harness.query_by_label_contains("holds no notes").is_some());
        harness.state_mut().apply(Action::Paste(None));
        harness.run();
        assert_eq!(
            note_count(&harness),
            12,
            "the menu pastes those copied last"
        );
    }
    #[test]
    fn ctrl_arrows_move_the_selection_and_ctrl_d_and_ctrl_m_duplicate_and_mirror() {
        let mut harness = editable_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.key_press(egui::Key::Q);
        harness.run();
        assert_eq!(note_count(&harness), 4);
        select_note(&mut harness, 3, 720);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::ArrowLeft);
        harness.run();
        assert_eq!(lanes_at(&harness, 720), [2]);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::ArrowDown);
        harness.run();
        assert_eq!(lanes_at(&harness, 720), Vec::<u16>::new());
        assert_eq!(lanes_at(&harness, 960), [2]);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::M);
        harness.run();
        assert_eq!(lanes_at(&harness, 960), [1]);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::D);
        harness.run();
        assert_eq!(lanes_at(&harness, 1200), [1]);
        assert_eq!(note_count(&harness), 5);
    }
    #[test]
    fn a_duplicated_hold_starts_after_it_ends() {
        let mut harness = editable_four_taps();
        select_note(&mut harness, 0, 0);
        harness.key_press(egui::Key::CloseBracket);
        harness.key_press(egui::Key::CloseBracket);
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::D);
        harness.run();
        assert_eq!(lanes_at(&harness, 240), [1]);
        assert_eq!(lanes_at(&harness, 720), [0, 3]);
    }
    #[test]
    fn what_the_menu_places_here_goes_where_it_was_opened_though_the_cursor_moved() {
        let mut harness = editable_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.event(egui::Event::Copy);
        harness.run();
        harness.key_press(egui::Key::PageDown);
        harness.run();
        let before = harness.state().song.as_ref().unwrap().cursor();
        let paste = Action::Here(Tick::new(7680), Box::new(Action::Paste(None)));
        harness.state_mut().apply(paste);
        harness.run();
        assert_eq!(lanes_at(&harness, 7680), [0]);
        assert_eq!(harness.state().song.as_ref().unwrap().cursor(), before);
    }
    #[test]
    fn the_menu_pastes_the_clipboard_where_it_was_opened_or_else_the_last_copy() {
        let mut harness = editable_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.event(egui::Event::Copy);
        harness.run();
        let clipped = Clipped {
            charter_notes: harness
                .state()
                .clipboard
                .clip
                .iter()
                .take(1)
                .cloned()
                .collect(),
        };
        let text = serde_json::to_string(&clipped).unwrap();
        harness
            .state_mut()
            .apply(Action::PasteHere(Tick::new(3840)));
        harness.event(egui::Event::Paste(text));
        harness.run();
        assert_eq!(note_count(&harness), 5, "the clipboard's one note");
        assert_eq!(lanes_at(&harness, 3840), [0]);
        harness
            .state_mut()
            .apply(Action::PasteHere(Tick::new(7680)));
        for _ in 0..5 {
            harness.step();
        }
        assert_eq!(note_count(&harness), 9, "no text, so the last copy");
        assert_eq!(lanes_at(&harness, 7680), [0]);
    }
    #[test]
    fn f1_shows_the_keyboard_shortcuts() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::F1);
        harness.run();
        let shown = |view: &Harness<'_, Window>| {
            view.query_all_by_label("Keyboard shortcuts")
                .next()
                .is_some()
        };
        assert!(shown(&harness));
        harness.key_press(egui::Key::F1);
        harness.run();
        assert!(!shown(&harness));
    }
    #[test]
    fn playback_menu_exposes_the_fine_speed_shortcuts() {
        let mut harness = playable();
        harness.get_by_label("Playback").click();
        harness.run();

        harness.get_by_label_contains("Slower by 1%").click();
        harness.run();

        assert!(harness.query_by_label("Speed 99%").is_some());
    }
    #[test]
    fn saving_a_song_into_another_folder_plans_its_audio_copy() {
        let mut harness = four_taps_at("/songs/four.json");
        let path = PathBuf::from("/elsewhere/four.json");
        assert_eq!(
            harness.state().audio_copy_for(&path),
            Some((
                PathBuf::from("/songs/a.ogg"),
                PathBuf::from("/elsewhere/a.ogg"),
            ))
        );
        harness.state_mut().saved(Saved::To(path));
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
    }
    #[test]
    fn saving_elsewhere_keeps_the_audios_relative_subdirectory() {
        let mut document: serde_json::Value = serde_json::from_slice(&four_taps()).unwrap();
        document["metadata"]["audio"] = serde_json::json!("assets/track.ogg");
        let mut app = Window::new(PLUGINS);
        app.open(
            Path::new("/songs/four.json"),
            &serde_json::to_vec(&document).unwrap(),
        );
        let mut harness = window(app);

        harness.key_press_modifiers(
            egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
            egui::Key::S,
        );
        harness.run();
        let Request::SaveAs { bytes, .. } = request(&mut harness) else {
            panic!("expected Save As");
        };
        let saved: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(saved["metadata"]["audio"], "assets/track.ogg");
        let path = PathBuf::from("/elsewhere/four.json");
        assert_eq!(
            harness.state().audio_copy_for(&path),
            Some((
                PathBuf::from("/songs/assets/track.ogg"),
                PathBuf::from("/elsewhere/assets/track.ogg"),
            ))
        );
        harness.state_mut().saved(Saved::To(path));
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
    }
    #[test]
    fn a_host_can_preflight_audio_without_marking_a_failed_save_as_saved() {
        let mut harness = created();
        drop_file(&mut harness, "/music/track.ogg");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert!(matches!(request(&mut harness), Request::SaveAs { .. }));

        let original = harness.state().audio_file();
        assert_eq!(
            harness
                .state()
                .audio_copy_for(Path::new("/songs/mine.json")),
            Some((
                PathBuf::from("/music/track.ogg"),
                PathBuf::from("/songs/track.ogg"),
            ))
        );
        assert_eq!(harness.state().audio_file(), original);
        harness
            .state_mut()
            .saved(Saved::Failed("the copy failed".to_owned()));
        harness.run();
        assert_eq!(harness.state().audio_file(), original);
        assert!(harness.state().song.as_ref().unwrap().is_modified());
        assert_eq!(harness.state().path, None);
        assert_eq!(harness.state_mut().take_requests(), []);
    }
    #[test]
    fn a_confirmed_save_after_audio_preflight_queues_no_second_copy() {
        let mut harness = created();
        drop_file(&mut harness, "/music/track.ogg");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert!(matches!(request(&mut harness), Request::SaveAs { .. }));
        let path = PathBuf::from("/songs/mine.json");
        assert!(harness.state().audio_copy_for(&path).is_some());

        harness.state_mut().saved(Saved::To(path));
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
        assert_eq!(
            harness.state().audio_file(),
            Some(PathBuf::from("/songs/track.ogg"))
        );
        assert!(!harness.state().song.as_ref().unwrap().is_modified());
    }
    #[test]
    fn saving_a_song_in_its_own_folder_copies_nothing() {
        let mut harness = four_taps_at("/songs/four.json");
        let path = PathBuf::from("/songs/renamed.json");
        assert_eq!(harness.state().audio_copy_for(&path), None);
        harness.state_mut().saved(Saved::To(path));
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
    }
    #[test]
    fn the_shortcuts_filter_to_what_is_typed() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::F1);
        harness.run();
        let filter = harness.get_by_role_and_label(Role::TextInput, "Filter");
        filter.focus();
        filter.type_text("bookmark");
        harness.run();
        assert!(harness.query_by_label("Ctrl+B").is_some());
        assert!(harness.query_by_label("Ctrl+Z").is_none());
    }
    #[test]
    fn shortcuts_work_with_no_song_open() {
        let mut harness = window(Window::new(GAMES));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::O);
        harness.run();
        assert_eq!(request(&mut harness), Request::Open);
    }
    #[test]
    fn saving_a_song_plans_to_copy_dropped_audio_beside_it() {
        let mut harness = created();
        drop_file(&mut harness, "/music/track.ogg");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert!(matches!(request(&mut harness), Request::SaveAs { .. }));
        let path = PathBuf::from("/songs/mine.json");
        assert_eq!(
            harness.state().audio_copy_for(&path),
            Some((
                PathBuf::from("/music/track.ogg"),
                PathBuf::from("/songs/track.ogg"),
            ))
        );
        harness.state_mut().saved(Saved::To(path.clone()));
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
        assert_eq!(harness.state().audio_copy_for(&path), None);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert!(matches!(request(&mut harness), Request::Write { .. }));
        harness.state_mut().saved(Saved::To(path));
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
    }
    #[test]
    fn audio_already_beside_the_song_is_not_copied() {
        let mut harness = created();
        drop_file(&mut harness, "/songs/track.ogg");
        let path = PathBuf::from("/songs/mine.json");
        assert_eq!(harness.state().audio_copy_for(&path), None);
        harness.state_mut().saved(Saved::To(path));
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
    }
    #[test]
    fn editing_actions_without_a_song_neither_create_one_nor_request_a_save() {
        let mut app = Window::new(PLUGINS);
        for action in [
            Action::Place {
                tick: Tick::new(0),
                lane: LaneIndex::new(0),
                note: Note::Tap,
            },
            Action::Undo,
            Action::Redo,
            Action::Copy,
            Action::Paste(None),
            Action::FindTiming(crate::Tracker::BeatThis),
            Action::TogglePlay,
            Action::Save,
            Action::SaveAs,
        ] {
            app.apply(action);
        }
        let mut harness = window(app);
        assert!(harness.state().song.is_none());
        assert_eq!(harness.state().title(), "charter");
        assert_eq!(harness.state_mut().take_requests(), []);
        assert_eq!(
            harness.state().message(),
            Some("Finding the timing needs the song's audio")
        );
    }
    #[test]
    fn a_clipboard_reply_cancels_the_wait_for_the_last_copied_notes() {
        let mut harness = editable_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.event(egui::Event::Copy);
        harness.run();
        let clipped = Clipped {
            charter_notes: harness
                .state()
                .clipboard
                .clip
                .iter()
                .take(1)
                .cloned()
                .collect(),
        };
        harness
            .state_mut()
            .apply(Action::PasteHere(Tick::new(3840)));
        harness.step();
        harness.event(egui::Event::Paste(serde_json::to_string(&clipped).unwrap()));
        harness.run();
        for _ in 0..PASTE_WAIT {
            harness.step();
        }
        assert_eq!(note_count(&harness), 5);
        assert_eq!(lanes_at(&harness, 3840), [0]);
        assert_eq!(harness.state().clipboard.pasting, None);
    }

    proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn arbitrary_external_clipboard_text_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let text = String::from_utf8_lossy(&bytes);
            let mut app = Window::new(PLUGINS);
            app.open(Path::new("four.json"), &four_taps());
            app.paste(Some(&text));
            prop_assert!(
                app.song
                    .as_ref()
                    .is_some_and(|song| song.document().to_json().is_ok())
            );
        }
    }
}
