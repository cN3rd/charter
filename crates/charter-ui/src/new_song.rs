//! The dialog that starts a song: which game it is for, the layout its charts share, its title
//! and artist, and the names of its charts.

use std::path::PathBuf;

use charter_core::{
    Difficulties, Extensions, Metadata, NewChart, NewSong, ResourcePath, RulesetPlugin,
};
use egui::{RichText, Ui};

use crate::style;

/// The audio a new song names until the user drops a file on it.
pub const NO_AUDIO: &str = "no-audio-yet";
/// What a new song is called, and by whom, until the user or its audio's tags say.
pub const UNTITLED: &str = "Untitled";
pub const UNKNOWN_ARTIST: &str = "Unknown artist";

/// Whether a new song gets a chart of one standard level, and what it is called.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pick {
    on: bool,
    name: String,
    /// The level's standard name, which a blank name falls back to.
    level: String,
}

/// The dialog's fields as the user has left them.
#[derive(Debug)]
pub struct Draft {
    /// Which of the games that start songs, in plugin order.
    game: usize,
    /// Which of the game's layouts.
    layout: usize,
    title: String,
    artist: String,
    /// The charts of a game without standard levels.
    charts: Vec<String>,
    /// For a game with standard levels, one for each, easiest first: whether a chart of that
    /// level is made, and its name. Empty until the dialog shows such a game.
    picks: Vec<Pick>,
    /// The audio file the song plays, once one is dropped on the dialog or chosen.
    pub audio: Option<PathBuf>,
    /// Why the last Create attempt was refused.
    pub(crate) error: Option<String>,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            game: 0,
            layout: 0,
            title: String::new(),
            artist: String::new(),
            charts: vec!["Easy".to_owned()],
            picks: Vec::new(),
            audio: None,
            error: None,
        }
    }
}

impl Draft {
    /// Whether the title and artist are both still to be written.
    pub fn blank(&self) -> bool {
        self.title.trim().is_empty() && self.artist.trim().is_empty()
    }

    /// Whether `title` or `artist`, where given, differ from what the draft has.
    pub fn differs(&self, title: Option<&str>, artist: Option<&str>) -> bool {
        title.is_some_and(|title| title != self.title.trim())
            || artist.is_some_and(|artist| artist != self.artist.trim())
    }

    /// Takes the title and artist given.
    pub fn fill(&mut self, title: Option<&str>, artist: Option<&str>) {
        if let Some(title) = title {
            title.clone_into(&mut self.title);
        }
        if let Some(artist) = artist {
            artist.clone_into(&mut self.artist);
        }
    }

    /// Makes a chart of each level `difficulties` has, the first preset's, unless the draft
    /// already has one pick a level.
    fn level_with(&mut self, difficulties: &Difficulties) {
        if self.picks.len() != difficulties.levels.len() {
            self.picks = difficulties
                .levels
                .iter()
                .map(|level| Pick {
                    on: false,
                    name: level.clone(),
                    level: level.clone(),
                })
                .collect();
            self.choose(difficulties, 0);
        }
    }

    /// Checks the levels of preset `index` and names them as it does, unchecking the rest.
    fn choose(&mut self, difficulties: &Difficulties, index: usize) {
        let Some(preset) = difficulties.presets.get(index) else {
            return;
        };
        for (level, pick) in self.picks.iter_mut().enumerate() {
            let named = preset.charts.iter().find(|(of, _)| *of == level);
            pick.on = named.is_some();
            pick.name = named.map_or_else(|| pick.level.clone(), |(_, name)| name.clone());
        }
    }

    /// The preset the checked levels and their names are, if they are one.
    fn preset(&self, difficulties: &Difficulties) -> Option<usize> {
        let checked: Vec<_> = (self.picks.iter().enumerate())
            .filter(|(_, pick)| pick.on)
            .map(|(level, pick)| (level, pick.name.trim().to_owned()))
            .collect();
        difficulties
            .presets
            .iter()
            .position(|preset| preset.charts == checked)
    }

    /// The charts the draft asks for: the checked levels of a game that has them, and otherwise
    /// the named charts.
    fn new_charts(&self) -> Vec<NewChart> {
        if self.picks.is_empty() {
            return self.charts.iter().map(NewChart::named).collect();
        }
        (self.picks.iter().enumerate())
            .filter(|(_, pick)| pick.on)
            .map(|(level, pick)| {
                let name = pick.name.trim();
                NewChart {
                    name: if name.is_empty() { &pick.level } else { name }.to_owned(),
                    level: Some(level),
                }
            })
            .collect()
    }

    /// What the draft asks `plugin` to make, if the draft names a game and a chart.
    fn song(&self) -> Option<NewSong> {
        let charts = self.new_charts();
        if charts.is_empty() {
            return None;
        }
        let or = |text: &str, blank: &str| {
            let text = text.trim();
            if text.is_empty() { blank } else { text }.to_owned()
        };
        Some(NewSong {
            metadata: Metadata {
                title: or(&self.title, UNTITLED),
                artist: or(&self.artist, UNKNOWN_ARTIST),
                audio: self
                    .audio
                    .as_ref()
                    .and_then(|file| file.file_name()?.to_str().map(str::to_owned))
                    .and_then(|name| ResourcePath::new(name).ok())
                    .or_else(|| ResourcePath::new(NO_AUDIO).ok())?,
                preview: None,
                ext: Extensions::new(),
            },
            layout: self.layout,
            charts,
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
    /// Ask the host to let the user choose the audio.
    ChooseAudio,
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

/// Shows the dialog over the window: the game, the layout every chart shares with a sketch of
/// its lanes, the title and artist, the charts, and the audio still to come. Escape or a click
/// outside cancels.
pub fn show(
    ui: &Ui,
    plugins: &[&'static dyn RulesetPlugin],
    draft: &mut Draft,
    no_drops: bool,
) -> Option<Answer> {
    let games = games(plugins);
    let mut answer = None;
    let modal = egui::Modal::new(egui::Id::new("new song")).show(ui.ctx(), |ui| {
        ui.set_width(WIDTH);
        ui.label(
            RichText::new("New song")
                .family(style::titles(ui))
                .size(18.0),
        );
        ui.weak("Pick the game and how its charts are laid out. Charts start empty.");
        ui.add_space(14.0);
        let Some(&game) = games.get(draft.game) else {
            ui.label("No ruleset plugin can start songs.");
            if ui.button("Cancel").clicked() {
                answer = Some(Answer::Cancel);
            }
            return;
        };
        choose_game(ui, &games, draft);
        let difficulties = game.difficulties();
        if let Some(difficulties) = &difficulties {
            draft.level_with(difficulties);
        }
        ui.add_space(12.0);
        fields(ui, game, draft);
        ui.add_space(12.0);
        if audio_row(ui, draft) {
            answer = Some(Answer::ChooseAudio);
        }
        ui.add_space(12.0);
        match &difficulties {
            Some(difficulties) => levels(ui, draft, difficulties),
            None => charts(ui, draft),
        }
        if let Some(error) = &draft.error {
            ui.colored_label(style::WARN, error);
        }
        ui.add_space(14.0);
        ui.separator();
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(style::glyph(ui, style::icon::MUSIC_NOTES).color(style::WEAK_TEXT));
            ui.add(
                egui::Label::new(
                    RichText::new(if no_drops {
                        "Choose an audio file; its tags fill in the title and artist. Dragging \
                         files in doesn't work under Wayland yet."
                    } else {
                        "Drop an audio file here, or choose one; its tags fill in the title and \
                         artist."
                    })
                    .small()
                    .color(style::WEAK_TEXT),
                )
                .wrap(),
            );
        });
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let song = draft.song();
            if ui
                .add_enabled(song.is_some(), style::primary("Create"))
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

/// The layout every chart shares, with a sketch of its lanes, and the title and artist.
fn fields(ui: &mut Ui, game: &dyn RulesetPlugin, draft: &mut Draft) {
    let layouts = game.layouts();
    egui::Grid::new("new song fields")
        .num_columns(2)
        .min_col_width(CAPTION_W)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            let caption = ui.weak(game.layout_kind());
            ui.horizontal(|ui| {
                let chosen = layouts.get(draft.layout).map(|layout| layout.name.as_str());
                egui::ComboBox::from_id_salt("layout")
                    .selected_text(chosen.unwrap_or_default())
                    .width(170.0)
                    .show_ui(ui, |ui| {
                        for (index, layout) in layouts.iter().enumerate() {
                            ui.selectable_value(&mut draft.layout, index, &layout.name);
                        }
                    })
                    .response
                    .labelled_by(caption.id);
                if let Some(layout) = layouts.get(draft.layout) {
                    let counted = layout.name.contains("lane");
                    lanes(ui, layout.lanes.into(), !counted);
                }
            });
            ui.end_row();
            for (label, text, blank) in [
                ("Title", &mut draft.title, "Untitled"),
                ("Artist", &mut draft.artist, "Unknown artist"),
            ] {
                let caption = ui.weak(label);
                let field = egui::TextEdit::singleline(text)
                    .hint_text(blank)
                    .desired_width(WIDTH - CAPTION_W - 12.0);
                ui.add(field).labelled_by(caption.id);
                ui.end_row();
            }
        });
}

/// How wide the dialog and its captions are.
const WIDTH: f32 = 500.0;
const CAPTION_W: f32 = 64.0;

/// The games, side by side as one segmented control, the one chosen in the accent.
fn choose_game(ui: &mut Ui, games: &[&'static dyn RulesetPlugin], draft: &mut Draft) {
    egui::Frame::new()
        .fill(style::WELL)
        .corner_radius(8)
        .inner_margin(egui::Margin::same(3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                #[allow(clippy::cast_precision_loss)] // a handful of games
                let each = (WIDTH - 6.0 - 3.0 * (games.len() as f32 - 1.0)) / games.len() as f32;
                for (index, game) in games.iter().enumerate() {
                    let chosen = draft.game == index;
                    let text = RichText::new(game.name());
                    let text = if chosen {
                        text.color(egui::Color32::WHITE)
                    } else {
                        text
                    };
                    let segment = egui::Button::new(text)
                        .fill(if chosen {
                            style::ACCENT_FILL
                        } else {
                            egui::Color32::TRANSPARENT
                        })
                        .frame_when_inactive(chosen)
                        .min_size(egui::vec2(each, 28.0));
                    if ui.add(segment).clicked() && !chosen {
                        draft.game = index;
                        draft.layout = 0;
                        draft.picks.clear();
                    }
                }
            });
        });
}

/// The audio the song plays, and a way to choose it or take it away. Says whether the user asked
/// to choose it.
fn audio_row(ui: &mut Ui, draft: &mut Draft) -> bool {
    let mut choose = false;
    ui.horizontal(|ui| {
        ui.add_sized(
            egui::vec2(CAPTION_W, 20.0),
            egui::Label::new(RichText::new("Audio").color(style::WEAK_TEXT)),
        );
        let named = draft
            .audio
            .as_ref()
            .and_then(|file| file.file_name())
            .map(|name| name.to_string_lossy().into_owned());
        match named {
            Some(name) => {
                ui.label(style::glyph(ui, style::icon::MUSIC_NOTES).color(style::ACCENT));
                ui.label(name);
                if style::quiet_icon(ui, style::icon::X, "Remove the audio", "Remove the audio")
                    .clicked()
                {
                    draft.audio = None;
                }
            }
            None => {
                ui.weak("None yet");
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            choose =
                style::icon_button(ui, style::icon::FOLDER_OPEN, "Choose audio…", false).clicked();
        });
    });
    choose
}

/// A sketch of `count` lanes side by side, and how many there are when `say` is set.
fn lanes(ui: &mut Ui, count: u16, say: bool) {
    let shown = count.min(20);
    let (lane, gap) = (7.0, 2.0);
    let width = f32::from(shown) * (lane + gap);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 20.0), egui::Sense::hover());
    for index in 0..shown {
        let left = rect.left() + f32::from(index) * (lane + gap);
        let column =
            egui::Rect::from_min_size(egui::pos2(left, rect.top()), egui::vec2(lane, 20.0));
        ui.painter().rect_filled(column, 2.0, style::HOLD);
    }
    if say {
        let noun = if count == 1 { "lane" } else { "lanes" };
        ui.weak(format!("{count} {noun}"));
    }
}

/// The charts to start, each a name that can change or go, and a way to add another.
fn charts(ui: &mut Ui, draft: &mut Draft) {
    ui.label(style::title(ui, "Charts"));
    ui.weak("Each gets every lane of the layout.");
    ui.add_space(6.0);
    let mut removed = None;
    for (index, name) in draft.charts.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let caption = ui.label(
                RichText::new(format!("Chart {}", index + 1))
                    .small()
                    .color(style::WEAK_TEXT),
            );
            let field = egui::TextEdit::singleline(name).desired_width(WIDTH - CAPTION_W - 44.0);
            ui.add(field).labelled_by(caption.id);
            let remove = style::quiet_icon(
                ui,
                style::icon::X,
                &format!("Remove {name}"),
                "Remove this chart",
            );
            if remove.clicked() {
                removed = Some(index);
            }
        });
    }
    if let Some(index) = removed {
        draft.charts.remove(index);
    }
    if style::icon_button(ui, style::icon::PLUS, "Add a chart", false).clicked() {
        draft
            .charts
            .push(format!("Chart {}", draft.charts.len() + 1));
    }
}

/// The game's standard levels, each checked for a chart and named, and presets that check and
/// name them as a game or its community does.
fn levels(ui: &mut Ui, draft: &mut Draft, difficulties: &Difficulties) {
    ui.label(style::title(ui, "Charts"));
    ui.weak("One for each level checked, called what its game calls it.");
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let caption = ui.add_sized(
            egui::vec2(CAPTION_W, 20.0),
            egui::Label::new(RichText::new("Preset").color(style::WEAK_TEXT)),
        );
        let chosen = draft.preset(difficulties);
        let shown = chosen
            .and_then(|index| difficulties.presets.get(index))
            .map_or("Your own", |preset| preset.name.as_str());
        let mut picked = None;
        egui::ComboBox::from_id_salt("difficulty preset")
            .selected_text(shown)
            .width(210.0)
            .show_ui(ui, |ui| {
                for (index, preset) in difficulties.presets.iter().enumerate() {
                    if ui
                        .selectable_label(chosen == Some(index), &preset.name)
                        .clicked()
                    {
                        picked = Some(index);
                    }
                }
            })
            .response
            .labelled_by(caption.id);
        if let Some(index) = picked {
            draft.choose(difficulties, index);
        }
    });
    for pick in &mut draft.picks {
        ui.horizontal(|ui| {
            ui.add_sized(
                egui::vec2(CAPTION_W, 20.0),
                egui::Checkbox::new(&mut pick.on, pick.level.as_str()),
            );
            let field = egui::TextEdit::singleline(&mut pick.name)
                .hint_text(pick.level.as_str())
                .desired_width(WIDTH - CAPTION_W - 12.0);
            ui.add_enabled(pick.on, field).widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::TextEdit,
                    pick.on,
                    format!("{} chart name", pick.level),
                )
            });
        });
    }
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
        assert_eq!(song.charts, [NewChart::named("Easy")]);
    }

    fn ttr() -> Difficulties {
        charter_ttr::Plugin.difficulties().unwrap()
    }

    fn levelled(charts: &[(&str, usize)]) -> Vec<NewChart> {
        charts
            .iter()
            .map(|&(name, level)| NewChart {
                name: name.to_owned(),
                level: Some(level),
            })
            .collect()
    }

    #[test]
    fn a_game_with_levels_starts_on_its_first_preset() {
        let mut draft = Draft::default();
        draft.level_with(&ttr());
        assert_eq!(draft.preset(&ttr()), Some(0));
        assert_eq!(draft.song().unwrap().charts, levelled(&[("Extreme", 3)]));
    }

    #[test]
    fn a_preset_checks_its_levels_under_its_names() {
        let mut draft = Draft::default();
        draft.level_with(&ttr());
        draft.choose(&ttr(), 3);
        assert_eq!(draft.preset(&ttr()), Some(3));
        assert_eq!(
            draft.song().unwrap().charts,
            levelled(&[
                ("Extreme", 0),
                ("Intense", 1),
                ("Severe", 2),
                ("Customs", 3)
            ])
        );
    }

    #[test]
    fn any_levels_can_be_checked_and_a_blank_name_takes_the_levels() {
        let mut draft = Draft::default();
        draft.level_with(&ttr());
        draft.picks[3].on = false;
        draft.picks[0].on = true;
        draft.picks[2].on = true;
        draft.picks[2].name = "  ".to_owned();
        assert_eq!(draft.preset(&ttr()), None);
        assert_eq!(
            draft.song().unwrap().charts,
            levelled(&[("Easy", 0), ("Hard", 2)])
        );
        for pick in &mut draft.picks {
            pick.on = false;
        }
        assert!(draft.song().is_none());
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
