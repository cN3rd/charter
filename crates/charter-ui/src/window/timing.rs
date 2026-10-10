//! Timing import and automatic timing discovery.

use super::{Finding, Request, Tracker, Window, found_message};

impl Window {
    /// Loads pasted timing text from a file into the import window.
    pub fn timing_file_read(&mut self, text: &str) {
        text.clone_into(&mut self.import.text);
        self.import.set_open(true);
    }
    /// Shows why the chosen timing file could not be read.
    pub fn timing_file_failed(&mut self, why: &str) {
        self.failure = Some(format!("Could not read the timing: {why}"));
    }
    /// Starts automatic timing, or asks for tracker weights before it can.
    pub(super) fn find_timing(&mut self, tracker: Tracker) {
        let (Some(song), Some(file)) = (&self.song, self.audio_file()) else {
            self.failure = Some("Finding the timing needs the song's audio".to_owned());
            return;
        };
        let ticks_per_beat = song.document().document().ticks_per_beat;
        self.requests.push(Request::FindTiming {
            file: file.clone(),
            ticks_per_beat,
            tracker,
        });
        self.dialogs.finding = Finding::Running(None);
        self.dialogs.finding_for = Some((file, ticks_per_beat));
        self.dialogs.tracker = tracker;
    }
    /// Accepts automatic timing from the host or shows why it failed. Timing found for audio or
    /// a resolution the open song no longer has is left out.
    pub fn timing_found(&mut self, found: Result<charter_core::Timing, String>) {
        self.dialogs.finding = Finding::Idle;
        let asked = self.dialogs.finding_for.take();
        let open = self.audio_file().zip(
            self.song
                .as_ref()
                .map(|song| song.document().document().ticks_per_beat),
        );
        let Some(song) = &mut self.song else {
            return;
        };
        self.failure = Some(match found {
            Ok(_) if asked != open => {
                "The timing found no longer fits the open song, so it was not used".to_owned()
            }
            Ok(timing) => match song.replace_timing(timing) {
                Ok(()) => found_message(song),
                Err(refusal) => format!("The timing found was refused: {refusal}"),
            },
            Err(why) => format!("No timing found: {why}"),
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::window::test_support::*;

    #[test]
    fn hidden_notes_stop_ticking_but_the_metronome_keeps_playing() {
        let mut song = Song::new(charter_editor::demo::song(), PLUGINS);
        let settings = Settings {
            note_ticks: true,
            ..Settings::default()
        };
        let before = clicks(
            &song,
            &settings,
            false,
            (0.0, 30.0),
            charter_timing::grid::Grid::default(),
        );
        assert!(!before.is_empty());
        song.set_layer_view(0, charter_core::LayerView::Hidden)
            .unwrap();
        let after = clicks(
            &song,
            &settings,
            false,
            (0.0, 30.0),
            charter_timing::grid::Grid::default(),
        );
        assert!(after.is_empty());
        let metronome = clicks(
            &song,
            &settings,
            true,
            (0.0, 1.0),
            charter_timing::grid::Grid::default(),
        );
        assert!(!metronome.is_empty());
    }
    #[test]
    #[expect(
        clippy::non_ascii_literal,
        reason = "the test locates the Choose audio menu label with its ellipsis"
    )]
    fn read_only_songs_disable_menu_items_that_would_edit_them() {
        let mut app = Window::new(PLUGINS);
        app.open(Path::new("example.json"), example());
        let mut harness = window(app);

        harness.get_by_label("File").click();
        harness.run();
        assert!(
            harness
                .get_by_label("Choose audio…")
                .accesskit_node()
                .is_disabled()
        );
        harness.get_by_label("Timing").click();
        harness.run();
        assert!(
            harness
                .get_by_label_contains("Raise the tempo")
                .accesskit_node()
                .is_disabled()
        );
        assert!(
            !harness
                .get_by_label_contains("Timing window")
                .accesskit_node()
                .is_disabled()
        );
    }
    #[test]
    fn the_tempo_at_the_cursor_shows_beside_the_song_and_in_the_status_bar() {
        let harness = editable_demo();
        assert_eq!(bpm_field(&harness).value().as_deref(), Some("150"));
        assert!(harness.query_by_label("150 BPM").is_some());
    }
    #[test]
    fn a_bpm_typed_changes_the_tempo_of_the_section_the_cursor_is_in() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        enter_bpm(&mut harness, "175.5", egui::Key::Enter);
        assert_eq!(bpm_at_cursor(&harness), Some((0, "175.5".to_owned())));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
    }
    #[test]
    fn a_bpm_keeps_notes_where_they_sound_unless_the_setting_keeps_them_on_their_beats() {
        let mut harness = editable_demo();
        let (tick, seconds) = first_note(&harness);
        enter_bpm(&mut harness, "175.5", egui::Key::Enter);
        let (moved, heard) = first_note(&harness);
        assert_ne!(moved, tick);
        assert!((heard - seconds).abs() < 1e-3, "{heard} {seconds}");

        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.state_mut().settings.notes_keep_time = false;
        harness.run();
        enter_bpm(&mut harness, "175.5", egui::Key::Enter);
        assert_eq!(first_note(&harness).0, tick);
    }
    #[test]
    fn a_bpm_typed_applies_when_the_field_is_left() {
        let mut harness = editable_demo();
        bpm_field(&harness).focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        bpm_field(&harness).type_text("160");
        harness.run();
        offset_field(&harness).focus();
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((0, "160".to_owned())));
    }
    #[test]
    fn the_bpm_steps_up_and_down_a_whole_bpm_at_a_time() {
        let mut harness = editable_demo();
        click_in_panel(&mut harness, "Raise the BPM by 1");
        assert_eq!(bpm_at_cursor(&harness), Some((0, "151".to_owned())));
        harness.get_by_label("Lower the BPM by 1").click();
        click_in_panel(&mut harness, "Lower the BPM by 1");
        assert_eq!(bpm_at_cursor(&harness), Some((0, "149".to_owned())));
    }
    #[test]
    fn a_bpm_left_with_escape_changes_nothing() {
        let mut harness = editable_demo();
        enter_bpm(&mut harness, "90", egui::Key::Escape);
        assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
        assert_eq!(bpm_field(&harness).value().as_deref(), Some("150"));
    }
    #[test]
    fn a_bpm_that_is_not_a_tempo_says_so() {
        let mut harness = editable_demo();
        enter_bpm(&mut harness, "fast", egui::Key::Enter);
        assert!(
            harness
                .query_by_label_contains("fast is not a tempo")
                .is_some()
        );
        assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
    }
    #[test]
    fn a_timing_point_is_added_at_the_cursor_and_removed_but_not_the_first() {
        let mut harness = editable_demo();
        harness.set_size(egui::vec2(1000.0, 1100.0));
        harness.run();
        assert!(harness.query_by_label("Remove this timing point").is_none());
        let add = |view: &Harness<'_, Window>| {
            view.get_by_label("Add a timing point here")
                .accesskit_node()
                .is_disabled()
        };
        assert!(add(&harness), "the cursor is on the first point");
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        assert!(!add(&harness));
        harness.get_by_label("Add a timing point here").click();
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((240, "150".to_owned())));
        assert!(add(&harness));
        click_in_panel(&mut harness, "Remove this timing point");
        assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
    }
    #[test]
    fn the_timing_points_list_shows_every_point_and_jumps_to_one_clicked() {
        let mut harness = editable_demo();
        // Tall enough that the whole song panel, the list with it, is on screen.
        harness.set_size(egui::vec2(1000.0, 1100.0));
        harness.run();
        let rows = harness
            .query_all_by_label_contains("Timing point at")
            .count();
        assert_eq!(rows, 5);
        harness.get_by_label("Timing point at 00:26.100").click();
        harness.run();
        assert!(cursor_shows(&harness, "16:0:0"));
        assert_eq!(bpm_field(&harness).value().as_deref(), Some("180"));
    }
    #[test]
    fn a_time_typed_for_a_point_moves_it_in_the_audio() {
        let mut harness = editable_demo();
        harness.set_size(egui::vec2(1000.0, 1100.0));
        harness.run();
        harness.get_by_label("Timing point at 00:26.100").click();
        harness.run();
        let time = harness.get_by_role_and_label(Role::TextInput, "Time");
        assert_eq!(time.value().as_deref(), Some("00:26.100"));
        time.focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness
            .get_by_role_and_label(Role::TextInput, "Time")
            .type_text("0:26.25");
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert_eq!(anchor_seconds(&harness), [0.0, 26.25]);
        harness.get_by_label("Timing point at 00:26.250").click();
        harness.run();
    }
    #[test]
    fn the_starting_bpm_sets_the_first_points_tempo_from_anywhere() {
        let mut harness = editable_demo();
        harness.get_by_label("Timing point at 00:26.100").click();
        harness.run();
        let start = harness.get_by_role_and_label(Role::TextInput, "Starting BPM");
        assert_eq!(start.value().as_deref(), Some("150"));
        start.focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness
            .get_by_role_and_label(Role::TextInput, "Starting BPM")
            .type_text("160");
        harness.key_press(egui::Key::Enter);
        harness.run();
        let song = harness.state().song.as_ref().unwrap();
        assert_eq!(
            song.timing().bpm_at(Tick::new(0)).map(f64::from),
            Some(160.0)
        );
        assert_eq!(anchor_seconds(&harness), [0.0, 26.1]);
    }
    #[test]
    fn a_meter_typed_starts_measures_of_it_at_the_timing_point() {
        let mut harness = editable_demo();
        assert_eq!(meter_field(&harness).value().as_deref(), Some("4/4"));
        meter_field(&harness).focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        meter_field(&harness).type_text("3/4");
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        let song = harness.state().song.as_ref().unwrap();
        let meter = song
            .timing()
            .measures_from(Tick::new(0))
            .next()
            .unwrap()
            .meter;
        assert_eq!(meter.to_string(), "3/4");
        meter_field(&harness).focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        meter_field(&harness).type_text("waltz");
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert!(
            harness
                .query_by_label_contains("waltz is not a meter")
                .is_some()
        );
    }
    #[test]
    fn a_read_only_song_shows_its_tempo_without_changing_it() {
        let mut harness = window(Window::new(&[]));
        harness.state_mut().open_demo();
        harness.run();
        assert!(
            harness
                .query_by_role_and_label(Role::TextInput, "BPM")
                .is_none()
        );
        assert!(harness.query_by_label("150 BPM").is_some());
    }
    #[test]
    fn clicking_a_marker_in_the_list_jumps_there() {
        let mut harness = editable_demo();
        // The list sits below the tempo controls, past the bottom of a short window.
        harness.get_by_label("Chorus").scroll_to_me();
        harness.run();
        harness.get_by_label("Chorus").click();
        harness.run();
        assert!(cursor_shows(&harness, "16:0:0"));
        assert_eq!(section_field(&harness).value().as_deref(), Some("Chorus"));
    }
    #[test]
    fn a_section_named_at_the_cursor_is_added_and_listed() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::PageDown);
        harness.run();
        assert_eq!(section_field(&harness).value().as_deref(), Some(""));
        enter_section(&mut harness, "Build", egui::Key::Enter);
        assert_eq!(marker_texts(&harness)[1], "Build");
        assert!(harness.query_by_label("Build").is_some());
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert!(!marker_texts(&harness).contains(&"Build".to_owned()));
    }
    #[test]
    fn a_section_renamed_at_the_cursor_keeps_its_place() {
        let mut harness = editable_demo();
        enter_section(&mut harness, "Opening", egui::Key::Enter);
        assert_eq!(marker_texts(&harness)[0], "Opening");
    }
    #[test]
    fn a_section_name_left_with_escape_changes_nothing() {
        let mut harness = editable_demo();
        enter_section(&mut harness, "Opening", egui::Key::Escape);
        assert_eq!(marker_texts(&harness)[0], "Intro");
    }
    #[test]
    fn a_read_only_song_jumps_to_markers_without_editing_them() {
        let mut harness = window(Window::new(&[]));
        harness.state_mut().open_demo();
        harness.run();
        click_in_panel(&mut harness, "Verse");
        assert!(cursor_shows(&harness, "8:0:0"));
        assert!(
            harness
                .query_by_role_and_label(Role::TextInput, "Section here")
                .is_none()
        );
        assert!(harness.query_by_label("Remove marker Verse").is_none());
    }
    #[test]
    fn alt_up_and_down_jump_between_sections() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowDown);
        harness.run();
        assert!(cursor_shows(&harness, "8:0:0"));
        harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowDown);
        harness.run();
        assert!(cursor_shows(&harness, "16:0:0"));
        harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowUp);
        harness.run();
        assert!(cursor_shows(&harness, "8:0:0"));
    }
    #[test]
    fn f8_and_f7_raise_and_lower_the_tempo_at_the_cursor_a_bpm_at_a_time() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::F8);
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((0, "151".to_owned())));
        harness.key_press(egui::Key::F7);
        harness.key_press(egui::Key::F7);
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((0, "149".to_owned())));
    }
    #[test]
    fn a_lane_key_while_playing_places_on_the_nearest_snap_line_without_seeking() {
        let mut harness = playable();
        space(&mut harness);
        harness.state_mut().take_requests();
        // 0.63 s is tick 1209 at 120 BPM; the nearest sixteenth is 1200.
        harness.state_mut().playhead(0.63);
        harness.run();
        assert!(lanes_at(&harness, 1200).is_empty());
        harness.key_press(egui::Key::Num1);
        harness.run();
        assert_eq!(lanes_at(&harness, 1200), [0]);
        assert_eq!(harness.state_mut().take_requests(), []);
    }
    #[test]
    fn an_edit_made_while_a_chord_is_held_is_its_own_step() {
        let mut harness = editable_four_taps();
        harness.key_press(egui::Key::PageDown);
        harness.run();
        harness.key_down(egui::Key::Num1);
        harness.key_down(egui::Key::Num2);
        harness.run();
        harness.key_press(egui::Key::F8);
        harness.run();
        harness.key_up(egui::Key::Num1);
        harness.key_up(egui::Key::Num2);
        harness.run();
        let bpm = |view: &Harness<'_, Window>| {
            let song = view.state().song.as_ref().unwrap();
            song.timing().tempo_at(Tick::new(0))
        };
        let raised = bpm(&harness);
        assert_eq!(note_count(&harness), 6);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert!(bpm(&harness) < raised, "the BPM change undoes alone");
        assert_eq!(note_count(&harness), 6);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(
            lanes_at(&harness, 3840),
            Vec::<u16>::new(),
            "then the chord as one"
        );
    }
    #[test]
    fn the_grid_draws_every_snap_line() {
        let n = |value: u16| NonZeroU16::new(value).unwrap();
        assert_eq!(shown_grid(n(4), n(16)), n(16));
        assert_eq!(shown_grid(n(16), n(4)), n(16));
        assert_eq!(shown_grid(n(48), n(12)), n(48));
        assert_eq!(shown_grid(n(16), n(12)), n(48));
        assert_eq!(shown_grid(n(48), n(64)), n(192));
        assert_eq!(shown_grid(n(16), n(16)), n(16));
    }
    #[test]
    fn the_metronome_follows_every_timing_point_offset_tempo_meter_and_stop() {
        let song = serde_json::json!({
            "format_version": 0,
            "ruleset": { "id": "sm", "version": "1" },
            "metadata": { "title": "Timed", "artist": "a", "audio": "a.ogg" },
            "timing": { "offset_seconds": 0.5, "events": [
                { "tick": 0, "kind": { "bpm": 120.0 } },
                { "tick": 3840, "kind": { "bpm": 240.0 } },
                { "tick": 3840, "kind": { "time_signature": [3, 4] } },
                { "tick": 4800, "kind": { "stop": 0.5 } }
            ] },
            "charts": [ { "name": "Easy", "lanes": 4 } ]
        });
        let settings = Settings {
            metronome: true,
            metronome_sound: Kit::PureSine,
            ..Settings::default()
        };
        let mut app = Window::new(PLUGINS).with_settings(settings);
        app.open(
            Path::new("/songs/timed.json"),
            &serde_json::to_vec(&song).unwrap(),
        );
        let mut harness = window(app);
        harness
            .state_mut()
            .audio_ready(Path::new("/songs/a.ogg"), 4.2);
        harness.run();
        let clicks = clicks_played(&mut harness);
        let (a, b) = (Tone::Accent(Kit::PureSine), Tone::Beat(Kit::PureSine));
        // 120 BPM in 4/4 from 0.5 s, its grid carried back into the lead-in; from 2.5 s,
        // 240 BPM in 3/4, with half a second stopped after the beat at 2.75 s.
        let expected = [
            (0.0, b),
            (0.5, a),
            (1.0, b),
            (1.5, b),
            (2.0, b),
            (2.5, a),
            (2.75, b),
            (3.5, b),
            (3.75, a),
            (4.0, b),
        ];
        assert_eq!(clicks.len(), expected.len(), "{clicks:?}");
        for ((at, tone), (want_at, want_tone)) in clicks.iter().zip(expected) {
            assert!((at - want_at).abs() < 1e-9, "{clicks:?}");
            assert_eq!(*tone, want_tone, "{clicks:?}");
        }
    }
    #[test]
    fn an_edit_while_looping_plays_the_loop_again_from_its_start() {
        let mut harness = listening(Settings::default());
        harness.key_press(egui::Key::L);
        harness.run();
        played(&mut harness);
        harness.key_press(egui::Key::Plus);
        harness.run();
        let (from, looping, _) = played(&mut harness);
        assert!(from.abs() < 1e-9);
        let (_, end) = looping.expect("looping");
        assert!(
            (end - 2.0).abs() > 1e-3,
            "the bar follows the new tempo: {end}"
        );
    }
    #[test]
    #[expect(
        clippy::non_ascii_literal,
        reason = "the test locates the Calibrate button with its ellipsis"
    )]
    fn calibrating_latency_plays_clicks_at_the_songs_first_tempo() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        settings_page(&mut harness, "Sounds");
        harness.get_by_label("Calibrate…").scroll_to_me();
        harness.run();
        harness.get_by_label("Calibrate…").click();
        harness.run();
        harness.get_by_label("Start").click();
        // The flash keeps the window drawing while the clicks play.
        harness.run_steps(2);
        let requests = harness.state_mut().take_requests();
        let Some(Request::Preview { clicks, .. }) = requests.last() else {
            panic!("expected clicks, got {requests:?}");
        };
        assert_eq!(clicks.len(), 32);
        let gap = clicks[1].0 - clicks[0].0;
        assert!(
            (gap - 0.4).abs() < 1e-9,
            "150 BPM, the demo's first tempo: {gap}"
        );
        assert!(harness.query_by_label_contains("of 16 taps").is_some());
    }
    #[test]
    fn lowering_the_tempo_below_nothing_says_so() {
        let mut harness = editable_demo();
        enter_bpm(&mut harness, "0.5", egui::Key::Enter);
        harness.key_press(egui::Key::F7);
        harness.run();
        assert!(harness.query_by_label_contains("is not a tempo").is_some());
        assert_eq!(bpm_at_cursor(&harness), Some((0, "0.5".to_owned())));
    }
    #[test]
    fn a_bpm_step_lands_on_the_decimal_a_double_sum_misses() {
        // 128.2 - 1.0 is 127.19999999999999 as a double.
        let mut harness = editable_demo();
        enter_bpm(&mut harness, "128.2", egui::Key::Enter);
        harness.key_press(egui::Key::F7);
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((0, "127.2".to_owned())));
        enter_bpm(&mut harness, "128.2", egui::Key::Enter);
        click_in_panel(&mut harness, "Lower the BPM by 1");
        assert_eq!(bpm_at_cursor(&harness), Some((0, "127.2".to_owned())));
    }
    #[test]
    fn saving_opening_and_help_work_while_a_field_has_the_keyboard() {
        let mut harness = editable_four_taps();
        bpm_field(&harness).focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert!(matches!(request(&mut harness), Request::Write { .. }));
        harness.key_press(egui::Key::Space);
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
        assert!(!harness.state().is_playing());
    }
    #[test]
    fn the_timing_menu_adds_a_bookmark_and_jumps_between_sections() {
        let mut harness = editable_demo();
        let before = marker_texts(&harness).len();
        harness.get_by_label("Timing").click();
        harness.run();
        harness
            .get_by_label_contains("Add or remove a bookmark")
            .click();
        harness.run();
        assert_eq!(marker_texts(&harness).len(), before + 1);
        harness.get_by_label("Timing").click();
        harness.run();
        harness.get_by_label_contains("Next section").click();
        harness.run();
        assert!(cursor_shows(&harness, "8:0:0"));
    }
    #[test]
    fn the_timing_window_opens_on_rows_of_seconds() {
        let mut harness = timed_song();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
        harness.run();
        assert!(harness.query_by_label("Row at 0.0 s").is_some());
        assert!(harness.query_by_label("Measure 0").is_none());
    }
    #[test]
    fn the_timing_window_uses_only_the_open_songs_waveform() {
        let mut harness = syncing();
        let waiting = "The waveform shows once the song's audio loads.";
        assert!(harness.query_by_label(waiting).is_some());
        harness
            .state_mut()
            .waveform_ready(Path::new("other.ogg"), peaks());
        harness.run();
        assert!(harness.query_by_label(waiting).is_some());
        harness
            .state_mut()
            .waveform_ready(Path::new("a.ogg"), peaks());
        harness.run();
        assert!(harness.query_by_label(waiting).is_none());
        assert!(harness.output().shapes.iter().any(|shape| matches!(
            &shape.shape,
            egui::Shape::LineSegment { stroke, .. }
                if stroke.color == style::TIMING_WAVE
        )));
    }
    #[test]
    fn the_timing_windows_start_bpm_sets_the_first_anchors_tempo() {
        let mut harness = syncing();
        let start = harness.get_by_role_and_label(Role::TextInput, "Start BPM");
        let before = start.value();
        start.focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness
            .get_by_role_and_label(Role::TextInput, "Start BPM")
            .type_text("140");
        harness.key_press(egui::Key::Enter);
        harness.run();
        let song = harness.state().song.as_ref().unwrap();
        assert_eq!(
            song.timing().bpm_at(Tick::new(0)).map(f64::from),
            Some(140.0)
        );
        assert_ne!(before.as_deref(), Some("140"));
    }
    #[test]
    fn finding_timing_asks_the_host_to_find_it_in_the_songs_audio() {
        let mut harness = playable();
        find_from_the_menu(&mut harness);
        assert_eq!(
            harness.state_mut().take_requests(),
            [Request::FindTiming {
                file: PathBuf::from("/songs/a.ogg"),
                ticks_per_beat: charter_core::TicksPerBeat::default(),
                tracker: crate::Tracker::BeatThis,
            }]
        );
        assert!(
            harness
                .query_by_label_contains("Finding the timing")
                .is_some()
        );
    }
    #[test]
    fn the_weights_are_downloaded_only_once_the_user_says_so() {
        let mut harness = playable();
        find_from_the_menu(&mut harness);
        harness.state_mut().take_requests();
        harness.state_mut().weights_needed();
        harness.run();
        assert!(harness.query_by_label_contains("81 MB").is_some());
        harness.get_by_label("Cancel").click();
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
        assert!(
            harness
                .query_by_label_contains("Finding the timing")
                .is_none()
        );

        find_from_the_menu(&mut harness);
        harness.state_mut().take_requests();
        harness.state_mut().weights_needed();
        harness.run();
        harness.get_by_label("Download").click();
        harness.run();
        assert_eq!(
            harness.state_mut().take_requests(),
            [Request::DownloadWeights]
        );
        harness.state_mut().weights_progress(0.42);
        harness.run();
        assert!(harness.query_by_label_contains("42%").is_some());
    }
    #[test]
    #[expect(
        clippy::non_ascii_literal,
        reason = "the test clicks the menu label with its ellipsis"
    )]
    fn mapperatorinator_is_offered_beside_beat_this_with_its_own_download() {
        let mut harness = playable();
        harness.query_all_by_label("Timing").next().unwrap().click();
        harness.run();
        harness
            .get_by_label("Find timing with Mapperatorinator…")
            .click();
        harness.run();
        assert_eq!(
            harness.state_mut().take_requests(),
            [Request::FindTiming {
                file: PathBuf::from("/songs/a.ogg"),
                ticks_per_beat: charter_core::TicksPerBeat::default(),
                tracker: crate::Tracker::Mapperatorinator,
            }]
        );
        harness.state_mut().weights_needed();
        harness.run();
        assert!(harness.query_by_label_contains("416 MB").is_some());
        harness.get_by_label("Download").click();
        harness.run();
        harness.state_mut().weights_progress(0.5);
        harness.run();
        assert!(
            harness
                .query_by_label_contains("Mapperatorinator weights, 50%")
                .is_some()
        );
    }

    #[test]
    fn without_a_graphics_card_the_user_chooses_whether_to_find_it_on_the_cpu() {
        let mut harness = playable();
        find_from_the_menu(&mut harness);
        harness.state_mut().take_requests();
        harness.state_mut().gpu_missing();
        harness.run();
        assert!(
            harness
                .query_by_label_contains("No graphics card")
                .is_some()
        );
        harness.get_by_label("Cancel").click();
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
        assert!(
            harness
                .query_by_label_contains("Finding the timing")
                .is_none()
        );

        find_from_the_menu(&mut harness);
        harness.state_mut().take_requests();
        harness.state_mut().gpu_missing();
        harness.run();
        harness.get_by_label("Run on the CPU").click();
        harness.run();
        assert_eq!(
            harness.state_mut().take_requests(),
            [Request::FindTimingOnCpu]
        );
        harness.state_mut().finding_progress(0.25);
        harness.run();
        assert!(
            harness
                .query_by_label_contains("Finding the timing, 25%")
                .is_some()
        );
    }
    #[test]
    fn timing_found_takes_the_songs_place_in_one_step_and_says_what_it_is() {
        let mut harness = playable();
        find_from_the_menu(&mut harness);
        harness.state_mut().take_requests();
        let before = anchor_seconds(&harness);
        let found: charter_core::Timing = serde_json::from_value(serde_json::json!({
            "anchors": [ { "tick": 0, "seconds": 3.616 } ],
            "tempo": 132.0,
            "events": []
        }))
        .unwrap();
        harness.state_mut().timing_found(Ok(found));
        harness.run();
        assert_eq!(anchor_seconds(&harness), [3.616]);
        assert!(
            harness
                .query_by_label_contains("Timing found: 132 BPM")
                .is_some()
        );
        assert!(
            harness
                .query_by_label_contains("Finding the timing")
                .is_none()
        );
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(anchor_seconds(&harness), before);
        harness
            .state_mut()
            .timing_found(Err("the tracker would not load".to_owned()));
        harness.run();
        assert!(harness.query_by_label_contains("would not load").is_some());
    }
    #[test]
    fn resetting_the_timing_asks_first_then_leaves_120_bpm_from_zero_in_one_step() {
        let mut harness = editable_demo();
        let before = harness
            .state()
            .song
            .as_ref()
            .unwrap()
            .timing()
            .events
            .clone();
        reset_from_the_menu(&mut harness);
        harness.get_by_label("Cancel").click();
        harness.run();
        assert_eq!(
            harness.state().song.as_ref().unwrap().timing().events,
            before
        );
        reset_from_the_menu(&mut harness);
        harness.get_by_label("Reset").click();
        harness.run();
        let song = harness.state().song.as_ref().unwrap();
        assert_eq!(song.timing().tempo.to_string(), "120");
        assert_eq!(
            serde_json::to_value(song.timing().anchors[0].seconds).unwrap(),
            serde_json::json!(0.0)
        );
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(
            harness.state().song.as_ref().unwrap().timing().events,
            before
        );
    }
    #[test]
    fn v_lays_the_rows_out_in_seconds_and_a_click_anchors_the_moment_under_it() {
        let mut harness = syncing();
        harness.key_press(egui::Key::V);
        harness.run();
        assert!(harness.query_by_label("Measure 0").is_none());
        let row = harness.get_by_label("Row at 0.0 s").rect();
        // Two seconds a row, a tenth of that from before it: 0.95 s is 1.15 of 2.2 across.
        let across = crate::cast::f64_f32(1.15 / 2.2);
        let at = egui::pos2(row.left() + across * row.width(), row.top() + 30.0);
        click(&mut harness, at, egui::Modifiers::NONE);
        let seconds = anchor_seconds(&harness);
        assert_eq!(seconds.len(), 3, "{seconds:?}");
        assert!((seconds[2] - 0.95).abs() < 5e-3, "{seconds:?}");
        assert!(
            (seconds[1] - 0.5).abs() < 1e-9,
            "the other anchors keep their moments"
        );
        harness.key_press(egui::Key::V);
        harness.run();
        assert!(harness.query_by_label("Measure 0").is_some());
    }
    #[test]
    fn ctrl_t_opens_and_closes_the_timing_window() {
        let mut harness = timed_song();
        assert!(harness.query_by_label("Row at 0.0 s").is_none());
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
        harness.run();
        assert!(harness.query_by_label("Row at 0.0 s").is_some());
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
        harness.run();
        assert!(harness.query_by_label("Row at 0.0 s").is_none());
    }
    #[test]
    fn clicking_a_row_adds_an_anchor_pinning_the_nearest_grid_line_to_the_moment_clicked() {
        let mut harness = syncing();
        let heard = seconds_at(&harness, 1950);
        let near = in_row(&harness, 1950);
        click(&mut harness, near, egui::Modifiers::NONE);
        assert_eq!(
            event_ticks(&harness),
            [Tick::new(0), Tick::new(960), Tick::new(1920)]
        );
        assert!((seconds_at(&harness, 1920) - heard).abs() < 5e-3);
        assert!((seconds_at(&harness, 960) - 0.5).abs() < 1e-9);
    }
    #[test]
    fn a_click_ten_ms_from_an_onset_anchors_on_it_and_auditions_a_beat_before() {
        // Tick 1920 falls 0.9 s in; the onset is a little later.
        let mut harness = syncing_with_an_onset(0.912);
        let pos = in_row(&harness, 1920);
        assert!((seconds_at(&harness, 1920) - 0.9).abs() < 1e-9);
        click(&mut harness, pos, egui::Modifiers::NONE);
        let seconds = anchor_seconds(&harness);
        assert_eq!(event_ticks(&harness)[2], Tick::new(1920));
        assert!((seconds[2] - 0.912).abs() < 1e-4, "{seconds:?}");
        let requests = harness.state_mut().take_requests();
        let from = requests.iter().find_map(|request| {
            if let Request::Play { from, .. } = request {
                Some(*from)
            } else {
                None
            }
        });
        assert!((from.expect("an audition").seconds() - (0.912 - 0.4)).abs() < 1e-6);
    }
    #[test]
    fn a_click_forty_ms_from_any_onset_stays_where_it_was_clicked() {
        let mut harness = syncing_with_an_onset(0.94);
        let heard = seconds_at(&harness, 1920);
        let pos = in_row(&harness, 1920);
        click(&mut harness, pos, egui::Modifiers::NONE);
        let seconds = anchor_seconds(&harness);
        assert!((seconds[2] - heard).abs() < 5e-3, "{seconds:?}");
    }
    #[test]
    fn an_anchor_is_not_added_within_half_a_grid_step_of_another_and_says_why() {
        let mut harness = syncing();
        let near = in_row(&harness, 1150);
        click(&mut harness, near, egui::Modifiers::NONE);
        assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
        assert!(
            harness
                .query_by_label_contains("Too close to the anchor")
                .is_some()
        );
    }
    #[test]
    fn dragging_an_anchor_moves_it_on_the_grid_keeping_its_moment() {
        let mut harness = syncing();
        let (from, to) = (in_row(&harness, 960), in_row(&harness, 1900));
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(1920)]);
        assert!((seconds_at(&harness, 1920) - 0.5).abs() < 1e-6);
    }
    #[test]
    fn ctrl_dragging_an_anchor_moves_it_in_the_audio_keeping_its_beat() {
        let mut harness = syncing();
        let heard = seconds_at(&harness, 1200);
        let (from, to) = (in_row(&harness, 960), in_row(&harness, 1200));
        drag(&mut harness, from, to, egui::Modifiers::COMMAND);
        assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
        assert!((seconds_at(&harness, 960) - heard).abs() < 5e-3);
    }
    #[test]
    fn an_anchor_dragged_into_its_neighbour_stops_short_of_it_in_one_step() {
        let mut harness = syncing();
        // Into the tenth of the bar before, shown ahead of the first downbeat.
        let (from, to) = (
            in_row(&harness, 960),
            in_row(&harness, 0) - egui::vec2(30.0, 0.0),
        );
        drag(&mut harness, from, to, egui::Modifiers::COMMAND);
        let seconds = anchor_seconds(&harness);
        assert!(seconds[1] > seconds[0] && seconds[1] < 0.01, "{seconds:?}");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert!((anchor_seconds(&harness)[1] - 0.5).abs() < 1e-9);
    }
    #[test]
    fn dragging_across_a_row_selects_the_anchors_inside_and_delete_removes_them() {
        let mut harness = syncing();
        let (from, to) = (in_row(&harness, 200), in_row(&harness, 1500));
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
        harness.key_press(egui::Key::Delete);
        harness.run();
        assert_eq!(event_ticks(&harness), [Tick::new(0)]);
    }
    #[test]
    fn a_double_click_on_an_anchor_opens_its_moment_to_type_not_a_removal() {
        let mut harness = syncing();
        harness
            .ctx
            .options_mut(|options| options.input_options.max_double_click_delay = 2.0);
        let at = in_row(&harness, 960);
        harness.hover_at(at);
        for _ in 0..2 {
            press(&harness, at, true, egui::Modifiers::NONE);
            press(&harness, at, false, egui::Modifiers::NONE);
            harness.step();
        }
        harness.run();
        harness.run();
        assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
        let field = harness.get_by_role_and_label(Role::TextInput, "t");
        assert_eq!(field.value().as_deref(), Some("0.5000"));
        field.type_text("0.6");
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness
            .get_by_role_and_label(Role::TextInput, "t")
            .type_text("0.6");
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert!((anchor_seconds(&harness)[1] - 0.6).abs() < 1e-6);
    }
    #[test]
    fn ctrl_wheel_nudges_the_focused_anchor_by_a_millisecond() {
        let mut harness = syncing();
        let at = in_row(&harness, 960);
        click(&mut harness, at, egui::Modifiers::NONE);
        harness.hover_at(in_row(&harness, 1000));
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, 1.0),
            modifiers: egui::Modifiers::COMMAND,
            phase: egui::TouchPhase::Move,
        });
        harness.run();
        assert!((seconds_at(&harness, 960) - 0.501).abs() < 1e-6);
    }
    #[test]
    fn twenty_arrow_presses_nudge_twenty_milliseconds_as_one_step() {
        let mut harness = syncing();
        let at = in_row(&harness, 960);
        click(&mut harness, at, egui::Modifiers::NONE);
        for _ in 0..20 {
            harness.key_press(egui::Key::ArrowRight);
        }
        harness.run();
        assert!((anchor_seconds(&harness)[1] - 0.52).abs() < 1e-6);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert!((anchor_seconds(&harness)[1] - 0.5).abs() < 1e-9);
    }
    #[test]
    fn number_keys_set_the_grid_while_the_timing_window_has_the_keyboard() {
        let mut harness = syncing();
        let at = in_row(&harness, 960);
        click(&mut harness, at, egui::Modifiers::NONE);
        harness.key_press(egui::Key::Num8);
        harness.run();
        assert_eq!(harness.state().sync.grid().resolution.get(), 16);
        assert!(lanes_at(&harness, 0).is_empty());
        let grid = harness.get_by_role_and_label(Role::ComboBox, "Grid");
        assert_eq!(grid.value().as_deref(), Some("1/16"));
    }
    #[test]
    fn the_end_of_a_row_in_the_timing_window_is_clear_of_its_scroll_bar() {
        // The demo runs to more bars than the window shows, so its rows scroll.
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
        harness.run();
        harness
            .state_mut()
            .sync
            .set_layout(charter_timing::rows::Layout::Bars);
        harness.run();
        let heard = seconds_at(&harness, 3830);
        let at = in_row(&harness, 3830);
        click(&mut harness, at, egui::Modifiers::NONE);
        assert!(
            event_ticks(&harness).contains(&Tick::new(3840)),
            "{:?}",
            event_ticks(&harness)
        );
        assert!((seconds_at(&harness, 3840) - heard).abs() < 5e-3);
    }
    #[test]
    #[expect(
        clippy::non_ascii_literal,
        reason = "the test reopens the Import timing menu label with its ellipsis"
    )]
    fn reopening_import_after_opening_another_game_previews_for_that_song() {
        let draft = charter_core::NewSong {
            metadata: charter_editor::demo::song().document().metadata.clone(),
            layout: 0,
            charts: vec![charter_core::NewChart::named("Easy")],
        };
        let first = charter_stepmania::Plugin.new_song(&draft).unwrap();
        let second = charter_ttr::Plugin.new_song(&draft).unwrap();
        let mut app = Window::new(CONTEXT_IMPORTING);
        app.open(
            Path::new("/songs/stepmania.json"),
            &first.to_json().unwrap(),
        );
        let mut harness = window(app);

        harness.state_mut().timing_file_read("0.895");
        harness.run();
        assert!(
            harness
                .query_by_label_contains("150 BPM at the start")
                .is_some()
        );
        harness.get_by_label("Close window").click();
        harness.run();
        harness
            .state_mut()
            .open(Path::new("/songs/ttr.json"), &second.to_json().unwrap());
        harness.run();
        harness.query_all_by_label("Timing").next().unwrap().click();
        harness.run();
        harness.get_by_label("Import timing…").click();
        harness.run();

        assert!(
            harness
                .query_by_label_contains("208.1 BPM at the start")
                .is_some()
        );
        harness.get_by_label("Replace the timing").click();
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((0, "208.1".to_owned())));
    }
    #[test]
    fn imported_timing_shows_what_it_holds_and_replaces_the_songs_in_one_step() {
        let mut harness = importing();
        assert!(harness.query_by_label_contains("test timing").is_some());
        harness.state_mut().timing_file_read("0.895,208.1");
        harness.run();
        assert!(harness.query_by_label_contains("208.1 BPM").is_some());
        assert!(harness.query_by_label_contains("895 ms").is_some());
        assert!(harness.query_by_label_contains("kiai time").is_some());
        harness.get_by_label("Replace the timing").click();
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((0, "208.1".to_owned())));
        assert_eq!(offset(&harness), serde_json::json!(0.895));
        assert!(harness.query_by_label("Replace the timing").is_none());
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
    }
    #[test]
    fn timing_that_does_not_read_says_why_and_imports_nothing() {
        let mut harness = importing();
        harness.state_mut().timing_file_read("nonsense");
        harness.run();
        assert!(harness.query_by_label_contains("no comma").is_some());
        harness.get_by_label("Replace the timing").click();
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
    }
    #[test]
    #[expect(
        clippy::non_ascii_literal,
        reason = "the test clicks the Read a file button with its ellipsis"
    )]
    fn the_import_window_reads_timing_from_a_file_the_host_opens() {
        let mut harness = importing();
        harness.get_by_label("Read a file…").click();
        harness.run();
        assert_eq!(
            harness.state_mut().take_requests(),
            [Request::ChooseTimingFile]
        );
    }
    #[test]
    fn the_import_window_can_be_cancelled_without_importing() {
        let mut harness = importing();
        harness.state_mut().timing_file_read("0.895,208.1");
        harness.run();

        harness.get_by_label("Cancel").click();
        harness.run();

        assert!(harness.query_by_label("Replace the timing").is_none());
        assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
    }
    #[test]
    fn dragging_a_tempo_point_moves_it_to_the_moment_under_the_pointer() {
        let mut harness = timed_song();
        let before = moments(&harness);
        let heard = seconds_at(&harness, 1920);
        let x = field(&harness).columns.timing_x();
        let (from, to) = (on_line(&harness, x, 960), on_line(&harness, x, 1920));
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
        let after = moments(&harness);
        assert!((after[1] - heard).abs() < 0.01, "{after:?} {heard}");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(moments(&harness), before);
    }
    #[test]
    fn a_bpm_change_and_a_meter_at_one_tick_drag_as_one_timing_point() {
        let mut harness = timed_song();
        let meter = serde_json::json!({ "tick": 960, "kind": { "time_signature": [3, 4] } });
        let mut document = harness
            .state()
            .song
            .as_ref()
            .unwrap()
            .document()
            .document()
            .clone();
        document
            .timing
            .events
            .push(serde_json::from_value(meter).unwrap());
        let bytes = serde_json::to_vec(&document).unwrap();
        harness.state_mut().open(Path::new("timed.json"), &bytes);
        harness.run();
        let x = field(&harness).columns.timing_x();
        let (from, to) = (on_line(&harness, x, 960), on_line(&harness, x, 1920));
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        assert_eq!(
            event_ticks(&harness),
            [Tick::new(0), Tick::new(960), Tick::new(960)]
        );
    }
    #[test]
    fn dragging_the_first_tempo_point_moves_the_whole_grid() {
        let mut harness = timed_song();
        let before = moments(&harness);
        let x = field(&harness).columns.timing_x();
        let (from, to) = (on_line(&harness, x, 0), on_line(&harness, x, 480));
        let later = seconds_at(&harness, 480) - seconds_at(&harness, 0);
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
        let after = moments(&harness);
        for (was, is) in before.iter().zip(&after) {
            assert!((is - was - later).abs() < 0.01, "{before:?} {after:?}");
        }
    }
    #[test]
    fn the_handles_of_a_read_only_song_stay_put() {
        let mut harness = timed_song();
        harness.state_mut().plugins = &[];
        let song = harness.state().song.as_ref().unwrap().document().clone();
        harness.state_mut().song = Some(Song::new(song, &[]));
        harness.run();
        let x = field(&harness).columns.timing_x();
        let (from, to) = (on_line(&harness, x, 960), on_line(&harness, x, 1920));
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
    }
    #[test]
    fn an_edit_the_game_refuses_says_why() {
        const PICKY: &[&dyn RulesetPlugin] = &[&NoFourthLane];
        let small = serde_json::json!({
            "format_version": 0,
            "ruleset": { "id": "sm", "version": "1" },
            "metadata": { "title": "Small", "artist": "a", "audio": "a.ogg" },
            "timing": { "offset_seconds": 0.0, "events": [ { "tick": 0, "kind": { "bpm": 120.0 } } ] },
            "charts": [ { "name": "Easy", "lanes": 4 } ]
        });
        let mut app = Window::new(PICKY);
        app.open(
            Path::new("small.json"),
            &serde_json::to_vec(&small).unwrap(),
        );
        let mut harness = window(app);
        harness.key_press(egui::Key::Num4);
        harness.run();
        assert!(
            harness
                .query_by_label("Refused: a note on lane 4")
                .is_some()
        );
        assert_eq!(lanes_at(&harness, 0), Vec::<u16>::new());
    }
    #[test]
    fn tags_for_a_title_already_typed_are_offered_rather_than_taken() {
        let mut harness = new_song_dialog();
        let title = harness.get_by_role_and_label(Role::TextInput, "Title");
        title.focus();
        title.type_text("Mine");
        harness.run();
        drop_file(&mut harness, "/music/track.ogg");
        read_tags(&mut harness, "/music/track.ogg", "Starlight", "The Lanes");
        assert!(harness.query_by_label_contains(OFFER).is_some());
        harness.get_by_label("Keep mine").click();
        harness.run();
        assert_eq!(field_value(&harness, "Title").as_deref(), Some("Mine"));
        read_tags(&mut harness, "/music/track.ogg", "Starlight", "The Lanes");
        harness.get_by_label("Import").click();
        harness.run();
        assert_eq!(field_value(&harness, "Title").as_deref(), Some("Starlight"));
    }
    #[test]
    fn a_song_with_a_title_of_its_own_asks_before_taking_the_tags() {
        let mut harness = editable_four_taps();
        drop_file(&mut harness, "/music/track.ogg");
        read_tags(&mut harness, "/music/track.ogg", "Starlight", "The Lanes");
        assert_eq!(facts(&harness), ("Four".to_owned(), "a".to_owned()));
        harness.get_by_label("Import").click();
        harness.run();
        assert_eq!(
            facts(&harness),
            ("Starlight".to_owned(), "The Lanes".to_owned())
        );
    }
    #[test]
    fn artist_tags_are_offered_even_when_the_song_title_is_unchanged() {
        let mut harness = editable_four_taps();
        drop_file(&mut harness, "/music/track.ogg");
        read_tags(&mut harness, "/music/track.ogg", "Four", "New artist");
        assert!(harness.query_by_label_contains(OFFER).is_some());
        harness.get_by_label("Import").click();
        harness.run();
        assert_eq!(
            facts(&harness),
            ("Four".to_owned(), "New artist".to_owned())
        );
    }
    #[test]
    fn the_offset_is_typed_in_milliseconds() {
        let mut harness = editable_demo();
        assert_eq!(offset_field(&harness).value().as_deref(), Some("0"));
        enter_offset(&mut harness, "1500.25");
        assert_eq!(offset(&harness), serde_json::json!(1.50025));
        assert_eq!(offset_field(&harness).value().as_deref(), Some("1500.25"));
    }
    #[test]
    fn an_offset_that_is_not_a_time_says_so() {
        let mut harness = editable_demo();
        enter_offset(&mut harness, "soon");
        assert!(
            harness
                .query_by_label_contains("soon is not an offset in milliseconds")
                .is_some()
        );
    }
    #[test]
    fn alt_arrows_and_the_offset_steppers_nudge_it_by_a_millisecond_or_ten_with_shift() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowRight);
        harness.run();
        assert_eq!(offset(&harness), serde_json::json!(0.001));
        let coarse = egui::Modifiers::ALT.plus(egui::Modifiers::SHIFT);
        harness.key_press_modifiers(coarse, egui::Key::ArrowLeft);
        harness.run();
        assert_eq!(offset(&harness), serde_json::json!(-0.009));
        harness.get_by_label("Raise the offset by 1 ms").click();
        harness.run();
        assert_eq!(offset(&harness), serde_json::json!(-0.008));
        harness.get_by_label("Lower the offset by 1 ms").click();
        harness.run();
        assert_eq!(offset(&harness), serde_json::json!(-0.009));
    }
    #[test]
    fn the_tempo_section_opens_the_sync_window() {
        let mut harness = editable_demo();
        click_in_panel(&mut harness, "Sync to the audio");
        assert!(harness.query_by_label("Row at 0.0 s").is_some());
    }
    #[test]
    fn a_read_only_song_refuses_edits_from_actions_without_changing_its_document() {
        let mut app = Window::new(&[]);
        app.open(Path::new("four.json"), &four_taps());
        let before = app.song.as_ref().unwrap().document().to_json().unwrap();
        for action in [
            Action::Place {
                tick: Tick::new(1920),
                lane: LaneIndex::new(0),
                note: Note::Tap,
            },
            Action::SetSection("Verse".to_owned()),
            Action::ToggleBookmark,
            Action::NudgeBpm(1.0),
            Action::SetOffset(charter_core::Origin::try_from(0.1).unwrap()),
        ] {
            app.apply(action);
            assert!(
                app.message()
                    .is_some_and(|message| message.starts_with("Refused:"))
            );
            assert_eq!(
                app.song.as_ref().unwrap().document().to_json().unwrap(),
                before
            );
        }
        assert!(!app.song.as_ref().unwrap().is_modified());
    }
    #[test]
    fn a_failed_timing_search_keeps_the_original_timing_and_stops_searching() {
        let mut harness = playable();
        let before = anchor_seconds(&harness);
        find_from_the_menu(&mut harness);
        harness.state_mut().take_requests();
        harness
            .state_mut()
            .timing_found(Err("no beats detected".to_owned()));
        harness.run();
        assert_eq!(anchor_seconds(&harness), before);
        assert!(
            harness
                .query_by_label_contains("no beats detected")
                .is_some()
        );
        assert!(
            harness
                .query_by_label_contains("Finding the timing")
                .is_none()
        );
    }
    #[test]
    fn a_timing_found_for_a_song_since_left_leaves_the_open_song_alone() {
        let mut harness = playable();
        find_from_the_menu(&mut harness);
        harness.state_mut().take_requests();
        harness
            .state_mut()
            .open(Path::new("/other/four.json"), &four_taps());
        harness.run();
        let before = anchor_seconds(&harness);
        let found = serde_json::from_value(serde_json::json!({
            "anchors": [{ "tick": 0, "seconds": 1.0 }],
            "tempo": 120.0,
            "events": []
        }))
        .unwrap();
        harness.state_mut().timing_found(Ok(found));
        harness.run();
        assert_eq!(anchor_seconds(&harness), before);
    }
    #[test]
    fn a_timing_result_for_a_song_that_cannot_be_edited_is_refused() {
        let mut app = Window::new(&[]);
        app.open(Path::new("four.json"), &four_taps());
        let before = app.song.as_ref().unwrap().document().to_json().unwrap();
        let found = serde_json::from_value(serde_json::json!({
            "anchors": [{ "tick": 0, "seconds": 1.0 }],
            "tempo": 120.0,
            "events": []
        }))
        .unwrap();
        app.find_timing(Tracker::BeatThis);
        app.timing_found(Ok(found));
        assert!(
            app.message()
                .is_some_and(|message| message.contains("timing found was refused"))
        );
        assert_eq!(
            app.song.as_ref().unwrap().document().to_json().unwrap(),
            before
        );

        let mut empty = Window::new(PLUGINS);
        empty.weights_needed();
        empty.timing_found(Ok(fresh_timing()));
        assert_eq!(empty.dialogs.finding, Finding::Idle);
        assert!(empty.song.is_none());
    }
    #[test]
    fn a_timing_file_read_opens_the_import_and_a_read_failure_explains_it() {
        let mut app = Window::new(PLUGINS);
        app.timing_file_read("not timing");
        assert!(app.import.is_open());
        assert_eq!(app.import.text, "not timing");
        app.timing_file_failed("permission denied");
        assert_eq!(
            app.message(),
            Some("Could not read the timing: permission denied")
        );
    }
}
