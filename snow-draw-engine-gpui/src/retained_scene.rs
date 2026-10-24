use crate::CanvasError;
use snow_draw_engine::{
    DecorationView, DisplayFilterType, DisplaySpotlightCutout, FrameView, OverlayDisplayItem,
    PatchCursor, SceneDisplayItem, SceneRenderRun, ViewportPatch,
};
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
static NEXT_RENDER_IDENTITY: AtomicU64 = AtomicU64::new(1);

/// Independently shared layers keep overlay changes independent of document size.
#[derive(Clone, Default)]
pub struct RetainedScene {
    cursor: Option<PatchCursor>,
    frame: FrameView,
    items: Arc<Vec<SceneDisplayItem>>,
    overlays: Arc<Vec<OverlayDisplayItem>>,
    decoration: DecorationView,
    spotlights: Arc<Vec<DisplaySpotlightCutout>>,
    runs: Arc<Vec<SceneRenderRun>>,
    render_identity: u64,
    damage: Arc<Vec<snow_draw_engine::DirtyRegion>>,
    overlay_damage: Arc<Vec<snow_draw_engine::DirtyRegion>>,
}
impl RetainedScene {
    pub fn cursor(&self) -> Option<PatchCursor> {
        self.cursor
    }
    pub(crate) fn render_identity(&self) -> u64 {
        self.render_identity
    }
    pub fn frame(&self) -> FrameView {
        self.frame
    }
    pub(crate) fn with_frame(&self, frame: FrameView) -> Self {
        Self {
            frame,
            ..self.clone()
        }
    }
    pub(crate) fn culled(
        &self,
        frame: FrameView,
        keep: impl Fn(&SceneDisplayItem) -> bool,
    ) -> Self {
        let mut prefix = Vec::with_capacity(self.items.len() + 1);
        prefix.push(0_u32);
        let mut items = Vec::new();
        for item in self.items.iter() {
            if keep(item) {
                items.push(item.clone());
            }
            prefix.push(items.len() as u32);
        }
        let runs = self
            .runs
            .iter()
            .filter_map(|run| {
                let start = prefix[run.start as usize];
                let count = prefix[(run.start + run.count) as usize] - start;
                (count > 0).then_some(SceneRenderRun {
                    start,
                    count,
                    ..*run
                })
            })
            .collect();
        Self {
            frame,
            items: Arc::new(items),
            runs: Arc::new(runs),
            render_identity: NEXT_RENDER_IDENTITY.fetch_add(1, Ordering::Relaxed),
            ..self.clone()
        }
    }
    pub fn items(&self) -> &[SceneDisplayItem] {
        &self.items
    }
    pub fn overlays(&self) -> &[OverlayDisplayItem] {
        &self.overlays
    }
    pub fn decoration(&self) -> DecorationView {
        self.decoration
    }
    pub fn spotlights(&self) -> &[DisplaySpotlightCutout] {
        &self.spotlights
    }
    pub fn runs(&self) -> &[SceneRenderRun] {
        &self.runs
    }
    /// Latest engine patch damage in viewport logical pixels. Diagnostic only;
    /// consumers must not assume this covers multiple coalesced scene revisions.
    pub fn dirty_regions(&self) -> &[snow_draw_engine::DirtyRegion] {
        &self.damage
    }
    pub fn overlay_dirty_regions(&self) -> &[snow_draw_engine::DirtyRegion] {
        &self.overlay_damage
    }
    pub fn apply(&mut self, patch: &ViewportPatch) -> Result<bool, CanvasError> {
        let cursor = PatchCursor {
            scene_revision: snow_draw_engine::SceneRevision(patch.scene.revision),
            overlay_revision: snow_draw_engine::OverlayRevision(patch.overlay.revision),
            decoration_revision: snow_draw_engine::DecorationRevision(patch.decoration.revision),
        };
        if self.cursor == Some(cursor) && self.frame == patch.frame_view {
            return Ok(false);
        }
        let old = self.cursor.unwrap_or_default();
        if self.cursor.is_some()
            && ((!patch.scene.reset && patch.scene.base_revision != old.scene_revision.0)
                || (!patch.overlay.reset && patch.overlay.base_revision != old.overlay_revision.0)
                || (!patch.decoration.reset
                    && patch.decoration.base_revision != old.decoration_revision.0))
        {
            return Err(stale());
        }
        let mut next = self.clone();
        if patch
            .scene
            .dirty_regions
            .len()
            .saturating_add(patch.decoration.dirty_regions.len())
            > 1024
            || patch.overlay.dirty_regions.len() > 1024
        {
            return Err(stale());
        }
        let damage: Vec<_> = patch
            .scene
            .dirty_regions
            .iter()
            .chain(&patch.decoration.dirty_regions)
            .copied()
            .collect();
        if damage.iter().chain(&patch.overlay.dirty_regions).any(|r| {
            [r.min_x, r.min_y, r.max_x, r.max_y]
                .into_iter()
                .any(|v| !v.is_finite())
                || r.min_x > r.max_x
                || r.min_y > r.max_y
        }) {
            return Err(stale());
        }
        next.damage = Arc::new(damage);
        next.overlay_damage = Arc::new(patch.overlay.dirty_regions.clone());
        if patch.scene.reset
            || patch.decoration.reset
            || old.scene_revision != cursor.scene_revision
            || old.decoration_revision != cursor.decoration_revision
        {
            // Engine cursors are local to a viewport. A new export viewport can
            // reuse the same cursor numbers for entirely different content.
            next.render_identity = NEXT_RENDER_IDENTITY.fetch_add(1, Ordering::Relaxed);
        }
        if patch.scene.reset || !patch.scene.ops.is_empty() {
            let mut items = if patch.scene.reset {
                Vec::new()
            } else {
                (*self.items).clone()
            };
            apply_ops(&mut items, &patch.scene.ops)?;
            next.items = Arc::new(items);
        }
        if patch.overlay.reset || !patch.overlay.ops.is_empty() {
            let mut overlays = if patch.overlay.reset {
                Vec::new()
            } else {
                (*self.overlays).clone()
            };
            apply_ops(&mut overlays, &patch.overlay.ops)?;
            next.overlays = Arc::new(overlays);
        }
        if patch.decoration.reset || !patch.decoration.spotlight_ops.is_empty() {
            let mut spots = if patch.decoration.reset {
                Vec::new()
            } else {
                (*self.spotlights).clone()
            };
            apply_ops(&mut spots, &patch.decoration.spotlight_ops)?;
            next.spotlights = Arc::new(spots);
        }
        // Rust items carry full Arc geometry; tail patches are derived-cache/C transport data.
        if let Some(runs) = &patch.scene_render_plan {
            next.runs = Arc::new(runs.clone());
        } else if patch.scene.reset {
            return Err(stale());
        }
        if patch.scene.reset || !patch.scene.ops.is_empty() || patch.scene_render_plan.is_some() {
            validate_runs(&next.items, &next.runs)?;
        }
        next.decoration = patch.decoration.view;
        next.frame = patch.frame_view;
        next.cursor = Some(cursor);
        *self = next;
        Ok(true)
    }
}
pub(crate) fn ordinary_filter(item: &SceneDisplayItem) -> bool {
    matches!(item, SceneDisplayItem::Filter(filter) if !matches!(filter.filter.filter_type,
        DisplayFilterType::SmartErase | DisplayFilterType::RestoreBackground))
}
fn validate_runs(items: &[SceneDisplayItem], runs: &[SceneRenderRun]) -> Result<(), CanvasError> {
    let mut next = 0;
    let mut sources = HashSet::new();
    let mut effects = HashSet::new();
    let mut previous = None;
    for run in runs {
        let start = run.start as usize;
        let end = start.checked_add(run.count as usize).ok_or_else(stale)?;
        if run.count == 0
            || start < next
            || end > items.len()
            || items[next..start].iter().any(ordinary_filter)
            || (previous == Some(run.source_pass) && start != next)
            || !effects.insert(run.effect_run)
        {
            return Err(stale());
        }
        if previous != Some(run.source_pass) && !sources.insert(run.source_pass) {
            return Err(stale());
        }
        let SceneDisplayItem::Filter(first) = &items[start] else {
            return Err(stale());
        };
        let mut spec = first.filter;
        spec.render_phase = 0;
        for item in &items[start..end] {
            let SceneDisplayItem::Filter(filter) = item else {
                return Err(stale());
            };
            let mut other = filter.filter;
            other.render_phase = 0;
            if !ordinary_filter(item) || other != spec {
                return Err(stale());
            }
        }
        next = end;
        previous = Some(run.source_pass);
    }
    if items[next..].iter().any(ordinary_filter) {
        return Err(stale());
    }
    Ok(())
}
fn stale() -> CanvasError {
    snow_draw_engine::ErrorCode::StaleRevision.into()
}
fn apply_ops<T: Clone>(
    values: &mut Vec<T>,
    ops: &[snow_draw_engine::ReplaceRangeOp<T>],
) -> Result<(), CanvasError> {
    // Follow the engine's existing forward-order consumer contract.
    for operation in ops {
        let start = operation.start as usize;
        let end = start
            .checked_add(operation.delete_count as usize)
            .ok_or_else(stale)?;
        if end > values.len() {
            return Err(stale());
        }
        values.splice(start..end, operation.insert_items.iter().cloned());
    }
    Ok(())
}
