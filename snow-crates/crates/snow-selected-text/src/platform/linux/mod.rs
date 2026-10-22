//! AT-SPI2 selected text. Clipboard fallback reads the X11/Wayland primary selection.

use std::process::Command;
use std::time::Instant;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

use crate::model::*;
use crate::policy::{Backend, Context, Probe};

const FOCUSED: u32 = 1 << 12;

pub(crate) fn capture_source() -> Result<SourceApplication, SelectionError> {
    let Some(source) = focused_application() else {
        return Err(SelectionError::new(
            ErrorKind::NoForegroundWindow,
            "atspi focus",
        ));
    };
    Ok(source)
}

pub(crate) struct Worker;

impl Worker {
    pub fn new() -> Result<Self, SelectionError> {
        Ok(Self)
    }
}

impl Backend for Worker {
    fn validate_target(&mut self, context: &Context) -> Result<(), SelectionError> {
        context.check()?;
        let Some(source) = focused_application() else {
            return Err(SelectionError::new(
                ErrorKind::TargetChanged,
                "atspi focus",
            ));
        };
        if source.process_id != context.source.process_id
            || !source
                .executable
                .eq_ignore_ascii_case(&context.source.executable)
        {
            return Err(SelectionError::new(
                ErrorKind::TargetChanged,
                "focused application identity",
            ));
        }
        Ok(())
    }

    fn accessibility(
        &mut self,
        context: &Context,
        deadline: Instant,
    ) -> Result<Probe, SelectionError> {
        context.check()?;
        let Some(text) = selected_text(deadline) else {
            return Ok(Probe::Empty);
        };
        if text.is_empty() {
            return Ok(Probe::Empty);
        }
        Ok(Probe::Selected(vec![SelectedRange {
            text,
            bounds: Vec::new(),
        }]))
    }

    fn native_control(
        &mut self,
        _context: &Context,
        _deadline: Instant,
    ) -> Result<Probe, SelectionError> {
        Ok(Probe::Unsupported)
    }

    fn copy(&mut self, context: &Context) -> CaptureResult {
        context.check()?;
        let Some(text) = primary_selection() else {
            return Ok(SelectionOutcome::Unsupported);
        };
        crate::model::assemble(
            vec![SelectedRange {
                text,
                bounds: Vec::new(),
            }],
            context.source.clone(),
            RetrievalMethod::Clipboard,
            ClipboardStatus::Unchanged,
            context.options.max_text_bytes,
        )
    }
}

fn focused_application() -> Option<SourceApplication> {
    let connection = a11y()?;
    let root = proxy(
        &connection,
        "org.a11y.atspi.Registry",
        "/org/a11y/atspi/accessible/root",
        "org.a11y.atspi.Accessible",
    )?;
    let count: i32 = root.call("GetChildCount", &()).ok()?;
    for index in 0..count.min(16) {
        let (name, path): (String, OwnedObjectPath) = root.call("GetChildAtIndex", &(index)).ok()?;
        if let Some(source) = application_identity(&connection, &name, path.as_str()) {
            return Some(source);
        }
    }
    None
}

fn application_identity(
    connection: &Connection,
    destination: &str,
    path: &str,
) -> Option<SourceApplication> {
    let accessible = proxy(
        connection,
        destination,
        path,
        "org.a11y.atspi.Accessible",
    )?;
    let (app_name, app_path): (String, OwnedObjectPath) =
        accessible.call("GetApplication", &()).ok()?;
    let properties = proxy(
        connection,
        &app_name,
        app_path.as_str(),
        "org.freedesktop.DBus.Properties",
    )?;
    let id: zbus::zvariant::OwnedValue = properties
        .call("Get", &("org.a11y.atspi.Application", "Id"))
        .ok()?;
    let process_id = match zbus::zvariant::Value::from(id) {
        zbus::zvariant::Value::U32(value) => value,
        zbus::zvariant::Value::I32(value) if value > 0 => value as u32,
        _ => return None,
    };
    if process_id == 0 {
        return None;
    }
    let executable = std::fs::read_to_string(format!("/proc/{process_id}/comm"))
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())?;
    Some(SourceApplication {
        native_window: None,
        process_id,
        native_focus: None,
        executable,
    })
}

fn selected_text(deadline: Instant) -> Option<String> {
    let connection = a11y()?;
    let mut found = None;
    walk_selection(
        &connection,
        "org.a11y.atspi.Registry",
        "/org/a11y/atspi/accessible/root",
        0,
        deadline,
        &mut found,
    );
    found
}

fn walk_selection(
    connection: &Connection,
    destination: &str,
    path: &str,
    depth: u32,
    deadline: Instant,
    found: &mut Option<String>,
) {
    if found.is_some() || depth > 6 || Instant::now() > deadline {
        return;
    }
    let Some(accessible) = proxy(connection, destination, path, "org.a11y.atspi.Accessible") else {
        return;
    };
    let states: Vec<u32> = accessible.call("GetState", &()).unwrap_or_default();
    if states.first().is_some_and(|word| word & FOCUSED != 0)
        && let Some(text) = text_selection(connection, destination, path)
    {
        *found = Some(text);
        return;
    }
    let count: i32 = accessible.call("GetChildCount", &()).unwrap_or(0);
    for index in 0..count.min(32) {
        let Ok((name, child)): Result<(String, OwnedObjectPath), _> =
            accessible.call("GetChildAtIndex", &(index))
        else {
            continue;
        };
        walk_selection(
            connection,
            &name,
            child.as_str(),
            depth + 1,
            deadline,
            found,
        );
        if found.is_some() {
            return;
        }
    }
}

fn text_selection(connection: &Connection, destination: &str, path: &str) -> Option<String> {
    let text = proxy(connection, destination, path, "org.a11y.atspi.Text")?;
    let count: i32 = text.call("GetNSelections", &()).ok()?;
    if count <= 0 {
        return None;
    }
    let (start, end): (i32, i32) = text.call("GetSelection", &(0i32,)).ok()?;
    if end <= start {
        return None;
    }
    let value: String = text.call("GetText", &(start, end)).ok()?;
    (!value.is_empty()).then_some(value)
}

fn primary_selection() -> Option<String> {
    for (program, args) in [
        ("wl-paste", &["--primary", "--no-newline"][..]),
        ("xclip", &["-selection", "primary", "-o"][..]),
        ("xsel", &["-p", "-o"][..]),
    ] {
        let Ok(output) = Command::new(program).args(args).output() else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

fn a11y() -> Option<Connection> {
    let session = Connection::session().ok()?;
    let bus = proxy(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus")?;
    let address: String = bus.call("GetAddress", &()).ok()?;
    zbus::blocking::connection::Builder::address(address.as_str())
        .ok()?
        .build()
        .ok()
}

fn proxy<'a>(
    connection: &'a Connection,
    destination: &'a str,
    path: &'a str,
    interface: &'a str,
) -> Option<Proxy<'a>> {
    Proxy::new(connection, destination, path, interface).ok()
}
