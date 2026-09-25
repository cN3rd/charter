//! The dialog that starts a song: which game it is for, the layout its charts share, its title
//! and artist, and the names of its charts.

use charter_core::{Extensions, Metadata, NewSong, ResourcePath, RulesetPlugin};
use egui::Ui;

/// The audio a new song names until the user drops a file on it.
pub const NO_AUDIO: &str = "no-audio-yet";

/// The dialog's fields as the user has left them.
#[derive(Debug)]
pub struct Draft {
    /// Which of the games that start songs, in plugin order.
    game: usize,
    /// Which of the game's layouts.
    layout: usize,
    title: String,
    artist: String,
    charts: Vec<String>,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            game: 0,
            layout: 0,
            title: String::new(),
            artist: String::new(),
            charts: vec!["Easy".to_owned()],
        }
    }
}

impl Draft {
    /// What the draft asks `plugin` to make, if the draft names a game and a chart.
    fn song(&self) -> Option<NewSong> {
        if self.charts.is_empty() {
            return None;
        }
        let or = |text: &str, blank: &str| {
            let text = text.trim();
            if text.is_empty() { blank } else { text }.to_owned()
        };
        Some(NewSong {
            metadata: Metadata {
                title: or(&self.title, "Untitled"),
                artist: or(&self.artist, "Unknown artist"),
                audio: ResourcePath::new(NO_AUDIO).ok()?,
                preview: None,
                ext: Extensions::new(),
            },
            layout: self.layout,
            charts: self.charts.clone(),
        })
    }
}

/// What the user did with the dialog.
#[derive(Debug)]
pub enum Answer {
    /// Make `song` with `plugin`.
    Create {
        plugin: &'static dyn RulesetPlugin,
        song: NewSong,
    },
    Cancel,
}

/// The plugins among `plugins` that can start songs.
pub fn games(plugins: &[&'static dyn RulesetPlugin]) -> Vec<&'static dyn RulesetPlugin> {
    plugins
        .iter()
        .copied()
        .filter(|plugin| !plugin.layouts().is_empty())
        .collect()
}

/// Shows the dialog over the window. Escape or a click outside cancels.
pub fn show(ui: &Ui, plugins: &[&'static dyn RulesetPlugin], draft: &mut Draft) -> Option<Answer> {
    let games = games(plugins);
    let mut answer = None;
    let modal = egui::Modal::new(egui::Id::new("new song")).show(ui.ctx(), |ui| {
        ui.set_width(380.0);
        ui.heading("New song");
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            for (index, game) in games.iter().enumerate() {
                if ui
                    .selectable_label(draft.game == index, game.name())
                    .clicked()
                    && draft.game != index
                {
                    draft.game = index;
                    draft.layout = 0;
                }
            }
        });
        let Some(&game) = games.get(draft.game) else {
            ui.label("No ruleset plugin can start songs.");
            if ui.button("Cancel").clicked() {
                answer = Some(Answer::Cancel);
            }
            return;
        };
        let layouts = game.layouts();
        ui.add_space(8.0);
        egui::Grid::new("new song fields")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                let caption = ui.weak(game.layout_kind());
                let chosen = layouts.get(draft.layout).map(|layout| layout.name.as_str());
                egui::ComboBox::from_id_salt("layout")
                    .selected_text(chosen.unwrap_or_default())
                    .show_ui(ui, |ui| {
                        for (index, layout) in layouts.iter().enumerate() {
                            ui.selectable_value(&mut draft.layout, index, &layout.name);
                        }
                    })
                    .response
                    .labelled_by(caption.id);
                ui.end_row();
                for (label, text, blank) in [
                    ("Title", &mut draft.title, "Untitled"),
                    ("Artist", &mut draft.artist, "Unknown artist"),
                ] {
                    let caption = ui.weak(label);
                    ui.add(egui::TextEdit::singleline(text).hint_text(blank))
                        .labelled_by(caption.id);
                    ui.end_row();
                }
            });
        ui.add_space(8.0);
        ui.weak("Charts, each with every lane of the layout");
        let mut removed = None;
        for (index, name) in draft.charts.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let caption = ui.weak(format!("Chart {}", index + 1));
                ui.text_edit_singleline(name).labelled_by(caption.id);
                let remove = ui.small_button("✕");
                let label = format!("Remove {name}");
                remove.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label)
                });
                if remove.clicked() {
                    removed = Some(index);
                }
            });
        }
        if let Some(index) = removed {
            draft.charts.remove(index);
        }
        if ui.button("Add a chart").clicked() {
            draft
                .charts
                .push(format!("Chart {}", draft.charts.len() + 1));
        }
        ui.add_space(8.0);
        ui.weak("Once the song is open, drop its audio file on the window, or choose it there.");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let song = draft.song();
            if ui
                .add_enabled(song.is_some(), egui::Button::new("Create"))
                .clicked()
                && let Some(song) = song
            {
                answer = Some(Answer::Create { plugin: game, song });
            }
            if ui.button("Cancel").clicked() {
                answer = Some(Answer::Cancel);
            }
        });
    });
    answer.or(modal.should_close().then_some(Answer::Cancel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_title_and_artist_make_an_untitled_song_by_an_unknown_artist() {
        let song = Draft::default().song().unwrap();
        assert_eq!(song.metadata.title, "Untitled");
        assert_eq!(song.metadata.artist, "Unknown artist");
        assert_eq!(song.metadata.audio.as_str(), NO_AUDIO);
        assert_eq!(song.charts, ["Easy"]);
    }

    #[test]
    fn the_title_and_artist_lose_their_surrounding_spaces() {
        let draft = Draft {
            title: "  Mine ".to_owned(),
            artist: " Me".to_owned(),
            ..Draft::default()
        };
        let song = draft.song().unwrap();
        assert_eq!(
            (song.metadata.title.as_str(), song.metadata.artist.as_str()),
            ("Mine", "Me")
        );
    }

    #[test]
    fn a_draft_without_charts_makes_nothing() {
        let draft = Draft {
            charts: Vec::new(),
            ..Draft::default()
        };
        assert!(draft.song().is_none());
    }

    /// Checks songs but cannot start them.
    #[derive(Debug)]
    struct Checks;

    impl RulesetPlugin for Checks {
        fn id(&self) -> &'static str {
            "checks"
        }

        fn check(
            &self,
            _: &charter_core::ValidDocument,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[test]
    fn only_plugins_with_layouts_start_songs() {
        let plugins: &[&'static dyn RulesetPlugin] = &[&Checks, &charter_stepmania::Plugin];
        let games = games(plugins);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].id(), "sm");
    }
}
