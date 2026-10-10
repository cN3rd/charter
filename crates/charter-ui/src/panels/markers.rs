use super::song::WIDE;
use super::{Action, MarkerKind, RichText, Song, Ui, egui, style, table};

/// The song's markers, each of which jumps the cursor to it when clicked. In a song open for
/// editing a name typed and entered names the section at the cursor, starting one there if
/// there is none, and each marker can be removed.
pub(crate) fn markers(
    ui: &mut Ui,
    song: &Song,
    draft: &mut Option<String>,
    actions: &mut Vec<Action>,
) {
    style::section(ui, "Markers", |ui| markers_body(ui, song, draft, actions));
}

/// Draws section names and the field for naming the section at the cursor.
fn markers_body(ui: &mut Ui, song: &Song, draft: &mut Option<String>, actions: &mut Vec<Action>) {
    let editable = song.read_only().is_none();
    if editable {
        table(ui, "section here", |ui| {
            let caption = ui.weak("Section here");
            let mut text = draft
                .take()
                .unwrap_or_else(|| song.section().unwrap_or_default().to_owned());
            let field = ui
                .add(
                    egui::TextEdit::singleline(&mut text)
                        .hint_text("none")
                        .desired_width(WIDE),
                )
                .labelled_by(caption.id);
            let entered =
                field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if field.has_focus() {
                *draft = Some(text);
            } else if entered {
                actions.push(Action::SetSection(text.trim().to_owned()));
            }
            ui.end_row();
        });
    }
    let document = song.document().document();
    if document.markers.is_empty() {
        ui.weak("No markers");
        return;
    }
    let timing = song.timing();
    egui::Grid::new("markers")
        .num_columns(4)
        .min_col_width(0.0)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            for marker in &document.markers {
                let at = timing.grid_position(marker.tick).to_string();
                ui.label(RichText::new(at).monospace().color(style::WEAK_TEXT));
                let kind = match &marker.kind {
                    MarkerKind::Section => "section".to_owned(),
                    MarkerKind::Bookmark => "bookmark".to_owned(),
                    MarkerKind::Other(key) => key.to_string(),
                };
                ui.weak(kind);
                let text = if marker.text.is_empty() {
                    "(unnamed)"
                } else {
                    marker.text.as_str()
                };
                let here = marker.tick == song.cursor();
                let label =
                    egui::Button::selectable(here, text).wrap_mode(egui::TextWrapMode::Truncate);
                if ui
                    .add_sized([120.0, 20.0], label)
                    .on_hover_text(text)
                    .clicked()
                {
                    actions.push(Action::JumpTo(marker.tick));
                }
                if editable {
                    let remove = style::quiet_icon(
                        ui,
                        style::icon::TRASH,
                        &format!("Remove marker {text}"),
                        "Remove this marker; Ctrl+Z brings it back",
                    );
                    if remove.clicked() {
                        actions.push(Action::RemoveMarker(marker.key()));
                    }
                }
                ui.end_row();
            }
        });
}
