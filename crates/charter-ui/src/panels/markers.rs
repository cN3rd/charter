use super::song::WIDE;
use super::{Action, MarkerKind, RichText, Song, Ui, egui, style, table};

/// The song's markers, each of which jumps the cursor to it when clicked. In a song open for
/// editing, Enter or leaving the field names the section at the cursor, starting one if there
/// is none. Escape cancels the draft, and each marker can be removed.
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
            let current = song.section().unwrap_or_default();
            let mut text = draft.take().unwrap_or_else(|| current.to_owned());
            let field = ui
                .add(
                    egui::TextEdit::singleline(&mut text)
                        .hint_text("none")
                        .desired_width(WIDE),
                )
                .labelled_by(caption.id);
            let left = field.lost_focus()
                && !ui.input(|input| input.key_pressed(egui::Key::Escape))
                && text.trim() != current;
            if field.has_focus() {
                *draft = Some(text);
            } else if left {
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
                // Read from the left like the rest of the list, not centred in the cell.
                let label = egui::Button::selectable(here, (text, egui::Atom::grow()))
                    .wrap_mode(egui::TextWrapMode::Truncate);
                // ponytail: a fixed width, the whole name on hover. Filling the panel needs the
                // other columns measured first: a Grid learns its widths a frame late, so a cell
                // sized from what is left keeps the grid laying itself out again.
                if ui
                    .add_sized([120.0, ui.spacing().interact_size.y], label)
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

#[cfg(test)]
mod tests {
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;

    fn editable_markers() -> Harness<'static, (Song, Option<String>, Vec<Action>)> {
        let song = Song::new(charter_editor::demo::song(), &[&charter_stepmania::Plugin]);
        let mut harness = Harness::builder()
            .with_size(egui::vec2(650.0, 900.0))
            .build_ui_state(
                |ui, (song, draft, actions): &mut (Song, Option<String>, Vec<Action>)| {
                    markers(ui, song, draft, actions);
                    ui.button("Elsewhere").clicked();
                },
                (song, None, Vec::new()),
            );
        harness.run();
        harness
    }

    #[test]
    fn leaving_a_changed_section_field_sets_its_name_without_enter() {
        let mut harness = editable_markers();
        harness
            .get_by_role_and_label(Role::TextInput, "Section here")
            .focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness
            .get_by_role_and_label(Role::TextInput, "Section here")
            .type_text("Opening");
        harness.run();
        harness.get_by_label("Elsewhere").click();
        harness.run();
        assert_eq!(
            harness.state().2,
            [Action::SetSection("Opening".to_owned())]
        );
    }

    #[test]
    fn leaving_an_unchanged_section_field_sends_no_edit() {
        let mut harness = editable_markers();
        harness
            .get_by_role_and_label(Role::TextInput, "Section here")
            .focus();
        harness.run();
        harness.get_by_label("Elsewhere").click();
        harness.run();
        assert!(harness.state().2.is_empty());
    }

    #[test]
    fn escape_discards_a_section_draft_without_editing_it() {
        let mut harness = editable_markers();
        harness
            .get_by_role_and_label(Role::TextInput, "Section here")
            .focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness
            .get_by_role_and_label(Role::TextInput, "Section here")
            .type_text("Opening");
        harness.run();
        harness.key_press(egui::Key::Escape);
        harness.run();
        assert!(harness.state().2.is_empty());
        assert!(harness.state().1.is_none());
        assert_eq!(
            harness
                .get_by_role_and_label(Role::TextInput, "Section here")
                .value()
                .as_deref(),
            Some("Intro")
        );
    }
}
