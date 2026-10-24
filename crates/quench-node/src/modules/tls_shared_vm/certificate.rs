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
        // Node omits X509 name components that OpenSSL cannot convert to a
        // string (for example BIT STRING or SEQUENCE values). A malformed
        // component must not make otherwise readable certificate metadata
        // unusable.
        let Ok(value) = entry.data().to_string() else {
            continue;
        };
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
fn escaped_utf8_name(value: &str) -> String {
    let safe = value.chars().all(|character| {
        (!character.is_control() || character == '\t')
            && !matches!(character, '"' | '\\' | ',')
    });
    if safe {
        return value.to_owned();
    }
    let mut out = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            ',' => out.push_str("\\u002c"),
            character if character.is_control() => {
                let code = character as u32;
                if code <= 0xff {
                    out.push_str(&format!("\\u00{code:02x}"));
                } else {
                    out.push_str(&format!("\\u{code:04x}"));
                }
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

struct MemoryBio(*mut openssl_sys::BIO);

#[repr(C)]
struct OtherName {
    type_id: *mut openssl_sys::ASN1_OBJECT,
    value: *mut openssl_sys::ASN1_TYPE,
}

#[repr(C)]
struct Asn1Type {
    type_id: std::ffi::c_int,
    value: *mut std::ffi::c_void,
}

impl MemoryBio {
    fn new() -> Result<Self, openssl::error::ErrorStack> {
        let pointer = unsafe { openssl_sys::BIO_new(openssl_sys::BIO_s_mem()) };
        if pointer.is_null() {
            Err(openssl::error::ErrorStack::get())
        } else {
            Ok(Self(pointer))
        }
    }

    fn as_ptr(&self) -> *mut openssl_sys::BIO {
        self.0
    }

    fn text(&self) -> String {
        let mut pointer = std::ptr::null_mut();
        let length = unsafe { openssl_sys::BIO_get_mem_data(self.0, &mut pointer) };
        if length <= 0 || pointer.is_null() {
            String::new()
        } else {
            let bytes = unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), length as usize) };
            String::from_utf8_lossy(bytes).into_owned()
        }
    }
}

impl Drop for MemoryBio {
    fn drop(&mut self) {
        unsafe { openssl_sys::BIO_free_all(self.0) };
    }
}

fn distinguished_name(name: &openssl::x509::X509NameRef) -> Result<String, openssl::error::ErrorStack> {
    use foreign_types::ForeignTypeRef;
    let bio = MemoryBio::new()?;
    // OpenSSL's RFC2253 printer matches Node's field order and separators.
    // Node leaves Unicode and control characters unescaped here, then applies
    // JSON-safe escaping to the complete rendered name below.
    const XN_FLAG_RFC2253_UNESCAPED_UTF8_CTRL: u64 = 0x0111_0311;
    let result = unsafe {
        X509_NAME_print_ex(bio.as_ptr(), name.as_ptr(), 0, XN_FLAG_RFC2253_UNESCAPED_UTF8_CTRL)
    };
    if result < 0 {
        return Err(openssl::error::ErrorStack::get());
    }
    Ok(bio.text())
}

pub(crate) fn multiline_name(
    name: &openssl::x509::X509NameRef,
) -> Result<String, openssl::error::ErrorStack> {
    use foreign_types::ForeignTypeRef;
    let bio = MemoryBio::new()?;
    const XN_FLAG_MULTILINE: u64 = 0x0004_0013;
    let result = unsafe { X509_NAME_print_ex(bio.as_ptr(), name.as_ptr(), 0, XN_FLAG_MULTILINE) };
    if result < 0 {
        return Err(openssl::error::ErrorStack::get());
    }
    Ok(bio.text())
}

fn registered_id(object: *mut openssl_sys::ASN1_OBJECT) -> Option<String> {
    if object.is_null() {
        return None;
    }
    let required = unsafe { openssl_sys::OBJ_obj2txt(std::ptr::null_mut(), 0, object, 1) };
    if required <= 0 || required > 4096 {
        return None;
    }
    let mut buffer = vec![0u8; required as usize + 1];
    let written = unsafe {
        openssl_sys::OBJ_obj2txt(
            buffer.as_mut_ptr().cast(),
            buffer.len() as i32,
            object,
            1,
        )
    };
    (written > 0).then(|| String::from_utf8_lossy(&buffer[..written as usize]).into_owned())
}

fn other_name_value(name: *mut openssl_sys::GENERAL_NAME) -> Option<(String, String)> {
    if name.is_null() {
        return None;
    }
    let general = unsafe { &*name };
    if general.d.is_null() {
        return None;
    }
    let other = unsafe { &*general.d.cast::<OtherName>() };
    let oid = registered_id(other.type_id)?;
    let (label, expected_type) = match oid.as_str() {
        "1.3.6.1.5.5.7.8.5" => ("XmppAddr", 12),
        "1.3.6.1.5.5.7.8.7" => ("SRVName", 22),
        _ => return None,
    };
    if other.value.is_null() {
        return None;
    }
    let value = unsafe { &*other.value.cast::<Asn1Type>() };
    if value.type_id != expected_type || value.value.is_null() {
        return None;
    }
    let string = value.value.cast::<openssl_sys::ASN1_STRING>();
    let length = unsafe { openssl_sys::ASN1_STRING_length(string) };
    if length < 0 {
        return None;
    }
    let bytes = if length == 0 {
        &[][..]
    } else {
        let data = unsafe { openssl_sys::ASN1_STRING_get0_data(string) };
        if data.is_null() {
            return None;
        }
        unsafe { std::slice::from_raw_parts(data, length as usize) }
    };
    let text = std::str::from_utf8(bytes).ok()?;
    if expected_type == 22 && !text.is_ascii() {
        return None;
    }
    Some((label.to_owned(), text.to_owned()))
}

fn other_name(name: *mut openssl_sys::GENERAL_NAME) -> Option<String> {
    let (label, value) = other_name_value(name)?;
    Some(format!(
        "othername:{}",
        escaped_utf8_name(&format!("{label}:{value}"))
    ))
}

unsafe extern "C" {
    fn X509_NAME_print_ex(
        output: *mut openssl_sys::BIO,
        name: *const openssl_sys::X509_NAME,
        indent: std::ffi::c_int,
        flags: std::ffi::c_ulong,
    ) -> std::ffi::c_int;
}
pub(crate) fn legacy(
    context: &mut NativeContext<'_, NodeHost>,
    cert: &X509Ref,
) -> Result<RootId, RootedError> {
    use foreign_types::ForeignTypeRef;
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
                openssl_sys::GEN_DIRNAME => {
                    let Some(name) = name.directory_name() else {
                        values.push("DirName:<unsupported>".to_owned());
                        continue;
                    };
                    format!("DirName:{}", escaped_utf8_name(&distinguished_name(name).map_err(
                        |error| crate::modules::crypto_error_shared_vm::decorated(context, error),
                    )?))
                }
                openssl_sys::GEN_RID => {
                    let Some(object) = registered_id(native.d.cast()) else {
                        values.push("Registered ID:<unsupported>".to_owned());
                        continue;
                    };
                    format!("Registered ID:{object}")
                }
                openssl_sys::GEN_OTHERNAME => {
                    other_name(name.as_ptr()).unwrap_or_else(|| "othername:<unsupported>".to_owned())
                }
                openssl_sys::GEN_X400 => "X400Name:<unsupported>".to_owned(),
                openssl_sys::GEN_EDIPARTY => "EdiPartyName:<unsupported>".to_owned(),
                _ => {
                    // Preserve the present-SAN sentinel for GeneralName
                    // choices that OpenSSL can decode but cannot render.
                    values.clear();
                    values.push("DNS:".to_owned());
                    break;
                }
            };
            values.push(value);
        }
        let value = context.string_rooted(&values.join(", "));
        put(context, object, "subjectaltname", value)?;
    } else if unsafe {
        openssl_sys::X509_get_ext_by_NID(
            cert.as_ptr(),
            openssl_sys::NID_subject_alt_name,
            -1,
        ) >= 0
    } {
        // Keep a malformed but present SAN from falling back to the subject
        // common name. Node's legacy projection uses this nonmatching sentinel
        // when OpenSSL cannot decode the extension's GeneralName choices.
        let value = context.string_rooted("DNS:");
        put(context, object, "subjectaltname", value)?;
    }
    if let Some(access) = cert.authority_info() {
        let info_access = context.object_rooted()?;
        let null = context.null();
        context.set_prototype_rooted(info_access, null)?;
        let mut fields: Vec<(String, Vec<String>)> = Vec::new();
        let mut display = Vec::new();
        for description in access {
            let method = description.method().to_string();
            let location = description.location();
            let native = unsafe { &*location.as_ptr() };
            let (kind, raw) = match native.type_ {
                openssl_sys::GEN_DNS => ("DNS", location.dnsname().unwrap_or("<unsupported>").to_owned()),
                openssl_sys::GEN_EMAIL => ("email", location.email().unwrap_or("<unsupported>").to_owned()),
                openssl_sys::GEN_URI => ("URI", location.uri().unwrap_or("<unsupported>").to_owned()),
                openssl_sys::GEN_IPADD => {
                    let bytes = location.ipaddress().unwrap_or_default();
                    let value = match bytes.len() {
                        4 => format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3]),
                        16 => std::net::Ipv6Addr::from(<[u8; 16]>::try_from(bytes).unwrap()).to_string(),
                        _ => "<unsupported>".to_owned(),
                    };
                    ("IP Address", value)
                }
                openssl_sys::GEN_DIRNAME => {
                    let value = location
                        .directory_name()
                        .and_then(|name| distinguished_name(name).ok())
                        .unwrap_or_else(|| "<unsupported>".to_owned());
                    ("DirName", value)
                }
                openssl_sys::GEN_RID => (
                    "Registered ID",
                    registered_id(native.d.cast()).unwrap_or_else(|| "<unsupported>".to_owned()),
                ),
                openssl_sys::GEN_OTHERNAME => {
                    let (name, value) = other_name_value(location.as_ptr())
                        .unwrap_or_else(|| (String::new(), "<unsupported>".to_owned()));
                    let key = format!("{method} - othername");
                    let display_value = if name.is_empty() {
                        escaped_name(value.as_bytes())
                    } else {
                        escaped_utf8_name(&format!("{name}:{value}"))
                    };
                    display.push(format!("{key}:{display_value}"));
                    let property_value = if name.is_empty() { value } else { format!("{name}:{value}") };
                    if let Some((_, values)) = fields.iter_mut().find(|(field, _)| *field == key) {
                        values.push(property_value);
                    } else {
                        fields.push((key, vec![property_value]));
                    }
                    continue;
                }
                _ => ("othername", "<unsupported>".to_owned()),
            };
            let key = format!("{method} - {kind}");
            display.push(format!("{key}:{}", escaped_name(raw.as_bytes())));
            if let Some((_, values)) = fields.iter_mut().find(|(field, _)| *field == key) {
                values.push(raw);
            } else {
                fields.push((key, vec![raw]));
            }
        }
        for (key, values) in fields {
            let values = values.iter().map(|value| context.string_rooted(value)).collect::<Vec<_>>();
            let value = context.array_rooted(&values)?;
            let key = context.string_rooted(&key);
            context.define_data_property_rooted(info_access, key, value, true, true, true)?;
        }
        put(context, object, "infoAccess", info_access)?;
        let text = context.string_rooted(&display.join("\n"));
        put(context, object, "infoaccess", text)?;
    }
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
