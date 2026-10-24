//! Owned one-shot signature work. Only native owners and byte snapshots cross
//! the existing task-pool boundary; sync byte views are supplied at run time.
use super::{
    crypto_asymmetric::{KeyError, OwnedKey},
    crypto_asymmetric_operations::{sign, verify, SignatureOptions},
    crypto_digest::Digest,
};
use foreign_types::ForeignType;
use openssl::md_ctx::MdCtx;
use std::{
    ffi::{c_char, c_void},
    ptr,
    sync::Arc,
};
unsafe extern "C" {
    fn EVP_PKEY_get0_type_name(key: *const openssl_sys::EVP_PKEY) -> *const c_char;
    fn EVP_SIGNATURE_fetch(
        ctx: *mut c_void,
        name: *const c_char,
        properties: *const c_char,
    ) -> *mut c_void;
    fn EVP_SIGNATURE_free(signature: *mut c_void);
    fn EVP_SIGNATURE_settable_ctx_params(
        signature: *const c_void,
    ) -> *const openssl_sys::OSSL_PARAM;
    fn OSSL_PARAM_locate_const(
        params: *const openssl_sys::OSSL_PARAM,
        name: *const c_char,
    ) -> *const openssl_sys::OSSL_PARAM;
    fn OSSL_PARAM_construct_utf8_string(
        name: *const c_char,
        value: *mut c_char,
        size: usize,
    ) -> openssl_sys::OSSL_PARAM;
    fn EVP_DigestSignInit_ex(
        ctx: *mut openssl_sys::EVP_MD_CTX,
        pkeyctx: *mut *mut openssl_sys::EVP_PKEY_CTX,
        mdname: *const c_char,
        libctx: *mut c_void,
        properties: *const c_char,
        key: *mut openssl_sys::EVP_PKEY,
        params: *const openssl_sys::OSSL_PARAM,
    ) -> i32;
    fn EVP_DigestVerifyInit_ex(
        ctx: *mut openssl_sys::EVP_MD_CTX,
        pkeyctx: *mut *mut openssl_sys::EVP_PKEY_CTX,
        mdname: *const c_char,
        libctx: *mut c_void,
        properties: *const c_char,
        key: *mut openssl_sys::EVP_PKEY,
        params: *const openssl_sys::OSSL_PARAM,
    ) -> i32;
}
pub(crate) fn supports_context(key: &OwnedKey) -> bool {
    let Some(algorithm) = super::crypto_provider_facts::key_algorithm_for(key.raw()) else {
        return false;
    };
    if !algorithm.is_one_shot() {
        return false;
    }
    let _scope = super::crypto_provider_facts::ErrorScope::new();
    unsafe {
        let signature = EVP_SIGNATURE_fetch(
            ptr::null_mut(),
            EVP_PKEY_get0_type_name(key.raw()),
            ptr::null(),
        );
        if signature.is_null() {
            return false;
        }
        let params = EVP_SIGNATURE_settable_ctx_params(signature);
        let supported = !params.is_null()
            && !OSSL_PARAM_locate_const(params, c"context-string".as_ptr()).is_null()
            && (algorithm.name() != "Ed25519"
                || !OSSL_PARAM_locate_const(params, c"instance".as_ptr()).is_null());
        EVP_SIGNATURE_free(signature);
        supported
    }
}
#[repr(u32)]
pub(crate) enum Mode {
    Sign = 0,
    Verify = 1,
}
#[repr(u32)]
pub(crate) enum Encoding {
    Der = 0,
    P1363 = 1,
}
pub(crate) const MAX_CONTEXT_STRING_BYTES: usize = 255;
pub(crate) const CONSTANTS: &[(&str, u32)] = &[
    ("kSignJobModeSign", Mode::Sign as u32),
    ("kSignJobModeVerify", Mode::Verify as u32),
    ("kSigEncDER", Encoding::Der as u32),
    ("kSigEncP1363", Encoding::P1363 as u32),
];
pub(crate) struct Request {
    pub key: Arc<OwnedKey>,
    pub digest: Option<Arc<Digest>>,
    pub data: Option<Vec<u8>>,
    pub signature: Option<Vec<u8>>,
    pub verify: bool,
    pub options: SignatureOptions,
    pub context: Option<Vec<u8>>,
}
pub(crate) enum Output {
    Signature(Vec<u8>),
    Verified(bool),
}
impl Request {
    pub(crate) fn run(self, data: Vec<u8>, signature: Vec<u8>) -> Result<Output, KeyError> {
        if self.context.as_ref().is_none_or(Vec::is_empty) {
            if let Some(digest) = self.digest.as_ref() {
                // The streaming Sign API uses an incremental MdCtx, while
                // WebCrypto one-shot RSA-PSS uses a direct digest-sign path.
                // OpenSSL providers can reject that path for ordinary RSA
                // keys even though the equivalent PkeyCtx operation works.
                // Hash the supplied bytes once and use the same prehashed
                // operation as Sign.final() for RSA-PSS.
                if self.options.padding == Some(openssl::rsa::Padding::PKCS1_PSS)
                    && matches!(
                        self.key.public_ref().id(),
                        openssl::pkey::Id::RSA | openssl::pkey::Id::RSA_PSS
                    )
                {
                    let digest_bytes = digest.hash(&data, None).map_err(|error| match error {
                        super::crypto_digest::DigestError::OpenSsl(stack) => {
                            KeyError::OpenSsl(stack)
                        }
                        _ => KeyError::Unsupported("RSA-PSS digest operation failed"),
                    })?;
                    return if self.verify {
                        super::crypto_asymmetric_operations::verify_prehashed(
                            &self.key,
                            digest,
                            &digest_bytes,
                            &signature,
                            &self.options,
                        )
                        .map(Output::Verified)
                    } else {
                        super::crypto_asymmetric_operations::sign_prehashed(
                            &self.key,
                            digest,
                            &digest_bytes,
                            &self.options,
                        )
                        .map(Output::Signature)
                    };
                }
                return if self.verify {
                    verify(&self.key, digest, &data, &signature, &self.options)
                        .map(Output::Verified)
                } else {
                    sign(&self.key, digest, &data, &self.options).map(Output::Signature)
                };
            }
        }
        let mut ctx = MdCtx::new()?;
        let nonempty = self.context.as_ref().filter(|bytes| !bytes.is_empty());
        if let Some(context) = nonempty {
            let mut params = Vec::new();
            unsafe {
                if super::crypto_provider_facts::key_algorithm_for(self.key.raw())
                    .is_some_and(|algorithm| algorithm.name() == "Ed25519")
                {
                    params.push(OSSL_PARAM_construct_utf8_string(
                        c"instance".as_ptr(),
                        c"Ed25519ctx".as_ptr().cast_mut(),
                        0,
                    ))
                }
                params.push(openssl_sys::OSSL_PARAM_construct_octet_string(
                    c"context-string".as_ptr(),
                    context.as_ptr().cast_mut().cast(),
                    context.len(),
                ));
                params.push(openssl_sys::OSSL_PARAM_construct_end());
                let status = if self.verify {
                    EVP_DigestVerifyInit_ex(
                        ctx.as_ptr(),
                        ptr::null_mut(),
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null(),
                        self.key.raw(),
                        params.as_ptr(),
                    )
                } else {
                    EVP_DigestSignInit_ex(
                        ctx.as_ptr(),
                        ptr::null_mut(),
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null(),
                        self.key.raw(),
                        params.as_ptr(),
                    )
                };
                if status <= 0 {
                    return Err(KeyError::OpenSsl(openssl::error::ErrorStack::get()));
                }
            }
        } else if self.verify {
            let pkey = ctx.digest_verify_init(None, self.key.public_ref())?;
            super::crypto_asymmetric_operations::apply_signature_options(
                pkey,
                self.key.public_ref().id(),
                &self.options,
            )?;
        } else {
            let pkey = ctx.digest_sign_init(
                None,
                self.key
                    .private_ref()
                    .ok_or(KeyError::Unsupported("private key required"))?,
            )?;
            super::crypto_asymmetric_operations::apply_signature_options(
                pkey,
                self.key.public_ref().id(),
                &self.options,
            )?;
        }
        if self.verify {
            Ok(Output::Verified(
                ctx.digest_verify(&data, &signature).unwrap_or(false),
            ))
        } else {
            let mut output = Vec::new();
            ctx.digest_sign_to_vec(&data, &mut output)?;
            if self.options.ieee_p1363 {
                output =
                    super::crypto_asymmetric_operations::to_p1363(self.key.public_ref(), &output)?;
            }
            Ok(Output::Signature(output))
        }
    }
}
