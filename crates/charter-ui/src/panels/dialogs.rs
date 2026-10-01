use super::empty::notice;
use super::{Align, Layout, RichText, Ui, egui, fields, icon, style};

/// What to do with unsaved changes that are about to be lost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Answer {
    /// Save the changes before leaving.
    Save,
    /// Leave without saving the changes.
    DontSave,
    /// Stay in the song without losing its changes.
    Cancel,
}

/// Asks whether to save the changes to the song in `name` before they are lost. Escape or a
/// click outside cancels.
pub(crate) fn ask_to_save(ui: &Ui, name: &str) -> Option<Answer> {
    let mut answer = None;
    let modal = egui::Modal::new(egui::Id::new("save changes")).show(ui.ctx(), |ui| {
        ui.set_max_width(380.0);
        ui.label(
            RichText::new(format!("Save changes to {name}?"))
                .family(style::titles(ui))
                .size(15.0),
        );
        ui.add_space(4.0);
        ui.label("Changes are lost unless you save them.");
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.add(style::primary("Save")).clicked() {
                answer = Some(Answer::Save);
            }
            if ui.button("Don't save").clicked() {
                answer = Some(Answer::DontSave);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(Answer::Cancel);
            }
        });
    });
    answer.or_else(|| modal.should_close().then_some(Answer::Cancel))
}

/// Asks whether to take the title and artist an audio file's tags carry over the song's own:
/// true to take them. Only an answer closes it.
pub(crate) fn offer_tags(ui: &Ui, title: Option<&str>, artist: Option<&str>) -> Option<bool> {
    let mut answer = None;
    egui::Modal::new(egui::Id::new("offer tags")).show(ui.ctx(), |ui| {
        ui.set_max_width(380.0);
        let heading = RichText::new("Import song metadata?")
            .family(style::titles(ui))
            .size(15.0);
        ui.label(heading);
        ui.add_space(4.0);
        ui.label("Do you want to import song metadata from tags?");
        ui.add_space(8.0);
        let rows: Vec<_> = [("Title", title), ("Artist", artist)]
            .into_iter()
            .filter_map(|(name, value)| value.map(|value| (name, value.to_owned())))
            .collect();
        fields(ui, "offered tags", &rows, "");
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.add(style::primary("Import")).clicked() {
                answer = Some(true);
            }
            if ui.button("Keep mine").clicked() {
                answer = Some(false);
            }
        });
    });
    answer
}

/// Asks whether to restore the unsaved changes to the song in `name` that an earlier run kept:
/// true to restore them, false to throw them away. Only an answer closes it.
pub(crate) fn ask_to_restore(ui: &Ui, name: &str) -> Option<bool> {
    let mut answer = None;
    egui::Modal::new(egui::Id::new("restore changes")).show(ui.ctx(), |ui| {
        ui.set_max_width(380.0);
        let question = format!("Restore unsaved changes to {name}?");
        ui.label(RichText::new(question).family(style::titles(ui)).size(15.0));
        ui.add_space(4.0);
        ui.label("charter closed before they were saved.");
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.add(style::primary("Restore")).clicked() {
                answer = Some(true);
            }
            if ui.button("Discard").clicked() {
                answer = Some(false);
            }
        });
    });
    answer
}

/// Whether to download the beat tracker's weights, which the user answers: true for Download.
pub(crate) fn ask_to_download_weights(ui: &Ui, tracker: crate::Tracker) -> Option<bool> {
    let mut answer = None;
    egui::Modal::new(egui::Id::new("download weights")).show(ui.ctx(), |ui| {
        ui.set_max_width(420.0);
        let question = "Download the beat tracker?";
        ui.label(RichText::new(question).family(style::titles(ui)).size(15.0));
        ui.add_space(4.0);
        ui.label(match tracker {
            crate::Tracker::BeatThis => {
                "Finding the timing uses Beat This!, a beat tracker from the Institute of \
                 Computational Perception at JKU Linz, released under the MIT licence. Its \
                 weights are 81 MB, downloaded once from the authors' server, cloud.cp.jku.at, \
                 into charter's data folder. Nothing about the song is sent."
            }
            crate::Tracker::Mapperatorinator => {
                "Finding the timing uses the timing model of Mapperatorinator by OliBomby, \
                 released under the MIT licence, and runs on the graphics card. Its weights are \
                 416 MB, downloaded once from Hugging Face, huggingface.co, into charter's data \
                 folder. Nothing about the song is sent."
            }
        });
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.add(style::primary("Download")).clicked() {
                answer = Some(true);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(false);
            }
        });
    });
    answer
}

/// Whether to find the timing on the CPU, there being no graphics card, which the user answers:
/// true for Run on the CPU.
pub(crate) fn ask_to_run_on_cpu(ui: &Ui) -> Option<bool> {
    let mut answer = None;
    egui::Modal::new(egui::Id::new("run on cpu")).show(ui.ctx(), |ui| {
        ui.set_max_width(420.0);
        let question = "No graphics card found";
        ui.label(RichText::new(question).family(style::titles(ui)).size(15.0));
        ui.add_space(4.0);
        ui.label(
            "Mapperatorinator runs on the graphics card, and charter found none it can use. It can \
             run on the CPU instead, but much more slowly: a few minutes of song can take over a \
             quarter of an hour, during which the timing cannot be found again.",
        );
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.add(style::primary("Run on the CPU")).clicked() {
                answer = Some(true);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(false);
            }
        });
    });
    answer
}

/// Which of `names` a file is, for formats that share its extension: the index the user picks
/// with `verb`, such as Import, or false for Cancel. `chosen` is the pick so far.
pub(crate) fn choose_format(
    ui: &Ui,
    verb: &str,
    names: &[String],
    chosen: &mut usize,
) -> Option<bool> {
    let mut answer = None;
    egui::Modal::new(egui::Id::new("choose format")).show(ui.ctx(), |ui| {
        ui.set_max_width(420.0);
        let question = "Which game is the file for?";
        ui.label(RichText::new(question).family(style::titles(ui)).size(15.0));
        ui.add_space(4.0);
        ui.label("Games read the same kind of file their own ways, so the notes depend on which.");
        ui.add_space(8.0);
        for (index, name) in names.iter().enumerate() {
            ui.radio_value(chosen, index, name.as_str());
        }
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.add(style::primary(verb)).clicked() {
                answer = Some(true);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(false);
            }
        });
    });
    answer
}

/// Whether to reset the open chart's timing, which the user answers: true for Reset.
pub(crate) fn ask_to_reset_timing(ui: &Ui) -> Option<bool> {
    let mut answer = None;
    egui::Modal::new(egui::Id::new("reset timing")).show(ui.ctx(), |ui| {
        ui.set_max_width(400.0);
        let question = "Reset the timing?";
        ui.label(RichText::new(question).family(style::titles(ui)).size(15.0));
        ui.add_space(4.0);
        ui.label(
            "Every BPM change, meter, stop and game timing event of the open chart's timing goes, \
             and it plays at 120 BPM in 4/4 from 0 ms. Notes keep their beats. Undo brings the \
             timing back.",
        );
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.add(style::primary("Reset")).clicked() {
                answer = Some(true);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(false);
            }
        });
    });
    answer
}

/// Why the last file did not open, or the last edit was refused, floating at the foot of the
/// area `ui` fills. Says whether the user dismissed it.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "egui position offsets use vector subtraction"
)]
pub(crate) fn toast(ui: &Ui, message: &str) -> bool {
    let area = ui.max_rect();
    let width = (area.width() - 32.0).clamp(160.0, 640.0);
    egui::Area::new(ui.id().with("toast"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_BOTTOM)
        .fixed_pos(area.center_bottom() - egui::vec2(0.0, 16.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .fill(style::NOTICE)
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(width);
                    ui.horizontal(|ui| {
                        ui.label(style::glyph(ui, icon::WARNING).color(style::WARN));
                        ui.add(egui::Label::new(RichText::new(message).color(style::WARN)).wrap());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let tip = "Dismiss this message";
                            style::quiet_icon(ui, icon::X, "Dismiss", tip).clicked()
                        })
                        .inner
                    })
                    .inner
                })
                .inner
        })
        .inner
}

/// Why the open song cannot be edited.
pub(crate) fn read_only(ui: &mut Ui, why: &str) {
    notice(ui, style::icon::LOCK_SIMPLE, |ui| {
        ui.label(format!("Read only: {why}"));
    });
    ui.add_space(4.0);
}
