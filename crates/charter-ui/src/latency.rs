//! Latency calibration: a click track plays at the song's first tempo in the metronome's sound.
//! A flash shows when each click is heard, and the audio latency moves it until it lands on the
//! click: the cursor takes that off the playhead to show what is heard. Taps along to the clicks
//! then land later still by the input latency, which live placement takes off where a lane key
//! places its note.

use charter_audio::{Kit, Tone};
use egui::RichText;

use crate::settings::Settings;
use crate::style;
use crate::window::Request;

/// How many clicks play, and after how long a lead in seconds.
const CLICKS: usize = 32;
const LEAD: f64 = 1.0;
/// How many taps make a measurement.
pub const WANTED: usize = 16;
/// How long the flash stays lit after each click is heard, in seconds.
const FLASH: f64 = 0.08;
/// The tempos the clicks fold into by doubling or halving, slow enough to tap each one and
/// fast enough to keep the beat.
const SLOWEST: f64 = 60.0;
const FASTEST: f64 = 180.0;

/// The click track: seconds between clicks, and how many make a measure, its first accented.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Beat {
    spacing: f64,
    per_measure: u16,
}

impl Beat {
    /// Clicks at `bpm`, doubled or halved into a tappable range, `per_measure` to a measure.
    #[must_use]
    pub fn of(bpm: f64, per_measure: u16) -> Self {
        let mut bpm = if bpm.is_finite() && bpm > 0.0 {
            bpm
        } else {
            120.0
        };
        while bpm > FASTEST {
            bpm /= 2.0;
        }
        while bpm < SLOWEST {
            bpm *= 2.0;
        }
        Self {
            spacing: 60.0 / bpm,
            per_measure: per_measure.max(1),
        }
    }

    /// The tempo the clicks play at.
    #[must_use]
    pub fn bpm(self) -> f64 {
        60.0 / self.spacing
    }

    /// When each click plays, from the start.
    #[must_use]
    pub fn clicks(self) -> Vec<f64> {
        (0..CLICKS)
            .map(|at| {
                #[allow(clippy::cast_precision_loss)] // a few dozen clicks
                let at = at as f64;
                LEAD + at * self.spacing
            })
            .collect()
    }

    /// Each click with its sound in `kit`, the first of each measure accented.
    #[must_use]
    pub fn tones(self, kit: Kit) -> Vec<(f64, Tone)> {
        let measure = usize::from(self.per_measure);
        self.clicks()
            .into_iter()
            .enumerate()
            .map(|(at, click)| {
                let tone = if at.is_multiple_of(measure) {
                    Tone::Accent(kit)
                } else {
                    Tone::Beat(kit)
                };
                (click, tone)
            })
            .collect()
    }
}

impl Default for Beat {
    fn default() -> Self {
        Self::of(120.0, 4)
    }
}

/// How much later than the click nearest it the middle tap came, in milliseconds, once there are
/// enough taps near clicks `spacing` seconds apart: over half the taps within half a spacing of
/// one.
#[must_use]
pub fn latency(taps: &[f64], clicks: &[f64], spacing: f64) -> Option<f64> {
    let mut offsets: Vec<f64> = taps
        .iter()
        .filter_map(|tap| {
            let nearest = clicks
                .iter()
                .map(|click| tap - click)
                .min_by(|a, b| a.abs().total_cmp(&b.abs()))?;
            (nearest.abs() < spacing / 2.0).then_some(nearest * 1000.0)
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

/// Whether the flash is lit `elapsed` seconds from the start: just after a click of `clicks` is
/// heard, `audio_ms` after it plays.
#[must_use]
pub fn flashing(elapsed: f64, audio_ms: f64, clicks: &[f64]) -> bool {
    let heard = elapsed - audio_ms / 1000.0;
    clicks
        .iter()
        .any(|click| (0.0..FLASH).contains(&(heard - click)))
}

/// The calibration window's state.
#[derive(Debug, Default)]
pub struct Calibration {
    pub open: bool,
    /// When the clicks started, on the frame clock, and at what beat.
    started: Option<(f64, Beat)>,
    /// When each tap came, from the start.
    taps: Vec<f64>,
}

/// Draws the calibration window while it is open, at `now` on the frame clock, its clicks at
/// `beat` in the sound of `kit`. Asks for the clicks to play when started, and keeps what it
/// measures in `settings`.
#[allow(clippy::too_many_lines)] // one window, top to bottom
pub fn show(
    ctx: &egui::Context,
    calibration: &mut Calibration,
    (now, kit, beat): (f64, Kit, Beat),
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
            ui.set_width(360.0);
            let playing = calibration.started.map_or(beat, |(_, beat)| beat);
            ui.label(format!(
                "A click plays {CLICKS} times at the song's first tempo, {:.0} BPM, in the \
                 metronome's sound.",
                playing.bpm()
            ));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.add(style::primary("Start")).clicked() {
                    calibration.taps.clear();
                    calibration.started = Some((now, beat));
                    request = Some(Request::Preview {
                        clicks: beat.tones(kit),
                        volumes: settings.volumes,
                    });
                }
            });
            let clicks = playing.clicks();
            let elapsed = calibration.started.map(|(started, _)| now - started);
            if elapsed.is_some_and(|elapsed| elapsed < clicks.last().copied().unwrap_or(0.0) + 1.0)
            {
                ctx.request_repaint();
            }

            ui.add_space(10.0);
            ui.label(RichText::new("Audio").strong());
            ui.weak("Move the audio latency until the flash lands on each click you hear.");
            ui.horizontal(|ui| {
                let lit = elapsed
                    .is_some_and(|elapsed| flashing(elapsed, settings.audio_latency_ms, &clicks));
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                let colour = if lit { style::ACCENT } else { style::WAVE_BG };
                ui.painter().circle_filled(rect.center(), 8.0, colour);
                if ui.button("−").clicked() {
                    settings.audio_latency_ms -= 5.0;
                }
                ui.add(
                    egui::DragValue::new(&mut settings.audio_latency_ms)
                        .speed(1.0)
                        .range(-500.0..=500.0)
                        .suffix(" ms"),
                )
                .labelled_by(ui.label("Audio latency").id);
                if ui.button("+").clicked() {
                    settings.audio_latency_ms += 5.0;
                }
            });

            ui.add_space(10.0);
            ui.label(RichText::new("Input").strong());
            ui.weak("Tap Space, or the Tap button, on each click as you would to the music.");
            ui.horizontal(|ui| {
                let tap = ui.add_enabled(calibration.started.is_some(), egui::Button::new("Tap"));
                if calibration.started.is_some() && calibration.taps.len() < WANTED {
                    tap.request_focus();
                }
                if tap.clicked()
                    && let Some(elapsed) = elapsed
                {
                    calibration.taps.push(elapsed);
                }
                ui.weak(format!(
                    "{} of {WANTED} taps",
                    calibration.taps.len().min(WANTED)
                ));
            });
            // Taps come the audio latency after the click plays, and the input latency after
            // that.
            let measured = latency(&calibration.taps, &clicks, playing.spacing)
                .map(|ms| ms - settings.audio_latency_ms);
            match measured {
                Some(ms) => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("Taps land {ms:+.0} ms late")).strong());
                        if ui.button("Keep it").clicked() {
                            settings.input_latency_ms = ms;
                            calibration.started = None;
                            calibration.taps.clear();
                        }
                    });
                }
                None => {
                    ui.weak(format!(
                        "Input latency now: {:+.0} ms",
                        settings.input_latency_ms
                    ));
                }
            }
        });
    calibration.open = open;
    request
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPACING: f64 = 0.5;

    #[test]
    fn the_clicks_keep_the_songs_first_tempo_folded_into_a_tappable_range() {
        let beat = Beat::of(150.0, 4);
        let clicks = beat.clicks();
        assert_eq!(clicks.len(), CLICKS);
        let gap = clicks.get(1).unwrap() - clicks.first().unwrap();
        assert!((gap - 0.4).abs() < 1e-9);
        assert!((Beat::of(300.0, 4).spacing - 0.4).abs() < 1e-9);
        assert!((Beat::of(50.0, 4).spacing - 0.6).abs() < 1e-9);
        let accents: Vec<bool> = beat
            .tones(Kit::default())
            .iter()
            .take(5)
            .map(|(_, tone)| matches!(tone, Tone::Accent(_)))
            .collect();
        assert_eq!(accents, [true, false, false, false, true]);
    }

    #[test]
    fn the_flash_shows_when_each_click_is_heard() {
        let clicks = Beat::of(120.0, 4).clicks();
        let first = *clicks.first().unwrap();
        assert!(flashing(first + 0.01, 0.0, &clicks));
        assert!(!flashing(first + 0.01, 50.0, &clicks));
        assert!(flashing(first + 0.06, 50.0, &clicks));
        assert!(!flashing(first - 0.01, 0.0, &clicks));
    }

    #[test]
    fn taps_thirty_milliseconds_late_measure_thirty_give_or_take_two() {
        let clicks = Beat::of(120.0, 4).clicks();
        let taps: Vec<f64> = clicks
            .iter()
            .enumerate()
            .map(|(at, click)| {
                #[allow(clippy::cast_precision_loss)] // twenty taps
                let jitter = ((at * 7) % 9) as f64 - 4.0;
                click + 0.030 + jitter / 1000.0
            })
            .collect();
        let measured = latency(&taps, &clicks, SPACING).unwrap();
        assert!((measured - 30.0).abs() <= 2.0, "{measured}");
    }

    #[test]
    fn too_few_taps_near_clicks_measure_nothing() {
        let clicks = Beat::of(120.0, 4).clicks();
        assert_eq!(latency(&[1.0, 1.5, 2.0], &clicks, SPACING), None);
        let stray: Vec<f64> = clicks.iter().map(|click| click + 0.25).collect();
        assert_eq!(latency(&stray, &clicks, SPACING), None);
    }
}
