//! ScreenCast sources. The portal is tried first, with a short wait so a
//! permission dialog does not stall capture. Mutter's ScreenCast then records
//! a rectangle without a dialog. The caller falls back to X11 after both.

use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use super::pipewire::{Pump, Region};

const PORTAL_WAIT: Duration = Duration::from_millis(1500);
const FRAME_WAIT: Duration = Duration::from_millis(800);

struct StreamInfo {
    node_id: u32,
    x: i32,
    y: i32,
}

pub(crate) fn open(x: i32, y: i32, width: u32, height: u32, window: Option<u64>) -> Option<Pump> {
    let region = Region {
        stream_x: x,
        stream_y: y,
        x,
        y,
        width,
        height,
    };
    if let Some(window) = window {
        let mut options = HashMap::new();
        options.insert("window-id".to_owned(), Value::U64(window));
        options.insert("cursor-mode".to_owned(), Value::U32(0));
        let mut window_region = region;
        if let Ok((origin_x, origin_y, _, _)) = super::x11::window_geometry(window) {
            window_region.stream_x = origin_x;
            window_region.stream_y = origin_y;
        }
        if let Some(pump) = mutter("RecordWindow", &options, window_region) {
            return Some(pump);
        }
    }
    if let Some(pump) = portal(region) {
        return Some(pump);
    }
    let mut options = HashMap::new();
    options.insert("cursor-mode".to_owned(), Value::U32(0));
    mutter(
        "RecordArea",
        &(x, y, width as i32, height as i32, options),
        region,
    )
}

fn portal(region: Region) -> Option<Pump> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = sender.send(portal_grant());
    });
    let grant = receiver.recv_timeout(PORTAL_WAIT).ok()?.ok()?;
    let region = Region {
        stream_x: grant.stream.x,
        stream_y: grant.stream.y,
        ..region
    };
    let mut pump = Pump::start(
        grant.fd,
        Some(grant.stream.node_id),
        region,
        Some(grant.connection),
    )
    .ok()?;
    pump.wait_ready(FRAME_WAIT).ok()?;
    Some(pump)
}

struct PortalGrant {
    connection: Connection,
    fd: OwnedFd,
    stream: StreamInfo,
}

fn portal_grant() -> Result<PortalGrant, String> {
    let connection = Connection::session().map_err(|error| error.to_string())?;
    let unique = connection
        .unique_name()
        .ok_or_else(|| "portal connection has no unique name".to_owned())?
        .as_str()
        .trim_start_matches(':')
        .replace('.', "_");
    let screencast = Proxy::new(
        &connection,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.ScreenCast",
    )
    .map_err(|error| error.to_string())?;
    let create_token = token();
    let session_token = token();
    let mut create = HashMap::new();
    create.insert("handle_token".to_owned(), string_value(&create_token));
    create.insert(
        "session_handle_token".to_owned(),
        string_value(&session_token),
    );
    let created = call_request(
        &connection,
        &screencast,
        "CreateSession",
        &create,
        &unique,
        &create_token,
    )?;
    let session = object_path(created.get("session_handle")).unwrap_or_else(|| {
        OwnedObjectPath::try_from(format!(
            "/org/freedesktop/portal/desktop/session/{unique}/{session_token}"
        ))
        .expect("session path")
    });
    let sources_token = token();
    let mut sources = HashMap::new();
    sources.insert("handle_token".to_owned(), string_value(&sources_token));
    sources.insert("types".to_owned(), Value::U32(1));
    sources.insert("cursor_mode".to_owned(), Value::U32(1));
    sources.insert("multiple".to_owned(), Value::Bool(false));
    call_request(
        &connection,
        &screencast,
        "SelectSources",
        &(session.clone(), sources),
        &unique,
        &sources_token,
    )?;
    let start_token = token();
    let mut start = HashMap::new();
    start.insert("handle_token".to_owned(), string_value(&start_token));
    let started = call_request(
        &connection,
        &screencast,
        "Start",
        &(session.clone(), "", start),
        &unique,
        &start_token,
    )?;
    let stream = started
        .get("streams")
        .and_then(|value| first_stream(value.clone()))
        .ok_or_else(|| "portal screencast returned no stream".to_owned())?;
    let remote: zbus::zvariant::OwnedFd = screencast
        .call(
            "OpenPipeWireRemote",
            &(session, HashMap::<String, Value>::new()),
        )
        .map_err(|error| error.to_string())?;
    Ok(PortalGrant {
        connection,
        fd: OwnedFd::from(remote),
        stream,
    })
}

fn call_request<B>(
    connection: &Connection,
    screencast: &Proxy<'_>,
    method: &str,
    body: &B,
    sender: &str,
    handle: &str,
) -> Result<HashMap<String, OwnedValue>, String>
where
    B: serde::ser::Serialize + zbus::zvariant::DynamicType,
{
    let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{handle}");
    let request = Proxy::new(
        connection,
        "org.freedesktop.portal.Desktop",
        path.as_str(),
        "org.freedesktop.portal.Request",
    )
    .map_err(|error| error.to_string())?;
    let mut signals = request
        .receive_signal("Response")
        .map_err(|error| error.to_string())?;
    let _returned: OwnedObjectPath = screencast
        .call(method, body)
        .map_err(|error| error.to_string())?;
    let message = signals
        .next()
        .ok_or_else(|| format!("portal {method} produced no response"))?;
    let (code, results): (u32, HashMap<String, OwnedValue>) = message
        .body()
        .deserialize()
        .map_err(|error| error.to_string())?;
    if code == 1 {
        return Err("portal screencast was cancelled".to_owned());
    }
    if code != 0 {
        return Err(format!("portal {method} failed with response {code}"));
    }
    Ok(results)
}

fn mutter<B>(method: &str, body: &B, region: Region) -> Option<Pump>
where
    B: serde::ser::Serialize + zbus::zvariant::DynamicType,
{
    let connection = match Connection::session() {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("mutter session bus: {error}");
            return None;
        }
    };
    let root = match Proxy::new(
        &connection,
        "org.gnome.Mutter.ScreenCast",
        "/org/gnome/Mutter/ScreenCast",
        "org.gnome.Mutter.ScreenCast",
    ) {
        Ok(root) => root,
        Err(error) => {
            eprintln!("mutter proxy: {error}");
            return None;
        }
    };
    let empty: HashMap<String, Value> = HashMap::new();
    let session: OwnedObjectPath = match root.call("CreateSession", &empty) {
        Ok(session) => session,
        Err(error) => {
            eprintln!("mutter CreateSession: {error}");
            return None;
        }
    };
    let session_proxy = match Proxy::new(
        &connection,
        "org.gnome.Mutter.ScreenCast",
        session.as_str(),
        "org.gnome.Mutter.ScreenCast.Session",
    ) {
        Ok(proxy) => proxy,
        Err(error) => {
            eprintln!("mutter session proxy: {error}");
            return None;
        }
    };
    let stream_path: OwnedObjectPath = match session_proxy.call(method, body) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("mutter {method}: {error}");
            return None;
        }
    };
    let stream = match Proxy::new(
        &connection,
        "org.gnome.Mutter.ScreenCast",
        stream_path.as_str(),
        "org.gnome.Mutter.ScreenCast.Stream",
    ) {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("mutter stream proxy: {error}");
            return None;
        }
    };
    let added = stream.receive_signal("PipeWireStreamAdded").ok();
    if let Err(error) = session_proxy.call::<_, _, ()>("Start", &()) {
        eprintln!("mutter session start: {error}");
        return None;
    }
    let node_id = added.and_then(|mut signals| {
        let message = signals.next()?;
        message.body().deserialize::<u32>().ok()
    });
    let remote = if let Some(node_id) = node_id {
        match pipewire_socket() {
            Some(fd) => (fd, Some(node_id)),
            None => return None,
        }
    } else {
        match stream.call::<_, _, zbus::zvariant::OwnedFd>("Start", &()) {
            Ok(fd) => (OwnedFd::from(fd), None),
            Err(error) => {
                eprintln!("mutter stream start: {error}");
                return None;
            }
        }
    };
    let mut pump = match Pump::start(remote.0, remote.1, region, Some(connection)) {
        Ok(pump) => pump,
        Err(error) => {
            eprintln!("mutter pipewire: {error}");
            return None;
        }
    };
    if let Err(error) = pump.wait_ready(FRAME_WAIT) {
        eprintln!("mutter frame: {error}");
        return None;
    }
    Some(pump)
}

fn pipewire_socket() -> Option<OwnedFd> {
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok()?;
    let remote = std::env::var("PIPEWIRE_REMOTE").unwrap_or_else(|_| "pipewire-0".to_owned());
    let path = if remote.starts_with('/') {
        remote
    } else {
        format!("{runtime}/{remote}")
    };
    UnixStream::connect(path).ok().map(OwnedFd::from)
}

fn token() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!(
        "snow{}{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

fn string_value(text: &str) -> Value<'static> {
    Value::new(text.to_owned())
}

fn object_path(value: Option<&OwnedValue>) -> Option<OwnedObjectPath> {
    let value: Value = value?.try_clone().ok()?.into();
    match value {
        Value::ObjectPath(path) => OwnedObjectPath::try_from(path.as_str()).ok(),
        Value::Str(text) => OwnedObjectPath::try_from(text.as_str()).ok(),
        _ => None,
    }
}

fn first_stream(value: OwnedValue) -> Option<StreamInfo> {
    let value: Value = value.into();
    let Value::Array(array) = value else {
        return None;
    };
    let item: Value = array.get(0).ok()??;
    let Value::Structure(structure) = item else {
        return None;
    };
    let fields = structure.fields();
    let node_id = match fields.first()? {
        Value::U32(id) => *id,
        _ => return None,
    };
    let mut info = StreamInfo { node_id, x: 0, y: 0 };
    if let Some(Value::Dict(dict)) = fields.get(1) {
        if let Some((x, y)) = dict_pair(dict, "position") {
            info.x = x;
            info.y = y;
        }
    }
    Some(info)
}

fn dict_pair(dict: &zbus::zvariant::Dict, key: &str) -> Option<(i32, i32)> {
    let value: Value = dict.get(&key).ok()??;
    let Value::Structure(structure) = value else {
        return None;
    };
    let fields = structure.fields();
    Some((as_i32(fields.first()?)?, as_i32(fields.get(1)?)?))
}

fn as_i32(value: &Value) -> Option<i32> {
    match value {
        Value::I32(value) => Some(*value),
        Value::U32(value) => i32::try_from(*value).ok(),
        _ => None,
    }
}
