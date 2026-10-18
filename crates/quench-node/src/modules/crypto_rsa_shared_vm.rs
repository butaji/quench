//! Native RSA primitives called by unchanged internal/crypto/cipher.js policy.
use super::{
    crypto_asymmetric::KeyKind,
    crypto_asymmetric_operations::{RsaOperation, RsaOptions},
    crypto_digest::Digest,
};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
fn coded(c: &mut Context<'_>, code: &str, text: &str) -> RootedError {
    match c.error_rooted(text) {
        Ok(error) => {
            let name = c.string_rooted("code");
            let value = c.string_rooted(code);
            match c.define_data_property_rooted(error, name, value, true, true, true) {
                Ok(_) => c.throw(error),
                Err(error) => error,
            }
        }
        Err(error) => error,
    }
}
fn digest(c: &mut Context<'_>, arg: RootId) -> Result<Option<Digest>, RootedError> {
    let name = c.string_text(arg)?;
    name.map(|name| {
        Digest::from_sdk_name(&name)
            .map_err(|_| coded(c, "ERR_OSSL_EVP_INVALID_DIGEST", "Invalid digest used"))
    })
    .transpose()
}
pub(crate) fn operation(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let operation = c.to_string(data)?;
    let private = matches!(operation.as_str(), "privateDecrypt" | "privateEncrypt");
    let kind = if private {
        KeyKind::Private
    } else {
        KeyKind::Public
    };
    let missing = c.undefined();
    let arg = |n: usize| args.get(n).copied().unwrap_or(missing);
    let target = c.number(if private { 2.0 } else { 1.0 });
    let input = [target, arg(0), arg(1), arg(2), arg(3), arg(4)];
    let key = super::crypto_asymmetric_shared_vm::import_input(c, kind, &input)?;
    let input = c
        .bytes_prefix_rooted(arg(5), usize::MAX)
        .ok_or_else(|| RootedError::host("Native RSA input is not a BufferSource"))?;
    let padding = openssl::rsa::Padding::from_raw(c.to_number(arg(6))? as i32);
    let md = digest(c, arg(7))?;
    let mgf1 = digest(c, arg(9))?;
    let label = c.bytes_prefix_rooted(arg(8), usize::MAX);
    let options = RsaOptions {
        padding,
        digest: md.as_ref(),
        mgf1_digest: mgf1.as_ref(),
        label: label.as_deref(),
    };
    let operation = match operation.as_str() {
        "publicEncrypt" => RsaOperation::PublicEncrypt,
        "privateDecrypt" => RsaOperation::PrivateDecrypt,
        "privateEncrypt" => RsaOperation::PrivateEncrypt,
        "publicDecrypt" => RsaOperation::PublicDecrypt,
        _ => return Err(RootedError::host("Invalid native RSA operation")),
    };
    let bytes = super::crypto_asymmetric_operations::rsa(&key, operation, &input, &options)
        .map_err(|error| match error {
            super::crypto_asymmetric::KeyError::OpenSsl(stack) => {
                super::crypto_error_shared_vm::decorated(c, stack)
            }
            _ => coded(c, "ERR_CRYPTO_OPERATION_FAILED", "RSA operation failed"),
        })?;
    super::buffer_shared_vm::from_bytes(c, &bytes)
}
pub(crate) fn install(c: &mut Context<'_>, binding: RootId) -> Result<(), RootedError> {
    for name in [
        "publicEncrypt",
        "publicDecrypt",
        "privateEncrypt",
        "privateDecrypt",
    ] {
        let data = c.string_rooted(name);
        let function =
            c.host_function_with_data(crate::host::shared_vm::operation("cryptoRsaNative"), data)?;
        let property = c.string_rooted("name");
        c.define_data_property_rooted(function, property, data, false, false, true)?;
        let property = c.string_rooted(name);
        c.define_data_property_rooted(binding, property, function, true, true, true)?;
    }
    Ok(())
}
