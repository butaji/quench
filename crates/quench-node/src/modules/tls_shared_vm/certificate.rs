//! Actual X509 data supplies TLS peer-certificate projections.
use crate::host::NodeHost;
use openssl::x509::{X509NameRef, X509Ref};
use quench_runtime::{NativeContext, RootId, RootedError};
fn name_object(
    context: &mut NativeContext<'_, NodeHost>,
    name: &X509NameRef,
) -> Result<RootId, RootedError> {
    let mut fields: Vec<(String, Vec<String>)> = Vec::new();
    for entry in name.entries() {
        let key = if entry.object().nid() == openssl::nid::Nid::UNDEF {
            entry.object().to_string()
        } else {
            entry
                .object()
                .nid()
                .short_name()
                .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?
                .to_owned()
        };
        let value = entry
            .data()
            .to_string()
            .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
        if let Some((_, values)) = fields.iter_mut().find(|(name, _)| *name == key) {
            values.push(value);
        } else {
            fields.push((key, vec![value]));
        }
    }
    let object = context.object_rooted()?;
    let null = context.null();
    context.set_prototype_rooted(object, null)?;
    for (key, values) in fields {
        let values = values
            .iter()
            .map(|value| context.string_rooted(value))
            .collect::<Vec<_>>();
        let value = if values.len() == 1 {
            values[0]
        } else {
            context.array_rooted(&values)?
        };
        let key = context.string_rooted(&key);
        context.define_data_property_rooted(object, key, value, true, true, true)?;
    }
    Ok(object)
}
fn escaped_name(bytes: &[u8]) -> String {
    const ASCII_PRINTABLE_START: u8 = b' ';
    const ASCII_PRINTABLE_END: u8 = b'~';
    let safe = bytes.iter().all(|byte| {
        (ASCII_PRINTABLE_START..=ASCII_PRINTABLE_END).contains(byte)
            && !matches!(byte, b'"' | b'\\' | b',' | b'\'')
    });
    if safe {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut out = String::from("\"");
    for byte in bytes {
        match byte {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            byte if (ASCII_PRINTABLE_START..=ASCII_PRINTABLE_END).contains(byte)
                && *byte != b',' =>
            {
                out.push(char::from(*byte))
            }
            byte => out.push_str(&format!("\\u00{byte:02x}")),
        }
    }
    out.push('"');
    out
}
pub(crate) fn legacy(
    context: &mut NativeContext<'_, NodeHost>,
    cert: &X509Ref,
) -> Result<RootId, RootedError> {
    let object = context.object_rooted()?;
    let subject = name_object(context, cert.subject_name())?;
    let key = context.string_rooted("subject");
    context.define_data_property_rooted(object, key, subject, true, true, true)?;
    let issuer = name_object(context, cert.issuer_name())?;
    put(context, object, "issuer", issuer)?;
    if let Some(names) = cert.subject_alt_names() {
        let mut values = Vec::new();
        for name in names {
            use foreign_types::ForeignType;
            let native = unsafe { &*name.as_ptr() };
            let value = match native.type_ {
                openssl_sys::GEN_DNS | openssl_sys::GEN_EMAIL | openssl_sys::GEN_URI => {
                    let prefix = match native.type_ {
                        openssl_sys::GEN_DNS => "DNS:",
                        openssl_sys::GEN_EMAIL => "email:",
                        _ => "URI:",
                    };
                    let pointer = native.d.cast::<openssl_sys::ASN1_STRING>();
                    let length = unsafe { openssl_sys::ASN1_STRING_length(pointer) } as usize;
                    let bytes = if length == 0 {
                        &[][..]
                    } else {
                        unsafe {
                            std::slice::from_raw_parts(
                                openssl_sys::ASN1_STRING_get0_data(pointer),
                                length,
                            )
                        }
                    };
                    format!("{prefix}{}", escaped_name(bytes))
                }
                openssl_sys::GEN_IPADD => {
                    let bytes = name.ipaddress().unwrap();
                    const IPV4_OCTETS: usize = std::mem::size_of::<std::net::Ipv4Addr>();
                    const IPV6_OCTETS: usize = std::mem::size_of::<std::net::Ipv6Addr>();
                    if bytes.len() == IPV4_OCTETS {
                        format!(
                            "IP Address:{}.{}.{}.{}",
                            bytes[0], bytes[1], bytes[2], bytes[3]
                        )
                    } else if bytes.len() == IPV6_OCTETS {
                        let groups = bytes
                            .chunks_exact(std::mem::size_of::<u16>())
                            .map(|pair| format!("{:X}", u16::from_be_bytes([pair[0], pair[1]])))
                            .collect::<Vec<_>>();
                        format!("IP Address:{}", groups.join(":"))
                    } else {
                        format!("IP Address:<invalid length={}>", bytes.len())
                    }
                }
                openssl_sys::GEN_X400 => "X400Name:<unsupported>".to_owned(),
                openssl_sys::GEN_EDIPARTY => "EdiPartyName:<unsupported>".to_owned(),
                _ => {
                    return Err(RootedError::host(
                        "Certificate alternative name kind is not implemented",
                    ));
                }
            };
            values.push(value);
        }
        let value = context.string_rooted(&values.join(", "));
        put(context, object, "subjectaltname", value)?;
    }
    use foreign_types::ForeignTypeRef;
    let ca = unsafe {
        openssl_sys::X509_get_extension_flags(cert.as_ptr()) & openssl_sys::EXFLAG_CA != 0
    };
    let value = context.boolean(ca);
    put(context, object, "ca", value)?;
    let key = cert
        .public_key()
        .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
    if let Ok(rsa) = key.rsa() {
        let modulus = rsa
            .n()
            .to_hex_str()
            .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
        let value = context.string_rooted(&modulus);
        put(context, object, "modulus", value)?;
        let exponent = rsa
            .e()
            .to_hex_str()
            .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
        let value = context.string_rooted(&format!("0x{}", exponent.trim_start_matches('0')));
        put(context, object, "exponent", value)?;
        let bytes = key
            .public_key_to_der()
            .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
        let value = crate::modules::buffer_shared_vm::from_bytes(context, &bytes)?;
        put(context, object, "pubkey", value)?;
        let value = context.number(f64::from(rsa.n().num_bits()));
        put(context, object, "bits", value)?;
    }
    for (name, text) in [
        ("valid_from", cert.not_before().to_string()),
        ("valid_to", cert.not_after().to_string()),
    ] {
        let value = context.string_rooted(&text);
        put(context, object, name, value)?;
    }
    for (name, digest) in [
        ("fingerprint", openssl::hash::MessageDigest::sha1()),
        ("fingerprint256", openssl::hash::MessageDigest::sha256()),
        ("fingerprint512", openssl::hash::MessageDigest::sha512()),
    ] {
        let bytes = cert
            .digest(digest)
            .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
        let text = bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(":");
        let value = context.string_rooted(&text);
        put(context, object, name, value)?;
    }
    let serial = cert
        .serial_number()
        .to_bn()
        .and_then(|value| value.to_hex_str())
        .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
    let value = context.string_rooted(&serial);
    put(context, object, "serialNumber", value)?;
    let bytes = cert
        .to_der()
        .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
    let value = crate::modules::buffer_shared_vm::from_bytes(context, &bytes)?;
    put(context, object, "raw", value)?;
    Ok(object)
}

fn put(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}
