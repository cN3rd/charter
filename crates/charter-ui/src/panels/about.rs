//! The About window: what charter is, which build this is, its licence and its credits.

use super::{egui, heading};

/// Where charter's source, releases and issues are.
const REPOSITORY: &str = "https://github.com/cN3rd/charter";
/// Where CI publishes the book, user guide first.
const BOOK: &str = "https://cn3rd.github.io/charter/";

/// The build's version, and the commit it was built from when packaging named one.
fn version_line(version: &str, commit: Option<&str>) -> String {
    match commit {
        Some(commit) => format!("Version {version}, commit {commit}"),
        None => format!("Version {version}, development build"),
    }
}

/// The About window, while `open` is set.
#[expect(
    clippy::non_ascii_literal,
    reason = "Jan Schl\u{fc}ter's name is credited as he spells it"
)]
pub(crate) fn about(ctx: &egui::Context, open: &mut bool) {
    let mut shown = *open;
    let mut closed = false;
    egui::Window::new("About charter")
        .open(&mut shown)
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.heading("charter");
            ui.label(version_line(
                env!("CARGO_PKG_VERSION"),
                option_env!("CHARTER_COMMIT"),
            ));
            ui.label("A chart editor for rhythm games: StepMania, Tap Tap Revenge and osu!mania.");
            ui.label("Licensed under MIT or Apache-2.0, at your choice.");
            ui.horizontal_wrapped(|ui| {
                ui.hyperlink_to("User guide", BOOK);
                ui.hyperlink_to("Source code", REPOSITORY);
                ui.hyperlink_to("Releases", format!("{REPOSITORY}/releases"));
                ui.hyperlink_to("Report a problem", format!("{REPOSITORY}/issues"));
            });
            ui.add_space(8.0);
            heading(ui, "Credits");
            for credit in [
                "Inter, by the Inter Project Authors, under the SIL Open Font License 1.1.",
                "Phosphor icons, by Helena Zhang and Tobias Fried, under the MIT licence.",
                "The metronome and note-tick sounds, made for charter and dedicated to the public \
                 domain (CC0).",
                "Beat This!, by Francesco Foscarin, Jan Schlüter and Gerhard Widmer, reimplemented \
                 in Rust; its weights download from its authors.",
                "Mapperatorinator, by OliBomby, under the MIT licence, ported to Rust; its \
                 weights download from Hugging Face.",
                "The Rust crates charter is built from are listed with their licences in \
                 licenses/THIRD-PARTY.md beside the program.",
            ] {
                ui.label(credit);
            }
            ui.add_space(8.0);
            if ui.button("Close").clicked() {
                closed = true;
            }
        });
    *open = shown && !closed;
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn a_packaged_build_names_its_commit_and_a_development_build_says_so() {
        assert_eq!(
            version_line("0.3.0", Some("1a2b3c4d5e6f")),
            "Version 0.3.0, commit 1a2b3c4d5e6f"
        );
        assert_eq!(
            version_line("0.3.0", None),
            "Version 0.3.0, development build"
        );
    }

    #[test]
    fn the_window_shows_the_version_licence_and_credits_and_closes() {
        let mut harness =
            Harness::builder().build_ui_state(|ui, open: &mut bool| about(ui.ctx(), open), true);
        harness.run();
        let version = version_line(env!("CARGO_PKG_VERSION"), option_env!("CHARTER_COMMIT"));
        for shown in [
            version.as_str(),
            "MIT or Apache-2.0",
            "Inter",
            "Mapperatorinator",
            "Beat This!",
        ] {
            assert!(harness.query_by_label_contains(shown).is_some(), "{shown}");
        }
        harness.get_by_label("Close").click();
        harness.run();
        assert!(!*harness.state());
    }
}
