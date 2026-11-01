//! OpenSSL EVP_MAC request used by the Web Crypto KMAC job.
use crate::modules::crypto_secret_material::Material;
use openssl::error::ErrorStack;
use openssl_sys::{EVP_MAC, EVP_MAC_CTX, OSSL_PARAM};
use std::{
    ffi::{c_char, c_void, CString},
    ptr,
    sync::Arc,
};

#[derive(Clone, Copy)]
pub(crate) enum Variant {
    Kmac128,
    Kmac256,
}

pub(crate) struct Request {
    pub(crate) key: Arc<Material>,
    pub(crate) variant: Variant,
    pub(crate) data: Vec<u8>,
    pub(crate) customization: Vec<u8>,
    pub(crate) output_length: usize,
    pub(crate) signature: Option<Vec<u8>>,
}

struct MacContext {
    mac: *mut EVP_MAC,
    context: *mut EVP_MAC_CTX,
}

impl Drop for MacContext {
    fn drop(&mut self) {
        unsafe {
            if !self.context.is_null() {
                openssl_sys::EVP_MAC_CTX_free(self.context);
            }
            if !self.mac.is_null() {
                openssl_sys::EVP_MAC_free(self.mac);
            }
        }
    }
}

impl Request {
    pub(crate) fn run(
        self,
    ) -> Result<crate::modules::crypto_signature_request::Output, ErrorStack> {
        let name = match self.variant {
            Variant::Kmac128 => c"KMAC128",
            Variant::Kmac256 => c"KMAC256",
        };
        let mac =
            unsafe { openssl_sys::EVP_MAC_fetch(ptr::null_mut(), name.as_ptr(), ptr::null()) };
        if mac.is_null() {
            return Err(ErrorStack::get());
        }
        let context = unsafe { openssl_sys::EVP_MAC_CTX_new(mac) };
        if context.is_null() {
            unsafe { openssl_sys::EVP_MAC_free(mac) };
            return Err(ErrorStack::get());
        }
        let owner = MacContext { mac, context };

        // EVP_MAC's KMAC provider requires an explicit output size. The
        // customization parameter is omitted when empty, matching Node's
        // native KmacJob implementation.
        let mut output_length = self.output_length;
        let mut parameters =
            vec![unsafe { OSSL_PARAM_construct_size_t(c"size".as_ptr(), &mut output_length) }];
        if !self.customization.is_empty() {
            parameters.push(unsafe {
                openssl_sys::OSSL_PARAM_construct_octet_string(
                    c"custom".as_ptr(),
                    self.customization.as_ptr().cast_mut().cast::<c_void>(),
                    self.customization.len(),
                )
            });
        }
        parameters.push(unsafe { openssl_sys::OSSL_PARAM_construct_end() });
        let initialized = unsafe {
            openssl_sys::EVP_MAC_init(
                owner.context,
                self.key.as_ptr(),
                self.key.len(),
                parameters.as_ptr(),
            )
        } == 1;
        if !initialized {
            return Err(ErrorStack::get());
        }
        if unsafe {
            openssl_sys::EVP_MAC_update(owner.context, self.data.as_ptr(), self.data.len())
        } != 1
        {
            return Err(ErrorStack::get());
        }

        // Keep a valid output pointer for zero-length signatures while
        // advertising a zero output capacity to EVP_MAC, as Node does.
        let mut output = Vec::new();
        output
            .try_reserve_exact(self.output_length.max(1))
            .map_err(|_| ErrorStack::get())?;
        output.resize(self.output_length.max(1), 0);
        let mut written = 0;
        if unsafe {
            openssl_sys::EVP_MAC_final(
                owner.context,
                output.as_mut_ptr(),
                &mut written,
                self.output_length,
            )
        } != 1
            || written > self.output_length
        {
            return Err(ErrorStack::get());
        }
        output.truncate(written);

        if let Some(signature) = self.signature {
            let verified = output.len() == signature.len()
                && ((!output.is_empty() && openssl::memcmp::eq(&output, &signature))
                    || (output.is_empty()));
            Ok(crate::modules::crypto_signature_request::Output::Verified(
                verified,
            ))
        } else {
            Ok(crate::modules::crypto_signature_request::Output::Signature(
                output,
            ))
        }
    }
}

unsafe extern "C" {
    fn OSSL_PARAM_construct_size_t(name: *const c_char, value: *mut usize) -> OSSL_PARAM;
    fn OSSL_PARAM_construct_utf8_string(
        name: *const c_char,
        value: *mut c_char,
        size: usize,
    ) -> OSSL_PARAM;
}

/// Compute one buffered CMAC value for the public `crypto.createMac` wrapper.
pub(crate) fn cmac(key: &[u8], cipher: &str, data: &[u8]) -> Result<Vec<u8>, ErrorStack> {
    let cipher = CString::new(cipher).map_err(|_| ErrorStack::get())?;
    let mac = unsafe { openssl_sys::EVP_MAC_fetch(ptr::null_mut(), c"CMAC".as_ptr(), ptr::null()) };
    if mac.is_null() {
        return Err(ErrorStack::get());
    }
    let context = unsafe { openssl_sys::EVP_MAC_CTX_new(mac) };
    if context.is_null() {
        unsafe { openssl_sys::EVP_MAC_free(mac) };
        return Err(ErrorStack::get());
    }
    let owner = MacContext { mac, context };
    let mut cipher = cipher.into_bytes_with_nul();
    let parameters = [
        unsafe {
            OSSL_PARAM_construct_utf8_string(c"cipher".as_ptr(), cipher.as_mut_ptr().cast(), 0)
        },
        unsafe { openssl_sys::OSSL_PARAM_construct_end() },
    ];
    if unsafe {
        openssl_sys::EVP_MAC_init(owner.context, key.as_ptr(), key.len(), parameters.as_ptr())
    } != 1
    {
        return Err(ErrorStack::get());
    }
    if unsafe { openssl_sys::EVP_MAC_update(owner.context, data.as_ptr(), data.len()) } != 1 {
        return Err(ErrorStack::get());
    }
    let mut output = vec![0; 128];
    let mut written = 0;
    if unsafe {
        openssl_sys::EVP_MAC_final(
            owner.context,
            output.as_mut_ptr(),
            &mut written,
            output.len(),
        )
    } != 1
        || written > output.len()
    {
        return Err(ErrorStack::get());
    }
    output.truncate(written);
    Ok(output)
}
