use super::*;
use image::Rgba;
use snow_draw_engine::FilterRenderSpec;

fn source() -> SourceImage {
    SourceImage::new(RgbaImage::from_fn(96, 80, |x, y| {
        if (36..60).contains(&x) && (30..50).contains(&y) {
            Rgba([240, 5, 9, 255])
        } else {
            Rgba([70, 120, 180, 255])
        }
    }))
    .unwrap()
}

fn filter() -> FilterDisplayItem {
    FilterDisplayItem {
        center_x: 48.0,
        center_y: 40.0,
        width: 24.0,
        height: 20.0,
        opacity: 1.0,
        filter: FilterRenderSpec::resolve(DisplayFilterType::SmartErase, 0.5),
        ..Default::default()
    }
}

#[test]
fn service_retains_only_cropped_source_grid_patches_and_reuses_opacity() {
    let source = source();
    let filter = filter();
    let mut service = SmartEraseService::default();
    let patch = service
        .solve(&filter, &source, &Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(
        patch.original().raster_pixels().unwrap().dimensions(),
        (28, 24)
    );
    assert_eq!(patch.original().origin(), Point::new(34.0, 28.0));
    assert_eq!(
        patch
            .original()
            .raster_pixels()
            .unwrap()
            .get_pixel(14, 12)
            .0,
        [240, 5, 9, 255]
    );
    assert_eq!(
        patch
            .reconstructed()
            .raster_pixels()
            .unwrap()
            .get_pixel(14, 12)
            .0,
        [70, 120, 180, 255]
    );
    assert_eq!(patch.retained_bytes(), 28 * 24 * 8);
    let mut translucent = filter.clone();
    translucent.opacity = 0.2;
    let reused = service
        .solve(&translucent, &source, &Default::default())
        .unwrap()
        .unwrap();
    assert!(Arc::ptr_eq(&patch, &reused));
    assert!(Arc::ptr_eq(
        &patch,
        &service.snapshot().patch(&translucent, &source).unwrap()
    ));
    assert_eq!(service.snapshot().result_count(), 1);
    translucent.filter.render_phase = 1;
    assert!(service.snapshot().patch(&translucent, &source).is_none());
    assert!(
        service
            .solve(&translucent, &source, &Default::default())
            .is_err()
    );
}

#[test]
fn result_identity_rejects_replaced_source_placement_and_mask() {
    let source = source();
    let filter = filter();
    let mut service = SmartEraseService::default();
    service
        .solve(&filter, &source, &Default::default())
        .unwrap();
    let snapshot = service.snapshot();
    let replacement = SourceImage::new((**source.raster_pixels().unwrap()).clone()).unwrap();
    assert!(snapshot.patch(&filter, &replacement).is_none());
    let moved = source
        .clone()
        .with_placement(Point::new(1.0, 0.0), 1.0)
        .unwrap();
    assert!(snapshot.patch(&filter, &moved).is_none());
    let mut moved_mask = filter.clone();
    moved_mask.center_x += 1.0;
    assert!(snapshot.patch(&moved_mask, &source).is_none());
    let mut frozen = SmartEraseService::default();
    frozen.restore_frozen(&snapshot);
    assert!(frozen.solve(&filter, &source, &Default::default()).is_ok());
    assert!(
        frozen
            .solve(&moved_mask, &source, &Default::default())
            .is_err()
    );
    frozen.unfreeze();
    assert!(
        frozen
            .solve(&moved_mask, &source, &Default::default())
            .is_ok()
    );
    assert_eq!(frozen.snapshot().result_count(), 2);
}

#[test]
fn eligible_pixels_supply_both_reconstruction_and_underpaint() {
    let eligible = source();
    let mut visual =
        SourceImage::new(RgbaImage::from_pixel(96, 80, Rgba([0, 255, 0, 255]))).unwrap();
    visual.smart_erase_pixels = Some(eligible.raster_pixels().unwrap().clone());
    let patch = SmartEraseService::default()
        .solve(&filter(), &visual, &Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(
        patch
            .original()
            .raster_pixels()
            .unwrap()
            .get_pixel(14, 12)
            .0,
        [240, 5, 9, 255]
    );
    assert_eq!(
        patch
            .reconstructed()
            .raster_pixels()
            .unwrap()
            .get_pixel(14, 12)
            .0,
        [70, 120, 180, 255]
    );
    assert_eq!(
        visual.raster_pixels().unwrap().get_pixel(48, 40).0,
        [0, 255, 0, 255]
    );
}

#[test]
fn background_job_reports_revision_and_failures_without_rendering_other_items() {
    let source = source();
    let mut unfinished = filter();
    unfinished.filter.render_phase = 1;
    let mut unrelated = filter();
    unrelated.filter = FilterRenderSpec::resolve(DisplayFilterType::GaussianBlur, 0.5);
    let mut no_donors = filter();
    no_donors.width = 400.0;
    no_donors.height = 400.0;
    let result = SmartEraseJob::new(
        72,
        Some(source.clone()),
        vec![unfinished, unrelated, filter(), no_donors],
        Default::default(),
    )
    .run(&Default::default())
    .unwrap();
    assert_eq!(result.revision, 72);
    assert_eq!(result.snapshot.result_count(), 1);
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("without unmasked source donors"));
    assert!(result.snapshot.patch(&filter(), &source).is_some());
}

#[test]
fn cancellation_never_publishes_partial_cache_or_job() {
    let source = source();
    let mut service = SmartEraseService::default();
    let cancel = CanvasCancellation::after_checks(4);
    assert!(matches!(
        service.solve(&filter(), &source, &cancel),
        Err(CanvasError::Cancelled)
    ));
    assert_eq!(service.snapshot().result_count(), 0);
    let job = SmartEraseJob::new(9, Some(source), vec![filter()], Default::default());
    assert!(matches!(job.run(&cancel), Err(CanvasError::Cancelled)));
}

#[test]
fn local_masks_are_precise_at_large_coordinates_and_support_brushes() {
    let origin = Point::new(999_999_000.0, -999_999_000.0);
    let units = 1.25;
    let mut translated = filter();
    translated.center_x = origin.x + 48.0 * units;
    translated.center_y = origin.y + 40.0 * units;
    translated.width *= units;
    translated.height *= units;
    let mask = reconstruction_mask(&translated, origin, units, 96, 80).unwrap();
    let baseline = reconstruction_mask(&filter(), Point::new(0.0, 0.0), 1.0, 96, 80).unwrap();
    assert_eq!(mask, baseline);
    let mut brush = filter();
    brush.is_pen_filter = true;
    brush.stroke_width = 10.0;
    brush.points = vec![[20.0, 20.0]].into();
    let circle = reconstruction_mask(&brush, Point::new(0.0, 0.0), 1.0, 64, 64).unwrap();
    assert_eq!(circle[20 * 64 + 20], 255);
    assert_eq!(circle[20 * 64 + 30], 0);
    brush.points = vec![[20.0, 20.0], [40.0, 20.0]].into();
    let stroke = reconstruction_mask(&brush, Point::new(0.0, 0.0), 1.0, 64, 64).unwrap();
    assert_eq!(stroke[20 * 64 + 30], 255);
    assert_eq!(stroke[30 * 64 + 30], 0);
}

#[test]
fn service_rejects_oversized_roi_and_invalid_masks_before_reconstruction() {
    let source = SourceImage::new(RgbaImage::from_pixel(1500, 1500, Rgba([5, 6, 7, 255]))).unwrap();
    let mut mask = filter();
    mask.center_x = 750.0;
    mask.center_y = 750.0;
    mask.width = 1000.0;
    mask.height = 1000.0;
    let mut service = SmartEraseService::default();
    assert!(matches!(
        service.solve(&mask, &source, &Default::default()),
        Err(CanvasError::ResourceLimit)
    ));
    assert_eq!(service.snapshot().retained_bytes(), 0);
    mask.width = f64::NAN;
    assert!(service.solve(&mask, &source, &Default::default()).is_err());
    mask = filter();
    mask.is_pen_filter = true;
    assert!(service.solve(&mask, &source, &Default::default()).is_err());
    mask = filter();
    mask.center_x = -10_000.0;
    assert!(
        service
            .solve(&mask, &source, &Default::default())
            .unwrap()
            .is_none()
    );
}
