//! OpenSSL3 provider facts and the process-wide default-property generation owner.
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    ptr,
    sync::Mutex,
};
static FIPS_GENERATION: Mutex<u64> = Mutex::new(0);
#[derive(Clone, Copy)]
pub enum Kind {
    Cipher,
    Digest,
    Mac,
}
#[cfg_attr(test, link(name = "crypto"))]
unsafe extern "C" {
    fn CRYPTO_secure_malloc_initialized() -> c_int;
    fn CRYPTO_secure_used() -> usize;
    fn EVP_default_properties_is_fips_enabled(ctx: *mut c_void) -> c_int;
    fn EVP_default_properties_enable_fips(ctx: *mut c_void, enable: c_int) -> c_int;
    fn EVP_CIPHER_do_all_provided(
        ctx: *mut c_void,
        cb: unsafe extern "C" fn(*mut c_void, *mut c_void),
        arg: *mut c_void,
    );
    fn EVP_MD_do_all_provided(
        ctx: *mut c_void,
        cb: unsafe extern "C" fn(*mut c_void, *mut c_void),
        arg: *mut c_void,
    );
    fn EVP_MAC_do_all_provided(
        ctx: *mut c_void,
        cb: unsafe extern "C" fn(*mut c_void, *mut c_void),
        arg: *mut c_void,
    );
    fn EVP_CIPHER_names_do_all(
        value: *const c_void,
        cb: unsafe extern "C" fn(*const c_char, *mut c_void),
        arg: *mut c_void,
    ) -> c_int;
    fn EVP_MD_names_do_all(
        value: *const c_void,
        cb: unsafe extern "C" fn(*const c_char, *mut c_void),
        arg: *mut c_void,
    ) -> c_int;
    fn EVP_MAC_names_do_all(
        value: *const c_void,
        cb: unsafe extern "C" fn(*const c_char, *mut c_void),
        arg: *mut c_void,
    ) -> c_int;
    fn EVP_CIPHER_fetch(ctx: *mut c_void, name: *const c_char, props: *const c_char)
    -> *mut c_void;
    fn EVP_MD_fetch(ctx: *mut c_void, name: *const c_char, props: *const c_char) -> *mut c_void;
    fn EVP_MAC_fetch(ctx: *mut c_void, name: *const c_char, props: *const c_char) -> *mut c_void;
    fn EVP_CIPHER_free(value: *mut c_void);
    fn EVP_MD_free(value: *mut c_void);
    fn EVP_MAC_free(value: *mut c_void);
    fn EVP_PKEY_CTX_new_from_name(
        ctx: *mut c_void,
        name: *const c_char,
        props: *const c_char,
    ) -> *mut c_void;
    fn EVP_PKEY_CTX_free(ctx: *mut c_void);
    fn ERR_set_mark() -> c_int;
    fn ERR_pop_to_mark() -> c_int;
    fn ERR_clear_error();
}
struct ErrorScope(bool);
impl ErrorScope {
    fn new() -> Self {
        Self(unsafe { ERR_set_mark() } == 1)
    }
}
impl Drop for ErrorScope {
    fn drop(&mut self) {
        unsafe {
            if self.0 {
                ERR_pop_to_mark();
            } else {
                ERR_clear_error();
            }
        }
    }
}
pub fn fips_enabled() -> bool {
    let _errors = ErrorScope::new();
    unsafe { EVP_default_properties_is_fips_enabled(ptr::null_mut()) == 1 }
}
pub fn fips_generation() -> u64 {
    *FIPS_GENERATION.lock().unwrap()
}
/// Actual default-property transitions and their generation are one authority.
/// Failed/no-change requests never advance the generation.
pub fn set_fips(enable: bool) -> bool {
    let mut generation = FIPS_GENERATION.lock().unwrap();
    let previous = fips_enabled();
    if previous == enable {
        return true;
    }
    let ok = unsafe { EVP_default_properties_enable_fips(ptr::null_mut(), enable as c_int) } == 1;
    if ok && fips_enabled() != previous {
        *generation = generation
            .checked_add(1)
            .expect("OpenSSL FIPS generation space");
    }
    ok
}
struct Names {
    kind: Kind,
    values: Vec<String>,
}
unsafe extern "C" fn name(value: *const c_char, arg: *mut c_void) {
    let names = unsafe { &mut *arg.cast::<Names>() };
    let original = unsafe { CStr::from_ptr(value) }.to_string_lossy();
    for candidate in [original.to_string(), original.to_ascii_lowercase()] {
        if available(names.kind, &candidate) {
            names.values.push(candidate)
        }
    }
}
unsafe extern "C" fn algorithm(value: *mut c_void, arg: *mut c_void) {
    let kind = unsafe { (*arg.cast::<Names>()).kind };
    unsafe {
        match kind {
            Kind::Cipher => {
                EVP_CIPHER_names_do_all(value, name, arg);
            }
            Kind::Digest => {
                EVP_MD_names_do_all(value, name, arg);
            }
            Kind::Mac => {
                EVP_MAC_names_do_all(value, name, arg);
            }
        }
    }
}
fn available(kind: Kind, name: &str) -> bool {
    let Ok(name) = CString::new(name) else {
        return false;
    };
    let _errors = ErrorScope::new();
    unsafe {
        let value = match kind {
            Kind::Cipher => EVP_CIPHER_fetch(ptr::null_mut(), name.as_ptr(), ptr::null()),
            Kind::Digest => EVP_MD_fetch(ptr::null_mut(), name.as_ptr(), ptr::null()),
            Kind::Mac => EVP_MAC_fetch(ptr::null_mut(), name.as_ptr(), ptr::null()),
        };
        if value.is_null() {
            return false;
        }
        match kind {
            Kind::Cipher => EVP_CIPHER_free(value),
            Kind::Digest => EVP_MD_free(value),
            Kind::Mac => EVP_MAC_free(value),
        }
        true
    }
}
pub fn names(kind: Kind) -> Vec<String> {
    let mut names = Names {
        kind,
        values: Vec::new(),
    };
    let arg = (&mut names as *mut Names).cast();
    let _errors = ErrorScope::new();
    unsafe {
        match kind {
            Kind::Cipher => EVP_CIPHER_do_all_provided(ptr::null_mut(), algorithm, arg),
            Kind::Digest => EVP_MD_do_all_provided(ptr::null_mut(), algorithm, arg),
            Kind::Mac => EVP_MAC_do_all_provided(ptr::null_mut(), algorithm, arg),
        }
    }
    names.values.sort_unstable();
    names.values.dedup();
    names.values
}
pub fn key_available(name: &str) -> bool {
    // SDK key algorithm names; arbitrary cipher/digest names are not key families.
    let name = match name {
        "Ed25519" => "ED25519",
        "Ed448" => "ED448",
        "X25519" => "X25519",
        "X448" => "X448",
        "RSA" => "RSA",
        "RSA-PSS" => "RSA-PSS",
        "EC" => "EC",
        "DSA" => "DSA",
        "DH" => "DH",
        "ML-KEM-512" => "ML-KEM-512",
        "ML-KEM-768" => "ML-KEM-768",
        "ML-KEM-1024" => "ML-KEM-1024",
        "ML-DSA-44" => "ML-DSA-44",
        "ML-DSA-65" => "ML-DSA-65",
        "ML-DSA-87" => "ML-DSA-87",
        _ => return false,
    };
    let name = CString::new(name).unwrap();
    let _errors = ErrorScope::new();
    unsafe {
        let ctx = EVP_PKEY_CTX_new_from_name(ptr::null_mut(), name.as_ptr(), ptr::null());
        if ctx.is_null() {
            false
        } else {
            EVP_PKEY_CTX_free(ctx);
            true
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_provider_and_generation() {
        assert!(names(Kind::Cipher).contains(&"aes-128-gcm".into()));
        assert!(names(Kind::Digest).contains(&"sha256".into()));
        assert!(names(Kind::Mac).contains(&"hmac".into()));
        assert!(!key_available("SHA-256"));
        assert!(key_available("Ed25519"));
        // OpenSSL default properties are process-global; mutating them here
        // races every concurrently running crypto/TLS test. Re-enter only this
        // exact test in a dedicated process, preserving the real transition test.
        const CHILD: &str = "QUENCH_PROVIDER_FACTS_FIPS_TEST_CHILD";
        const COMPLETED: &str = "quench-provider-fips-child-transitions-complete";
        if std::env::var(CHILD).ok().as_deref() == Some("1") {
            let before = fips_enabled();
            let generation = fips_generation();
            assert!(set_fips(before));
            assert_eq!(fips_generation(), generation);
            assert!(set_fips(!before));
            assert_eq!(fips_enabled(), !before);
            assert_eq!(fips_generation(), generation + 1);
            assert!(set_fips(!before));
            assert_eq!(fips_generation(), generation + 1);
            assert!(set_fips(before));
            assert_eq!(fips_enabled(), before);
            assert_eq!(fips_generation(), generation + 2);
            eprintln!("{COMPLETED}");
        } else {
            let before = fips_enabled();
            let generation = fips_generation();
            let (_, module) = module_path!().split_once("::").unwrap();
            let name = format!("{module}::live_provider_and_generation");
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &name, "--nocapture", "--test-threads=1"])
                .env(CHILD, "1")
                .output()
                .expect("launch isolated OpenSSL provider transition test");
            assert!(output.status.success(), "child failed: {output:?}");
            assert!(String::from_utf8_lossy(&output.stderr).contains(COMPLETED),
                "child did not execute the actual transitions: {output:?}");
            assert_eq!(fips_enabled(), before);
            assert_eq!(fips_generation(), generation);
        }
    }
}

/// Real heap state: default Node builds use ordinary backing allocations when
/// OpenSSL secure heap is disabled. Active secure heap needs owner adoption.
pub fn secure_heap_initialized() -> bool {
    unsafe { CRYPTO_secure_malloc_initialized() == 1 }
}
pub fn secure_heap_used() -> usize {
    if secure_heap_initialized() {
        unsafe { CRYPTO_secure_used() }
    } else {
        0
    }
}
