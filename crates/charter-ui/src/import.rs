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
    /// Whether the import dialog is showing.
    pub open: bool,
    /// Which of the plugins that read timing reads it, by its place among them.
    format: usize,
    /// Text pasted into the dialog or read by the host from a file.
    pub(crate) text: String,
    /// The last text read and how it read, so a long paste is read once, not every frame.
    read: Option<(String, usize, Result<ImportedTiming, String>)>,
}

impl Import {
    /// Forces the next display to read the current text and selected format again.
    pub(crate) fn invalidate(&mut self) {
        self.read = None;
    }
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
    let space = ctx.content_rect().size();
    let compact = space.x < 600.0 || space.y < 500.0;
    let width = 560.0_f32.min((space.x - 24.0).max(260.0));
    let height = 460.0_f32.min((space.y - 24.0).max(180.0));
    egui::Window::new("Import timing")
        .open(&mut open)
        .collapsible(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .default_size([width, height])
        .show(ctx, |ui| {
            if compact {
                egui::ScrollArea::vertical()
                    .max_height(height - 42.0)
                    .show(ui, |ui| {
                        done = body(
                            ui,
                            song,
                            &formats,
                            import,
                            actions,
                            (height - 220.0).max(60.0),
                        );
                    });
            } else {
                done = body(ui, song, &formats, import, actions, 200.0);
            }
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
    text_height: f32,
) -> bool {
    import.format = import.format.min(formats.len().saturating_sub(1));
    let Some(&(name, _)) = formats.get(import.format) else {
        ui.weak("No game charter knows reads timing.");
        return false;
    };
    ui.horizontal(|ui| {
        ui.label("From");
        if formats.len() > 1 {
            egui::ComboBox::from_id_salt("timing format")
                .selected_text(name)
                .show_ui(ui, |ui| {
                    for (index, (format_name, _)) in formats.iter().enumerate() {
                        ui.selectable_value(&mut import.format, index, *format_name);
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
    let Some(&(_, plugin)) = formats.get(import.format) else {
        return false;
    };
    ui.weak("Paste the section that holds the timing, or a whole file.");
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .max_height(text_height)
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
    let mut close = false;
    ui.horizontal(|ui| {
        let enabled = editable && imported.is_some();
        let replace = ui.add_enabled_ui(enabled, |ui| {
            style::icon_button(ui, icon::DOWNLOAD_SIMPLE, "Replace the timing", true)
        });
        if let (true, Some(imported)) = (replace.inner.clicked(), imported) {
            actions.push(Action::ImportTiming(Box::new(imported.timing.clone())));
            close = true;
        }
        if ui.button("Cancel").clicked() {
            close = true;
        }
    });
    close
}

/// What `imported` holds, and what its format had that it leaves out.
fn summary(ui: &mut Ui, imported: &ImportedTiming) {
    let events = &imported.timing.events;
    let tempos: Vec<_> = imported
        .timing
        .anchors
        .iter()
        .map(|_| imported.timing.tempo)
        .collect();
    let meters = events
        .iter()
        .filter(|event| matches!(event.kind, TimingKind::TimeSignature(_)))
        .count();
    let others = events.len().saturating_sub(meters);
    let offset = imported
        .timing
        .anchors
        .first()
        .map_or(0.0, |anchor| f64::from(anchor.seconds) * 1000.0);
    style::table(ui, "imported", |ui| {
        ui.weak("Offset");
        ui.label(format!("{} ms", trimmed(offset)));
        ui.end_row();
        ui.weak("Tempo");
        let first = tempos.first().map_or_else(String::new, ToString::to_string);
        let changes = match tempos.len() {
            0 | 1 => "no changes".to_owned(),
            2 => "1 change".to_owned(),
            count => format!("{} changes", count.saturating_sub(1)),
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
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[derive(Debug)]
    struct Importer;

    impl RulesetPlugin for Importer {
        fn id(&self) -> &'static str {
            "importer"
        }

        fn check(
            &self,
            _: &charter_core::ValidDocument,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        fn timing_format(&self) -> Option<&str> {
            Some("test timing")
        }

        fn import_timing(
            &self,
            text: &str,
            into: &charter_core::ValidDocument,
        ) -> Result<ImportedTiming, Box<dyn std::error::Error + Send + Sync>> {
            if text != "good" && text != "three" {
                return Err("bad timing".into());
            }
            let mut timing = into.document().timing.clone();
            if text == "three" {
                timing.anchors.push(charter_core::Anchor {
                    tick: charter_core::Tick::new(100_000),
                    seconds: charter_core::Origin::try_from(50.0)?,
                });
            }
            timing.events.push(charter_core::TimingEvent {
                tick: charter_core::Tick::new(0),
                kind: TimingKind::Other {
                    name: "test.scroll".parse()?,
                    value: serde_json::json!(1.5),
                },
                ext: std::collections::BTreeMap::new(),
            });
            Ok(ImportedTiming {
                timing,
                left_out: vec!["visual effects".to_owned()],
            })
        }
    }

    #[derive(Debug)]
    struct Sm;

    impl RulesetPlugin for Sm {
        fn id(&self) -> &'static str {
            "sm"
        }

        fn check(
            &self,
            _: &charter_core::ValidDocument,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct OtherImporter;

    impl RulesetPlugin for OtherImporter {
        fn id(&self) -> &'static str {
            "other-importer"
        }

        fn check(
            &self,
            _: &charter_core::ValidDocument,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        fn timing_format(&self) -> Option<&str> {
            Some("alternate timing")
        }
    }

    const PLUGINS: &[&dyn RulesetPlugin] = &[&Sm, &Importer];

    fn preview(
        editable: bool,
        plugins: &'static [&'static dyn RulesetPlugin],
    ) -> Harness<'static, (Import, Vec<Action>)> {
        let song = Song::new(
            charter_editor::demo::song(),
            if editable { PLUGINS } else { &[] },
        );
        Harness::builder().build_ui_state(
            move |ui, (import, actions): &mut (Import, Vec<Action>)| {
                show(ui.ctx(), Some(&song), plugins, import, actions);
            },
            (
                Import {
                    open: true,
                    ..Import::default()
                },
                Vec::new(),
            ),
        )
    }

    #[test]
    fn a_plugin_without_timing_import_offers_no_format_or_apply_action() {
        let mut harness = preview(true, &[&Sm]);
        harness.state_mut().0.text = "good".to_owned();
        harness.run();
        assert!(!offered(&[&Sm]));
        assert!(
            harness
                .query_by_label_contains("No game charter knows")
                .is_some()
        );
        assert!(harness.query_by_label("Replace the timing").is_none());
        assert!(harness.state().1.is_empty());
    }

    #[test]
    fn malformed_pasted_timing_shows_an_error_and_cannot_be_applied() {
        let mut harness = preview(true, PLUGINS);
        harness.state_mut().0.text = "garbage".to_owned();
        harness.run();
        assert!(harness.query_by_label_contains("bad timing").is_some());
        harness.get_by_label("Replace the timing").click();
        harness.run();
        assert!(harness.state().0.open);
        assert!(harness.state().1.is_empty());
    }

    #[test]
    fn changing_pasted_text_refreshes_the_preview_and_imports_its_timing() {
        let mut harness = preview(true, PLUGINS);
        harness.state_mut().0.text = "garbage".to_owned();
        harness.run();
        harness.state_mut().0.text = "good".to_owned();
        harness.run();
        assert!(harness.query_by_label_contains("visual effects").is_some());
        assert!(harness.query_by_label_contains("Game events").is_some());
        assert!(harness.query_by_label_contains("1 change").is_some());
        harness.get_by_label("Replace the timing").click();
        harness.run();
        assert!(!harness.state().0.open);
        assert!(matches!(
            harness.state().1.as_slice(),
            [Action::ImportTiming(_)]
        ));
    }

    #[test]
    fn multiple_imported_tempo_changes_are_counted_in_the_preview() {
        let mut harness = preview(true, PLUGINS);
        harness.state_mut().0.text = "three".to_owned();
        harness.run();
        assert!(harness.query_by_label_contains("2 changes").is_some());
    }

    #[test]
    fn a_read_only_song_previews_timing_but_refuses_to_replace_it() {
        let mut harness = preview(false, PLUGINS);
        harness.state_mut().0.text = "good".to_owned();
        harness.run();
        assert!(harness.query_by_label_contains("visual effects").is_some());
        harness.get_by_label("Replace the timing").click();
        harness.run();
        assert!(harness.state().0.open);
        assert!(harness.state().1.is_empty());
    }

    #[test]
    fn switching_import_formats_rejects_the_old_formats_cached_preview() {
        let mut harness = preview(true, &[&Sm, &Importer, &OtherImporter]);
        harness.state_mut().0.text = "good".to_owned();
        harness.run();
        assert!(harness.query_by_label_contains("visual effects").is_some());
        harness.get_by_role(Role::ComboBox).click();
        harness.run();
        harness.get_by_label("alternate timing").click();
        harness.run();
        assert_eq!(harness.state().0.format, 1);
        assert!(
            harness
                .query_by_label_contains("imports no timing")
                .is_some()
        );
        assert!(harness.state().1.is_empty());
    }

    #[test]
    fn import_can_be_cancelled_in_a_narrow_window() {
        let plugins: &[&dyn RulesetPlugin] = &[&Importer];
        let song = Song::open(&charter_editor::demo::song().to_json().unwrap(), plugins).unwrap();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build_ui_state(
                move |ui, (import, actions): &mut (Import, Vec<Action>)| {
                    show(ui.ctx(), Some(&song), plugins, import, actions);
                },
                (
                    Import {
                        open: true,
                        ..Import::default()
                    },
                    Vec::new(),
                ),
            );
        harness.run();
        let cancel = harness.get_by_label("Cancel");
        assert!(cancel.rect().left() >= 0.0, "{:?}", cancel.rect());
        assert!(cancel.rect().right() <= 400.0, "{:?}", cancel.rect());
        assert!(cancel.rect().bottom() <= 300.0, "{:?}", cancel.rect());
        cancel.click();
        harness.run();
        assert!(!harness.state().0.open);
    }

    #[test]
    fn milliseconds_show_without_trailing_zeros() {
        assert_eq!(trimmed(895.0), "895");
        assert_eq!(trimmed(895.25), "895.25");
        assert_eq!(trimmed(-14.0), "-14");
    }
}
