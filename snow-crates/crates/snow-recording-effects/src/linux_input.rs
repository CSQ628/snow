//! XRecord listens to key and pointer events. On Wayland this observes clients
//! that pass through XWayland; native Wayland input is not injected.

use std::ffi::c_char;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Instant;

#[repr(C)]
struct Intercept {
    _id_base: u64,
    _server_time: u64,
    _client_seq: u64,
    category: i32,
    _client_swapped: i32,
    data: *mut u8,
    data_len: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct Recorded {
    pub kind: u8,
    pub detail: u8,
    pub x: i32,
    pub y: i32,
    pub state: u16,
    pub keysym: u32,
    pub at: Instant,
}

struct CallbackState {
    sender: mpsc::SyncSender<Recorded>,
    display: *mut u8,
}

pub(crate) struct Recorder {
    stop: Sender<()>,
    events: Option<Receiver<Recorded>>,
    worker: Option<JoinHandle<()>>,
}

impl Recorder {
    pub fn start() -> Result<Self, String> {
        let (event_tx, event_rx) = mpsc::sync_channel(256);
        let (stop_tx, stop_rx) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("snow-xrecord".into())
            .spawn(move || record_loop(event_tx, stop_rx))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            stop: stop_tx,
            events: Some(event_rx),
            worker: Some(worker),
        })
    }

    pub fn take_events(&mut self) -> Receiver<Recorded> {
        self.events.take().expect("record events")
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn record_loop(sender: mpsc::SyncSender<Recorded>, stop: Receiver<()>) {
    unsafe {
        XInitThreads();
        let control = XOpenDisplay(std::ptr::null());
        let data = XOpenDisplay(std::ptr::null());
        if control.is_null() || data.is_null() {
            return;
        }
        let range = XRecordAllocRange();
        if range.is_null() {
            XCloseDisplay(control);
            XCloseDisplay(data);
            return;
        }
        *range.add(18) = 2;
        *range.add(19) = 6;
        let mut clients: u64 = 3;
        let mut range_ptr = range;
        let context = XRecordCreateContext(control, 0, &mut clients, 1, &mut range_ptr, 1);
        let state = Box::new(CallbackState {
            sender,
            display: data.cast(),
        });
        let state_ptr = Box::into_raw(state);
        if XRecordEnableContextAsync(data, context, on_intercept, state_ptr.cast()) == 0 {
            drop(Box::from_raw(state_ptr));
            XCloseDisplay(control);
            XCloseDisplay(data);
            return;
        }
        let fd = XConnectionNumber(data);
        while stop.try_recv().is_err() {
            let mut poll = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            if libc::poll(&mut poll, 1, 50) > 0 {
                XRecordProcessReplies(data);
            }
        }
        XRecordDisableContext(control, context);
        XRecordFreeContext(control, context);
        XFlush(control);
        XCloseDisplay(control);
        XCloseDisplay(data);
        drop(Box::from_raw(state_ptr));
    }
}

unsafe extern "C" fn on_intercept(closure: *mut c_char, data: *mut Intercept) {
    unsafe {
        if closure.is_null() || data.is_null() || (*data).category != 0 || (*data).data.is_null() {
            return;
        }
        let bytes = (*data).data_len as usize * 4;
        if bytes < 32 {
            return;
        }
        let event = std::slice::from_raw_parts((*data).data, bytes);
        let state = &*closure.cast::<CallbackState>();
        let keycode = event[1];
        let keysym = XKeycodeToKeysym(state.display.cast(), keycode, 0) as u32;
        let x = i16::from_ne_bytes([event[20], event[21]]) as i32;
        let y = i16::from_ne_bytes([event[22], event[23]]) as i32;
        let modifiers = u16::from_ne_bytes([event[28], event[29]]);
        let _ = state.sender.try_send(Recorded {
            kind: event[0],
            detail: keycode,
            x,
            y,
            state: modifiers,
            keysym,
            at: Instant::now(),
        });
    }
}

unsafe extern "C" {
    fn XInitThreads() -> i32;
    fn XOpenDisplay(name: *const c_char) -> *mut u8;
    fn XCloseDisplay(display: *mut u8) -> i32;
    fn XFlush(display: *mut u8) -> i32;
    fn XConnectionNumber(display: *mut u8) -> i32;
    fn XKeycodeToKeysym(display: *mut u8, keycode: u8, index: i32) -> u64;
    fn XKeysymToString(keysym: u64) -> *const c_char;
    fn XRecordAllocRange() -> *mut u8;
    fn XRecordCreateContext(
        display: *mut u8,
        datum_flags: i32,
        clients: *mut u64,
        nclients: i32,
        ranges: *mut *mut u8,
        nranges: i32,
    ) -> u64;
    fn XRecordEnableContextAsync(
        display: *mut u8,
        context: u64,
        callback: unsafe extern "C" fn(*mut c_char, *mut Intercept),
        closure: *mut c_char,
    ) -> i32;
    fn XRecordProcessReplies(display: *mut u8);
    fn XRecordDisableContext(display: *mut u8, context: u64) -> i32;
    fn XRecordFreeContext(display: *mut u8, context: u64) -> i32;
}

pub(crate) fn keysym_name(keysym: u32) -> Option<String> {
    unsafe {
        let name = XKeysymToString(u64::from(keysym));
        if name.is_null() {
            return None;
        }
        Some(std::ffi::CStr::from_ptr(name).to_string_lossy().into_owned())
    }
}
