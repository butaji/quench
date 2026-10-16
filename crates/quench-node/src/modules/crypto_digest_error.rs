//! Node26 operation-specific presentation, delegated to the shared crypto/TLS
//! error authority. SDK validation remains in the unmodified original SDK.
use super::{crypto_digest::DigestError, crypto_error_shared_vm};
use quench_runtime::{Host, NativeContext, RootedError};
use std::ffi::{c_char, c_int};
#[derive(Clone, Copy)]
pub enum Operation<'a> {
    Hash,
    Hmac(&'a str),
}
unsafe extern "C" {
    fn ERR_new();
    fn ERR_set_debug(file: *const c_char, line: c_int, function: *const c_char);
    fn ERR_set_error(library: c_int, reason: c_int, fmt: *const c_char, ...);
}
const ERR_LIB_EVP: c_int = 6; // OpenSSL err.h.
const EVP_R_NOT_XOF_OR_INVALID_LENGTH: c_int = 178; // OpenSSL>=3 evperr.h.
pub fn invalid_xof_length() -> openssl::error::ErrorStack {
    unsafe {
        ERR_new();
        ERR_set_debug(
            c"crypto_digest.rs".as_ptr(),
            line!() as c_int,
            c"validate_output_length".as_ptr(),
        );
        ERR_set_error(
            ERR_LIB_EVP,
            EVP_R_NOT_XOF_OR_INVALID_LENGTH,
            std::ptr::null(),
        );
    }
    openssl::error::ErrorStack::get()
}
pub fn throw<H: Host>(
    context: &mut NativeContext<'_, H>,
    operation: Operation<'_>,
    error: DigestError,
) -> RootedError {
    let root = match error {
        DigestError::Unsupported => {
            let result = match operation {
                Operation::Hash => context.error_rooted("Digest method not supported"),
                Operation::Hmac(name) => context.type_error_rooted(&format!(
                    "Invalid digest: {}",
                    name.split('\0').next().unwrap()
                )),
            };
            let root = match result {
                Ok(root) => root,
                Err(e) => return e,
            };
            if matches!(operation, Operation::Hmac(_)) {
                let key = context.string_rooted("code");
                let code = context.string_rooted("ERR_CRYPTO_INVALID_DIGEST");
                if let Err(e) =
                    context.define_data_property_rooted(root, key, code, true, true, true)
                {
                    return e;
                }
            }
            root
        }
        DigestError::InvalidLength { .. } => {
            return crypto_error_shared_vm::decorated(context, invalid_xof_length());
        }
        DigestError::OpenSsl(stack) => return crypto_error_shared_vm::decorated(context, stack),
        DigestError::Allocation => {
            match context.range_error_rooted("Digest output allocation failed") {
                Ok(root) => root,
                Err(e) => return e,
            }
        }
    };
    context.throw(root)
}
