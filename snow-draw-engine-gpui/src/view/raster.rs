use crate::{CanvasError, RetainedScene, engine as draw};

/// A device-pixel frame covering the logical viewport plus ordered filter support.
/// Keeping the rounded extent in the frame also makes presentation one-to-one at
/// fractional DPI: the final pixel must not be stretched over a logical extent.
pub(super) fn frame(
    scene: &RetainedScene,
    viewport: draw::FrameView,
    scale: f32,
) -> Result<draw::FrameView, CanvasError> {
    let [width, height] = crate::export::scaled_dimensions(viewport.surface, scale)?;
    let zoom = viewport.camera.zoom * f64::from(scale);
    let support = (crate::snapshot::sampling_support(scene) * zoom).ceil();
    if !support.is_finite() || support > 8192.0 {
        return Err(CanvasError::ResourceLimit);
    }
    // Two additional pixels retain antialiasing and sampling across the clip.
    let padding = support as u32 + 2;
    let render_width = width
        .checked_add(padding * 2)
        .ok_or(CanvasError::ResourceLimit)?;
    let render_height = height
        .checked_add(padding * 2)
        .ok_or(CanvasError::ResourceLimit)?;
    crate::document::validate_surface(render_width, render_height)?;
    Ok(draw::FrameView {
        camera: draw::Camera {
            zoom,
            center: draw::Point::new(
                viewport.camera.center.x
                    + (f64::from(width) - f64::from(viewport.surface.width) * f64::from(scale))
                        / (2.0 * zoom),
                viewport.camera.center.y
                    + (f64::from(height) - f64::from(viewport.surface.height) * f64::from(scale))
                        / (2.0 * zoom),
            ),
        },
        surface: draw::SurfaceSize {
            width: render_width,
            height: render_height,
        },
        ..viewport
    })
}
