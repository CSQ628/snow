mod element;
mod host_controls;
pub use host_controls::{
    CanvasCommandKeyResolver, CanvasCrosshair, CanvasCrosshairFactory, CanvasCursorLayer,
};
#[cfg(test)]
mod tests;
mod text_input;

use std::sync::{Arc, Mutex};

use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, Corners, CursorStyle, Element, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, KeyDownEvent, KeyUpEvent,
    LayoutId, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder,
    Pixels, Point, RenderImage, ScrollDelta, ScrollWheelEvent, SharedString, Style, Subscription,
    Task, UTF16Selection, Window, div, fill, point, prelude::*, px, relative, rgb, rgba,
};
use snow_draw_engine as draw;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    CanvasDocument, CanvasError, CanvasRenderer, CanvasSession, SourceImage,
    text_buffer::TextBuffer,
    vector::{TextLayout, VectorCache},
};

/// Semantic notifications for a containing toolbar or future Snow Shot shell.
pub enum CanvasEvent {
    Changed,
    Error(SharedString),
    Rendered,
}

struct TextDraft {
    buffer: TextBuffer,
    target: draw::TextCommitTarget,
    center: draw::Point<f64>,
    rotation: f64,
    style: draw::TextStyle,
    width: f64,
    auto_resize: bool,
    layout: TextLayout,
    changed: bool,
    minimum_grapheme_width: f64,
    ink_reserve: f64,
    preferred_x: Option<f64>,
    caret_affinity: Option<(usize, usize, f64)>,
}

impl Drop for CanvasView {
    fn drop(&mut self) {
        self.render_cancel.cancel();
    }
}

/// Retained native canvas. Domain state, transient viewport state and render caches have
/// distinct owners. Rendering is demand-driven with at most one worker and one latest request.
pub struct CanvasView {
    command_key_resolver: Option<CanvasCommandKeyResolver>,
    cursor_layers: [Option<CursorStyle>; 2],
    crosshair_factory: Option<CanvasCrosshairFactory>,
    crosshair_cache: Option<(u32, Option<CanvasCrosshair>)>,
    show_dirty_regions: bool,
    clear_renderer_pending: bool,
    options: crate::CanvasViewOptions,
    text_style_popup_depth: u32,
    document: CanvasDocument,
    session: CanvasSession,
    reference_session: CanvasSession,
    reference_valid: bool,
    focus: FocusHandle,
    bounds: Bounds<Pixels>,
    scale: f32,
    image: Option<Arc<RenderImage>>,
    image_frame: Option<draw::FrameView>,
    renderer: Option<Arc<Mutex<CanvasRenderer>>>,
    metrics: Option<VectorCache>,
    render_task: Option<Task<()>>,
    clipboard_task: Option<Task<()>>,
    render_pending: bool,
    render_cancel: crate::smart_erase::Cancellation,
    generation: u64,
    rendered_generation: u64,
    requested_key: Option<(
        draw::SceneRevision,
        draw::DecorationRevision,
        draw::FrameView,
        u64,
        u32,
    )>,
    draft: Option<TextDraft>,
    selecting_text: bool,
    pan: Option<(Point<Pixels>, draw::Camera)>,
    space_down: bool,
    space_key: Option<String>,
    cursor: CursorStyle,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CanvasEvent> for CanvasView {}
impl Focusable for CanvasView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl CanvasView {
    pub fn new(
        mut document: CanvasDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Self, CanvasError> {
        let session = CanvasSession::new(&mut document)?;
        let reference_session = CanvasSession::new(&mut document)?;
        let focus = cx.focus_handle().tab_stop(true);
        let subscriptions = vec![
            cx.on_release_in(window, |this, window, cx| {
                this.render_cancel.cancel();
                if let Some(image) = this.image.take() {
                    let _ = window.drop_image(image);
                }
                if this.document.engine().has_pointer_capture() {
                    window.release_pointer();
                }
                crate::cleanup::retire_send_resources(
                    (
                        this.renderer.take(),
                        this.metrics.take(),
                        this.draft.take().map(|draft| draft.layout),
                    ),
                    cx,
                );
                crate::retire_canvas_document(std::mem::take(&mut this.document), cx);
            }),
            cx.on_blur(&focus, window, |this, window, cx| {
                if this.text_style_popup_depth == 0 {
                    this.finish_text(true, window, cx);
                }
                this.cancel_gesture(window, cx);
            }),
            cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.cancel_gesture(window, cx);
                }
            }),
        ];
        Ok(Self {
            command_key_resolver: None,
            cursor_layers: [None, None],
            crosshair_factory: None,
            crosshair_cache: None,
            show_dirty_regions: false,
            clear_renderer_pending: false,
            options: Default::default(),
            text_style_popup_depth: 0,
            document,
            session,
            reference_session,
            reference_valid: false,
            focus,
            bounds: Bounds::default(),
            scale: window.scale_factor(),
            image: None,
            image_frame: None,
            renderer: Some(Arc::new(Mutex::new(CanvasRenderer::default()))),
            metrics: Some(VectorCache::default()),
            render_task: None,
            clipboard_task: None,
            render_pending: false,
            render_cancel: Default::default(),
            generation: 0,
            rendered_generation: 0,
            requested_key: None,
            draft: None,
            selecting_text: false,
            pan: None,
            space_down: false,
            space_key: None,
            cursor: CursorStyle::Crosshair,
            error: None,
            _subscriptions: subscriptions,
        })
    }

    pub fn document(&self) -> &CanvasDocument {
        &self.document
    }

    pub fn options(&self) -> &crate::CanvasViewOptions {
        &self.options
    }
    pub fn set_options(
        &mut self,
        options: crate::CanvasViewOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), CanvasError> {
        options.presentation.validate()?;
        if let Some(area) = options.text_editing_bounds {
            area.validate()?;
        }
        if let Some(reference) = options.filter_render_reference {
            reference.validate()?;
        }
        if !options.interaction_enabled {
            self.finish_text(true, window, cx);
            self.cancel_gesture(window, cx);
        }
        self.options = options;
        self.invalidate_render();
        if self.draft.is_some() {
            self.update_draft(window, cx);
        } else {
            self.changed(window, cx);
        }
        Ok(())
    }
    pub fn begin_text_style_popup_interaction(&mut self) {
        self.text_style_popup_depth = self.text_style_popup_depth.saturating_add(1);
    }
    pub fn end_text_style_popup_interaction(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.text_style_popup_depth = self.text_style_popup_depth.saturating_sub(1);
        if self.text_style_popup_depth == 0 && self.draft.is_some() {
            self.focus.focus(window, cx);
        }
    }
    pub fn set_active_text_style(
        &mut self,
        style: draw::TextStyle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), CanvasError> {
        self.document.command(|engine| {
            engine.set_viewport_text_creation_style(
                self.session.viewport(),
                style.clone(),
                snow_draw_engine_editor::TEXT_STYLE_ALL_PROPERTIES,
            )
        })?;
        if let Some(draft) = &mut self.draft {
            draft.changed |= draft.style != style;
            draft.style = style;
            self.update_draft(window, cx);
        } else {
            self.changed(window, cx);
        }
        Ok(())
    }
    pub fn session(&self) -> &CanvasSession {
        &self.session
    }
    /// Latest logical content bounds, useful to coordinate a containing native host.
    pub fn surface_bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }
    pub fn snapshot(&self) -> Result<crate::CanvasSnapshot, CanvasError> {
        if !self.reference_valid {
            return Err(draw::ErrorCode::InvalidState.into());
        }
        crate::CanvasSnapshot::from_reference(
            &self.document,
            &self.reference_session,
            self.session.scene().frame(),
        )
        .with_presentation(self.options.presentation.clone())
    }
    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }
    pub fn is_rendering(&self) -> bool {
        self.render_task.is_some()
            || (self.reference_valid && self.rendered_generation != self.generation)
    }
    #[doc(hidden)]
    pub fn render_diagnostics(&self) -> String {
        format!(
            "generation={} rendered={} worker={} pending={} bounds={:?} reference={:?} error={:?}",
            self.generation,
            self.rendered_generation,
            self.render_task.is_some(),
            self.render_pending,
            self.bounds,
            self.reference_session.scene().frame(),
            self.error
        )
    }
    pub fn has_text_draft(&self) -> bool {
        self.draft.is_some()
    }

    /// Copies a strict, overlay-free PNG of the current viewport asynchronously.
    pub fn copy_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.clipboard_task.is_some() {
            return;
        }
        self.finish_text(true, window, cx);
        self.cancel_gesture(window, cx);
        let snapshot = match self.snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.fail(error, cx);
                return;
            }
        };
        let scale = window.scale_factor();
        let renderer = self.renderer.as_ref().unwrap().clone();
        self.clipboard_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    renderer
                        .lock()
                        .map_err(|_| CanvasError::InvalidFile("Renderer worker failed".into()))?
                        .export_snapshot_png(&snapshot, scale)
                })
                .await;
            let _ = this.update_in(cx, |this, _, cx| {
                this.clipboard_task = None;
                match result {
                    Ok(bytes) => cx.write_to_clipboard(ClipboardItem::new_image(
                        &gpui::Image::from_bytes(gpui::ImageFormat::Png, bytes),
                    )),
                    Err(error) => this.fail(error, cx),
                }
            });
        }));
    }

    pub fn set_tool(
        &mut self,
        tool: draw::ActiveTool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_text(true, window, cx);
        self.cancel_gesture(window, cx);
        match self.session.set_tool(&mut self.document, tool) {
            Ok(_) => {
                self.cursor = if tool == draw::ActiveTool::Select {
                    CursorStyle::Arrow
                } else if tool == draw::ActiveTool::Text {
                    CursorStyle::IBeam
                } else {
                    CursorStyle::Crosshair
                };
                self.changed(window, cx);
            }
            Err(error) => self.fail(error, cx),
        }
        self.focus.focus(window, cx);
    }

    pub fn command(
        &mut self,
        command: impl FnOnce(
            &mut draw::Engine,
            draw::ViewportId,
        ) -> Result<draw::MutationResult, draw::ErrorCode>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_text(true, window, cx);
        self.cancel_gesture(window, cx);
        match self.session.mutate(&mut self.document, command) {
            Ok(_) => self.changed(window, cx),
            Err(error) => self.fail(error, cx),
        }
    }

    pub fn replace_document(
        &mut self,
        mut document: CanvasDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), CanvasError> {
        self.cancel_gesture(window, cx);
        let mut session = CanvasSession::new(&mut document)?;
        let reference_session = CanvasSession::new(&mut document)?;
        if self.bounds.size.width > px(0.0) && self.bounds.size.height > px(0.0) {
            session.set_surface(
                &mut document,
                f32::from(self.bounds.size.width).round().max(1.0) as u32,
                f32::from(self.bounds.size.height).round().max(1.0) as u32,
            )?;
        }
        let previous_document = std::mem::replace(&mut self.document, document);
        crate::retire_canvas_document(previous_document, cx);
        if let Some(previous) = self.image.take() {
            let _ = window.drop_image(previous);
        }
        self.image_frame = None;
        self.session = session;
        self.reference_session = reference_session;
        self.draft = None;
        self.requested_key = None;
        self.error = None;
        self.fit(window, cx);
        Ok(())
    }

    pub fn set_background(
        &mut self,
        image: SourceImage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_text(true, window, cx);
        self.cancel_gesture(window, cx);
        if let Err(error) = self.document.replace_background(Some(image)) {
            self.fail(error, cx);
            return;
        }
        self.fit(window, cx);
    }

    pub fn fit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let surface = self.session.scene().frame().surface;
        let (center, width, height) = if let Some(image) = self.document.background() {
            let width = f64::from(image.pixels().width()) * image.units_per_pixel();
            let height = f64::from(image.pixels().height()) * image.units_per_pixel();
            (
                draw::Point::new(
                    image.origin().x + width / 2.0,
                    image.origin().y + height / 2.0,
                ),
                width,
                height,
            )
        } else {
            (draw::Point::new(480.0, 320.0), 960.0, 640.0)
        };
        let camera = draw::Camera {
            center,
            zoom: (f64::from(surface.width) / width * 0.92)
                .min(f64::from(surface.height) / height * 0.92)
                .clamp(1.0e-6, 8.0),
        };
        if let Err(error) = self.session.set_camera(&mut self.document, camera) {
            self.fail(error, cx);
        }
        self.changed(window, cx);
    }

    pub fn zoom(&mut self, factor: f64, window: &mut Window, cx: &mut Context<Self>) {
        let mut camera = self.session.scene().frame().camera;
        camera.zoom = (camera.zoom * factor).clamp(1.0e-6, 8.0);
        if let Err(error) = self.session.set_camera(&mut self.document, camera) {
            self.fail(error, cx);
        }
        self.changed(window, cx);
    }

    fn fail(&mut self, error: impl std::fmt::Display, cx: &mut Context<Self>) {
        let error: SharedString = error.to_string().into();
        self.error = Some(error.clone());
        cx.emit(CanvasEvent::Error(error));
        cx.notify();
    }

    fn changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reference_valid = false;
        if let Err(error) = self.sync_text_measurements() {
            self.invalidate_render();
            self.fail(error, cx);
            return;
        }
        if let Err(error) = self.session.refresh(&self.document) {
            self.invalidate_render();
            self.fail(error, cx);
            return;
        }
        if let Err(error) = self.update_reference() {
            self.invalidate_render();
            self.fail(error, cx);
            return;
        }
        self.reference_valid = true;
        let cursor = self.reference_session.scene().cursor().unwrap_or_default();
        let key = (
            cursor.scene_revision,
            cursor.decoration_revision,
            self.reference_session.scene().frame(),
            self.document.resource_revision(),
            1.0_f32.to_bits(),
        );
        if self.requested_key != Some(key) {
            self.render_cancel.cancel();
            self.requested_key = Some(key);
            self.generation = self.generation.wrapping_add(1);
            self.schedule_render(window, cx);
        }
        cx.emit(CanvasEvent::Changed);
        cx.notify();
    }

    fn invalidate_render(&mut self) {
        self.render_cancel.cancel();
        self.generation = self.generation.wrapping_add(1);
        self.render_pending = false;
        self.requested_key = None;
    }

    fn schedule_render(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.reference_valid
            || self.bounds.size.width <= px(0.0)
            || self.bounds.size.height <= px(0.0)
        {
            return;
        }
        if self.render_task.is_some() {
            self.render_pending = true;
            return;
        }
        self.render_pending = false;
        let clear_renderer = std::mem::take(&mut self.clear_renderer_pending);
        let scene = self.reference_session.scene().clone();
        let presentation = self.options.presentation.clone();
        let image_frame = scene.frame();
        let background = self.document.background().cloned();
        let resources = self.document.resource_revision();
        let scale = 1.0;
        let generation = self.generation;
        let cancel = crate::smart_erase::Cancellation::default();
        self.render_cancel = cancel.clone();
        let renderer = self.renderer.as_ref().unwrap().clone();
        self.render_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut renderer = renderer
                        .lock()
                        .map_err(|_| CanvasError::InvalidFile("Renderer worker failed".into()))?;
                    if clear_renderer {
                        renderer.clear_render_state();
                    }
                    let image = renderer.render_presented_cancellable(
                        &scene,
                        background.as_ref(),
                        resources,
                        scale,
                        &cancel,
                        false,
                        &presentation,
                    )?;
                    Ok::<_, CanvasError>((image, renderer.warnings().to_vec()))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.render_task = None;
                if this.reference_valid && generation == this.generation {
                    match result {
                        Ok((image, warnings)) => {
                            let mut bgra = (*image).clone();
                            for pixel in bgra.pixels_mut() {
                                pixel.0.swap(0, 2);
                            }
                            let image = Arc::new(RenderImage::new(vec![image::Frame::new(bgra)]));
                            if let Some(previous) = this.image.replace(image) {
                                let _ = window.drop_image(previous);
                            }
                            this.image_frame = Some(image_frame);
                            this.rendered_generation = generation;
                            this.error = None;
                            if let Some(warning) = warnings.first() {
                                this.fail(warning, cx);
                            }
                            cx.emit(CanvasEvent::Rendered);
                            cx.notify();
                        }
                        Err(error) => {
                            this.rendered_generation = generation;
                            this.fail(error, cx);
                        }
                    }
                }
                if this.render_pending || generation != this.generation {
                    this.schedule_render(window, cx);
                }
            });
        }));
    }

    fn cancel_gesture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pan = None;
        self.space_down = false;
        self.space_key = None;
        self.selecting_text = false;
        window.release_pointer();
        match self
            .session
            .input(&mut self.document, draw::InputEvent::FocusLost)
        {
            Ok(_) => {}
            Err(error) => self.fail(error, cx),
        }
        self.changed(window, cx);
    }

    fn input(
        &mut self,
        event: draw::InputEvent,
        hitbox: &Hitbox,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.session.input(&mut self.document, event) {
            Ok(update) => {
                match update.interaction.capture {
                    draw::PointerCaptureCommand::Capture(_) => window.capture_pointer(hitbox.id),
                    draw::PointerCaptureCommand::Release => window.release_pointer(),
                    _ => {}
                }
                if let draw::CursorCommand::Set(cursor) = update.interaction.cursor {
                    self.cursor = native_cursor(cursor);
                }
                self.changed(window, cx);
            }
            Err(error) => {
                window.release_pointer();
                self.fail(error, cx);
            }
        }
    }

    fn sync_text_measurements(&mut self) -> Result<(), CanvasError> {
        crate::text_layout::synchronize(
            self.metrics.as_mut().unwrap(),
            &mut self.document,
            self.session.viewport(),
        )
    }
    fn update_reference(&mut self) -> Result<(), CanvasError> {
        if let Some(reference) = self.options.filter_render_reference {
            return crate::snapshot::prepare_fixed_reference(
                &mut self.document,
                &mut self.reference_session,
                reference,
            );
        }
        crate::snapshot::prepare_reference(
            &mut self.document,
            &mut self.reference_session,
            self.session.scene().frame(),
        )
    }
}

impl Render for CanvasView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("snow-canvas")
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .track_focus(&self.focus)
            .key_context("SnowCanvas")
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(Self::key_up))
            .child(element::CanvasElement { view: cx.entity() })
    }
}

fn native_cursor(cursor: draw::CursorStyle) -> CursorStyle {
    match cursor {
        draw::CursorStyle::Text => CursorStyle::IBeam,
        draw::CursorStyle::Crosshair
        | draw::CursorStyle::CornerRadius
        | draw::CursorStyle::Stroke
        | draw::CursorStyle::Eraser => CursorStyle::Crosshair,
        draw::CursorStyle::Grab => CursorStyle::OpenHand,
        draw::CursorStyle::Grabbing | draw::CursorStyle::Move => CursorStyle::ClosedHand,
        draw::CursorStyle::ResizeHorizontal => CursorStyle::ResizeLeftRight,
        draw::CursorStyle::ResizeVertical => CursorStyle::ResizeUpDown,
        draw::CursorStyle::ResizeNwSe => CursorStyle::ResizeUpLeftDownRight,
        draw::CursorStyle::ResizeNeSw => CursorStyle::ResizeUpRightDownLeft,
        _ => CursorStyle::Arrow,
    }
}
