use super::{Action, Ui, Value, egui};

/// A setting's value as text: strings bare, anything else as JSON.
pub(super) fn shown(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// A value typed into a field named by `caption`, showing `current` until typing starts and
/// kept in `draft` while the field has the keyboard. Entered, `read` turns it into an action,
/// or into why it is not a value; Escape leaves it.
pub(crate) fn entry(
    ui: &mut Ui,
    caption: egui::Id,
    current: &str,
    draft: &mut Option<String>,
    read: impl FnOnce(&str) -> Result<Action, String>,
    actions: &mut Vec<Action>,
) -> Option<String> {
    entry_sized(ui, (caption, 72.0), current, draft, read, actions)
}

/// [`entry`] `width` points wide.
pub(crate) fn entry_sized(
    ui: &mut Ui,
    (caption, width): (egui::Id, f32),
    current: &str,
    draft: &mut Option<String>,
    read: impl FnOnce(&str) -> Result<Action, String>,
    actions: &mut Vec<Action>,
) -> Option<String> {
    let had_draft = draft.is_some();
    let mut text = draft.take().unwrap_or_else(|| current.to_owned());
    let empty = current.is_empty();
    let field = egui::TextEdit::singleline(&mut text).desired_width(width);
    let field = if empty {
        field.hint_text("none")
    } else {
        field
    };
    let field = ui.add(field).labelled_by(caption);
    // Leaving the field takes what was typed, as Enter does; only Escape drops it. A field left
    // as it was does nothing.
    let left = field.lost_focus()
        && !ui.input(|input| input.key_pressed(egui::Key::Escape))
        && text.trim() != current;
    if field.has_focus() {
        *draft = Some(text);
    } else if left {
        match read(text.trim()) {
            Ok(action) => actions.push(action),
            Err(why) => {
                *draft = Some(text);
                return Some(why);
            }
        }
    } else if had_draft
        && text.trim() != current
        && !ui.input(|input| input.key_pressed(egui::Key::Escape))
    {
        *draft = Some(text);
    }
    None
}
