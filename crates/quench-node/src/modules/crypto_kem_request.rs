//! OpenSSL EVP_PKEY KEM operations used by Node's KEM jobs.
use super::super::crypto_asymmetric::OwnedKey;
use super::super::crypto_secret_material::Material;
use std::sync::Arc;

pub(crate) struct Result {
    pub(crate) shared_key: Material,
    pub(crate) ciphertext: Vec<u8>,
    pub(crate) decapsulation: bool,
}

pub(crate) struct Request {
    pub(crate) key: Arc<OwnedKey>,
    pub(crate) ciphertext: Option<Vec<u8>>,
}

unsafe extern "C" {
    fn OSSL_PARAM_construct_utf8_string(
        key: *const std::ffi::c_char,
        buf: *mut std::ffi::c_char,
        bsize: usize,
    ) -> openssl_sys::OSSL_PARAM;
}

fn set_operation(ctx: *mut openssl_sys::EVP_PKEY_CTX, key: &OwnedKey) -> bool {
    let operation = match key.public_ref().id() {
        openssl::pkey::Id::RSA => c"RSASVE",
        openssl::pkey::Id::EC | openssl::pkey::Id::X25519 | openssl::pkey::Id::X448 => c"DHKEM",
        _ => return true,
    };
    let key_name = c"operation";
    // `EVP_PKEY_CTX_set_params` copies provider parameters during the call.
    let parameter = unsafe {
        OSSL_PARAM_construct_utf8_string(key_name.as_ptr(), operation.as_ptr() as *mut _, 0)
    };
    let end = unsafe { openssl_sys::OSSL_PARAM_construct_end() };
    let params = [parameter, end];
    unsafe { openssl_sys::EVP_PKEY_CTX_set_params(ctx, params.as_ptr()) > 0 }
}

impl Request {
    pub(crate) fn encapsulate(key: Arc<OwnedKey>) -> Self {
        Self {
            key,
            ciphertext: None,
        }
    }

    pub(crate) fn decapsulate(key: Arc<OwnedKey>, ciphertext: Vec<u8>) -> Self {
        Self {
            key,
            ciphertext: Some(ciphertext),
        }
    }

    pub(crate) fn is_decapsulation(&self) -> bool {
        self.ciphertext.is_some()
    }

    pub(crate) fn run(self) -> std::result::Result<Result, openssl::error::ErrorStack> {
        use std::ptr;

        let raw_key = self.key.raw();
        let ctx = unsafe { openssl_sys::EVP_PKEY_CTX_new(raw_key, ptr::null_mut()) };
        if ctx.is_null() {
            return Err(openssl::error::ErrorStack::get());
        }
        struct Context(*mut openssl_sys::EVP_PKEY_CTX);
        impl Drop for Context {
            fn drop(&mut self) {
                unsafe { openssl_sys::EVP_PKEY_CTX_free(self.0) }
            }
        }
        let ctx = Context(ctx);
        let valid = unsafe {
            if let Some(ciphertext) = self.ciphertext.as_ref() {
                openssl_sys::EVP_PKEY_decapsulate_init(ctx.0, ptr::null()) > 0
                    && set_operation(ctx.0, &self.key)
                    && decapsulate(ctx.0, ciphertext).is_some()
            } else {
                openssl_sys::EVP_PKEY_encapsulate_init(ctx.0, ptr::null()) > 0
                    && set_operation(ctx.0, &self.key)
            }
        };
        if !valid {
            return Err(openssl::error::ErrorStack::get());
        }
        if let Some(ciphertext) = self.ciphertext {
            let mut shared_len = 0;
            if unsafe {
                openssl_sys::EVP_PKEY_decapsulate(
                    ctx.0,
                    ptr::null_mut(),
                    &mut shared_len,
                    ciphertext.as_ptr(),
                    ciphertext.len(),
                )
            } <= 0
            {
                return Err(openssl::error::ErrorStack::get());
            }
            let mut shared_key = Material::new(vec![0; shared_len]);
            if unsafe {
                openssl_sys::EVP_PKEY_decapsulate(
                    ctx.0,
                    shared_key.as_mut_ptr(),
                    &mut shared_len,
                    ciphertext.as_ptr(),
                    ciphertext.len(),
                )
            } <= 0
            {
                return Err(openssl::error::ErrorStack::get());
            }
            Ok(Result {
                shared_key,
                ciphertext: Vec::new(),
                decapsulation: true,
            })
        } else {
            let mut ciphertext_len = 0;
            let mut shared_len = 0;
            if unsafe {
                openssl_sys::EVP_PKEY_encapsulate(
                    ctx.0,
                    ptr::null_mut(),
                    &mut ciphertext_len,
                    ptr::null_mut(),
                    &mut shared_len,
                )
            } <= 0
            {
                return Err(openssl::error::ErrorStack::get());
            }
            let mut ciphertext = vec![0; ciphertext_len];
            let mut shared_key = Material::new(vec![0; shared_len]);
            if unsafe {
                openssl_sys::EVP_PKEY_encapsulate(
                    ctx.0,
                    ciphertext.as_mut_ptr(),
                    &mut ciphertext_len,
                    shared_key.as_mut_ptr(),
                    &mut shared_len,
                )
            } <= 0
            {
                return Err(openssl::error::ErrorStack::get());
            }
            ciphertext.truncate(ciphertext_len);
            Ok(Result {
                shared_key,
                ciphertext,
                decapsulation: false,
            })
        }
    }
}

unsafe fn decapsulate(ctx: *mut openssl_sys::EVP_PKEY_CTX, ciphertext: &[u8]) -> Option<()> {
    let mut shared_len = 0;
    (unsafe {
        openssl_sys::EVP_PKEY_decapsulate(
            ctx,
            std::ptr::null_mut(),
            &mut shared_len,
            ciphertext.as_ptr(),
            ciphertext.len(),
        )
    } > 0)
        .then_some(())
}
