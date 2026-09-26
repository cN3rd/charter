//! The import timing window: timing pasted or read from another program's format, such as osu!'s
//! `[TimingPoints]`, shown for what it holds before it takes the place of the song's. Any ruleset
//! plugin can read a format; the window offers each one that does.

use charter_core::{ImportedTiming, RulesetPlugin, TimingKind};
use charter_editor::Song;
use egui::{RichText, Ui};

use crate::style::{self, icon};
use crate::window::Action;

/// Whether the window shows, which format it reads, and what has been pasted into it.
#[derive(Debug, Default)]
pub struct Import {
    pub open: bool,
    /// Which of the plugins that read timing reads it, by its place among them.
    format: usize,
    pub(crate) text: String,
    /// The last text read and how it read, so a long paste is read once, not every frame.
    read: Option<(String, usize, Result<ImportedTiming, String>)>,
}

/// The plugins that read a timing format, with the format's name.
fn formats<'a>(
    plugins: &'a [&'static dyn RulesetPlugin],
) -> Vec<(&'a str, &'static dyn RulesetPlugin)> {
    plugins
        .iter()
        .filter_map(|plugin| plugin.timing_format().map(|name| (name, *plugin)))
        .collect()
}

/// Whether any of `plugins` reads timing, so there is anything to import.
pub(crate) fn offered(plugins: &[&'static dyn RulesetPlugin]) -> bool {
    !formats(plugins).is_empty()
}

/// Draws the import window over the open `song` while it is open.
pub(crate) fn show(
    ctx: &egui::Context,
    song: Option<&Song>,
    plugins: &[&'static dyn RulesetPlugin],
    import: &mut Import,
    actions: &mut Vec<Action>,
) {
    let (Some(song), true) = (song, import.open) else {
        return;
    };
    let formats = formats(plugins);
    let mut open = import.open;
    let mut done = false;
    egui::Window::new("Import timing")
        .open(&mut open)
        .collapsible(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .default_size([560.0, 460.0])
        .show(ctx, |ui| {
            done = body(ui, song, &formats, import, actions);
        });
    import.open = open && !done;
}

/// The window's contents. Says whether the timing was taken, which closes it.
fn body(
    ui: &mut Ui,
    song: &Song,
    formats: &[(&str, &'static dyn RulesetPlugin)],
    import: &mut Import,
    actions: &mut Vec<Action>,
) -> bool {
    import.format = import.format.min(formats.len().saturating_sub(1));
    let Some(&(name, plugin)) = formats.get(import.format) else {
        ui.weak("No game charter knows reads timing.");
        return false;
    };
    ui.horizontal(|ui| {
        ui.label("From");
        if formats.len() > 1 {
            egui::ComboBox::from_id_salt("timing format")
                .selected_text(name)
                .show_ui(ui, |ui| {
                    for (index, (name, _)) in formats.iter().enumerate() {
                        ui.selectable_value(&mut import.format, index, *name);
                    }
                });
        } else {
            ui.strong(name);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if style::icon_button(ui, icon::FOLDER_OPEN, "Read a file…", false).clicked() {
                actions.push(Action::ChooseTimingFile);
            }
        });
    });
    ui.weak("Paste the section that holds the timing, or a whole file.");
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .max_height(200.0)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut import.text)
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(10)
                    .desired_width(f32::INFINITY)
                    .hint_text("[TimingPoints]\n895,288.3229,4,1,0,60,1,0"),
            );
        });
    ui.add_space(6.0);
    let stale = import
        .read
        .as_ref()
        .is_none_or(|(text, format, _)| *text != import.text || *format != import.format);
    if stale {
        let read = plugin
            .import_timing(&import.text, song.document())
            .map_err(|why| why.to_string());
        import.read = Some((import.text.clone(), import.format, read));
    }
    let Some((_, _, read)) = &import.read else {
        return false;
    };
    let imported = if import.text.trim().is_empty() {
        ui.weak("Nothing pasted yet.");
        None
    } else {
        match read {
            Ok(imported) => {
                summary(ui, imported);
                Some(imported)
            }
            Err(why) => {
                ui.colored_label(style::WARN, format!("This does not read: {why}"));
                None
            }
        }
    };
    ui.add_space(6.0);
    ui.label(
        RichText::new(
            "This replaces the tempo, meters and offset of the open chart's timing. Notes keep \
             their beats, so they move with the new tempo; undo takes it back.",
        )
        .small()
        .color(style::WEAK_TEXT),
    );
    ui.add_space(8.0);
    let editable = song.read_only().is_none();
    let mut taken = false;
    ui.horizontal(|ui| {
        let enabled = editable && imported.is_some();
        let replace = ui.add_enabled_ui(enabled, |ui| {
            style::icon_button(ui, icon::DOWNLOAD_SIMPLE, "Replace the timing", true)
        });
        if let (true, Some(imported)) = (replace.inner.clicked(), imported) {
            actions.push(Action::ImportTiming(Box::new(imported.timing.clone())));
            taken = true;
        }
    });
    taken
}

/// What `imported` holds, and what its format had that it leaves out.
fn summary(ui: &mut Ui, imported: &ImportedTiming) {
    let events = &imported.timing.events;
    let tempos: Vec<_> = events
        .iter()
        .filter_map(|event| match event.kind {
            TimingKind::Bpm(bpm) => Some(bpm),
            _ => None,
        })
        .collect();
    let meters = events
        .iter()
        .filter(|event| matches!(event.kind, TimingKind::TimeSignature(_)))
        .count();
    let others = events.len() - tempos.len() - meters;
    let offset = f64::from(imported.timing.offset_seconds) * 1000.0;
    style::table(ui, "imported", |ui| {
        ui.weak("Offset");
        ui.label(format!("{} ms", trimmed(offset)));
        ui.end_row();
        ui.weak("Tempo");
        let first = tempos.first().map_or_else(String::new, ToString::to_string);
        let changes = match tempos.len() {
            0 | 1 => "no changes".to_owned(),
            2 => "1 change".to_owned(),
            count => format!("{} changes", count - 1),
        };
        ui.label(format!("{first} BPM at the start, {changes}"));
        ui.end_row();
        ui.weak("Meters");
        ui.label(meters.to_string());
        ui.end_row();
        if others > 0 {
            ui.weak("Game events");
            ui.label(format!("{others}, such as scroll speeds"));
            ui.end_row();
        }
    });
    if !imported.left_out.is_empty() {
        ui.add_space(4.0);
        ui.weak(format!("Left out: {}.", imported.left_out.join("; ")));
    }
}

/// `value` to three places, without trailing zeros.
fn trimmed(value: f64) -> String {
    let text = format!("{value:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milliseconds_show_without_trailing_zeros() {
        assert_eq!(trimmed(895.0), "895");
        assert_eq!(trimmed(895.25), "895.25");
        assert_eq!(trimmed(-14.0), "-14");
    }
}
