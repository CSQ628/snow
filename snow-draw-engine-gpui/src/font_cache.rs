//! Own used font bytes once. File-backed fontdb sources otherwise map/unmap the
//! same font for each outlined glyph and each bidi run, dominating long text.
use resvg::usvg::{self, fontdb};
use std::{
    collections::HashMap,
    io::Read,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub(crate) struct FontBytes {
    files: HashMap<PathBuf, Arc<dyn AsRef<[u8]> + Send + Sync>>,
    bytes: usize,
}
pub(crate) type Cache = Arc<Mutex<FontBytes>>;

pub(crate) fn owned_font(
    id: fontdb::ID,
    database: &mut Arc<fontdb::Database>,
    cache: &Cache,
) -> Option<fontdb::ID> {
    let mut face = database.face(id)?.clone();
    let fontdb::Source::File(path) = &face.source else {
        return Some(id);
    };
    let data = {
        let mut cache = cache.lock().ok()?;
        if let Some(data) = cache.files.get(path) {
            data.clone()
        } else {
            const BUDGET: usize = 64 * 1024 * 1024;
            let remaining = BUDGET.saturating_sub(cache.bytes);
            let file = std::fs::File::open(path).ok()?;
            if file.metadata().ok()?.len() > remaining as u64 {
                return Some(id);
            }
            let mut bytes = Vec::new();
            file.take(remaining as u64 + 1)
                .read_to_end(&mut bytes)
                .ok()?;
            if bytes.len() > remaining {
                return Some(id);
            }
            cache.bytes += bytes.len();
            let data: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(bytes);
            cache.files.insert(path.clone(), data.clone());
            data
        }
    };
    if let Some(existing) = database.faces().find(|candidate| candidate.index == face.index
        && matches!(&candidate.source, fontdb::Source::Binary(bytes) if Arc::ptr_eq(bytes, &data))) {
        return Some(existing.id);
    }
    face.id = fontdb::ID::dummy();
    face.source = fontdb::Source::Binary(data);
    Some(Arc::make_mut(database).push_face_info(face))
}

pub(crate) fn resolver() -> (usvg::FontResolver<'static>, Cache) {
    let cache = Cache::default();
    let font_cache = cache.clone();
    let select = usvg::FontResolver::default_font_selector();
    let fallback = usvg::FontResolver::default_fallback_selector();
    let fallback_cache = cache.clone();
    (
        usvg::FontResolver {
            select_font: Box::new(move |font, database| {
                let id = select(font, database)?;
                owned_font(id, database, &font_cache)
            }),
            select_fallback: Box::new(move |character, excluded, database| {
                // The original file face and its owned-byte copy are the same font.
                let names: std::collections::HashSet<_> = excluded
                    .iter()
                    .filter_map(|id| database.face(*id))
                    .map(|face| face.post_script_name.as_str())
                    .collect();
                let mut excluded = excluded.to_vec();
                excluded.extend(
                    database
                        .faces()
                        .filter(|face| names.contains(face.post_script_name.as_str()))
                        .map(|face| face.id),
                );
                let id = fallback(character, &excluded, database)?;
                owned_font(id, database, &fallback_cache)
            }),
        },
        cache,
    )
}
