//! A retained native GPUI adapter for the UI-independent Snow Draw Engine.
//!
//! This crate does not link Qt or depend on the unfinished Ant Design GPUI components.
mod cleanup;
mod damage;
#[cfg(test)]
mod damage_tests;
mod document;
mod export;
mod file_io;
mod font_cache;
mod host;
mod persistence;
mod profile;
mod renderer;
mod retained_scene;
mod sample;
mod smart_erase;
mod snapshot;
mod text_buffer;
mod text_layout;
mod text_shaping;
mod vector;
mod view;

pub use cleanup::{CanvasCleanupDiagnostics, canvas_cleanup_diagnostics, retire_canvas_document};
pub use document::{CanvasDocument, CanvasError, CanvasSession, SourceImage};
pub use file_io::{read_bounded, read_document, write_atomic, write_atomic_with};
pub use host::{
    CanvasPresentation, CanvasRect, CanvasRenderReference, CanvasViewOptions, ExportTile,
    ImageSourceLayer,
};
pub use persistence::DrawingSnapshot;
pub use profile::CanvasRenderProfile;
pub use renderer::{CanvasRenderer, SmartEraseSnapshot};
pub use retained_scene::RetainedScene;
pub use sample::showcase_document;
pub use smart_erase::Cancellation as CanvasCancellation;
pub use snapshot::CanvasSnapshot;
pub use snow_draw_engine as engine;
pub use view::{
    CanvasCommandKeyResolver, CanvasCrosshair, CanvasCrosshairFactory, CanvasCursorLayer,
    CanvasEvent, CanvasView,
};
