//! Native Node HKDF: one-shot HMAC extraction followed by provider HKDF expand.
//! The fetched digest and every native context have an explicit owning lifetime.
use super::crypto_digest::Digest;
use super::crypto_kdf::Context;
use super::crypto_secret_material::Material;
use foreign_types::ForeignTypeRef;
use openssl::error::ErrorStack;
use openssl_sys::{EVP_MD, OSSL_PARAM};
use std::ffi::{c_char, c_int, c_void};
const HKDF_COUNTER_VALUES: usize = u8::MAX as usize;
// OpenSSL kdf.h's EVP_KDF_HKDF_MODE_EXPAND_ONLY, not the EVP_PKEY mode ABI.
const HKDF_EXPAND_ONLY: c_int = 2;
unsafe extern "C" {
    fn HMAC(
        md: *const EVP_MD,
        key: *const c_void,
        key_len: c_int,
        data: *const u8,
        data_len: usize,
        output: *mut u8,
        output_len: *mut u32,
    ) -> *mut u8;
    fn EVP_MD_get0_name(md: *const EVP_MD) -> *const c_char;
    fn OSSL_PARAM_construct_int(name: *const c_char, value: *mut c_int) -> OSSL_PARAM;
    fn OSSL_PARAM_construct_utf8_string(
        name: *const c_char,
        value: *mut c_char,
        len: usize,
    ) -> OSSL_PARAM;
}
pub(crate) fn maximum(digest: &Digest) -> usize {
    digest.size() * HKDF_COUNTER_VALUES
}
pub(crate) fn derive(
    digest: &Digest,
    key: &[u8],
    salt: &[u8],
    info: &[u8],
    length: usize,
) -> Result<Material, ErrorStack> {
    if length > maximum(digest)
        || salt.len() > c_int::MAX as usize
        || info.len() > c_int::MAX as usize
    {
        return Err(ErrorStack::get());
    }
    let md = digest.native_md();
    let default_salt = [0u8; openssl_sys::EVP_MAX_MD_SIZE as usize];
    let salt = if salt.is_empty() {
        &default_salt[..digest.size()]
    } else {
        salt
    };
    let mut pseudorandom = Material::new(vec![0; openssl_sys::EVP_MAX_MD_SIZE as usize]);
    let mut pseudorandom_length = pseudorandom.len() as u32;
    // Node's native implementation deliberately extracts with HMAC first: this
    // accepts empty IKM and feeds precisely one digest block to expand-only HKDF.
    let salt_len = c_int::try_from(salt.len()).map_err(|_| ErrorStack::get())?;
    if unsafe {
        HMAC(
            md.as_ptr(),
            salt.as_ptr().cast::<c_void>(),
            salt_len,
            key.as_ptr(),
            key.len(),
            pseudorandom.as_mut_ptr(),
            &mut pseudorandom_length,
        )
    }
    .is_null()
    {
        return Err(ErrorStack::get());
    }
    // Node DataPointer::Alloc(0) returns empty. Extraction still happens first;
    // do not call provider expand and manufacture a different native error.
    if length == 0 {
        return Err(ErrorStack::get());
    }
    let context = Context::fetch(c"HKDF")?;
    let name = unsafe { EVP_MD_get0_name(md.as_ptr()) };
    if name.is_null() {
        return Err(ErrorStack::get());
    }
    let mut mode = HKDF_EXPAND_ONLY;
    let mut params = Vec::with_capacity(5);
    unsafe {
        params.push(OSSL_PARAM_construct_int(c"mode".as_ptr(), &mut mode));
        params.push(OSSL_PARAM_construct_utf8_string(
            c"digest".as_ptr(),
            name.cast_mut(),
            0,
        ));
        params.push(openssl_sys::OSSL_PARAM_construct_octet_string(
            c"key".as_ptr(),
            pseudorandom.as_mut_ptr().cast(),
            pseudorandom_length as usize,
        ));
        if !info.is_empty() {
            params.push(openssl_sys::OSSL_PARAM_construct_octet_string(
                c"info".as_ptr(),
                info.as_ptr().cast_mut().cast(),
                info.len(),
            ));
        }
        params.push(openssl_sys::OSSL_PARAM_construct_end());
    }
    let mut output = Material::new(vec![0; length]);
    if unsafe {
        openssl_sys::EVP_KDF_derive(
            context.as_ptr(),
            output.as_mut_ptr(),
            length,
            params.as_ptr(),
        )
    } != 1
    {
        return Err(ErrorStack::get());
    }
    Ok(output)
}
