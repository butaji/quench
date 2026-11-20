//! Apple FSEvents/CoreFoundation ABI (SDK FSEvents.h and CFRunLoop.h).
use std::ffi::{c_char, c_void};
use std::io;
use std::sync::{Mutex, OnceLock};
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
// One ABI declaration derives both the callable table and its symbol resolution.
macro_rules! fsevents {
    ($($field:ident: $symbol:ident($($argument:ty),*) -> $result:ty;)+) => {
        pub(super) struct FsEvents {
            $(pub $field: unsafe extern "C" fn($($argument),*) -> $result,)+
            pub default_mode: Object,
            pub array_callbacks: *const ArrayCallbacks,
        }
        // SAFETY: only callable pointers and immutable SDK constants are shared. The
        // successful framework handle stays loaded for every stream and VM thread.
        unsafe impl Send for FsEvents {}
        unsafe impl Sync for FsEvents {}
        impl FsEvents {
            unsafe fn symbols(handle: *mut c_void) -> io::Result<Self> {
                $(
                    // SAFETY: the handle is live; the signatures come from FSEvents.h/CFRunLoop.h.
                    let address = unsafe { libc::dlsym(handle,
                        concat!(stringify!($symbol), "\0").as_ptr().cast()) };
                    if address.is_null() { return Err(io::Error::from_raw_os_error(libc::ENOENT)); }
                    let $field = unsafe { std::mem::transmute::<*mut c_void,
                        unsafe extern "C" fn($($argument),*) -> $result>(address) };
                )+
                // dlsym searches the framework's CoreFoundation dependency as well.
                let mode = unsafe { libc::dlsym(handle, c"kCFRunLoopDefaultMode".as_ptr()) };
                let callbacks = unsafe { libc::dlsym(handle, c"kCFTypeArrayCallBacks".as_ptr()) };
                if mode.is_null() || callbacks.is_null() {
                    return Err(io::Error::from_raw_os_error(libc::ENOENT));
                }
                // SAFETY: kCFRunLoopDefaultMode is a CFStringRef global; callbacks is a struct global.
                let default_mode = unsafe { mode.cast::<Object>().read() };
                Ok(Self { $($field,)+ default_mode, array_callbacks: callbacks.cast() })
            }
        }
    };
}

fsevents! {
    create: FSEventStreamCreate(Object, Callback, *mut Context, Object, u64, f64, u32) -> Stream;
    schedule: FSEventStreamScheduleWithRunLoop(Stream, Object, Object) -> ();
    start: FSEventStreamStart(Stream) -> u8;
    stop: FSEventStreamStop(Stream) -> ();
    invalidate: FSEventStreamInvalidate(Stream) -> ();
    release: FSEventStreamRelease(Stream) -> ();
    run_loop_current: CFRunLoopGetCurrent() -> Object;
    run_loop_run: CFRunLoopRunInMode(Object, f64, u8) -> i32;
    string_create: CFStringCreateWithFileSystemRepresentation(Object, *const c_char) -> Object;
    array_create: CFArrayCreate(Object, *const Object, isize, Object) -> Object;
    cf_release: CFRelease(Object) -> ();
}

impl FsEvents {
    /// libuv loads these frameworks on its first directory watch. Keep the successful
    /// handle for the process lifetime so every stream and callback retains its code.
    /// Failed loads are retryable; no function table is published until all symbols resolve.
    pub(super) fn get() -> io::Result<&'static Self> {
        static API: OnceLock<FsEvents> = OnceLock::new();
        static LOAD: Mutex<()> = Mutex::new(());
        if let Some(api) = API.get() {
            return Ok(api);
        }
        let _load = LOAD
            .lock()
            .map_err(|_| io::Error::from_raw_os_error(libc::EIO))?;
        if let Some(api) = API.get() {
            return Ok(api);
        }
        // SAFETY: the absolute system-framework path matches libuv; loading runs its initializers.
        let handle = unsafe {
            libc::dlopen(
                c"/System/Library/Frameworks/CoreServices.framework/Versions/A/CoreServices"
                    .as_ptr(),
                libc::RTLD_LAZY | libc::RTLD_LOCAL,
            )
        };
        if handle.is_null() {
            return Err(io::Error::from_raw_os_error(libc::ENOSYS));
        }
        let api = match unsafe { Self::symbols(handle) } {
            Ok(api) => api,
            Err(error) => {
                // SAFETY: no table or stream escaped this unsuccessful load.
                unsafe {
                    libc::dlclose(handle);
                }
                return Err(error);
            }
        };
        Ok(API.get_or_init(|| api))
    }
}
#[repr(C)]
pub(super) struct ArrayCallbacks {
    version: isize,
    retain: Option<unsafe extern "C" fn(Object, Object) -> Object>,
    release: Option<unsafe extern "C" fn(Object, Object)>,
    description: Option<unsafe extern "C" fn(Object) -> Object>,
    equal: Option<unsafe extern "C" fn(Object, Object) -> u8>,
}
