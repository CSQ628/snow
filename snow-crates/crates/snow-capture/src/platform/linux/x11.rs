//! X11 root-window capture. Used for X11 sessions and as the last resort on XWayland.

use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat, Window};
use x11rb::rust_connection::RustConnection;

use crate::error::{CaptureError, CaptureResult};
use crate::monitor::{MonitorDesktopGeometry, MonitorId};

use super::pixels::bgrx_to_rgba;

pub(crate) struct Output {
    pub monitor: MonitorId,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub(crate) fn outputs() -> CaptureResult<Vec<Output>> {
    let (conn, screen) = connect()?;
    let root = conn.setup().roots[screen].root;
    if let Ok(outputs) = randr_outputs(&conn, root) {
        if !outputs.is_empty() {
            return Ok(outputs);
        }
    }
    let root_info = &conn.setup().roots[screen];
    Ok(vec![output(0, "screen", true, 0, 0, root_info.width_in_pixels as u32, root_info.height_in_pixels as u32)])
}

pub(crate) fn synthetic_output(width: u32, height: u32) -> Output {
    output(0, "screen", true, 0, 0, width, height)
}

fn output(index: u32, name: &str, primary: bool, x: i32, y: i32, width: u32, height: u32) -> Output {
    Output {
        monitor: MonitorId::from_parts(0, u64::from(index) + 1, index as isize, name, primary)
            .with_desktop_geometry(MonitorDesktopGeometry {
                x: f64::from(x),
                y: f64::from(y),
                width: f64::from(width),
                height: f64::from(height),
                pixel_width: width,
                pixel_height: height,
            }),
        x,
        y,
        width,
        height,
    }
}

fn randr_outputs(conn: &RustConnection, root: Window) -> CaptureResult<Vec<Output>> {
    use x11rb::protocol::randr::{self, ConnectionExt as _};
    let _ = conn.randr_query_version(1, 5).map_err(platform)?.reply().map_err(platform)?;
    let resources = conn
        .randr_get_screen_resources_current(root)
        .map_err(platform)?
        .reply()
        .map_err(platform)?;
    let primary = conn
        .randr_get_output_primary(root)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| reply.output);
    let mut outputs = Vec::new();
    for (index, output_id) in resources.outputs.iter().copied().enumerate() {
        let info = conn
            .randr_get_output_info(output_id, resources.config_timestamp)
            .map_err(platform)?
            .reply()
            .map_err(platform)?;
        if info.crtc == 0 || info.connection != randr::Connection::CONNECTED {
            continue;
        }
        let crtc = conn
            .randr_get_crtc_info(info.crtc, resources.config_timestamp)
            .map_err(platform)?
            .reply()
            .map_err(platform)?;
        if crtc.width == 0 || crtc.height == 0 {
            continue;
        }
        let name = String::from_utf8_lossy(&info.name).to_string();
        outputs.push(output(
            index as u32,
            &name,
            primary == Some(output_id),
            i32::from(crtc.x),
            i32::from(crtc.y),
            u32::from(crtc.width),
            u32::from(crtc.height),
        ));
    }
    if !outputs.iter().any(|output| output.monitor.is_primary()) {
        if let Some(first) = outputs.first_mut() {
            let geometry = first.monitor.desktop_geometry();
            first.monitor = MonitorId::from_parts(
                0,
                1,
                first.monitor.raw_handle(),
                first.monitor.name(),
                true,
            );
            if let Some(geometry) = geometry {
                first.monitor = first.monitor.clone().with_desktop_geometry(geometry);
            }
        }
    }
    Ok(outputs)
}

pub(crate) fn capture_root(x: i32, y: i32, width: u32, height: u32) -> CaptureResult<Vec<u8>> {
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return Err(CaptureError::InvalidConfig(
            "X11 capture rectangle is empty or too large".into(),
        ));
    }
    let (conn, screen) = connect()?;
    let root = conn.setup().roots[screen].root;
    let image = conn
        .get_image(
            ImageFormat::Z_PIXMAP,
            root,
            x as i16,
            y as i16,
            width as u16,
            height as u16,
            u32::MAX,
        )
        .map_err(platform)?
        .reply()
        .map_err(platform)?;
    bgrx_to_rgba(&image.data, width, height, width as usize * 4).ok_or_else(|| {
        CaptureError::UnsupportedFormat("X11 image is not packed 32-bit pixels".into())
    })
}

/// Topmost window that contains the point and is not in `excluded`.
/// Candidates are stacked top-first as `(id, x, y, width, height)`.
pub(crate) fn select_content_window(
    stacked_top_first: &[(u32, i32, i32, u32, u32)],
    x: i32,
    y: i32,
    excluded: &[u32],
) -> Option<u64> {
    stacked_top_first.iter().find_map(|window| {
        let (id, left, top, width, height) = *window;
        let contains = x >= left
            && y >= top
            && x < left.saturating_add(width as i32)
            && y < top.saturating_add(height as i32);
        (contains && !excluded.contains(&id)).then_some(u64::from(id))
    })
}

pub(crate) fn content_window(x: i32, y: i32, excluded: &[u32]) -> CaptureResult<Option<u64>> {
    let (conn, screen) = connect()?;
    let root = conn.setup().roots[screen].root;
    let tree = conn.query_tree(root).map_err(platform)?.reply().map_err(platform)?;
    let mut stacked = Vec::new();
    for child in tree.children.iter().rev() {
        let Ok(geometry) = conn.get_geometry(*child).map_err(platform)?.reply() else {
            continue;
        };
        let Ok(translated) = conn
            .translate_coordinates(*child, root, 0, 0)
            .map_err(platform)?
            .reply()
        else {
            continue;
        };
        stacked.push((
            *child,
            i32::from(translated.dst_x),
            i32::from(translated.dst_y),
            u32::from(geometry.width),
            u32::from(geometry.height),
        ));
    }
    Ok(select_content_window(&stacked, x, y, excluded))
}

#[allow(dead_code)]
pub(crate) fn window_under(x: i32, y: i32) -> CaptureResult<Option<u64>> {
    let (conn, screen) = connect()?;
    let root = conn.setup().roots[screen].root;
    let reply = conn
        .translate_coordinates(root, root, x as i16, y as i16)
        .map_err(platform)?
        .reply()
        .map_err(platform)?;
    let window = reply.child;
    if window == 0 || window == root {
        return Ok(None);
    }
    Ok(Some(u64::from(window)))
}

pub(crate) fn window_geometry(id: u64) -> CaptureResult<(i32, i32, u32, u32)> {
    let window = u32::try_from(id).map_err(|_| {
        CaptureError::InvalidTarget("window id is outside the X11 range".into())
    })?;
    let (conn, screen) = connect()?;
    let root = conn.setup().roots[screen].root;
    let geometry = conn
        .get_geometry(window)
        .map_err(platform)?
        .reply()
        .map_err(platform)?;
    let translated = conn
        .translate_coordinates(window, root, 0, 0)
        .map_err(platform)?
        .reply()
        .map_err(platform)?;
    Ok((
        i32::from(translated.dst_x),
        i32::from(translated.dst_y),
        u32::from(geometry.width),
        u32::from(geometry.height),
    ))
}

fn connect() -> CaptureResult<(RustConnection, usize)> {
    x11rb::connect(None).map_err(|error| {
        CaptureError::platform(anyhow::anyhow!("X11 connection failed: {error}"))
    })
}

fn platform(error: impl std::fmt::Display) -> CaptureError {
    CaptureError::platform(anyhow::anyhow!("{error}"))
}

pub(crate) struct X11Source {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    previous: Option<Vec<u8>>,
}

impl X11Source {
    pub(crate) fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
            previous: None,
        }
    }

    pub(crate) fn next_rgba(&mut self, _timeout: Duration) -> CaptureResult<(Vec<u8>, bool)> {
        let pixels = capture_root(self.x, self.y, self.width, self.height)?;
        let duplicate = self.previous.as_ref() == Some(&pixels);
        if !duplicate {
            self.previous = Some(pixels.clone());
        }
        Ok((pixels, duplicate))
    }
}

#[cfg(test)]
mod tests {
    use super::select_content_window;

    #[test]
    fn excluded_overlay_is_not_the_scroll_source() {
        let stacked = [(10, 0, 0, 100, 100), (20, 0, 0, 100, 80)];
        assert_eq!(select_content_window(&stacked, 4, 4, &[10]), Some(20));
        assert_eq!(select_content_window(&stacked, 4, 90, &[10]), None);
        assert_eq!(select_content_window(&stacked, 4, 4, &[]), Some(10));
    }
}
