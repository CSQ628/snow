use super::*;

pub(super) struct CanvasElement {
    pub view: Entity<CanvasView>,
}
impl IntoElement for CanvasElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for CanvasElement {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Hitbox {
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        self.view.update(cx, |this, cx| {
            let resized = this.bounds.size != bounds.size || this.scale != window.scale_factor();
            this.bounds = bounds;
            this.scale = window.scale_factor();
            this.update_crosshair(this.scale, cx);
            if resized && bounds.size.width > px(0.0) && bounds.size.height > px(0.0) {
                match this.session.set_surface(
                    &mut this.document,
                    f32::from(bounds.size.width).round().max(1.0) as u32,
                    f32::from(bounds.size.height).round().max(1.0) as u32,
                ) {
                    Ok(_) => {
                        if this.generation == 0 {
                            this.fit(window, cx);
                        } else {
                            this.changed(window, cx);
                        }
                    }
                    Err(error) => this.fail(error, cx),
                }
            }
            if this.document.engine().has_pointer_capture()
                || this.pan.is_some()
                || this.selecting_text
            {
                window.capture_pointer(hitbox.id);
            }
        });
        hitbox
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        hitbox: &mut Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (image, image_frame, focus, editing, cursor, scene, options, crosshair, dirty) =
            self.view.read_with(cx, |this, _| {
                (
                    this.image.clone(),
                    this.image_frame,
                    this.focus.clone(),
                    this.draft.is_some(),
                    this.effective_cursor(),
                    this.session.scene().clone(),
                    this.options.clone(),
                    this.crosshair_cache.and_then(|(_, value)| value),
                    this.show_dirty_regions,
                )
            });
        window.set_cursor_style(cursor, hitbox);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            let clear = scene.frame().clear_color;
            if options.presentation.clear_background {
                window.paint_quad(fill(bounds, gpui_color(clear)));
            }
            if let Some(image) = image
                && let Some(reference) = image_frame
            {
                let live = scene.frame();
                let (left, top, right, bottom) =
                    draw::canvas_viewport(reference.camera, reference.surface);
                let origin =
                    draw::canvas_to_view(draw::Point::new(left, top), &live.camera, live.surface);
                let end = draw::canvas_to_view(
                    draw::Point::new(right, bottom),
                    &live.camera,
                    live.surface,
                );
                let image_bounds = Bounds::from_corners(
                    bounds.origin + point(px(origin.x as f32), px(origin.y as f32)),
                    bounds.origin + point(px(end.x as f32), px(end.y as f32)),
                );
                let _ = window.paint_image(
                    bounds,
                    image_bounds,
                    Corners::all(px(0.0)),
                    image,
                    0,
                    false,
                );
            }
            if options.presentation.content_visible {
                paint_overlays(scene.overlays(), scene.frame(), bounds, window, cx);
            }
            if editing {
                window.handle_input(
                    &focus,
                    ElementInputHandler::new(bounds, self.view.clone()),
                    cx,
                );
                self.view
                    .update(cx, |this, cx| paint_text_selection(this, window, cx));
            }
            if dirty {
                for (regions, color) in [
                    (scene.dirty_regions(), rgba(0xe34b4be0)),
                    (scene.overlay_dirty_regions(), rgba(0x387bd1e0)),
                ] {
                    for r in regions {
                        let rect = Bounds::from_corners(
                            bounds.origin + point(px(r.min_x as f32), px(r.min_y as f32)),
                            bounds.origin + point(px(r.max_x as f32), px(r.max_y as f32)),
                        );
                        window.paint_quad(gpui::outline(rect, color, Default::default()));
                    }
                }
            }
            if cursor == CursorStyle::Crosshair
                && !editing
                && (hitbox.is_hovered(window) || window.captured_hitbox() == Some(hitbox.id))
                && let Some(crosshair) = crosshair
            {
                host_controls::paint_crosshair(crosshair, window.mouse_position(), window);
            }
        });
        let entity = self.view.downgrade();
        let hit = hitbox.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase.bubble() && hit.is_hovered_at(event.position, window) {
                let _ = entity.update(cx, |this, cx| this.mouse_down(event, &hit, window, cx));
                cx.stop_propagation();
            }
        });
        let entity = self.view.downgrade();
        let hit = hitbox.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase.bubble()
                && (hit.is_hovered_at(event.position, window)
                    || window.captured_hitbox() == Some(hit.id))
            {
                let _ = entity.update(cx, |this, cx| this.mouse_move(event, &hit, window, cx));
            }
        });
        let entity = self.view.downgrade();
        let hit = hitbox.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase.bubble()
                && (hit.is_hovered_at(event.position, window)
                    || window.captured_hitbox() == Some(hit.id))
            {
                let _ = entity.update(cx, |this, cx| this.mouse_up(event, &hit, window, cx));
            }
        });
        let entity = self.view.downgrade();
        let hit = hitbox.clone();
        window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
            if phase.bubble() && hit.should_handle_scroll(window) {
                let _ = entity.update(cx, |this, cx| {
                    if !this.options.interaction_enabled || !this.options.wheel_zoom_enabled {
                        return;
                    }
                    let (x, y, kind) = match event.delta {
                        ScrollDelta::Pixels(delta) => (
                            f64::from(f32::from(delta.x)),
                            f64::from(f32::from(delta.y)),
                            draw::WheelDeltaKind::Pixel,
                        ),
                        ScrollDelta::Lines(delta) => (
                            f64::from(delta.x) * 120.0,
                            f64::from(delta.y) * 120.0,
                            draw::WheelDeltaKind::Angle,
                        ),
                    };
                    let position = this.local(event.position);
                    this.input(
                        draw::InputEvent::Wheel(draw::WheelEvent {
                            position,
                            delta: draw::Vector2 { x: -x, y: -y },
                            delta_kind: kind,
                            modifiers: modifiers(event.modifiers),
                        }),
                        &hit,
                        window,
                        cx,
                    );
                });
                cx.stop_propagation();
            }
        });
    }
}

impl CanvasView {
    fn local(&self, point: Point<Pixels>) -> draw::Point<f64> {
        let local = point - self.bounds.origin;
        draw::Point::new(f64::from(f32::from(local.x)), f64::from(f32::from(local.y)))
    }
    fn world(&self, point: Point<Pixels>) -> draw::Point<f64> {
        let frame = self.session.scene().frame();
        draw::view_to_canvas(self.local(point), &frame.camera, frame.surface)
    }
    fn pointer(
        &self,
        kind: draw::PointerEventType,
        position: Point<Pixels>,
        button: Option<MouseButton>,
        modifier: Modifiers,
    ) -> draw::InputEvent {
        draw::InputEvent::Pointer(draw::PointerEvent {
            pointer_id: 1,
            event_type: kind,
            device: draw::PointerDevice::Mouse,
            position: self.local(position),
            button: button.map(pointer_button),
            buttons: draw::PointerButtons(if kind == draw::PointerEventType::Up {
                0
            } else {
                match button {
                    Some(MouseButton::Left) => draw::PointerButtons::PRIMARY,
                    Some(MouseButton::Right) => draw::PointerButtons::SECONDARY,
                    _ => 0,
                }
            }),
            modifiers: modifiers(modifier),
            pressure: None,
            timestamp_ms: 0,
        })
    }
    fn mouse_down(
        &mut self,
        event: &MouseDownEvent,
        hitbox: &Hitbox,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.options.interaction_enabled {
            return;
        }
        self.focus.focus(window, cx);
        if event.button == MouseButton::Middle || self.space_down {
            let space = self.space_down;
            self.finish_text(true, window, cx);
            self.cancel_gesture(window, cx);
            self.space_down = space;
            self.pan = Some((event.position, self.session.scene().frame().camera));
            self.cursor = CursorStyle::ClosedHand;
            window.capture_pointer(hitbox.id);
            cx.notify();
            return;
        }
        if let Some(draft) = self.draft.as_ref() {
            let world = self.world(event.position);
            let (sin, cos) = draft.rotation.sin_cos();
            let dx = world.x - draft.center.x;
            let dy = world.y - draft.center.y;
            let width = if draft.auto_resize {
                draft.layout.width
            } else {
                draft.width
            };
            if (dx * cos + dy * sin).abs() <= width / 2.0 + 8.0
                && (-dx * sin + dy * cos).abs() <= draft.layout.height / 2.0 + 8.0
                && let Some(index) = self.text_index_at(event.position)
            {
                let draft = self.draft.as_mut().unwrap();
                draft.buffer.select(index, event.modifiers.shift);
                self.selecting_text = true;
                window.capture_pointer(hitbox.id);
                cx.notify();
                return;
            }
            self.finish_text(true, window, cx);
        }
        if self
            .options
            .presentation
            .content_clip
            .is_some_and(|area| !area.contains(self.world(event.position)))
            && !self.hits_existing_selection(event.position)
        {
            return;
        }
        let kind = if event.click_count >= 2 {
            draw::PointerEventType::DoubleClick
        } else {
            draw::PointerEventType::Down
        };
        self.input(
            self.pointer(kind, event.position, Some(event.button), event.modifiers),
            hitbox,
            window,
            cx,
        );
        self.check_text_request(event.position, window, cx);
    }
    pub(super) fn hits_existing_selection(&self, position: Point<Pixels>) -> bool {
        let world = self.world(position);
        let selected = self.document.engine().selected_ids();
        if selected.is_empty() {
            return false;
        }
        let frame = self.session.scene().frame();
        if self
            .document
            .engine()
            .viewport_active_tool(self.session.viewport())
            .ok()
            == Some(draw::ActiveTool::Select)
            && self
                .document
                .engine()
                .hit_quick_selection_at(
                    self.session.viewport(),
                    self.local(position),
                    draw::PointerButton::Primary,
                )
                .ok()
                .flatten()
                .is_some_and(|id| selected.contains(&id))
        {
            return true;
        }
        self.session.scene().overlays().iter().any(|overlay| {
            let draw::OverlayDisplayItem::Rectangle(item) = overlay else {
                return false;
            };
            if !matches!(
                item.kind,
                draw::UiShapeKind::SelectionResizeHandle
                    | draw::UiShapeKind::SelectionRotationHandle
                    | draw::UiShapeKind::SelectionCornerRadiusHandle
                    | draw::UiShapeKind::ArrowEndpointHandle
                    | draw::UiShapeKind::ArrowFocusHandle
                    | draw::UiShapeKind::ArrowSegmentHandle
                    | draw::UiShapeKind::MagnifierMoveHandle
                    | draw::UiShapeKind::SpeechBubbleAddTailHandle
                    | draw::UiShapeKind::SpeechBubbleTailBaseHandle
                    | draw::UiShapeKind::SpeechBubbleTailTipHandle
                    | draw::UiShapeKind::ShapeTextHandle
            ) {
                return false;
            }
            let (sin, cos) = item.rotation.sin_cos();
            let dx = world.x - item.center_x;
            let dy = world.y - item.center_y;
            (dx * cos + dy * sin).abs() <= item.width / 2.0 + 4.0 / frame.camera.zoom
                && (-dx * sin + dy * cos).abs() <= item.height / 2.0 + 4.0 / frame.camera.zoom
        })
    }
    fn mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        hitbox: &Hitbox,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((origin, camera)) = self.pan {
            let delta = event.position - origin;
            let next = draw::Camera {
                center: draw::Point::new(
                    camera.center.x - f64::from(f32::from(delta.x)) / camera.zoom,
                    camera.center.y - f64::from(f32::from(delta.y)) / camera.zoom,
                ),
                ..camera
            };
            if let Err(error) = self.session.set_camera(&mut self.document, next) {
                self.fail(error, cx);
            }
            self.changed(window, cx);
            return;
        }
        if self.selecting_text {
            if let Some(index) = self.text_index_at(event.position) {
                self.draft.as_mut().unwrap().buffer.select(index, true);
                cx.notify();
            }
            return;
        }
        if self.draft.is_some() {
            return;
        }
        self.input(
            self.pointer(
                draw::PointerEventType::Move,
                event.position,
                event.pressed_button,
                event.modifiers,
            ),
            hitbox,
            window,
            cx,
        );
    }
    fn mouse_up(
        &mut self,
        event: &MouseUpEvent,
        hitbox: &Hitbox,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pan.take().is_some() {
            window.release_pointer();
            self.cursor = CursorStyle::OpenHand;
            cx.notify();
            return;
        }
        if self.selecting_text {
            self.selecting_text = false;
            window.release_pointer();
            return;
        }
        if self.draft.is_some() {
            return;
        }
        self.input(
            self.pointer(
                draw::PointerEventType::Up,
                event.position,
                Some(event.button),
                event.modifiers,
            ),
            hitbox,
            window,
            cx,
        );
        self.check_text_request(event.position, window, cx);
    }
    fn check_text_request(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let viewport = self.session.viewport();
        let existing = self
            .document
            .command(|engine| engine.take_text_edit_request(viewport))
            .ok()
            .flatten();
        let bubble = self
            .document
            .command(|engine| engine.take_speech_bubble_text_edit_request(viewport))
            .ok()
            .flatten();
        let new = self
            .document
            .command(|engine| engine.take_new_text_draft_request(viewport))
            .unwrap_or(false);
        if existing.is_some() || bubble.is_some() || new {
            self.begin_text(self.world(position), existing, window, cx);
        }
    }
}

fn pointer_button(button: MouseButton) -> draw::PointerButton {
    match button {
        MouseButton::Right => draw::PointerButton::Secondary,
        MouseButton::Middle => draw::PointerButton::Middle,
        _ => draw::PointerButton::Primary,
    }
}
fn modifiers(modifiers: Modifiers) -> draw::Modifiers {
    draw::Modifiers {
        ctrl: modifiers.secondary(),
        shift: modifiers.shift,
        alt: modifiers.alt,
        meta: modifiers.platform,
    }
}
fn gpui_color(color: draw::ColorRgba8) -> Hsla {
    rgba(
        (u32::from(color.r) << 24)
            | (u32::from(color.g) << 16)
            | (u32::from(color.b) << 8)
            | u32::from(color.a),
    )
    .into()
}

fn paint_overlays(
    overlays: &[draw::OverlayDisplayItem],
    frame: draw::FrameView,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let map = |x: f64, y: f64| {
        let p = draw::canvas_to_view(draw::Point::new(x, y), &frame.camera, frame.surface);
        bounds.origin + point(px(p.x as f32), px(p.y as f32))
    };
    for overlay in overlays {
        match overlay {
            draw::OverlayDisplayItem::Rectangle(item) => {
                let (sin, cos) = item.rotation.sin_cos();
                let w = item.width / 2.0;
                let h = item.height / 2.0;
                let circular = matches!(
                    item.kind,
                    draw::UiShapeKind::ArrowEndpointHandle
                        | draw::UiShapeKind::ArrowFocusHandle
                        | draw::UiShapeKind::ArrowSegmentHandle
                        | draw::UiShapeKind::EraserCursor
                        | draw::UiShapeKind::SpeechBubbleTailBaseHandle
                        | draw::UiShapeKind::SpeechBubbleTailTipHandle
                );
                let local: Vec<_> = if circular {
                    (0..48)
                        .map(|i| {
                            let angle = i as f64 * std::f64::consts::TAU / 48.0;
                            (w * angle.cos(), h * angle.sin())
                        })
                        .collect()
                } else {
                    vec![(-w, -h), (w, -h), (w, h), (-w, h)]
                };
                let points: Vec<_> = local
                    .into_iter()
                    .map(|(x, y)| {
                        map(
                            item.center_x + x * cos - y * sin,
                            item.center_y + x * sin + y * cos,
                        )
                    })
                    .collect();
                if item.kind == draw::UiShapeKind::TextHoverUnderline {
                    let mut path =
                        PathBuilder::stroke(px((item.stroke_width * frame.camera.zoom) as f32));
                    path.move_to(points[3]);
                    path.line_to(points[2]);
                    if let Ok(path) = path.build() {
                        window.paint_path(path, gpui_color(item.stroke));
                    }
                    continue;
                }
                if item.fill.a != 0 {
                    let mut path = PathBuilder::fill();
                    path.add_polygon(&points, true);
                    if let Ok(path) = path.build() {
                        window.paint_path(path, gpui_color(item.fill));
                    }
                }
                if item.stroke.a != 0 && item.stroke_width > 0.0 {
                    let mut path =
                        PathBuilder::stroke(px((item.stroke_width * frame.camera.zoom) as f32));
                    path.add_polygon(&points, true);
                    if let Ok(path) = path.build() {
                        window.paint_path(path, gpui_color(item.stroke));
                    }
                }
            }
            draw::OverlayDisplayItem::SnapGuide(item) => {
                let mut path = PathBuilder::stroke(px(item.line_width as f32));
                path.move_to(map(item.start.x, item.start.y));
                path.line_to(map(item.end.x, item.end.y));
                for marker in item.markers.iter().take(item.marker_count as usize) {
                    let p = map(marker.x, marker.y);
                    let r = px(item.marker_size as f32 / 2.0);
                    path.move_to(p - point(r, r));
                    path.line_to(p + point(r, r));
                    path.move_to(p + point(-r, r));
                    path.line_to(p + point(r, -r));
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, gpui_color(item.color));
                }
                if let Some(label) = item.label {
                    let text: SharedString = format!("{label:.0}").into();
                    let line = window.text_system().shape_line(
                        text.clone(),
                        px(12.0),
                        &[gpui::TextRun {
                            len: text.len(),
                            font: gpui::font("Segoe UI"),
                            color: gpui_color(item.color),
                            ..Default::default()
                        }],
                        None,
                    );
                    let midpoint = map(
                        (item.start.x + item.end.x) / 2.0,
                        (item.start.y + item.end.y) / 2.0,
                    );
                    let origin = if item.axis == draw::SnapGuideAxis::Horizontal {
                        midpoint - point(line.width / 2.0, px(20.0))
                    } else {
                        midpoint + point(px(6.0), px(-8.0))
                    };
                    let _ = line.paint(origin, px(16.0), gpui::TextAlign::Left, None, window, cx);
                }
            }
            draw::OverlayDisplayItem::FocusConnection(item)
            | draw::OverlayDisplayItem::PenFilterContour(item) => {
                let mut path = PathBuilder::stroke(px(
                    (item.stroke_width * frame.camera.zoom).max(1.0) as f32
                ));
                for command in &item.path_commands {
                    match *command {
                        draw::PathCommand::MoveTo { point: [x, y] } => path.move_to(map(x, y)),
                        draw::PathCommand::LineTo { point: [x, y] } => path.line_to(map(x, y)),
                        draw::PathCommand::QuadTo {
                            control: [x, y],
                            end: [ex, ey],
                        } => path.curve_to(map(ex, ey), map(x, y)),
                        draw::PathCommand::CubicTo {
                            control_1: [x, y],
                            control_2: [x2, y2],
                            end: [ex, ey],
                        } => path.cubic_bezier_to(map(ex, ey), map(x, y), map(x2, y2)),
                    }
                }
                if item.path_commands.is_empty() {
                    for (index, [x, y]) in item.points.iter().enumerate() {
                        if index == 0 {
                            path.move_to(map(*x, *y));
                        } else {
                            path.line_to(map(*x, *y));
                        }
                    }
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, gpui_color(item.stroke));
                }
                for head in &item.arrowhead_primitives {
                    let points: Vec<_> = if head.kind == draw::ArrowheadDisplayPrimitiveKind::Circle
                    {
                        (0..32)
                            .map(|i| {
                                let t = i as f64 * std::f64::consts::TAU / 32.0;
                                map(
                                    head.center[0] + head.diameter / 2.0 * t.cos(),
                                    head.center[1] + head.diameter / 2.0 * t.sin(),
                                )
                            })
                            .collect()
                    } else {
                        head.points.iter().map(|[x, y]| map(*x, *y)).collect()
                    };
                    let closed = head.kind != draw::ArrowheadDisplayPrimitiveKind::Line;
                    if closed && head.fill_mode == draw::ArrowheadDisplayFillMode::Stroke {
                        let mut path = PathBuilder::fill();
                        path.add_polygon(&points, true);
                        if let Ok(path) = path.build() {
                            window.paint_path(path, gpui_color(item.stroke));
                        }
                    }
                    let mut path =
                        PathBuilder::stroke(px((item.stroke_width * frame.camera.zoom) as f32));
                    path.add_polygon(&points, closed);
                    if let Ok(path) = path.build() {
                        window.paint_path(path, gpui_color(item.stroke));
                    }
                }
            }
        }
    }
}

fn paint_text_selection(this: &mut CanvasView, window: &mut Window, _: &mut Context<CanvasView>) {
    let Some(draft) = this.draft.as_ref() else {
        return;
    };
    let selection = draft.buffer.selection();
    let caret = draft.buffer.caret;
    let marked = draft.buffer.marked.clone();
    let range = if selection.is_empty() {
        caret..caret
    } else {
        selection
    };
    for (left, top, right, bottom) in this.text_segments(range.clone()) {
        let points = [(left, top), (right, top), (right, bottom), (left, bottom)]
            .map(|(x, y)| this.text_world_position(x, y).unwrap());
        let mut path = PathBuilder::fill();
        path.add_polygon(&points, true);
        if let Ok(path) = path.build() {
            window.paint_path(
                path,
                if range.is_empty() {
                    rgba(0x1668dcff)
                } else {
                    rgba(0x1668dc44)
                },
            );
        }
    }
    if let Some(marked) = marked {
        for (left, _, right, bottom) in this.text_segments(marked) {
            let mut path = PathBuilder::stroke(px(1.0));
            path.move_to(this.text_world_position(left, bottom).unwrap());
            path.line_to(this.text_world_position(right, bottom).unwrap());
            if let Ok(path) = path.build() {
                window.paint_path(path, rgb(0x1668dc));
            }
        }
    }
}
