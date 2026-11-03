//! Darwin timezone queries retain the upstream CoreFoundation calls and ownership.
//! Load their implementation on first query so unused timezone support has no eager
//! framework dependency. Successful handles remain live for the process lifetime.

use core_foundation_sys::base::{Boolean, CFRange, CFTypeRef};
use core_foundation_sys::string::{CFStringEncoding, CFStringRef};
use core_foundation_sys::timezone::CFTimeZoneRef;
use std::ffi::{c_char, c_void};
use std::sync::{Mutex, OnceLock};

static API: OnceLock<CoreFoundation> = OnceLock::new();

// Derive callable bindings and their resolution from one SDK signature list.
macro_rules! functions {
    ($($name:ident($($argument:ident: $ty:ty),*) -> $result:ty;)+) => {
        #[allow(non_snake_case)]
        pub(super) struct CoreFoundation {
            $($name: unsafe extern "C" fn($($ty),*) -> $result,)+
        }
        impl CoreFoundation {
            #[allow(non_snake_case)] // Retain the SDK names in the derived bindings.
            unsafe fn symbols(handle: *mut c_void) -> Option<Self> {
                $(
                    // SAFETY: the handle is live and the symbol's SDK ABI is declared below.
                    let address = unsafe { libc::dlsym(handle,
                        concat!(stringify!($name), "\0").as_ptr().cast()) };
                    if address.is_null() { return None; }
                    // SAFETY: the resolved SDK symbol has this declared C function signature.
                    let $name = unsafe { std::mem::transmute::<*mut c_void,
                        unsafe extern "C" fn($($ty),*) -> $result>(address) };
                )+
                Some(Self { $($name,)+ })
            }
        }
        $(
            #[allow(non_snake_case)]
            pub(super) unsafe fn $name($($argument: $ty),*) -> $result {
                // SAFETY: get_timezone loads the whole table before creating any CF value;
                // its handle remains live, and the caller preserves the original SDK contract.
                unsafe { (API.get().expect("timezone API loaded").$name)($($argument),*) }
            }
        )+
    };
}

functions! {
    CFRelease(value: CFTypeRef) -> ();
    CFTimeZoneResetSystem() -> ();
    CFTimeZoneCopySystem() -> CFTimeZoneRef;
    CFTimeZoneGetName(value: CFTimeZoneRef) -> CFStringRef;
    CFStringGetCStringPtr(value: CFStringRef, encoding: CFStringEncoding) -> *const c_char;
    CFStringGetLength(value: CFStringRef) -> isize;
    CFStringGetBytes(value: CFStringRef, range: CFRange, encoding: CFStringEncoding,
        loss: u8, external: Boolean, buffer: *mut u8, capacity: isize, used: *mut isize) -> isize;
}

pub(super) fn load() -> Option<&'static CoreFoundation> {
    static LOAD: Mutex<()> = Mutex::new(());
    if let Some(api) = API.get() {
        return Some(api);
    }
    let _load = LOAD.lock().ok()?;
    if let Some(api) = API.get() {
        return Some(api);
    }
    // SAFETY: load the system framework, without exposing its symbols globally.
    let handle = unsafe {
        libc::dlopen(
            b"/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation\0"
                .as_ptr()
                .cast(),
            libc::RTLD_LAZY | libc::RTLD_LOCAL,
        )
    };
    if handle.is_null() {
        return None;
    }
    // SAFETY: the framework handle is live until either failure cleanup or process exit.
    let api = match unsafe { CoreFoundation::symbols(handle) } {
        Some(api) => api,
        None => {
            // SAFETY: no table or CF object escaped this failed initialization.
            unsafe {
                libc::dlclose(handle);
            }
            return None;
        }
    };
    Some(API.get_or_init(|| api))
}
