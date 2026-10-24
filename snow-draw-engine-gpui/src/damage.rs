//! Conservative retained damage for ordinary Architect source-over geometry.
use crate::{RetainedScene, engine::*};
pub(crate) enum Damage {
    Reuse,
    Region {
        rect: [u32; 4],
        scene: Box<RetainedScene>,
    },
}
fn supported(item: &SceneDisplayItem) -> bool {
    match item {
        SceneDisplayItem::Rectangle(r) => {
            r.stroke_rendering_style == StrokeRenderingStyle::Architect
                && r.blend_mode != DisplayBlendMode::Multiply
        }
        SceneDisplayItem::Arrow(a) => {
            a.stroke_rendering_style == StrokeRenderingStyle::Architect
                && a.blend_mode != DisplayBlendMode::Multiply
        }
        _ => false,
    }
}
pub(crate) fn plan(
    old: &RetainedScene,
    next: &RetainedScene,
    scale: f32,
    size: [u32; 2],
) -> Option<Damage> {
    let frame = next.frame();
    let zoom = frame.camera.zoom * f64::from(scale);
    // Paint matrices and SVG geometry ultimately use f32. At large translated
    // coordinates their rounding can exceed our four-pixel paint guard, even
    // when the f64 world bounds are precise. Reserve a conservative half-pixel
    // transform-error budget; otherwise use the identical full renderer.
    let precise = |coordinate: f64| {
        coordinate.is_finite()
            && coordinate.abs() * zoom.abs() * f64::from(f32::EPSILON) * 32.0 <= 0.5
    };
    if old.frame() != next.frame()
        || old.items().len() != next.items().len()
        || next.decoration().spotlight.active
        || old.decoration().spotlight.active
        || next.decoration().watermark.text_len > 0
        || old.decoration().watermark.text_len > 0
        || !zoom.is_finite()
        || zoom <= 0.0
        || !precise(frame.camera.center.x)
        || !precise(frame.camera.center.y)
    {
        return None;
    }
    let mut changed = 0;
    let mut bounds = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for (a, b) in old.items().iter().zip(next.items()) {
        if !supported(a) || !supported(b) || crate::vector::item_id(a) != crate::vector::item_id(b)
        {
            return None;
        }
        for item in [a, b] {
            let (left, top, right, bottom) = crate::export::item_bounds(item);
            if ![left, top, right, bottom].into_iter().all(precise) {
                return None;
            }
        }
        if a != b {
            changed += 1;
            if changed > 64 {
                return None;
            }
            for item in [a, b] {
                let v = crate::export::item_bounds(item);
                bounds = (
                    bounds.0.min(v.0),
                    bounds.1.min(v.1),
                    bounds.2.max(v.2),
                    bounds.3.max(v.3),
                );
            }
        }
    }
    if changed == 0 {
        return Some(Damage::Reuse);
    }
    let origin = view_to_canvas(Point::new(0.0, 0.0), &frame.camera, frame.surface);
    let x = ((bounds.0 - origin.x) * zoom).floor() - 4.0;
    let y = ((bounds.1 - origin.y) * zoom).floor() - 4.0;
    let right = ((bounds.2 - origin.x) * zoom).ceil() + 4.0;
    let bottom = ((bounds.3 - origin.y) * zoom).ceil() + 4.0;
    let [width, height] = size;
    let x = x.clamp(0.0, f64::from(width)) as u32;
    let y = y.clamp(0.0, f64::from(height)) as u32;
    let right = right.clamp(0.0, f64::from(width)) as u32;
    let bottom = bottom.clamp(0.0, f64::from(height)) as u32;
    if right <= x || bottom <= y {
        return Some(Damage::Reuse);
    }
    let w = right - x;
    let h = bottom - y;
    if u64::from(w) * u64::from(h) * 100 > u64::from(width) * u64::from(height) * 35 {
        return None;
    }
    let center = Point::new(
        origin.x + (f64::from(x) + f64::from(w) / 2.0) / zoom,
        origin.y + (f64::from(y) + f64::from(h) / 2.0) / zoom,
    );
    let tile = FrameView {
        camera: Camera { center, zoom },
        surface: SurfaceSize {
            width: w,
            height: h,
        },
        ..frame
    };
    let tile_bounds = canvas_viewport(tile.camera, tile.surface);
    let scene = next.culled(frame, |item| {
        let b = crate::export::item_bounds(item);
        b.0 <= tile_bounds.2 && b.2 >= tile_bounds.0 && b.1 <= tile_bounds.3 && b.3 >= tile_bounds.1
    });
    Some(Damage::Region {
        rect: [x, y, w, h],
        scene: Box::new(scene),
    })
}
