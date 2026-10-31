//! Apple FSEvents/CoreFoundation ABI (SDK FSEvents.h and CFRunLoop.h).
use std::ffi::{c_char, c_void};
pub(super) type Object = *const c_void;
pub(super) type Stream = *mut c_void;
pub(super) const CONTEXT_VERSION: isize = 0;
pub(super) const POLL_WITHOUT_WAIT_SECONDS: f64 = 0.0;
pub(super) const CREATE_NO_DEFER: u32 = 0x00000002;
pub(super) const CREATE_FILE_EVENTS: u32 = 0x00000010;
pub(super) const SINCE_NOW: u64 = u64::MAX;
pub(super) const SYSTEM_EVENTS: u32 = 0x00000002 // UserDropped
    | 0x00000004 // KernelDropped
    | 0x00000008 // EventIdsWrapped
    | 0x00000010 // HistoryDone
    | 0x00000020 // RootChanged
    | 0x00000040 // Mount
    | 0x00000080; // Unmount
pub(super) const RENAMED: u32 = 0x00000100 // ItemCreated
    | 0x00000200 // ItemRemoved
    | 0x00000800; // ItemRenamed
pub(super) const MODIFIED: u32 = 0x00000400 // ItemInodeMetaMod
    | 0x00001000 // ItemModified
    | 0x00002000 // ItemFinderInfoMod
    | 0x00004000 // ItemChangeOwner
    | 0x00008000; // ItemXattrMod
pub(super) const IS_DIRECTORY: u32 = 0x00020000;
// Match pinned libuv fsevents.c: bounded delivery latency under continuous writes.
pub(super) const DELIVERY_LATENCY_SECONDS: f64 = 0.05;
#[repr(C)]
pub(super) struct Context {
    pub version: isize,
    pub info: *mut c_void,
    pub retain: Option<unsafe extern "C" fn(*const c_void) -> *const c_void>,
    pub release: Option<unsafe extern "C" fn(*const c_void)>,
    pub description: Option<unsafe extern "C" fn(*const c_void) -> Object>,
}
pub(super) type Callback =
    unsafe extern "C" fn(Object, *mut c_void, usize, *mut c_void, *const u32, *const u64);
#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    pub(super) fn FSEventStreamCreate(
        allocator: Object,
        callback: Callback,
        context: *mut Context,
        paths: Object,
        since: u64,
        latency: f64,
        flags: u32,
    ) -> Stream;
    pub(super) fn FSEventStreamScheduleWithRunLoop(stream: Stream, run_loop: Object, mode: Object);
    pub(super) fn FSEventStreamStart(stream: Stream) -> u8;
    pub(super) fn FSEventStreamStop(stream: Stream);
    pub(super) fn FSEventStreamInvalidate(stream: Stream);
    pub(super) fn FSEventStreamRelease(stream: Stream);
}
#[repr(C)]
pub(super) struct ArrayCallbacks {
    version: isize,
    retain: Option<unsafe extern "C" fn(Object, Object) -> Object>,
    release: Option<unsafe extern "C" fn(Object, Object)>,
    description: Option<unsafe extern "C" fn(Object) -> Object>,
    equal: Option<unsafe extern "C" fn(Object, Object) -> u8>,
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub(super) static kCFRunLoopDefaultMode: Object;
    pub(super) static kCFTypeArrayCallBacks: ArrayCallbacks;
    pub(super) fn CFRunLoopGetCurrent() -> Object;
    pub(super) fn CFRunLoopRunInMode(mode: Object, seconds: f64, return_after_source: u8) -> i32;
    pub(super) fn CFStringCreateWithFileSystemRepresentation(
        allocator: Object,
        path: *const c_char,
    ) -> Object;
    pub(super) fn CFArrayCreate(
        allocator: Object,
        values: *const Object,
        count: isize,
        callbacks: Object,
    ) -> Object;
    pub(super) fn CFRelease(object: Object);
}
