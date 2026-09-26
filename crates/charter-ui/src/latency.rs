//! Latency calibration: a click plays twenty times and the user taps along; the median of how
//! late the taps come is how much later than it plays a sound is heard and answered, which the
//! window then takes off the playhead so what it shows is what is heard.

use charter_audio::{Kit, Tone};
use egui::RichText;

use crate::settings::Settings;
use crate::style;
use crate::window::Request;

/// How many clicks play, how far apart in seconds, and after how long a lead.
const CLICKS: u32 = 20;
const SPACING: f64 = 0.5;
const LEAD: f64 = 1.0;
/// How many taps make a measurement.
pub const WANTED: usize = 16;

/// The calibration window's state.
#[derive(Debug, Default)]
pub struct Calibration {
    pub open: bool,
    /// When the clicks started, on the frame clock.
    started: Option<f64>,
    /// When each tap came, from the start.
    taps: Vec<f64>,
}

/// When each click plays, from the start.
#[must_use]
pub fn clicks() -> Vec<f64> {
    (0..CLICKS)
        .map(|at| LEAD + f64::from(at) * SPACING)
        .collect()
}

/// How much later than the click nearest it the middle tap came, in milliseconds, once there are
/// enough taps near clicks: over half the taps within a quarter of a second of one.
#[must_use]
pub fn latency(taps: &[f64], clicks: &[f64]) -> Option<f64> {
    let mut offsets: Vec<f64> = taps
        .iter()
        .filter_map(|tap| {
            let nearest = clicks
                .iter()
                .map(|click| tap - click)
                .min_by(|a, b| a.abs().total_cmp(&b.abs()))?;
            (nearest.abs() < SPACING / 2.0).then_some(nearest * 1000.0)
        })
        .collect();
    if offsets.len() * 2 < WANTED {
        return None;
    }
    offsets.sort_by(f64::total_cmp);
    let middle = offsets.len() / 2;
    if offsets.len().is_multiple_of(2) {
        Some((offsets.get(middle - 1)? + offsets.get(middle)?) / 2.0)
    } else {
        offsets.get(middle).copied()
    }
}

/// Draws the calibration window while it is open, at `now` on the frame clock. Asks for the
/// clicks to play when started, and keeps the measurement in `settings` when saved.
pub fn show(
    ctx: &egui::Context,
    calibration: &mut Calibration,
    (now, kit): (f64, Kit),
    settings: &mut Settings,
) -> Option<Request> {
    let mut request = None;
    let mut open = calibration.open;
    egui::Window::new("Calibrate latency")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(
                "A click plays twenty times. Tap Space, or the Tap button, on each one, as you \
                 would to the music. How late the taps come is taken off the playhead.",
            );
            ui.add_space(8.0);
            let measured = latency(&calibration.taps, &clicks());
            ui.horizontal(|ui| {
                if ui.add(style::primary("Start")).clicked() {
                    calibration.taps.clear();
                    calibration.started = Some(now);
                    let tone = Tone::Accent(kit);
                    request = Some(Request::Preview {
                        clicks: clicks().into_iter().map(|at| (at, tone)).collect(),
                        volumes: settings.volumes,
                    });
                }
                let tap = ui.add_enabled(calibration.started.is_some(), egui::Button::new("Tap"));
                if calibration.started.is_some() && calibration.taps.len() < WANTED {
                    tap.request_focus();
                }
                if tap.clicked()
                    && let Some(started) = calibration.started
                {
                    calibration.taps.push(now - started);
                }
                ui.weak(format!(
                    "{} of {WANTED} taps",
                    calibration.taps.len().min(WANTED)
                ));
            });
            ui.add_space(6.0);
            match measured {
                Some(ms) => {
                    ui.label(RichText::new(format!("Measured {ms:+.0} ms")).strong());
                    if ui.button("Keep it").clicked() {
                        settings.latency_ms = ms;
                        calibration.started = None;
                        calibration.taps.clear();
                    }
                }
                None => {
                    ui.weak(format!("Kept now: {:+.0} ms", settings.latency_ms));
                }
            }
        });
    calibration.open = open;
    request
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taps_thirty_milliseconds_late_measure_thirty_give_or_take_two() {
        let clicks = clicks();
        let taps: Vec<f64> = clicks
            .iter()
            .enumerate()
            .map(|(at, click)| {
                #[allow(clippy::cast_precision_loss)] // twenty taps
                let jitter = ((at * 7) % 9) as f64 - 4.0;
                click + 0.030 + jitter / 1000.0
            })
            .collect();
        let measured = latency(&taps, &clicks).unwrap();
        assert!((measured - 30.0).abs() <= 2.0, "{measured}");
    }

    #[test]
    fn too_few_taps_near_clicks_measure_nothing() {
        let clicks = clicks();
        assert_eq!(latency(&[1.0, 1.5, 2.0], &clicks), None);
        let stray: Vec<f64> = clicks.iter().map(|click| click + 0.25).collect();
        assert_eq!(latency(&stray, &clicks), None);
    }
}
