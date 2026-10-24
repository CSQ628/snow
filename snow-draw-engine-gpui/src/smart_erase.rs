mod patch;
#[cfg(test)]
mod tests;
use crate::CanvasError;
use image::RgbaImage;
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// Cooperative cancellation checked between stages, rows and bounded target batches.
#[derive(Clone, Default)]
pub struct Cancellation {
    cancelled: Arc<AtomicBool>,
    #[cfg(test)]
    remaining: Option<Arc<std::sync::atomic::AtomicUsize>>,
}
impl Cancellation {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
    pub fn check(&self) -> Result<(), CanvasError> {
        #[cfg(test)]
        if let Some(remaining) = &self.remaining
            && remaining
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
                .is_err()
        {
            self.cancel();
        }
        if self.cancelled.load(Ordering::Relaxed) {
            Err(CanvasError::Cancelled)
        } else {
            Ok(())
        }
    }
    #[cfg(test)]
    pub(crate) fn after_checks(count: usize) -> Self {
        Self {
            cancelled: Default::default(),
            remaining: Some(Arc::new(std::sync::atomic::AtomicUsize::new(count))),
        }
    }
}
type Color = [f32; 3];
fn distance(a: Color, b: Color) -> f32 {
    a.into_iter().zip(b).map(|(a, b)| (a - b) * (a - b)).sum()
}
fn mix(a: Color, b: Color, t: f32) -> Color {
    std::array::from_fn(|i| a[i] * (1.0 - t) + b[i] * t)
}

/// Reconstructs original source pixels. Alpha and every unmasked byte remain exact.
/// Masked colors never enter observations, guides, donor patches or the pyramid.
#[cfg(test)]
pub(crate) fn reconstruct(
    source: &RgbaImage,
    mask: &[u8],
    cancel: &Cancellation,
) -> Result<RgbaImage, CanvasError> {
    reconstruct_with_donors(source, mask, None, cancel)
}
pub(crate) fn reconstruct_with_donors(
    source: &RgbaImage,
    mask: &[u8],
    donors: Option<&[u8]>,
    cancel: &Cancellation,
) -> Result<RgbaImage, CanvasError> {
    let width = source.width() as usize;
    let height = source.height() as usize;
    if mask.len() != width * height || donors.is_some_and(|d| d.len() != mask.len()) {
        return Err(snow_draw_engine::ErrorCode::InvalidArgument.into());
    }
    cancel.check()?;
    let hole: Vec<_> = mask
        .iter()
        .zip(source.pixels())
        .map(|(a, p)| *a != 0 && p[3] != 0)
        .collect();
    let indices: Vec<_> = hole
        .iter()
        .enumerate()
        .filter_map(|(i, h)| h.then_some(i))
        .collect();
    if indices.is_empty() {
        return Ok(source.clone());
    }
    let known: Vec<_> = hole
        .iter()
        .zip(source.pixels())
        .enumerate()
        .map(|(i, (h, p))| !*h && p[3] != 0 && donors.is_none_or(|d| d[i] != 0))
        .collect();
    if !known.iter().any(|k| *k) {
        return Err(CanvasError::Unsupported(
            "Smart Erase without unmasked source donors",
        ));
    }
    let x0 = indices.iter().map(|i| i % width).min().unwrap();
    let y0 = indices.iter().map(|i| i / width).min().unwrap();
    let x1 = indices.iter().map(|i| i % width).max().unwrap() + 1;
    let y1 = indices.iter().map(|i| i / width).max().unwrap() + 1;
    let left = x0.saturating_sub(512);
    let top = y0.saturating_sub(512);
    let right = (x1 + 512).min(width);
    let bottom = (y1 + 512).min(height);
    // Includes all float colors, masks, coordinates, pyramid and Gaussian scratch
    // at their peak live overlap. Reject before allocating the reconstruction grid.
    // This budget is independent of the (larger) input and render surface limits.
    const MAX_SCRATCH_BYTES: usize = 512 * 1024 * 1024;
    let estimated = (right - left)
        .checked_mul(bottom - top)
        .and_then(|n| n.checked_mul(256))
        .ok_or(CanvasError::ResourceLimit)?;
    if estimated > MAX_SCRATCH_BYTES {
        return Err(CanvasError::ResourceLimit);
    }
    let cropped_width = right - left;
    let mut level = Level::new(right - left, bottom - top);
    for y in top..bottom {
        cancel.check()?;
        for x in left..right {
            let i = y * width + x;
            let j = (y - top) * level.width + x - left;
            level.hole[j] = hole[i];
            level.covered[j] = source.as_raw()[i * 4 + 3] != 0 && (hole[i] || known[i]);
            if known[i] {
                level.source[j] = std::array::from_fn(|c| f32::from(source.as_raw()[i * 4 + c]));
            }
        }
    }
    let filled = if let Some(image) = surface(&level, cancel)? {
        image
    } else {
        level.make_domain(cancel)?;
        if let Some(image) = periodic(&level, cancel)? {
            image
        } else {
            patch::multiscale(level, cancel)?
        }
    };
    let mut output = source.clone();
    for i in indices {
        let j = (i / width - top) * cropped_width + i % width - left;
        for (c, value) in filled[j].iter().enumerate() {
            output.as_mut()[i * 4 + c] = value.round().clamp(0.0, 255.0) as u8;
        }
    }
    Ok(output)
}
#[derive(Clone)]
struct Level {
    width: usize,
    height: usize,
    source: Vec<Color>,
    hole: Vec<bool>,
    covered: Vec<bool>,
    domain: Vec<bool>,
}
impl Level {
    fn new(width: usize, height: usize) -> Self {
        let n = width * height;
        Self {
            width,
            height,
            source: vec![[0.0; 3]; n],
            hole: vec![false; n],
            covered: vec![false; n],
            domain: vec![false; n],
        }
    }
    fn at(&self, x: isize, y: isize) -> Option<usize> {
        (x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height)
            .then(|| y as usize * self.width + x as usize)
    }
    fn holes(&self) -> Vec<usize> {
        self.hole
            .iter()
            .enumerate()
            .filter_map(|(i, h)| h.then_some(i))
            .collect()
    }
    fn known(&self, i: usize) -> bool {
        self.covered[i] && !self.hole[i]
    }
    fn make_domain(&mut self, cancel: &Cancellation) -> Result<(), CanvasError> {
        let holes = self.holes();
        let nearest = nearest(self.width, self.height, &self.hole, cancel)?;
        let x0 = holes.iter().map(|i| i % self.width).min().unwrap();
        let x1 = holes.iter().map(|i| i % self.width).max().unwrap();
        let y0 = holes.iter().map(|i| i / self.width).min().unwrap();
        let y1 = holes.iter().map(|i| i / self.width).max().unwrap();
        let mut reach = ((x1 - x0 + 1).min(y1 - y0 + 1) / 2).clamp(16, 96);
        loop {
            cancel.check()?;
            for (i, p) in nearest.iter().enumerate() {
                let dx = (i % self.width).abs_diff(p % self.width);
                let dy = (i / self.width).abs_diff(p / self.width);
                self.domain[i] = self.known(i) && dx * dx + dy * dy <= reach * reach;
            }
            if erode(&self.domain, self.width, self.height, 3)
                .iter()
                .filter(|v| **v)
                .count()
                >= 64
                || reach >= 512
            {
                break;
            }
            reach = (reach * 2).min(512);
        }
        Ok(())
    }
}
// Linear multi-source propagation avoids a donor-count by target-count search.
fn nearest(
    width: usize,
    height: usize,
    seeds: &[bool],
    cancel: &Cancellation,
) -> Result<Vec<usize>, CanvasError> {
    let mut result = vec![usize::MAX; seeds.len()];
    let mut queue = VecDeque::new();
    for (i, seed) in seeds.iter().enumerate() {
        if *seed {
            result[i] = i;
            queue.push_back(i);
        }
    }
    if queue.is_empty() {
        return Err(CanvasError::Unsupported(
            "Smart Erase without source patches",
        ));
    }
    while let Some(i) = queue.pop_front() {
        if i % 4096 == 0 {
            cancel.check()?;
        }
        let x = i % width;
        let y = i / width;
        for j in [
            x.checked_sub(1).map(|_| i - 1),
            (x + 1 < width).then_some(i + 1),
            y.checked_sub(1).map(|_| i - width),
            (y + 1 < height).then_some(i + width),
        ]
        .into_iter()
        .flatten()
        {
            if result[j] == usize::MAX {
                result[j] = result[i];
                queue.push_back(j);
            }
        }
    }
    Ok(result)
}
fn erode(mask: &[bool], width: usize, height: usize, radius: usize) -> Vec<bool> {
    let stride = width + 1;
    let mut sums = vec![0_u32; (width + 1) * (height + 1)];
    for y in 0..height {
        let mut row = 0;
        for x in 0..width {
            row += u32::from(mask[y * width + x]);
            sums[(y + 1) * stride + x + 1] = sums[y * stride + x + 1] + row;
        }
    }
    let mut out = vec![false; mask.len()];
    let side = radius * 2 + 1;
    for y in radius..height.saturating_sub(radius) {
        for x in radius..width.saturating_sub(radius) {
            let (a, b, c, d) = (
                (y - radius) * stride + x - radius,
                (y - radius) * stride + x + radius + 1,
                (y + radius + 1) * stride + x - radius,
                (y + radius + 1) * stride + x + radius + 1,
            );
            out[y * width + x] = sums[d] + sums[a] - sums[b] - sums[c] == (side * side) as u32;
        }
    }
    out
}
fn surface(level: &Level, cancel: &Cancellation) -> Result<Option<Vec<Color>>, CanvasError> {
    let holes = level.holes();
    let w = level.width;
    let x0 = holes.iter().map(|i| i % w).min().unwrap();
    let x1 = holes.iter().map(|i| i % w).max().unwrap() + 1;
    let y0 = holes.iter().map(|i| i / w).min().unwrap();
    let y1 = holes.iter().map(|i| i / w).max().unwrap() + 1;
    let mut ring = vec![false; level.source.len()];
    for (n, i) in holes.iter().enumerate() {
        if n % 64 == 0 {
            cancel.check()?;
        }
        for dy in -4_isize..=4 {
            for dx in -4_isize..=4 {
                if dx * dx + dy * dy <= 20
                    && let Some(j) = level.at((i % w) as isize + dx, (i / w) as isize + dy)
                    && level.known(j)
                {
                    ring[j] = true;
                }
            }
        }
    }
    let points: Vec<_> = ring
        .iter()
        .enumerate()
        .filter_map(|(i, b)| b.then_some(i))
        .collect();
    if points.len() < 12 {
        return Ok(None);
    }
    let stride = (points.len() / 4096).max(1);
    let mut coefficients = [[0.0_f64; 3]; 3];
    let basis = |i: usize| {
        [
            1.0,
            ((i % w) as f64 - x0 as f64) / (x1 - x0) as f64,
            ((i / w) as f64 - y0 as f64) / (y1 - y0) as f64,
        ]
    };
    let predict = |b: [f64; 3], c: [[f64; 3]; 3]| {
        std::array::from_fn::<_, 3, _>(|channel| (0..3).map(|j| b[j] * c[j][channel]).sum::<f64>())
    };
    for iteration in 0..5 {
        cancel.check()?;
        let mut normal = [[0.0; 3]; 3];
        let mut rhs = [[0.0; 3]; 3];
        for i in points.iter().step_by(stride) {
            let b = basis(*i);
            let value = level.source[*i].map(|v| f64::from(v) / 255.0);
            let p = predict(b, coefficients);
            let residual = (0..3)
                .map(|c| (p[c] - value[c]).powi(2))
                .sum::<f64>()
                .sqrt();
            let weight = if iteration == 0 {
                1.0
            } else {
                (0.008 / residual.max(1e-8)).min(1.0)
            };
            for a in 0..3 {
                for c in 0..3 {
                    normal[a][c] += weight * b[a] * b[c];
                    rhs[a][c] += weight * b[a] * value[c];
                }
            }
        }
        let Some(solution) = solve(normal, rhs) else {
            return Ok(None);
        };
        coefficients = solution;
    }
    let mut inliers = 0;
    let mut count = 0;
    let mut error = 0.0;
    for i in points.iter().step_by(stride) {
        count += 1;
        let p = predict(basis(*i), coefficients);
        let e = (0..3)
            .map(|c| (p[c] - f64::from(level.source[*i][c]) / 255.0).powi(2))
            .sum::<f64>();
        if e < (4.0_f64 / 255.0).powi(2) {
            inliers += 1;
            error += e;
        }
    }
    if inliers * 100 < count * 97 || error > inliers as f64 * 6.0 / (255.0 * 255.0) {
        return Ok(None);
    }
    let mut out = level.source.clone();
    for i in holes {
        out[i] = predict(basis(i), coefficients).map(|v| (v * 255.0) as f32);
    }
    Ok(Some(out))
}
fn solve(mut a: [[f64; 3]; 3], mut b: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    for k in 0..3 {
        let p = (k..3).max_by(|i, j| a[*i][k].abs().total_cmp(&a[*j][k].abs()))?;
        if a[p][k].abs() < 1e-12 {
            return None;
        }
        a.swap(k, p);
        b.swap(k, p);
        let pivot = a[k][k];
        for c in 0..3 {
            a[k][c] /= pivot;
            b[k][c] /= pivot;
        }
        for r in 0..3 {
            if r != k {
                let f = a[r][k];
                for c in 0..3 {
                    a[r][c] -= f * a[k][c];
                    b[r][c] -= f * b[k][c];
                }
            }
        }
    }
    Some(b)
}
fn periodic(level: &Level, cancel: &Cancellation) -> Result<Option<Vec<Color>>, CanvasError> {
    let points: Vec<_> = level
        .domain
        .iter()
        .enumerate()
        .filter_map(|(i, d)| d.then_some(i))
        .collect();
    for (dx, dy) in [(1_isize, 0_isize), (0, 1)] {
        for period in 2..=64 {
            cancel.check()?;
            let mut count = 0;
            let mut error = 0.0;
            for (n, i) in points.iter().enumerate() {
                if n % 4096 == 0 {
                    cancel.check()?;
                }
                if let Some(j) = level.at(
                    (i % level.width) as isize + dx * period,
                    (i / level.width) as isize + dy * period,
                ) && level.domain[j]
                {
                    count += 1;
                    error += distance(level.source[*i], level.source[j]);
                    if error > points.len() as f32 * 0.5 {
                        break;
                    }
                }
            }
            if count < 64 || error > count as f32 * 0.5 {
                continue;
            }
            let mut output = level.source.clone();
            let mut complete = true;
            for (n, i) in level.holes().into_iter().enumerate() {
                if n % 64 == 0 {
                    cancel.check()?;
                }
                let mut found = None;
                for n in 1..level.width.max(level.height) as isize / period + 1 {
                    for sign in [-1, 1] {
                        if let Some(j) = level.at(
                            (i % level.width) as isize + dx * period * n * sign,
                            (i / level.width) as isize + dy * period * n * sign,
                        ) && level.domain[j]
                        {
                            found = Some(j);
                            break;
                        }
                    }
                    if found.is_some() {
                        break;
                    }
                }
                if let Some(j) = found {
                    output[i] = level.source[j];
                } else {
                    complete = false;
                    break;
                }
            }
            if complete {
                return Ok(Some(output));
            }
        }
    }
    Ok(None)
}
