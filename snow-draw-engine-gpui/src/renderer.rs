use std::{fmt::Write, sync::Arc, time::Instant};
#[cfg(test)]
#[path = "renderer_tests.rs"]
mod tests;

use image::{Rgba, RgbaImage};
use resvg::tiny_skia::{Pixmap, Transform};
use snow_draw_engine::*;

use crate::{
    CanvasError, RetainedScene, SourceImage,
    document::validate_surface,
    vector::{VectorCache, color, rectangle_path},
};

/// Bounded retained compositor. Overlay revisions do not enter the document render key.
pub struct CanvasRenderer {
    profiling: bool,
    profile: crate::CanvasRenderProfile,
    pub(crate) vectors: VectorCache,
    last_key: Option<RenderKey>,
    last_image: Option<Arc<RgbaImage>>,
    last_scene: Option<RetainedScene>,
    damage_enabled: bool,
    renders: u64,
    last_ms: f64,
    peak_working_pixels: u64,
    smart_cache: std::collections::VecDeque<SmartResult>,
    smart_frozen: bool,
    warnings: Vec<String>,
}

#[derive(Clone)]
struct SmartResult {
    geometry: FilterDisplayItem,
    source: SmartSourceKey,
    original: SourceImage,
    filled: SourceImage,
    bytes: usize,
}

#[derive(Clone, Copy, PartialEq)]
struct SmartSourceKey {
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

/// Immutable shared results retain source identity and geometry, so importing a
/// seed can never apply a reconstruction to different source pixels.
#[derive(Clone, Default)]
pub struct SmartEraseSnapshot {
    entries: std::collections::VecDeque<SmartResult>,
}
impl SmartEraseSnapshot {
    pub fn result_count(&self) -> usize {
        self.entries.len()
    }
    pub fn retained_bytes(&self) -> usize {
        self.entries.iter().map(|entry| entry.bytes).sum()
    }
}

#[derive(Clone, Copy, PartialEq)]
struct RenderKey {
    identity: u64,
    scene: SceneRevision,
    decoration: DecorationRevision,
    frame: FrameView,
    resources: u64,
    scale: f32,
    strict: bool,
    presentation: u64,
}

impl Default for CanvasRenderer {
    fn default() -> Self {
        Self {
            profiling: false,
            profile: Default::default(),
            vectors: VectorCache::default(),
            last_key: None,
            last_image: None,
            last_scene: None,
            damage_enabled: true,
            renders: 0,
            last_ms: 0.0,
            peak_working_pixels: 0,
            smart_cache: Default::default(),
            smart_frozen: false,
            warnings: Vec::new(),
        }
    }
}

impl CanvasRenderer {
    /// CPU phase timing is opt-in and excludes GPUI upload/presentation.
    pub fn set_profiling(&mut self, enabled: bool) {
        self.profiling = enabled;
        self.vectors.profiling = enabled;
    }
    pub fn last_render_profile(&self) -> crate::CanvasRenderProfile {
        self.profile
    }
    /// Unsupported dependencies retain full rendering. Disable for reference comparisons.
    pub fn set_damage_rendering(&mut self, enabled: bool) {
        self.damage_enabled = enabled;
    }
    pub fn smart_erase_snapshot(&self) -> SmartEraseSnapshot {
        SmartEraseSnapshot {
            entries: self.smart_cache.clone(),
        }
    }
    pub fn seed_smart_erase_snapshot(&mut self, seed: &SmartEraseSnapshot) {
        for entry in &seed.entries {
            if !self
                .smart_cache
                .iter()
                .any(|old| old.geometry == entry.geometry && old.source == entry.source)
            {
                while self.smart_cache.iter().map(|old| old.bytes).sum::<usize>() + entry.bytes
                    > 128 * 1024 * 1024
                {
                    self.smart_cache.pop_front();
                }
                self.smart_cache.push_back(entry.clone());
            }
        }
        self.last_key = None;
    }
    /// Freezes completed results; missing geometry returns an explicit error.
    pub fn restore_smart_erase_snapshot(&mut self, snapshot: &SmartEraseSnapshot) {
        self.smart_cache = snapshot.entries.clone();
        self.smart_frozen = true;
        self.last_key = None;
    }
    pub fn unfreeze_smart_erase(&mut self) {
        self.smart_frozen = false;
        self.last_key = None;
    }
    /// Prepares a reusable export snapshot with complete ordered-effect dependencies.
    pub fn snapshot(
        &mut self,
        document: &mut crate::CanvasDocument,
        session: &mut crate::CanvasSession,
    ) -> Result<crate::CanvasSnapshot, CanvasError> {
        self.snapshot_with_reference(document, session, None)
    }
    pub fn snapshot_with_reference(
        &mut self,
        document: &mut crate::CanvasDocument,
        session: &mut crate::CanvasSession,
        grid: Option<crate::CanvasRenderReference>,
    ) -> Result<crate::CanvasSnapshot, CanvasError> {
        self.prepare(document, session)?;
        let mut reference = crate::CanvasSession::new(document)?;
        let prepared = if let Some(grid) = grid {
            crate::snapshot::prepare_fixed_reference(document, &mut reference, grid)
        } else {
            crate::snapshot::prepare_reference(document, &mut reference, session.scene().frame())
        };
        let result = prepared.map(|()| {
            crate::CanvasSnapshot::from_reference(document, &reference, session.scene().frame())
        });
        reference.close(document)?;
        result
    }

    /// Exports the requested viewport, including pixels sampled beyond its edges.
    pub fn export_snapshot_png(
        &mut self,
        snapshot: &crate::CanvasSnapshot,
        scale: f32,
    ) -> Result<Vec<u8>, CanvasError> {
        self.export_snapshot_png_cancellable(snapshot, scale, &Default::default())
    }
    pub fn export_snapshot_png_cancellable(
        &mut self,
        snapshot: &crate::CanvasSnapshot,
        scale: f32,
        cancel: &crate::CanvasCancellation,
    ) -> Result<Vec<u8>, CanvasError> {
        cancel.check()?;
        if !scale.is_finite() || !(0.1..=8.0).contains(&scale) {
            return Err(ErrorCode::InvalidArgument.into());
        }
        let frame = snapshot.viewport;
        let [width, height] = crate::export::dimensions(snapshot, scale)?;
        if validate_surface(width, height).is_err() {
            let mut output = crate::export::BoundedOutput::default();
            self.export_snapshot_png_to_writer(snapshot, scale, &mut output, cancel)?;
            return Ok(output.bytes);
        }
        validate_surface(width, height)?;
        let zoom = frame.camera.zoom * f64::from(scale);
        let support = (crate::snapshot::sampling_support(&snapshot.scene) * zoom).ceil();
        if !support.is_finite() || support > 8192.0 {
            return Err(CanvasError::ResourceLimit);
        }
        let padding = support as u32 + 2;
        let render_width = width
            .checked_add(padding * 2)
            .ok_or(CanvasError::ResourceLimit)?;
        let render_height = height
            .checked_add(padding * 2)
            .ok_or(CanvasError::ResourceLimit)?;
        if validate_surface(render_width, render_height).is_err() {
            let mut output = crate::export::BoundedOutput::default();
            self.export_snapshot_png_to_writer(snapshot, scale, &mut output, cancel)?;
            return Ok(output.bytes);
        }
        let scene = snapshot.scene.with_frame(FrameView {
            camera: Camera {
                zoom,
                center: Point::new(
                    frame.camera.center.x
                        + (f64::from(width) - f64::from(frame.surface.width) * f64::from(scale))
                            / (2.0 * zoom),
                    frame.camera.center.y
                        + (f64::from(height) - f64::from(frame.surface.height) * f64::from(scale))
                            / (2.0 * zoom),
                ),
            },
            surface: SurfaceSize {
                width: render_width,
                height: render_height,
            },
            ..frame
        });
        let image = self.render_presented_cancellable(
            &scene,
            snapshot.background.as_ref(),
            snapshot.resources,
            1.0,
            cancel,
            true,
            &snapshot.presentation,
        )?;
        let cropped =
            image::imageops::crop_imm(image.as_ref(), padding, padding, width, height).to_image();
        let mut bytes = std::io::Cursor::new(Vec::new());
        cropped
            .write_to(&mut bytes, image::ImageFormat::Png)
            .map_err(|e| CanvasError::InvalidFile(e.to_string()))?;
        cancel.check()?;
        Ok(bytes.into_inner())
    }

    pub fn export_snapshot_svg(
        &mut self,
        snapshot: &crate::CanvasSnapshot,
        scale: f32,
    ) -> Result<String, CanvasError> {
        let [width, height] = crate::export::dimensions(snapshot, scale)?;
        if validate_surface(width, height).is_err()
            || snapshot
                .background
                .as_ref()
                .is_some_and(|source| source.pixels().as_raw().len() > 64 * 1024 * 1024)
        {
            let mut output = crate::export::BoundedOutput::default();
            self.export_snapshot_svg_to_writer(snapshot, scale, &mut output, &Default::default())?;
            return String::from_utf8(output.bytes)
                .map_err(|error| CanvasError::InvalidFile(error.to_string()));
        }
        let flatten = !snapshot.presentation.is_default()
            || snapshot.scene.decoration().spotlight.active
            || snapshot.scene.decoration().watermark.text_len > 0
            || snapshot.scene.items().iter().any(|item| {
                matches!(
                    item,
                    SceneDisplayItem::Filter(_) | SceneDisplayItem::Magnifier(_)
                )
            });
        if !flatten {
            return self.export_svg(
                &snapshot.scene.with_frame(snapshot.viewport),
                snapshot.background.as_ref(),
                snapshot.resources,
                scale,
            );
        }
        let mut output = crate::export::BoundedOutput::default();
        self.export_snapshot_svg_to_writer(snapshot, scale, &mut output, &Default::default())?;
        String::from_utf8(output.bytes).map_err(|error| CanvasError::InvalidFile(error.to_string()))
    }

    /// Packs disjoint rectangles from one synchronized source pixel frame. The
    /// dependency halos and the original pixel grid are retained per output tile.
    pub fn export_snapshot_tiles_png(
        &mut self,
        snapshot: &crate::CanvasSnapshot,
        scale: f32,
        output_size: [u32; 2],
        tiles: &[crate::ExportTile],
    ) -> Result<Vec<u8>, CanvasError> {
        let mut output = crate::export::BoundedOutput::default();
        self.export_snapshot_tiles_to_writer(
            snapshot,
            scale,
            output_size,
            tiles,
            &mut output,
            &Default::default(),
        )?;
        Ok(output.bytes)
    }

    pub fn clear_render_state(&mut self) {
        self.last_key = None;
        self.last_image = None;
        self.last_scene = None;
        self.smart_cache.clear();
        self.smart_frozen = false;
        self.vectors.clear();
    }

    /// Completes engine label/resize measurements before taking a renderer snapshot.
    pub fn prepare(
        &mut self,
        document: &mut crate::CanvasDocument,
        session: &mut crate::CanvasSession,
    ) -> Result<(), CanvasError> {
        crate::text_layout::synchronize(&mut self.vectors, document, session.viewport())?;
        session.refresh(document)?;
        Ok(())
    }
    pub fn measure_text(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: f64,
        fixed_width: bool,
    ) -> TextLayoutSize {
        let layout = self.vectors.layout(text, style, max_width.max(1.0));
        TextLayoutSize::with_content(
            if fixed_width {
                max_width.max(1.0)
            } else {
                layout.width
            },
            layout.height,
            layout.width,
            layout.height,
        )
    }
    pub fn render_count(&self) -> u64 {
        self.renders
    }
    pub fn vector_build_count(&self) -> u64 {
        self.vectors.builds
    }
    pub fn retained_vector_bytes(&self) -> usize {
        self.vectors.retained_bytes().0
    }
    pub fn retained_text_layout_bytes(&self) -> usize {
        self.vectors.retained_bytes().1
    }
    pub fn last_render_ms(&self) -> f64 {
        self.last_ms
    }
    pub fn peak_working_surface_pixels(&self) -> u64 {
        self.peak_working_pixels
    }

    /// Renders logical viewport pixels at `scale` physical pixels per logical pixel.
    pub fn render(
        &mut self,
        scene: &RetainedScene,
        background: Option<&SourceImage>,
        resource_revision: u64,
        scale: f32,
    ) -> Result<Arc<RgbaImage>, CanvasError> {
        self.render_cancellable(
            scene,
            background,
            resource_revision,
            scale,
            &Default::default(),
            true,
        )
    }

    pub fn render_cancellable(
        &mut self,
        scene: &RetainedScene,
        background: Option<&SourceImage>,
        resource_revision: u64,
        scale: f32,
        cancel: &crate::smart_erase::Cancellation,
        strict: bool,
    ) -> Result<Arc<RgbaImage>, CanvasError> {
        self.render_presented_cancellable(
            scene,
            background,
            resource_revision,
            scale,
            cancel,
            strict,
            &Default::default(),
        )
    }

    pub fn render_with_presentation(
        &mut self,
        scene: &RetainedScene,
        background: Option<&SourceImage>,
        resource_revision: u64,
        scale: f32,
        presentation: &crate::CanvasPresentation,
    ) -> Result<Arc<RgbaImage>, CanvasError> {
        self.render_presented_cancellable(
            scene,
            background,
            resource_revision,
            scale,
            &Default::default(),
            true,
            presentation,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_presented_cancellable(
        &mut self,
        scene: &RetainedScene,
        background: Option<&SourceImage>,
        resource_revision: u64,
        scale: f32,
        cancel: &crate::smart_erase::Cancellation,
        strict: bool,
        presentation: &crate::CanvasPresentation,
    ) -> Result<Arc<RgbaImage>, CanvasError> {
        let call_start = self.profiling.then(Instant::now);
        presentation.validate()?;
        cancel.check()?;
        if !scale.is_finite() || !(0.1..=8.0).contains(&scale) {
            return Err(ErrorCode::InvalidArgument.into());
        }
        let cursor = scene.cursor().ok_or(ErrorCode::InvalidState)?;
        let key = RenderKey {
            identity: scene.render_identity(),
            scene: cursor.scene_revision,
            decoration: cursor.decoration_revision,
            frame: scene.frame(),
            resources: resource_revision,
            scale,
            strict,
            presentation: presentation.cache_key(),
        };
        if self.last_key == Some(key)
            && let Some(image) = &self.last_image
        {
            if self.profiling {
                self.profile = crate::CanvasRenderProfile {
                    raster_cache_hit: true,
                    total_ms: crate::profile::elapsed(call_start),
                    ..Default::default()
                };
            }
            return Ok(image.clone());
        }
        let start = Instant::now();
        let full_scene = scene;
        let output_size = crate::export::scaled_dimensions(scene.frame().surface, scale)?;
        let compatible = self.last_key.is_some_and(|old| {
            old.frame == key.frame
                && old.resources == key.resources
                && old.scale == key.scale
                && old.strict == key.strict
                && old.presentation == key.presentation
        });
        let damage = if self.damage_enabled
            && compatible
            && presentation.is_default()
            && self.last_image.is_some()
        {
            self.last_scene
                .as_ref()
                .and_then(|old| crate::damage::plan(old, scene, scale, output_size))
        } else {
            None
        };
        if matches!(damage, Some(crate::damage::Damage::Reuse)) {
            self.last_key = Some(key);
            self.last_scene = Some(scene.clone());
            if self.profiling {
                self.profile = crate::CanvasRenderProfile {
                    raster_cache_hit: true,
                    total_ms: crate::profile::elapsed(call_start),
                    ..Default::default()
                };
            }
            return Ok(self.last_image.as_ref().unwrap().clone());
        }
        let (scene, scale, damage_rect) = match &damage {
            Some(crate::damage::Damage::Region { rect, scene }) => {
                (scene.as_ref(), scale, Some(*rect))
            }
            _ => (scene, scale, None),
        };
        let mut profile = crate::CanvasRenderProfile::default();
        profile.partial_recomposition = damage_rect.is_some();
        profile.output_pixels = u64::from(output_size[0]) * u64::from(output_size[1]);
        profile.damaged_pixels =
            damage_rect.map_or(profile.output_pixels, |r| u64::from(r[2]) * u64::from(r[3]));
        self.vectors.profile = Default::default();
        self.warnings.clear();
        let frame = scene.frame();
        self.vectors.set_clear_color(frame.clear_color);
        let [width, height] = crate::export::scaled_dimensions(frame.surface, scale)?;
        validate_surface(width, height)?;
        profile.working_pixels = u64::from(width) * u64::from(height);
        self.peak_working_pixels = self
            .peak_working_pixels
            .max(u64::from(width) * u64::from(height));
        let mut background_frame = frame;
        if !presentation.clear_background {
            background_frame.clear_color = ColorRgba8 {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            };
        }
        let background_start = self.profiling.then(Instant::now);
        let pristine = if let Some(rect) = damage_rect {
            background_region(
                background_frame,
                background,
                width,
                height,
                scale,
                rect,
                cancel,
            )?
        } else {
            background_surface(background_frame, background, width, height, scale)
        };
        let mut target = if let Some(image) = &presentation.background_override {
            image_to_pixmap(&background_surface(
                background_frame,
                Some(image),
                width,
                height,
                scale,
            ))?
        } else {
            image_to_pixmap(&pristine)?
        };
        paint_host_source(
            &mut target,
            presentation.before.as_ref(),
            frame,
            scale,
            cancel,
        )?;
        let before_content = presentation.content_clip.map(|_| target.clone());
        profile.background_ms = crate::profile::elapsed(background_start);
        let transform = viewport_transform(frame, scale);
        if presentation.content_visible {
            let before_spotlight = presentation.spotlight_area.map(|_| target.clone());
            self.spotlight(scene, transform, &mut target)?;
            restore_outside(
                &mut target,
                before_spotlight.as_ref(),
                presentation.spotlight_area,
                frame,
                scale,
            );
            let mut pass_id = None;
            let mut entering = None;
            let mut index = 0;
            while index < scene.items().len() {
                cancel.check()?;
                if let Some(run) = scene.runs().iter().find(|run| run.start as usize == index) {
                    let effect_start = self.profiling.then(Instant::now);
                    if pass_id != Some(run.source_pass) {
                        entering = Some(pixmap_to_image(&target));
                        pass_id = Some(run.source_pass);
                    }
                    let SceneDisplayItem::Filter(first) = &scene.items()[index] else {
                        return Err(ErrorCode::InvalidState.into());
                    };
                    let source = entering.as_ref().ok_or(ErrorCode::InvalidState)?;
                    let mut union = vec![0_u8; width as usize * height as usize];
                    for item in &scene.items()[index..index + run.count as usize] {
                        let SceneDisplayItem::Filter(filter) = item else {
                            return Err(ErrorCode::InvalidState.into());
                        };
                        let mask = self.filter_mask(filter, transform, width, height)?;
                        for (alpha, mask) in union.iter_mut().zip(mask.pixels()) {
                            *alpha = (u32::from(*alpha)
                                + (u32::from(mask.alpha()) * (255 - u32::from(*alpha)) + 127) / 255)
                                as u8;
                        }
                    }
                    // Only process the affected crop plus the complete sampling support.
                    // The entering pass stays immutable and mosaic remains world anchored.
                    if let Some((left, top, right, bottom)) = mask_bounds(&union, width, height) {
                        let support =
                            (first.filter.sampling_radius * frame.camera.zoom * f64::from(scale))
                                .ceil()
                                .max(0.0) as u32;
                        let x = left.saturating_sub(support);
                        let y = top.saturating_sub(support);
                        let w = right.saturating_add(support).min(width) - x;
                        let h = bottom.saturating_add(support).min(height) - y;
                        let source = image::imageops::crop_imm(source, x, y, w, h).to_image();
                        let original = image::imageops::crop_imm(&pristine, x, y, w, h).to_image();
                        let center = view_to_canvas(
                            Point::new(
                                (f64::from(x) + f64::from(w) / 2.0) / f64::from(scale),
                                (f64::from(y) + f64::from(h) / 2.0) / f64::from(scale),
                            ),
                            &frame.camera,
                            frame.surface,
                        );
                        let crop_frame = FrameView {
                            surface: SurfaceSize {
                                width: w,
                                height: h,
                            },
                            camera: Camera {
                                center,
                                zoom: frame.camera.zoom * f64::from(scale),
                            },
                            ..frame
                        };
                        let filtered = effect(&source, &original, &first.filter, crop_frame, 1.0)?;
                        replace_masked_region(&mut target, &filtered, &union, x, y);
                    }
                    index += run.count as usize;
                    profile.effects_ms += crate::profile::elapsed(effect_start);
                    continue;
                }
                let item = &scene.items()[index];
                pass_id = None;
                entering = None;
                match item {
                    SceneDisplayItem::Filter(filter) => {
                        let effect_start = self.profiling.then(Instant::now);
                        if filter.filter.filter_type == DisplayFilterType::SmartErase {
                            if strict && filter.filter.render_phase != 0 {
                                return Err(CanvasError::Unsupported(
                                    "exporting an unfinished Smart Erase gesture",
                                ));
                            }
                            match self.smart_erase(
                                filter,
                                background,
                                frame,
                                scale,
                                &mut target,
                                cancel,
                            ) {
                                Ok(()) => {}
                                Err(CanvasError::Cancelled) => return Err(CanvasError::Cancelled),
                                Err(error) if !strict => {
                                    self.warnings.push(error.to_string());
                                    let mut unresolved = filter.clone();
                                    unresolved.filter.render_phase = 1;
                                    self.smart_erase(
                                        &unresolved,
                                        background,
                                        frame,
                                        scale,
                                        &mut target,
                                        cancel,
                                    )?;
                                }
                                Err(error) => return Err(error),
                            }
                            index += 1;
                            profile.effects_ms += crate::profile::elapsed(effect_start);
                            continue;
                        }
                        let mask = self.filter_mask(filter, transform, width, height)?;
                        let alpha: Vec<_> =
                            mask.pixels().iter().map(|pixel| pixel.alpha()).collect();
                        let result = match filter.filter.filter_type {
                            DisplayFilterType::RestoreBackground => pristine.clone(),
                            _ => return Err(ErrorCode::InvalidState.into()),
                        };
                        replace_masked(&mut target, &result, &alpha);
                        profile.effects_ms += crate::profile::elapsed(effect_start);
                    }
                    SceneDisplayItem::Magnifier(lens) => {
                        let effect_start = self.profiling.then(Instant::now);
                        self.magnifier(lens, background, frame, scale, &mut target)?;
                        profile.effects_ms += crate::profile::elapsed(effect_start);
                    }
                    _ => {
                        let tree = self.vectors.item(item)?;
                        let raster_start = self.profiling.then(Instant::now);
                        let multiply = matches!(item, SceneDisplayItem::Rectangle(value) if value.blend_mode == DisplayBlendMode::Multiply)
                            || matches!(item, SceneDisplayItem::Arrow(value) if value.blend_mode == DisplayBlendMode::Multiply);
                        if multiply {
                            let mut layer =
                                Pixmap::new(width, height).ok_or(CanvasError::ResourceLimit)?;
                            resvg::render(&tree, transform, &mut layer.as_mut());
                            target.draw_pixmap(
                                0,
                                0,
                                layer.as_ref(),
                                &resvg::tiny_skia::PixmapPaint {
                                    blend_mode: resvg::tiny_skia::BlendMode::Multiply,
                                    ..Default::default()
                                },
                                Transform::identity(),
                                None,
                            );
                        } else {
                            resvg::render(&tree, transform, &mut target.as_mut());
                        }
                        profile.raster_ms += crate::profile::elapsed(raster_start);
                    }
                }
                index += 1;
            }
            let before_watermark = presentation.watermark_area.map(|_| target.clone());
            self.watermark(scene, transform, &mut target)?;
            restore_outside(
                &mut target,
                before_watermark.as_ref(),
                presentation.watermark_area,
                frame,
                scale,
            );
        }
        restore_outside(
            &mut target,
            before_content.as_ref(),
            presentation.content_clip,
            frame,
            scale,
        );
        paint_host_source(
            &mut target,
            presentation.after.as_ref(),
            frame,
            scale,
            cancel,
        )?;
        cancel.check()?;
        let finalize_start = self.profiling.then(Instant::now);
        self.vectors.retain(full_scene.items());
        let image = if let Some([x, y, w, h]) = damage_rect {
            let crop = target
                .clone_rect(
                    resvg::tiny_skia::IntRect::from_xywh(x as i32, y as i32, w, h)
                        .ok_or(CanvasError::ResourceLimit)?,
                )
                .ok_or(CanvasError::ResourceLimit)?;
            let pixels = pixmap_to_image(&crop);
            let mut complete = (**self.last_image.as_ref().unwrap()).clone();
            image::imageops::replace(&mut complete, &pixels, i64::from(x), i64::from(y));
            Arc::new(complete)
        } else {
            Arc::new(pixmap_to_image(&target))
        };
        cancel.check()?;
        self.renders += 1;
        self.last_ms = start.elapsed().as_secs_f64() * 1000.0;
        if self.profiling {
            profile.finalize_ms = crate::profile::elapsed(finalize_start);
            profile.total_ms = crate::profile::elapsed(call_start);
            profile.vector_lower_and_layout_ms = self.vectors.profile.vector_lower_and_layout_ms;
            profile.svg_parse_and_shape_ms = self.vectors.profile.svg_parse_and_shape_ms;
            profile.vector_cache_ms = self.vectors.profile.vector_cache_ms;
            profile.vector_requests = self.vectors.profile.vector_requests;
            profile.vector_hits = self.vectors.profile.vector_hits;
            profile.vector_builds = self.vectors.profile.vector_builds;
            self.profile = profile;
        }
        self.last_key = Some(key);
        self.last_scene = Some(full_scene.clone());
        self.last_image = Some(image.clone());
        Ok(image)
    }

    fn smart_erase(
        &mut self,
        filter: &FilterDisplayItem,
        background: Option<&SourceImage>,
        frame: FrameView,
        scale: f32,
        target: &mut Pixmap,
        cancel: &crate::smart_erase::Cancellation,
    ) -> Result<(), CanvasError> {
        let mut geometry = filter.clone();
        geometry.opacity = 1.0;
        geometry.filter.render_phase = 0;
        let width = target.width();
        let height = target.height();
        let mask = self.filter_mask(&geometry, viewport_transform(frame, scale), width, height)?;
        if filter.filter.render_phase != 0 {
            for (pixel, alpha) in target.pixels_mut().iter_mut().zip(mask.pixels()) {
                *pixel = over(*pixel, [220, 66, 66, 54], f64::from(alpha.alpha()) / 255.0);
            }
            return Ok(());
        }
        let Some(source) = background else {
            return Ok(());
        };
        if !mask.pixels().iter().any(|pixel| pixel.alpha() != 0) {
            return Ok(());
        }
        let key = SmartSourceKey::from(source);
        let cached = self
            .smart_cache
            .iter()
            .position(|entry| entry.geometry == geometry && entry.source == key);
        let (original_source, filled) = if let Some(index) = cached {
            let entry = self.smart_cache.remove(index).unwrap();
            let result = (entry.original.clone(), entry.filled.clone());
            self.smart_cache.push_back(entry);
            result
        } else {
            if self.smart_frozen {
                return Err(CanvasError::Unsupported(
                    "Smart Erase geometry missing from the frozen result snapshot",
                ));
            }
            let pixels = source.smart_erase_pixels();
            let units = source.units_per_pixel();
            let (left, top, right, bottom) = filter_canvas_bounds(&geometry);
            let x0 = (((left - source.origin().x) / units).floor() - 512.0)
                .clamp(0.0, f64::from(pixels.width())) as u32;
            let y0 = (((top - source.origin().y) / units).floor() - 512.0)
                .clamp(0.0, f64::from(pixels.height())) as u32;
            let x1 = (((right - source.origin().x) / units).ceil() + 512.0)
                .clamp(0.0, f64::from(pixels.width())) as u32;
            let y1 = (((bottom - source.origin().y) / units).ceil() + 512.0)
                .clamp(0.0, f64::from(pixels.height())) as u32;
            if x1 <= x0 || y1 <= y0 {
                return Ok(());
            }
            if u64::from(x1 - x0) * u64::from(y1 - y0) > 2 * 1024 * 1024 {
                return Err(CanvasError::ResourceLimit);
            }
            cancel.check()?;
            let crop =
                image::imageops::crop_imm(pixels.as_ref(), x0, y0, x1 - x0, y1 - y0).to_image();
            let origin = Point::new(
                source.origin().x + f64::from(x0) * units,
                source.origin().y + f64::from(y0) * units,
            );
            let original = SourceImage::new(crop)?.with_placement(origin, units)?;
            let native = FrameView {
                camera: Camera {
                    center: Point::new(
                        origin.x + f64::from(x1 - x0) * units / 2.0,
                        origin.y + f64::from(y1 - y0) * units / 2.0,
                    ),
                    zoom: 1.0 / units,
                },
                surface: SurfaceSize {
                    width: x1 - x0,
                    height: y1 - y0,
                },
                ..frame
            };
            let native_mask =
                self.filter_mask(&geometry, viewport_transform(native, 1.0), x1 - x0, y1 - y0)?;
            let alpha: Vec<_> = native_mask
                .pixels()
                .iter()
                .map(|pixel| pixel.alpha())
                .collect();
            let output = crate::smart_erase::reconstruct_with_donors(
                original.pixels(),
                &alpha,
                None,
                cancel,
            )?;
            // Donor context belongs to the solve, not its retained result. Keep
            // only the affected mask plus bilinear sampling guard pixels.
            let mut affected = [x1 - x0, y1 - y0, 0, 0];
            for (index, pixel) in native_mask.pixels().iter().enumerate() {
                if pixel.alpha() != 0 {
                    let x = index as u32 % native_mask.width();
                    let y = index as u32 / native_mask.width();
                    affected[0] = affected[0].min(x);
                    affected[1] = affected[1].min(y);
                    affected[2] = affected[2].max(x + 1);
                    affected[3] = affected[3].max(y + 1);
                }
            }
            if affected[2] == 0 || affected[3] == 0 {
                return Ok(());
            }
            let left = affected[0].saturating_sub(2);
            let top = affected[1].saturating_sub(2);
            let right = affected[2].saturating_add(2).min(x1 - x0);
            let bottom = affected[3].saturating_add(2).min(y1 - y0);
            let retained_origin = Point::new(
                origin.x + f64::from(left) * units,
                origin.y + f64::from(top) * units,
            );
            let original = SourceImage::new(
                image::imageops::crop_imm(
                    original.pixels().as_ref(),
                    left,
                    top,
                    right - left,
                    bottom - top,
                )
                .to_image(),
            )?
            .with_placement(retained_origin, units)?;
            let filled = SourceImage::new(
                image::imageops::crop_imm(&output, left, top, right - left, bottom - top)
                    .to_image(),
            )?
            .with_placement(retained_origin, units)?;
            let bytes = original.pixels().as_raw().len() * 2;
            while self
                .smart_cache
                .iter()
                .map(|entry| entry.bytes)
                .sum::<usize>()
                + bytes
                > 128 * 1024 * 1024
            {
                if self.smart_cache.pop_front().is_none() {
                    break;
                }
            }
            if bytes <= 128 * 1024 * 1024 {
                self.smart_cache.push_back(SmartResult {
                    geometry,
                    source: key,
                    original: original.clone(),
                    filled: filled.clone(),
                    bytes,
                });
            }
            (original, filled)
        };
        // Match the engine host contract: draw pristine source under the result,
        // then apply reconstructed source with item opacity. This is source-over,
        // unlike Restore Background's premultiplied replacement operation.
        for y in 0..height {
            cancel.check()?;
            for x in 0..width {
                let i = y as usize * width as usize + x as usize;
                let alpha = f64::from(mask.pixels()[i].alpha()) / 255.0;
                if alpha == 0.0 {
                    continue;
                }
                let world = view_to_canvas(
                    Point::new(
                        (f64::from(x) + 0.5) / f64::from(scale),
                        (f64::from(y) + 0.5) / f64::from(scale),
                    ),
                    &frame.camera,
                    frame.surface,
                );
                let original = sample_source(&original_source, world).unwrap_or(Rgba([0; 4]));
                let value = sample_source(&filled, world).unwrap_or(Rgba([0; 4]));
                target.pixels_mut()[i] = over(target.pixels()[i], original.0, alpha);
                target.pixels_mut()[i] = over(target.pixels()[i], value.0, alpha * filter.opacity);
            }
        }
        Ok(())
    }

    pub fn export_png(
        &mut self,
        scene: &RetainedScene,
        background: Option<&SourceImage>,
        resource_revision: u64,
        scale: f32,
    ) -> Result<Vec<u8>, CanvasError> {
        let image = self.render(scene, background, resource_revision, scale)?;
        let mut output = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut output, image::ImageFormat::Png)
            .map_err(|error| CanvasError::InvalidFile(error.to_string()))?;
        Ok(output.into_inner())
    }

    /// Recoverable interactive item failures. Strict exports return these as errors.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Plain documents retain vector paths and outlined text. Ordered backdrop effects
    /// are flattened losslessly because their sampling semantics are not portable SVG.
    pub fn export_svg(
        &mut self,
        scene: &RetainedScene,
        background: Option<&SourceImage>,
        resource_revision: u64,
        scale: f32,
    ) -> Result<String, CanvasError> {
        self.export_svg_bounded(
            scene,
            background,
            resource_revision,
            scale,
            96 * 1024 * 1024,
        )
    }
    pub(crate) fn export_svg_bounded(
        &mut self,
        scene: &RetainedScene,
        background: Option<&SourceImage>,
        resource_revision: u64,
        scale: f32,
        limit: usize,
    ) -> Result<String, CanvasError> {
        use base64::Engine as _;
        if !scale.is_finite() || !(0.1..=8.0).contains(&scale) {
            return Err(ErrorCode::InvalidArgument.into());
        }
        let frame = scene.frame();
        let [width, height] = crate::export::scaled_dimensions(frame.surface, scale)?;
        let flatten = scene.decoration().spotlight.active
            || scene.decoration().watermark.text_len > 0
            || scene.items().iter().any(|item| {
                matches!(
                    item,
                    SceneDisplayItem::Filter(_) | SceneDisplayItem::Magnifier(_)
                )
            });
        if !flatten {
            self.vectors.set_clear_color(frame.clear_color);
            let mut svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}"><rect width="100%" height="100%" fill="{}" fill-opacity="{}"/>"#,
                width,
                height,
                width,
                height,
                color(frame.clear_color),
                f64::from(frame.clear_color.a) / 255.0
            );
            let matrix = viewport_transform(frame, scale);
            append_svg(
                &mut svg,
                &format!(
                    r#"<g transform="matrix({} {} {} {} {} {})">"#,
                    matrix.sx, matrix.ky, matrix.kx, matrix.sy, matrix.tx, matrix.ty
                ),
                limit,
            )?;
            if let Some(source) = background {
                let mut bytes = crate::export::BoundedOutput::default();
                {
                    use std::io::Write as _;
                    let mut png = png::Encoder::new(
                        &mut bytes,
                        source.pixels().width(),
                        source.pixels().height(),
                    );
                    png.set_color(png::ColorType::Rgba);
                    png.set_depth(png::BitDepth::Eight);
                    let mut png = png
                        .write_header()
                        .map_err(|e| CanvasError::InvalidFile(e.to_string()))?;
                    let mut stream = png
                        .stream_writer_with_size(64 * 1024)
                        .map_err(|e| CanvasError::InvalidFile(e.to_string()))?;
                    stream
                        .write_all(source.pixels().as_raw())
                        .map_err(|e| CanvasError::InvalidFile(e.to_string()))?;
                    stream
                        .finish()
                        .map_err(|e| CanvasError::InvalidFile(e.to_string()))?;
                    png.finish()
                        .map_err(|e| CanvasError::InvalidFile(e.to_string()))?;
                }
                if base64::encoded_len(bytes.bytes.len(), true)
                    .is_none_or(|len| len.saturating_add(svg.len()).saturating_add(1024) > limit)
                {
                    return Err(CanvasError::ResourceLimit);
                }
                let encoded = base64::engine::general_purpose::STANDARD.encode(bytes.bytes);
                append_svg(
                    &mut svg,
                    &format!(
                        r#"<image x="{}" y="{}" width="{}" height="{}" href="data:image/png;base64,{encoded}"/>"#,
                        source.origin().x,
                        source.origin().y,
                        f64::from(source.pixels().width()) * source.units_per_pixel(),
                        f64::from(source.pixels().height()) * source.units_per_pixel()
                    ),
                    limit,
                )?;
            }
            for (index, item) in scene.items().iter().enumerate() {
                let tree = self.vectors.item(item)?;
                let portable = tree.to_string(&resvg::usvg::WriteOptions {
                    id_prefix: Some(format!("item-{index}-")),
                    preserve_text: false,
                    ..Default::default()
                });
                if let Some((_, body)) = portable.split_once('>')
                    && let Some((body, _)) = body.rsplit_once("</svg>")
                {
                    append_svg(&mut svg, body, limit)?;
                }
            }
            append_svg(&mut svg, "</g></svg>", limit)?;
            return Ok(svg);
        }
        let bytes = self.export_png(scene, background, resource_revision, scale)?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}"><image width="100%" height="100%" href="data:image/png;base64,{encoded}"/></svg>"#,
            width, height
        );
        if svg.len() > limit {
            return Err(CanvasError::ResourceLimit);
        }
        Ok(svg)
    }

    fn filter_mask(
        &self,
        filter: &FilterDisplayItem,
        transform: Transform,
        width: u32,
        height: u32,
    ) -> Result<Pixmap, CanvasError> {
        let mut mask = Pixmap::new(width, height).ok_or(CanvasError::ResourceLimit)?;
        let content = if filter.is_pen_filter && filter.points.len() == 1 {
            format!(
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"white\" opacity=\"{}\"/>",
                filter.points[0][0],
                filter.points[0][1],
                filter.stroke_width / 2.0,
                filter.opacity
            )
        } else if filter.is_pen_filter {
            let mut path = String::new();
            for (index, [x, y]) in filter.points.iter().enumerate() {
                write!(path, "{}{x} {y} ", if index == 0 { "M" } else { "L" }).unwrap();
            }
            format!(
                "<path d=\"{path}\" fill=\"none\" stroke=\"white\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\" opacity=\"{}\"/>",
                filter.stroke_width, filter.opacity
            )
        } else {
            format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" transform=\"rotate({} {} {})\" fill=\"white\" opacity=\"{}\"/>",
                filter.center_x - filter.width / 2.0,
                filter.center_y - filter.height / 2.0,
                filter.width,
                filter.height,
                filter.rotation.to_degrees(),
                filter.center_x,
                filter.center_y,
                filter.opacity
            )
        };
        resvg::render(
            &self.vectors.parse(&content)?,
            transform,
            &mut mask.as_mut(),
        );
        Ok(mask)
    }

    fn spotlight(
        &self,
        scene: &RetainedScene,
        transform: Transform,
        target: &mut Pixmap,
    ) -> Result<(), CanvasError> {
        let spotlight = scene.decoration().spotlight;
        if !spotlight.active {
            return Ok(());
        }
        let mut mask =
            Pixmap::new(target.width(), target.height()).ok_or(CanvasError::ResourceLimit)?;
        for cutout in scene.spotlights() {
            let rectangle = RectangleDisplayItem {
                width: cutout.width,
                height: cutout.height,
                shape: cutout.shape,
                corner_radii: cutout.corner_radii,
                ..Default::default()
            };
            let content = format!(
                "<path d=\"{}\" transform=\"translate({} {}) rotate({})\" fill=\"white\"/>",
                rectangle_path(&rectangle),
                cutout.center_x,
                cutout.center_y,
                cutout.rotation.to_degrees()
            );
            resvg::render(
                &self.vectors.parse(&content)?,
                transform,
                &mut mask.as_mut(),
            );
        }
        let c = spotlight.color;
        for (pixel, mask) in target.pixels_mut().iter_mut().zip(mask.pixels()) {
            let amount =
                (1.0 - f64::from(mask.alpha()) / 255.0) * spotlight.opacity * f64::from(c.a)
                    / 255.0;
            *pixel = over(*pixel, [c.r, c.g, c.b, 255], amount);
        }
        Ok(())
    }

    fn watermark(
        &mut self,
        scene: &RetainedScene,
        transform: Transform,
        target: &mut Pixmap,
    ) -> Result<(), CanvasError> {
        let watermark = scene.decoration().watermark;
        let text = String::from_utf8_lossy(&watermark.text[..watermark.text_len as usize]);
        if text.is_empty() || watermark.opacity <= 0.0 {
            return Ok(());
        }
        let frame = scene.frame();
        let (left, top, right, bottom) = canvas_viewport(frame.camera, frame.surface);
        let gap = (watermark.gap + watermark.font_size * 4.0).max(32.0);
        let mut content = String::new();
        let family =
            String::from_utf8_lossy(&watermark.font_family[..watermark.font_family_len as usize]);
        let mut y = (top / gap).floor() * gap;
        let mut count = 0;
        while y < bottom + gap {
            let mut x = (left / gap).floor() * gap;
            while x < right + gap {
                write!(content, "<text x=\"{x}\" y=\"{y}\" transform=\"rotate({} {x} {y})\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\" opacity=\"{}\">{}</text>", watermark.angle, crate::vector::xml(&family), watermark.font_size, color(watermark.color), watermark.opacity * f64::from(watermark.color.a)/255.0, crate::vector::xml(&text)).unwrap();
                count += 1;
                if count > 4096 {
                    return Err(CanvasError::ResourceLimit);
                }
                x += gap;
            }
            y += gap;
        }
        resvg::render(
            &self.vectors.parse(&content)?,
            transform,
            &mut target.as_mut(),
        );
        Ok(())
    }

    fn magnifier(
        &mut self,
        lens: &MagnifierDisplayItem,
        background: Option<&SourceImage>,
        frame: FrameView,
        scale: f32,
        target: &mut Pixmap,
    ) -> Result<(), CanvasError> {
        let mut mask =
            Pixmap::new(target.width(), target.height()).ok_or(CanvasError::ResourceLimit)?;
        let body = &lens.lens;
        let (mask_x, mask_y, mask_right, mask_bottom) =
            canvas_viewport(frame.camera, frame.surface);
        let mask_width = mask_right - mask_x;
        let mask_height = mask_bottom - mask_y;
        let source_body = RectangleDisplayItem {
            center_x: lens.source_center.x,
            center_y: lens.source_center.y,
            width: lens.source_width,
            height: lens.source_height,
            corner_radii: body.corner_radii.scaled(1.0 / lens.factor),
            fill: ColorRgba8::default(),
            ..*body
        };
        let lens_path = format!(
            "<path d=\"{}\" transform=\"translate({} {}) rotate({})\" fill=\"black\"/>",
            rectangle_path(body),
            body.center_x,
            body.center_y,
            body.rotation.to_degrees()
        );
        let source_path = format!(
            "<path d=\"{}\" transform=\"translate({} {}) rotate({})\" fill=\"black\"/>",
            rectangle_path(&source_body),
            source_body.center_x,
            source_body.center_y,
            source_body.rotation.to_degrees()
        );
        if lens.connection_type == MagnifierConnectionType::Gradient && body.stroke.a > 0 {
            let mut points = self.contour_points(body)?;
            points.extend(self.contour_points(&source_body)?);
            let hull = convex_hull(points);
            let mut path = String::new();
            for (index, [x, y]) in hull.iter().enumerate() {
                write!(path, "{}{x} {y} ", if index == 0 { "M" } else { "L" }).unwrap();
            }
            path.push('Z');
            let c = color(body.stroke);
            let opacity = body.opacity * f64::from(body.stroke.a) / 255.0;
            let content = format!(
                r#"<defs><linearGradient id="bridge" gradientUnits="userSpaceOnUse" x1="{}" y1="{}" x2="{}" y2="{}"><stop offset="0" stop-color="{c}" stop-opacity="0"/><stop offset="1" stop-color="{c}" stop-opacity="0.3"/></linearGradient><mask id="outside" maskUnits="userSpaceOnUse" x="{mask_x}" y="{mask_y}" width="{mask_width}" height="{mask_height}"><path d="{path}" fill="white"/>{lens_path}{source_path}</mask></defs><path d="{path}" fill="url(#bridge)" mask="url(#outside)" opacity="{opacity}"/>"#,
                lens.source_center.x, lens.source_center.y, body.center_x, body.center_y
            );
            resvg::render(
                &self.vectors.parse(&content)?,
                viewport_transform(frame, scale),
                &mut target.as_mut(),
            );
        }
        if lens.source_outline && body.stroke.a > 0 {
            let outline = self
                .vectors
                .content(&SceneDisplayItem::Rectangle(source_body))?;
            let content = format!(
                r#"<defs><mask id="outside-lens" maskUnits="userSpaceOnUse" x="{mask_x}" y="{mask_y}" width="{mask_width}" height="{mask_height}"><rect x="{mask_x}" y="{mask_y}" width="{mask_width}" height="{mask_height}" fill="white"/>{lens_path}</mask></defs><g mask="url(#outside-lens)">{outline}</g>"#
            );
            resvg::render(
                &self.vectors.parse(&content)?,
                viewport_transform(frame, scale),
                &mut target.as_mut(),
            );
        }
        let content = format!(
            "<path d=\"{}\" transform=\"translate({} {}) rotate({})\" fill=\"white\" opacity=\"{}\"/>",
            rectangle_path(body),
            body.center_x,
            body.center_y,
            body.rotation.to_degrees(),
            body.opacity
        );
        resvg::render(
            &self.vectors.parse(&content)?,
            viewport_transform(frame, scale),
            &mut mask.as_mut(),
        );
        let mut sample = RgbaImage::new(target.width(), target.height());
        for (x, y, pixel) in sample.enumerate_pixels_mut() {
            let world = view_to_canvas(
                Point::new(
                    (f64::from(x) + 0.5) / f64::from(scale),
                    (f64::from(y) + 0.5) / f64::from(scale),
                ),
                &frame.camera,
                frame.surface,
            );
            let dx = world.x - body.center_x;
            let dy = world.y - body.center_y;
            let source = Point::new(
                lens.source_center.x + dx / lens.factor,
                lens.source_center.y + dy / lens.factor,
            );
            *pixel = background
                .and_then(|background| sample_source(background, source))
                .unwrap_or(Rgba([0, 0, 0, 0]));
        }
        let alpha: Vec<_> = mask.pixels().iter().map(|pixel| pixel.alpha()).collect();
        for ((destination, source), alpha) in target
            .pixels_mut()
            .iter_mut()
            .zip(sample.pixels())
            .zip(alpha)
        {
            if alpha > 0 {
                *destination = over(*destination, source.0, f64::from(alpha) / 255.0);
            }
        }
        if let Some(leader) = &lens.leader {
            resvg::render(
                self.vectors
                    .item(&SceneDisplayItem::Arrow(leader.clone()))?
                    .as_ref(),
                viewport_transform(frame, scale),
                &mut target.as_mut(),
            );
        }
        let outline = SceneDisplayItem::Rectangle(RectangleDisplayItem {
            fill: ColorRgba8::default(),
            ..*body
        });
        resvg::render(
            self.vectors.item(&outline)?.as_ref(),
            viewport_transform(frame, scale),
            &mut target.as_mut(),
        );
        Ok(())
    }

    fn contour_points(
        &self,
        rectangle: &RectangleDisplayItem,
    ) -> Result<Vec<[f64; 2]>, CanvasError> {
        use resvg::tiny_skia::PathSegment;
        let content = format!("<path d=\"{}\"/>", rectangle_path(rectangle));
        let tree = self.vectors.parse(&content)?;
        let mut points = Vec::new();
        if let Some(resvg::usvg::Node::Path(path)) = tree.root().children().first() {
            let (sin, cos) = rectangle.rotation.sin_cos();
            let transform = Transform::from_row(
                cos as f32,
                sin as f32,
                -sin as f32,
                cos as f32,
                rectangle.center_x as f32,
                rectangle.center_y as f32,
            );
            let mut current = resvg::tiny_skia::Point::from_xy(0.0, 0.0);
            for segment in path.data().segments() {
                match segment {
                    PathSegment::MoveTo(point) | PathSegment::LineTo(point) => {
                        let mut mapped = point;
                        transform.map_point(&mut mapped);
                        points.push([f64::from(mapped.x), f64::from(mapped.y)]);
                        current = point;
                    }
                    PathSegment::QuadTo(control, end) => {
                        for step in 1..=12 {
                            let t = step as f32 / 12.0;
                            let inverse = 1.0 - t;
                            let mut point = resvg::tiny_skia::Point::from_xy(
                                inverse * inverse * current.x
                                    + 2.0 * inverse * t * control.x
                                    + t * t * end.x,
                                inverse * inverse * current.y
                                    + 2.0 * inverse * t * control.y
                                    + t * t * end.y,
                            );
                            transform.map_point(&mut point);
                            points.push([f64::from(point.x), f64::from(point.y)]);
                        }
                        current = end;
                    }
                    PathSegment::CubicTo(a, b, end) => {
                        for step in 1..=16 {
                            let t = step as f32 / 16.0;
                            let inverse = 1.0 - t;
                            let mut point = resvg::tiny_skia::Point::from_xy(
                                inverse.powi(3) * current.x
                                    + 3.0 * inverse * inverse * t * a.x
                                    + 3.0 * inverse * t * t * b.x
                                    + t.powi(3) * end.x,
                                inverse.powi(3) * current.y
                                    + 3.0 * inverse * inverse * t * a.y
                                    + 3.0 * inverse * t * t * b.y
                                    + t.powi(3) * end.y,
                            );
                            transform.map_point(&mut point);
                            points.push([f64::from(point.x), f64::from(point.y)]);
                        }
                        current = end;
                    }
                    PathSegment::Close => {}
                }
            }
        }
        Ok(points)
    }
}

fn convex_hull(mut points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points.dedup();
    let cross = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let mut lower = Vec::new();
    for point in &points {
        while lower.len() >= 2
            && cross(lower[lower.len() - 2], lower[lower.len() - 1], *point) <= 0.0
        {
            lower.pop();
        }
        lower.push(*point);
    }
    let boundary = lower.len();
    for point in points.iter().rev().skip(1) {
        while lower.len() > boundary
            && cross(lower[lower.len() - 2], lower[lower.len() - 1], *point) <= 0.0
        {
            lower.pop();
        }
        lower.push(*point);
    }
    lower.pop();
    lower
}

fn append_svg(svg: &mut String, part: &str, limit: usize) -> Result<(), CanvasError> {
    if svg.len().saturating_add(part.len()) > limit {
        return Err(CanvasError::ResourceLimit);
    }
    svg.push_str(part);
    Ok(())
}
pub(crate) fn viewport_transform(frame: FrameView, scale: f32) -> Transform {
    let zoom = frame.camera.zoom as f32 * scale;
    Transform::from_row(
        zoom,
        0.0,
        0.0,
        zoom,
        frame.surface.width as f32 * scale / 2.0 - frame.camera.center.x as f32 * zoom,
        frame.surface.height as f32 * scale / 2.0 - frame.camera.center.y as f32 * zoom,
    )
}

fn restore_outside(
    target: &mut Pixmap,
    before: Option<&Pixmap>,
    area: Option<crate::CanvasRect>,
    frame: FrameView,
    scale: f32,
) {
    let (Some(before), Some(area)) = (before, area) else {
        return;
    };
    let width = target.width() as usize;
    for (i, pixel) in target.pixels_mut().iter_mut().enumerate() {
        let world = view_to_canvas(
            Point::new(
                (i % width) as f64 / f64::from(scale) + 0.5 / f64::from(scale),
                (i / width) as f64 / f64::from(scale) + 0.5 / f64::from(scale),
            ),
            &frame.camera,
            frame.surface,
        );
        if !area.contains(world) {
            *pixel = before.pixels()[i];
        }
    }
}

fn paint_host_source(
    target: &mut Pixmap,
    source: Option<&SourceImage>,
    frame: FrameView,
    scale: f32,
    cancel: &crate::smart_erase::Cancellation,
) -> Result<(), CanvasError> {
    let Some(source) = source else {
        return Ok(());
    };
    let width = target.width() as usize;
    for y in 0..target.height() {
        cancel.check()?;
        for x in 0..target.width() {
            let world = view_to_canvas(
                Point::new(
                    (f64::from(x) + 0.5) / f64::from(scale),
                    (f64::from(y) + 0.5) / f64::from(scale),
                ),
                &frame.camera,
                frame.surface,
            );
            if let Some(pixel) = sample_source(source, world) {
                let i = y as usize * width + x as usize;
                target.pixels_mut()[i] = over(target.pixels()[i], pixel.0, 1.0);
            }
        }
    }
    Ok(())
}

fn sample_background(
    source: Option<&SourceImage>,
    world: Point<f64>,
    clear: ColorRgba8,
) -> Rgba<u8> {
    if let Some(pixel) = source.and_then(|source| sample_source(source, world)) {
        return pixel;
    }
    Rgba([clear.r, clear.g, clear.b, clear.a])
}

pub(crate) fn filter_canvas_bounds(filter: &FilterDisplayItem) -> (f64, f64, f64, f64) {
    if filter.is_pen_filter && !filter.points.is_empty() {
        let radius = filter.stroke_width / 2.0 + 2.0;
        let mut bounds = (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        for [x, y] in filter.points.iter().copied() {
            bounds.0 = bounds.0.min(x - radius);
            bounds.1 = bounds.1.min(y - radius);
            bounds.2 = bounds.2.max(x + radius);
            bounds.3 = bounds.3.max(y + radius);
        }
        bounds
    } else {
        let (sin, cos) = filter.rotation.sin_cos();
        let width = cos.abs() * filter.width / 2.0 + sin.abs() * filter.height / 2.0 + 2.0;
        let height = sin.abs() * filter.width / 2.0 + cos.abs() * filter.height / 2.0 + 2.0;
        (
            filter.center_x - width,
            filter.center_y - height,
            filter.center_x + width,
            filter.center_y + height,
        )
    }
}

pub(crate) fn sample_source(source: &SourceImage, world: Point<f64>) -> Option<Rgba<u8>> {
    let x = (world.x - source.origin().x) / source.units_per_pixel();
    let y = (world.y - source.origin().y) / source.units_per_pixel();
    if x < 0.0
        || y < 0.0
        || x >= f64::from(source.pixels().width())
        || y >= f64::from(source.pixels().height())
    {
        return None;
    }
    // Stable16-bit fractional texel coordinates prevent a translated export tile
    // from crossing an8-bit rounding tie through floating-point reassociation.
    // The quantization error is below1/65536 source pixel, including at alpha edges.
    let fx = ((x - 0.5) * 65536.0).round() / 65536.0;
    let fy = ((y - 0.5) * 65536.0).round() / 65536.0;
    let x0 = fx.floor();
    let y0 = fy.floor();
    let tx = fx - x0;
    let ty = fy - y0;
    let mut sums = [0.0; 4];
    for (dx, dy, weight) in [
        (0.0, 0.0, (1.0 - tx) * (1.0 - ty)),
        (1.0, 0.0, tx * (1.0 - ty)),
        (0.0, 1.0, (1.0 - tx) * ty),
        (1.0, 1.0, tx * ty),
    ] {
        let pixel = source.pixels().get_pixel(
            (x0 + dx).clamp(0.0, f64::from(source.pixels().width() - 1)) as u32,
            (y0 + dy).clamp(0.0, f64::from(source.pixels().height() - 1)) as u32,
        );
        for channel in 0..3 {
            sums[channel] += f64::from(pixel[channel]) * f64::from(pixel[3]) / 255.0 * weight;
        }
        sums[3] += f64::from(pixel[3]) * weight;
    }
    let alpha = sums[3].round().clamp(0.0, 255.0) as u8;
    Some(Rgba([
        if alpha > 0 {
            (sums[0] * 255.0 / sums[3]).round().clamp(0.0, 255.0) as u8
        } else {
            0
        },
        if alpha > 0 {
            (sums[1] * 255.0 / sums[3]).round().clamp(0.0, 255.0) as u8
        } else {
            0
        },
        if alpha > 0 {
            (sums[2] * 255.0 / sums[3]).round().clamp(0.0, 255.0) as u8
        } else {
            0
        },
        alpha,
    ]))
}

#[allow(clippy::too_many_arguments)]
fn background_region(
    frame: FrameView,
    source: Option<&SourceImage>,
    width: u32,
    height: u32,
    scale: f32,
    rect: [u32; 4],
    cancel: &crate::CanvasCancellation,
) -> Result<RgbaImage, CanvasError> {
    let mut pixels = RgbaImage::new(width, height);
    let [left, top, w, h] = rect;
    for y in top..top + h {
        cancel.check()?;
        for x in left..left + w {
            let world = view_to_canvas(
                Point::new(
                    (f64::from(x) + 0.5) / f64::from(scale),
                    (f64::from(y) + 0.5) / f64::from(scale),
                ),
                &frame.camera,
                frame.surface,
            );
            pixels.put_pixel(x, y, sample_background(source, world, frame.clear_color));
        }
    }
    Ok(pixels)
}
fn background_surface(
    frame: FrameView,
    source: Option<&SourceImage>,
    width: u32,
    height: u32,
    scale: f32,
) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        let world = view_to_canvas(
            Point::new(
                (f64::from(x) + 0.5) / f64::from(scale),
                (f64::from(y) + 0.5) / f64::from(scale),
            ),
            &frame.camera,
            frame.surface,
        );
        sample_background(source, world, frame.clear_color)
    })
}

fn image_to_pixmap(image: &RgbaImage) -> Result<Pixmap, CanvasError> {
    let mut map = Pixmap::new(image.width(), image.height()).ok_or(CanvasError::ResourceLimit)?;
    for (destination, source) in map.pixels_mut().iter_mut().zip(image.pixels()) {
        *destination =
            resvg::tiny_skia::ColorU8::from_rgba(source[0], source[1], source[2], source[3])
                .premultiply();
    }
    Ok(map)
}

fn pixmap_to_image(map: &Pixmap) -> RgbaImage {
    let mut output = RgbaImage::new(map.width(), map.height());
    for (destination, source) in output.pixels_mut().zip(map.pixels()) {
        let source = source.demultiply();
        *destination = Rgba([source.red(), source.green(), source.blue(), source.alpha()]);
    }
    output
}

fn over(
    destination: resvg::tiny_skia::PremultipliedColorU8,
    source: [u8; 4],
    amount: f64,
) -> resvg::tiny_skia::PremultipliedColorU8 {
    let alpha = (amount.clamp(0.0, 1.0) * f64::from(source[3]) / 255.0) as f32;
    let color = resvg::tiny_skia::Color::from_rgba(
        f32::from(source[0]) / 255.0,
        f32::from(source[1]) / 255.0,
        f32::from(source[2]) / 255.0,
        alpha,
    )
    .unwrap()
    .to_color_u8()
    .premultiply();
    let inverse = 1.0 - f64::from(color.alpha()) / 255.0;
    resvg::tiny_skia::PremultipliedColorU8::from_rgba(
        (f64::from(color.red()) + f64::from(destination.red()) * inverse).round() as u8,
        (f64::from(color.green()) + f64::from(destination.green()) * inverse).round() as u8,
        (f64::from(color.blue()) + f64::from(destination.blue()) * inverse).round() as u8,
        (f64::from(color.alpha()) + f64::from(destination.alpha()) * inverse).round() as u8,
    )
    .unwrap()
}

fn replace_masked(target: &mut Pixmap, source: &RgbaImage, mask: &[u8]) {
    replace_masked_region(target, source, mask, 0, 0);
}

fn mask_bounds(mask: &[u8], width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    let mut left = width;
    let mut top = height;
    let mut right = 0;
    let mut bottom = 0;
    for (i, alpha) in mask.iter().enumerate() {
        if *alpha > 0 {
            let x = (i % width as usize) as u32;
            let y = (i / width as usize) as u32;
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + 1);
            bottom = bottom.max(y + 1);
        }
    }
    (left < right && top < bottom).then_some((left, top, right, bottom))
}

fn replace_masked_region(target: &mut Pixmap, source: &RgbaImage, mask: &[u8], x: u32, y: u32) {
    let stride = target.width() as usize;
    for (sx, sy, source) in source.enumerate_pixels() {
        let i = (sy + y) as usize * stride + (sx + x) as usize;
        let destination = &mut target.pixels_mut()[i];
        let mask = mask[i];
        let source =
            resvg::tiny_skia::ColorU8::from_rgba(source[0], source[1], source[2], source[3])
                .premultiply();
        let blend = |a: u8, b: u8| {
            ((u32::from(a) * (255 - u32::from(mask)) + u32::from(b) * u32::from(mask) + 127) / 255)
                as u8
        };
        *destination = resvg::tiny_skia::PremultipliedColorU8::from_rgba(
            blend(destination.red(), source.red()),
            blend(destination.green(), source.green()),
            blend(destination.blue(), source.blue()),
            blend(destination.alpha(), source.alpha()),
        )
        .unwrap();
    }
}

fn effect(
    source: &RgbaImage,
    pristine: &RgbaImage,
    filter: &FilterRenderSpec,
    frame: FrameView,
    scale: f32,
) -> Result<RgbaImage, CanvasError> {
    let mut output = source.clone();
    match filter.filter_type {
        DisplayFilterType::GaussianBlur => {
            let mut premultiplied = source.clone();
            for pixel in premultiplied.pixels_mut() {
                for channel in 0..3 {
                    pixel[channel] =
                        ((u32::from(pixel[channel]) * u32::from(pixel[3]) + 127) / 255) as u8;
                }
            }
            output = image::imageops::blur(
                &premultiplied,
                (filter.blur_sigma * frame.camera.zoom * f64::from(scale)).max(0.01) as f32,
            );
            for pixel in output.pixels_mut() {
                if pixel[3] > 0 {
                    for channel in 0..3 {
                        pixel[channel] = ((u32::from(pixel[channel]) * 255
                            + u32::from(pixel[3]) / 2)
                            / u32::from(pixel[3]))
                        .min(255) as u8;
                    }
                }
            }
        }
        DisplayFilterType::Mosaic => {
            let block = filter.mosaic_block_size;
            for (x, y, pixel) in output.enumerate_pixels_mut() {
                let world = view_to_canvas(
                    Point::new(
                        f64::from(x) / f64::from(scale),
                        f64::from(y) / f64::from(scale),
                    ),
                    &frame.camera,
                    frame.surface,
                );
                let center = Point::new(
                    ((world.x / block).floor() + 0.5) * block,
                    ((world.y / block).floor() + 0.5) * block,
                );
                let center = canvas_to_view(center, &frame.camera, frame.surface);
                let sample_x = (center.x * f64::from(scale))
                    .floor()
                    .clamp(0.0, f64::from(source.width() - 1))
                    as u32;
                let sample_y = (center.y * f64::from(scale))
                    .floor()
                    .clamp(0.0, f64::from(source.height() - 1))
                    as u32;
                *pixel = *source.get_pixel(sample_x, sample_y);
            }
        }
        DisplayFilterType::Grayscale => {
            for pixel in output.pixels_mut() {
                let gray = (0.2126 * f64::from(pixel[0])
                    + 0.7152 * f64::from(pixel[1])
                    + 0.0722 * f64::from(pixel[2]))
                .round() as u8;
                pixel.0[..3].fill(gray);
            }
        }
        DisplayFilterType::Inversion => {
            for pixel in output.pixels_mut() {
                for channel in &mut pixel.0[..3] {
                    *channel = 255 - *channel;
                }
            }
        }
        DisplayFilterType::Brightness => {
            for pixel in output.pixels_mut() {
                for channel in &mut pixel.0[..3] {
                    let original = f64::from(*channel);
                    *channel = (if filter.strength <= 0.5 {
                        original * filter.strength * 2.0
                    } else {
                        original + (255.0 - original) * (filter.strength * 2.0 - 1.0)
                    })
                    .clamp(0.0, 255.0)
                    .round() as u8;
                }
            }
        }
        DisplayFilterType::Emboss => {
            let radius = (frame.camera.zoom * f64::from(scale)).round().max(1.0) as u32;
            for (x, y, pixel) in output.enumerate_pixels_mut() {
                let before = source.get_pixel(x.saturating_sub(radius), y.saturating_sub(radius));
                let after = source.get_pixel(
                    (x + radius).min(source.width() - 1),
                    (y + radius).min(source.height() - 1),
                );
                let delta = (0..3)
                    .map(|channel| {
                        f64::from(after[channel]) * f64::from(after[3]) / 255.0
                            - f64::from(before[channel]) * f64::from(before[3]) / 255.0
                    })
                    .sum::<f64>();
                let gray = (127.5 + filter.strength * 10.0 * delta / 3.0)
                    .clamp(0.0, 255.0)
                    .round() as u8;
                pixel.0[..3].fill(gray);
            }
        }
        DisplayFilterType::RestoreBackground => output = pristine.clone(),
        DisplayFilterType::SmartErase => {
            return Err(CanvasError::Unsupported(
                "Smart Erase without a reconstruction mask",
            ));
        }
    }
    Ok(output)
}
