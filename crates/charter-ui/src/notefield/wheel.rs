use super::egui;

/// Cursor steps the mouse wheel asks for in `events`, later first, keeping part-steps in
/// `remainder`: a whole line of scrolling, one wheel notch, is one step, and touchpads and smooth
/// wheels send fractions that add up.
pub fn wheel_steps(events: &[egui::Event], remainder: &mut f32) -> i32 {
    let mut steps = 0.0;
    for event in events {
        let egui::Event::MouseWheel {
            unit,
            delta,
            modifiers,
            ..
        } = event
        else {
            continue;
        };
        // Ctrl zooms, and Shift moves by a measure.
        if modifiers.command || modifiers.ctrl || modifiers.shift {
            continue;
        }
        let points = match unit {
            egui::MouseWheelUnit::Point => -delta.y,
            egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => -delta.y * POINTS_PER_STEP,
        };
        *remainder += points;
        let whole = (*remainder / POINTS_PER_STEP).trunc();
        *remainder -= whole * POINTS_PER_STEP;
        steps += whole;
    }

    crate::cast::f32_i32(steps)
}

/// How many measures the wheel turned under Shift this frame, later positive: a notch a measure,
/// with what a touchpad turned short of a whole one kept in `remainder` for the next frame.
/// Shift turns some wheels sideways, so either way counts.
pub(super) fn measure_steps(events: &[egui::Event], remainder: &mut f32) -> i32 {
    let notches: f32 = events
        .iter()
        .filter_map(|event| match event {
            egui::Event::MouseWheel {
                unit,
                delta,
                modifiers,
                ..
            } if modifiers.shift && !modifiers.command && !modifiers.ctrl => {
                let along = if delta.y == 0.0 { delta.x } else { delta.y };
                Some(match unit {
                    egui::MouseWheelUnit::Point => -along / POINTS_PER_STEP,
                    egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => -along,
                })
            }
            egui::Event::Copy
            | egui::Event::Cut
            | egui::Event::Paste(_)
            | egui::Event::Text(_)
            | egui::Event::Key { .. }
            | egui::Event::ModifiersChanged(_)
            | egui::Event::PointerMoved(_)
            | egui::Event::MouseMoved(_)
            | egui::Event::PointerButton { .. }
            | egui::Event::PointerGone
            | egui::Event::Zoom(_)
            | egui::Event::Rotate(_)
            | egui::Event::Ime(_)
            | egui::Event::Touch { .. }
            | egui::Event::MouseWheel { .. }
            | egui::Event::WindowFocused(_)
            | egui::Event::AccessKitActionRequest(_)
            | egui::Event::Screenshot { .. } => None,
        })
        .sum();
    *remainder += notches;
    let whole = remainder.trunc();
    *remainder -= whole;

    crate::cast::f32_i32(whole)
}

/// Points of touchpad scrolling that make one cursor step.
const POINTS_PER_STEP: f32 = 40.0;
