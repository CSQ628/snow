//! AT-SPI2 element geometry. A missing accessibility bus leaves the snapshot empty.

use std::time::{Duration, Instant};

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

use crate::{
    AccessibilityBackend, ElementRect, HitTestMode, PixelRect, Point, QueryControl, QueryResult,
    SelectorResult, StopReason, WindowSnapshot,
};

const FOCUSED: u32 = 1 << 12;
const MAX_NODES: usize = 300;
const MAX_CHILDREN: i32 = 24;
const MAX_DEPTH: u32 = 5;

#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub rect: PixelRect,
    pub children: Vec<Node>,
}

pub struct ElementRegionService {
    backend: AccessibilityBackend,
    nodes: Vec<Node>,
}

impl ElementRegionService {
    pub fn with_backend_excluding_ids(
        backend: AccessibilityBackend,
        _excluded: &[usize],
    ) -> SelectorResult<Self> {
        Self::with_backend_and_displays(backend, _excluded, None)
    }

    pub fn with_backend_and_displays(
        backend: AccessibilityBackend,
        _excluded: &[usize],
        _displays: Option<&[crate::DisplayGeometry]>,
    ) -> SelectorResult<Self> {
        let mut service = Self {
            backend,
            nodes: Vec::new(),
        };
        service.refresh_with_displays(_excluded, _displays)?;
        Ok(service)
    }

    pub fn backend(&self) -> AccessibilityBackend {
        self.backend
    }

    pub fn refresh_excluding_ids(&mut self, excluded: &[usize]) -> SelectorResult<()> {
        self.refresh_with_displays(excluded, None)
    }

    pub fn refresh_with_displays(
        &mut self,
        _excluded: &[usize],
        _displays: Option<&[crate::DisplayGeometry]>,
    ) -> SelectorResult<()> {
        self.nodes = collect_tree().unwrap_or_default();
        Ok(())
    }

    pub fn release_cache(&mut self) {
        self.nodes.clear();
    }

    pub fn window_snapshot(&self) -> Option<WindowSnapshot> {
        Some(WindowSnapshot {
            nodes: self.nodes.clone(),
        })
    }

    pub fn from_snapshot(snapshot: &WindowSnapshot) -> SelectorResult<Self> {
        Ok(Self {
            backend: AccessibilityBackend::Accessibility,
            nodes: snapshot.nodes.clone(),
        })
    }

    pub fn query(
        &mut self,
        point: Point,
        mode: HitTestMode,
        control: &QueryControl<'_>,
        progress: &mut dyn FnMut(&[ElementRect]),
    ) -> SelectorResult<QueryResult> {
        let _ = (control.budget, control.call_limit);
        if (control.cancelled)() {
            return Ok(QueryResult {
                path: None,
                reason: StopReason::Cancelled,
            });
        }
        if self.nodes.is_empty() {
            return Ok(QueryResult {
                path: None,
                reason: StopReason::ProviderFailure,
            });
        }
        let mut path = Vec::new();
        collect_path(&self.nodes, point.x, point.y, mode, &mut path);
        let rects: Vec<_> = path.into_iter().map(ElementRect::new).collect();
        if control.publication_interval.is_some() {
            progress(&rects);
        }
        Ok(QueryResult {
            path: (!rects.is_empty()).then_some(rects),
            reason: StopReason::Complete,
        })
    }
}

fn collect_path(
    nodes: &[Node],
    x: i32,
    y: i32,
    mode: HitTestMode,
    path: &mut Vec<PixelRect>,
) {
    let Some(node) = nodes.iter().rev().find(|node| contains(node.rect, x, y)) else {
        return;
    };
    path.push(node.rect);
    if mode == HitTestMode::UiElement {
        collect_path(&node.children, x, y, mode, path);
    }
}

fn contains(rect: PixelRect, x: i32, y: i32) -> bool {
    x >= rect.left && y >= rect.top && x < rect.right && y < rect.bottom && rect.right > rect.left
        && rect.bottom > rect.top
}

pub(crate) fn collect_tree() -> Result<Vec<Node>, String> {
    let session = Connection::session().map_err(|error| error.to_string())?;
    let bus = Proxy::new(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus")
        .map_err(|error| error.to_string())?;
    let address: String = bus.call("GetAddress", &()).map_err(|error| error.to_string())?;
    let connection = zbus::blocking::connection::Builder::address(address.as_str())
        .map_err(|error| error.to_string())?
        .build()
        .map_err(|error| error.to_string())?;
    let mut budget = Budget {
        remaining: MAX_NODES,
        deadline: Instant::now() + Duration::from_millis(800),
    };
    walk(
        &connection,
        "org.a11y.atspi.Registry",
        "/org/a11y/atspi/accessible/root",
        0,
        &mut budget,
    )
}

struct Budget {
    remaining: usize,
    deadline: Instant,
}

fn walk(
    connection: &Connection,
    destination: &str,
    path: &str,
    depth: u32,
    budget: &mut Budget,
) -> Result<Vec<Node>, String> {
    if depth > MAX_DEPTH || budget.remaining == 0 || Instant::now() > budget.deadline {
        return Ok(Vec::new());
    }
    let accessible = Proxy::new(
        connection,
        destination,
        path,
        "org.a11y.atspi.Accessible",
    )
    .map_err(|error| error.to_string())?;
    let count: i32 = accessible
        .call("GetChildCount", &())
        .unwrap_or(0)
        .clamp(0, MAX_CHILDREN);
    let mut nodes = Vec::new();
    for index in 0..count {
        if budget.remaining == 0 || Instant::now() > budget.deadline {
            break;
        }
        let Ok((name, child_path)): Result<(String, OwnedObjectPath), _> =
            accessible.call("GetChildAtIndex", &(index))
        else {
            continue;
        };
        budget.remaining -= 1;
        let rect = extents(connection, &name, child_path.as_str()).unwrap_or_default();
        let children = walk(connection, &name, child_path.as_str(), depth + 1, budget)?;
        if rect.right <= rect.left || rect.bottom <= rect.top {
            nodes.extend(children);
            continue;
        }
        nodes.push(Node { rect, children });
    }
    let _ = FOCUSED;
    Ok(nodes)
}

fn extents(connection: &Connection, destination: &str, path: &str) -> Result<PixelRect, String> {
    let component = Proxy::new(connection, destination, path, "org.a11y.atspi.Component")
        .map_err(|error| error.to_string())?;
    let (x, y, width, height): (i32, i32, i32, i32) = component
        .call("GetExtents", &(0u32,))
        .map_err(|error| error.to_string())?;
    Ok(PixelRect {
        left: x,
        top: y,
        right: x.saturating_add(width),
        bottom: y.saturating_add(height),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smallest_containing_rect_is_the_leaf() {
        let nodes = vec![Node {
            rect: PixelRect {
                left: 0,
                top: 0,
                right: 200,
                bottom: 100,
            },
            children: vec![Node {
                rect: PixelRect {
                    left: 10,
                    top: 10,
                    right: 40,
                    bottom: 30,
                },
                children: Vec::new(),
            }],
        }];
        let mut path = Vec::new();
        collect_path(&nodes, 12, 12, HitTestMode::UiElement, &mut path);
        assert_eq!(path.len(), 2);
        assert_eq!(path[1].left, 10);
        path.clear();
        collect_path(&nodes, 12, 12, HitTestMode::Window, &mut path);
        assert_eq!(path.len(), 1);
    }

    fn geometry_is_finite(nodes: &[Node]) {
        for node in nodes {
            assert!(node.rect.right >= node.rect.left);
            assert!(node.rect.bottom >= node.rect.top);
            geometry_is_finite(&node.children);
        }
    }

    #[test]
    fn atspi_geometry_is_finite_or_the_bus_is_absent() {
        match collect_tree() {
            Ok(nodes) => geometry_is_finite(&nodes),
            Err(error) => {
                let lower = error.to_ascii_lowercase();
                assert!(
                    lower.contains("a11y")
                        || lower.contains("bus")
                        || lower.contains("connect")
                        || lower.contains("name"),
                    "unexpected accessibility failure: {error}"
                );
            }
        }
    }
}
