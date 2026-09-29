//! charter's editor window: the notefield, the panels around it, and the state that ties them to
//! an open song. It draws into any egui `Ui` and never touches a file: it asks its host to read,
//! write and choose files through [`Request`]s, so the binary that hosts it opens the window and
//! does that I/O.

mod artwork;
/// Casts at the boundary between screen coordinates, samples and integer indices.
mod cast {
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "pixel and sample values need floating-point casts"
    )]
    /// A sample or pixel count as a floating-point value for UI calculations.
    pub(crate) fn usize_f64(value: usize) -> f64 {
        value as f64
    }

    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "UI pixel values use f32"
    )]
    /// A calculated coordinate narrowed to the precision egui draws with.
    pub(crate) fn f64_f32(value: f64) -> f32 {
        value as f32
    }

    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "flooring a screen coordinate yields a lane"
    )]
    /// A floored screen coordinate narrowed to a lane index.
    pub(crate) fn f32_u16(value: f32) -> u16 {
        value as u16
    }

    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "saturating float-to-index cast for display tiles"
    )]
    /// A floating-point display coordinate saturated to a tile index.
    pub(crate) fn f64_usize(value: f64) -> usize {
        value as usize
    }

    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "rounded color channel is in u8 range"
    )]
    /// A rounded colour channel narrowed to its byte value.
    pub(crate) fn f32_u8(value: f32) -> u8 {
        value as u8
    }

    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "UI scroll notches fit in i32"
    )]
    /// A scroll distance narrowed to a count of notches.
    pub(crate) fn f32_i32(value: f32) -> i32 {
        value as i32
    }

    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "bounded audio pitch in cents fits in i16"
    )]
    /// An audio pitch offset in cents narrowed to the track's pitch type.
    pub(crate) fn f64_i16(value: f64) -> i16 {
        value as i16
    }
}
mod flip;
mod import;
mod keys;
mod latency;
mod minimap;
mod new_song;
mod notefield;
mod panels;
mod recovery;
mod settings;
// Every window shares one look; the name keeps `crate::style` paths as they were.
use charter_style as style;
mod timing_panel;
mod window;

pub use artwork::Picture;
pub use flip::Direction;
pub use settings::{Settings, Sound};
use window::{Action, Note};
pub use window::{Request, Saved, Window};
