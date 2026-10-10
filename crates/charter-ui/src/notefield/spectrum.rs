use super::{Canvas, Columns, Rangef, Rect, Spectrum, View, egui, style};

/// Spectrum frames each tile image holds a row of, at its level of detail.
pub(super) const TILE: usize = 1024;
/// The coarsest level of detail: each of its rows holds the loudest of 2^8 frames, 1.28 s.
pub(super) const COARSEST: u32 = 8;

/// A song's frequencies as images the GPU stretches over the strip, a band a column and a spectrum
/// frame a row, in tiles of [`TILE`] rows. Each level of detail holds, in each row, the loudest of
/// twice as many frames as the one before, so zoomed out a short sound still shows; the GPU's
/// own shrinking would skip it. Tiles are made the first time they show.
pub struct Tiles {
    /// Frequency frames shared by tiles at every level of detail.
    spectrum: std::sync::Arc<Spectrum>,
    /// Each level's tiles, by index, once made.
    pub(super) made: std::cell::RefCell<Vec<Vec<Option<egui::TextureHandle>>>>,
}

impl Tiles {
    /// Unrendered spectrum tiles, generated on demand as they come into view.
    #[must_use]
    pub const fn new(spectrum: std::sync::Arc<Spectrum>) -> Self {
        Self {
            spectrum,
            made: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// Tile `index` of `level`, made now unless it was before, or none past the song's end.
    pub(super) fn tile(
        &self,
        ctx: &egui::Context,
        level: u32,
        index: usize,
    ) -> Option<egui::TextureId> {
        let mut made = self.made.borrow_mut();
        let level_at = usize::try_from(level).ok()?;
        // Past the track there is no tile, and no room is made for one: a long stop puts the
        // view's time far past the end.
        let frames_a_tile = TILE.checked_mul(1_usize.checked_shl(level)?)?;
        if index >= self.spectrum.frames().len().div_ceil(frames_a_tile) {
            return None;
        }
        if made.len() <= level_at {
            made.resize_with(level_at.saturating_add(1), Vec::new);
        }
        let tiles = made.get_mut(level_at)?;
        if tiles.len() <= index {
            tiles.resize_with(index.saturating_add(1), || None);
        }
        let slot = tiles.get_mut(index)?;
        if slot.is_none() {
            let image = tile_image(self.spectrum.frames(), level, index)?;
            let name = format!("spectrum {level} {index}");
            *slot = Some(ctx.load_texture(name, image, egui::TextureOptions::LINEAR));
        }
        slot.as_ref().map(egui::TextureHandle::id)
    }
}

impl std::fmt::Debug for Tiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tiles")
            .field("frames", &self.spectrum.frames().len())
            .finish_non_exhaustive()
    }
}

/// The image of tile `index` at `level`: a row for each run of 2^`level` frames, holding each
/// band's loudest over the run, or none past the last frame.
pub(super) fn tile_image(
    frames: &[[u8; Spectrum::BANDS]],
    level: u32,
    index: usize,
) -> Option<egui::ColorImage> {
    let run = 1_usize.checked_shl(level)?;
    let first = index.checked_mul(TILE)?.checked_mul(run)?;
    let covered = frames.get(first..)?;
    if covered.is_empty() {
        return None;
    }
    let rows: Vec<[u8; Spectrum::BANDS]> = covered
        .chunks(run)
        .take(TILE)
        .map(|chunk| {
            let mut loudest = [0; Spectrum::BANDS];
            for frame in chunk {
                for (most, amplitude) in loudest.iter_mut().zip(frame) {
                    *most = (*most).max(*amplitude);
                }
            }
            loudest
        })
        .collect();
    let pixels = rows
        .iter()
        .flatten()
        .map(|amplitude| heat(*amplitude))
        .collect();
    Some(egui::ColorImage::new([Spectrum::BANDS, rows.len()], pixels))
}

/// The seconds tile `index` of `level` spans, `rows` rows tall: from the start of its first
/// frame, each frame centred on its time, to the end of its last.
pub(super) fn tile_span(level: u32, index: usize, rows: usize) -> (f64, f64) {
    let run = f64::from(1_u32 << level.min(COARSEST));
    let (first, rows) = (
        crate::cast::usize_f64(index) * crate::cast::usize_f64(TILE) * run,
        crate::cast::usize_f64(rows),
    );
    let from = (first - 0.5) / Spectrum::PER_SECOND;
    (from, from + rows * run / Spectrum::PER_SECOND)
}

/// The level of detail for `pixels_per_second` of screen: the finest whose rows are no closer
/// than a pixel apart.
pub(super) fn level_for(pixels_per_second: f32) -> u32 {
    let mut level = 0;
    while level < COARSEST
        && Spectrum::PER_SECOND / f64::from(1_u32 << level) > f64::from(pixels_per_second)
    {
        level = level.saturating_add(1);
    }
    level
}

/// The song's frequencies in the strip: the tiles the rows show, each band a column, lowest on
/// the left.
pub(super) fn frequencies(
    painter: &Canvas,
    ctx: &egui::Context,
    (columns, view, rows): (&Columns, View, Rangef),
    tiles: &Tiles,
) {
    let strip = columns.wave.shrink(2.0);
    let pixels = view.zoom.points_per_second() * ctx.pixels_per_point();
    let level = level_for(pixels);
    let (top, bottom) = (view.seconds_at(rows.min), view.seconds_at(rows.max));
    let (from, to) = (top.min(bottom), top.max(bottom));
    let seconds = crate::cast::usize_f64(TILE) * f64::from(1_u32 << level) / Spectrum::PER_SECOND;
    let (first, last) = (
        crate::cast::f64_usize((from / seconds).floor().max(0.0)),
        crate::cast::f64_usize((to / seconds).floor().max(0.0)),
    );
    for index in first..=last {
        let Some(texture) = tiles.tile(ctx, level, index) else {
            break;
        };
        let rows_held = ctx
            .tex_manager()
            .read()
            .meta(texture)
            .map_or(TILE, |meta| meta.size[1]);
        let (start, end) = tile_span(level, index, rows_held);
        let rect = Rect::from_x_y_ranges(strip, view.y_of(start)..=view.y_of(end));
        painter.image(texture, rect);
    }
}

/// The colour of a band `level` loud, from silent at 0 to the track's loudest at 255: nothing
/// until it is heard, fading in through the waveform's blue, and rising to the near-white of a
/// selection at the loudest.
pub(super) fn heat(level: u8) -> egui::Color32 {
    let t = f32::from(level) / 255.0;
    let (low, high) = (style::WAVE.gamma_multiply(0.35), style::SELECTED);
    let alpha = ((t - 0.06) / 0.19).clamp(0.0, 1.0);
    egui::Color32::from_rgba_unmultiplied(
        mix(low.r(), high.r(), t),
        mix(low.g(), high.g(), t),
        mix(low.b(), high.b(), t),
        mix(0, 255, alpha),
    )
}

/// A channel value between `low` and `high` at the clamped fraction `t`.
fn mix(low: u8, high: u8, t: f32) -> u8 {
    let value = f32::from(low) + (f32::from(high) - f32::from(low)) * t.clamp(0.0, 1.0);

    crate::cast::f32_u8(value.round())
}
