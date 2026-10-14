use super::entry::{entry_sized, shown};
use super::layers::{layer_menu, layers};
use super::{
    Action, ChartIndex, LayerIndex, Metadata, ResolvedTime, ResourcePath, SettingScope, Song, Ui,
    egui, fields, icon, style, table,
};
use charter_core::LayerName;

/// The song panel's Song and Charts sections. `artwork` is the song's artwork to preview, or why
/// there is none to show, when the song names some.
pub(crate) fn song_panel(
    ui: &mut Ui,
    song: &Song,
    (drafts, artwork): (&mut SongDrafts, Option<Result<&egui::TextureHandle, &str>>),
    actions: &mut Vec<Action>,
) -> Option<String> {
    let document = song.document().document();
    let mut refused = None;
    style::section(ui, "Song", |ui| {
        table(ui, "song", |ui| {
            refused = song_rows(ui, song, (drafts, artwork), actions);
        });
    });
    style::section(ui, "Charts", |ui| {
        if document.charts.is_empty() {
            ui.weak("No charts yet");
        }
        table(ui, "charts", |ui| {
            for (index, chart) in document.charts.iter().enumerate() {
                let index = ChartIndex::new(index);
                let open = index == song.chart_index();
                if ui.selectable_label(open, &chart.name).clicked() {
                    actions.push(Action::OpenChart(index));
                }
                ui.weak(
                    chart
                        .lanes
                        .map_or_else(|| "no lanes".to_owned(), |lanes| format!("{lanes} lanes")),
                );
                ui.end_row();
            }
            let offered = setting_rows(ui, song, SettingScope::Chart, drafts, actions);
            refused = refused.take().or(offered);
        });
    });
    if song.chart().is_some() {
        style::section_with_actions(ui, "Layers", |ui, header| {
            if header {
                let read_only = song.read_only().map(|why| format!("Read only: {why}"));
                ui.menu_button(style::glyph(ui, icon::DOTS_THREE), |ui| {
                    layer_menu(ui, song, song.active_layer(), drafts, actions);
                })
                .response
                .widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Layer actions")
                });
                if ui
                    .add_enabled_ui(read_only.is_none(), |ui| {
                        style::quiet_icon(ui, icon::PLUS, "Add layer", "Add layer (Ctrl+Shift+N)")
                    })
                    .inner
                    .on_disabled_hover_text(read_only.as_deref().unwrap_or_default())
                    .clicked()
                {
                    actions.push(Action::AddLayer);
                }
            } else {
                layers(ui, song, drafts, actions);
            }
        });
    }
    refused
}

/// What is being typed into the song section's fields, while one has the keyboard.
#[derive(Debug, Default)]
pub(crate) struct SongDrafts {
    /// Name being typed for a layer, kept when the rename is refused.
    pub(crate) layer_name: Option<(LayerIndex, String)>,
    /// Restore the rename field's keyboard focus after a refusal.
    pub(crate) refocus_layer_name: bool,
    /// Layer whose notes need a deletion decision.
    pub(super) deleting_layer: Option<LayerName>,
    /// Title being edited, while its field has the keyboard.
    pub(super) title: Option<String>,
    /// Artist being edited, while its field has the keyboard.
    pub(super) artist: Option<String>,
    /// Genre being edited, while its field has the keyboard.
    genre: Option<String>,
    /// Artwork file name being edited, while its field has the keyboard.
    pub(super) artwork: Option<String>,
    /// The game's text settings, by whether they are the chart's, and key.
    settings: std::collections::HashMap<(bool, String), Option<String>>,
    /// Preview start being edited, while its field has the keyboard.
    pub(super) preview: Option<String>,
    /// Audio length being edited, while its field has the keyboard.
    pub(super) length: Option<String>,
}

/// How wide the song section's text fields are.
pub(super) const WIDE: f32 = 180.0;

/// The song's title, artist, audio, preview and ruleset, each that can change a field of its
/// own. Says why something typed is not a value.
#[expect(clippy::too_many_lines, reason = "one row per fact")]
fn song_rows(
    ui: &mut Ui,
    song: &Song,
    (drafts, preview): (&mut SongDrafts, Option<Result<&egui::TextureHandle, &str>>),
    actions: &mut Vec<Action>,
) -> Option<String> {
    let document = song.document().document();
    let metadata = &document.metadata;
    let editable = song.read_only().is_none();
    let mut refused = None;
    let mut text_row = |row_ui: &mut Ui,
                        label: &str,
                        current: &str,
                        draft: &mut Option<String>,
                        set: fn(&mut Metadata, &str)| {
        let caption = row_ui.weak(label);
        if editable {
            let read = |typed: &str| Ok(changed(metadata, |new| set(new, typed)));
            let offered = entry_sized(row_ui, (caption.id, WIDE), current, draft, read, actions);
            refused = refused.take().or(offered);
        } else {
            row_ui.label(current);
        }
        row_ui.end_row();
    };
    text_row(
        ui,
        "Title",
        &metadata.title,
        &mut drafts.title,
        |new, typed| {
            typed.clone_into(&mut new.title);
        },
    );
    text_row(
        ui,
        "Artist",
        &metadata.artist,
        &mut drafts.artist,
        |new, typed| {
            typed.clone_into(&mut new.artist);
        },
    );
    text_row(
        ui,
        "Genre",
        metadata.genre.as_deref().unwrap_or_default(),
        &mut drafts.genre,
        |new, typed| new.genre = (!typed.is_empty()).then(|| typed.to_owned()),
    );
    ui.weak("Audio");
    ui.horizontal(|ui| {
        ui.label(audio_name(metadata.audio.as_str()));
        if editable
            && style::quiet_icon(
                ui,
                icon::FOLDER_OPEN,
                "Choose the song's audio",
                "Choose another audio file",
            )
            .clicked()
        {
            actions.push(Action::ChooseAudio);
        }
    });
    ui.end_row();
    let caption = ui
        .weak("Artwork")
        .on_hover_text("The song's picture, a file beside the song, such as artwork.png");
    let artwork = metadata.artwork.as_ref().map(ToString::to_string);
    let artwork = artwork.as_deref().unwrap_or_default();
    if editable {
        let read = |typed: &str| {
            if typed.is_empty() {
                return Ok(changed(metadata, |new| new.artwork = None));
            }
            let path = ResourcePath::new(typed)
                .map_err(|_invalid_path| format!("{typed} is not a file beside the song"))?;
            Ok(changed(metadata, |new| new.artwork = Some(path)))
        };
        ui.horizontal(|ui| {
            let offered = entry_sized(
                ui,
                (caption.id, WIDE - 28.0),
                artwork,
                &mut drafts.artwork,
                read,
                actions,
            );
            refused = refused.take().or(offered);
            if style::quiet_icon(
                ui,
                icon::FOLDER_OPEN,
                "Choose the song's artwork",
                "Choose a PNG or JPEG picture",
            )
            .clicked()
            {
                actions.push(Action::ChooseArtwork);
            }
        });
    } else {
        ui.label(if artwork.is_empty() { "none" } else { artwork });
    }
    ui.end_row();
    if let Some(preview) = preview {
        ui.label("");
        match preview {
            Ok(texture) => {
                let image = egui::Image::new(texture)
                    .max_size(egui::vec2(WIDE, 120.0))
                    .corner_radius(4);
                ui.add(image).on_hover_text(artwork);
            }
            Err(why) => {
                ui.weak(why);
            }
        }
        ui.end_row();
    }
    let offered = preview_rows(ui, (metadata, editable), drafts, actions);
    refused = refused.take().or(offered);
    ui.weak("Ruleset");
    ui.label(format!(
        "{} {}",
        document.ruleset.id, document.ruleset.version
    ));
    ui.end_row();
    let settings_offered = setting_rows(ui, song, SettingScope::Song, drafts, actions);
    refused.take().or(settings_offered)
}

/// The display name of `audio`, substituting a label for the new-song placeholder.
fn audio_name(audio: &str) -> &str {
    if audio == crate::new_song::NO_AUDIO {
        "None yet"
    } else {
        audio
    }
}

/// A field for each of the game's text settings in `scope`, the open chart's for a chart scope,
/// each clearing its key when cleared. Says why something typed is not a value.
fn setting_rows(
    ui: &mut Ui,
    song: &Song,
    scope: SettingScope,
    drafts: &mut SongDrafts,
    actions: &mut Vec<Action>,
) -> Option<String> {
    let plugin = song.plugin()?;
    let document = song.document().document();
    let settings = match scope {
        SettingScope::Song => &document.settings,
        SettingScope::Chart => &document.charts.get(song.chart_index().get())?.settings,
    };
    let mut refused = None;
    for setting in plugin
        .text_settings()
        .into_iter()
        .filter(|setting| setting.scope == scope)
    {
        let name = match scope {
            SettingScope::Song => setting.name,
            SettingScope::Chart => format!("Chart {}", setting.name.to_lowercase()),
        };
        let caption = ui.weak(&name).on_hover_text(&setting.about);
        let current = settings
            .get(&setting.key)
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let draft = drafts
            .settings
            .entry((scope == SettingScope::Chart, setting.key.clone()))
            .or_default();
        let read = |typed: &str| {
            Ok(Action::SetSetting {
                scope,
                key: setting.key.clone(),
                value: (!typed.is_empty()).then(|| typed.to_owned()),
            })
        };
        let offered = entry_sized(ui, (caption.id, WIDE), current, draft, read, actions);
        refused = refused.take().or(offered);
        ui.end_row();
    }
    refused
}

/// `metadata` changed by `change`, as the action that gives the song it.
fn changed(metadata: &Metadata, change: impl FnOnce(&mut Metadata)) -> Action {
    let mut new = metadata.clone();
    change(&mut new);
    Action::SetMetadata(Box::new(new))
}

/// Where a song list starts playing the song, and for how long once it has a start. Clearing the
/// start takes the preview away. Says why something typed is not a value.
fn preview_rows(
    ui: &mut Ui,
    (metadata, editable): (&Metadata, bool),
    drafts: &mut SongDrafts,
    actions: &mut Vec<Action>,
) -> Option<String> {
    let mut refused = None;
    let caption = ui
        .weak("Preview")
        .on_hover_text("Where a song list starts playing the song. Clear it for none.");
    let start = metadata.preview.map_or_else(String::new, |preview| {
        style::clock(ResolvedTime::from(preview.start_seconds).seconds())
    });
    if editable {
        let read = |typed: &str| {
            if typed.is_empty() {
                return Ok(changed(metadata, |new| new.preview = None));
            }
            let new_start = crate::timing_panel::moment(typed)
                .and_then(|at| charter_core::AudioTime::nearest(at).ok())
                .ok_or_else(|| format!("{typed} is not a time in the audio, such as 00:45.000"))?;
            Ok(changed(metadata, |new| {
                let length_seconds = new.preview.and_then(|preview| preview.length_seconds);
                new.preview = Some(charter_core::Preview {
                    start_seconds: new_start,
                    length_seconds,
                });
            }))
        };
        let offered = entry_sized(
            ui,
            (caption.id, 72.0),
            &start,
            &mut drafts.preview,
            read,
            actions,
        );
        refused = refused.take().or(offered);
    } else {
        ui.label(if start.is_empty() {
            "none".to_owned()
        } else {
            start
        });
    }
    ui.end_row();
    if let Some(preview) = metadata.preview {
        let length_caption = ui.weak("Preview length");
        let length = preview
            .length_seconds
            .map_or_else(String::new, |length| length.to_string());
        if editable {
            ui.horizontal(|ui| {
                let read = |typed: &str| {
                    if typed.is_empty() {
                        return Ok(changed(metadata, |new| {
                            new.preview = Some(charter_core::Preview {
                                length_seconds: None,
                                ..preview
                            });
                        }));
                    }
                    let new_length = typed
                        .parse::<f64>()
                        .ok()
                        .and_then(|seconds| charter_core::Seconds::try_from(seconds).ok())
                        .ok_or_else(|| format!("{typed} is not a length in seconds"))?;
                    Ok(changed(metadata, |new| {
                        new.preview = Some(charter_core::Preview {
                            length_seconds: Some(new_length),
                            ..preview
                        });
                    }))
                };
                let offered = entry_sized(
                    ui,
                    (length_caption.id, 72.0),
                    &length,
                    &mut drafts.length,
                    read,
                    actions,
                );
                refused = refused.take().or(offered);
                ui.weak("s");
            });
        } else {
            ui.label(if length.is_empty() {
                "to the end".to_owned()
            } else {
                format!("{length} s")
            });
        }
        ui.end_row();
    }
    refused
}

/// What is read more than changed while charting: the open chart's settings that the Charts
/// section has no field for, and the song's keysound bank, folded away until wanted.
pub(crate) fn details(ui: &mut Ui, song: &Song) {
    if let Some(chart) = song.chart() {
        let with_fields: Vec<_> = song.plugin().map_or_else(Vec::new, |plugin| {
            plugin
                .text_settings()
                .into_iter()
                .filter(|setting| setting.scope == SettingScope::Chart)
                .map(|setting| setting.key)
                .collect()
        });
        let settings: Vec<_> = chart
            .settings
            .iter()
            .filter(|(key, _)| !with_fields.contains(key))
            .map(|(key, value)| (key.as_str(), shown(value)))
            .collect();
        if !settings.is_empty() {
            style::folded_section(ui, "Chart settings", |ui| {
                fields(ui, "settings", &settings, "No chart settings");
            });
        }
    }
    style::folded_section(ui, "Samples", |ui| {
        let samples: Vec<_> = song
            .document()
            .document()
            .samples
            .iter()
            .map(|(id, path)| (id.as_str(), path.to_string()))
            .collect();
        fields(ui, "samples", &samples, "No samples");
    });
}

#[cfg(test)]
mod tests {
    use charter_core::RulesetPlugin;
    use egui::accesskit::{Role, Toggled};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};

    use super::super::*;
    use super::*;

    #[test]
    fn shortcuts_filter_stays_visible_in_a_narrow_window() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build_ui_state(
                |ui, (open, filter): &mut (bool, String)| {
                    shortcuts(ui.ctx(), open, filter);
                },
                (true, String::new()),
            );
        harness.run();
        let filter = harness.get_by_label("Filter").rect();
        assert!(filter.left() >= 0.0, "{filter:?}");
        assert!(filter.right() <= 400.0, "{filter:?}");
    }

    fn note(n: u16) -> NoteValue {
        NoteValue::new(n).unwrap()
    }

    #[test]
    fn snaps_read_as_ordinals() {
        let names: Vec<_> = [4, 12, 16, 32, 48, 192].map(|n| ordinal(note(n))).into();
        assert_eq!(names, ["4th", "12th", "16th", "32nd", "48th", "192nd"]);
    }

    #[test]
    fn a_song_waiting_for_audio_shows_no_filename_placeholder() {
        let bytes = charter_editor::demo::song().to_json().unwrap();
        let bytes = String::from_utf8(bytes)
            .unwrap()
            .replace("demo.ogg", crate::new_song::NO_AUDIO);
        let song = Song::open(bytes.as_bytes(), &[]).unwrap();
        let mut harness = Harness::builder().build_ui_state(
            |ui, song: &mut Song| {
                song_panel(
                    ui,
                    song,
                    (&mut SongDrafts::default(), None),
                    &mut Vec::new(),
                );
            },
            song,
        );
        harness.run();
        assert!(harness.query_by_label("None yet").is_some());
        assert!(harness.query_by_label(crate::new_song::NO_AUDIO).is_none());
    }

    #[test]
    fn expanded_chart_settings_and_samples_show_their_values() {
        let mut document: serde_json::Value =
            serde_json::from_slice(&charter_editor::demo::song().to_json().unwrap()).unwrap();
        document["charts"][0]["settings"] = serde_json::json!({"level": 7});
        document["samples"]["drum"] = serde_json::json!("kick.wav");
        let song = Song::open(&serde_json::to_vec(&document).unwrap(), &[]).unwrap();
        let mut harness =
            Harness::builder().build_ui_state(|ui, song: &mut Song| details(ui, song), song);
        harness.run();
        harness.get_by_label("Chart settings").click();
        harness.run();
        harness.get_by_label("Samples").click();
        harness.run();
        assert!(harness.query_by_label("7").is_some());
        assert!(harness.query_by_label("kick.wav").is_some());
    }

    /// A Tap Tap Revenge song of one chart called `name`, of the standard `level` if any.
    fn ttr_song(name: &str, level: Option<usize>) -> Song {
        let document = charter_ttr::Plugin
            .new_song(&charter_core::NewSong {
                metadata: charter_editor::demo::song().document().metadata.clone(),
                layout: 0,
                charts: vec![charter_core::NewChart {
                    name: name.to_owned(),
                    level,
                }],
            })
            .unwrap();
        Song::open(&document.to_json().unwrap(), &[&charter_ttr::Plugin]).unwrap()
    }

    #[test]
    fn a_click_on_a_section_title_folds_it_and_screen_readers_hear_whether_it_is_open() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, song: &mut Song| {
                song_panel(
                    ui,
                    song,
                    (&mut SongDrafts::default(), None),
                    &mut Vec::new(),
                );
            },
            ttr_song("Extreme", Some(3)),
        );
        harness.run();
        let open = |panel: &Harness<'_, Song>| {
            panel
                .query_by_role_and_label(Role::DisclosureTriangle, "Song")
                .expect("the Song section's header")
                .accesskit_node()
                .data()
                .is_expanded()
        };
        assert_eq!(open(&harness), Some(true));
        // At its centre, on the title rather than the triangle.
        harness
            .get_by_role_and_label(Role::DisclosureTriangle, "Song")
            .click();
        harness.run();
        assert_eq!(open(&harness), Some(false));
        assert!(
            harness
                .query_by_role_and_label(Role::TextInput, "Title")
                .is_none()
        );
    }

    #[test]
    fn chart_settings_and_samples_are_headed_as_the_other_sections_are_and_start_folded() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, song: &mut Song| {
                song_panel(
                    ui,
                    song,
                    (&mut SongDrafts::default(), None),
                    &mut Vec::new(),
                );
                details(ui, song);
            },
            ttr_song("Extreme", Some(3)),
        );
        harness.run();
        let song = harness
            .query_by_label("Song")
            .expect("the Song section's header");
        assert_eq!(song.accesskit_node().data().is_expanded(), Some(true));
        for name in ["Chart settings", "Samples"] {
            let fold = harness.get_by_label(name);
            assert_eq!(fold.accesskit_node().role(), song.accesskit_node().role());
            assert_eq!(fold.rect().left(), song.rect().left(), "{name}");
            assert_eq!(fold.rect().height(), song.rect().height(), "{name}");
            assert_eq!(fold.accesskit_node().data().is_expanded(), Some(false));
        }
    }

    #[test]
    fn chart_settings_leave_out_those_the_charts_section_has_fields_for() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, song: &mut Song| details(ui, song),
            ttr_song("Extreme", Some(3)),
        );
        harness.run();
        harness.get_by_label("Chart settings").click();
        harness.run();
        assert!(harness.query_by_label("extreme").is_some());
        assert!(harness.query_by_label("theme").is_none());
    }

    #[test]
    fn a_chart_with_a_field_for_each_of_its_settings_has_no_chart_settings_section() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, song: &mut Song| details(ui, song),
            ttr_song("Mine", None),
        );
        harness.run();
        assert!(harness.query_by_label("Chart settings").is_none());
        assert!(harness.query_by_label("Samples").is_some());
    }

    #[test]
    fn a_recent_path_without_a_filename_is_shown_as_its_full_path() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, actions: &mut Vec<Action>| {
                open_recent(ui, &[std::path::PathBuf::from("/")], actions);
            },
            Vec::new(),
        );
        harness.run();
        harness.get_by_label("Open recent").click();
        harness.run();
        harness.get_by_label_contains("/").click();
        harness.run();
        assert_eq!(
            harness.state().as_slice(),
            [Action::OpenRecent(std::path::PathBuf::from("/"))]
        );
    }

    #[test]
    fn the_welcome_page_can_open_a_recent_path_without_a_filename() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, actions: &mut Vec<Action>| {
                style::apply(ui.ctx(), &[]);
                empty(ui, (false, &[], &[std::path::PathBuf::from("/")]), actions);
            },
            Vec::new(),
        );
        harness.run();
        harness.get_by_label("/").click();
        harness.run();
        assert_eq!(
            harness.state().as_slice(),
            [Action::OpenRecent(std::path::PathBuf::from("/"))]
        );
    }

    #[test]
    fn a_chart_without_lanes_is_named_as_such_in_the_song_panel() {
        let mut document: serde_json::Value =
            serde_json::from_slice(&charter_editor::demo::song().to_json().unwrap()).unwrap();
        document["charts"][0]["lanes"] = serde_json::Value::Null;
        document["charts"][0]["objects"] = serde_json::json!([]);
        let song = Song::open(&serde_json::to_vec(&document).unwrap(), &[]).unwrap();
        let mut harness = Harness::builder().build_ui_state(
            |ui, song: &mut Song| {
                song_panel(
                    ui,
                    song,
                    (&mut SongDrafts::default(), None),
                    &mut Vec::new(),
                );
            },
            song,
        );
        harness.run();
        assert!(harness.query_by_label("no lanes").is_some());
    }

    #[test]
    fn adding_a_marker_keeps_the_notefield_wide_enough_for_eight_lanes_at_800() {
        const PLUGINS: &[&dyn RulesetPlugin] = &[&charter_stepmania::Plugin];
        let game = &charter_stepmania::Plugin;
        let layout = game
            .layouts()
            .iter()
            .position(|layout| layout.name == "dance-double")
            .unwrap();
        let song = game
            .new_song(&charter_core::NewSong {
                metadata: charter_editor::demo::song().document().metadata.clone(),
                layout,
                charts: vec![charter_core::NewChart::named("Hard")],
            })
            .unwrap();
        let mut app = crate::Window::new(PLUGINS);
        app.open(
            std::path::Path::new("double.json"),
            &song.to_json().unwrap(),
        );
        let mut harness = Harness::builder()
            .with_size(egui::vec2(800.0, 600.0))
            .build_ui_state(|ui, app: &mut crate::Window| app.ui(ui), app);
        harness.run();
        let before = harness.get_by_label("Notefield").rect().width();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::B);
        harness.run();
        let after = harness.get_by_label("Notefield").rect().width();
        // The ruler, the narrowest timing gutter and eight of the narrowest lanes.
        let fits = 80.0 + 72.0 + 8.0 * 20.0;
        assert!(after >= fits, "before {before}, after {after}");
    }

    #[test]
    fn an_unmatched_help_filter_hides_shortcuts_without_losing_the_filter() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, (open, filter): &mut (bool, String)| shortcuts(ui.ctx(), open, filter),
            (true, "no such shortcut xyz".to_owned()),
        );
        harness.run();
        assert_eq!(
            harness
                .get_by_role_and_label(Role::TextInput, "Filter")
                .value()
                .as_deref(),
            Some("no such shortcut xyz")
        );
        assert!(harness.query_by_label("Note entry").is_none());
        assert!(harness.query_by_label("Selection").is_none());
    }

    fn editable_panel() -> Harness<'static, (Song, SongDrafts, Vec<Action>, Option<String>)> {
        let mut value = serde_json::to_value(charter_editor::demo::song().document()).unwrap();
        value["metadata"]["preview"] =
            serde_json::json!({"start_seconds": 1.0, "length_seconds": 2.0});
        let song = Song::open(
            &serde_json::to_vec(&value).unwrap(),
            &[&charter_stepmania::Plugin],
        )
        .unwrap();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(650.0, 900.0))
            .build_ui_state(
                |ui,
                 (song, drafts, actions, refused): &mut (
                    Song,
                    SongDrafts,
                    Vec<Action>,
                    Option<String>,
                )| {
                    *refused =
                        song_panel(ui, song, (drafts, None), actions).or_else(|| refused.take());
                },
                (song, SongDrafts::default(), Vec::new(), None),
            );
        harness.run();
        harness
    }

    #[test]
    fn a_read_only_song_disables_adding_a_layer() {
        let song = Song::new(charter_editor::demo::song(), &[]);
        assert!(song.read_only().is_some());
        assert!(song.chart().is_some());
        let mut harness = Harness::builder().build_ui_state(
            |ui, (song, drafts, actions): &mut (Song, SongDrafts, Vec<Action>)| {
                song_panel(ui, song, (drafts, None), actions);
            },
            (song, SongDrafts::default(), Vec::new()),
        );
        harness.run();
        assert!(
            harness
                .get_by_label("Add layer")
                .accesskit_node()
                .is_disabled()
        );
    }

    #[test]
    fn clicking_the_full_layer_row_activates_it_and_ctrl_click_selects_notes() {
        let mut harness = editable_panel();
        harness.state_mut().0.add_layer().unwrap();
        harness.run();
        let rect = harness.get_by_label("Layer row Layer 0").rect();
        let at = egui::pos2(rect.right() - 3.0, rect.center().y);
        harness.hover_at(at);
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        harness.run();
        assert!(
            harness
                .state()
                .2
                .contains(&Action::SetActiveLayer(LayerIndex::new(1)))
        );
        harness.state_mut().2.clear();
        harness
            .get_by_label("Layer row Layer 0")
            .click_modifiers(egui::Modifiers::COMMAND);
        harness.run();
        assert!(
            harness
                .state()
                .2
                .contains(&Action::SelectLayer(LayerIndex::new(1)))
        );
        assert!(
            !harness
                .state()
                .2
                .contains(&Action::SetActiveLayer(LayerIndex::new(1)))
        );
    }

    #[test]
    fn clicking_a_layer_control_does_not_change_the_active_row() {
        let mut harness = editable_panel();
        harness.state_mut().0.add_layer().unwrap();
        harness.run();
        harness.get_by_label("Cycle visibility of Layer 0").click();
        harness.run();
        assert!(
            !harness
                .state()
                .2
                .iter()
                .any(|action| matches!(action, Action::SetActiveLayer(_))),
            "{:?}",
            harness.state().2
        );
    }

    #[test]
    fn right_clicking_the_full_row_opens_visibility_and_lock_commands() {
        let mut harness = editable_panel();
        harness.state_mut().0.add_layer().unwrap();
        harness.run();
        let rect = harness.get_by_label("Layer row Layer 0").rect();
        let at = egui::pos2(rect.right() - 3.0, rect.center().y);
        harness.hover_at(at);
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        harness.run();
        harness.get_by_label("Ghost").click();
        harness.run();
        assert!(harness.state().2.contains(&Action::SetLayerView(
            LayerIndex::new(1),
            LayerView::Ghosted
        )));
        harness.state_mut().2.clear();
        harness.get_by_label("Layer row Layer 0").click_secondary();
        harness.run();
        harness.get_by_label("Lock").click();
        harness.run();
        assert!(
            harness
                .state()
                .2
                .contains(&Action::SetLayerLocked(LayerIndex::new(1), true))
        );
    }

    #[test]
    fn layer_menu_visibility_uses_radio_items_for_show_ghost_and_hide() {
        let mut harness = editable_panel();
        harness.get_by_label("Layer row Layer 0").click_secondary();
        harness.run();
        harness
            .get_by_role_and_label(Role::RadioButton, "Hide")
            .click();
        harness.run();
        assert!(
            harness
                .state()
                .2
                .contains(&Action::SetLayerView(LayerIndex::new(0), LayerView::Hidden))
        );
        harness.state_mut().2.clear();
        harness
            .state_mut()
            .0
            .set_layer_view(LayerIndex::new(0), LayerView::Hidden)
            .unwrap();
        harness.run();
        harness.get_by_label("Layer row Layer 0").click_secondary();
        harness.run();
        harness
            .get_by_role_and_label(Role::RadioButton, "Show")
            .click();
        harness.run();
        assert!(
            harness
                .state()
                .2
                .contains(&Action::SetLayerView(LayerIndex::new(0), LayerView::Shown))
        );
    }

    #[test]
    fn layer_menu_offers_solo_new_above_and_moving_selected_notes() {
        let mut harness = editable_panel();
        harness.state_mut().0.add_layer().unwrap();
        harness.state_mut().0.select_all();
        harness.run();
        harness.get_by_label("Layer row Layer 0").click_secondary();
        harness.run();
        harness.get_by_label("Move selected notes here").click();
        harness.run();
        assert!(
            harness
                .state()
                .2
                .contains(&Action::MoveSelectionToLayer(LayerIndex::new(1)))
        );
        harness.state_mut().2.clear();
        harness.get_by_label("Layer row Layer 0").click_secondary();
        harness.run();
        harness.get_by_label("Solo").click();
        harness.run();
        assert!(
            harness
                .state()
                .2
                .contains(&Action::SoloLayer(LayerIndex::new(1)))
        );
        harness.state_mut().2.clear();
        harness.get_by_label("Layer row Layer 0").click_secondary();
        harness.run();
        harness.get_by_label("New layer above").click();
        harness.run();
        assert!(
            harness
                .state()
                .2
                .contains(&Action::SetActiveLayer(LayerIndex::new(1)))
        );
        assert!(harness.state().2.contains(&Action::AddLayer));
    }

    #[test]
    fn layer_panel_add_and_visibility_buttons_dispatch_actions() {
        let mut harness = editable_panel();
        harness.get_by_label("Add layer").click();
        harness.run();
        assert!(harness.state().2.contains(&Action::AddLayer));
        harness.get_by_label("Cycle visibility of Layer 0").click();
        harness.run();
        assert!(harness.state().2.contains(&Action::SetLayerView(
            LayerIndex::new(0),
            charter_core::LayerView::Ghosted
        )));
    }

    #[test]
    fn layer_panel_solos_locks_and_recolours_the_chosen_row() {
        let mut harness = editable_panel();
        harness.state_mut().0.add_layer().unwrap();
        harness.run();
        harness
            .get_by_label("Cycle visibility of Layer 0")
            .click_modifiers(egui::Modifiers::ALT);
        harness.run();
        assert!(
            harness
                .state()
                .2
                .contains(&Action::SoloLayer(LayerIndex::new(1)))
        );
        harness.get_by_label("Lock Layer 0").click();
        harness.run();
        assert!(
            harness
                .state()
                .2
                .contains(&Action::SetLayerLocked(LayerIndex::new(1), true))
        );
        harness.get_by_label("Colour of Layer 0").click();
        harness.run();
        harness.get_by_label("Pink").click();
        harness.run();
        assert!(harness.state().2.contains(&Action::RecolourLayer(
            LayerIndex::new(1),
            LayerColour::Pink
        )));
    }

    #[test]
    fn control_clicking_a_layer_selects_its_notes_instead_of_activating_it() {
        let mut harness = editable_panel();
        harness.state_mut().0.add_layer().unwrap();
        harness.run();
        harness
            .get_by_label("Layer Layer 0")
            .click_modifiers(egui::Modifiers::COMMAND);
        harness.run();
        assert!(
            harness
                .state()
                .2
                .contains(&Action::SelectLayer(LayerIndex::new(1)))
        );
        assert!(
            !harness
                .state()
                .2
                .contains(&Action::SetActiveLayer(LayerIndex::new(1)))
        );
    }

    #[test]
    fn deleting_a_layer_with_notes_asks_what_to_do_with_them() {
        let mut harness = editable_panel();
        harness.state_mut().0.add_layer().unwrap();
        harness
            .state_mut()
            .0
            .move_layer(LayerIndex::new(1), LayerIndex::new(0))
            .unwrap();
        harness.run();
        harness.get_by_label("Layer Layer 0").click_secondary();
        harness.run();
        harness.get_by_label("Delete").click();
        harness.run();
        assert!(
            harness
                .query_by_label_contains("Delete 124 notes")
                .is_some()
        );
        assert!(
            harness
                .state()
                .2
                .iter()
                .all(|a| !matches!(a, Action::DeleteLayer(..)))
        );
        harness.get_by_label("Merge them down").click();
        harness.run();
        assert!(harness.state().2.contains(&Action::DeleteLayer(
            LayerIndex::new(0),
            LayerNotes::MergeDown
        )));
    }

    #[test]
    fn delete_layer_modal_can_cancel_or_delete_its_notes() {
        let mut harness = editable_panel();
        harness.state_mut().0.add_layer().unwrap();
        harness.run();
        harness.get_by_label("Layer Layer 0").click_secondary();
        harness.run();
        harness.get_by_label("Delete").click();
        harness.run();
        harness.get_by_label("Cancel").click();
        harness.run();
        assert!(harness.state().2.is_empty());
        harness.get_by_label("Layer Layer 0").click_secondary();
        harness.run();
        harness.get_by_label("Delete").click();
        harness.run();
        harness.get_by_label("Delete 124 notes").click();
        harness.run();
        assert!(
            harness
                .state()
                .2
                .contains(&Action::DeleteLayer(LayerIndex::new(1), LayerNotes::Delete))
        );
    }

    #[test]
    fn dragging_a_layer_handle_reorders_the_rows() {
        let mut harness = editable_panel();
        harness.state_mut().0.add_layer().unwrap();
        harness.run();
        let from = harness.get_by_label("Reorder Layer 0").rect().center();
        let to = harness.get_by_label("Reorder Layer 1").rect().center();
        harness.drag_at(from);
        harness.run();
        harness.hover_at(to);
        harness.run();
        harness.drop_at(to);
        harness.run();
        assert!(harness.state().2.contains(&Action::MoveLayer {
            from: LayerIndex::new(1),
            to: LayerIndex::new(0)
        }));
    }

    #[test]
    fn double_clicking_a_layer_name_renames_it_on_enter() {
        let mut harness = editable_panel();
        harness
            .ctx
            .options_mut(|options| options.input_options.max_double_click_delay = 2.0);
        harness.get_by_label("Layer Layer 0").click();
        harness.step();
        harness.get_by_label("Layer Layer 0").click();
        harness.step();
        harness.run();
        let name = harness.get_by_role_and_label(Role::TextInput, "Rename Layer 0");
        name.click();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.event(egui::Event::Text("Kick".into()));
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert!(
            harness
                .state()
                .2
                .contains(&Action::RenameLayer(LayerIndex::new(0), "Kick".into()))
        );
    }

    #[test]
    fn layer_menu_copies_its_notes_to_another_chart() {
        let mut harness = editable_panel();
        harness.state_mut().0.add_layer().unwrap();
        harness
            .state_mut()
            .0
            .move_layer(LayerIndex::new(1), LayerIndex::new(0))
            .unwrap();
        harness.run();
        harness.get_by_label("Layer Layer 0").click_secondary();
        harness.run();
        harness.get_by_label_contains("Copy to chart").click();
        harness.run();
        harness.get_by_label("Copy to Hard").click();
        harness.run();
        assert!(harness.state().2.contains(&Action::CopyLayerToChart {
            layer: LayerIndex::new(0),
            chart: ChartIndex::new(1)
        }));
    }

    #[test]
    fn status_bar_switches_the_active_layer_from_its_menu() {
        let mut song = Song::new(charter_editor::demo::song(), &[&charter_stepmania::Plugin]);
        song.add_layer().unwrap();
        let mut harness = Harness::builder().build_ui_state(
            |ui, (song, actions): &mut (Song, Vec<Action>)| {
                status_bar(ui, Some(song), ("Ready", false), None, actions);
            },
            (song, Vec::new()),
        );
        harness.run();
        harness.get_by_label("Layer 1").click();
        harness.run();
        harness.get_by_label("Layer 0").click();
        harness.run();
        assert!(
            harness
                .state()
                .1
                .contains(&Action::SetActiveLayer(LayerIndex::new(1)))
        );
    }

    #[test]
    fn status_bar_names_the_active_layer_even_in_a_single_layer_chart() {
        let song = Song::new(charter_editor::demo::song(), &[&charter_stepmania::Plugin]);
        let mut harness = Harness::builder().build_ui_state(
            |ui, (song, actions): &mut (Song, Vec<Action>)| {
                status_bar(ui, Some(song), ("Ready", false), None, actions);
            },
            (song, Vec::new()),
        );
        harness.run();
        assert!(harness.query_by_label("Layer 0").is_some());
        assert!(harness.query_by_label("Active layer Layer 0").is_none());
    }

    #[test]
    fn invalid_song_fields_refuse_an_edit_without_losing_the_typed_draft() {
        for (label, typed) in [
            ("Artwork", "../outside.png"),
            ("Preview", "not a time"),
            ("Preview length", "-2"),
        ] {
            let mut harness = editable_panel();
            let field = harness.get_by_role_and_label(Role::TextInput, label);
            field.focus();
            harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
            field.type_text(typed);
            harness.run();
            harness.key_press(egui::Key::Enter);
            harness.run();
            let state = harness.state();
            assert!(
                state
                    .3
                    .as_deref()
                    .is_some_and(|reason| reason.contains(typed)),
                "{label}"
            );
            assert!(state.2.is_empty(), "{label}: {:?}", state.2);
            let draft = match label {
                "Artwork" => &state.1.artwork,
                "Preview" => &state.1.preview,
                _ => &state.1.length,
            };
            assert_eq!(draft.as_deref(), Some(typed), "{label}");
        }
    }

    #[test]
    fn correcting_a_refused_artwork_path_submits_only_the_corrected_value() {
        let mut harness = editable_panel();
        let artwork_field = harness.get_by_role_and_label(Role::TextInput, "Artwork");
        artwork_field.focus();
        artwork_field.type_text("../outside.png");
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert_eq!(harness.state().1.artwork.as_deref(), Some("../outside.png"));
        assert_eq!(
            harness
                .get_by_role_and_label(Role::TextInput, "Artwork")
                .value()
                .as_deref(),
            Some("../outside.png")
        );
        let corrected_field = harness.get_by_role_and_label(Role::TextInput, "Artwork");
        corrected_field.focus();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        corrected_field.type_text("cover.png");
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert!(
            harness.state().3.is_some(),
            "the earlier refusal was recorded"
        );
        let [Action::SetMetadata(metadata)] = harness.state().2.as_slice() else {
            panic!("expected one metadata edit");
        };
        assert_eq!(
            metadata
                .artwork
                .as_ref()
                .map(ToString::to_string)
                .as_deref(),
            Some("cover.png")
        );
        assert_eq!(harness.state().1.artwork, None);
    }

    #[test]
    fn clearing_preview_length_keeps_the_preview_start_and_ends_with_the_audio() {
        let mut harness = editable_panel();
        let field = harness.get_by_role_and_label(Role::TextInput, "Preview length");
        field.focus();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.key_press(egui::Key::Backspace);
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        let [Action::SetMetadata(metadata)] = harness.state().2.as_slice() else {
            panic!("expected a preview length edit");
        };
        let preview = metadata.preview.unwrap();
        assert!((f64::from(preview.start_seconds) - 1.0).abs() < f64::EPSILON);
        assert_eq!(preview.length_seconds, None);
    }

    #[test]
    fn leaving_a_changed_preview_length_submits_it_without_enter() {
        let mut harness = editable_panel();
        let field = harness.get_by_role_and_label(Role::TextInput, "Preview length");
        field.focus();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        field.type_text("4");
        harness.run();
        harness
            .get_by_role_and_label(Role::TextInput, "Title")
            .focus();
        harness.run();
        let [Action::SetMetadata(metadata)] = harness.state().2.as_slice() else {
            panic!("expected one preview edit on leaving the field");
        };
        assert_eq!(
            metadata.preview.unwrap().length_seconds.map(f64::from),
            Some(4.0)
        );
    }

    #[test]
    fn escape_leaves_a_typed_preview_length_uncommitted() {
        let mut harness = editable_panel();
        let field = harness.get_by_role_and_label(Role::TextInput, "Preview length");
        field.focus();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        field.type_text("4");
        harness.run();
        harness.key_press(egui::Key::Escape);
        harness.run();
        assert!(harness.state().2.is_empty());
        assert_eq!(harness.state().1.length, None);
        assert_eq!(
            harness
                .get_by_role_and_label(Role::TextInput, "Preview length")
                .value()
                .as_deref(),
            Some("2")
        );
    }

    #[test]
    fn a_narrow_transport_keeps_playback_and_sound_controls_without_crowding_in_zoom() {
        let song = Song::open(
            &charter_editor::demo::song().to_json().unwrap(),
            &[&charter_stepmania::Plugin],
        )
        .unwrap();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(700.0, 500.0))
            .build_ui_state(
                |ui, (song, actions): &mut (Song, Vec<Action>)| {
                    transport_bar(
                        ui,
                        Some(song),
                        (
                            Zoom::default(),
                            Rate::default(),
                            false,
                            true,
                            NoteValue::QUARTER,
                        ),
                        (false, (false, false), charter_audio::Volumes::default()),
                        actions,
                    );
                },
                (song, Vec::new()),
            );
        harness.run();
        assert!(harness.query_by_label("Play").is_some());
        assert!(harness.query_by_label("Zoom in").is_none());
        assert!(harness.query_by_label("Finer snap").is_none());
        harness.get_by_label("Play").click();
        harness.run();
        assert_eq!(harness.state().1, [Action::TogglePlay]);
    }

    #[test]
    fn toolbar_toggles_say_whether_they_are_on_rather_than_only_lighting_up() {
        for on in [false, true] {
            let song = Song::open(
                &charter_editor::demo::song().to_json().unwrap(),
                &[&charter_stepmania::Plugin],
            )
            .unwrap();
            let mut harness = Harness::builder()
                .with_size(egui::vec2(1280.0, 200.0))
                .build_ui_state(
                    move |ui, (song, actions): &mut (Song, Vec<Action>)| {
                        transport_bar(
                            ui,
                            Some(song),
                            (Zoom::default(), Rate::default(), on, on, NoteValue::QUARTER),
                            (on, (on, on), charter_audio::Volumes::default()),
                            actions,
                        );
                    },
                    (song, Vec::new()),
                );
            harness.run();
            let expected = if on { Toggled::True } else { Toggled::False };
            // Each keeps one name in either state, so the state is the only thing that changes.
            for name in [
                "Metronome",
                "Note ticks",
                "Tuned lanes",
                "Show the song panel",
                "Play",
            ] {
                assert_eq!(
                    harness.get_by_label(name).accesskit_node().toggled(),
                    Some(expected),
                    "{name} with on={on}"
                );
            }
            // A button that does something rather than holding a state claims no state at all.
            assert_eq!(
                harness
                    .get_by_label("Jump to the start")
                    .accesskit_node()
                    .toggled(),
                None
            );
        }
    }

    #[test]
    fn artwork_preview_failure_is_visible_without_changing_the_song() {
        let song = Song::open(
            &charter_editor::demo::song().to_json().unwrap(),
            &[&charter_stepmania::Plugin],
        )
        .unwrap();
        let mut harness = Harness::builder().build_ui_state(
            |ui, (song, actions): &mut (Song, Vec<Action>)| {
                song_panel(
                    ui,
                    song,
                    (&mut SongDrafts::default(), Some(Err("Picture missing"))),
                    actions,
                );
            },
            (song, Vec::new()),
        );
        harness.run();
        assert!(harness.query_by_label("Picture missing").is_some());
        assert!(harness.state().1.is_empty());
    }

    #[test]
    fn markers_jump_to_their_tick_and_offer_removal_only_when_editable() {
        let song = Song::open(
            &charter_editor::demo::song().to_json().unwrap(),
            &[&charter_stepmania::Plugin],
        )
        .unwrap();
        let mut harness = Harness::builder().build_ui_state(
            |ui, (song, actions): &mut (Song, Vec<Action>)| {
                markers(ui, song, &mut None, actions);
            },
            (song, Vec::new()),
        );
        harness.run();
        let first = harness
            .state()
            .0
            .document()
            .document()
            .markers
            .first()
            .unwrap();
        let (tick, text, key) = (first.tick, first.text.clone(), first.key());
        assert!(
            harness
                .query_by_label(&format!("Remove marker {text}"))
                .is_some()
        );
        harness.get_by_label(&text).click();
        harness.run();
        assert_eq!(harness.state().1, [Action::JumpTo(tick)]);
        harness.state_mut().1.clear();
        harness
            .get_by_label(&format!("Remove marker {text}"))
            .click();
        harness.run();
        assert_eq!(harness.state().1, [Action::RemoveMarker(key)]);
    }

    #[test]
    fn menu_choices_dispatch_the_same_actions_as_their_shortcuts() {
        for (menu, item, expected) in [
            ("File", "Choose artwork", Action::ChooseArtwork),
            ("Edit", "Mirror across the lanes", Action::Mirror),
            ("Timing", "Reset timing", Action::AskResetTiming),
            ("View", "Song panel", Action::ToggleSongPanel),
            ("Playback", "50% speed", Action::SetRate(0.5)),
            ("Help", "Keyboard shortcuts", Action::Help),
            ("Help", "About charter", Action::About),
        ] {
            let song = Song::open(
                &charter_editor::demo::song().to_json().unwrap(),
                &[&charter_stepmania::Plugin],
            )
            .unwrap();
            let mut harness = Harness::builder().build_ui_state(
                |ui, (song, actions): &mut (Song, Vec<Action>)| {
                    menu_bar(
                        ui,
                        (Some(song), &[]),
                        (false, true, (true, false), false),
                        (true, true),
                        actions,
                    );
                },
                (song, Vec::new()),
            );
            harness.run();
            harness.get_by_label(menu).click();
            harness.run();
            harness.get_by_label_contains(item).click();
            harness.run();
            assert_eq!(harness.state().1, [expected], "{menu}: {item}");
        }
    }

    #[test]
    fn plugin_song_and_chart_text_settings_keep_their_separate_scopes() {
        for (label, scope) in [
            ("Theme", SettingScope::Song),
            ("Chart theme", SettingScope::Chart),
        ] {
            let document = charter_ttr::Plugin
                .new_song(&charter_core::NewSong {
                    metadata: charter_editor::demo::song().document().metadata.clone(),
                    layout: 0,
                    charts: vec![charter_core::NewChart::named("Easy")],
                })
                .unwrap();
            let song = Song::open(&document.to_json().unwrap(), &[&charter_ttr::Plugin]).unwrap();
            let mut harness = Harness::builder().build_ui_state(
                |ui, (song, drafts, actions): &mut (Song, SongDrafts, Vec<Action>)| {
                    song_panel(ui, song, (drafts, None), actions);
                },
                (song, SongDrafts::default(), Vec::new()),
            );
            harness.run();
            let field = harness.get_by_role_and_label(Role::TextInput, label);
            field.focus();
            harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
            field.type_text("NewTheme");
            harness.run();
            harness.key_press(egui::Key::Enter);
            harness.run();
            assert_eq!(
                harness.state().2,
                [Action::SetSetting {
                    scope,
                    key: "theme".to_owned(),
                    value: Some("NewTheme".to_owned()),
                }],
                "{label}"
            );
        }
    }

    #[test]
    fn status_distinguishes_no_song_playback_and_read_only_charts() {
        let plugin: &dyn RulesetPlugin = &charter_stepmania::Plugin;
        let mut empty = Harness::builder().build_ui_state(
            |ui, (): &mut ()| status_bar(ui, None, ("", false), None, &mut Vec::new()),
            (),
        );
        empty.run();
        assert!(empty.query_by_label("No song open").is_some());

        for (plugins, playing, expected_state, expected_mode) in [
            (&[plugin][..], true, "Playing", "Editing"),
            (&[][..], false, "Paused", "Read only"),
        ] {
            let song =
                Song::open(&charter_editor::demo::song().to_json().unwrap(), plugins).unwrap();
            let mut harness = Harness::builder().build_ui_state(
                move |ui, song: &mut Song| {
                    status_bar(
                        ui,
                        Some(song),
                        ("Audio ready", playing),
                        None,
                        &mut Vec::new(),
                    );
                },
                song,
            );
            harness.run();
            assert!(harness.query_by_label(expected_state).is_some());
            assert!(harness.query_by_label("Audio ready").is_some());
            assert!(harness.query_by_label(expected_mode).is_some());
        }
    }
}
