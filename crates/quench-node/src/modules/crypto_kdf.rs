//! Shared owning lifetime for fetched OpenSSL provider KDF algorithms/contexts.
use openssl::error::ErrorStack;
use openssl_sys::{EVP_KDF, EVP_KDF_CTX};
use std::{
    ffi::CStr,
    ptr::{self, NonNull},
};
struct Algorithm(NonNull<EVP_KDF>);
impl Drop for Algorithm {
    fn drop(&mut self) {
        unsafe { openssl_sys::EVP_KDF_free(self.0.as_ptr()) }
    }
}
pub(crate) struct Context {
    pointer: NonNull<EVP_KDF_CTX>,
    _algorithm: Algorithm,
}
impl Context {
    pub(crate) fn fetch(name: &CStr) -> Result<Self, ErrorStack> {
        let algorithm = Algorithm(
            NonNull::new(unsafe {
                openssl_sys::EVP_KDF_fetch(ptr::null_mut(), name.as_ptr(), ptr::null())
            })
            .ok_or_else(ErrorStack::get)?,
        );
        let pointer = NonNull::new(unsafe { openssl_sys::EVP_KDF_CTX_new(algorithm.0.as_ptr()) })
            .ok_or_else(ErrorStack::get)?;
        Ok(Self {
            pointer,
            _algorithm: algorithm,
        })
    }
    pub(crate) fn as_ptr(&self) -> *mut EVP_KDF_CTX {
        self.pointer.as_ptr()
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        unsafe { openssl_sys::EVP_KDF_CTX_free(self.pointer.as_ptr()) }
    }
}
