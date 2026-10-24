//! Opt-in CPU phase instrumentation; no timers run in the normal item hot path.
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct CanvasRenderProfile {
    pub raster_cache_hit: bool,
    pub partial_recomposition: bool,
    pub output_pixels: u64,
    pub working_pixels: u64,
    pub damaged_pixels: u64,
    pub total_ms: f64,
    pub background_ms: f64,
    pub vector_lower_and_layout_ms: f64,
    pub svg_parse_and_shape_ms: f64,
    pub vector_cache_ms: f64,
    pub raster_ms: f64,
    pub effects_ms: f64,
    pub finalize_ms: f64,
    pub vector_requests: u64,
    pub vector_hits: u64,
    pub vector_builds: u64,
}

pub(crate) fn elapsed(start: Option<std::time::Instant>) -> f64 {
    start.map_or(0.0, |start| start.elapsed().as_secs_f64() * 1000.0)
}
