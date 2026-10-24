//! Native HTTP/2 session primitives backed by the Node-pinned nghttp2 sources.
//!
//! This layer deliberately owns no sockets or JavaScript objects. Hosts feed
//! received bytes, consume protocol events, and write the session's outbound
//! bytes through their existing TCP/TLS paths.

use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum H2Event {
    Headers {
        stream_id: i32,
        headers: Vec<(Vec<u8>, Vec<u8>)>,
        end_stream: bool,
    },
    PushPromise {
        stream_id: i32,
        parent_stream_id: i32,
        headers: Vec<(Vec<u8>, Vec<u8>)>,
    },
    Data {
        stream_id: i32,
        bytes: Vec<u8>,
        end_stream: bool,
    },
    StreamClose {
        stream_id: i32,
        error_code: u32,
    },
}

struct Session(*mut c_void);

// Sessions are never accessed outside SESSIONS' mutex, including their C
// callback queues, so moving the owning pointer between lock holders is safe.
unsafe impl Send for Session {}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static SESSIONS: OnceLock<Mutex<HashMap<u64, Session>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<u64, Session>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn with_session<T>(id: u64, f: impl FnOnce(*mut c_void) -> Result<T, String>) -> Result<T, String> {
    let sessions = sessions().lock().unwrap_or_else(|error| error.into_inner());
    let session = sessions
        .get(&id)
        .ok_or_else(|| format!("unknown HTTP/2 session {id}"))?;
    f(session.0)
}

fn error_text(code: c_int) -> String {
    // nghttp2_strerror returns a static string for every protocol error code.
    unsafe {
        let ptr = qh2_error_string(code);
        if ptr.is_null() {
            format!("nghttp2 error {code}")
        } else {
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}

fn check(code: c_int) -> Result<(), String> {
    if code < 0 {
        Err(error_text(code))
    } else {
        Ok(())
    }
}

/// Create an independent server (`server = true`) or client session.
pub(crate) fn new_session(server: bool) -> Result<u64, String> {
    let ptr = unsafe { qh2_new(i32::from(server)) };
    if ptr.is_null() {
        return Err("could not allocate nghttp2 session".to_owned());
    }
    let id = NEXT_ID
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .map_err(|_| {
            unsafe { qh2_free(ptr) };
            "HTTP/2 session identifier space exhausted".to_owned()
        })?;
    sessions()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(id, Session(ptr));
    Ok(id)
}

/// Configure the local connection receive window before the session's first
/// SETTINGS frame is drained. nghttp2 then emits the required WINDOW_UPDATE
/// and reports effective/local/remote window sizes consistently.
pub(crate) fn set_local_window_size(id: u64, size: i32) -> Result<(), String> {
    with_session(id, |session| check(unsafe { qh2_set_local_window_size(session, size) }))
}

pub(crate) fn submit_rst_stream(
    id: u64,
    stream_id: i32,
    error_code: u32,
) -> Result<(), String> {
    with_session(id, |session| {
        check(unsafe { qh2_submit_rst_stream(session, stream_id, error_code) })
    })
}

/// Feed bytes received from the socket and return all completed protocol events.
pub(crate) fn feed(id: u64, bytes: &[u8]) -> Result<Vec<H2Event>, String> {
    with_session(id, |session| {
        let consumed = unsafe { qh2_mem_recv(session, bytes.as_ptr(), bytes.len()) };
        if consumed < 0 {
            return Err(error_text(consumed as c_int));
        }
        if consumed as usize != bytes.len() {
            return Err(format!(
                "nghttp2 consumed {consumed} of {} input bytes",
                bytes.len()
            ));
        }
        let mut events = Vec::new();
        loop {
            let event = unsafe { qh2_next_event(session) };
            if event.is_null() {
                break;
            }
            let kind = unsafe { qh2_event_kind(event) };
            let stream_id = unsafe { qh2_event_stream_id(event) };
            let converted = match kind {
                1 | 4 => {
                    let count = unsafe { qh2_event_header_count(event) };
                    let mut headers = Vec::with_capacity(count);
                    for index in 0..count {
                        let mut name = std::ptr::null();
                        let mut name_len = 0;
                        let mut value = std::ptr::null();
                        let mut value_len = 0;
                        let result = unsafe {
                            qh2_event_header(
                                event,
                                index,
                                &mut name,
                                &mut name_len,
                                &mut value,
                                &mut value_len,
                            )
                        };
                        if result != 0
                            || (name.is_null() && name_len != 0)
                            || (value.is_null() && value_len != 0)
                        {
                            unsafe { qh2_event_free(event) };
                            return Err("invalid nghttp2 header event".to_owned());
                        }
                        let name = unsafe { std::slice::from_raw_parts(name, name_len) }.to_vec();
                        let value =
                            unsafe { std::slice::from_raw_parts(value, value_len) }.to_vec();
                        headers.push((name, value));
                    }
                    if kind == 4 {
                        H2Event::PushPromise {
                            stream_id,
                            parent_stream_id: unsafe { qh2_event_parent_stream_id(event) },
                            headers,
                        }
                    } else {
                        H2Event::Headers {
                            stream_id,
                            headers,
                            end_stream: unsafe { qh2_event_end_stream(event) } != 0,
                        }
                    }
                }
                2 => {
                    let len = unsafe { qh2_event_data_len(event) };
                    let data = unsafe { qh2_event_data(event) };
                    if data.is_null() && len != 0 {
                        unsafe { qh2_event_free(event) };
                        return Err("invalid nghttp2 data event".to_owned());
                    }
                    H2Event::Data {
                        stream_id,
                        bytes: unsafe { std::slice::from_raw_parts(data, len) }.to_vec(),
                        end_stream: unsafe { qh2_event_end_stream(event) } != 0,
                    }
                }
                3 => H2Event::StreamClose {
                    stream_id,
                    error_code: unsafe { qh2_event_code(event) },
                },
                _ => {
                    unsafe { qh2_event_free(event) };
                    return Err(format!("unknown nghttp2 event kind {kind}"));
                }
            };
            unsafe { qh2_event_free(event) };
            events.push(converted);
        }
        Ok(events)
    })
}

/// Collect all currently serializable bytes, including initial preface/settings.
pub(crate) fn drain_outbound(id: u64) -> Result<Vec<u8>, String> {
    with_session(id, |session| {
        let mut output = Vec::new();
        loop {
            let mut data = std::ptr::null();
            let len = unsafe { qh2_mem_send(session, &mut data) };
            if len < 0 {
                return Err(error_text(len as c_int));
            }
            if len == 0 {
                break;
            }
            if data.is_null() {
                return Err("nghttp2 returned a null output buffer".to_owned());
            }
            let chunk = unsafe { std::slice::from_raw_parts(data, len as usize) };
            output.extend_from_slice(chunk);
        }
        Ok(output)
    })
}

/// Submit a HEADERS frame, commonly a response or trailer block.
pub(crate) fn submit_headers(
    id: u64,
    stream_id: i32,
    headers: &[(Vec<u8>, Vec<u8>)],
    end_stream: bool,
) -> Result<(), String> {
    with_session(id, |session| {
        let names = headers
            .iter()
            .map(|(name, _)| name.as_ptr())
            .collect::<Vec<_>>();
        let name_lens = headers
            .iter()
            .map(|(name, _)| name.len())
            .collect::<Vec<_>>();
        let values = headers
            .iter()
            .map(|(_, value)| value.as_ptr())
            .collect::<Vec<_>>();
        let value_lens = headers
            .iter()
            .map(|(_, value)| value.len())
            .collect::<Vec<_>>();
        check(unsafe {
            qh2_submit_headers(
                session,
                stream_id,
                names.as_ptr(),
                name_lens.as_ptr(),
                values.as_ptr(),
                value_lens.as_ptr(),
                headers.len(),
                i32::from(end_stream),
            )
        })
    })
}

/// Submit a SETTINGS frame through nghttp2 so it is ordered after the
/// connection preface and before any queued request frames.
pub(crate) fn submit_settings(id: u64, settings: &[(u16, u32)]) -> Result<(), String> {
    with_session(id, |session| {
        let ids = settings.iter().map(|(settings_id, _)| u32::from(*settings_id)).collect::<Vec<_>>();
        let values = settings.iter().map(|(_, value)| *value).collect::<Vec<_>>();
        check(unsafe { qh2_submit_settings(session, ids.as_ptr(), values.as_ptr(), ids.len()) })
    })
}

/// Submit a DATA chunk; `end_stream` marks this chunk as the final body data.
pub(crate) fn submit_data(
    id: u64,
    stream_id: i32,
    bytes: &[u8],
    end_stream: bool,
) -> Result<(), String> {
    with_session(id, |session| {
        check(unsafe {
            qh2_submit_data(
                session,
                stream_id,
                bytes.as_ptr(),
                bytes.len(),
                i32::from(end_stream),
            )
        })
    })
}

/// Open a client request stream. When `end_stream` is false, a deferred body
/// source is installed and subsequent `submit_data` calls resume it.
pub(crate) fn submit_request(
    id: u64,
    headers: &[(Vec<u8>, Vec<u8>)],
    end_stream: bool,
) -> Result<i32, String> {
    with_session(id, |session| {
        let names = headers
            .iter()
            .map(|(name, _)| name.as_ptr())
            .collect::<Vec<_>>();
        let name_lens = headers
            .iter()
            .map(|(name, _)| name.len())
            .collect::<Vec<_>>();
        let values = headers
            .iter()
            .map(|(_, value)| value.as_ptr())
            .collect::<Vec<_>>();
        let value_lens = headers
            .iter()
            .map(|(_, value)| value.len())
            .collect::<Vec<_>>();
        let stream_id = unsafe {
            qh2_submit_request(
                session,
                names.as_ptr(),
                name_lens.as_ptr(),
                values.as_ptr(),
                value_lens.as_ptr(),
                headers.len(),
                i32::from(end_stream),
            )
        };
        if stream_id < 0 {
            Err(error_text(stream_id))
        } else {
            Ok(stream_id)
        }
    })
}

/// Submit a server push promise and return the promised stream identifier.
pub(crate) fn submit_push_promise(
    id: u64,
    parent_stream_id: i32,
    headers: &[(Vec<u8>, Vec<u8>)],
) -> Result<i32, String> {
    with_session(id, |session| {
        let names = headers
            .iter()
            .map(|(name, _)| name.as_ptr())
            .collect::<Vec<_>>();
        let name_lens = headers
            .iter()
            .map(|(name, _)| name.len())
            .collect::<Vec<_>>();
        let values = headers
            .iter()
            .map(|(_, value)| value.as_ptr())
            .collect::<Vec<_>>();
        let value_lens = headers
            .iter()
            .map(|(_, value)| value.len())
            .collect::<Vec<_>>();
        let stream_id = unsafe {
            qh2_submit_push_promise(
                session,
                parent_stream_id,
                names.as_ptr(),
                name_lens.as_ptr(),
                values.as_ptr(),
                value_lens.as_ptr(),
                headers.len(),
            )
        };
        if stream_id < 0 {
            Err(error_text(stream_id))
        } else {
            Ok(stream_id)
        }
    })
}

/// Close and release a session. Repeated close calls are harmless.
pub(crate) fn close_session(id: u64) {
    let session = sessions()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .remove(&id);
    if let Some(Session(ptr)) = session {
        unsafe { qh2_free(ptr) };
    }
}

unsafe extern "C" {
    fn qh2_new(server: i32) -> *mut c_void;
    fn qh2_set_local_window_size(session: *mut c_void, size: i32) -> c_int;
    fn qh2_submit_rst_stream(
        session: *mut c_void,
        stream_id: i32,
        error_code: u32,
    ) -> c_int;
    fn qh2_free(session: *mut c_void);
    fn qh2_mem_recv(session: *mut c_void, bytes: *const u8, len: usize) -> i64;
    fn qh2_mem_send(session: *mut c_void, bytes: *mut *const u8) -> i64;
    fn qh2_submit_headers(
        session: *mut c_void,
        stream_id: i32,
        names: *const *const u8,
        name_lens: *const usize,
        values: *const *const u8,
        value_lens: *const usize,
        count: usize,
        end_stream: i32,
    ) -> c_int;
    fn qh2_submit_settings(
        session: *mut c_void,
        ids: *const u32,
        values: *const u32,
        count: usize,
    ) -> c_int;
    fn qh2_submit_data(
        session: *mut c_void,
        stream_id: i32,
        bytes: *const u8,
        len: usize,
        end_stream: i32,
    ) -> c_int;
    fn qh2_submit_request(
        session: *mut c_void,
        names: *const *const u8,
        name_lens: *const usize,
        values: *const *const u8,
        value_lens: *const usize,
        count: usize,
        end_stream: i32,
    ) -> i32;
    fn qh2_submit_push_promise(
        session: *mut c_void,
        stream_id: i32,
        names: *const *const u8,
        name_lens: *const usize,
        values: *const *const u8,
        value_lens: *const usize,
        count: usize,
    ) -> i32;
    fn qh2_next_event(session: *mut c_void) -> *mut c_void;
    fn qh2_event_kind(event: *mut c_void) -> c_int;
    fn qh2_event_stream_id(event: *mut c_void) -> i32;
    fn qh2_event_parent_stream_id(event: *mut c_void) -> i32;
    fn qh2_event_code(event: *mut c_void) -> u32;
    fn qh2_event_end_stream(event: *mut c_void) -> c_int;
    fn qh2_event_data(event: *mut c_void) -> *const u8;
    fn qh2_event_data_len(event: *mut c_void) -> usize;
    fn qh2_event_header_count(event: *mut c_void) -> usize;
    fn qh2_event_header(
        event: *mut c_void,
        index: usize,
        name: *mut *const u8,
        name_len: *mut usize,
        value: *mut *const u8,
        value_len: *mut usize,
    ) -> c_int;
    fn qh2_event_free(event: *mut c_void);
    fn qh2_error_string(code: c_int) -> *const c_char;
}
