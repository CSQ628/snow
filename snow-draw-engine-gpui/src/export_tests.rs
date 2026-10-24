use super::*;
use crate::RetainedScene;

#[test]
fn streamed_culling_preserves_thick_connectors_and_external_magnifier_leaders() {
    let frame = FrameView {
        camera: Camera {
            center: Point::new(1100.0, 150.0),
            zoom: 1.0,
        },
        surface: SurfaceSize {
            width: 2200,
            height: 300,
        },
        ..Default::default()
    };
    let stroke = ColorRgba8 {
        r: 30,
        g: 100,
        b: 200,
        a: 255,
    };
    let connector = SceneDisplayItem::SerialNumberConnector(SerialNumberConnectorDisplayItem {
        start_x: 0.0,
        start_y: -20.0,
        end_x: 2200.0,
        end_y: -20.0,
        stroke,
        stroke_width: 60.0,
        opacity: 1.0,
        ..Default::default()
    });
    let leader = ArrowDisplayItem {
        points: vec![[0.0, 150.0], [2200.0, 150.0]],
        path_commands: vec![
            PathCommand::MoveTo {
                point: [0.0, 150.0],
            },
            PathCommand::LineTo {
                point: [2200.0, 150.0],
            },
        ],
        stroke,
        stroke_width: 4.0,
        opacity: 1.0,
        ..Default::default()
    };
    let magnifier = SceneDisplayItem::Magnifier(MagnifierDisplayItem {
        lens: RectangleDisplayItem {
            center_x: 20.0,
            center_y: -100.0,
            width: 20.0,
            height: 20.0,
            stroke,
            stroke_width: 2.0,
            opacity: 1.0,
            ..Default::default()
        },
        source_center: Point::new(60.0, -100.0),
        source_width: 10.0,
        source_height: 10.0,
        factor: 2.0,
        connection_type: MagnifierConnectionType::None,
        source_outline: false,
        leader: Some(leader),
    });
    let mut patch = ViewportPatch {
        frame_view: frame,
        ..Default::default()
    };
    patch.scene.ops.push(ReplaceRangeOp {
        start: 0,
        delete_count: 0,
        insert_items: vec![connector, magnifier],
    });
    let mut scene = RetainedScene::default();
    scene.apply(&patch).unwrap();
    let snapshot = CanvasSnapshot {
        scene: scene.clone(),
        background: None,
        resources: 1,
        viewport: frame,
        presentation: Default::default(),
    };
    let mut renderer = CanvasRenderer::default();
    let full = renderer.render(&scene, None, 1, 1.0).unwrap();
    assert_ne!(full.get_pixel(1100, 2).0[3], 0);
    assert_ne!(full.get_pixel(1100, 150).0[3], 0);
    let tile = renderer
        .export_region(&snapshot, 1.0, [1000, 0, 200, 250], &Default::default())
        .unwrap();
    let expected = image::imageops::crop_imm(full.as_ref(), 1000, 0, 200, 250).to_image();
    assert_eq!(tile, expected);
}
