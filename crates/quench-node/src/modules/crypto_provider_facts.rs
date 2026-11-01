//! OpenSSL3 provider facts and the process-wide default-property generation owner.
use std::{
    ffi::{c_char, c_int, c_void, CStr, CString},
    ptr,
    sync::Mutex,
};
static FIPS_GENERATION: Mutex<u64> = Mutex::new(0);
#[derive(Clone, Copy)]
pub enum Kind {
    Cipher,
    Mac,
}
#[cfg_attr(test, link(name = "crypto"))]
unsafe extern "C" {
    fn CRYPTO_secure_malloc_initialized() -> c_int;
    fn CRYPTO_secure_used() -> usize;
    fn EVP_default_properties_is_fips_enabled(ctx: *mut c_void) -> c_int;
    fn EVP_default_properties_enable_fips(ctx: *mut c_void, enable: c_int) -> c_int;
    fn OSSL_PROVIDER_available(ctx: *mut c_void, name: *const c_char) -> c_int;
    fn OSSL_PROVIDER_load(ctx: *mut c_void, name: *const c_char) -> *mut c_void;
    fn OSSL_PROVIDER_self_test(provider: *const c_void) -> c_int;
    fn OSSL_PROVIDER_unload(provider: *mut c_void) -> c_int;
    fn EVP_CIPHER_do_all_provided(
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
    fn EVP_MAC_names_do_all(
        value: *const c_void,
        cb: unsafe extern "C" fn(*const c_char, *mut c_void),
        arg: *mut c_void,
    ) -> c_int;
    fn EVP_CIPHER_fetch(ctx: *mut c_void, name: *const c_char, props: *const c_char)
        -> *mut c_void;
    fn EVP_MAC_fetch(ctx: *mut c_void, name: *const c_char, props: *const c_char) -> *mut c_void;
    fn EVP_CIPHER_free(value: *mut c_void);
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
pub(crate) struct ErrorScope(bool);
impl ErrorScope {
    pub(crate) fn new() -> Self {
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
/// Match Node's test-only provider probe: availability is insufficient unless
/// the loaded FIPS provider also completes its self-test.
pub fn test_fips_provider() -> bool {
    let _errors = ErrorScope::new();
    if unsafe { OSSL_PROVIDER_available(ptr::null_mut(), c"fips".as_ptr()) } != 1 {
        return false;
    }
    let provider = unsafe { OSSL_PROVIDER_load(ptr::null_mut(), c"fips".as_ptr()) };
    if provider.is_null() {
        return false;
    }
    let passed = unsafe { OSSL_PROVIDER_self_test(provider) } == 1;
    unsafe { OSSL_PROVIDER_unload(provider) };
    passed
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
            Kind::Mac => EVP_MAC_fetch(ptr::null_mut(), name.as_ptr(), ptr::null()),
        };
        if value.is_null() {
            return false;
        }
        match kind {
            Kind::Cipher => EVP_CIPHER_free(value),
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
            Kind::Mac => EVP_MAC_do_all_provided(ptr::null_mut(), algorithm, arg),
        }
    }
    names.values.sort_unstable();
    names.values.dedup();
    names.values
}
// Single SDK ncrypto KeyAlgorithm descriptor authority. Native backend probes
// determine availability; metadata and admission derive from this same table.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KeyFamily {
    Other,
    EdDSA,
    XDH,
    MLDSA,
    MLKEM,
    SLHDSA,
}
pub struct KeyAlgorithm {
    name: &'static std::ffi::CStr,
    pub family: KeyFamily,
    has_key_type: bool,
}
impl KeyAlgorithm {
    pub fn name(&self) -> &'static str {
        self.name.to_str().expect("SDK algorithm names are ASCII")
    }
    pub fn public_name(&self) -> Option<String> {
        self.has_key_type.then(|| self.name().to_ascii_lowercase())
    }
    pub fn is_okp(&self) -> bool {
        matches!(self.family, KeyFamily::EdDSA | KeyFamily::XDH)
    }
    pub fn is_pqc(&self) -> bool {
        matches!(
            self.family,
            KeyFamily::MLDSA | KeyFamily::MLKEM | KeyFamily::SLHDSA
        )
    }
    pub fn is_one_shot(&self) -> bool {
        matches!(
            self.family,
            KeyFamily::EdDSA | KeyFamily::MLDSA | KeyFamily::SLHDSA
        )
    }
}
const KEY_ALGORITHMS: &[KeyAlgorithm] = &[
    KeyAlgorithm {
        name: c"RSA",
        family: KeyFamily::Other,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"RSA-PSS",
        family: KeyFamily::Other,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"DSA",
        family: KeyFamily::Other,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"DH",
        family: KeyFamily::Other,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"EC",
        family: KeyFamily::Other,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"Ed25519",
        family: KeyFamily::EdDSA,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"Ed448",
        family: KeyFamily::EdDSA,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"X25519",
        family: KeyFamily::XDH,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"X448",
        family: KeyFamily::XDH,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"SM2",
        family: KeyFamily::Other,
        has_key_type: false,
    },
    KeyAlgorithm {
        name: c"ML-DSA-44",
        family: KeyFamily::MLDSA,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"ML-DSA-65",
        family: KeyFamily::MLDSA,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"ML-DSA-87",
        family: KeyFamily::MLDSA,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"ML-KEM-512",
        family: KeyFamily::MLKEM,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"ML-KEM-768",
        family: KeyFamily::MLKEM,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"ML-KEM-1024",
        family: KeyFamily::MLKEM,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"SLH-DSA-SHA2-128f",
        family: KeyFamily::SLHDSA,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"SLH-DSA-SHA2-128s",
        family: KeyFamily::SLHDSA,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"SLH-DSA-SHA2-192f",
        family: KeyFamily::SLHDSA,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"SLH-DSA-SHA2-192s",
        family: KeyFamily::SLHDSA,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"SLH-DSA-SHA2-256f",
        family: KeyFamily::SLHDSA,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"SLH-DSA-SHA2-256s",
        family: KeyFamily::SLHDSA,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"SLH-DSA-SHAKE-128f",
        family: KeyFamily::SLHDSA,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"SLH-DSA-SHAKE-128s",
        family: KeyFamily::SLHDSA,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"SLH-DSA-SHAKE-192f",
        family: KeyFamily::SLHDSA,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"SLH-DSA-SHAKE-192s",
        family: KeyFamily::SLHDSA,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"SLH-DSA-SHAKE-256f",
        family: KeyFamily::SLHDSA,
        has_key_type: true,
    },
    KeyAlgorithm {
        name: c"SLH-DSA-SHAKE-256s",
        family: KeyFamily::SLHDSA,
        has_key_type: true,
    },
];
pub fn key_algorithm(name: &str) -> Option<&'static KeyAlgorithm> {
    let name = name.split('\0').next().unwrap();
    KEY_ALGORITHMS
        .iter()
        .find(|algorithm| algorithm.name().eq_ignore_ascii_case(name))
}
pub fn key_algorithm_for(key: *const openssl_sys::EVP_PKEY) -> Option<&'static KeyAlgorithm> {
    if key.is_null() {
        return None;
    }
    KEY_ALGORITHMS
        .iter()
        .find(|algorithm| unsafe { openssl_sys::EVP_PKEY_is_a(key, algorithm.name.as_ptr()) } == 1)
}
pub fn named_generation_algorithm(name: &str, web_crypto: bool) -> Option<&'static str> {
    let algorithm = key_algorithm(name)?;
    if !algorithm.is_okp() && !algorithm.is_pqc() {
        return None;
    }
    if !web_crypto
        && (algorithm.public_name().as_deref() != Some(name) || !key_available(algorithm.name()))
    {
        return None;
    }
    Some(algorithm.name())
}
pub fn key_available(name: &str) -> bool {
    let Some(algorithm) = KEY_ALGORITHMS
        .iter()
        .find(|algorithm| algorithm.name() == name)
    else {
        return false;
    };
    let _errors = ErrorScope::new();
    unsafe {
        let ctx = EVP_PKEY_CTX_new_from_name(ptr::null_mut(), algorithm.name.as_ptr(), ptr::null());
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
            assert!(
                String::from_utf8_lossy(&output.stderr).contains(COMPLETED),
                "child did not execute the actual transitions: {output:?}"
            );
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
