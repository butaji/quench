//! Provider PBKDF2 matching the configured SDK's ncrypto operation.
use super::crypto_kdf::Context;
use super::{crypto_digest::Digest, crypto_secret_material::Material};
use foreign_types::ForeignTypeRef;
use openssl::error::ErrorStack;
use openssl_sys::{EVP_MD, OSSL_PARAM};
use std::ffi::{c_char, c_int};
unsafe extern "C" {
    fn EVP_MD_get0_name(md: *const EVP_MD) -> *const c_char;
    fn OSSL_PARAM_construct_int(name: *const c_char, value: *mut c_int) -> OSSL_PARAM;
    fn OSSL_PARAM_construct_utf8_string(
        name: *const c_char,
        value: *mut c_char,
        len: usize,
    ) -> OSSL_PARAM;
}
pub(crate) fn derive(
    digest: &Digest,
    password: &[u8],
    salt: &[u8],
    iterations: u32,
    length: usize,
) -> Result<Material, ErrorStack> {
    if password.len() > c_int::MAX as usize
        || salt.len() > c_int::MAX as usize
        || length > c_int::MAX as usize
        || iterations > c_int::MAX as u32
        || length == 0
    {
        return Err(ErrorStack::get());
    }
    let ctx = Context::fetch(c"PBKDF2")?;
    let name = unsafe { EVP_MD_get0_name(digest.native_md().as_ptr()) };
    if name.is_null() {
        return Err(ErrorStack::get());
    }
    // OpenSSL 3 SDK behavior permits short passwords/salts and iteration counts.
    let mut pkcs5: c_int = 1;
    let mut count = iterations as c_int;
    let params = unsafe {
        [
            openssl_sys::OSSL_PARAM_construct_octet_string(
                c"pass".as_ptr(),
                password.as_ptr().cast_mut().cast(),
                password.len(),
            ),
            OSSL_PARAM_construct_int(c"pkcs5".as_ptr(), &mut pkcs5),
            openssl_sys::OSSL_PARAM_construct_octet_string(
                c"salt".as_ptr(),
                salt.as_ptr().cast_mut().cast(),
                salt.len(),
            ),
            OSSL_PARAM_construct_int(c"iter".as_ptr(), &mut count),
            OSSL_PARAM_construct_utf8_string(c"digest".as_ptr(), name.cast_mut(), 0),
            openssl_sys::OSSL_PARAM_construct_end(),
        ]
    };
    let mut output = Material::new(vec![0; length]);
    if unsafe {
        openssl_sys::EVP_KDF_derive(ctx.as_ptr(), output.as_mut_ptr(), length, params.as_ptr())
    } != 1
    {
        return Err(ErrorStack::get());
    }
    Ok(output)
}
