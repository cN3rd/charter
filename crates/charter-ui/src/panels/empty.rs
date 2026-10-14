use super::{Action, Align, Layout, RichText, Ui, egui, style};

/// What the centre shows with no song open.
#[expect(
    clippy::non_ascii_literal,
    reason = "the empty-window Open song button displays an ellipsis"
)]
pub(crate) fn empty(
    ui: &mut Ui,
    (no_drops, recent): (bool, &[std::path::PathBuf]),
    actions: &mut Vec<Action>,
) {
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() / 3.0 - 40.0).max(0.0));
        ui.label(
            RichText::new("charter")
                .family(style::titles(ui))
                .size(34.0),
        );
        ui.add_space(2.0);
        ui.weak("A chart editor for rhythm games");
        ui.add_space(22.0);
        // ponytail: the three buttons' measured width, so the parent centres a row egui would
        // otherwise start at the left; the test that centres them fails if this drifts.
        let row = 3.0 * 136.0;
        let fits = ui.available_width() >= row;
        ui.allocate_ui_with_layout(
            egui::vec2(row.min(ui.available_width()), 30.0),
            // Stacked, still centred, where the row would run off a narrow window.
            if fits {
                Layout::left_to_right(Align::Center)
            } else {
                Layout::top_down(Align::Center)
            },
            |ui| {
                ui.spacing_mut().button_padding = egui::vec2(14.0, 7.0);
                if style::icon_button(ui, style::icon::FILE_PLUS, "New song", true).clicked() {
                    actions.push(Action::NewSong);
                }
                if style::icon_button(ui, style::icon::FOLDER_OPEN, "Open song…", false).clicked()
                {
                    actions.push(Action::Open);
                }
                if style::icon_button(ui, style::icon::MUSIC_NOTES, "Open the demo song", false)
                    .clicked()
                {
                    actions.push(Action::OpenDemo);
                }
            },
        );
        if !no_drops {
            ui.add_space(16.0);
            ui.weak("or drop a charter song (.json) on this window");
        }
        if !recent.is_empty() {
            ui.add_space(28.0);
            style::heading(ui, "Recent");
            ui.add_space(4.0);
            for path in recent.iter().take(5) {
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                );
                let link = ui.link(name).on_hover_text(path.display().to_string());
                if link.clicked() {
                    actions.push(Action::OpenRecent(path.clone()));
                }
            }
        }
    });
}

/// That the open song has no audio yet, and how to give it some: choosing a file, or dropping
/// one unless `no_drops`.
#[expect(
    clippy::non_ascii_literal,
    reason = "the missing-audio chooser button displays an ellipsis"
)]
pub(crate) fn no_audio(ui: &mut Ui, no_drops: bool, actions: &mut Vec<Action>) {
    notice(ui, style::icon::MUSIC_NOTES, |ui| {
        if no_drops {
            ui.label("No audio yet (.ogg, .mp3, .wav, .flac).");
        } else {
            ui.label(
                "No audio yet. Drop an audio file (.ogg, .mp3, .wav, .flac) on the window, or",
            );
        }
        let choose = style::icon_button(ui, style::icon::FOLDER_OPEN, "Choose audio…", true);
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
