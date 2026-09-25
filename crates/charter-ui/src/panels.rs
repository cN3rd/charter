//! The panels around the notefield: the menu bar, the song panel, the inspector and the status
//! bar, and what the window shows with no song open.

use std::num::NonZeroU16;

use charter_core::{MarkerKind, ResolvedTime};
use egui::{self, Align, Layout, RichText, Ui};
use serde_json::Value;

use charter_editor::{Song, Step};

use crate::Action;
use crate::notefield::Zoom;
use crate::style::{fields, heading, table, title};

pub fn menu_bar(ui: &mut Ui, song_open: bool, actions: &mut Vec<Action>) {
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            if ui.button("Open the demo song").clicked() {
                actions.push(Action::OpenDemo);
            }
        });
        ui.menu_button("View", |ui| {
            let items = [
                ("Zoom in", Action::ZoomBy(Zoom::STEP)),
                ("Zoom out", Action::ZoomBy(Zoom::STEP.recip())),
                ("Finer snap", Action::FinerSnap),
                ("Coarser snap", Action::CoarserSnap),
            ];
            for (label, action) in items {
                if ui
                    .add_enabled(song_open, egui::Button::new(label))
                    .clicked()
                {
                    actions.push(action);
                }
            }
        });
        ui.menu_button("Help", |ui| {
            ui.label(title("Keys"));
            table(ui, "keys", |ui| {
                for (keys, does) in KEYS {
                    ui.monospace(keys);
                    ui.label(does);
                    ui.end_row();
                }
            });
        });
    });
}

/// The keys the notefield answers to, and what each does.
const KEYS: [(&str, &str); 6] = [
    ("Up, Down", "Move the cursor by the snap"),
    ("Page Up, Page Down", "Move the cursor by a measure"),
    ("Left, Right", "Coarser or finer snap"),
    ("Wheel", "Move the cursor by the snap"),
    ("Ctrl+Wheel", "Zoom"),
    ("Ctrl+Plus, Ctrl+Minus", "Zoom"),
];

pub fn song_panel(ui: &mut Ui, song: &Song, actions: &mut Vec<Action>) {
    let document = song.document().document();
    let metadata = &document.metadata;
    heading(ui, "Song");
    let preview = metadata.preview.map_or_else(
        || "none".to_owned(),
        |preview| match preview.length_seconds {
            Some(length) => format!("{} s for {length} s", preview.start_seconds),
            None => format!("{} s", preview.start_seconds),
        },
    );
    fields(
        ui,
        "song",
        &[
            ("Title", metadata.title.clone()),
            ("Artist", metadata.artist.clone()),
            ("Audio", metadata.audio.to_string()),
            ("Preview", preview),
            (
                "Ruleset",
                format!("{} {}", document.ruleset.id, document.ruleset.version),
            ),
        ],
        "",
    );
    heading(ui, "Charts");
    if document.charts.is_empty() {
        ui.weak("No charts yet");
    }
    table(ui, "charts", |ui| {
        for (index, chart) in document.charts.iter().enumerate() {
            let open = index == song.chart_index();
            if ui.selectable_label(open, &chart.name).clicked() {
                actions.push(Action::OpenChart(index));
            }
            ui.weak(
                chart
                    .lanes
                    .map_or_else(|| "no lanes".to_owned(), |lanes| format!("{lanes} lanes")),
            );
            ui.end_row();
        }
    });
    if let Some(chart) = song.chart() {
        heading(ui, "Chart settings");
        let settings: Vec<_> = chart
            .settings
            .iter()
            .map(|(key, value)| (key.as_str(), shown(value)))
            .collect();
        fields(ui, "settings", &settings, "No chart settings");
    }
    heading(ui, "Samples");
    let samples: Vec<_> = document
        .samples
        .iter()
        .map(|(id, path)| (id.as_str(), path.to_string()))
        .collect();
    fields(ui, "samples", &samples, "No samples");
    heading(ui, "Markers");
    let markers: Vec<_> = document
        .markers
        .iter()
        .map(|marker| {
            let kind = match &marker.kind {
                MarkerKind::Section => "Section".to_owned(),
                MarkerKind::Bookmark => "Bookmark".to_owned(),
                MarkerKind::Other(key) => key.to_string(),
            };
            (kind, marker.text.clone())
        })
        .collect();
    let markers: Vec<_> = markers
        .iter()
        .map(|(kind, text)| (kind.as_str(), text.clone()))
        .collect();
    fields(ui, "markers", &markers, "No markers");
}

/// A setting's value as text: strings bare, anything else as JSON.
fn shown(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

pub fn inspector(ui: &mut Ui) {
    heading(ui, "Inspector");
    ui.weak("Nothing selected");
}

pub fn status_bar(ui: &mut Ui, song: Option<&Song>, zoom: Zoom) {
    ui.horizontal(|ui| {
        let Some(song) = song else {
            ui.weak("No song open");
            return;
        };
        let timing = song.timing();
        ui.monospace(timing.grid_position(song.cursor()).to_string());
        ui.monospace(clock(timing.time_at(song.cursor())));
        ui.separator();
        ui.label(format!("Snap {}", ordinal(song.snap())));
        ui.label(format!("Zoom {:.0}%", zoom.percent()));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.weak("Read only");
        });
    });
}

/// An audio time as minutes, seconds and milliseconds, such as `01:23.456`.
fn clock(time: ResolvedTime) -> String {
    let seconds = time.seconds();
    let sign = if seconds < 0.0 { "-" } else { "" };
    let millis = (seconds.abs() * 1000.0).round();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // whole, positive, small
    let millis = millis as u64;
    format!(
        "{sign}{:02}:{:02}.{:03}",
        millis / 60_000,
        millis / 1000 % 60,
        millis % 1000
    )
}

/// A snap's name, such as `16th` or `32nd`.
pub fn ordinal(n: NonZeroU16) -> String {
    let n = n.get();
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// What the centre shows with no song open.
pub fn empty(ui: &mut Ui, actions: &mut Vec<Action>) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.heading("No song open");
        ui.label(
            "Drop a charter song (.json) on this window, or give its path when you start charter.",
        );
        ui.add_space(8.0);
        if ui.button("Open the demo song").clicked() {
            actions.push(Action::OpenDemo);
        }
    });
}

/// Why the last file did not open.
pub fn failure(ui: &mut Ui, message: &str) {
    let colour = ui.visuals().warn_fg_color;
    ui.label(RichText::new(message).color(colour));
}

/// The step a key asks for, if any.
pub fn key_actions(input: &egui::InputState) -> Vec<Action> {
    let mut actions = Vec::new();
    let pressed = |key| input.key_pressed(key);
    if input.modifiers.command {
        if pressed(egui::Key::Plus) || pressed(egui::Key::Equals) {
            actions.push(Action::ZoomBy(Zoom::STEP));
        }
        if pressed(egui::Key::Minus) {
            actions.push(Action::ZoomBy(Zoom::STEP.recip()));
        }
        return actions;
    }
    let keys = [
        (egui::Key::ArrowDown, Action::Move(Step::Later)),
        (egui::Key::ArrowUp, Action::Move(Step::Earlier)),
        (egui::Key::PageDown, Action::MoveMeasure(Step::Later)),
        (egui::Key::PageUp, Action::MoveMeasure(Step::Earlier)),
        (egui::Key::ArrowRight, Action::FinerSnap),
        (egui::Key::ArrowLeft, Action::CoarserSnap),
    ];
    for (key, action) in keys {
        for _ in 0..input.num_presses(key) {
            actions.push(action);
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(n: u16) -> NonZeroU16 {
        NonZeroU16::new(n).unwrap()
    }

    #[test]
    fn snaps_read_as_ordinals() {
        let names: Vec<_> = [4, 12, 16, 32, 48, 192].map(|n| ordinal(note(n))).into();
        assert_eq!(names, ["4th", "12th", "16th", "32nd", "48th", "192nd"]);
    }
}
