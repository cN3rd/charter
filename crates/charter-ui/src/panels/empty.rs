use std::path::{Component, Path, PathBuf};

use super::{Action, NEW_SONG, OPEN, RichText, Ui, egui, style};

/// How wide the column of ways to start is.
const START_W: f32 = 320.0;
/// The narrowest the recent songs' column gets beside the start column, and the widest.
const RECENT_W: (f32, f32) = (300.0, 420.0);
/// Between the two columns.
const GAP: f32 = 32.0;
/// A row's height: its name over a line about it.
const ROW_H: f32 = 44.0;
/// Between a row's edge and its text, which titles and lines around the rows line up with.
const PAD: f32 = 10.0;
/// The size of the line under a row's name.
const ABOUT_SIZE: f32 = 11.5;

/// What the centre shows with no song open: the ways to start beside the songs opened last, one
/// above the other where the window is too narrow for both, then what can be dropped on the
/// window unless `no_drops`. `games` are those whose songs can be imported.
pub(crate) fn empty(
    ui: &mut Ui,
    (no_drops, games, recent): (bool, &[&str], &[PathBuf]),
    actions: &mut Vec<Action>,
) {
    let top = (ui.available_height() * 0.1).clamp(16.0, 72.0);
    style::solid_scroll_bars(ui);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.add_space(top);
            let width = ui.available_width();
            let side_by_side = width >= START_W + GAP + RECENT_W.0;
            let whole = if side_by_side {
                width.min(START_W + GAP + RECENT_W.1)
            } else {
                width.min(RECENT_W.1)
            };
            let corner = egui::pos2(ui.max_rect().center().x - whole / 2.0, ui.cursor().top());
            let area = egui::Rect::from_min_size(corner, egui::vec2(whole, ui.available_height()));
            ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
                header(ui);
                ui.add_space(32.0);
                if side_by_side {
                    ui.horizontal_top(|ui| {
                        column(ui, START_W, |ui| start(ui, games, actions));
                        ui.add_space(GAP - ui.spacing().item_spacing.x);
                        column(ui, whole - START_W - GAP, |ui| {
                            recent_songs(ui, recent, actions);
                        });
                    });
                } else {
                    column(ui, whole, |ui| start(ui, games, actions));
                    ui.add_space(28.0);
                    column(ui, whole, |ui| recent_songs(ui, recent, actions));
                }
                ui.add_space(32.0);
                footer(ui, no_drops);
            });
        });
}

/// charter's mark and name, over what it is.
fn header(ui: &mut Ui) {
    inset(ui, |ui| {
        ui.horizontal(|ui| {
            mark(ui);
            ui.label(
                RichText::new("charter")
                    .family(style::titles(ui))
                    .size(28.0),
            );
        });
        ui.weak("A chart editor for rhythm games");
    });
}

/// charter's mark: a notefield in small, with notes stepping across its four lanes down to the
/// line where they are hit.
fn mark(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(35.0), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    painter.rect(
        rect,
        9.0,
        style::STAGE,
        egui::Stroke::new(1.0, style::EDGE),
        egui::StrokeKind::Inside,
    );
    // In whole pixels, so the little lines stay sharp: four lanes five wide, one apart.
    let corner = egui::pos2(rect.left().round() + 6.0, rect.top().round() + 7.0);
    for between in [5.5, 11.5, 17.5] {
        let lane_line = egui::Stroke::new(1.0, style::BEAT_LINE);
        painter.vline(corner.x + between, corner.y..=corner.y + 18.0, lane_line);
    }
    for (step, left) in [(0.0, 0.0), (1.0, 6.0), (2.0, 12.0), (3.0, 18.0)] {
        let at = egui::pos2(corner.x + left, corner.y + step * 4.0);
        let note = egui::Rect::from_min_size(at, egui::vec2(5.0, 3.0));
        painter.rect_filled(note, 1.0, style::TAP);
    }
    let hit = egui::Stroke::new(2.0, style::NOW_LINE);
    painter.hline(corner.x..=corner.x + 23.0, corner.y + 19.0, hit);
}

/// `body` laid out top to bottom, `width` points wide.
fn column(ui: &mut Ui, width: f32, body: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(width);
            // Rows close together, as one list.
            ui.spacing_mut().item_spacing.y = 2.0;
            body(ui);
        },
    );
}

/// `body` in line with the rows' text.
fn inset<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 10,
            ..egui::Margin::ZERO
        })
        .show(ui, body)
        .inner
}

/// A column's title in small capitals, in line with its rows' text.
fn title(ui: &mut Ui, text: &str) {
    inset(ui, |ui| ui.label(style::title(ui, text)));
    ui.add_space(4.0);
}

/// The ways to start: a new song, a saved one, another game's, or the demo.
#[expect(
    clippy::non_ascii_literal,
    reason = "the commands that ask for more end in an ellipsis"
)]
fn start(ui: &mut Ui, games: &[&str], actions: &mut Vec<Action>) {
    title(ui, "Start");
    let (new_keys, open_keys) = (
        ui.ctx().format_shortcut(&NEW_SONG),
        ui.ctx().format_shortcut(&OPEN),
    );
    let way = |icon, name, about, keys, primary| Row {
        icon: Some(icon),
        name,
        about,
        said: about,
        keys,
        primary,
    };
    let about_new = "Pick the game, its charts and the audio";
    let new = way(
        style::icon::FILE_PLUS,
        "New song…",
        about_new,
        Some(new_keys),
        true,
    );
    if row(ui, new).clicked() {
        actions.push(Action::NewSong);
    }
    let about_open = "A charter song (.json)";
    let open = way(
        style::icon::FOLDER_OPEN,
        "Open song…",
        about_open,
        Some(open_keys),
        false,
    );
    if row(ui, open).clicked() {
        actions.push(Action::Open);
    }
    let from = format!("From {}", listed(games));
    if !games.is_empty() {
        let import = way(
            style::icon::ARROW_SQUARE_IN,
            "Import song…",
            &from,
            None,
            false,
        );
        if row(ui, import).clicked() {
            actions.push(Action::ImportSong);
        }
    }
    let about_demo = "A song with two charts, to try things on";
    let demo = way(
        style::icon::MUSIC_NOTES,
        "Open the demo song",
        about_demo,
        None,
        false,
    );
    if row(ui, demo).clicked() {
        actions.push(Action::OpenDemo);
    }
}

/// `names` as a sentence lists them: `A`, `A or B`, `A, B or C`.
fn listed(names: &[&str]) -> String {
    match names.split_last() {
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// The songs opened last, newest first, each named over its folder, with its whole path on
/// hover; or where they will be, before there are any.
fn recent_songs(ui: &mut Ui, recent: &[PathBuf], actions: &mut Vec<Action>) {
    title(ui, "Recent");
    if recent.is_empty() {
        inset(ui, |ui| ui.weak("Songs you open or save show up here."));
        return;
    }
    let room = ui.available_width() - 2.0 * PAD;
    for path in recent {
        let name = song_name(path);
        let folder = path.parent().unwrap_or_else(|| Path::new(""));
        let whole = folder.display().to_string();
        let shown = elided(folder, |text| {
            let text = egui::WidgetText::from(RichText::new(text).size(ABOUT_SIZE));
            let galley = text.into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Body,
            );
            galley.size().x <= room
        });
        let song = Row {
            icon: None,
            name: &name,
            about: &shown,
            said: &whole,
            keys: None,
            primary: false,
        };
        if row(ui, song)
            .on_hover_text(path.display().to_string())
            .clicked()
        {
            actions.push(Action::OpenRecent(path.clone()));
        }
    }
}

/// What a recent song is called: its file's name without `.json`, or its whole path where it
/// has no file name.
pub(super) fn song_name(path: &Path) -> String {
    match (path.file_stem(), path.extension()) {
        (Some(stem), Some(extension)) if extension == "json" => stem.to_string_lossy().into_owned(),
        _ => path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        ),
    }
}

/// `folder` as `fits` takes it: whole, or without folders from its start, so those nearest the
/// song stay, as in `…/Spring Pack/Hanami`. Where even its last does not fit, egui cuts it short.
fn elided(folder: &Path, fits: impl Fn(&str) -> bool) -> String {
    let whole = folder.display().to_string();
    if fits(&whole) {
        return whole;
    }
    let names: Vec<_> = folder
        .components()
        .filter(|part| matches!(part, Component::Normal(_)))
        .collect();
    let from = |first: usize| {
        let rest: PathBuf = names.iter().skip(first).collect();
        format!("…{}{}", std::path::MAIN_SEPARATOR, rest.display())
    };
    (0..names.len())
        .map(from)
        .find(|shown| fits(shown))
        .unwrap_or_else(|| from(names.len().saturating_sub(1)))
}

/// What to drop on the window to open or start a song, unless `no_drops`, and which charter
/// this is.
fn footer(ui: &mut Ui, no_drops: bool) {
    ui.separator();
    inset(ui, |ui| {
        if !no_drops {
            let hint =
                "Drop a charter song on the window to open it, or an audio file to start one.";
            ui.add(egui::Label::new(RichText::new(hint).color(style::WEAK_TEXT)).wrap());
        }
        let version = super::about::version_line(
            env!("CARGO_PKG_VERSION"),
            option_env!("CHARTER_COMMIT"),
            option_env!("CHARTER_BUILD"),
        );
        ui.label(
            RichText::new(version)
                .size(ABOUT_SIZE)
                .color(style::WEAK_TEXT),
        );
    });
}

/// A way to start, or a recent song: one row of the welcome screen.
struct Row<'a> {
    /// The icon before a way to start.
    icon: Option<&'a str>,
    /// What it is, which names it to screen readers too.
    name: &'a str,
    /// The line under its name, as shown.
    about: &'a str,
    /// The line under its name, whole, for screen readers.
    said: &'a str,
    /// Its keyboard shortcut, at its trailing end.
    keys: Option<String>,
    /// Whether it is the screen's main action, filled with the accent.
    primary: bool,
}

/// `row` across `ui`, lit on hover and ringed while it has the keyboard, as the panel's rows and
/// controls are. Clicked, or Enter while focused, it does what it names.
fn row(ui: &mut Ui, row: Row<'_>) -> egui::Response {
    let size = egui::vec2(ui.available_width(), ROW_H);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, row.name));
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_description(row.said.to_owned());
        if let Some(keys) = &row.keys {
            node.set_keyboard_shortcut(keys.clone());
        }
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let lit = response.hovered() || response.has_focus();
    let (fill, ink, weak) = if row.primary {
        (style::ACCENT_FILL, style::ON_ACCENT, style::ON_ACCENT)
    } else if lit {
        (style::ROW_HOVER, style::TEXT, style::WEAK_TEXT)
    } else {
        (egui::Color32::TRANSPARENT, style::TEXT, style::WEAK_TEXT)
    };
    ui.painter().rect_filled(rect, 6.0, fill);
    if row.primary && lit {
        // An edge for the accent row, since a lighter fill would leave its white text under 4.5:1.
        let edge = egui::Stroke::new(1.0, style::ACCENT);
        ui.painter()
            .rect_stroke(rect, 6.0, edge, egui::StrokeKind::Inside);
    }
    let galley = |text: RichText, room: f32| {
        egui::WidgetText::from(text).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            room.max(0.0),
            egui::TextStyle::Body,
        )
    };
    let mut left = rect.left() + PAD;
    if let Some(icon) = row.icon {
        let glyph = galley(style::glyph(ui, icon).size(17.0).color(ink), f32::INFINITY);
        let at = egui::pos2(left, rect.center().y - glyph.size().y / 2.0);
        ui.painter().galley(at, glyph, ink);
        left += 28.0;
    }
    let right = rect.right() - PAD;
    let keys = row.keys.map(|keys| {
        galley(
            RichText::new(keys).size(ABOUT_SIZE).color(weak),
            f32::INFINITY,
        )
    });
    let keys_room = keys.as_ref().map_or(0.0, |keys| keys.size().x + 12.0);
    let name = galley(RichText::new(row.name).color(ink), right - keys_room - left);
    let about = galley(
        RichText::new(row.about).size(ABOUT_SIZE).color(weak),
        right - left,
    );
    let top = rect.center().y - (name.size().y + 1.0 + about.size().y) / 2.0;
    if let Some(keys) = keys {
        let at = egui::pos2(
            right - keys.size().x,
            top + (name.size().y - keys.size().y) / 2.0,
        );
        ui.painter().galley(at, keys, weak);
    }
    let under = top + name.size().y + 1.0;
    ui.painter().galley(egui::pos2(left, top), name, ink);
    ui.painter().galley(egui::pos2(left, under), about, weak);
    if response.has_focus() {
        let ring = egui::Stroke::new(2.0, style::ACCENT);
        ui.painter()
            .rect_stroke(rect.expand(2.0), 8.0, ring, egui::StrokeKind::Outside);
    }
    response
}

/// That the open song has no audio yet, and how to give an editable one some: choosing a file,
/// or dropping one unless `no_drops`.
#[expect(
    clippy::non_ascii_literal,
    reason = "the missing-audio chooser button displays an ellipsis"
)]
pub(crate) fn no_audio(ui: &mut Ui, no_drops: bool, editable: bool, actions: &mut Vec<Action>) {
    notice(ui, style::icon::MUSIC_NOTES, |ui| {
        if no_drops || !editable {
            ui.label("No audio yet (.ogg, .mp3, .wav, .flac).");
        } else {
            ui.label(
                "No audio yet. Drop an audio file (.ogg, .mp3, .wav, .flac) on the window, or",
            );
        }
        let choose = ui
            .add_enabled_ui(editable, |ui| {
                style::icon_button(ui, style::icon::FOLDER_OPEN, "Choose audio…", true)
            })
            .inner
            .on_disabled_hover_text("The song is read-only; its audio cannot be changed.");
        let choose = if no_drops {
            choose.on_hover_text(NO_DROPS)
        } else {
            choose
        };
        if choose.clicked() {
            actions.push(Action::ChooseAudio);
        }
    });
}

/// Why files cannot be dragged onto the window, where they cannot.
pub(crate) const NO_DROPS: &str = "Dragging files in doesn't work under Wayland yet: the windowing library charter uses does \
     not take drops there.";

/// A strip across the top of the centre that says something about the song, led by `symbol`.
pub(super) fn notice<R>(ui: &mut Ui, symbol: &str, body: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(style::NOTICE)
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(style::glyph(ui, symbol).color(style::WARN));
                body(ui)
            })
            .inner
        })
        .inner
}

#[cfg(test)]
#[expect(
    clippy::non_ascii_literal,
    reason = "the welcome screen's commands that ask for more end in an ellipsis"
)]
mod tests {
    use super::{elided, empty};
    use crate::window::test_support::*;

    /// The welcome screen `width` points wide, where files dropped on the window arrive when
    /// `drops`, offering to import the songs of `games` and to open `recent`.
    fn welcome(
        width: f32,
        (drops, games): (bool, &'static [&'static str]),
        recent: Vec<PathBuf>,
    ) -> Harness<'static, (Vec<PathBuf>, Vec<Action>)> {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(width, 720.0))
            .build_ui_state(
                move |ui, (recent, actions): &mut (Vec<PathBuf>, Vec<Action>)| {
                    crate::style::apply(ui.ctx(), &[]);
                    empty(ui, (!drops, games, recent), actions);
                },
                (recent, Vec::new()),
            );
        harness.run();
        harness
    }

    #[test]
    fn each_way_to_start_names_its_shortcut_and_does_what_it_says() {
        let mut harness = welcome(1280.0, (true, &["StepMania"]), Vec::new());
        for (label, keys, action) in [
            ("New song…", Some("Ctrl+N"), Action::NewSong),
            ("Open song…", Some("Ctrl+O"), Action::Open),
            ("Import song…", None, Action::ImportSong),
            ("Open the demo song", None, Action::OpenDemo),
        ] {
            let row = harness.get_by_role_and_label(Role::Button, label);
            assert_eq!(row.accesskit_node().data().keyboard_shortcut(), keys);
            row.click();
            harness.run();
            assert_eq!(harness.state_mut().1.pop(), Some(action), "{label}");
        }
    }

    #[test]
    fn import_is_offered_only_for_games_charter_reads_and_names_them() {
        let none = welcome(1280.0, (true, &[]), Vec::new());
        assert!(none.query_by_label("Import song…").is_none());
        let games = &["StepMania", "osu!mania", "Tap Tap Revenge"];
        let three = welcome(1280.0, (true, games), Vec::new());
        let import = three.get_by_role_and_label(Role::Button, "Import song…");
        assert_eq!(
            import.accesskit_node().data().description(),
            Some("From StepMania, osu!mania or Tap Tap Revenge")
        );
    }

    #[test]
    fn every_recent_song_shows_its_name_and_folder_and_opens_on_a_click() {
        let recent: Vec<_> = (0..10)
            .map(|n| PathBuf::from(format!("/charts/Song {n}/Song {n}.json")))
            .collect();
        let mut harness = welcome(1280.0, (true, &[]), recent.clone());
        for n in 0..10 {
            let name = format!("Song {n}");
            let row = harness.get_by_role_and_label(Role::Button, &name);
            let folder = format!("/charts/Song {n}");
            assert_eq!(
                row.accesskit_node().data().description(),
                Some(folder.as_str())
            );
        }
        harness
            .get_by_role_and_label(Role::Button, "Song 9")
            .click();
        harness.run();
        assert_eq!(harness.state().1, [Action::OpenRecent(recent[9].clone())]);
    }

    #[test]
    fn with_no_recent_songs_the_welcome_screen_says_where_they_will_appear() {
        let harness = welcome(1280.0, (true, &[]), Vec::new());
        assert!(harness.query_by_label_contains("Songs you open").is_some());
    }

    #[test]
    fn start_and_recent_sit_side_by_side_when_wide_and_stack_when_narrow() {
        let recent = vec![PathBuf::from("/charts/Anthem/Anthem.json")];
        // 427 points is the default window at the largest interface size the settings allow.
        for width in [1280.0_f32, 640.0, 427.0, 360.0] {
            let harness = welcome(width, (true, &["StepMania"]), recent.clone());
            let start = harness
                .get_by_role_and_label(Role::Button, "New song…")
                .rect();
            let song = harness.get_by_role_and_label(Role::Button, "Anthem").rect();
            for rect in [start, song] {
                assert!(
                    rect.left() >= 0.0 && rect.right() <= width,
                    "at {width} wide, a row sits at {rect:?}"
                );
            }
            let both = start.union(song);
            assert!(
                (both.center().x - width / 2.0).abs() < 2.0,
                "at {width} wide, the rows span {both:?}"
            );
            if width >= 1280.0 {
                assert!(song.left() > start.right(), "at {width} wide");
            }
            if width <= 427.0 {
                assert!(song.top() > start.bottom(), "at {width} wide");
            }
        }
    }

    #[test]
    fn the_drop_hint_shows_only_where_dropped_files_arrive() {
        let drops = welcome(1280.0, (true, &[]), Vec::new());
        assert!(
            drops
                .query_by_label_contains("Drop a charter song")
                .is_some()
        );
        let none = welcome(1280.0, (false, &[]), Vec::new());
        assert!(
            none.query_by_label_contains("Drop a charter song")
                .is_none()
        );
    }

    #[test]
    fn the_welcome_screen_says_which_version_this_is() {
        let harness = welcome(1280.0, (true, &[]), Vec::new());
        let version = env!("CARGO_PKG_VERSION");
        assert!(harness.query_by_label_contains(version).is_some());
    }

    #[test]
    fn a_long_folder_loses_folders_from_its_start_not_those_nearest_the_song() {
        let folder = Path::new("/home/sam/Downloads/packs/Spring Pack 2026/Hanami");
        let fits = |shown: &str| shown.chars().count() <= 26;
        assert_eq!(elided(folder, fits), "…/Spring Pack 2026/Hanami");
        assert_eq!(elided(folder, |_| true), folder.display().to_string());
    }

    #[test]
    fn read_only_song_without_audio_offers_neither_drops_nor_chooser() {
        let mut document: serde_json::Value = serde_json::from_slice(&four_taps()).unwrap();
        document["metadata"]["audio"] = serde_json::json!(crate::new_song::NO_AUDIO);
        let mut app = Window::new(&[]);
        app.open(
            Path::new("/songs/no-audio.json"),
            &serde_json::to_vec(&document).unwrap(),
        );
        let harness = window(app);

        assert!(harness.query_by_label_contains("Read only:").is_some());
        assert!(
            harness
                .query_by_label_contains("Drop an audio file")
                .is_none()
        );
        assert!(
            harness
                .get_by_label_contains("Choose audio")
                .accesskit_node()
                .is_disabled()
        );
    }
}
