use crate::{
    CanvasDocument, CanvasError, CanvasSession, RetainedScene, SourceImage, engine as draw,
};

/// Immutable, Send-able document export snapshot with enough lower-layer content
/// for the ordered effect pipeline. Transient selection overlays are never exported.
#[derive(Clone)]
pub struct CanvasSnapshot {
    pub(crate) scene: RetainedScene,
    pub(crate) background: Option<SourceImage>,
    pub(crate) resources: u64,
    pub(crate) viewport: draw::FrameView,
    pub(crate) presentation: crate::CanvasPresentation,
}
impl CanvasSnapshot {
    pub(crate) fn from_reference(
        document: &CanvasDocument,
        reference: &CanvasSession,
        viewport: draw::FrameView,
    ) -> Self {
        Self {
            scene: reference.scene().clone(),
            background: document.background().cloned(),
            resources: document.resource_revision(),
            viewport,
            presentation: Default::default(),
        }
    }
    pub fn viewport(&self) -> draw::FrameView {
        self.viewport
    }
    pub fn with_presentation(
        mut self,
        presentation: crate::CanvasPresentation,
    ) -> Result<Self, CanvasError> {
        presentation.validate()?;
        self.presentation = presentation;
        Ok(self)
    }
}

/// Sampling supports accumulate between ordered passes; siblings share one input.
pub(crate) fn sampling_support(scene: &RetainedScene) -> f64 {
    let mut total = 0.0;
    let mut pass = None;
    let mut largest = 0.0_f64;
    for run in scene.runs() {
        if pass != Some(run.source_pass) {
            total += largest;
            largest = 0.0;
            pass = Some(run.source_pass);
        }
        if let Some(draw::SceneDisplayItem::Filter(filter)) = scene.items().get(run.start as usize)
        {
            largest = largest.max(filter.filter.sampling_radius);
        }
    }
    total + largest
}

pub(crate) fn prepare_reference(
    document: &mut CanvasDocument,
    reference: &mut CanvasSession,
    frame: draw::FrameView,
) -> Result<(), CanvasError> {
    if frame.surface.width == 0 || frame.surface.height == 0 {
        return Ok(());
    }
    reference.refresh(document)?;
    let base = draw::canvas_viewport(frame.camera, frame.surface);
    let native_grid = document
        .background()
        .map_or(1.0, SourceImage::units_per_pixel);
    let mut padding =
        (128.0 * native_grid).max(sampling_support(reference.scene()) + 2.0 * native_grid);
    // Expanding the query may reveal an additional downstream pass. Iterate until
    // every visible dependency has its full support; never silently truncate it.
    for _ in 0..16 {
        let (mut left, mut top, mut right, mut bottom) = base;
        if let Some(source) = document.background() {
            let origin = source.origin();
            let candidate = (
                left.min(origin.x),
                top.min(origin.y),
                right.max(origin.x + f64::from(source.pixels().width()) * native_grid),
                bottom.max(origin.y + f64::from(source.pixels().height()) * native_grid),
            );
            if ((candidate.2 - candidate.0 + padding * 2.0) / native_grid)
                * ((candidate.3 - candidate.1 + padding * 2.0) / native_grid)
                <= crate::document::MAX_SURFACE_PIXELS as f64
            {
                (left, top, right, bottom) = candidate;
            }
        }
        let grid = native_grid
            .max((right - left + padding * 2.0) / 3500.0)
            .max((bottom - top + padding * 2.0) / 3500.0);
        let tile = 256.0 * grid;
        let pad = padding.max(128.0 * grid);
        left = ((left - pad) / tile).floor() * tile;
        top = ((top - pad) / tile).floor() * tile;
        right = ((right + pad) / tile).ceil() * tile;
        bottom = ((bottom + pad) / tile).ceil() * tile;
        reference.set_surface(
            document,
            ((right - left) / grid).round().max(1.0) as u32,
            ((bottom - top) / grid).round().max(1.0) as u32,
        )?;
        reference.set_camera(
            document,
            draw::Camera {
                center: draw::Point::new((left + right) / 2.0, (top + bottom) / 2.0),
                zoom: 1.0 / grid,
            },
        )?;
        let required = sampling_support(reference.scene()) + 2.0 * grid;
        if required <= pad {
            return Ok(());
        }
        padding = required;
    }
    Err(CanvasError::ResourceLimit)
}

pub(crate) fn prepare_fixed_reference(
    document: &mut CanvasDocument,
    session: &mut CanvasSession,
    reference: crate::CanvasRenderReference,
) -> Result<(), CanvasError> {
    reference.validate()?;
    let rect = reference.canvas_rect;
    let density = reference.pixels_per_canvas_unit;
    let mut padding = 2.0 / density;
    for _ in 0..16 {
        let width = ((rect.width + padding * 2.0) * density).ceil() as u32;
        let height = ((rect.height + padding * 2.0) * density).ceil() as u32;
        session.set_surface(document, width, height)?;
        session.set_camera(
            document,
            draw::Camera {
                zoom: density,
                center: draw::Point::new(
                    rect.x - padding + f64::from(width) / (2.0 * density),
                    rect.y - padding + f64::from(height) / (2.0 * density),
                ),
            },
        )?;
        let needed = sampling_support(session.scene()) + 2.0 / density;
        if needed <= padding {
            return Ok(());
        }
        padding = needed;
    }
    Err(CanvasError::ResourceLimit)
}
