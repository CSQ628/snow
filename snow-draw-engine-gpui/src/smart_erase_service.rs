//! CPU-only, source-grid Smart Erase reconstruction. No display renderer or GPU
//! resource is needed to solve a job; completed cropped patches can be uploaded
//! once and composited at any camera, output scale, or item opacity.

use std::{collections::VecDeque, sync::Arc};

use image::RgbaImage;
use snow_draw_engine::{DisplayFilterType, FilterDisplayItem, Point};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::{CanvasCancellation, CanvasError, SourceImage};

const CACHE_BYTES: usize = 128 * 1024 * 1024;
const MAX_WORKING_PIXELS: u64 = 2 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct SmartSourceKey {
    pixels: u64,
    origin: Point<f64>,
    units: f64,
}

impl From<&SourceImage> for SmartSourceKey {
    fn from(source: &SourceImage) -> Self {
        Self {
            pixels: source.pixels_identity(),
            origin: source.origin(),
            units: source.units_per_pixel(),
        }
    }
}

/// An immutable source-grid result. Both images contain only the affected ROI
/// plus a sampling guard, never the reconstruction's donor halo.
pub struct SmartErasePatch {
    pub(crate) geometry: FilterDisplayItem,
    pub(crate) source: SmartSourceKey,
    pub(crate) original: SourceImage,
    pub(crate) filled: SourceImage,
    pub(crate) bytes: usize,
}

impl SmartErasePatch {
    /// Pristine donor-eligible pixels to source-over beneath the reconstruction.
    pub fn original(&self) -> &SourceImage {
        &self.original
    }

    pub fn reconstructed(&self) -> &SourceImage {
        &self.filled
    }

    pub fn retained_bytes(&self) -> usize {
        self.bytes
    }

    fn matches(&self, filter: &FilterDisplayItem, source: &SourceImage) -> bool {
        self.source == SmartSourceKey::from(source) && self.geometry == normalized(filter)
    }
}

/// Shared completed results keyed by immutable source identity/placement and
/// normalized mask geometry. Cloning a snapshot does not copy image bytes.
#[derive(Clone, Default)]
pub struct SmartEraseSnapshot {
    pub(crate) entries: VecDeque<Arc<SmartErasePatch>>,
}

impl SmartEraseSnapshot {
    pub fn result_count(&self) -> usize {
        self.entries.len()
    }

    pub fn retained_bytes(&self) -> usize {
        self.entries.iter().map(|entry| entry.bytes).sum()
    }

    /// Looking up an unfinished gesture cannot make it appear reconstructed.
    pub fn patch(
        &self,
        filter: &FilterDisplayItem,
        source: &SourceImage,
    ) -> Option<Arc<SmartErasePatch>> {
        if filter.filter.filter_type != DisplayFilterType::SmartErase
            || filter.filter.render_phase != 0
        {
            return None;
        }
        self.entries
            .iter()
            .find(|entry| entry.matches(filter, source))
            .cloned()
    }
}

/// Bounded reconstruction cache usable by display, export, and background jobs.
#[derive(Default)]
pub struct SmartEraseService {
    snapshot: SmartEraseSnapshot,
    frozen: bool,
}

impl SmartEraseService {
    pub fn snapshot(&self) -> SmartEraseSnapshot {
        self.snapshot.clone()
    }

    pub fn seed(&mut self, snapshot: &SmartEraseSnapshot) {
        for patch in &snapshot.entries {
            insert_patch(&mut self.snapshot.entries, patch.clone());
        }
    }

    /// Frozen services never reconstruct missing masks, including during export.
    pub fn restore_frozen(&mut self, snapshot: &SmartEraseSnapshot) {
        self.snapshot = snapshot.clone();
        self.frozen = true;
    }

    pub fn unfreeze(&mut self) {
        self.frozen = false;
    }

    pub fn clear(&mut self) {
        self.snapshot.entries.clear();
    }

    /// Returns no patch when the mask lies outside the source. All other failures
    /// are explicit; callers decide whether to retain a live warning mask or fail
    /// a strict export. Cancellation never publishes an incomplete result.
    pub fn solve(
        &mut self,
        filter: &FilterDisplayItem,
        source: &SourceImage,
        cancel: &CanvasCancellation,
    ) -> Result<Option<Arc<SmartErasePatch>>, CanvasError> {
        cancel.check()?;
        if filter.filter.filter_type != DisplayFilterType::SmartErase
            || filter.filter.render_phase != 0
        {
            return Err(CanvasError::Unsupported(
                "reconstructing an unfinished Smart Erase mask",
            ));
        }
        if let Some(index) = self
            .snapshot
            .entries
            .iter()
            .position(|p| p.matches(filter, source))
        {
            let patch = self.snapshot.entries.remove(index).expect("cached patch");
            self.snapshot.entries.push_back(patch.clone());
            return Ok(Some(patch));
        }
        if self.frozen {
            return Err(CanvasError::Unsupported(
                "Smart Erase geometry missing from the frozen result snapshot",
            ));
        }
        let patch = reconstruct_patch(filter, source, cancel)?;
        if let Some(patch) = &patch {
            cancel.check()?;
            insert_patch(&mut self.snapshot.entries, patch.clone());
        }
        Ok(patch)
    }
}

/// A Send-able job containing only immutable source and mask data. The revision
/// is returned unchanged so the owner can reject completion after invalidation.
pub struct SmartEraseJob {
    revision: u64,
    source: Option<SourceImage>,
    filters: Vec<FilterDisplayItem>,
    seed: SmartEraseSnapshot,
}

pub struct SmartEraseJobOutput {
    pub revision: u64,
    pub snapshot: SmartEraseSnapshot,
    pub warnings: Vec<String>,
}

impl SmartEraseJob {
    pub fn new(
        revision: u64,
        source: Option<SourceImage>,
        filters: Vec<FilterDisplayItem>,
        seed: SmartEraseSnapshot,
    ) -> Self {
        Self {
            revision,
            source,
            filters,
            seed,
        }
    }

    pub fn run(self, cancel: &CanvasCancellation) -> Result<SmartEraseJobOutput, CanvasError> {
        cancel.check()?;
        let mut service = SmartEraseService::default();
        service.seed(&self.seed);
        let mut warnings = Vec::new();
        if let Some(source) = &self.source {
            for filter in &self.filters {
                cancel.check()?;
                if filter.filter.filter_type != DisplayFilterType::SmartErase
                    || filter.filter.render_phase != 0
                {
                    continue;
                }
                match service.solve(filter, source, cancel) {
                    Ok(_) => {}
                    Err(CanvasError::Cancelled) => return Err(CanvasError::Cancelled),
                    Err(error) => warnings.push(error.to_string()),
                }
            }
        }
        cancel.check()?;
        Ok(SmartEraseJobOutput {
            revision: self.revision,
            snapshot: service.snapshot(),
            warnings,
        })
    }
}

pub(crate) fn normalized(filter: &FilterDisplayItem) -> FilterDisplayItem {
    let mut geometry = filter.clone();
    geometry.opacity = 1.0;
    geometry.filter.render_phase = 0;
    geometry
}

pub(crate) fn insert_patch(
    entries: &mut VecDeque<Arc<SmartErasePatch>>,
    patch: Arc<SmartErasePatch>,
) {
    if patch.bytes > CACHE_BYTES {
        return;
    }
    if let Some(index) = entries
        .iter()
        .position(|p| p.source == patch.source && p.geometry == patch.geometry)
    {
        entries.remove(index);
    }
    let mut bytes = entries.iter().map(|p| p.bytes).sum::<usize>();
    while bytes + patch.bytes > CACHE_BYTES {
        let Some(old) = entries.pop_front() else {
            break;
        };
        bytes -= old.bytes;
    }
    entries.push_back(patch);
}

pub(crate) fn reconstruct_patch(
    filter: &FilterDisplayItem,
    source: &SourceImage,
    cancel: &CanvasCancellation,
) -> Result<Option<Arc<SmartErasePatch>>, CanvasError> {
    cancel.check()?;
    let geometry = normalized(filter);
    let units = source.units_per_pixel();
    let (left, top, right, bottom) = mask_bounds(&geometry)?;
    let x0 = (((left - source.origin().x) / units).floor() - 512.0)
        .clamp(0.0, f64::from(source.width())) as u32;
    let y0 = (((top - source.origin().y) / units).floor() - 512.0)
        .clamp(0.0, f64::from(source.height())) as u32;
    let x1 = (((right - source.origin().x) / units).ceil() + 512.0)
        .clamp(0.0, f64::from(source.width())) as u32;
    let y1 = (((bottom - source.origin().y) / units).ceil() + 512.0)
        .clamp(0.0, f64::from(source.height())) as u32;
    if x1 <= x0 || y1 <= y0 {
        return Ok(None);
    }
    let width = x1 - x0;
    let height = y1 - y0;
    if u64::from(width) * u64::from(height) > MAX_WORKING_PIXELS {
        return Err(CanvasError::ResourceLimit);
    }
    let origin = Point::new(
        source.origin().x + f64::from(x0) * units,
        source.origin().y + f64::from(y0) * units,
    );
    let mask = reconstruction_mask(&geometry, origin, units, width, height)?;
    let mut affected = [width, height, 0, 0];
    for y in 0..height {
        cancel.check()?;
        for x in 0..width {
            if mask[y as usize * width as usize + x as usize] != 0 {
                affected[0] = affected[0].min(x);
                affected[1] = affected[1].min(y);
                affected[2] = affected[2].max(x + 1);
                affected[3] = affected[3].max(y + 1);
            }
        }
    }
    if affected[2] == 0 || affected[3] == 0 {
        return Ok(None);
    }
    cancel.check()?;
    let original = source.eligible_region([x0, y0, width, height], cancel)?;
    let output = crate::smart_erase::reconstruct_with_donors(&original, &mask, None, cancel)?;
    cancel.check()?;
    let left = affected[0].saturating_sub(2);
    let top = affected[1].saturating_sub(2);
    let right = affected[2].saturating_add(2).min(width);
    let bottom = affected[3].saturating_add(2).min(height);
    let retained_origin = Point::new(
        origin.x + f64::from(left) * units,
        origin.y + f64::from(top) * units,
    );
    let crop = |image: &RgbaImage| -> Result<SourceImage, CanvasError> {
        SourceImage::new(
            image::imageops::crop_imm(image, left, top, right - left, bottom - top).to_image(),
        )?
        .with_placement(retained_origin, units)
    };
    let original = crop(&original)?;
    let filled = crop(&output)?;
    cancel.check()?;
    let bytes = original.pixels.as_raw().len() + filled.pixels.as_raw().len();
    Ok(Some(Arc::new(SmartErasePatch {
        geometry,
        source: SmartSourceKey::from(source),
        original,
        filled,
        bytes,
    })))
}

fn mask_bounds(filter: &FilterDisplayItem) -> Result<(f64, f64, f64, f64), CanvasError> {
    let bounds = if filter.is_pen_filter {
        if filter.points.is_empty()
            || !filter.stroke_width.is_finite()
            || filter.stroke_width <= 0.0
        {
            return Err(snow_draw_engine::ErrorCode::InvalidArgument.into());
        }
        let radius = filter.stroke_width / 2.0 + 2.0;
        let mut bounds = (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        for [x, y] in filter.points.iter().copied() {
            if !x.is_finite() || !y.is_finite() {
                return Err(snow_draw_engine::ErrorCode::InvalidArgument.into());
            }
            bounds.0 = bounds.0.min(x - radius);
            bounds.1 = bounds.1.min(y - radius);
            bounds.2 = bounds.2.max(x + radius);
            bounds.3 = bounds.3.max(y + radius);
        }
        bounds
    } else {
        if filter.width <= 0.0 || filter.height <= 0.0 {
            return Err(snow_draw_engine::ErrorCode::InvalidArgument.into());
        }
        let (sin, cos) = filter.rotation.sin_cos();
        let width = cos.abs() * filter.width / 2.0 + sin.abs() * filter.height / 2.0 + 2.0;
        let height = sin.abs() * filter.width / 2.0 + cos.abs() * filter.height / 2.0 + 2.0;
        (
            filter.center_x - width,
            filter.center_y - height,
            filter.center_x + width,
            filter.center_y + height,
        )
    };
    if [bounds.0, bounds.1, bounds.2, bounds.3]
        .into_iter()
        .any(|value| !value.is_finite())
    {
        return Err(snow_draw_engine::ErrorCode::InvalidArgument.into());
    }
    Ok(bounds)
}

fn reconstruction_mask(
    filter: &FilterDisplayItem,
    origin: Point<f64>,
    units: f64,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, CanvasError> {
    let point = |x: f64, y: f64| {
        (
            ((x - origin.x) / units) as f32,
            ((y - origin.y) / units) as f32,
        )
    };
    let mut path = PathBuilder::new();
    let mut stroke = None;
    if filter.is_pen_filter {
        let (x, y) = point(filter.points[0][0], filter.points[0][1]);
        if filter.points.len() == 1 {
            let radius = (filter.stroke_width / (2.0 * units)) as f32;
            path.push_circle(x, y, radius);
        } else {
            path.move_to(x, y);
            for [x, y] in filter.points.iter().copied().skip(1) {
                let (x, y) = point(x, y);
                path.line_to(x, y);
            }
            stroke = Some(Stroke {
                width: (filter.stroke_width / units) as f32,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Default::default()
            });
        }
    } else {
        let (sin, cos) = filter.rotation.sin_cos();
        for (index, [x, y]) in [[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]]
            .into_iter()
            .enumerate()
        {
            let dx = x * filter.width;
            let dy = y * filter.height;
            let (x, y) = point(
                filter.center_x + dx * cos - dy * sin,
                filter.center_y + dx * sin + dy * cos,
            );
            if index == 0 {
                path.move_to(x, y);
            } else {
                path.line_to(x, y);
            }
        }
        path.close();
    }
    let path = path
        .finish()
        .ok_or(snow_draw_engine::ErrorCode::InvalidArgument)?;
    let mut mask = Pixmap::new(width, height).ok_or(CanvasError::ResourceLimit)?;
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    if let Some(stroke) = stroke {
        mask.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    } else {
        mask.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    Ok(mask.pixels().iter().map(|pixel| pixel.alpha()).collect())
}

#[cfg(test)]
#[path = "smart_erase/service_tests.rs"]
mod tests;
