//! Projects the native constructor ABI after unchanged SDK policy validation.
use super::{
    crypto_asymmetric::Format,
    crypto_digest::Digest,
    crypto_keypair_request::{
        DhOwned, EncodingOptions, Family, OutputEncoding, Parameters, PssParameters, Request,
    },
    crypto_secret_material::Material,
};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::sync::Arc;
type Context<'a> = NativeContext<'a, NodeHost>;
fn fail(c: &mut Context<'_>, code: &str, message: &str) -> RootedError {
    match c.type_error_rooted(message) {
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
fn number(c: &mut Context<'_>, value: RootId) -> Result<u32, RootedError> {
    Ok(c.to_number(value)? as u32)
}
fn string(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    c.string_text(value)?.ok_or_else(|| {
        fail(
            c,
            "ERR_INVALID_ARG_TYPE",
            "Expected native key generation string",
        )
    })
}
fn absent(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value)
        .is_some_and(|v| v.is_undefined() || v.is_null())
}
fn digest(
    c: &mut Context<'_>,
    value: RootId,
    mgf: bool,
) -> Result<Option<Arc<Digest>>, RootedError> {
    if absent(c, value) {
        return Ok(None);
    }
    let name = string(c, value)?;
    Digest::from_sdk_name(&name)
        .map(Arc::new)
        .map(Some)
        .map_err(|_| {
            fail(
                c,
                "ERR_CRYPTO_INVALID_DIGEST",
                &format!(
                    "Invalid {}digest: {}",
                    if mgf { "MGF1 " } else { "" },
                    super::crypto_strings::null_terminated_prefix(&name)
                ),
            )
        })
}
fn enc(
    c: &mut Context<'_>,
    format: RootId,
    kind: RootId,
) -> Result<Option<OutputEncoding>, RootedError> {
    if absent(c, format) {
        return Ok(None);
    }
    use super::crypto_key_layout::{Encoding as NativeEncoding, Format as NativeFormat};
    let format = match NativeFormat::from_native(number(c, format)?) {
        Some(NativeFormat::Der) => Format::Der,
        Some(NativeFormat::Pem) => Format::Pem,
        Some(NativeFormat::Jwk) => return Ok(Some(OutputEncoding::Jwk)),
        Some(NativeFormat::RawPublic) => {
            let compressed = match number(c, kind)? {
                2 => true,  // POINT_CONVERSION_COMPRESSED
                4 => false, // POINT_CONVERSION_UNCOMPRESSED
                _ => {
                    return Err(fail(
                        c,
                        "ERR_INVALID_ARG_VALUE",
                        "Unsupported raw public point conversion",
                    ));
                }
            };
            return Ok(Some(OutputEncoding::RawPublic { compressed }));
        }
        Some(NativeFormat::RawPrivate) => return Ok(Some(OutputEncoding::RawPrivate)),
        Some(NativeFormat::RawSeed) => return Ok(Some(OutputEncoding::RawSeed)),
        _ => {
            return Err(fail(
                c,
                "ERR_INVALID_ARG_VALUE",
                "Unsupported native output format",
            ));
        }
    };
    let kind = NativeEncoding::from_native(number(c, kind)?)
        .ok_or_else(|| {
            fail(
                c,
                "ERR_INVALID_ARG_VALUE",
                "Unsupported native output encoding",
            )
        })?
        .provider();
    Ok(Some(OutputEncoding::Bytes(format, kind)))
}
pub(crate) fn configure(
    c: &mut Context<'_>,
    family: Family,
    args: &[RootId],
) -> Result<(Request, Vec<(&'static str, RootId)>), RootedError> {
    let missing = c.undefined();
    let arg = |n: usize| args.get(n).copied().unwrap_or(missing);
    let mode = number(c, arg(0))?;
    let mut offset;
    let parameters = match family {
        Family::Rsa => {
            let variant = number(c, arg(1))?;
            let bits = number(c, arg(2))?;
            let exponent = number(c, arg(3))?;
            let pss = if variant == 1 {
                offset = 7;
                Some(PssParameters {
                    digest: digest(c, arg(4), false)?,
                    mgf1: digest(c, arg(5), true)?,
                    salt: if absent(c, arg(6)) {
                        None
                    } else {
                        Some(number(c, arg(6))?)
                    },
                })
            } else {
                offset = 4;
                None
            };
            Parameters::Rsa {
                bits,
                exponent,
                // Web Crypto's RSA-PSS hash belongs to the CryptoKey
                // algorithm metadata. The generated RSA key itself must stay
                // unrestricted so exported key material can be imported with
                // a different hash, as Node permits. Native crypto key-pair
                // generation still honors the rsa-pss restrictions.
                pss: if mode == 2 { None } else { pss },
            }
        }
        Family::Ec => {
            offset = 3;
            let curve = string(c, arg(1))?;
            if !super::crypto_asymmetric_generate::curve_available(&curve) {
                return Err(fail(c, "ERR_CRYPTO_INVALID_CURVE", "Invalid EC curve name"));
            }
            Parameters::Ec {
                curve,
                explicit: !absent(c, arg(2)) && number(c, arg(2))? == 0,
            }
        }
        Family::Dsa => {
            offset = 3;
            let divisor = c.to_number(arg(2))?;
            Parameters::Dsa {
                bits: number(c, arg(1))?,
                divisor_bits: (divisor >= 0.0).then_some(divisor as u32),
            }
        }
        Family::Dh => {
            let params = if let Some(group) = c.string_text(arg(1))? {
                offset = 2;
                if super::crypto_asymmetric_generate::standard_group(&group).is_err() {
                    let error = c.error_rooted("Unknown DH group")?;
                    let name = c.string_rooted("code");
                    let code = c.string_rooted("ERR_CRYPTO_UNKNOWN_DH_GROUP");
                    c.define_data_property_rooted(error, name, code, true, true, true)?;
                    return Err(c.throw(error));
                }
                DhOwned::Group(group)
            } else {
                offset = 3;
                match c.bytes_prefix_rooted(arg(1), usize::MAX) {
                    Some(bytes) => DhOwned::Prime(bytes),
                    None => DhOwned::Bits(number(c, arg(1))?),
                }
            };
            let generator = if offset == 2 { 2 } else { number(c, arg(2))? };
            Parameters::Dh { params, generator }
        }
        Family::Named => {
            offset = 2;
            let name = string(c, arg(1))?;
            let native = super::crypto_provider_facts::named_generation_algorithm(&name, mode == 2)
                .ok_or_else(|| {
                    fail(
                        c,
                        "ERR_INVALID_ARG_VALUE",
                        &format!(
                            "The argument 'type' must be a supported key type. Received '{}'",
                            super::crypto_strings::null_terminated_prefix(&name)
                        ),
                    )
                })?;
            Parameters::Named(native.into())
        }
    };
    let mut fields = vec![("mode", arg(0))];
    let encoding = if mode == 2 {
        for name in ["algorithm", "publicUsages", "privateUsages", "extractable"] {
            fields.push((name, arg(offset)));
            offset += 1
        }
        EncodingOptions {
            public: None,
            private: None,
            cipher: None,
            passphrase: None,
        }
    } else {
        EncodingOptions {
            public: enc(c, arg(offset), arg(offset + 1))?,
            private: enc(c, arg(offset + 2), arg(offset + 3))?,
            cipher: c.string_text(arg(offset + 4))?,
            passphrase: c
                .bytes_prefix_rooted(arg(offset + 5), usize::MAX)
                .map(Material::new),
        }
    };
    Ok((
        Request {
            parameters,
            encoding,
        },
        fields,
    ))
}
