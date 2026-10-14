use super::empty::notice;
use super::{Align, Layout, RichText, Ui, egui, fields, icon, style};

/// What to do with unsaved changes that are about to be lost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SaveChanges {
    /// Save the changes before leaving.
    Save,
    /// Leave without saving the changes.
    DontSave,
    /// Stay in the song without losing its changes.
    Cancel,
}

/// Which song metadata to keep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OfferTags {
    /// Import the tags.
    Import,
    /// Keep the song's own values.
    KeepMine,
}

/// What to do with unsaved changes from an earlier run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Restore {
    /// Restore the changes.
    Restore,
    /// Discard the changes.
    Discard,
}

/// Whether to download the tracker's weights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DownloadWeights {
    /// Download the weights.
    Download,
    /// Cancel the request.
    Cancel,
}

/// Answer to help installing `FFmpeg`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FfmpegAnswer {
    /// Copy the package manager's exact install command.
    CopyCommand,
    /// Download a release build into charter's data folder.
    Download,
    /// Dismiss the help.
    Later,
}

/// Whether to run the tracker on the CPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RunOnCpu {
    /// Run on the CPU.
    Run,
    /// Cancel the request.
    Cancel,
}

/// Whether to use the selected file format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChooseFormat {
    /// Use the selected format.
    Choose,
    /// Cancel the import or export.
    Cancel,
}

/// Whether to reset the open chart's timing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResetTiming {
    /// Reset the timing.
    Reset,
    /// Keep the timing.
    Cancel,
}

/// Asks whether to save the changes to the song in `name` before they are lost. Escape or a
/// click outside cancels.
pub(crate) fn ask_to_save(ui: &Ui, name: &str) -> Option<SaveChanges> {
    let mut answer = None;
    let modal = egui::Modal::new(egui::Id::new("save changes")).show(ui.ctx(), |ui| {
        ui.set_max_width(380.0);
        ui.label(style::dialog_title(ui, &format!("Save changes to {name}?")));
        ui.add_space(4.0);
        ui.label("Changes are lost unless you save them.");
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.add(style::primary("Save")).clicked() {
                answer = Some(SaveChanges::Save);
            }
            if ui.button("Don't save").clicked() {
                answer = Some(SaveChanges::DontSave);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(SaveChanges::Cancel);
            }
        });
    });
    answer.or_else(|| modal.should_close().then_some(SaveChanges::Cancel))
}

/// Asks whether to take the title and artist an audio file's tags carry over the song's own.
/// Only an answer closes it.
pub(crate) fn offer_tags(ui: &Ui, title: Option<&str>, artist: Option<&str>) -> Option<OfferTags> {
    let mut answer = None;
    egui::Modal::new(egui::Id::new("offer tags")).show(ui.ctx(), |ui| {
        ui.set_max_width(380.0);
        let heading = style::dialog_title(ui, "Import song metadata?");
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
                answer = Some(OfferTags::Import);
            }
            if ui.button("Keep mine").clicked() {
                answer = Some(OfferTags::KeepMine);
            }
        });
    });
    answer
}

/// Asks whether to restore the unsaved changes to the song in `name` that an earlier run kept.
/// Only an answer closes it.
pub(crate) fn ask_to_restore(ui: &Ui, name: &str) -> Option<Restore> {
    let mut answer = None;
    egui::Modal::new(egui::Id::new("restore changes")).show(ui.ctx(), |ui| {
        ui.set_max_width(380.0);
        let question = format!("Restore unsaved changes to {name}?");
        ui.label(style::dialog_title(ui, &question));
        ui.add_space(4.0);
        ui.label("charter closed before they were saved.");
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.add(style::primary("Restore")).clicked() {
                answer = Some(Restore::Restore);
            }
            if ui.button("Discard").clicked() {
                answer = Some(Restore::Discard);
            }
        });
    });
    answer
}

/// Asks whether to download the beat tracker's weights.
pub(crate) fn ask_to_download_weights(ui: &Ui, tracker: crate::Tracker) -> Option<DownloadWeights> {
    let mut answer = None;
    egui::Modal::new(egui::Id::new("download weights")).show(ui.ctx(), |ui| {
        ui.set_max_width(420.0);
        let question = "Download the beat tracker?";
        ui.label(style::dialog_title(ui, question));
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
                answer = Some(DownloadWeights::Download);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(DownloadWeights::Cancel);
            }
        });
    });
    answer
}

/// Offer a system package command on Linux, or explain how to choose a binary elsewhere.
pub(crate) fn ask_to_install_ffmpeg(
    ui: &Ui,
    platform: crate::FfmpegPlatform,
    command: Option<&str>,
) -> Option<FfmpegAnswer> {
    let mut answer = None;
    let modal = egui::Modal::new(egui::Id::new("install ffmpeg")).show(ui.ctx(), |ui| {
        ui.set_max_width(420.0);
        ui.label(style::dialog_title(ui, "Install ffmpeg?"));
        ui.add_space(4.0);
        match platform {
            crate::FfmpegPlatform::Windows => {
                ui.label(
                    "Download ffmpeg from gyan.dev (about 115 MB) into charter's data folder? \
                     Nothing about your songs is sent.",
                );
            }
            crate::FfmpegPlatform::MacOs => {
                ui.label(
                    "Download ffmpeg from ffmpeg.martin-riedl.de into charter's data folder? \
                     Nothing about your songs is sent.",
                );
            }
            crate::FfmpegPlatform::Linux => {
                ui.label("Install ffmpeg with your system's package manager.");
                if let Some(command) = command {
                    ui.label(RichText::new(command).monospace());
                }
            }
            crate::FfmpegPlatform::Other => {
                ui.label("Install ffmpeg and set its path in Settings > Sounds.");
            }
        }
        if platform != crate::FfmpegPlatform::Other {
            ui.label("If ffmpeg is elsewhere, set its path in Settings > Sounds.");
        }
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if matches!(
                platform,
                crate::FfmpegPlatform::Windows | crate::FfmpegPlatform::MacOs
            ) && ui.add(style::primary("Download")).clicked()
            {
                answer = Some(FfmpegAnswer::Download);
            }
            if platform == crate::FfmpegPlatform::Linux
                && command.is_some()
                && ui.add(style::primary("Copy command")).clicked()
            {
                answer = Some(FfmpegAnswer::CopyCommand);
            }
            if ui.button("Not now").clicked() {
                answer = Some(FfmpegAnswer::Later);
            }
        });
    });
    answer.or_else(|| modal.should_close().then_some(FfmpegAnswer::Later))
}

/// Asks whether to find the timing on the CPU when there is no graphics card.
pub(crate) fn ask_to_run_on_cpu(ui: &Ui) -> Option<RunOnCpu> {
    let mut answer = None;
    egui::Modal::new(egui::Id::new("run on cpu")).show(ui.ctx(), |ui| {
        ui.set_max_width(420.0);
        let question = "No graphics card found";
        ui.label(style::dialog_title(ui, question));
        ui.add_space(4.0);
        ui.label(
            "Mapperatorinator runs on the graphics card, and charter found none it can use. It can \
             run on the CPU instead, but much more slowly: a few minutes of song can take over a \
             quarter of an hour, during which the timing cannot be found again.",
        );
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.add(style::primary("Run on the CPU")).clicked() {
                answer = Some(RunOnCpu::Run);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(RunOnCpu::Cancel);
            }
        });
    });
    answer
}

/// Asks which of `names` a file is, for formats that share its extension.
/// `chosen` is the pick so far.
pub(crate) fn choose_format(
    ui: &Ui,
    verb: &str,
    names: &[String],
    chosen: &mut usize,
) -> Option<ChooseFormat> {
    let mut answer = None;
    egui::Modal::new(egui::Id::new("choose format")).show(ui.ctx(), |ui| {
        ui.set_max_width(420.0);
        let question = "Which game is the file for?";
        ui.label(style::dialog_title(ui, question));
        ui.add_space(4.0);
        ui.label("Games read the same kind of file their own ways, so the notes depend on which.");
        ui.add_space(8.0);
        for (index, name) in names.iter().enumerate() {
            ui.radio_value(chosen, index, name.as_str());
        }
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.add(style::primary(verb)).clicked() {
                answer = Some(ChooseFormat::Choose);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(ChooseFormat::Cancel);
            }
        });
    });
    answer
}

/// Asks whether to reset the open chart's timing.
pub(crate) fn ask_to_reset_timing(ui: &Ui) -> Option<ResetTiming> {
    let mut answer = None;
    egui::Modal::new(egui::Id::new("reset timing")).show(ui.ctx(), |ui| {
        ui.set_max_width(400.0);
        let question = "Reset the timing?";
        ui.label(style::dialog_title(ui, question));
        ui.add_space(4.0);
        ui.label(
            "Every BPM change, meter, stop and game timing event of the open chart's timing goes, \
             and it plays at 120 BPM in 4/4 from 0 ms. Notes keep their beats. Undo brings the \
             timing back.",
        );
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.add(style::primary("Reset")).clicked() {
                answer = Some(ResetTiming::Reset);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(ResetTiming::Cancel);
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
                        let text = ui.add(
                            egui::Label::new(RichText::new(message).color(style::WARN)).wrap(),
                        );
                        // An assertive live alert, so a screen reader reads the failure out when it
                        // appears: AccessKit announces live nodes, whatever their role. Both go on
                        // the text itself, which is what there is to announce, and name it: egui
                        // files a label's text under the value an alert does not carry.
                        ui.ctx().accesskit_node_builder(text.id, |node| {
                            node.set_role(egui::accesskit::Role::Alert);
                            node.set_live(egui::accesskit::Live::Assertive);
                            node.set_label(message.to_owned());
                        });
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
pub(crate) fn read_only(ui: &mut Ui, why: &charter_editor::ReadOnly) {
    notice(ui, style::icon::LOCK_SIMPLE, |ui| {
        ui.label(format!("Read only: {why}"));
    });
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use egui::accesskit::{Live, Role};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};

    use super::*;

    fn answer<T: Copy + std::fmt::Debug + PartialEq + 'static>(
        dialog: fn(&Ui) -> Option<T>,
        button: &str,
        expected: T,
    ) {
        let mut harness = Harness::builder().build_ui_state(
            move |ui, result: &mut Option<T>| *result = dialog(ui).or(*result),
            None,
        );
        harness.run();
        assert_eq!(harness.state(), &None);
        harness.get_by_label(button).click();
        harness.run();
        assert_eq!(harness.state(), &Some(expected), "{button}");
    }

    #[test]
    fn every_question_returns_the_choice_named_by_its_button() {
        answer(|ui| ask_to_restore(ui, "Demo"), "Restore", Restore::Restore);
        answer(|ui| ask_to_restore(ui, "Demo"), "Discard", Restore::Discard);
        answer(
            |ui| offer_tags(ui, Some("Title"), Some("Artist")),
            "Import",
            OfferTags::Import,
        );
        answer(
            |ui| offer_tags(ui, Some("Title"), Some("Artist")),
            "Keep mine",
            OfferTags::KeepMine,
        );
        answer(
            |ui| ask_to_download_weights(ui, crate::Tracker::BeatThis),
            "Download",
            DownloadWeights::Download,
        );
        answer(
            |ui| ask_to_download_weights(ui, crate::Tracker::BeatThis),
            "Cancel",
            DownloadWeights::Cancel,
        );
        answer(ask_to_run_on_cpu, "Run on the CPU", RunOnCpu::Run);
        answer(ask_to_run_on_cpu, "Cancel", RunOnCpu::Cancel);
        answer(ask_to_reset_timing, "Reset", ResetTiming::Reset);
        answer(ask_to_reset_timing, "Cancel", ResetTiming::Cancel);
        answer(|ui| ask_to_save(ui, "Demo"), "Save", SaveChanges::Save);
        answer(
            |ui| ask_to_save(ui, "Demo"),
            "Don't save",
            SaveChanges::DontSave,
        );
        answer(|ui| ask_to_save(ui, "Demo"), "Cancel", SaveChanges::Cancel);
    }

    #[test]
    fn choosing_a_format_reports_choose_or_cancel_and_keeps_the_selection() {
        for (button, expected) in [
            ("Import", ChooseFormat::Choose),
            ("Cancel", ChooseFormat::Cancel),
        ] {
            let mut harness = Harness::builder().build_ui_state(
                |ui, (chosen, answer): &mut (usize, Option<ChooseFormat>)| {
                    *answer = choose_format(
                        ui,
                        "Import",
                        &["First".to_owned(), "Second".to_owned()],
                        chosen,
                    )
                    .or(*answer);
                },
                (0, None),
            );
            harness.run();
            harness.get_by_label("Second").click();
            harness.run();
            harness.get_by_label(button).click();
            harness.run();
            assert_eq!(harness.state(), &(1, Some(expected)), "{button}");
        }
    }

    #[test]
    fn linux_ffmpeg_help_offers_a_command_to_copy() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, answer: &mut Option<FfmpegAnswer>| {
                *answer = ask_to_install_ffmpeg(
                    ui,
                    crate::FfmpegPlatform::Linux,
                    Some("sudo apt install ffmpeg"),
                )
                .or(*answer);
            },
            None,
        );
        harness.run();
        assert!(harness.query_by_label("sudo apt install ffmpeg").is_some());
        harness.get_by_label("Copy command").click();
        harness.run();
        assert_eq!(harness.state(), &Some(FfmpegAnswer::CopyCommand));
    }

    #[test]
    fn windows_and_mac_help_offer_their_respective_release_downloads() {
        for (platform, provider) in [
            (crate::FfmpegPlatform::Windows, "gyan.dev"),
            (crate::FfmpegPlatform::MacOs, "ffmpeg.martin-riedl.de"),
        ] {
            let mut harness = Harness::builder().build_ui_state(
                move |ui, answer: &mut Option<FfmpegAnswer>| {
                    *answer = ask_to_install_ffmpeg(ui, platform, None).or(*answer);
                },
                None,
            );
            harness.run();
            assert!(
                harness.query_by_label_contains(provider).is_some(),
                "{platform:?}"
            );
            harness.get_by_label("Download").click();
            harness.run();
            assert_eq!(harness.state(), &Some(FfmpegAnswer::Download));
        }
    }

    #[test]
    fn unsupported_platform_help_requires_manual_install_and_setting_the_path() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, answer: &mut Option<FfmpegAnswer>| {
                *answer = ask_to_install_ffmpeg(ui, crate::FfmpegPlatform::Other, None).or(*answer);
            },
            None,
        );
        harness.run();
        assert!(
            harness
                .query_by_label("Install ffmpeg and set its path in Settings > Sounds.")
                .is_some()
        );
        assert!(harness.query_by_label("Download").is_none());
    }

    #[test]
    fn a_failure_toast_is_an_alert_so_a_screen_reader_reads_it_out() {
        let message = "Could not save: the disk is full";
        let mut harness = Harness::builder().build_ui_state(
            |ui, dismissed: &mut bool| *dismissed |= toast(ui, message),
            false,
        );
        harness.run();
        // The node carrying the failure text is itself the alert, so there is one thing to read,
        // and it is live, since AccessKit announces live nodes whatever their role.
        let alert = harness.get_by_label(message);
        assert_eq!(alert.accesskit_node().role(), Role::Alert);
        assert_eq!(alert.accesskit_node().live(), Live::Assertive);
        harness.get_by_label("Dismiss").click();
        harness.run();
        assert!(harness.state());
    }
}
