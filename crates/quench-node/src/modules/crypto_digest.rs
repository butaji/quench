//! Shared digest authority for the OpenSSL >=3 native backend.
//! Fetched Md owns EVP_MD throughout all MdCtx operations. No pointer is
//! disguised as a static MessageDigest, and no algorithm-name table is used.
use foreign_types::{ForeignType, ForeignTypeRef};
use openssl::{error::ErrorStack, hash::MessageDigest, md::Md, md_ctx::MdCtx, pkey::PKey};
use openssl_sys::EVP_MD;
use std::{
    ffi::{c_char, c_int, c_ulong, c_void, CStr},
    ptr,
};
const EVP_MD_FLAG_XOF: c_ulong = 0x0002; // openssl/evp.h authoritative flag.
unsafe extern "C" {
    fn EVP_MD_is_a(md: *const EVP_MD, name: *const c_char) -> c_int;
    fn EVP_MD_get_flags(md: *const EVP_MD) -> c_ulong;
    fn EVP_MD_get0_name(md: *const EVP_MD) -> *const c_char;
    fn EVP_MD_do_all_sorted(
        callback: Option<
            unsafe extern "C" fn(*const EVP_MD, *const c_char, *const c_char, *mut c_void),
        >,
        data: *mut c_void,
    );
    fn EVP_MD_do_all_provided(
        ctx: *mut c_void,
        callback: Option<unsafe extern "C" fn(*mut EVP_MD, *mut c_void)>,
        data: *mut c_void,
    );
    fn EVP_MD_names_do_all(
        md: *const EVP_MD,
        callback: Option<unsafe extern "C" fn(*const c_char, *mut c_void)>,
        data: *mut c_void,
    ) -> c_int;
}
#[derive(Debug)]
pub enum DigestError {
    Unsupported,
    InvalidLength { expected: usize, actual: usize },
    Allocation,
    OpenSsl(ErrorStack),
}
impl std::fmt::Display for DigestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => write!(f, "Digest method not supported"),
            Self::InvalidLength { expected, actual } => write!(
                f,
                "Output length {actual} is invalid; expected {expected} for a non-XOF digest"
            ),
            Self::Allocation => write!(f, "Digest output allocation failed"),
            Self::OpenSsl(error) => write!(f, "{error}"),
        }
    }
}
impl std::error::Error for DigestError {}
impl From<ErrorStack> for DigestError {
    fn from(e: ErrorStack) -> Self {
        Self::OpenSsl(e)
    }
}
pub struct Digest {
    md: NativeDigest,
}
// A fetched digest owns its provider reference. OpenSSL's legacy descriptors are
// genuinely process-static objects; never cast a fetched pointer to that lifetime.
enum NativeDigest {
    Fetched(Md),
    Legacy(MessageDigest),
}
impl Digest {
    /// ncrypto::Digest::FromName accepts known legacy descriptors even when
    /// their provider is unavailable; the actual operation then owns failure.
    pub(crate) fn from_sdk_name(name: &str) -> Result<Self, DigestError> {
        let name = name.split('\0').next().unwrap_or("");
        let legacy = if matches!(name, "dss1" | "DSS1") {
            Some(MessageDigest::sha1())
        } else {
            MessageDigest::from_name(name)
        };
        match legacy {
            Some(md) if md.as_ptr() != unsafe { openssl_sys::EVP_md_null() } => Ok(Self {
                md: NativeDigest::Legacy(md),
            }),
            _ => Self::fetch(name),
        }
    }

    pub(crate) fn fetch_preserving_errors(name: &str) -> Option<Self> {
        struct Scope(bool);
        unsafe extern "C" {
            fn ERR_set_mark() -> c_int;
            fn ERR_pop_to_mark() -> c_int;
            fn ERR_clear_error();
            fn EVP_MD_fetch(
                ctx: *mut c_void,
                name: *const c_char,
                props: *const c_char,
            ) -> *mut EVP_MD;
        }
        impl Drop for Scope {
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
        let _scope = Scope(unsafe { ERR_set_mark() } == 1);
        // Node's SDK passes Utf8Value through the C-string crypto boundary.
        let name = name.split('\0').next().unwrap_or("");
        // Historical exact-case DSA aliases in ncrypto::getDigestByName.
        let legacy = if matches!(name, "dss1" | "DSS1") {
            Some(MessageDigest::sha1())
        } else {
            MessageDigest::from_name(name)
        };
        if legacy.is_some_and(|md| md.as_ptr() == unsafe { openssl_sys::EVP_md_null() }) {
            return None;
        }
        let canonical = legacy.and_then(|md| {
            let ptr = unsafe { EVP_MD_get0_name(md.as_ptr()) };
            if ptr.is_null() {
                None
            } else {
                unsafe { CStr::from_ptr(ptr) }.to_str().ok()
            }
        });
        let name = std::ffi::CString::new(canonical.unwrap_or(name)).ok()?;
        let md = unsafe { EVP_MD_fetch(ptr::null_mut(), name.as_ptr(), ptr::null()) };
        if md.is_null() {
            return None;
        }
        let md = unsafe { Md::from_ptr(md) };
        if unsafe { EVP_MD_is_a(md.as_ptr(), c"NULL".as_ptr()) } == 1 {
            return None;
        }
        Some(Self {
            md: NativeDigest::Fetched(md),
        })
    }
    pub(crate) fn aliases(&self) -> Vec<String> {
        unsafe extern "C" fn collect(name: *const c_char, data: *mut c_void) {
            if !name.is_null() {
                let name = unsafe { CStr::from_ptr(name) }
                    .to_string_lossy()
                    .into_owned();
                unsafe { &mut *(data as *mut Vec<String>) }.push(name);
            }
        }
        let mut names = Vec::new();
        unsafe {
            EVP_MD_names_do_all(
                self.native_md().as_ptr(),
                Some(collect),
                &mut names as *mut _ as *mut c_void,
            )
        };
        names
    }
    pub(crate) fn native_md(&self) -> &openssl::md::MdRef {
        match &self.md {
            NativeDigest::Fetched(md) => md,
            NativeDigest::Legacy(md) => unsafe {
                openssl::md::MdRef::from_ptr(md.as_ptr() as *mut EVP_MD)
            },
        }
    }
    pub(crate) fn identity(&self) -> usize {
        self.native_md().as_ptr() as usize
    }

    pub fn fetch(name: &str) -> Result<Self, DigestError> {
        Self::fetch_preserving_errors(name).ok_or(DigestError::Unsupported)
    }
    pub fn size(&self) -> usize {
        let size = self.native_md().size();
        // Node26 DEP0198 compatibility: OpenSSL3.4+ removed SHAKE defaults.
        // Native digest identity determines this SDK policy, including aliases.
        if size == 0 && self.is_xof() {
            match self.native_md().type_().short_name().ok() {
                Some("SHAKE128") => 16,
                Some("SHAKE256") => 32,
                _ => size,
            }
        } else {
            size
        }
    }
    pub fn is_xof(&self) -> bool {
        unsafe { EVP_MD_get_flags(self.native_md().as_ptr()) & EVP_MD_FLAG_XOF != 0 }
    }
    pub fn hash(&self, input: &[u8], output_length: Option<usize>) -> Result<Vec<u8>, DigestError> {
        let expected = self.size();
        let size = output_length.unwrap_or(expected);
        if !self.is_xof() && size != expected {
            return Err(DigestError::InvalidLength {
                expected,
                actual: size,
            });
        }
        let mut output = Vec::new();
        output
            .try_reserve_exact(size)
            .map_err(|_| DigestError::Allocation)?;
        output.resize(size, 0);
        let mut ctx = MdCtx::new()?;
        ctx.digest_init(self.native_md())?;
        ctx.digest_update(input)?;
        if self.is_xof() {
            ctx.digest_final_xof(&mut output)?;
        } else {
            let n = ctx.digest_final(&mut output)?;
            output.truncate(n);
        }
        Ok(output)
    }
    pub fn hmac(&self, key: &[u8], input: &[u8]) -> Result<Vec<u8>, DigestError> {
        // Keep both key and fetched Md alive until the context is finalized.
        // OpenSSL rejects unsupported XOF HMACs at native initialization.
        // EVP_PKEY_new_mac_key rejects the empty key. HMAC's defined zero
        // block padding makes one zero byte exactly equivalent to an empty key.
        let key = if key.is_empty() { &[0u8][..] } else { key };
        let pkey = PKey::hmac(key)?;
        let mut ctx = MdCtx::new()?;
        ctx.digest_sign_init(Some(self.native_md()), &pkey)?;
        ctx.digest_sign_update(input)?;
        let mut output = Vec::new();
        ctx.digest_sign_final_to_vec(&mut output)?;
        Ok(output)
    }
}
unsafe extern "C" fn legacy_name(
    _: *const EVP_MD,
    from: *const c_char,
    _: *const c_char,
    data: *mut c_void,
) {
    if from.is_null() {
        return;
    }
    let names = unsafe { &mut *(data as *mut Vec<String>) };
    if let Ok(name) = unsafe { CStr::from_ptr(from) }.to_str() {
        names.push(name.to_owned());
    }
}
unsafe extern "C" fn provider_name(name: *const c_char, data: *mut c_void) {
    if name.is_null() {
        return;
    }
    let Ok(name) = unsafe { CStr::from_ptr(name) }.to_str() else {
        return;
    };
    if name.contains('.') && name.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return;
    }
    unsafe { &mut *(data as *mut Vec<String>) }.push(name.to_ascii_lowercase());
}
unsafe extern "C" fn provider_names(md: *mut EVP_MD, data: *mut c_void) {
    unsafe { EVP_MD_names_do_all(md, Some(provider_name), data) };
}
/// Node26's legacy-alias plus provider-name policy, derived from native metadata.
/// No global cache: provider/FIPS selection changes cannot leave stale names.
pub fn supported_names() -> Vec<String> {
    openssl::init();
    let mut names = Vec::<String>::new();
    let data = &mut names as *mut _ as *mut c_void;
    unsafe {
        EVP_MD_do_all_sorted(Some(legacy_name), data);
        EVP_MD_do_all_provided(ptr::null_mut(), Some(provider_names), data);
    }
    names
        .into_iter()
        .filter(|name| Digest::fetch_preserving_errors(name).is_some())
        .collect()
}
