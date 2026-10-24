use super::*;
use crate::{CanvasRect, ImageSourceLayer};
#[test]
fn decode_preflights_rgba_budget_before_native_decode_or_conversion() {
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, 16, 16);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&[90; 16 * 16 * 3])
        .unwrap();
    // Native RGB fits, converted RGBA does not. A header-only input also rejects
    // on dimensions, before the deliberately missing image payload is decoded.
    for bytes in [&png[..], &png[..41]] {
        assert!(matches!(
            SourceImage::decode_with_limit(bytes, 800),
            Err(CanvasError::ResourceLimit)
        ));
    }
    assert_eq!(
        SourceImage::decode_with_limit(&png, 2048)
            .unwrap()
            .pixels()
            .dimensions(),
        (16, 16)
    );
}
#[test]
fn visual_overlays_preserve_the_eligible_composite_and_persistence() {
    let base = SourceImage::new(RgbaImage::from_fn(80, 60, |x, y| {
        image::Rgba([40 + (x % 10) as u8, 90 + (y % 10) as u8, 150, 255])
    }))
    .unwrap();
    let overlay = SourceImage::new(RgbaImage::from_pixel(
        80,
        60,
        image::Rgba([240, 20, 30, 128]),
    ))
    .unwrap();
    let layered = SourceImage::from_layers(&[
        ImageSourceLayer::new(base.clone()),
        ImageSourceLayer {
            image: overlay,
            smart_erase_source: false,
            canvas_rect: None,
            coverage: None,
        },
    ])
    .unwrap()
    .unwrap();
    assert_eq!(layered.smart_erase_pixels(), base.pixels());
    assert_ne!(layered.pixels(), base.pixels());
    let mut document = CanvasDocument::default();
    document.set_background(Some(layered));
    let restored = CanvasDocument::deserialize(&document.serialize().unwrap()).unwrap();
    assert_eq!(
        restored.background().unwrap().smart_erase_pixels(),
        base.pixels()
    );
}
#[test]
fn effective_coverage_and_anisotropic_placement_bound_projection() {
    let image = SourceImage::new(RgbaImage::from_pixel(
        100,
        100,
        image::Rgba([10, 20, 30, 255]),
    ))
    .unwrap();
    let layers = [
        ImageSourceLayer {
            image: image.clone(),
            canvas_rect: None,
            coverage: Some(CanvasRect::new(10.0, 10.0, 1.0, 1.0).unwrap()),
            smart_erase_source: true,
        },
        ImageSourceLayer {
            image: image.clone(),
            canvas_rect: Some(CanvasRect::new(10.0, 10.0, 1.0, 2.0).unwrap()),
            coverage: None,
            smart_erase_source: true,
        },
    ];
    let result = SourceImage::from_layers(&layers).unwrap().unwrap();
    assert_eq!(result.pixels().dimensions(), (100, 200));
    assert_eq!(result.origin(), Point::new(10.0, 10.0));
    assert_ne!(
        SourceImage::new((**image.pixels()).clone())
            .unwrap()
            .pixels_identity(),
        image.pixels_identity()
    );
    assert_eq!(image.clone().pixels_identity(), image.pixels_identity());
    let cancel = crate::CanvasCancellation::default();
    cancel.cancel();
    assert!(matches!(
        SourceImage::from_layers_cancellable(&layers, &cancel),
        Err(CanvasError::Cancelled)
    ));
}
