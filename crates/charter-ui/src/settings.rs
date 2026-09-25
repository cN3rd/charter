//! What the user chooses about the editor, and the window where they choose it.

use std::num::NonZeroU16;

use serde::{Deserialize, Serialize};

use charter_editor::SNAPS;

use crate::panels::ordinal;
use crate::style::table;

/// The user's choices, which the host keeps between runs. Choices missing from what the host
/// kept, such as ones newer than it, take their defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Whether the cursor moves on a snap once the lane keys that placed a note are up.
    pub auto_jump: bool,
    /// The snap a song opens on, where its resolution holds it.
    pub snap: NonZeroU16,
    /// How close the notefield opens, as a percentage of its usual zoom.
    pub zoom: f32,
    /// How large the whole window draws, as a factor.
    pub scale: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_jump: true,
            snap: NonZeroU16::new(16).unwrap_or(NonZeroU16::MIN),
            zoom: 100.0,
            scale: 1.0,
        }
    }
}

/// The interface sizes on offer, as factors.
const SCALES: [f32; 6] = [0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

impl Settings {
    /// The interface size, kept to what the window can draw.
    pub(crate) fn scale(&self) -> f32 {
        if self.scale.is_finite() {
            self.scale.clamp(0.5, 3.0)
        } else {
            1.0
        }
    }
}

/// The settings window, while `open` is set. Choices take effect as they are made.
pub fn show(ctx: &egui::Context, settings: &mut Settings, open: &mut bool) {
    egui::Window::new("Settings")
        .open(open)
        .resizable(false)
        .collapsible(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            table(ui, "settings", |ui| {
                ui.weak("Note entry");
                ui.checkbox(
                    &mut settings.auto_jump,
                    "Move on a snap after placing a note",
                );
                ui.end_row();
                ui.weak("Songs open on");
                egui::ComboBox::from_id_salt("snap")
                    .selected_text(format!("{} snap", ordinal(settings.snap)))
                    .show_ui(ui, |ui| {
                        for snap in SNAPS.into_iter().filter_map(NonZeroU16::new) {
                            let text = format!("{} snap", ordinal(snap));
                            ui.selectable_value(&mut settings.snap, snap, text);
                        }
                    });
                ui.end_row();
                ui.weak("Songs open at");
                let zoom = egui::Slider::new(&mut settings.zoom, 25.0..=400.0).suffix("% zoom");
                ui.add(zoom);
                ui.end_row();
                ui.weak("Interface size");
                egui::ComboBox::from_id_salt("scale")
                    .selected_text(percent(settings.scale))
                    .show_ui(ui, |ui| {
                        for scale in SCALES {
                            ui.selectable_value(&mut settings.scale, scale, percent(scale));
                        }
                    });
                ui.end_row();
            });
        });
}

fn percent(factor: f32) -> String {
    format!("{:.0}%", factor * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_open_on_sixteenths_at_the_usual_zoom_moving_on_after_notes() {
        let settings = Settings::default();
        assert!(settings.auto_jump);
        assert_eq!(settings.snap.get(), 16);
        assert!((settings.zoom - 100.0).abs() < f32::EPSILON);
        assert!((settings.scale - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn stored_settings_come_back_and_missing_ones_take_their_defaults() {
        let settings = Settings {
            auto_jump: false,
            snap: NonZeroU16::new(24).unwrap(),
            zoom: 150.0,
            scale: 1.25,
        };
        let stored = serde_json::to_string(&settings).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&stored).unwrap(), settings);
        let older: Settings = serde_json::from_str(r#"{ "auto_jump": false }"#).unwrap();
        assert_eq!(
            older,
            Settings {
                auto_jump: false,
                ..Settings::default()
            }
        );
    }
}
