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
/// Default artist until the user or audio tags supply one.
pub const UNKNOWN_ARTIST: &str = "Unknown artist";

/// Whether a new song gets a chart of one standard level, and what it is called.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pick {
    /// Whether to create a chart for this standard level.
    on: bool,
    /// Editable chart name, blank when the standard level name should be used.
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
    /// Title typed in the dialog; blank until supplied or found in audio tags.
    title: String,
    /// Artist typed in the dialog; blank until supplied or found in audio tags.
    artist: String,
    /// The charts of a game without standard levels.
    charts: Vec<String>,
    /// For a game with standard levels, one for each, easiest first: whether a chart of that
    /// level is made, and its name. Empty until the dialog shows such a game.
    picks: Vec<Pick>,
    /// The audio file the song plays, once one is dropped on the dialog or chosen.
    audio: Option<PathBuf>,
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
    /// The audio file chosen for the new song, if any.
    pub fn audio(&self) -> Option<&std::path::Path> {
        self.audio.as_deref()
    }

    /// Names the audio file chosen for the new song.
    pub fn set_audio(&mut self, audio: Option<PathBuf>) {
        self.audio = audio;
    }

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
                artwork: None,
                genre: None,
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
        /// The ruleset used to check the new song's charts.
        plugin: &'static dyn RulesetPlugin,
        /// The new song and its initial charts, ready for the editor.
        song: Box<NewSong>,
    },
    /// Ask the host to let the user choose the audio.
    ChooseAudio,
    /// Close the dialog without starting a song.
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
    let id = egui::Id::new("new song");
    // Pinned from the top, so the dialog stays put as a game's fields change its height.
    let top = ((ui.ctx().content_rect().height() - TALLEST) / 2.0).max(24.0);
    let area = egui::Modal::default_area(id).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, top));
    let modal = egui::Modal::new(id).area(area).show(ui.ctx(), |ui| {
        ui.set_width(WIDTH.min((ui.ctx().content_rect().width() - 32.0).max(260.0)));
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.label(
            RichText::new("New song")
                .family(style::titles(ui))
                .size(18.0),
        );
        ui.weak("Pick the game and how its charts are laid out. Charts start empty.");
        ui.add_space(14.0);
        if games.get(draft.game).is_none() {
            ui.label("No ruleset plugin can start songs.");
            if ui.button("Cancel").clicked() {
                answer = Some(Answer::Cancel);
            }
            return;
        }
        let available = (ui.ctx().content_rect().bottom() - ui.min_rect().top() - 160.0).max(120.0);
        egui::ScrollArea::vertical()
            .max_height(available)
            .show(ui, |ui| {
                choose_game(ui, &games, draft);
                let Some(&game) = games.get(draft.game) else {
                    return;
                };
                let difficulties = game.difficulties();
                if let Some(difficulties) = &difficulties {
                    draft.level_with(difficulties);
                }
                ui.add_space(14.0);
                fields(ui, game, draft);
                if audio_row(ui, draft, no_drops) {
                    answer = Some(Answer::ChooseAudio);
                }
                ui.add_space(14.0);
                match &difficulties {
                    Some(difficulties) => levels(ui, draft, difficulties),
                    None => charts(ui, draft),
                }
                if let Some(error) = &draft.error {
                    row(
                        ui,
                        |_| {},
                        |ui, ()| note(ui, RichText::new(error).color(style::WARN)),
                    );
                }
            });
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let song = draft.song();
            if ui
                .add_enabled(song.is_some(), style::primary("Create"))
                .clicked()
                && let (Some(song), Some(&game)) = (song, games.get(draft.game))
            {
                answer = Some(Answer::Create {
                    plugin: game,
                    song: Box::new(song),
                });
            }
            if ui.button("Cancel").clicked() {
                answer = Some(Answer::Cancel);
            }
        });
    });
    answer.or_else(|| modal.should_close().then_some(Answer::Cancel))
}

/// The layout every chart shares, with a sketch of its lanes, and the title and artist.
fn fields(ui: &mut Ui, game: &dyn RulesetPlugin, draft: &mut Draft) {
    let layouts = game.layouts();
    row(
        ui,
        |ui| ui.weak(game.layout_kind()).id,
        |ui, caption| {
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
                .labelled_by(caption);
            if let Some(layout) = layouts.get(draft.layout) {
                let counted = layout.name.contains("lane");
                lanes(ui, layout.lanes.into(), !counted);
            }
        },
    );
    for (label, text, blank) in [
        ("Title", &mut draft.title, "Untitled"),
        ("Artist", &mut draft.artist, "Unknown artist"),
    ] {
        row(
            ui,
            |ui| ui.weak(label).id,
            |ui, caption| {
                let field = egui::TextEdit::singleline(text)
                    .hint_text(blank)
                    .desired_width(ui.available_width());
                ui.add(field).labelled_by(caption);
            },
        );
    }
}

/// One row of the dialog: what `caption` draws in the caption column, then what `body` draws
/// from the edge every field starts at, handed what `caption` returned.
fn row<C, R>(
    ui: &mut Ui,
    caption: impl FnOnce(&mut Ui) -> C,
    body: impl FnOnce(&mut Ui, C) -> R,
) -> R {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        let size = egui::vec2(CAPTION_W, ui.spacing().interact_size.y);
        let layout = egui::Layout::left_to_right(egui::Align::Center);
        let made = ui
            .allocate_ui_with_layout(size, layout, |ui| {
                ui.set_min_width(CAPTION_W);
                caption(ui)
            })
            .inner;
        body(ui, made)
    })
    .inner
}

/// How wide the dialog and its captions are, and the gap between a caption and its field.
const WIDTH: f32 = 500.0;
/// Width of labels in the new-song dialog, in pixels.
const CAPTION_W: f32 = 88.0;
/// Horizontal gap between labels and their fields, in pixels.
const GAP: f32 = 12.0;
/// About how tall the dialog grows, for the game with the most rows; its top stays where this
/// height would be centred.
const TALLEST: f32 = 600.0;

/// The games, side by side as one segmented control, the one chosen in the accent.
fn choose_game(ui: &mut Ui, games: &[&'static dyn RulesetPlugin], draft: &mut Draft) {
    let width = ui.available_width();
    egui::Frame::new()
        .fill(style::WELL)
        .corner_radius(8)
        .inner_margin(egui::Margin::same(3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                let count = f32::from(u16::try_from(games.len()).unwrap_or(u16::MAX));
                let each = (width - 6.0 - 3.0 * (count - 1.0)) / count;
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

/// The audio the song plays, a way to choose it or take it away, and how dropping one fills in
/// the title and artist. Says whether the user asked to choose it.
#[expect(
    clippy::non_ascii_literal,
    reason = "the audio chooser button displays an ellipsis"
)]
fn audio_row(ui: &mut Ui, draft: &mut Draft, no_drops: bool) -> bool {
    let mut choose = false;
    row(
        ui,
        |ui| ui.weak("Audio"),
        |ui, _| {
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
                choose = style::icon_button(ui, style::icon::FOLDER_OPEN, "Choose audio…", false)
                    .clicked();
            });
        },
    );
    let hint = if no_drops {
        "Choose an audio file; its tags fill in the title and artist. Dragging files in doesn't \
         work under Wayland yet."
    } else {
        "Drop an audio file here, or choose one; its tags fill in the title and artist."
    };
    row(
        ui,
        |_| {},
        |ui, ()| note(ui, RichText::new(hint).small().color(style::WEAK_TEXT)),
    );
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
    heading(ui, "Each gets every lane of the layout.");
    let mut removed = None;
    for (index, name) in draft.charts.iter_mut().enumerate() {
        row(
            ui,
            |ui| ui.weak(format!("Chart {}", index.saturating_add(1))).id,
            |ui, caption| {
                // The remove button is a glyph within the button padding at each side.
                let icon = 14.0 + 2.0 * ui.spacing().button_padding.x;
                let field = egui::TextEdit::singleline(name)
                    .desired_width(ui.available_width() - icon - GAP);
                ui.add(field).labelled_by(caption);
                let remove = style::quiet_icon(
                    ui,
                    style::icon::X,
                    &format!("Remove {name}"),
                    "Remove this chart",
                );
                if remove.clicked() {
                    removed = Some(index);
                }
            },
        );
    }
    if let Some(index) = removed {
        draft.charts.remove(index);
    }
    row(
        ui,
        |_| {},
        |ui, ()| {
            if style::icon_button(ui, style::icon::PLUS, "Add a chart", false).clicked() {
                draft
                    .charts
                    .push(format!("Chart {}", draft.charts.len().saturating_add(1)));
            }
        },
    );
}

/// `text` from the field edge, wrapped short of the dialog's right edge. A wrapping label laid
/// out along a row would start its lines back at the row's left, so it gets a column of its own.
fn note(ui: &mut Ui, text: RichText) {
    ui.vertical(|ui| {
        ui.set_max_width(ui.available_width());
        ui.add(egui::Label::new(text).wrap());
    });
}

/// The charts section's title, and what its rows make.
fn heading(ui: &mut Ui, about: &str) {
    ui.label(style::title(ui, "Charts"));
    ui.add_space(-4.0);
    ui.weak(about);
    ui.add_space(2.0);
}

/// The game's standard levels, each checked for a chart and named, and presets that check and
/// name them as a game or its community does.
fn levels(ui: &mut Ui, draft: &mut Draft, difficulties: &Difficulties) {
    heading(
        ui,
        "One for each level checked, called what its game calls it.",
    );
    let chosen = draft.preset(difficulties);
    let mut picked = None;
    row(
        ui,
        |ui| ui.weak("Preset").id,
        |ui, caption| {
            let shown = chosen
                .and_then(|index| difficulties.presets.get(index))
                .map_or("Your own", |preset| preset.name.as_str());
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
                .labelled_by(caption);
        },
    );
    if let Some(index) = picked {
        draft.choose(difficulties, index);
    }
    for Pick { on, name, level } in &mut draft.picks {
        let level = level.as_str();
        row(
            ui,
            |ui| {
                style::check(ui, on, level);
                *on
            },
            |ui, on| {
                let field = egui::TextEdit::singleline(name)
                    .hint_text(level)
                    .desired_width(ui.available_width());
                ui.add_enabled(on, field).widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::TextEdit,
                        on,
                        format!("{level} chart name"),
                    )
                });
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn a_new_song_can_be_created_in_a_narrow_window() {
        let plugins: &[&dyn RulesetPlugin] = &[&charter_stepmania::Plugin];
        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build_ui_state(
                move |ui, draft: &mut Draft| {
                    show(ui, plugins, draft, false);
                },
                Draft::default(),
            );
        harness.run();
        let create = harness.get_by_label("Create");
        assert!(create.rect().left() >= 0.0, "{:?}", create.rect());
        assert!(create.rect().right() <= 400.0, "{:?}", create.rect());
        assert!(create.rect().bottom() <= 300.0, "{:?}", create.rect());
        create.click();
        harness.run();
        assert_eq!(harness.state().charts, ["Easy"]);
    }

    #[test]
    fn a_song_with_many_charts_can_still_be_created_on_a_short_screen() {
        let draft = Draft {
            charts: (1..=9).map(|i| format!("Chart {i}")).collect(),
            ..Draft::default()
        };
        let plugins: &[&dyn RulesetPlugin] = &[&charter_stepmania::Plugin];
        let mut harness = Harness::builder()
            .with_size(egui::vec2(800.0, 600.0))
            .build_ui_state(
                move |ui, draft: &mut Draft| {
                    show(ui, plugins, draft, false);
                },
                draft,
            );
        harness.run();
        let create = harness.get_by_label("Create");
        assert!(create.rect().bottom() < 600.0, "{:?}", create.rect());
        create.click();
        harness.run();
        assert_eq!(harness.state().charts.len(), 9);
    }

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

    #[expect(
        clippy::missing_trait_methods,
        reason = "a test plugin: the trait's defaults for every optional method"
    )]
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

    #[test]
    fn tags_fill_only_the_fields_the_audio_supplies_and_repeated_tags_do_not_offer_changes() {
        let mut draft = Draft::default();
        assert!(draft.blank());
        draft.fill(Some(" Track "), None);
        assert!(!draft.blank());
        assert!(!draft.differs(Some("Track"), None));
        draft.fill(None, Some("Artist"));
        assert_eq!(draft.song().unwrap().metadata.title, "Track");
        assert_eq!(draft.song().unwrap().metadata.artist, "Artist");
        assert!(draft.differs(None, Some("Another artist")));
    }

    #[test]
    fn an_unknown_preset_does_not_erase_a_custom_selection() {
        let mut draft = Draft::default();
        let difficulties = ttr();
        draft.level_with(&difficulties);
        draft.picks[0].on = true;
        draft.picks[0].name = "Mine".to_owned();
        draft.choose(&difficulties, usize::MAX);
        draft.level_with(&difficulties);
        assert_eq!(
            draft.song().unwrap().charts,
            levelled(&[("Mine", 0), ("Extreme", 3)])
        );
    }

    #[test]
    fn an_audio_name_that_cannot_be_a_resource_keeps_the_new_song_playable_later() {
        let draft = Draft {
            audio: Some(PathBuf::from("/music/..")),
            ..Draft::default()
        };
        assert_eq!(draft.song().unwrap().metadata.audio.as_str(), NO_AUDIO);
    }

    #[test]
    fn a_dialog_without_a_game_explains_why_it_cannot_create_a_song() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, draft: &mut Draft| {
                show(ui, &[], draft, false);
            },
            Draft::default(),
        );
        harness.run();
        assert!(
            harness
                .query_by_label("No ruleset plugin can start songs.")
                .is_some()
        );
        assert!(harness.query_by_label("Create").is_none());
        harness.get_by_label("Cancel").click();
        harness.run();
    }

    #[test]
    fn removing_the_chosen_audio_restores_the_placeholder_on_creation() {
        let plugins: &[&dyn RulesetPlugin] = &[&charter_stepmania::Plugin];
        let mut harness = Harness::builder().build_ui_state(
            move |ui, draft: &mut Draft| {
                show(ui, plugins, draft, false);
            },
            Draft {
                audio: Some(PathBuf::from("/music/song.ogg")),
                ..Draft::default()
            },
        );
        harness.run();
        assert!(harness.query_by_label("song.ogg").is_some());
        harness.get_by_label("Remove the audio").click();
        harness.run();
        assert!(harness.state().audio.is_none());
        assert_eq!(
            harness.state().song().unwrap().metadata.audio.as_str(),
            NO_AUDIO
        );
    }
}
