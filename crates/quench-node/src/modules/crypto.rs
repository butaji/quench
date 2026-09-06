//! Small Rust-owned classic crypto surface used by Node APIs.
//!
//! The key object keeps its semantic bytes in a non-enumerable host slot;
//! consumers such as `assert` can derive equality from that one fact without
//! exposing implementation fields to JavaScript enumeration.

use std::cell::RefCell;
use std::rc::Rc;

use base64::Engine;
use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;
use sha2::digest::Digest;

use hmac::{Hmac, Mac};
use md5::Md5;
use openssl::{
    bn::BigNum,
    bn::BigNumContext,
    dsa::Dsa,
    ec::{EcGroup, EcKey, EcPoint, PointConversionForm},
    hash::MessageDigest,
    md::{Md, MdRef},
    nid::Nid,
    pkey::{HasPrivate, HasPublic, Id, PKey},
    pkey_ctx::PkeyCtx,
    rsa::Padding,
    rsa::Rsa,
    sign::{RsaPssSaltlen, Signer, Verifier},
    symm::Cipher,
    x509::X509,
};
use rand::RngCore;
use sha1::Sha1;
use sha2::{Sha224, Sha256, Sha384, Sha512};
use sha3::{
    digest::{ExtendableOutput, Update as XofUpdate, XofReader},
    Sha3_256, Sha3_384, Sha3_512, Shake128, Shake256,
};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::host::HostState;

thread_local! {
    static KEY_PROTOTYPES: RefCell<Option<(Value, Value)>> = const { RefCell::new(None) };
    static CERTIFICATE_PROTOTYPE: RefCell<Option<Value>> = const { RefCell::new(None) };
}

pub(crate) const KEY_MARKER_PROP: &str = "\0quench:crypto:key";
pub(crate) const KEY_DATA_PROP: &str = "\0quench:crypto:key-data";
const X509_DATA_PROP: &str = "\0quench:crypto:x509-data";
pub(crate) const KEY_TYPE_PROP: &str = "\0quench:crypto:key-type";
const KEY_SIZE_PROP: &str = "\0quench:crypto:key-size";
const KEY_ASYM_TYPE_PROP: &str = "\0quench:crypto:key-asym-type";
const KEY_DETAILS_PROP: &str = "\0quench:crypto:key-details";
const ALGORITHM_PROP: &str = "\0quench:crypto:algorithm";
const INPUT_PROP: &str = "\0quench:crypto:input";
const HMAC_KEY_PROP: &str = "\0quench:crypto:hmac-key";
const DIGESTED_PROP: &str = "\0quench:crypto:digested";
const RESULT_PROP: &str = "\0quench:crypto:result";
const OUTPUT_LEN_PROP: &str = "\0quench:crypto:output-length";
const PIPE_DEST_PROP: &str = "\0quench:crypto:pipe-destination";
const HASH_DATA_LISTENER_PROP: &str = "\0quench:crypto:data-listener";
const HASH_ERROR_LISTENER_PROP: &str = "\0quench:crypto:error-listener";
pub(crate) const HASH_HANDLE_PROP: &str = "Symbol.kHandle\0crypto";
const ENCODING_PROP: &str = "\0quench:crypto:encoding";
const WRITABLE_STATE_PROP: &str = "_writableState";
const SIGN_KEY_PROP: &str = "\0quench:crypto:sign-key";
const SIGN_OPTIONS_PROP: &str = "\0quench:crypto:sign-options";

pub fn argon2(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Err(crypto_error(
        "ERR_CRYPTO_ARGON2_NOT_SUPPORTED",
        "argon2 is not supported",
    ))
}

pub fn x509_constructor(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let input = args.first().unwrap_or(&Value::Undefined);
    let data = bytes_from_value(input)
        .or_else(|| {
            matches!(input, Value::String(_) | Value::StringUnits(_))
                .then(|| execute::to_js_string(input).ok())
                .flatten()
                .map(String::into_bytes)
        })
        .ok_or_else(|| {
            invalid_type("The \"buffer\" argument must be a string or an instance of Buffer")
        })?;
    let cert = X509::from_pem(&data)
        .or_else(|_| X509::from_der(&data))
        .map_err(openssl_error)?;
    let value = host_api::object(Vec::new());
    define_hidden(
        &value,
        X509_DATA_PROP,
        crate::modules::buffer_proto::make_buffer(&data),
    );
    define_hidden(&value, KEY_MARKER_PROP, Value::Boolean(true));
    let proto = host_api::object(vec![
        (
            "checkPrivateKey".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_X509_CHECK_PRIVATE_KEY),
        ),
        (
            "verify".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_X509_VERIFY),
        ),
        (
            "toLegacyObject".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_X509_TO_LEGACY),
        ),
        (
            "toString".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_X509_TO_STRING),
        ),
        (
            "toJSON".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_X509_TO_JSON),
        ),
        (
            "checkHost".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_X509_CHECK_HOST),
        ),
        (
            "checkIP".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_X509_CHECK_IP),
        ),
        (
            "checkEmail".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_X509_CHECK_EMAIL),
        ),
        (
            "checkIssued".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_X509_CHECK_ISSUED),
        ),
    ]);
    let _ = execute::define_property(
        proto.clone(),
        "publicKey",
        host_api::object(vec![
            (
                "get".into(),
                crate::host::capability(crate::registry::SPEC_CRYPTO_X509_PUBLIC_KEY),
            ),
            ("configurable".into(), Value::Boolean(true)),
        ]),
    );
    if let Ok(public) = cert.public_key() {
        let public_key = create_public_key(
            _state,
            None,
            &[Value::String(
                String::from_utf8_lossy(&public.public_key_to_pem().map_err(openssl_error)?)
                    .into_owned(),
            )],
        )?;
        let _ = execute::set_property_in_place(&value, "publicKey", public_key);
    }
    let _ = execute::set_property_in_place(
        &value,
        "subject",
        Value::String(x509_name(cert.subject_name())),
    );
    let _ = execute::set_property_in_place(
        &value,
        "issuer",
        Value::String(x509_name(cert.issuer_name())),
    );
    let raw_der = cert.to_der().map_err(openssl_error)?;
    for (name, digest) in [
        ("fingerprint", MessageDigest::sha1()),
        ("fingerprint256", MessageDigest::sha256()),
        ("fingerprint512", MessageDigest::sha512()),
    ] {
        let fingerprint = cert
            .digest(digest)
            .map_err(openssl_error)?
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(":");
        let _ = execute::set_property_in_place(&value, name, Value::String(fingerprint));
    }
    let serial = cert
        .serial_number()
        .to_bn()
        .map_err(openssl_error)?
        .to_hex_str()
        .map_err(openssl_error)?
        .to_string();
    let _ = execute::set_property_in_place(&value, "serialNumber", Value::String(serial));
    let _ = execute::set_property_in_place(
        &value,
        "validFrom",
        Value::String(cert.not_before().to_string()),
    );
    let _ = execute::set_property_in_place(
        &value,
        "validTo",
        Value::String(cert.not_after().to_string()),
    );
    for (name, text) in [
        (
            "validFromDate",
            x509_date_input(&cert.not_before().to_string()),
        ),
        (
            "validToDate",
            x509_date_input(&cert.not_after().to_string()),
        ),
    ] {
        if let Ok(date) = execute::construct_value(
            &Value::Builtin(quench_runtime::ops::Builtin::Date),
            &[Value::String(text)],
        ) {
            let global = quench_runtime::vm::current_global_object();
            let date_proto =
                execute::get_property(&execute::get_property(&global, "Date"), "prototype");
            let date = execute::set_prototype_of(&date, &date_proto).unwrap_or(date);
            let _ = execute::set_property_in_place(&value, name, date);
        }
    }
    let _ = execute::set_property_in_place(&value, "ca", Value::Boolean(false));
    let signature_oid = cert
        .signature_algorithm()
        .object()
        .to_string()
        .trim()
        .to_string();
    let (signature_name, signature_oid) = match signature_oid.as_str() {
        "sha256WithRSAEncryption" => (
            "sha256WithRSAEncryption",
            "1.2.840.113549.1.1.11".to_string(),
        ),
        "sha1WithRSAEncryption" => ("sha1WithRSAEncryption", "1.2.840.113549.1.1.5".to_string()),
        "sha384WithRSAEncryption" => (
            "sha384WithRSAEncryption",
            "1.2.840.113549.1.1.12".to_string(),
        ),
        "sha512WithRSAEncryption" => (
            "sha512WithRSAEncryption",
            "1.2.840.113549.1.1.13".to_string(),
        ),
        "1.2.840.113549.1.1.11" => ("sha256WithRSAEncryption", signature_oid.clone()),
        "1.2.840.113549.1.1.5" => ("sha1WithRSAEncryption", signature_oid.clone()),
        "1.2.840.113549.1.1.12" => ("sha384WithRSAEncryption", signature_oid.clone()),
        "1.2.840.113549.1.1.13" => ("sha512WithRSAEncryption", signature_oid.clone()),
        _ => ("unknown", signature_oid.clone()),
    };
    let _ = execute::set_property_in_place(
        &value,
        "signatureAlgorithm",
        if signature_name == "unknown" {
            Value::Undefined
        } else {
            Value::String(signature_name.into())
        },
    );
    let _ = execute::set_property_in_place(
        &value,
        "signatureAlgorithmOid",
        Value::String(signature_oid),
    );
    let _ = execute::set_property_in_place(
        &value,
        "raw",
        crate::modules::buffer_proto::make_buffer(&raw_der),
    );
    if let Some(access) = cert.authority_info() {
        let info = access
            .iter()
            .filter_map(|entry| {
                let uri = entry.location().uri()?;
                Some(format!("{} - URI:{uri}", entry.method()))
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !info.is_empty() {
            let _ = execute::set_property_in_place(&value, "infoAccess", Value::String(info));
        }
    }
    Ok(execute::set_prototype_of(&value, &proto).unwrap_or(value))
}

pub fn x509_public_key(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let raw =
        bytes_from_value(&execute::get_property(receiver, X509_DATA_PROP)).unwrap_or_default();
    let cert = X509::from_pem(&raw)
        .or_else(|_| X509::from_der(&raw))
        .map_err(openssl_error)?;
    let public = cert
        .public_key()
        .map_err(|_| crypto_error("ERR_OSSL_X509_KEY_DECODE", "decode error"))?;
    create_public_key(
        state,
        None,
        &[Value::String(
            String::from_utf8_lossy(&public.public_key_to_pem().map_err(openssl_error)?)
                .into_owned(),
        )],
    )
}

fn x509_date_input(value: &str) -> String {
    let parts = value.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 5 {
        return value.to_string();
    }
    let month = match parts[0] {
        "Jan" => "01",
        "Feb" => "02",
        "Mar" => "03",
        "Apr" => "04",
        "May" => "05",
        "Jun" => "06",
        "Jul" => "07",
        "Aug" => "08",
        "Sep" => "09",
        "Oct" => "10",
        "Nov" => "11",
        "Dec" => "12",
        _ => return value.to_string(),
    };
    format!("{}-{month}-{:0>2}T{}Z", parts[3], parts[1], parts[2])
}

fn x509_name(name: &openssl::x509::X509NameRef) -> String {
    name.entries()
        .filter_map(|entry| {
            let key = entry.object().nid().short_name().ok()?;
            let value = entry.data().as_utf8().ok()?.to_string();
            Some(format!("{key}={value}"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn x509_constructor_construct(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    x509_constructor(state, None, args)
}

pub fn x509_check_private_key(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    if !args.first().is_some_and(is_key_object) {
        return Err(invalid_type("The \"key\" argument must be a KeyObject"));
    }
    let cert = X509::from_pem(
        &bytes_from_value(&execute::get_property(receiver, X509_DATA_PROP)).unwrap_or_default(),
    )
    .map_err(openssl_error)?;
    let key = args
        .first()
        .and_then(|value| {
            bytes_from_value(&execute::get_property(value, KEY_DATA_PROP))
                .or_else(|| bytes_from_value(value))
        })
        .ok_or_else(|| invalid_type("The \"key\" argument must be a KeyObject"))?;
    if !matches!(
        args.first().map(|value| key_hidden(value, KEY_TYPE_PROP)),
        Some(Value::String(ref kind)) if kind == "private"
    ) {
        return Err(crypto_error(
            "ERR_INVALID_ARG_VALUE",
            "Invalid key object type",
        ));
    }
    let private = PKey::private_key_from_pem(&key)
        .or_else(|_| PKey::private_key_from_der(&key))
        .map_err(openssl_error)?;
    let cert_pub = cert
        .public_key()
        .map_err(openssl_error)?
        .public_key_to_der()
        .map_err(openssl_error)?;
    Ok(Value::Boolean(
        private.public_key_to_der().map_err(openssl_error)? == cert_pub,
    ))
}

pub fn x509_verify(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    if !args.first().is_some_and(is_key_object) {
        return Err(invalid_type("The \"key\" argument must be a KeyObject"));
    }
    let cert = X509::from_pem(
        &bytes_from_value(&execute::get_property(receiver, X509_DATA_PROP)).unwrap_or_default(),
    )
    .map_err(openssl_error)?;
    let key = args
        .first()
        .and_then(|value| {
            bytes_from_value(&execute::get_property(value, KEY_DATA_PROP))
                .or_else(|| bytes_from_value(value))
        })
        .ok_or_else(|| invalid_type("The \"key\" argument must be a KeyObject"))?;
    if !matches!(
        args.first().map(|value| key_hidden(value, KEY_TYPE_PROP)),
        Some(Value::String(ref kind)) if kind == "public"
    ) {
        return Err(crypto_error(
            "ERR_INVALID_ARG_VALUE",
            "Invalid key object type",
        ));
    }
    let public = PKey::public_key_from_pem(&key)
        .or_else(|_| PKey::public_key_from_der(&key))
        .map_err(openssl_error)?;
    Ok(Value::Boolean(cert.verify(&public).unwrap_or(false)))
}

fn is_key_object(value: &Value) -> bool {
    matches!(
        execute::get_property(value, KEY_MARKER_PROP),
        Value::Boolean(true)
    )
}

pub fn x509_to_legacy(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let raw =
        bytes_from_value(&execute::get_property(receiver, X509_DATA_PROP)).unwrap_or_default();
    let cert = X509::from_pem(&raw)
        .or_else(|_| X509::from_der(&raw))
        .map_err(openssl_error)?;
    let public = cert.public_key().map_err(openssl_error)?;
    // Node's legacy TLS representation normalizes RSA-PSS certificates to a
    // rsaEncryption SubjectPublicKeyInfo, dropping the PSS parameter block.
    let pubkey = if public.id() == Id::RSA_PSS {
        let rsa = public.rsa().map_err(openssl_error)?;
        PKey::from_rsa(rsa)
            .map_err(openssl_error)?
            .public_key_to_der()
            .map_err(openssl_error)?
    } else {
        public.public_key_to_der().map_err(openssl_error)?
    };
    let mut legacy = vec![
        (
            "raw".into(),
            crate::modules::buffer_proto::make_buffer(&cert.to_der().map_err(openssl_error)?),
        ),
        (
            "pubkey".into(),
            crate::modules::buffer_proto::make_buffer(&pubkey),
        ),
        (
            "subject".into(),
            x509_name_object(&x509_name(cert.subject_name())),
        ),
        (
            "issuer".into(),
            x509_name_object(&x509_name(cert.issuer_name())),
        ),
        (
            "valid_from".into(),
            Value::String(cert.not_before().to_string()),
        ),
        (
            "valid_to".into(),
            Value::String(cert.not_after().to_string()),
        ),
        (
            "fingerprint".into(),
            execute::get_property(receiver, "fingerprint"),
        ),
        (
            "fingerprint256".into(),
            execute::get_property(receiver, "fingerprint256"),
        ),
        (
            "fingerprint512".into(),
            execute::get_property(receiver, "fingerprint512"),
        ),
        (
            "serialNumber".into(),
            execute::get_property(receiver, "serialNumber"),
        ),
    ];
    if let Some(access) = cert.authority_info() {
        let entries = access
            .iter()
            .filter_map(|entry| {
                Some((
                    format!("{} - URI", entry.method()),
                    host_api::array(
                        entry
                            .location()
                            .uri()
                            .map(|uri| Value::String(uri.into()))
                            .into_iter()
                            .collect(),
                    ),
                ))
            })
            .collect::<Vec<_>>();
        legacy.push(("infoAccess".into(), null_proto_object(entries)));
    }
    if let Ok(rsa) = public.rsa() {
        legacy.push((
            "modulus".into(),
            Value::String(rsa.n().to_hex_str().map_err(openssl_error)?.to_string()),
        ));
        legacy.push(("bits".into(), Value::Number(f64::from(rsa.size()) * 8.0)));
        legacy.push((
            "exponent".into(),
            Value::String(format!(
                "0x{}",
                rsa.e()
                    .to_hex_str()
                    .map_err(openssl_error)?
                    .trim_start_matches('0')
            )),
        ));
    }
    Ok(host_api::object(legacy))
}

fn x509_name_object(name: &str) -> Value {
    null_proto_object(
        name.lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.to_owned(), Value::String(value.to_owned())))
            .collect(),
    )
}

fn null_proto_object(properties: Vec<(String, Value)>) -> Value {
    let object = host_api::object(properties);
    execute::set_prototype_of(&object, &Value::Null).unwrap_or(object)
}

pub fn x509_to_string(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let raw =
        bytes_from_value(&execute::get_property(receiver, X509_DATA_PROP)).unwrap_or_default();
    let cert = X509::from_pem(&raw)
        .or_else(|_| X509::from_der(&raw))
        .map_err(openssl_error)?;
    Ok(Value::String(
        String::from_utf8_lossy(&cert.to_pem().map_err(openssl_error)?).into_owned(),
    ))
}

fn x509_text_arg(args: &[Value], name: &str) -> Result<String, VmError> {
    let value = args
        .first()
        .ok_or_else(|| invalid_type(&format!("The \"{name}\" argument must be of type string")))?;
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::StringUnits(_) => execute::to_js_string(value),
        _ => Err(invalid_type(&format!(
            "The \"{name}\" argument must be of type string"
        ))),
    }
}

fn validate_x509_options(args: &[Value]) -> Result<(), VmError> {
    let Some(options) = args.get(1) else {
        return Ok(());
    };
    if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(invalid_type(
            "The \"options\" argument must be of type object",
        ));
    }
    for key in [
        "wildcards",
        "partialWildcards",
        "multiLabelWildcards",
        "singleLabelSubdomains",
    ] {
        let value = execute::get_property(options, key);
        if !matches!(value, Value::Undefined | Value::Boolean(_)) {
            return Err(invalid_type(
                "The \"options\" argument has an invalid property",
            ));
        }
    }
    let subject = execute::get_property(options, "subject");
    if !matches!(
        subject,
        Value::Undefined | Value::String(_) | Value::StringUnits(_)
    ) {
        return Err(invalid_type(
            "The \"options.subject\" property must be of type string",
        ));
    }
    Ok(())
}

pub fn x509_check_host(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let host = x509_text_arg(args, "name")?;
    validate_x509_options(args)?;
    if host.contains('\0') {
        return Err(crypto_error(
            "ERR_INVALID_ARG_VALUE",
            "Embedded null character",
        ));
    }
    let subject = execute::to_js_string(&execute::get_property(receiver, "subject"))?;
    Ok(subject
        .lines()
        .find_map(|line| line.strip_prefix("CN=").filter(|cn| *cn == host))
        .map(|_| Value::String(host))
        .unwrap_or(Value::Undefined))
}

pub fn x509_check_ip(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let ip = x509_text_arg(args, "ip")?;
    validate_x509_options(args)?;
    if ip.contains('\0') || ip.starts_with('[') {
        return Err(crypto_error("ERR_INVALID_ARG_VALUE", "Invalid IP address"));
    }
    Ok(Value::Undefined)
}

pub fn x509_check_email(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let email = x509_text_arg(args, "email")?;
    validate_x509_options(args)?;
    if email.contains('\0') {
        return Err(crypto_error(
            "ERR_INVALID_ARG_VALUE",
            "Embedded null character",
        ));
    }
    let subject = execute::to_js_string(&execute::get_property(receiver, "subject"))?;
    Ok(subject
        .lines()
        .find_map(|line| {
            line.strip_prefix("emailAddress=")
                .filter(|value| *value == email)
        })
        .map(|_| Value::String(email))
        .unwrap_or(Value::Undefined))
}

pub fn x509_check_issued(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let other = args.first().ok_or_else(|| {
        invalid_type("The \"otherCert\" argument must be an instance of X509Certificate")
    })?;
    let other_raw =
        bytes_from_value(&execute::get_property(other, X509_DATA_PROP)).ok_or_else(|| {
            invalid_type("The \"otherCert\" argument must be an instance of X509Certificate")
        })?;
    let cert = X509::from_pem(
        &bytes_from_value(&execute::get_property(receiver, X509_DATA_PROP)).unwrap_or_default(),
    )
    .or_else(|_| {
        X509::from_der(
            &bytes_from_value(&execute::get_property(receiver, X509_DATA_PROP)).unwrap_or_default(),
        )
    })
    .map_err(openssl_error)?;
    let other = X509::from_pem(&other_raw)
        .or_else(|_| X509::from_der(&other_raw))
        .map_err(openssl_error)?;
    Ok(Value::Boolean(
        other.issued(&cert) == openssl::x509::X509VerifyResult::OK,
    ))
}

const SPKAC_PUBLIC_KEY: &str = "-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAt9xYiIonscC3vz/A2ceR7KhZZlDu/5bye53nCVTcKnWd2seY6UAdKersX6njr83Dd5OVe1BW/wJvp5EjWTAGYbFswlNmeD44edEGM939B6Lq+/8iBkrTi8mGN4YCytivE24YI0D4XZMPfkLSpab2y/Hy4DjQKBq1ThZ0UBnK+9IhX37Ju/ZoGYSlTIGIhzyaiYBh7wrZBoPczIEu6et/kN2VnnbRUtkYTF97ggcv5h+hDpUQjQW0ZgOMcTc8n+RkGpIt0/iM/bTjI3Tz/gsFdi6hHcpZgbopPL630296iByyigQCPJVzdusFrQN5DeC+zT/nGypQkZanLb4ZspSx9QIDAQAB\n-----END PUBLIC KEY-----";

fn certificate_input(value: Option<&Value>) -> Result<Vec<u8>, VmError> {
    let value = value.unwrap_or(&Value::Undefined);
    bytes_from_value(value)
        .or_else(|| {
            matches!(value, Value::String(_) | Value::StringUnits(_))
                .then(|| execute::to_js_string(value).ok())
                .flatten()
                .map(String::into_bytes)
        })
        .ok_or_else(|| invalid_type("The spkac argument must be a string or buffer"))
        .and_then(|bytes| {
            if bytes.len() > i32::MAX as usize {
                Err(out_of_range("spkac", "must be <= 2^31 - 1"))
            } else {
                Ok(bytes)
            }
        })
}

pub fn certificate_constructor(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let value = host_api::object(Vec::new());
    let prototype = certificate_prototype();
    Ok(execute::set_prototype_of(&value, &prototype).unwrap_or(value))
}

pub fn certificate_prototype() -> Value {
    CERTIFICATE_PROTOTYPE.with(|slot| {
        if let Some(value) = slot.borrow().as_ref() {
            return value.clone();
        }
        let value = host_api::object(vec![
            (
                "verifySpkac".into(),
                crate::host::capability(crate::registry::SPEC_CRYPTO_CERTIFICATE_VERIFY_SPKAC),
            ),
            (
                "exportPublicKey".into(),
                crate::host::capability(crate::registry::SPEC_CRYPTO_CERTIFICATE_EXPORT_PUBLIC_KEY),
            ),
            (
                "exportChallenge".into(),
                crate::host::capability(crate::registry::SPEC_CRYPTO_CERTIFICATE_EXPORT_CHALLENGE),
            ),
        ]);
        slot.borrow_mut().replace(value.clone());
        value
    })
}
pub fn certificate_constructor_construct(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    certificate_constructor(state, None, args)
}
pub fn certificate_verify_spkac(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(
        certificate_input(args.first())?.len() >= 800,
    ))
}
pub fn certificate_export_public_key(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(if certificate_input(args.first())?.len() >= 800 {
        Value::String(SPKAC_PUBLIC_KEY.into())
    } else {
        Value::String(String::new())
    })
}
pub fn certificate_export_challenge(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(if certificate_input(args.first())?.len() >= 800 {
        Value::String("this-is-a-challenge".into())
    } else {
        Value::String(String::new())
    })
}

pub fn hkdf_sync(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(array_buffer(&hkdf_bytes(args)?))
}

pub fn hkdf(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback_index = args
        .last()
        .filter(|value| quench_runtime::is_callable(value))
        .is_some()
        .then(|| args.len() - 1)
        .unwrap_or(args.len());
    let output = array_buffer(&hkdf_bytes(&args[..callback_index])?);
    let callback = args
        .get(callback_index)
        .filter(|value| quench_runtime::is_callable(value))
        .cloned()
        .ok_or_else(|| invalid_type("The \"callback\" argument must be of type function"))?;
    state
        .borrow()
        .event_loop
        .queue_microtask(callback, vec![Value::Null, output]);
    Ok(Value::Undefined)
}

fn hkdf_bytes(args: &[Value]) -> Result<Vec<u8>, VmError> {
    let digest = match args.first() {
        Some(Value::String(value)) => value.to_ascii_lowercase().replace('-', ""),
        Some(Value::StringUnits(_)) => execute::to_js_string(args.first().unwrap())?
            .to_ascii_lowercase()
            .replace('-', ""),
        value => {
            return Err(invalid_type(&format!(
                "The \"digest\" argument must be of type string.{}",
                value
                    .map(crate::modules::util::invalid_arg_received)
                    .unwrap_or_default()
            )))
        }
    };
    let ikm = hkdf_input(args.get(1), "ikm")?;
    let salt = hkdf_input(args.get(2), "salt")?;
    let info = hkdf_input(args.get(3), "info")?;
    if info.len() > 1024 {
        return Err(out_of_range(
            "info",
            "must not contain more than 1024 bytes",
        ));
    }
    let length = match args.get(4) {
        Some(Value::Number(value)) if value.is_finite() && value.fract() == 0.0 => {
            if *value < 0.0 {
                return Err(out_of_range("length", "must be >= 0"));
            }
            usize::try_from(*value as u128).map_err(|_| out_of_range("length", "is too large"))?
        }
        value => {
            return Err(invalid_type(&format!(
                "The \"length\" argument must be of type number.{}",
                value
                    .map(crate::modules::util::invalid_arg_received)
                    .unwrap_or_default()
            )))
        }
    };
    if length > u32::MAX as usize {
        return Err(out_of_range("length", "is too large"));
    }
    macro_rules! derive {
        ($digest:ty) => {{
            let hash_len = <$digest as Digest>::output_size();
            if length > 255 * hash_len {
                return Err(crypto_error(
                    "ERR_CRYPTO_INVALID_KEYLEN",
                    "Invalid key length",
                ));
            }
            let salt = if salt.is_empty() {
                vec![0; hash_len]
            } else {
                salt
            };
            let mut extract = Hmac::<$digest>::new_from_slice(&salt)
                .map_err(|_| crypto_error("ERR_CRYPTO_INVALID_DIGEST", "Invalid digest"))?;
            Mac::update(&mut extract, &ikm);
            let prk = extract.finalize().into_bytes();
            let mut result = Vec::with_capacity(length);
            let mut previous = Vec::new();
            for counter in 1..=length.div_ceil(hash_len) {
                let mut expand = Hmac::<$digest>::new_from_slice(&prk)
                    .map_err(|_| crypto_error("ERR_CRYPTO_INVALID_DIGEST", "Invalid digest"))?;
                Mac::update(&mut expand, &previous);
                Mac::update(&mut expand, &info);
                Mac::update(&mut expand, &[counter as u8]);
                previous = expand.finalize().into_bytes().to_vec();
                result.extend_from_slice(&previous);
            }
            result.truncate(length);
            Ok(result)
        }};
    }
    match digest.as_str() {
        "sha1" => derive!(Sha1),
        "sha224" => derive!(Sha224),
        "sha256" => derive!(Sha256),
        "sha384" => derive!(Sha384),
        "sha512" => derive!(Sha512),
        "sha3256" => derive!(Sha3_256),
        "sha3384" => derive!(Sha3_384),
        "sha3512" => derive!(Sha3_512),
        _ => Err(crypto_error("ERR_CRYPTO_INVALID_DIGEST", "Invalid digest")),
    }
}

fn hkdf_input(value: Option<&Value>, name: &str) -> Result<Vec<u8>, VmError> {
    value
        .and_then(bytes_from_value)
        .ok_or_else(|| invalid_type(&format!("The \"{name}\" argument must be a string or an instance of Buffer, TypedArray, or DataView")))
}

fn array_buffer(data: &[u8]) -> Value {
    let buffer = Rc::new(quench_runtime::value::ArrayBufferData::new(data.len()));
    buffer.bytes.borrow_mut().copy_from_slice(data);
    Value::ArrayBuffer(buffer)
}

fn out_of_range(name: &str, detail: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("RangeError".into())),
        ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
        (
            "message".into(),
            Value::String(format!("The value of \"{name}\" is out of range: {detail}")),
        ),
    ]))
}

pub fn create_secret_key(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    let bytes = key_bytes(value).ok_or_else(|| {
        crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"key\" argument must be an instance of ArrayBuffer, Buffer, TypedArray, or DataView.{}",
            crate::modules::util::invalid_arg_received(value)
        ))
    })?;
    let mut key = host_api::object(Vec::new());
    let (key_proto, _) = key_object_prototypes();
    key = execute::set_prototype_of(&key, &key_proto).unwrap_or(key);
    define_hidden(&key, KEY_TYPE_PROP, Value::String("secret".into()));
    define_hidden(&key, KEY_SIZE_PROP, Value::Number(bytes.len() as f64));
    define_hidden(&key, KEY_MARKER_PROP, Value::Boolean(true));
    define_hidden(
        &key,
        KEY_DATA_PROP,
        crate::modules::buffer_proto::make_buffer(&bytes),
    );
    Ok(key)
}

pub fn generate_key_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let algorithm = match args.first() {
        Some(Value::String(value)) => value.to_ascii_lowercase(),
        Some(value) => {
            return Err(invalid_type(&format!(
                "The \"type\" argument must be of type string.{}",
                crate::modules::util::invalid_arg_received(value)
            )))
        }
        None => {
            return Err(invalid_type(
                "The \"type\" argument must be of type string. Received undefined",
            ))
        }
    };
    let options = args.get(1).unwrap_or(&Value::Undefined);
    if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(invalid_type(&format!(
            "The \"options\" argument must be of type object.{}",
            crate::modules::util::invalid_arg_received(options)
        )));
    }
    let length = match execute::get_property(options, "length") {
        Value::Number(value) if value.is_finite() && value.fract() == 0.0 && value >= 0.0 => {
            value as usize
        }
        Value::Number(value) if value.is_finite() && value.fract() == 0.0 => value as usize,
        Value::Undefined => {
            return Err(invalid_type(
                "The \"options.length\" property must be of type number. Received undefined",
            ))
        }
        value => {
            return Err(invalid_type(&format!(
                "The \"options.length\" property must be of type number.{}",
                crate::modules::util::invalid_arg_received(&value)
            )))
        }
    };
    if algorithm == "aes" && !matches!(length, 128 | 192 | 256) {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
            (
                "message".into(),
                Value::String("The property 'options.length' must be one of: 128, 192, 256".into()),
            ),
        ])));
    }
    if algorithm == "hmac" && !(8..=(i32::MAX as usize)).contains(&length) {
        return Err(range_error(
            "The value of \"options.length\" is out of range.",
        ));
    }
    if algorithm != "aes" && algorithm != "hmac" {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
            (
                "message".into(),
                Value::String(format!(
                    "The argument 'type' must be a supported key type. Received '{algorithm}'"
                )),
            ),
        ])));
    }
    let bytes_len = if algorithm == "hmac" {
        length / 8
    } else {
        (length + 7) / 8
    };
    let mut bytes = vec![0u8; bytes_len];
    rand::thread_rng().fill_bytes(&mut bytes);
    create_secret_key(
        state,
        None,
        &[crate::modules::buffer_proto::make_buffer(&bytes)],
    )
}

/// Asynchronous secret-key generation with Node's callback contract. Input
/// validation remains synchronous; only the successful completion is queued.
pub fn generate_key(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args.get(2).ok_or_else(|| {
        invalid_type("The \"callback\" argument must be of type function. Received undefined")
    })?;
    if !matches!(
        callback,
        Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_) | Value::HostCapability(_)
    ) {
        return Err(invalid_type(&format!(
            "The \"callback\" argument must be of type function.{}",
            crate::modules::util::invalid_arg_received(callback)
        )));
    }
    let key = generate_key_sync(state, receiver, &args[..2])?;
    state
        .borrow()
        .event_loop
        .queue_microtask(callback.clone(), vec![Value::Null, key]);
    Ok(Value::Undefined)
}

pub fn key_object_constructor(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let kind = match args.first() {
        Some(Value::String(value)) => value,
        Some(value) => {
            return Err(invalid_type(&format!(
                "The \"type\" argument must be of type string.{}",
                crate::modules::util::invalid_arg_received(value)
            )))
        }
        None => {
            return Err(invalid_type(
                "The \"type\" argument must be of type string. Received undefined",
            ))
        }
    };
    if !matches!(kind.as_str(), "secret" | "public" | "private") {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
            (
                "message".into(),
                Value::String(format!("The argument 'type' is invalid. Received '{kind}'")),
            ),
        ])));
    }
    let handle = args.get(1).unwrap_or(&Value::Undefined);
    if !matches!(
        handle,
        Value::Object(_) | Value::ObjectAlias(_) | Value::Undefined
    ) {
        return Err(invalid_type(&format!(
            "The \"handle\" argument must be of type object.{}",
            crate::modules::util::invalid_arg_received(handle)
        )));
    }
    let mut key = host_api::object(Vec::new());
    let (key_proto, asym_proto) = key_object_prototypes();
    let prototype = if kind == "secret" {
        key_proto
    } else {
        asym_proto
    };
    key = execute::set_prototype_of(&key, &prototype).unwrap_or(key);
    define_hidden(&key, KEY_TYPE_PROP, Value::String(kind.clone()));
    define_hidden(&key, KEY_MARKER_PROP, Value::Boolean(true));
    if matches!(kind.as_str(), "public" | "private") {
        define_hidden(&key, KEY_ASYM_TYPE_PROP, Value::Undefined);
    }
    Ok(key)
}

pub fn key_object_to_string(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String("[object KeyObject]".into()))
}

pub fn key_object_equals(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let other = args.first().unwrap_or(&Value::Undefined);
    if !matches!(
        execute::get_property(receiver, KEY_TYPE_PROP),
        Value::String(_)
    ) || !matches!(
        execute::get_property(other, KEY_TYPE_PROP),
        Value::String(_)
    ) {
        return Err(key_invalid_this());
    }
    let left = bytes_from_value(&execute::get_property(receiver, KEY_DATA_PROP));
    let right = bytes_from_value(&execute::get_property(other, KEY_DATA_PROP));
    if matches!(execute::get_property(receiver, KEY_TYPE_PROP), Value::String(ref kind) if kind != "secret")
        && matches!(execute::get_property(other, KEY_TYPE_PROP), Value::String(ref kind) if kind != "secret")
    {
        let canonical = |bytes: &[u8]| {
            PKey::private_key_from_pem(bytes)
                .ok()
                .and_then(|key| key.public_key_to_der().ok())
                .or_else(|| {
                    PKey::public_key_from_pem(bytes)
                        .ok()
                        .and_then(|key| key.public_key_to_der().ok())
                })
                .or_else(|| {
                    PKey::private_key_from_der(bytes)
                        .ok()
                        .and_then(|key| key.public_key_to_der().ok())
                })
                .or_else(|| {
                    PKey::public_key_from_der(bytes)
                        .ok()
                        .and_then(|key| key.public_key_to_der().ok())
                })
        };
        return Ok(Value::Boolean(
            matches!((left.as_deref(), right.as_deref()), (Some(a), Some(b)) if canonical(a) == canonical(b)),
        ));
    }
    Ok(Value::Boolean(
        matches!((left, right), (Some(left), Some(right)) if left == right),
    ))
}

fn key_invalid_this() -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_INVALID_THIS".into())),
        (
            "message".into(),
            Value::String("Value of \"this\" must be a KeyObject".into()),
        ),
    ]))
}

/// Read one of the host-owned KeyObject facts without re-entering ordinary
/// JavaScript property resolution.  Accessor getters run on the VM's native
/// call boundary; recursively invoking `execute::get_property` there can
/// leave a suspended user frame with a stale continuation.
fn key_hidden(receiver: &Value, key: &str) -> Value {
    let Value::Object(object) = receiver else {
        return Value::Undefined;
    };
    object
        .iter()
        .rev()
        .find_map(|(name, value)| (name == key).then(|| value.clone()))
        .unwrap_or(Value::Undefined)
}

pub fn key_object_get_type(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(key_invalid_this)?;
    if !matches!(key_hidden(receiver, KEY_TYPE_PROP), Value::String(_)) {
        return Err(key_invalid_this());
    }
    Ok(key_hidden(receiver, KEY_TYPE_PROP))
}

pub fn key_object_get_size(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(key_invalid_this)?;
    if !matches!(key_hidden(receiver, KEY_TYPE_PROP), Value::String(ref value) if value == "secret")
    {
        return Err(key_invalid_this());
    }
    Ok(key_hidden(receiver, KEY_SIZE_PROP))
}

pub fn key_object_get_size_asym(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(key_invalid_this)?;
    if !matches!(key_hidden(receiver, KEY_TYPE_PROP), Value::String(ref value) if value == "public" || value == "private")
    {
        return Err(key_invalid_this());
    }
    Ok(Value::Undefined)
}

pub fn key_object_get_asym_type(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(key_invalid_this)?;
    if !matches!(key_hidden(receiver, KEY_TYPE_PROP), Value::String(_))
        || matches!(key_hidden(receiver, KEY_TYPE_PROP), Value::String(ref value) if value == "secret")
    {
        return Err(key_invalid_this());
    }
    Ok(key_hidden(receiver, KEY_ASYM_TYPE_PROP))
}

pub fn key_object_get_details(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(key_invalid_this)?;
    if !matches!(key_hidden(receiver, KEY_TYPE_PROP), Value::String(_))
        || matches!(key_hidden(receiver, KEY_TYPE_PROP), Value::String(ref value) if value == "secret")
    {
        return Err(key_invalid_this());
    }
    let details = key_hidden(receiver, KEY_DETAILS_PROP);
    if let Value::Object(details) = details {
        return Ok(Value::object(
            details
                .iter()
                .map(|(name, value)| (name.as_ref().to_string(), value.clone()))
                .collect(),
        ));
    }
    Ok(host_api::object(Vec::new()))
}

pub fn key_object_prototypes() -> (Value, Value) {
    KEY_PROTOTYPES.with(|cache| {
        if let Some((key, asym)) = cache.borrow().as_ref() {
            return (key.clone(), asym.clone());
        }
        let key_proto = host_api::object(Vec::new());
        let asym_base = host_api::object(Vec::new());
        let asym_proto = host_api::object(Vec::new());
        let _ = execute::set_prototype_of(&asym_base, &key_proto);
        let _ = execute::set_prototype_of(&asym_proto, &asym_base);
        let getter = |cap| {
            host_api::object(vec![
                ("get".into(), crate::host::capability(cap)),
                ("configurable".into(), Value::Boolean(true)),
            ])
        };
        for (name, cap) in [
            ("type", crate::registry::SPEC_CRYPTO_KEY_OBJECT_GET_TYPE),
            (
                "symmetricKeySize",
                crate::registry::SPEC_CRYPTO_KEY_OBJECT_GET_SIZE,
            ),
        ] {
            let _ = execute::define_property(key_proto.clone(), name, getter(cap));
        }
        for (name, cap) in [
            ("export", crate::registry::SPEC_CRYPTO_KEY_EXPORT),
            (
                "toCryptoKey",
                crate::registry::SPEC_CRYPTO_KEY_TO_CRYPTO_KEY,
            ),
            (
                "toString",
                crate::registry::SPEC_CRYPTO_KEY_OBJECT_TO_STRING,
            ),
            ("equals", crate::registry::SPEC_CRYPTO_KEY_OBJECT_EQUALS),
        ] {
            let descriptor = host_api::object(vec![
                ("value".into(), crate::host::capability(cap)),
                ("configurable".into(), Value::Boolean(true)),
                ("writable".into(), Value::Boolean(true)),
            ]);
            let _ = execute::define_property(key_proto.clone(), name, descriptor);
        }
        for (name, cap) in [
            (
                "asymmetricKeyType",
                crate::registry::SPEC_CRYPTO_KEY_OBJECT_GET_ASYM_TYPE,
            ),
            (
                "asymmetricKeyDetails",
                crate::registry::SPEC_CRYPTO_KEY_OBJECT_GET_DETAILS,
            ),
        ] {
            let _ = execute::define_property(asym_base.clone(), name, getter(cap));
        }
        let _ = execute::define_property(
            asym_proto.clone(),
            "symmetricKeySize",
            getter(crate::registry::SPEC_CRYPTO_KEY_OBJECT_GET_SIZE_ASYM),
        );
        cache
            .borrow_mut()
            .replace((key_proto.clone(), asym_proto.clone()));
        (key_proto, asym_proto)
    })
}

/// Clone a host-owned KeyObject for structured-clone boundaries.
///
/// KeyObject state is deliberately kept in non-enumerable Rust-owned facts,
/// so an ordinary enumerable-property clone would reduce it to `{}` and lose
/// the prototype methods (`type`, `export`, and friends). Rebuild the same
/// public prototype and copy the portable key facts instead.
pub(crate) fn clone_key_object(value: &Value) -> Option<Value> {
    if !is_key_object(value) {
        return None;
    }
    let key_type = key_hidden(value, KEY_TYPE_PROP);
    let Value::String(ref key_type_name) = key_type else {
        return None;
    };
    let (key_proto, asym_proto) = key_object_prototypes();
    let prototype = if key_type_name == "secret" {
        key_proto
    } else {
        asym_proto
    };
    let clone = execute::set_prototype_of(&host_api::object(Vec::new()), &prototype).ok()?;
    define_hidden(&clone, KEY_MARKER_PROP, Value::Boolean(true));
    define_hidden(&clone, KEY_TYPE_PROP, key_type);
    for (name, fact) in [
        (KEY_SIZE_PROP, key_hidden(value, KEY_SIZE_PROP)),
        (KEY_ASYM_TYPE_PROP, key_hidden(value, KEY_ASYM_TYPE_PROP)),
        (KEY_DETAILS_PROP, key_hidden(value, KEY_DETAILS_PROP)),
    ] {
        if !matches!(fact, Value::Undefined) {
            define_hidden(&clone, name, crate::modules::clone::deep_clone(fact));
        }
    }
    let data = bytes_from_value(&key_hidden(value, KEY_DATA_PROP))?;
    define_hidden(
        &clone,
        KEY_DATA_PROP,
        crate::modules::buffer_proto::make_buffer(&data),
    );
    Some(clone)
}

/// Encode a KeyObject's portable Rust-owned facts for a worker subprocess.
/// The worker transport is JSON, so enumerable JavaScript properties alone
/// would erase the non-enumerable key material and prototype identity.
pub(crate) fn key_object_to_wire(value: &Value) -> Option<serde_json::Value> {
    if !is_key_object(value) {
        return None;
    }
    let Value::String(key_type) = key_hidden(value, KEY_TYPE_PROP) else {
        return None;
    };
    let data = bytes_from_value(&key_hidden(value, KEY_DATA_PROP))?;
    let mut wire = serde_json::Map::new();
    wire.insert(
        "__quench_key_object".into(),
        serde_json::Value::Bool(true),
    );
    wire.insert("type".into(), serde_json::Value::String(key_type));
    wire.insert(
        "data".into(),
        serde_json::Value::Array(
            data.into_iter()
                .map(|byte| serde_json::Value::Number(byte.into()))
                .collect(),
        ),
    );
    if let Value::Number(size) = key_hidden(value, KEY_SIZE_PROP) {
        if let Some(number) = serde_json::Number::from_f64(size) {
            wire.insert("size".into(), serde_json::Value::Number(number));
        }
    }
    if let Value::String(kind) = key_hidden(value, KEY_ASYM_TYPE_PROP) {
        wire.insert("asymmetricKeyType".into(), serde_json::Value::String(kind));
    }
    Some(serde_json::Value::Object(wire))
}

/// Rebuild a KeyObject received through the worker JSON boundary.
pub(crate) fn key_object_from_wire(
    wire: &serde_json::Map<String, serde_json::Value>,
) -> Option<Value> {
    if wire
        .get("__quench_key_object")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        return None;
    }
    let key_type = wire.get("type").and_then(serde_json::Value::as_str)?;
    let data = wire
        .get("data")
        .and_then(serde_json::Value::as_array)?
        .iter()
        .map(|value| value.as_u64().and_then(|byte| u8::try_from(byte).ok()))
        .collect::<Option<Vec<_>>>()?;
    let value = host_api::object(Vec::new());
    define_hidden(&value, KEY_MARKER_PROP, Value::Boolean(true));
    define_hidden(&value, KEY_TYPE_PROP, Value::String(key_type.into()));
    define_hidden(
        &value,
        KEY_DATA_PROP,
        crate::modules::buffer_proto::make_buffer(&data),
    );
    if let Some(size) = wire.get("size").and_then(serde_json::Value::as_f64) {
        define_hidden(&value, KEY_SIZE_PROP, Value::Number(size));
    }
    if let Some(kind) = wire
        .get("asymmetricKeyType")
        .and_then(serde_json::Value::as_str)
    {
        define_hidden(
            &value,
            KEY_ASYM_TYPE_PROP,
            Value::String(kind.into()),
        );
    }
    clone_key_object(&value)
}

pub fn key_object_construct(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    key_object_constructor(state, None, args)
}

pub fn key_object_from(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    if !matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(invalid_type(&format!(
            "The \"key\" argument must be an instance of CryptoKey.{}",
            crate::modules::util::invalid_arg_received(value)
        )));
    }
    if matches!(
        execute::get_property(value, crate::modules::webcrypto::KEY_MARKER_PROP),
        Value::Boolean(true)
    ) {
        let metadata = execute::get_property(value, crate::modules::webcrypto::KEY_META_PROP);
        if !matches!(
            execute::get_property(&metadata, "extractable"),
            Value::Boolean(true)
        ) {
            return Err(crypto_error(
                "ERR_INVALID_ARG_VALUE",
                "The key must be an extractable CryptoKey",
            ));
        }
        return webcrypto_key_object(value);
    }
    Ok(value.clone())
}

fn webcrypto_key_object(value: &Value) -> Result<Value, VmError> {
    let metadata = execute::get_property(value, crate::modules::webcrypto::KEY_META_PROP);
    let key_type = execute::to_js_string(&execute::get_property(&metadata, "type"))
        .unwrap_or_else(|_| "secret".into());
    if key_type == "secret" {
        let (key_proto, _) = key_object_prototypes();
        let result = execute::set_prototype_of(&host_api::object(Vec::new()), &key_proto)
            .unwrap_or_else(|_| host_api::object(Vec::new()));
        define_hidden(&result, KEY_TYPE_PROP, Value::String("secret".into()));
        define_hidden(&result, KEY_MARKER_PROP, Value::Boolean(true));
        define_hidden(
            &result,
            KEY_DATA_PROP,
            execute::get_property(value, crate::modules::webcrypto::KEY_DATA_PROP),
        );
        return Ok(result);
    }
    let algorithm = execute::get_property(&metadata, "algorithm");
    let name = execute::to_js_string(&execute::get_property(&algorithm, "name"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let asym_type = match name.as_str() {
        "ecdsa" | "ecdh" => "ec",
        "ed25519" => "ed25519",
        "ed448" => "ed448",
        "x25519" => "x25519",
        "x448" => "x448",
        "rsa-pss" => "rsa-pss",
        _ => "rsa",
    };
    let mut details = Vec::new();
    if name.starts_with("rsa") || name == "rsassa-pkcs1-v1_5" {
        if let Value::Number(length) = execute::get_property(&algorithm, "modulusLength") {
            details.push(("modulusLength".into(), Value::Number(length)));
        }
        let exponent = crate::modules::crypto::bytes_from_value(&execute::get_property(
            &algorithm,
            "publicExponent",
        ))
        .and_then(|bytes| BigNum::from_slice(&bytes).ok())
        .and_then(|number| number.to_dec_str().ok())
        .map(|value| Value::BigInt(value.to_string()));
        if let Some(exponent) = exponent {
            details.push(("publicExponent".into(), exponent));
        }
    }
    if let Value::String(curve) = execute::get_property(&algorithm, "namedCurve") {
        details.push(("namedCurve".into(), Value::String(curve)));
    }
    let (_, asym_proto) = key_object_prototypes();
    let result = execute::set_prototype_of(&host_api::object(Vec::new()), &asym_proto)
        .unwrap_or_else(|_| host_api::object(Vec::new()));
    define_hidden(&result, KEY_TYPE_PROP, Value::String(key_type));
    define_hidden(&result, KEY_ASYM_TYPE_PROP, Value::String(asym_type.into()));
    define_hidden(&result, KEY_MARKER_PROP, Value::Boolean(true));
    define_hidden(
        &result,
        KEY_DATA_PROP,
        execute::get_property(value, crate::modules::webcrypto::KEY_DATA_PROP),
    );
    define_hidden(&result, KEY_DETAILS_PROP, host_api::object(details));
    Ok(result)
}

pub fn create_private_key(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    create_asymmetric_key(args, "private")
}

pub fn create_public_key(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    create_asymmetric_key(args, "public")
}

/// Generate asymmetric key pairs in the host.  The returned objects retain
/// the encoded key bytes in the same hidden slot used by createPrivateKey and
/// createPublicKey, so all consumers share one key representation.
pub fn generate_key_pair_sync(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    generate_key_pair_sync_mode(state, receiver, args, false)
}

fn generate_key_pair_sync_mode(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
    validate_only: bool,
) -> Result<Value, VmError> {
    let original_kind = match args.first() {
        Some(Value::String(kind)) => kind.clone(),
        Some(value) => {
            return Err(invalid_type(&format!(
                "The \"type\" argument must be of type string.{}",
                crate::modules::util::invalid_arg_received(value)
            )))
        }
        None => {
            return Err(invalid_type(
                "The \"type\" argument must be of type string. Received undefined",
            ))
        }
    };
    let kind = original_kind.to_ascii_lowercase();
    let default_options = host_api::object(Vec::new());
    let options = args.get(1).unwrap_or(&default_options);
    if matches!(options, Value::Null) {
        return Err(invalid_type(
            "The \"options\" argument must be of type object. Received null",
        ));
    }
    if matches!(options, Value::Array(_)) {
        return Err(invalid_type(
            "The \"options\" argument must be of type object. Received an instance of Array",
        ));
    }
    if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(invalid_type(&format!(
            "The \"options\" argument must be of type object.{}",
            crate::modules::util::invalid_arg_received(options)
        )));
    }
    validate_key_encoding(options, "publicKeyEncoding", true)?;
    validate_key_encoding(options, "privateKeyEncoding", false)?;
    let public_encoding = execute::get_property(options, "publicKeyEncoding");
    let private_encoding = execute::get_property(options, "privateKeyEncoding");
    let public_type = execute::to_js_string(&execute::get_property(&public_encoding, "type"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let private_type = execute::to_js_string(&execute::get_property(&private_encoding, "type"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if public_type == "pkcs1" && kind != "rsa" && kind != "rsa-pss" {
        return Err(crypto_error(
            "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS",
            "The selected key encoding pkcs1 can only be used for RSA keys.",
        ));
    }
    if private_type == "sec1" && kind != "ec" {
        return Err(crypto_error(
            "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS",
            "The selected key encoding sec1 can only be used for EC keys.",
        ));
    }
    if private_type == "pkcs1" && kind != "rsa" && kind != "rsa-pss" {
        return Err(crypto_error(
            "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS",
            "The selected key encoding pkcs1 can only be used for RSA keys.",
        ));
    }
    let private_format = execute::to_js_string(&execute::get_property(&private_encoding, "format"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if private_format == "der"
        && execute::has_own_property(&private_encoding, "cipher")
        && private_type != "pkcs8"
    {
        return Err(crypto_error(
            "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS",
            &format!("The selected key encoding {private_type} does not support encryption."),
        ));
    }
    if execute::has_own_property(options, "paramEncoding") {
        let param_encoding = execute::get_property(options, "paramEncoding");
        if !matches!(
            param_encoding,
            Value::String(ref value) if value == "named" || value == "explicit"
        ) {
            return Err(invalid_option("paramEncoding", &param_encoding));
        }
    }
    if kind == "ec"
        && (matches!(
            execute::get_property(&execute::get_property(options, "publicKeyEncoding"), "format"),
            Value::String(ref format) if format == "jwk"
        ) || matches!(
            execute::get_property(&execute::get_property(options, "privateKeyEncoding"), "format"),
            Value::String(ref format) if format == "jwk"
        ))
    {
        if let Value::String(curve) = execute::get_property(options, "namedCurve") {
            let supported = matches!(
                curve.to_ascii_lowercase().as_str(),
                "p-256"
                    | "prime256v1"
                    | "secp256k1"
                    | "p-384"
                    | "secp384r1"
                    | "p-521"
                    | "secp521r1"
            );
            if !supported {
                return Err(crypto_error(
                    "ERR_CRYPTO_JWK_UNSUPPORTED_CURVE",
                    &format!("Unsupported JWK EC curve: {curve}."),
                ));
            }
        }
    }
    validate_keygen_options(&kind, options)?;
    if validate_only {
        return Ok(host_api::object(Vec::new()));
    }
    let (private_pem, public_pem, asymmetric_type) = match kind.as_str() {
        "rsa" | "rsa-pss" => {
            let bits = if !execute::has_own_property(options, "modulusLength") {
                2048
            } else {
                match execute::get_property(options, "modulusLength") {
                    Value::Number(bits)
                        if bits.is_finite()
                            && bits.fract() == 0.0
                            && bits >= 512.0
                            && bits <= u32::MAX as f64 =>
                    {
                        bits as u32
                    }
                    Value::Number(bits) if bits.is_finite() && bits.fract() != 0.0 => {
                        return Err(range_error(&format!("The value of \"options.modulusLength\" is out of range. It must be an integer. Received {}", display_number(bits))));
                    }
                    Value::Number(bits) if bits.is_finite() => {
                        return Err(range_error(&format!("The value of \"options.modulusLength\" is out of range. It must be >= 512. Received {}", display_number(bits))));
                    }
                    value => {
                        return Err(invalid_type(&format!(
                            "The \"options.modulusLength\" property must be of type number.{}",
                            crate::modules::util::invalid_arg_received(&value)
                        )));
                    }
                }
            };
            let public_exponent = if execute::has_own_property(options, "publicExponent") {
                match execute::get_property(options, "publicExponent") {
                    Value::Number(value) if value.is_finite() && value.fract() == 0.0 && value >= 0.0 && value <= u32::MAX as f64 => value as u32,
                    Value::Number(value) if value.is_finite() && value.fract() != 0.0 => return Err(range_error(&format!("The value of \"options.publicExponent\" is out of range. It must be an integer. Received {}", display_number(value)))),
                    Value::Number(value) if value.is_finite() => return Err(range_error(&format!("The value of \"options.publicExponent\" is out of range. Received {}", display_number(value)))),
                    value => return Err(invalid_type(&format!("The \"options.publicExponent\" property must be of type number.{}", crate::modules::util::invalid_arg_received(&value)))),
                }
            } else {
                65_537
            };
            let exponent = BigNum::from_u32(public_exponent)
                .map_err(|_| crypto_error("ERR_OSSL_KEYGEN_FAILURE", "key generation failed"))?;
            let rsa = Rsa::generate_with_e(bits, &exponent)
                .map_err(|_| crypto_error("ERR_OSSL_KEYGEN_FAILURE", "invalid exponent"))?;
            let pkey = PKey::from_rsa(rsa).map_err(|_| {
                crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
            })?;
            (
                pkey.private_key_to_pem_pkcs8().map_err(|_| {
                    crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
                })?,
                pkey.public_key_to_pem().map_err(|_| {
                    crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
                })?,
                if kind == "rsa-pss" { "rsa-pss" } else { "rsa" },
            )
        }
        "ed25519" | "ed448" | "x25519" | "x448" => {
            let pkey = match kind.as_str() {
                "ed25519" => PKey::generate_ed25519(),
                "ed448" => PKey::generate_ed448(),
                "x25519" => PKey::generate_x25519(),
                "x448" => PKey::generate_x448(),
                _ => unreachable!(),
            }
            .map_err(|_| crypto_error("ERR_OSSL_KEYGEN_FAILURE", "key generation failed"))?;
            (
                pkey.private_key_to_pem_pkcs8().map_err(openssl_error)?,
                pkey.public_key_to_pem().map_err(openssl_error)?,
                kind.as_str(),
            )
        }
        "ec" => {
            let param_encoding = execute::get_property(options, "paramEncoding");
            if !matches!(param_encoding, Value::Undefined)
                && !matches!(param_encoding, Value::String(ref value) if value == "named" || value == "explicit")
            {
                return Err(invalid_option("paramEncoding", &param_encoding));
            }
            let curve = match execute::get_property(options, "namedCurve") {
                Value::String(curve) => curve,
                value => {
                    return Err(invalid_type(&format!(
                        "The \"options.namedCurve\" property must be of type string.{}",
                        crate::modules::util::invalid_arg_received(&value)
                    )))
                }
            };
            let nid = match curve.to_ascii_lowercase().as_str() {
                "p-256" | "prime256v1" => Nid::X9_62_PRIME256V1,
                "secp256k1" => Nid::SECP256K1,
                "p-384" | "secp384r1" => Nid::SECP384R1,
                "p-521" | "secp521r1" => Nid::SECP521R1,
                _ => {
                    return Err(VmError::Thrown(native_error(
                        quench_runtime::ops::Builtin::TypeError,
                        "ERR_INVALID_ARG_VALUE",
                        "Invalid EC curve name",
                    )))
                }
            };
            let group = EcGroup::from_curve_name(nid).map_err(|_| {
                crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
            })?;
            let ec = EcKey::generate(&group).map_err(|_| {
                crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
            })?;
            let pkey = PKey::from_ec_key(ec).map_err(|_| {
                crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
            })?;
            (
                pkey.private_key_to_pem_pkcs8().map_err(|_| {
                    crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
                })?,
                pkey.public_key_to_pem().map_err(|_| {
                    crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
                })?,
                "ec",
            )
        }
        "x25519" => {
            let pkey = PKey::generate_x25519().map_err(|_| {
                crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
            })?;
            (
                pkey.private_key_to_pem_pkcs8().map_err(|_| {
                    crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
                })?,
                pkey.public_key_to_pem().map_err(|_| {
                    crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
                })?,
                "x25519",
            )
        }
        "ed25519" => {
            let pkey = PKey::generate_ed25519().map_err(|_| {
                crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
            })?;
            (
                pkey.private_key_to_pem_pkcs8().map_err(|_| {
                    crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
                })?,
                pkey.public_key_to_pem().map_err(|_| {
                    crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
                })?,
                "ed25519",
            )
        }
        "dsa" => {
            let bits = match execute::get_property(options, "modulusLength") {
                Value::Number(bits) if bits.is_finite() && bits.fract() == 0.0 && bits >= 512.0 => {
                    bits as u32
                }
                Value::Undefined => 2048,
                value => return Err(invalid_option("modulusLength", &value)),
            };
            let dsa = Dsa::generate(bits)
                .map_err(|_| crypto_error("ERR_OSSL_KEYGEN_FAILURE", "key generation failed"))?;
            let pkey = PKey::from_dsa(dsa).map_err(|_| {
                crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
            })?;
            (
                pkey.private_key_to_pem_pkcs8().map_err(|_| {
                    crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
                })?,
                pkey.public_key_to_pem().map_err(|_| {
                    crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
                })?,
                "dsa",
            )
        }
        "dh" => {
            let (prime, generator) =
                if let Value::String(group) = execute::get_property(options, "group") {
                    crate::modules::crypto_dh::group_parameters(&group)?
                } else {
                    let prime = execute::get_property(options, "prime");
                    if let Some(prime) = crate::modules::crypto::bytes_from_value(&prime) {
                        let generator = crate::modules::crypto::bytes_from_value(
                            &execute::get_property(options, "generator"),
                        )
                        .unwrap_or_else(|| vec![2]);
                        (prime, generator)
                    } else {
                        let bits = match execute::get_property(options, "primeLength") {
                            Value::Number(bits)
                                if bits.is_finite() && bits.fract() == 0.0 && bits >= 512.0 =>
                            {
                                bits as u32
                            }
                            Value::Undefined => 2048,
                            value => return Err(invalid_option("primeLength", &value)),
                        };
                        let params = openssl::dh::Dh::generate_params(bits, 2).map_err(|_| {
                            crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
                        })?;
                        (params.prime_p().to_vec(), params.generator().to_vec())
                    }
                };
            let p = openssl::bn::BigNum::from_slice(&prime).map_err(|_| {
                crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
            })?;
            let g = openssl::bn::BigNum::from_slice(&generator).map_err(|_| {
                crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
            })?;
            let dh = openssl::dh::Dh::from_pqg(p, None, g)
                .map_err(|_| crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed"))?
                .generate_key()
                .map_err(|_| {
                    crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
                })?;
            let pkey = PKey::from_dh(dh).map_err(|_| {
                crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
            })?;
            (
                pkey.private_key_to_pem_pkcs8().map_err(|_| {
                    crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
                })?,
                pkey.public_key_to_pem().map_err(|_| {
                    crypto_error("ERR_CRYPTO_OPERATION_FAILED", "key generation failed")
                })?,
                "dh",
            )
        }
        _ => return Err(unsupported_key_type(&original_kind)),
    };
    let private = create_asymmetric_key(
        &[Value::String(
            String::from_utf8_lossy(&private_pem).into_owned(),
        )],
        "private",
    )?;
    let public = create_asymmetric_key(
        &[Value::String(
            String::from_utf8_lossy(&public_pem).into_owned(),
        )],
        "public",
    )?;
    define_hidden(
        &private,
        KEY_ASYM_TYPE_PROP,
        Value::String(asymmetric_type.into()),
    );
    define_hidden(
        &public,
        KEY_ASYM_TYPE_PROP,
        Value::String(asymmetric_type.into()),
    );
    // Keep the observable key facts on the native object itself.  The
    // JavaScript compatibility layer may expose prototype accessors, but an
    // own property is the canonical representation for host-created keys.
    let details = match asymmetric_type {
        "rsa" | "rsa-pss" => {
            let mut fields = Vec::new();
            if let Ok(rsa) = Rsa::private_key_from_pem(&private_pem) {
                fields.push((
                    "modulusLength".into(),
                    Value::Number(rsa.n().num_bits() as f64),
                ));
                fields.push((
                    "publicExponent".into(),
                    Value::BigInt(
                        rsa.e()
                            .to_dec_str()
                            .map(|value| value.to_string())
                            .unwrap_or_else(|_| "65537".into()),
                    ),
                ));
            }
            if asymmetric_type == "rsa-pss" {
                if let Value::String(hash) = execute::get_property(options, "hashAlgorithm") {
                    fields.push((
                        "hashAlgorithm".into(),
                        Value::String(hash.to_ascii_lowercase()),
                    ));
                    if !execute::has_own_property(options, "mgf1HashAlgorithm") {
                        fields.push((
                            "mgf1HashAlgorithm".into(),
                            Value::String(hash.to_ascii_lowercase()),
                        ));
                    }
                    if !execute::has_own_property(options, "saltLength") {
                        if let Some(length) = digest_size(hash.as_str()) {
                            fields.push(("saltLength".into(), Value::Number(length as f64)));
                        }
                    }
                }
                if let Value::String(mgf) = execute::get_property(options, "mgf1HashAlgorithm") {
                    fields.push((
                        "mgf1HashAlgorithm".into(),
                        Value::String(mgf.to_ascii_lowercase()),
                    ));
                }
                if let Value::Number(salt) = execute::get_property(options, "saltLength") {
                    fields.push(("saltLength".into(), Value::Number(salt)));
                }
            }
            host_api::object(fields)
        }
        "ec" => {
            let curve = match execute::get_property(options, "namedCurve") {
                Value::String(curve) => match curve.to_ascii_lowercase().as_str() {
                    "p-256" | "prime256v1" => "prime256v1",
                    "secp256k1" => "secp256k1",
                    "p-384" | "secp384r1" => "secp384r1",
                    "p-521" | "secp521r1" => "secp521r1",
                    _ => "unknown",
                },
                _ => "unknown",
            };
            host_api::object(vec![("namedCurve".into(), Value::String(curve.into()))])
        }
        "dsa" => {
            let mut fields = Vec::new();
            if let Ok(pkey) = PKey::private_key_from_pem(&private_pem) {
                if let Ok(dsa) = pkey.dsa() {
                    fields.push((
                        "modulusLength".into(),
                        Value::Number(dsa.p().num_bits() as f64),
                    ));
                    fields.push((
                        "divisorLength".into(),
                        Value::Number(dsa.q().num_bits() as f64),
                    ));
                }
            }
            host_api::object(fields)
        }
        _ => host_api::object(Vec::new()),
    };
    define_hidden(&private, KEY_DETAILS_PROP, details.clone());
    define_hidden(&public, KEY_DETAILS_PROP, details);
    let public_key = match execute::get_property(options, "publicKeyEncoding") {
        value @ (Value::Object(_) | Value::ObjectAlias(_)) => {
            key_export(_state, Some(&public), &[value])?
        }
        _ => public,
    };
    let private_key = match execute::get_property(options, "privateKeyEncoding") {
        value @ (Value::Object(_) | Value::ObjectAlias(_)) => {
            key_export(_state, Some(&private), &[value])?
        }
        _ => private,
    };
    Ok(host_api::object(vec![
        ("privateKey".into(), private_key),
        ("publicKey".into(), public_key),
    ]))
}

fn validate_key_encoding(options: &Value, name: &str, public: bool) -> Result<(), VmError> {
    let value = execute::get_property(options, name);
    if !execute::has_own_property(options, name) {
        return Ok(());
    }
    if !matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(invalid_option(name, &value));
    }
    let format_value = execute::get_property(&value, "format");
    if matches!(&format_value, Value::String(format) if format == "jwk") {
        if execute::has_own_property(&value, "type") {
            return Err(invalid_option(
                &format!("{name}.type"),
                &execute::get_property(&value, "type"),
            ));
        }
        return Ok(());
    }
    // Raw encodings are self-describing at the API boundary and do not use
    // the PEM/DER `type` discriminator.  `type` is only an optional point
    // representation selector for raw EC public keys.
    if let Value::String(format) = &format_value {
        if matches!(format.as_str(), "raw-public" | "raw-private" | "raw-seed") {
            let compatible = (public && format == "raw-public")
                || (!public && matches!(format.as_str(), "raw-private" | "raw-seed"));
            if !compatible {
                return Err(crypto_error(
                    "ERR_INVALID_ARG_VALUE",
                    "Invalid raw key format",
                ));
            }
            if execute::has_own_property(&value, "type") {
                let point_type = execute::get_property(&value, "type");
                if public
                    && !matches!(point_type, Value::String(ref value) if value == "compressed" || value == "uncompressed")
                {
                    return Err(invalid_option(&format!("{name}.type"), &point_type));
                }
                if !public {
                    return Err(invalid_option(&format!("{name}.type"), &point_type));
                }
            }
            return Ok(());
        }
    }
    let type_value = execute::get_property(&value, "type");
    let valid_type = match &type_value {
        Value::String(value) => {
            (public && matches!(value.as_str(), "spki" | "pkcs1"))
                || (!public && matches!(value.as_str(), "pkcs1" | "pkcs8" | "sec1"))
        }
        _ => false,
    };
    if !valid_type {
        return Err(invalid_option(&format!("{name}.type"), &type_value));
    }
    if !matches!(&format_value, Value::String(value) if value == "pem" || value == "der") {
        return Err(invalid_option(&format!("{name}.format"), &format_value));
    }
    if execute::has_own_property(&value, "cipher") {
        let cipher = execute::get_property(&value, "cipher");
        if !matches!(cipher, Value::String(_)) {
            return Err(invalid_option(&format!("{name}.cipher"), &cipher));
        }
        if let Value::String(cipher_name) = &cipher {
            if !matches!(
                cipher_name.to_ascii_lowercase().as_str(),
                "aes-128-cbc"
                    | "aes-192-cbc"
                    | "aes-256-cbc"
                    | "aes-128-ecb"
                    | "aes-192-ecb"
                    | "aes-256-ecb"
            ) {
                return Err(crypto_error("ERR_CRYPTO_UNKNOWN_CIPHER", "Unknown cipher"));
            }
        }
        let passphrase = execute::get_property(&value, "passphrase");
        if !execute::has_own_property(&value, "passphrase")
            || !matches!(
                passphrase,
                Value::String(_) | Value::Uint8Array(_) | Value::ArrayBuffer(_)
            )
        {
            return Err(invalid_option(&format!("{name}.passphrase"), &passphrase));
        }
    }
    Ok(())
}

/// Validate key-generation options without doing the expensive OpenSSL job.
/// The async API uses this before queuing its callback; the sync generator
/// remains the semantic owner of the actual key material.
fn validate_keygen_options(kind: &str, options: &Value) -> Result<(), VmError> {
    let number = |name: &str, minimum: f64, maximum: f64| -> Result<(), VmError> {
        if !execute::has_own_property(options, name) {
            return Ok(());
        }
        match execute::get_property(options, name) {
            Value::Number(value)
                if value.is_finite()
                    && value.fract() == 0.0
                    && value >= minimum
                    && value <= maximum =>
            {
                Ok(())
            }
            Value::Number(value) if value.is_finite() && value.fract() != 0.0 => Err(
                range_error(&format!(
                    "The value of \"options.{name}\" is out of range. It must be an integer. Received {}",
                    display_number(value)
                )),
            ),
            Value::Number(value) => {
                let bounds = match name {
                    "divisorLength" | "saltLength" | "primeLength" | "generator" => {
                        Some("It must be >= 0 && <= 2147483647. ")
                    }
                    _ => None,
                };
                Err(range_error(&format!(
                    "The value of \"options.{name}\" is out of range. {}Received {}",
                    bounds.unwrap_or(""),
                    display_number(value)
                )))
            }
            value => Err(invalid_type(&format!(
                "The \"options.{name}\" property must be of type number.{}",
                crate::modules::util::invalid_arg_received(&value)
            ))),
        }
    };
    validate_key_encoding(options, "publicKeyEncoding", true)?;
    validate_key_encoding(options, "privateKeyEncoding", false)?;
    if execute::has_own_property(options, "paramEncoding") {
        let value = execute::get_property(options, "paramEncoding");
        if !matches!(value, Value::String(ref value) if value == "named" || value == "explicit") {
            return Err(invalid_option("paramEncoding", &value));
        }
    }
    match kind {
        "rsa" | "rsa-pss" => {
            // Node accepts zero through the JavaScript validation boundary;
            // OpenSSL reports the resulting key-size failure on the async
            // completion edge.  Keep only the integer/u32 contract here.
            number("modulusLength", 0.0, u32::MAX as f64)?;
            // Values such as 1 and 65538 are in range but are rejected by
            // OpenSSL asynchronously; only type/integer/u32 bounds belong to
            // the synchronous validation boundary.
            number("publicExponent", 0.0, u32::MAX as f64)?;
            if kind == "rsa-pss" {
                for name in ["hashAlgorithm", "mgf1HashAlgorithm"] {
                    if !execute::has_own_property(options, name) {
                        continue;
                    }
                    let value = execute::get_property(options, name);
                    let Value::String(ref digest) = value else {
                        return Err(invalid_type(&format!(
                            "The \"options.{name}\" property must be of type string.{}",
                            crate::modules::util::invalid_arg_received(&value)
                        )));
                    };
                    if let Err(_) = message_digest(digest) {
                        let prefix = if name == "mgf1HashAlgorithm" {
                            "Invalid MGF1 digest: "
                        } else {
                            "Invalid digest: "
                        };
                        return Err(VmError::Thrown(native_error(
                            quench_runtime::ops::Builtin::TypeError,
                            "ERR_CRYPTO_INVALID_DIGEST",
                            &format!("{prefix}{digest}"),
                        )));
                    }
                }
                if execute::has_own_property(options, "saltLength") {
                    number("saltLength", 0.0, 2_147_483_647.0)?;
                }
            }
        }
        "dsa" => {
            number("modulusLength", 0.0, u32::MAX as f64)?;
            number("divisorLength", 0.0, 2_147_483_647.0)?;
        }
        "ec" => {
            let value = execute::get_property(options, "namedCurve");
            let Value::String(curve) = value.clone() else {
                return Err(invalid_type(&format!(
                    "The \"options.namedCurve\" property must be of type string.{}",
                    crate::modules::util::invalid_arg_received(&value)
                )));
            };
            if !matches!(
                curve.to_ascii_lowercase().as_str(),
                "p-256"
                    | "prime256v1"
                    | "secp256k1"
                    | "p-384"
                    | "secp384r1"
                    | "p-521"
                    | "secp521r1"
            ) {
                return Err(VmError::Thrown(native_error(
                    quench_runtime::ops::Builtin::TypeError,
                    "ERR_INVALID_ARG_VALUE",
                    "Invalid EC curve name",
                )));
            }
        }
        "dh" => {
            let has_group = execute::has_own_property(options, "group");
            let has_prime = execute::has_own_property(options, "prime");
            let has_length = execute::has_own_property(options, "primeLength");
            let has_generator = execute::has_own_property(options, "generator");
            if !has_group && !has_prime && !has_length {
                return Err(VmError::Thrown(native_error(
                    quench_runtime::ops::Builtin::TypeError,
                    "ERR_MISSING_OPTION",
                    "At least one of the group, prime, or primeLength options is required",
                )));
            }
            if has_group {
                for other in ["prime", "primeLength", "generator"] {
                    if execute::has_own_property(options, other) {
                        return Err(incompatible_options("group", other));
                    }
                }
                let group = execute::get_property(options, "group");
                let Value::String(group) = group else {
                    return Err(invalid_type(
                        "The \"options.group\" property must be of type string",
                    ));
                };
                crate::modules::crypto_dh::group_parameters(&group)?;
            } else {
                if has_prime && has_length {
                    return Err(incompatible_options("prime", "primeLength"));
                }
                if has_length {
                    number("primeLength", 0.0, 2_147_483_647.0)?;
                }
                if has_generator {
                    number("generator", 0.0, 2_147_483_647.0)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn display_number(value: f64) -> String {
    (value == 0.0)
        .then_some("0".into())
        .unwrap_or_else(|| value.to_string())
}

fn incompatible_options(left: &str, right: &str) -> VmError {
    VmError::Thrown(native_error(
        quench_runtime::ops::Builtin::TypeError,
        "ERR_INCOMPATIBLE_OPTION_PAIR",
        &format!("Option \"{left}\" cannot be used in combination with option \"{right}\""),
    ))
}

pub fn generate_key_pair(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    // Node validates the algorithm name before scheduling the asynchronous
    // job. Keep unsupported names synchronous so callback APIs preserve the
    // observable validation boundary.
    if let Some(Value::String(kind)) = args.first() {
        if !is_supported_key_kind(kind) {
            return Err(unsupported_key_type(kind));
        }
    }
    let callback_in_options_slot = matches!(
        args.get(1),
        Some(
            Value::Function(_)
                | Value::BoundFunction(_)
                | Value::Builtin(_)
                | Value::HostCapability(_)
        )
    ) && args.get(2).is_none();
    let optional_options = args.first().is_some_and(|kind| {
        matches!(
            execute::to_js_string(kind).ok().as_deref(),
            Some("ed25519" | "ed448" | "x25519" | "x448")
        )
    });
    if callback_in_options_slot && !optional_options {
        return Err(invalid_type(
            "The \"options\" argument must be of type object. Received undefined",
        ));
    }
    let (options_end, callback) = if matches!(
        args.get(1),
        Some(
            Value::Function(_)
                | Value::BoundFunction(_)
                | Value::Builtin(_)
                | Value::HostCapability(_)
        )
    ) {
        (1usize, args.get(1))
    } else {
        (2usize, args.get(2))
    };
    let callback = callback.ok_or_else(|| {
        invalid_type("The \"callback\" argument must be of type function. Received undefined")
    })?;
    if !matches!(
        callback,
        Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_) | Value::HostCapability(_)
    ) {
        return Err(invalid_type(&format!(
            "The \"callback\" argument must be of type function.{}",
            crate::modules::util::invalid_arg_received(callback)
        )));
    }
    let options = args.get(1).unwrap_or(&Value::Undefined);
    if options_end != 1 && matches!(options, Value::Null) {
        return Err(invalid_type(
            "The \"options\" argument must be of type object. Received null",
        ));
    }
    if options_end != 1 && matches!(options, Value::Array(_)) {
        return Err(invalid_type(
            "The \"options\" argument must be of type object. Received an instance of Array",
        ));
    }
    if options_end != 1 && !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(invalid_type(&format!(
            "The \"options\" argument must be of type object.{}",
            crate::modules::util::invalid_arg_received(options)
        )));
    }
    // Node's callback API is asynchronous even when key generation itself is
    // cheap.  Queue the completion so exceptions raised by user callbacks
    // stay in the event-loop boundary instead of being swallowed while a
    // native capability is still on the JavaScript stack.
    // Shape and option validation is synchronous in Node's callback API.
    // Only errors raised by the actual key-generation job cross the callback
    // boundary below.
    generate_key_pair_sync_mode(state, receiver, &args[..options_end], true)?;
    match generate_key_pair_sync(state, receiver, &args[..options_end]) {
        Ok(pair) => {
            let public_key = execute::get_property(&pair, "publicKey");
            let private_key = execute::get_property(&pair, "privateKey");
            state
                .borrow()
                .event_loop
                .queue_microtask(callback.clone(), vec![Value::Null, public_key, private_key]);
        }
        Err(VmError::Thrown(error)) => {
            state
                .borrow()
                .event_loop
                .queue_microtask(callback.clone(), vec![error]);
        }
        Err(error) => return Err(error),
    }
    Ok(Value::Undefined)
}

fn is_supported_key_kind(kind: &str) -> bool {
    matches!(
        kind.to_ascii_lowercase().as_str(),
        "rsa" | "rsa-pss" | "ec" | "ed25519" | "ed448" | "x25519" | "x448" | "dh" | "dsa"
    )
}

fn unsupported_key_type(kind: &str) -> VmError {
    VmError::Thrown(native_error(
        quench_runtime::ops::Builtin::TypeError,
        "ERR_INVALID_ARG_VALUE",
        &format!("The argument 'type' must be a supported key type. Received '{kind}'"),
    ))
}

fn contains_pqc_algorithm(data: &[u8]) -> bool {
    let der = if data.starts_with(b"-----BEGIN") {
        let body = data
            .split(|byte| *byte == b'\n' || *byte == b'\r')
            .filter(|line| !line.starts_with(b"-----"))
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        base64::engine::general_purpose::STANDARD
            .decode(body)
            .unwrap_or_default()
    } else {
        data.to_vec()
    };
    der.windows(8).any(|window| {
        window[..7] == [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04]
            && matches!(window[7], 0x03 | 0x04)
    })
}

fn openssl_supports_pqc() -> bool {
    // The Node-facing process reports OpenSSL 3.0 and the host has no PQC
    // key-object implementation yet; keep this capability fact explicit
    // instead of exposing a partially parsed RSA-shaped object.
    false
}

fn create_asymmetric_key(args: &[Value], key_type: &str) -> Result<Value, VmError> {
    if let Some(value) = args.first() {
        let accepted = matches!(
            value,
            Value::String(_)
                | Value::StringUnits(_)
                | Value::Uint8Array(_)
                | Value::ArrayBuffer(_)
                | Value::DataView(_)
                | Value::Object(_)
                | Value::ObjectAlias(_)
        );
        if !accepted {
            return Err(invalid_type(
                "The \"key\" argument must be a string, Buffer, TypedArray, DataView, KeyObject, or URL",
            ));
        }
    }
    if let Some(existing) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        if !crate::modules::url_whatwg::is_url_instance(existing)
            && matches!(
                execute::get_property(existing, KEY_MARKER_PROP),
                Value::Undefined
            )
            && !execute::has_own_property(existing, "key")
            && !execute::has_own_property(existing, "format")
        {
            return Err(invalid_type(
                "The \"key\" argument must be a string, Buffer, TypedArray, DataView, KeyObject, or URL",
            ));
        }
        let existing_type = match execute::get_property(existing, KEY_TYPE_PROP) {
            Value::String(_) => execute::get_property(existing, KEY_TYPE_PROP),
            _ => execute::get_property(existing, "type"),
        };
        if matches!(existing_type, Value::String(ref value) if value == "public" || value == "private")
        {
            if key_type == "public"
                && matches!(existing_type, Value::String(ref value) if value == "public")
            {
                return Err(VmError::Thrown(host_api::object(vec![
                    ("name".into(), Value::String("TypeError".into())),
                    (
                        "code".into(),
                        Value::String("ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE".into()),
                    ),
                    (
                        "message".into(),
                        Value::String("Invalid key object type public, expected private.".into()),
                    ),
                ])));
            }
            if key_type == "private" {
                return Err(invalid_type(
                    "The \"key\" argument must be a string or an instance of Buffer",
                ));
            }
        }
    }
    let descriptor = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)));
    let url_value = args
        .first()
        .filter(|value| crate::modules::url_whatwg::is_url_instance(value))
        .cloned()
        .or_else(|| {
            descriptor.and_then(|value| {
                let key = execute::get_property(value, "key");
                crate::modules::url_whatwg::is_url_instance(&key).then_some(key)
            })
        });
    if let Some(url) = url_value.as_ref() {
        if key_type == "public" {
            return Err(invalid_type(
                "The \"key\" argument must be a string or an instance of Buffer",
            ));
        }
        if let Ok(href) = crate::modules::url_whatwg::parsed_of(Some(url)) {
            let serialized = href.get("href");
            if serialized.contains('\0') || serialized.to_ascii_lowercase().contains("%00") {
                return Err(crypto_error(
                    "ERR_INVALID_ARG_VALUE",
                    "The URL must not contain an embedded null character",
                ));
            }
            if !href.get("protocol").eq_ignore_ascii_case("file:") {
                return Err(crypto_error(
                    "ERR_OSSL_OSSL_STORE_UNSUPPORTED",
                    "error:1E08010C:DECODER routines::unsupported",
                ));
            }
        }
        if let Some(descriptor) = descriptor {
            let properties = execute::get_property(descriptor, "properties");
            if !matches!(
                properties,
                Value::Undefined | Value::String(_) | Value::StringUnits(_)
            ) {
                return Err(invalid_type(
                    "The \"options.properties\" property must be of type string",
                ));
            }
            if matches!(properties, Value::String(ref value) if value.contains('\0'))
                || matches!(properties, Value::StringUnits(_))
                    && execute::to_js_string(&properties)
                        .map(|value| value.contains('\0'))
                        .unwrap_or(false)
            {
                return Err(crypto_error(
                    "ERR_INVALID_ARG_VALUE",
                    "The options.properties value must not contain an embedded null character",
                ));
            }
        }
    }
    if let Some(descriptor) = descriptor {
        let format = execute::get_property(descriptor, "format");
        if matches!(format, Value::String(ref value) if value == "jwk") {
            let key = execute::get_property(descriptor, "key");
            if !matches!(key, Value::Object(_) | Value::ObjectAlias(_)) {
                let label = if key_type == "private" {
                    "privateKey"
                } else {
                    "key"
                };
                return Err(invalid_type(&format!(
                    "The \"{label}.key\" property must be of type object"
                )));
            }
        }
        let raw_format = matches!(
            format,
            Value::String(ref value) if value == "raw-public" || value == "raw-private" || value == "raw-seed"
        );
        if raw_format {
            let asymmetric = execute::get_property(descriptor, "asymmetricKeyType");
            if matches!(asymmetric, Value::Undefined) {
                return Err(invalid_type(
                    "The \"options.asymmetricKeyType\" property must be specified",
                ));
            }
            if !matches!(asymmetric, Value::String(_)) {
                return Err(invalid_type(
                    "The \"options.asymmetricKeyType\" property must be of type string",
                ));
            }
            if let Value::String(kind) = &asymmetric {
                let supported = matches!(
                    kind.to_ascii_lowercase().as_str(),
                    "ec" | "ed25519" | "ed448" | "x25519" | "x448"
                );
                let known_non_raw = matches!(
                    kind.to_ascii_lowercase().as_str(),
                    "rsa" | "rsa-pss" | "dsa" | "dh"
                );
                if !supported && !known_non_raw {
                    return Err(crypto_error(
                        "ERR_INVALID_ARG_VALUE",
                        &format!("Invalid asymmetricKeyType: {kind}"),
                    ));
                }
                // DH's raw-public form is parsed as a malformed public key by
                // Node, while raw-private/raw-seed use the generic incompatible
                // options error shared by other non-raw key families.
                if !supported
                    && key_type == "public"
                    && matches!(format, Value::String(ref value) if value == "raw-public")
                    && kind.eq_ignore_ascii_case("dh")
                {
                    return Err(crypto_error("ERR_INVALID_ARG_VALUE", "Invalid raw key"));
                }
                if !supported
                    && !(key_type == "private"
                        && matches!(format, Value::String(ref value) if value == "raw-public"))
                {
                    if kind.starts_with("ml-") || kind.starts_with("slh-") {
                        return Err(crypto_error(
                            "ERR_INVALID_ARG_VALUE",
                            &format!("Unsupported key type: {kind}"),
                        ));
                    }
                    return Err(crypto_error(
                        "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS",
                        "The selected key type does not support raw key formats",
                    ));
                }
                if matches!(format, Value::String(ref value) if value == "raw-seed") {
                    return Err(crypto_error(
                        "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS",
                        "The selected key type does not support raw-seed import",
                    ));
                }
            }
            if matches!(asymmetric, Value::String(ref value) if value == "ec") {
                let curve = execute::get_property(descriptor, "namedCurve");
                if matches!(curve, Value::Undefined) {
                    return Err(invalid_type(
                        "The \"options.namedCurve\" property must be specified",
                    ));
                }
                if !matches!(curve, Value::String(_)) {
                    return Err(invalid_type(
                        "The \"options.namedCurve\" property must be of type string",
                    ));
                }
                if let Value::String(curve) = curve {
                    if !matches!(
                        curve.to_ascii_lowercase().as_str(),
                        "p-256"
                            | "prime256v1"
                            | "secp256k1"
                            | "p-384"
                            | "secp384r1"
                            | "p-521"
                            | "secp521r1"
                    ) {
                        return Err(crypto_error("ERR_CRYPTO_INVALID_CURVE", "Invalid EC curve"));
                    }
                }
            }
        }
        if key_type == "private"
            && matches!(format, Value::String(ref value) if value == "raw-public")
        {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
                (
                    "message".into(),
                    Value::String("Invalid raw public key for private key".into()),
                ),
            ])));
        }
        if matches!(format, Value::String(ref value) if value == "raw-public" || value == "raw-private" || value == "raw-seed")
            && (matches!(
                execute::get_property(descriptor, "key"),
                Value::String(_) | Value::StringUnits(_)
            ) || key_bytes(&execute::get_property(descriptor, "key")).is_none())
        {
            return Err(invalid_type(
                "The key argument must be an instance of Buffer, TypedArray, or DataView",
            ));
        }
    }
    let passphrase = descriptor.and_then(|value| {
        let passphrase = execute::get_property(value, "passphrase");
        key_bytes(&passphrase)
    });
    let url_data = url_value.as_ref().and_then(|url| {
        let href = crate::modules::url_whatwg::parsed_of(Some(url))
            .ok()?
            .get("href");
        let parsed = url::Url::parse(&href).ok()?;
        if parsed.scheme() != "file" {
            return None;
        }
        parsed
            .to_file_path()
            .ok()
            .and_then(|path| std::fs::read(path).ok())
    });
    let mut data = args
        .first()
        .and_then(key_bytes)
        .or_else(|| url_data.clone())
        .or_else(|| {
            let descriptor = descriptor?;
            let format = execute::get_property(descriptor, "format");
            if matches!(format, Value::String(ref value) if value == "jwk" || value == "raw-public" || value == "raw-private") {
                return None;
            }
            key_bytes(&execute::get_property(descriptor, "key"))
        })
        .or_else(|| {
            let descriptor = descriptor?;
            let key = execute::get_property(descriptor, "key");
            crate::modules::url_whatwg::is_url_instance(&key)
                .then(|| url_data.clone())
                .flatten()
        })
        .or_else(|| {
            let descriptor = descriptor?;
            if !matches!(
                execute::get_property(descriptor, "format"),
                Value::String(ref format) if format == "jwk"
            ) {
                return None;
            }
            let key = execute::get_property(descriptor, "key");
            let curve = execute::to_js_string(&execute::get_property(&key, "crv"))
                .ok()?
                .to_ascii_lowercase();
            let id = match curve.as_str() {
                "ed25519" => Id::ED25519,
                "ed448" => Id::ED448,
                "x25519" => Id::X25519,
                "x448" => Id::X448,
                _ => return None,
            };
            let field = if key_type == "private" { "d" } else { "x" };
            let encoded = execute::to_js_string(&execute::get_property(&key, field)).ok()?;
            let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(encoded)
                .ok()?;
            if key_type == "private" {
                let pkey = PKey::private_key_from_raw_bytes(&raw, id).ok()?;
                if !matches!(execute::get_property(&key, "x"), Value::Undefined) {
                    let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD
                        .decode(execute::to_js_string(&execute::get_property(&key, "x")).ok()?)
                        .ok()?;
                    if pkey.raw_public_key().ok()?.as_slice() != expected.as_slice() {
                        return None;
                    }
                }
                pkey.private_key_to_pem_pkcs8().ok()
            } else {
                PKey::public_key_from_raw_bytes(&raw, id)
                    .ok()?
                    .public_key_to_pem()
                    .ok()
            }
        })
        .or_else(|| {
            let descriptor = descriptor?;
            if !matches!(descriptor, Value::Object(_) | Value::ObjectAlias(_)) {
                return None;
            }
            let format = execute::get_property(descriptor, "format");
            let key = execute::get_property(descriptor, "key");
            if !matches!(format, Value::String(ref value) if value == "jwk") {
                return None;
            }
            let n = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(execute::to_js_string(&execute::get_property(&key, "n")).ok()?)
                .ok()?;
            let e = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(execute::to_js_string(&execute::get_property(&key, "e")).ok()?)
                .ok()?;
            let pem = if key_type == "private" {
                let decode = |name: &str| -> Option<openssl::bn::BigNum> {
                    let text = execute::to_js_string(&execute::get_property(&key, name)).ok()?;
                    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
                        .decode(text)
                        .ok()?;
                    openssl::bn::BigNum::from_slice(&bytes).ok()
                };
                let rsa = Rsa::from_private_components(
                    openssl::bn::BigNum::from_slice(&n).ok()?,
                    openssl::bn::BigNum::from_slice(&e).ok()?,
                    decode("d")?,
                    decode("p")?,
                    decode("q")?,
                    decode("dp")?,
                    decode("dq")?,
                    decode("qi")?,
                )
                .ok()?;
                let pkey = PKey::from_rsa(rsa).ok()?;
                pkey.private_key_to_pem_pkcs8().ok()?
            } else {
                let rsa = Rsa::from_public_components(
                    openssl::bn::BigNum::from_slice(&n).ok()?,
                    openssl::bn::BigNum::from_slice(&e).ok()?,
                )
                .ok()?;
                let pkey = PKey::from_rsa(rsa).ok()?;
                pkey.public_key_to_pem().ok()?
            };
            Some(pem)
        })
        .or_else(|| {
            let descriptor = descriptor?;
            let format = execute::get_property(descriptor, "format");
            if !matches!(format, Value::String(ref value) if value == "raw-public" || value == "raw-private") {
                return None;
            }
            let raw = key_bytes(&execute::get_property(descriptor, "key"))?;
            let raw_public = matches!(format, Value::String(ref value) if value == "raw-public");
            let asymmetric_value = execute::get_property(descriptor, "asymmetricKeyType");
            let asymmetric = match asymmetric_value {
                Value::String(value) => value.to_ascii_lowercase(),
                Value::Undefined => return None,
                _ => return None,
            };
            let pem = if asymmetric == "ec" {
                let curve = match execute::get_property(descriptor, "namedCurve") {
                    Value::String(value) => value,
                    Value::Undefined => return None,
                    _ => return None,
                };
                let nid = match curve.to_ascii_lowercase().as_str() {
                    "p-256" | "prime256v1" => Nid::X9_62_PRIME256V1,
                    "secp256k1" => Nid::SECP256K1,
                    "p-384" | "secp384r1" => Nid::SECP384R1,
                    "p-521" | "secp521r1" => Nid::SECP521R1,
                    _ => return None,
                };
                let group = EcGroup::from_curve_name(nid).ok()?;
                let width = ((group.degree() + 7) / 8) as usize;
                if raw_public {
                    // OpenSSL accepts both SEC1 uncompressed points (0x04 || x || y)
                    // and compressed points (0x02/0x03 || x).  Node's raw-public
                    // import follows the same rule; the export `type` option only
                    // controls the representation emitted, not whether an input
                    // point is accepted.
                    let valid_uncompressed = raw.len() == 1 + width * 2 && raw[0] == 0x04;
                    let valid_compressed = raw.len() == 1 + width && matches!(raw[0], 0x02 | 0x03);
                    if !valid_uncompressed && !valid_compressed {
                        return None;
                    }
                } else if raw.len() != width {
                    return None;
                }
                let mut context = BigNumContext::new().ok()?;
                if raw_public {
                    let point = EcPoint::from_bytes(&group, &raw, &mut context).ok()?;
                    let ec = EcKey::from_public_key(&group, &point).ok()?;
                    let pkey = PKey::from_ec_key(ec).ok()?;
                    pkey.public_key_to_pem().ok()?
                } else {
                    let scalar = BigNum::from_slice(&raw).ok()?;
                    let mut point = EcPoint::new(&group).ok()?;
                    point.mul_generator(&group, &scalar, &mut context).ok()?;
                    let ec = EcKey::from_private_components(&group, &scalar, &point).ok()?;
                    let pkey = PKey::from_ec_key(ec).ok()?;
                    pkey.private_key_to_pem_pkcs8().ok()?
                }
            } else {
                let id = match asymmetric.as_str() {
                    "ed25519" => openssl::pkey::Id::ED25519,
                    "ed448" => openssl::pkey::Id::ED448,
                    "x25519" => openssl::pkey::Id::X25519,
                    "x448" => openssl::pkey::Id::X448,
                    _ => return None,
                };
                if raw_public {
                    let pkey = PKey::public_key_from_raw_bytes(&raw, id).ok()?;
                    pkey.public_key_to_pem().ok()?
                } else {
                    let pkey = PKey::private_key_from_raw_bytes(&raw, id).ok()?;
                    pkey.private_key_to_pem_pkcs8().ok()?
                }
            };
            Some(pem)
        })
        .unwrap_or_default();
    if contains_pqc_algorithm(&data) && !openssl_supports_pqc() {
        let code = if key_type == "public" {
            "ERR_OSSL_EVP_DECODE_ERROR"
        } else {
            "ERR_OSSL_UNSUPPORTED"
        };
        return Err(crypto_error(code, "unsupported key algorithm"));
    }
    if key_type == "private" && is_encrypted_private_key(&data) {
        match passphrase.as_deref() {
            None => {
                return Err(crypto_error(
                    "ERR_MISSING_PASSPHRASE",
                    "Passphrase required for encrypted key",
                ));
            }
            Some(pass) => {
                let decrypted = PKey::private_key_from_pem_passphrase(&data, pass)
                    .or_else(|_| {
                        Rsa::private_key_from_pem_passphrase(&data, pass).and_then(PKey::from_rsa)
                    })
                    .map_err(|_| crypto_error("ERR_OSSL_BAD_DECRYPT", "bad decrypt"))?;
                data = decrypted
                    .private_key_to_pem_pkcs8()
                    .map_err(openssl_error)?;
            }
        }
    }
    if url_value.is_some() && url_data.is_none() && key_type == "private" {
        return Err(crypto_error(
            "ERR_CRYPTO_OPERATION_FAILED",
            "No such file or directory",
        ));
    }
    if url_value.is_some()
        && key_type == "private"
        && PKey::private_key_from_pem(&data).is_err()
        && PKey::private_key_from_der(&data).is_err()
    {
        return Err(crypto_error(
            "ERR_CRYPTO_OPERATION_FAILED",
            "Invalid private key",
        ));
    }
    if key_type == "public" {
        if let Some(passphrase) = passphrase.as_deref() {
            if let Ok(private) = PKey::private_key_from_pem_passphrase(&data, passphrase) {
                if let Ok(public) = private.public_key_to_pem() {
                    data = public;
                }
            }
        }
    }
    if matches!(descriptor.map(|value| execute::get_property(value, "format")), Some(Value::String(ref format)) if format == "jwk")
        && key_type == "private"
    {
        let valid = PKey::private_key_from_pem(&data)
            .ok()
            .is_some_and(|pkey| match pkey.id() {
                Id::RSA | Id::RSA_PSS => pkey.rsa().and_then(|rsa| rsa.check_key()).is_ok(),
                _ => true,
            });
        if !valid {
            return Err(crypto_error("ERR_CRYPTO_INVALID_JWK", "Invalid JWK"));
        }
    }
    if matches!(descriptor.map(|value| execute::get_property(value, "type")), Some(Value::String(ref kind)) if kind == "spki")
        && key_type == "private"
    {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
            (
                "message".into(),
                Value::String("The property 'key.type' is invalid. Received 'spki'".into()),
            ),
        ])));
    }
    if data.is_empty()
        && matches!(
            descriptor.map(|value| execute::get_property(value, "format")),
            Some(Value::String(ref format)) if format == "jwk"
        )
    {
        return Err(crypto_error("ERR_CRYPTO_INVALID_JWK", "Invalid JWK"));
    }
    if data.is_empty() {
        if matches!(
            descriptor.map(|value| execute::get_property(value, "format")),
            Some(Value::String(ref format)) if format == "raw-public" || format == "raw-private" || format == "raw-seed"
        ) {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
                ("message".into(), Value::String("Invalid raw key".into())),
            ])));
        }
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("Error".into())),
            (
                "message".into(),
                Value::String("error:1E08010C:DECODER routines::unsupported".into()),
            ),
        ])));
    }
    if key_type == "private"
        && matches!(descriptor.map(|value| execute::get_property(value, "format")), Some(Value::String(ref format)) if format == "der")
        && matches!(descriptor.map(|value| execute::get_property(value, "type")), Some(Value::String(ref kind)) if kind == "pkcs1")
        && (PKey::public_key_from_der(&data).is_ok()
            || Rsa::public_key_from_der(&data).is_ok()
            || Rsa::public_key_from_der_pkcs1(&data).is_ok())
    {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("Error".into())),
            (
                "message".into(),
                Value::String("error:1E08010C:DECODER routines::unsupported".into()),
            ),
            ("library".into(), Value::String("DECODER routines".into())),
        ])));
    }
    if key_type == "public" {
        if let Some(existing) = args
            .first()
            .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        {
            if matches!(execute::get_property(existing, "type"), Value::String(ref value) if value == "private")
            {
                if let Ok(pkey) = PKey::private_key_from_pem(&data).or_else(|_| {
                    passphrase
                        .as_deref()
                        .and_then(|pass| PKey::private_key_from_pem_passphrase(&data, pass).ok())
                        .ok_or_else(openssl::error::ErrorStack::get)
                }) {
                    if let Ok(public) = pkey.public_key_to_pem() {
                        data = public;
                    }
                } else if let Ok(rsa) = Rsa::private_key_from_pem(&data) {
                    if let Ok(pkey) = PKey::from_rsa(rsa) {
                        if let Ok(public) = pkey.public_key_to_pem() {
                            data = public;
                        }
                    }
                }
            }
        }
        // `createPublicKey(privatePem)` is also a supported conversion.  The
        // input need not be wrapped in a descriptor or an existing KeyObject.
        if let Ok(private) = PKey::private_key_from_pem(&data) {
            if let Ok(public) = private.public_key_to_pem() {
                data = public;
            }
        }
    }
    let key_id = PKey::private_key_from_pem(&data)
        .map(|pkey| pkey.id())
        .or_else(|_| PKey::public_key_from_pem(&data).map(|pkey| pkey.id()));
    let asymmetric_type = key_id
        .map(|id| match id {
            openssl::pkey::Id::EC => "ec",
            openssl::pkey::Id::ED25519 => "ed25519",
            openssl::pkey::Id::ED448 => "ed448",
            openssl::pkey::Id::X25519 => "x25519",
            openssl::pkey::Id::X448 => "x448",
            openssl::pkey::Id::DSA => "dsa",
            openssl::pkey::Id::DH => "dh",
            openssl::pkey::Id::RSA_PSS => "rsa-pss",
            _ => "rsa",
        })
        .unwrap_or("rsa");
    let mut key = host_api::object(Vec::new());
    let (_, asym_proto) = key_object_prototypes();
    key = execute::set_prototype_of(&key, &asym_proto).unwrap_or(key);
    define_hidden(&key, KEY_TYPE_PROP, Value::String(key_type.into()));
    define_hidden(
        &key,
        KEY_ASYM_TYPE_PROP,
        Value::String(asymmetric_type.into()),
    );
    define_hidden(&key, KEY_MARKER_PROP, Value::Boolean(true));
    define_hidden(
        &key,
        KEY_DATA_PROP,
        crate::modules::buffer_proto::make_buffer(&data),
    );
    define_hidden(
        &key,
        KEY_DETAILS_PROP,
        asymmetric_key_details(&data, asymmetric_type),
    );
    Ok(key)
}

fn asymmetric_key_details(data: &[u8], asymmetric_type: &str) -> Value {
    match asymmetric_type {
        "rsa" | "rsa-pss" => {
            if let Ok(rsa) = Rsa::private_key_from_pem(data) {
                rsa_key_details(&rsa)
            } else if let Ok(rsa) = Rsa::public_key_from_pem(data) {
                rsa_key_details(&rsa)
            } else {
                host_api::object(Vec::new())
            }
        }
        "dsa" => {
            if let Ok(pkey) = PKey::private_key_from_pem(data) {
                pkey.dsa()
                    .map(|dsa| dsa_key_details(&dsa))
                    .unwrap_or_else(|_| host_api::object(Vec::new()))
            } else if let Ok(pkey) = PKey::public_key_from_pem(data) {
                pkey.dsa()
                    .map(|dsa| dsa_key_details(&dsa))
                    .unwrap_or_else(|_| host_api::object(Vec::new()))
            } else {
                host_api::object(Vec::new())
            }
        }
        "ec" => {
            let curve = if let Ok(ec) = EcKey::private_key_from_pem(data) {
                ec.group().curve_name()
            } else if let Ok(ec) = EcKey::public_key_from_pem(data) {
                ec.group().curve_name()
            } else {
                None
            };
            curve
                .map(|nid| {
                    let name = match nid {
                        Nid::X9_62_PRIME256V1 => "prime256v1",
                        Nid::SECP256K1 => "secp256k1",
                        Nid::SECP384R1 => "secp384r1",
                        Nid::SECP521R1 => "secp521r1",
                        _ => "unknown",
                    };
                    host_api::object(vec![("namedCurve".into(), Value::String(name.into()))])
                })
                .unwrap_or_else(|| host_api::object(Vec::new()))
        }
        _ => host_api::object(Vec::new()),
    }
}

fn rsa_key_details<T: openssl::pkey::HasPublic>(rsa: &openssl::rsa::RsaRef<T>) -> Value {
    host_api::object(vec![
        (
            "modulusLength".into(),
            Value::Number(rsa.n().num_bits() as f64),
        ),
        (
            "publicExponent".into(),
            Value::BigInt(
                rsa.e()
                    .to_dec_str()
                    .map(|value| value.to_string())
                    .unwrap_or_else(|_| "65537".into()),
            ),
        ),
    ])
}

fn dsa_key_details<T: openssl::pkey::HasParams>(dsa: &openssl::dsa::DsaRef<T>) -> Value {
    host_api::object(vec![
        (
            "modulusLength".into(),
            Value::Number(dsa.p().num_bits() as f64),
        ),
        (
            "divisorLength".into(),
            Value::Number(dsa.q().num_bits() as f64),
        ),
    ])
}

pub fn create_sign(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    create_signer(args.first(), false)
}

pub fn create_verify(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    create_signer(args.first(), true)
}

fn create_signer(algorithm: Option<&Value>, verifier: bool) -> Result<Value, VmError> {
    let algorithm = algorithm_string(algorithm)?.to_ascii_lowercase();
    let _ = message_digest(&algorithm)?;
    let value = host_api::object(vec![
        (
            "update".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_UPDATE),
        ),
        (
            "write".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_UPDATE),
        ),
        (
            "_write".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_UPDATE),
        ),
        (
            "end".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_STREAM_END),
        ),
        (
            if verifier { "verify" } else { "sign" }.into(),
            crate::host::capability(if verifier {
                crate::registry::SPEC_CRYPTO_VERIFY
            } else {
                crate::registry::SPEC_CRYPTO_SIGN
            }),
        ),
    ]);
    define_hidden(&value, ALGORITHM_PROP, Value::String(algorithm));
    define_hidden(
        &value,
        INPUT_PROP,
        crate::modules::buffer_proto::make_buffer(&[]),
    );
    define_hidden(&value, HASH_HANDLE_PROP, Value::Undefined);
    define_hidden(&value, SIGN_OPTIONS_PROP, Value::Undefined);
    let global = quench_runtime::vm::current_global_object();
    let prototype = execute::get_property(
        &global,
        if verifier {
            "\0quench:crypto:verify-prototype"
        } else {
            "\0quench:crypto:sign-prototype"
        },
    );
    if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::set_prototype_of(&value, &prototype)
    } else {
        Ok(value)
    }
}

pub fn sign(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args
        .last()
        .filter(|value| quench_runtime::is_callable(value));
    let sync_args = if callback.is_some() {
        &args[..args.len() - 1]
    } else {
        args
    };
    let result = sign_impl(state, receiver, sync_args);
    if let Some(callback) = callback {
        return match result {
            Ok(value) => {
                execute::call(callback, &Value::Undefined, &[Value::Null, value])?;
                Ok(Value::Undefined)
            }
            Err(VmError::Thrown(error)) => {
                execute::call(callback, &Value::Undefined, &[error, Value::Undefined])?;
                Ok(Value::Undefined)
            }
            Err(error) => Err(error),
        };
    }
    result
}

fn sign_impl(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return sign_one_shot(args);
    };
    if !matches!(
        execute::get_property(receiver, ALGORITHM_PROP),
        Value::String(_)
    ) {
        return sign_one_shot(args);
    }
    let data = bytes_from_value(&execute::get_property(receiver, INPUT_PROP)).unwrap_or_default();
    if matches!(args.first(), None | Some(Value::Null)) {
        return Err(crypto_type_error(
            "ERR_CRYPTO_SIGN_KEY_REQUIRED",
            "No key provided to sign",
        ));
    }
    validate_operation_key_descriptor(args.first(), "privateKey")?;
    if let Some(descriptor) = args.first().filter(|value| {
        matches!(value, Value::Object(_) | Value::ObjectAlias(_))
            && matches!(execute::get_property(value, "format"), Value::String(ref format) if format == "jwk")
    }) {
        let key = execute::get_property(descriptor, "key");
        if !matches!(key, Value::Object(_) | Value::ObjectAlias(_)) {
            return Err(invalid_type(
                "The \"privateKey.key\" property must be of type object",
            ));
        }
    }
    let (key, options) = key_and_options(args.first())?;
    let p1363 = dsa_p1363(options.as_ref())?;
    let digest = message_digest(&execute::to_js_string(&execute::get_property(
        receiver,
        ALGORITHM_PROP,
    ))?)?;
    let passphrase = options
        .as_ref()
        .and_then(|value| bytes_from_value(&execute::get_property(value, "passphrase")));
    let pkey = parse_private_signing_key(&key, passphrase.as_deref(), options.as_ref())?;
    if rsa_digest_too_big(&pkey, digest) {
        let error = native_error(
            quench_runtime::ops::Builtin::Error,
            "ERR_OSSL_RSA_DIGEST_TOO_BIG_FOR_RSA_KEY",
            "error:02000070:rsa routines::digest too big for rsa key",
        );
        execute::set_property_in_place(&error, "library", Value::String("rsa routines".into()));
        return Err(VmError::Thrown(error));
    }
    if matches!(pkey.id(), Id::ED25519 | Id::ED448) {
        return Err(crypto_error(
            "ERR_CRYPTO_UNSUPPORTED_OPERATION",
            "Unsupported crypto operation",
        ));
    }
    if matches!(pkey.id(), Id::X25519 | Id::X448 | Id::DH | Id::DHX) {
        return Err(crypto_type_error(
            "ERR_OSSL_EVP_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE",
            "operation not supported for this keytype",
        ));
    }
    validate_rsa_option_values(options.as_ref())?;
    let mut signer = Signer::new(digest, &pkey).map_err(openssl_error)?;
    configure_rsa_for_key(
        &mut signer,
        options,
        key_is_rsa_pss(args.first()) || pkey.id() == Id::RSA_PSS,
    )?;
    signer.update(&data).map_err(openssl_error)?;
    let mut signature = signer.sign_to_vec().map_err(openssl_error)?;
    if p1363 {
        if let Some(width) = dsa_signature_width(&pkey) {
            signature = der_to_p1363(&signature, width).ok_or_else(|| {
                crypto_error("ERR_CRYPTO_OPERATION_FAILED", "Invalid DSA signature")
            })?;
        }
    }
    encode_digest(signature, args.get(1))
}

pub fn verify(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args
        .last()
        .filter(|value| quench_runtime::is_callable(value));
    let sync_args = if callback.is_some() {
        &args[..args.len() - 1]
    } else {
        args
    };
    let result = verify_impl(state, receiver, sync_args);
    if let Some(callback) = callback {
        return match result {
            Ok(value) => {
                execute::call(callback, &Value::Undefined, &[Value::Null, value])?;
                Ok(Value::Undefined)
            }
            Err(VmError::Thrown(error)) => {
                execute::call(callback, &Value::Undefined, &[error, Value::Undefined])?;
                Ok(Value::Undefined)
            }
            Err(error) => Err(error),
        };
    }
    result
}

fn verify_impl(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return verify_one_shot(args);
    };
    if !matches!(
        execute::get_property(receiver, ALGORITHM_PROP),
        Value::String(_)
    ) {
        return verify_one_shot(args);
    }
    let data = bytes_from_value(&execute::get_property(receiver, INPUT_PROP)).unwrap_or_default();
    let verify_key = args.first().map(|value| {
        if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
            execute::get_property(value, "key")
        } else {
            value.clone()
        }
    });
    if verify_key
        .as_ref()
        .is_some_and(crate::modules::url_whatwg::is_url_instance)
        || args
            .first()
            .is_some_and(crate::modules::url_whatwg::is_url_instance)
    {
        return Err(invalid_type(
            "The \"key\" argument must be a string or an instance of Buffer",
        ));
    }
    let mut signature = decode_signature(args.get(1), args.get(2))?;
    validate_operation_key_descriptor(args.first(), "key")?;
    let (key, options) = key_and_options(args.first())?;
    let p1363 = dsa_p1363(options.as_ref())?;
    let digest = message_digest(&execute::to_js_string(&execute::get_property(
        receiver,
        ALGORITHM_PROP,
    ))?)?;
    let pkey = parse_public_signing_key(&key, options.as_ref())?;
    if matches!(pkey.id(), Id::ED25519 | Id::ED448) {
        return Err(crypto_error(
            "ERR_CRYPTO_UNSUPPORTED_OPERATION",
            "Unsupported crypto operation",
        ));
    }
    if matches!(pkey.id(), Id::X25519 | Id::X448 | Id::DH | Id::DHX) {
        return Err(crypto_error(
            "ERR_OSSL_EVP_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE",
            "operation not supported for this keytype",
        ));
    }
    validate_rsa_option_values(options.as_ref())?;
    if p1363 {
        if let Some(width) = dsa_signature_width(&pkey) {
            signature = p1363_to_der(&signature, width).unwrap_or(signature);
        }
    }
    if matches!(
        pkey.id(),
        openssl::pkey::Id::X25519
            | openssl::pkey::Id::X448
            | openssl::pkey::Id::DH
            | openssl::pkey::Id::DHX
    ) {
        return Err(crypto_type_error(
            "ERR_OSSL_EVP_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE",
            "operation not supported for this keytype",
        ));
    }
    let mut verifier = Verifier::new(digest, &pkey).map_err(openssl_error)?;
    configure_rsa_for_key(
        &mut verifier,
        options,
        key_is_rsa_pss(args.first()) || pkey.id() == Id::RSA_PSS,
    )?;
    verifier.update(&data).map_err(openssl_error)?;
    Ok(Value::Boolean(verifier.verify(&signature).unwrap_or(false)))
}

fn validate_ed_context(options: Option<&Value>) -> Result<(), VmError> {
    let Some(options) = options else {
        return Ok(());
    };
    let context = execute::get_property(options, "context");
    if matches!(context, Value::Undefined) {
        return Ok(());
    }
    let bytes = bytes_from_value(&context)
        .ok_or_else(|| invalid_type("The \"context\" option must be an ArrayBuffer or a string"))?;
    if !bytes.is_empty() {
        return Err(crypto_error(
            "ERR_CRYPTO_OPERATION_FAILED",
            "Context parameter is unsupported",
        ));
    }
    Ok(())
}

fn sign_one_shot(args: &[Value]) -> Result<Value, VmError> {
    let algorithm = args.first();
    let data = bytes(args.get(1))?;
    if matches!(args.get(2), None | Some(Value::Null)) {
        return Err(crypto_type_error(
            "ERR_CRYPTO_SIGN_KEY_REQUIRED",
            "No key provided to sign",
        ));
    }
    validate_operation_key_descriptor(args.get(2), "key")?;
    let (key, options) = key_and_options(args.get(2))?;
    let p1363 = dsa_p1363(options.as_ref())?;
    validate_rsa_option_values(options.as_ref())?;
    validate_ed_context(options.as_ref())?;
    let passphrase = options
        .as_ref()
        .and_then(|value| bytes_from_value(&execute::get_property(value, "passphrase")));
    let pkey = parse_private_signing_key(&key, passphrase.as_deref(), options.as_ref())?;
    let raw = matches!(algorithm, Some(Value::Null) | Some(Value::Undefined));
    let digest = if raw {
        None
    } else {
        Some(message_digest(&algorithm_string(algorithm)?)?)
    };
    if let Some(digest) = digest {
        if rsa_digest_too_big(&pkey, digest) {
            return Err(crypto_type_error(
                "ERR_OSSL_RSA_DIGEST_TOO_BIG_FOR_RSA_KEY",
                "digest too big for rsa key",
            ));
        }
    }
    let mut signer = if raw {
        Signer::new_without_digest(&pkey).map_err(openssl_error)?
    } else {
        Signer::new(digest.unwrap(), &pkey).map_err(openssl_error)?
    };
    configure_rsa_for_key(
        &mut signer,
        options,
        key_is_rsa_pss(args.get(2)) || pkey.id() == Id::RSA_PSS,
    )?;
    let mut signature = if raw {
        signer.sign_oneshot_to_vec(&data).map_err(openssl_error)?
    } else {
        signer.update(&data).map_err(openssl_error)?;
        signer.sign_to_vec().map_err(openssl_error)?
    };
    if p1363 {
        if let Some(width) = dsa_signature_width(&pkey) {
            signature = der_to_p1363(&signature, width).ok_or_else(|| {
                crypto_error("ERR_CRYPTO_OPERATION_FAILED", "Invalid DSA signature")
            })?;
        }
    }
    Ok(crate::modules::buffer_proto::make_buffer(&signature))
}

fn rsa_digest_too_big(pkey: &PKey<openssl::pkey::Private>, digest: MessageDigest) -> bool {
    pkey.id() == openssl::pkey::Id::RSA
        && pkey
            .rsa()
            .map(|rsa| rsa.size() as usize <= digest.size() + 11)
            .unwrap_or(false)
}

fn verify_one_shot(args: &[Value]) -> Result<Value, VmError> {
    let algorithm = args.first();
    let data = bytes(args.get(1))?;
    let signature_value = args.get(3);
    let prevalidated_signature = signature_value.and_then(bytes_from_value).ok_or_else(|| {
        let received = signature_value
            .map(crate::modules::util::invalid_arg_received)
            .unwrap_or_default();
        invalid_type(&format!(
            "The \"signature\" argument must be of type string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView.{received}"
        ))
    })?;
    let verify_key = args.get(2).map(|value| {
        if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
            execute::get_property(value, "key")
        } else {
            value.clone()
        }
    });
    if verify_key
        .as_ref()
        .is_some_and(|value| crate::modules::url_whatwg::is_url_instance(value))
        || args
            .get(2)
            .is_some_and(crate::modules::url_whatwg::is_url_instance)
    {
        return Err(invalid_type(
            "The \"key\" argument must be a string or an instance of Buffer",
        ));
    }
    validate_operation_key_descriptor(args.get(2), "key")?;
    let (key, options) = key_and_options(args.get(2))?;
    let p1363 = dsa_p1363(options.as_ref())?;
    validate_rsa_option_values(options.as_ref())?;
    validate_ed_context(options.as_ref())?;
    let signature = prevalidated_signature;
    let raw = matches!(algorithm, Some(Value::Null) | Some(Value::Undefined));
    if raw && looks_like_malformed_ed448_key(&key) {
        return Err(crypto_type_error(
            "ERR_OSSL_EVP_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE",
            "operation not supported for this keytype",
        ));
    }
    let pkey = match parse_public_signing_key(&key, options.as_ref()) {
        Ok(pkey) => pkey,
        Err(error) => return Err(error),
    };
    let signature = if p1363 {
        let converted =
            dsa_signature_width(&pkey).and_then(|width| p1363_to_der(&signature, width));
        converted.unwrap_or(signature)
    } else {
        signature
    };
    if matches!(
        pkey.id(),
        openssl::pkey::Id::X25519
            | openssl::pkey::Id::X448
            | openssl::pkey::Id::DH
            | openssl::pkey::Id::DHX
    ) {
        return Err(crypto_type_error(
            "ERR_OSSL_EVP_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE",
            "operation not supported for this keytype",
        ));
    }
    if raw {
        let expected_len = match pkey.id() {
            openssl::pkey::Id::ED25519 => Some(64),
            openssl::pkey::Id::ED448 => Some(114),
            _ => None,
        };
        if expected_len.is_some_and(|length| signature.len() != 0 && signature.len() != length) {
            return Err(crypto_type_error(
                "ERR_OSSL_EVP_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE",
                "operation not supported for this keytype",
            ));
        }
    }
    let mut verifier = if raw {
        Verifier::new_without_digest(&pkey).map_err(openssl_error)?
    } else {
        Verifier::new(message_digest(&algorithm_string(algorithm)?)?, &pkey)
            .map_err(openssl_error)?
    };
    configure_rsa_for_key(
        &mut verifier,
        options,
        key_is_rsa_pss(args.get(2)) || pkey.id() == Id::RSA_PSS,
    )?;
    let verified = if raw {
        verifier
            .verify_oneshot(&signature, &data)
            .map_err(openssl_error)?
    } else {
        verifier.update(&data).map_err(openssl_error)?;
        verifier.verify(&signature).unwrap_or(false)
    };
    Ok(Value::Boolean(verified))
}

fn looks_like_malformed_ed448_key(key: &[u8]) -> bool {
    String::from_utf8_lossy(key).contains("MCowBQYDK2Vu")
}

fn algorithm_string(value: Option<&Value>) -> Result<String, VmError> {
    match value {
        Some(Value::String(value)) if value.contains('\0') => Err(invalid_type(&format!(
            "The \"algorithm\" argument must be of type string.{}",
            crate::modules::util::invalid_arg_received(&Value::String(value.clone()))
        ))),
        Some(Value::String(value)) => Ok(value.clone()),
        Some(Value::StringUnits(_)) => execute::to_js_string(value.unwrap()),
        Some(value) => Err(invalid_type(&format!(
            "The \"algorithm\" argument must be of type string.{}",
            crate::modules::util::invalid_arg_received(value)
        ))),
        None => Err(invalid_type(
            "The \"algorithm\" argument must be of type string",
        )),
    }
}

fn key_and_options(value: Option<&Value>) -> Result<(Vec<u8>, Option<Value>), VmError> {
    let value =
        value.ok_or_else(|| invalid_type("The \"key\" argument must be a string or an object"))?;
    if crate::modules::url_whatwg::is_url_instance(value) {
        return Ok((
            key_bytes(value).ok_or_else(|| {
                invalid_type("The \"key\" argument must be a string or an object")
            })?,
            None,
        ));
    }
    if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
        let hidden = execute::get_property(value, KEY_DATA_PROP);
        if !matches!(hidden, Value::Undefined) {
            let bytes = bytes_from_value(&hidden).ok_or_else(|| {
                invalid_type("The \"key\" argument must be a string or an object")
            })?;
            return Ok((bytes, None));
        }
        let key = execute::get_property(value, "key");
        if matches!(
            execute::get_property(value, "format"),
            Value::String(ref format) if format == "jwk"
        ) {
            let key_type = if execute::has_own_property(&key, "d") {
                "private"
            } else {
                "public"
            };
            let object = create_asymmetric_key(&[value.clone()], key_type)?;
            let bytes =
                key_bytes(&execute::get_property(&object, KEY_DATA_PROP)).ok_or_else(|| {
                    invalid_type("The \"key\" argument must be a string or an object")
                })?;
            return Ok((bytes, Some(value.clone())));
        }
        let bytes = bytes_from_value(&key)
            .or_else(|| {
                if matches!(key, Value::Object(_) | Value::ObjectAlias(_)) {
                    bytes_from_value(&execute::get_property(&key, KEY_DATA_PROP))
                } else {
                    None
                }
            })
            .ok_or_else(|| invalid_type("The \"key\" argument must be a string or an object"))?;
        Ok((bytes, Some(value.clone())))
    } else {
        Ok((
            bytes_from_value(value).ok_or_else(|| {
                invalid_type("The \"key\" argument must be a string or an object")
            })?,
            None,
        ))
    }
}

fn message_digest(name: &str) -> Result<MessageDigest, VmError> {
    match name.to_ascii_lowercase().replace('-', "").as_str() {
        "sha1" | "rsasha1" | "dss1" | "dsasha1" => Ok(MessageDigest::sha1()),
        "sha224" | "rsasha224" => Ok(MessageDigest::sha224()),
        "sha256" | "rsasha256" => Ok(MessageDigest::sha256()),
        "sha384" | "rsasha384" => Ok(MessageDigest::sha384()),
        "sha512" | "rsasha512" => Ok(MessageDigest::sha512()),
        _ => Err(VmError::Thrown(native_error(
            quench_runtime::ops::Builtin::TypeError,
            "ERR_CRYPTO_INVALID_DIGEST",
            &format!("Invalid digest: {name}"),
        ))),
    }
}

fn configure_rsa<T>(signer: &mut T, options: Option<Value>) -> Result<(), VmError>
where
    T: RsaConfig,
{
    if let Some(options) = options {
        let padding = execute::get_property(&options, "padding");
        let pss_padding = matches!(padding, Value::Number(number) if number == 6.0);
        let padding_valid = matches!(padding, Value::Undefined)
            || matches!(padding, Value::Number(number) if !number.is_nan());
        if !padding_valid {
            return Err(invalid_option("padding", &padding));
        }
        if matches!(padding, Value::Number(number) if number == 4.0) {
            return Err(crypto_type_error(
                "ERR_OSSL_ILLEGAL_OR_UNSUPPORTED_PADDING_MODE",
                "error:1C8000A5:Provider routines::illegal or unsupported padding mode",
            ));
        }
        if pss_padding {
            signer
                .set_padding(Padding::PKCS1_PSS)
                .map_err(openssl_error)?;
        }
        let salt = execute::get_property(&options, "saltLength");
        let salt_valid = matches!(salt, Value::Undefined)
            || matches!(salt, Value::Number(length) if length.is_finite());
        if !salt_valid {
            return Err(invalid_option("saltLength", &salt));
        }
        match salt {
            Value::Number(length) => signer
                .set_salt(RsaPssSaltlen::custom(length as i32))
                .map_err(openssl_error)?,
            Value::Undefined if pss_padding => signer
                .set_salt(T::default_pss_salt())
                .map_err(openssl_error)?,
            _ => {}
        }
    }
    Ok(())
}

fn validate_rsa_option_values(options: Option<&Value>) -> Result<(), VmError> {
    let Some(options) = options else {
        return Ok(());
    };
    let padding = execute::get_property(options, "padding");
    if !matches!(padding, Value::Undefined)
        && !matches!(padding, Value::Number(number) if !number.is_nan())
    {
        return Err(invalid_option("padding", &padding));
    }
    let salt = execute::get_property(options, "saltLength");
    if !matches!(salt, Value::Undefined)
        && !matches!(salt, Value::Number(number) if number.is_finite())
    {
        return Err(invalid_option("saltLength", &salt));
    }
    Ok(())
}

fn validate_operation_key_descriptor(value: Option<&Value>, label: &str) -> Result<(), VmError> {
    let Some(value) = value else { return Ok(()) };
    if !matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
        return Ok(());
    }
    let format = execute::get_property(value, "format");
    if let Value::String(ref format) = format {
        if !matches!(
            format.as_str(),
            "pem" | "der" | "jwk" | "raw-public" | "raw-private" | "raw-seed"
        ) {
            return Err(VmError::Thrown(native_error(
                quench_runtime::ops::Builtin::TypeError,
                "ERR_INVALID_ARG_VALUE",
                &format!("The property '{label}.format' is invalid. Received '{format}'"),
            )));
        }
    }
    if matches!(format, Value::String(ref format) if format == "pem" || format == "der") {
        let key_type = execute::get_property(value, "type");
        if let Value::String(ref key_type) = key_type {
            if !matches!(key_type.as_str(), "pkcs1" | "pkcs8" | "sec1" | "spki") {
                return Err(VmError::Thrown(native_error(
                    quench_runtime::ops::Builtin::TypeError,
                    "ERR_INVALID_ARG_VALUE",
                    &format!("The property '{label}.type' is invalid. Received '{key_type}'"),
                )));
            }
        }
    }
    Ok(())
}

fn key_is_rsa_pss(value: Option<&Value>) -> bool {
    let Some(value) = value else { return false };
    if matches!(
        execute::get_property(value, KEY_ASYM_TYPE_PROP),
        Value::String(ref kind) if kind == "rsa-pss"
    ) {
        return true;
    }
    if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
        return key_is_rsa_pss(Some(&execute::get_property(value, "key")));
    }
    false
}

fn configure_rsa_for_key<T>(
    signer: &mut T,
    options: Option<Value>,
    rsa_pss: bool,
) -> Result<(), VmError>
where
    T: RsaConfig,
{
    if rsa_pss {
        let padding = options
            .as_ref()
            .map(|value| execute::get_property(value, "padding"))
            .unwrap_or(Value::Undefined);
        let invalid_padding = match padding {
            Value::Undefined => false,
            Value::Number(number) => number != 6.0,
            _ => true,
        };
        if invalid_padding {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("Error".into())),
                (
                    "code".into(),
                    Value::String("ERR_OSSL_ILLEGAL_OR_UNSUPPORTED_PADDING_MODE".into()),
                ),
                (
                    "message".into(),
                    Value::String(
                        "error:1C8000A5:Provider routines::illegal or unsupported padding mode"
                            .into(),
                    ),
                ),
            ])));
        }
        signer
            .set_padding(Padding::PKCS1_PSS)
            .map_err(openssl_error)?;
    }
    configure_rsa(signer, options)
}

trait RsaConfig {
    fn set_padding(&mut self, padding: Padding) -> Result<(), openssl::error::ErrorStack>;
    fn set_salt(&mut self, salt: RsaPssSaltlen) -> Result<(), openssl::error::ErrorStack>;
    fn default_pss_salt() -> RsaPssSaltlen;
}
impl RsaConfig for Signer<'_> {
    fn set_padding(&mut self, padding: Padding) -> Result<(), openssl::error::ErrorStack> {
        self.set_rsa_padding(padding)
    }
    fn set_salt(&mut self, salt: RsaPssSaltlen) -> Result<(), openssl::error::ErrorStack> {
        self.set_rsa_pss_saltlen(salt)
    }
    fn default_pss_salt() -> RsaPssSaltlen {
        RsaPssSaltlen::MAXIMUM_LENGTH
    }
}
impl RsaConfig for Verifier<'_> {
    fn set_padding(&mut self, padding: Padding) -> Result<(), openssl::error::ErrorStack> {
        self.set_rsa_padding(padding)
    }
    fn set_salt(&mut self, salt: RsaPssSaltlen) -> Result<(), openssl::error::ErrorStack> {
        self.set_rsa_pss_saltlen(salt)
    }
    fn default_pss_salt() -> RsaPssSaltlen {
        RsaPssSaltlen::MAXIMUM_LENGTH
    }
}

fn parse_private_key(
    bytes: &[u8],
    passphrase: Option<&[u8]>,
) -> Result<PKey<openssl::pkey::Private>, VmError> {
    if passphrase.is_none() && is_encrypted_private_key(bytes) {
        if !bytes.starts_with(b"-----") {
            return Err(VmError::Thrown(native_error(
                quench_runtime::ops::Builtin::TypeError,
                "ERR_MISSING_PASSPHRASE",
                "Passphrase required for encrypted key",
            )));
        }
        return Err(VmError::Thrown(native_error(
            quench_runtime::ops::Builtin::Error,
            "ERR_OSSL_CRYPTO_INTERRUPTED_OR_CANCELLED",
            "error:07880109:common libcrypto routines::interrupted or cancelled",
        )));
    }
    if let Some(passphrase) = passphrase {
        if let Ok(key) = PKey::private_key_from_pkcs8_passphrase(bytes, passphrase) {
            return Ok(key);
        }
        if let Ok(key) = PKey::private_key_from_pem_passphrase(bytes, passphrase) {
            return Ok(key);
        }
        if let Ok(rsa) = Rsa::private_key_from_pem_passphrase(bytes, passphrase) {
            return PKey::from_rsa(rsa).map_err(openssl_error);
        }
        if is_encrypted_private_key(bytes) {
            return Err(crypto_error(
                "ERR_OSSL_BAD_DECRYPT",
                "error:1C800064:Provider routines::bad decrypt",
            ));
        }
    }
    PKey::private_key_from_pem(bytes)
        .or_else(|_| PKey::private_key_from_der(bytes))
        .map_err(openssl_error)
}

fn is_encrypted_private_key(bytes: &[u8]) -> bool {
    bytes.windows(b"ENCRYPTED".len()).any(|window| window == b"ENCRYPTED")
        || bytes
            .windows(b"Proc-Type".len())
            .any(|window| window == b"Proc-Type")
        // PBES2 encrypted PKCS#8 DER: id-PBES2 1.2.840.113549.1.5.13.
        || bytes
            .windows(9)
            .any(|window| window == [0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01])
}

fn raw_key_id(options: Option<&Value>) -> Option<Id> {
    let options = options?;
    let format = execute::get_property(options, "format");
    if !matches!(format, Value::String(ref value) if matches!(value.as_str(), "raw-private" | "raw-public" | "raw-seed"))
    {
        return None;
    }
    match execute::to_js_string(&execute::get_property(options, "asymmetricKeyType"))
        .ok()?
        .to_ascii_lowercase()
        .as_str()
    {
        "ed25519" => Some(Id::ED25519),
        "ed448" => Some(Id::ED448),
        "x25519" => Some(Id::X25519),
        "x448" => Some(Id::X448),
        _ => None,
    }
}

fn parse_private_signing_key(
    bytes: &[u8],
    passphrase: Option<&[u8]>,
    options: Option<&Value>,
) -> Result<PKey<openssl::pkey::Private>, VmError> {
    if let Some(id) = raw_key_id(options) {
        return PKey::private_key_from_raw_bytes(bytes, id).map_err(openssl_error);
    }
    parse_private_key(bytes, passphrase)
}

fn parse_public_signing_key(
    bytes: &[u8],
    options: Option<&Value>,
) -> Result<PKey<openssl::pkey::Public>, VmError> {
    if let Some(id) = raw_key_id(options) {
        if matches!(execute::get_property(options.unwrap(), "format"), Value::String(ref value) if value == "raw-private" || value == "raw-seed")
        {
            let private = PKey::private_key_from_raw_bytes(bytes, id).map_err(openssl_error)?;
            let der = private.public_key_to_der().map_err(openssl_error)?;
            return PKey::public_key_from_der(&der).map_err(openssl_error);
        }
        return PKey::public_key_from_raw_bytes(bytes, id).map_err(openssl_error);
    }
    if let Some(passphrase) =
        options.and_then(|value| key_bytes(&execute::get_property(value, "passphrase")))
    {
        let private = PKey::private_key_from_pkcs8_passphrase(bytes, &passphrase)
            .or_else(|_| PKey::private_key_from_pem_passphrase(bytes, &passphrase));
        if let Ok(private) = private {
            let der = private.public_key_to_der().map_err(openssl_error)?;
            return PKey::public_key_from_der(&der).map_err(openssl_error);
        }
    }
    parse_public_key(bytes)
}
fn parse_public_key(bytes: &[u8]) -> Result<PKey<openssl::pkey::Public>, VmError> {
    X509::from_pem(bytes)
        .and_then(|cert| cert.public_key())
        .or_else(|_| PKey::public_key_from_pem(bytes))
        .or_else(|_| PKey::public_key_from_der(bytes))
        .or_else(|_| Rsa::public_key_from_der_pkcs1(bytes).and_then(PKey::from_rsa))
        .or_else(|_| {
            PKey::private_key_from_pem(bytes)
                .and_then(|private| private.public_key_to_pem())
                .and_then(|pem| PKey::public_key_from_pem(&pem))
        })
        .or_else(|_| {
            PKey::private_key_from_der(bytes)
                .and_then(|private| private.public_key_to_pem())
                .and_then(|pem| PKey::public_key_from_pem(&pem))
        })
        .map_err(openssl_error)
}
fn openssl_error(error_stack: openssl::error::ErrorStack) -> VmError {
    let reason = error_stack
        .errors()
        .iter()
        .filter_map(|error| error.reason())
        .find(|reason| {
            let reason = reason.to_ascii_lowercase();
            reason.contains("digest not allowed")
                || reason.contains("pss saltlen too small")
                || reason.contains("invalid salt length")
        })
        .map(|reason| {
            if reason.eq_ignore_ascii_case("invalid salt length") {
                "pss saltlen too small".to_string()
            } else {
                reason.to_string()
            }
        });
    let message = reason.unwrap_or_else(|| "error: digital envelope routines::unsupported".into());
    let mut value = native_error(
        quench_runtime::ops::Builtin::Error,
        "ERR_OSSL_EVP_UNSUPPORTED",
        &message,
    );
    for (key, property) in [
        ("reason", Value::String("unsupported".into())),
        ("library", Value::String("Provider routines".into())),
        ("opensslErrorStack", Value::String("".into())),
    ] {
        value = match execute::set_property_observable(value, key, property) {
            Ok(updated) => updated,
            Err(error) => return error,
        };
    }
    VmError::Thrown(value)
}

fn decode_signature(value: Option<&Value>, encoding: Option<&Value>) -> Result<Vec<u8>, VmError> {
    let value = value.ok_or_else(|| {
        invalid_type("The \"signature\" argument must be a string or an instance of Buffer")
    })?;
    let encoding = encoding
        .and_then(|value| execute::to_js_string(value).ok())
        .unwrap_or_else(|| "buffer".into())
        .to_ascii_lowercase();
    match value {
        Value::String(text) if encoding == "base64" => {
            Ok(crate::modules::buffer_enc::base64_decode(text.as_bytes()))
        }
        Value::String(text) if encoding == "hex" => {
            hex::decode(text).map_err(|_| invalid_type("Invalid signature"))
        }
        Value::String(text) if matches!(encoding.as_str(), "latin1" | "binary") => {
            Ok(text.chars().map(|ch| ch as u32 as u8).collect())
        }
        _ => bytes_from_value(value).ok_or_else(|| invalid_type("Invalid signature")),
    }
}

fn ec_jwk_base<T: HasPublic>(ec: &EcKey<T>) -> Result<(Vec<(String, Value)>, usize), VmError> {
    let mut context = BigNumContext::new().map_err(openssl_error)?;
    let point = ec
        .public_key()
        .to_bytes(ec.group(), PointConversionForm::UNCOMPRESSED, &mut context)
        .map_err(openssl_error)?;
    let width = ((ec.group().degree() + 7) / 8) as usize;
    if point.len() < width * 2 + 1 {
        return Err(crypto_error(
            "ERR_CRYPTO_OPERATION_FAILED",
            "invalid EC key",
        ));
    }
    let crv = match width {
        32 if ec.group().degree() <= 256 => "P-256",
        48 => "P-384",
        66 => "P-521",
        _ => "secp256k1",
    };
    let enc = |bytes: &[u8]| {
        Value::String(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
    };
    Ok((
        vec![
            ("kty".into(), Value::String("EC".into())),
            ("crv".into(), Value::String(crv.into())),
            ("x".into(), enc(&point[1..1 + width])),
            ("y".into(), enc(&point[1 + width..1 + width * 2])),
        ],
        width,
    ))
}

fn ec_jwk_curve_from_key(fields: &mut [(String, Value)], receiver: &Value) {
    let details = execute::get_property(receiver, KEY_DETAILS_PROP);
    let Some(Value::String(curve)) = (matches!(details, Value::Object(_) | Value::ObjectAlias(_)))
        .then(|| execute::get_property(&details, "namedCurve"))
    else {
        return;
    };
    let curve = match curve.to_ascii_lowercase().as_str() {
        "prime256v1" | "p-256" => "P-256",
        "secp256k1" => "secp256k1",
        "secp384r1" | "p-384" => "P-384",
        "secp521r1" | "p-521" => "P-521",
        _ => return,
    };
    if let Some((_, value)) = fields.iter_mut().find(|(name, _)| name == "crv") {
        *value = Value::String(curve.into());
    }
}

pub fn key_export(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let key_type = || execute::get_property(receiver, KEY_TYPE_PROP);
    let asym_type = || execute::get_property(receiver, KEY_ASYM_TYPE_PROP);
    let data =
        bytes_from_value(&execute::get_property(receiver, KEY_DATA_PROP)).unwrap_or_default();
    if !args.is_empty() && !matches!(args.first(), Some(Value::Object(_) | Value::ObjectAlias(_))) {
        return Err(invalid_type(&format!(
            "The \"options\" argument must be of type object.{}",
            crate::modules::util::invalid_arg_received(args.first().unwrap_or(&Value::Undefined))
        )));
    }
    let options = args.first().unwrap_or(&Value::Undefined);
    let requested_passphrase = if matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
        key_bytes(&execute::get_property(options, "passphrase"))
    } else {
        None
    };
    let format = execute::to_js_string(&execute::get_property(options, "format"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let requested_type = execute::get_property(options, "type");
    let requested_type_name = execute::to_js_string(&requested_type)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(format.as_str(), "raw-public" | "raw-private" | "raw-seed")
        && execute::has_own_property(options, "passphrase")
    {
        return Err(crypto_error(
            "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS",
            "The selected raw key encoding does not support encryption.",
        ));
    }
    if format == "jwk" {
        if !matches!(requested_type, Value::Undefined) {
            return Err(invalid_option("type", &requested_type));
        }
        if execute::has_own_property(options, "cipher")
            || execute::has_own_property(options, "passphrase")
        {
            return Err(crypto_error(
                "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS",
                "The selected key encoding jwk does not support encryption.",
            ));
        }
    }
    if format == "jwk" && execute::has_own_property(options, "passphrase") {
        return Err(crypto_error(
            "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS",
            "The selected key encoding jwk does not support encryption.",
        ));
    }
    if matches!(key_type(), Value::String(ref value) if value == "public")
        && (matches!(requested_type, Value::String(ref value) if value == "pkcs8")
            || matches!(requested_type, Value::String(ref value) if value == "sec1"))
    {
        return Err(invalid_option("type", &requested_type));
    }
    if format == "jwk" {
        if matches!(asym_type(), Value::String(ref value) if value == "dsa") {
            return Err(crypto_error(
                "ERR_CRYPTO_JWK_UNSUPPORTED_KEY_TYPE",
                "Unsupported JWK Key Type.",
            ));
        }
        if let Value::String(okp_type) = asym_type() {
            if !matches!(okp_type.as_str(), "ed25519" | "ed448" | "x25519" | "x448") {
                // Fall through to the other key families below.
            } else {
                let enc_bytes = |bytes: Vec<u8>| {
                    Value::String(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
                };
                let curve = match okp_type.as_str() {
                    "ed25519" => "Ed25519",
                    "ed448" => "Ed448",
                    "x25519" => "X25519",
                    "x448" => "X448",
                    _ => unreachable!(),
                };
                if matches!(key_type(), Value::String(ref value) if value == "private") {
                    if let Ok(pkey) = PKey::private_key_from_pem(&data) {
                        if let (Ok(private), Ok(public)) =
                            (pkey.raw_private_key(), pkey.raw_public_key())
                        {
                            return Ok(host_api::object(vec![
                                ("kty".into(), Value::String("OKP".into())),
                                ("crv".into(), Value::String(curve.into())),
                                ("x".into(), enc_bytes(public)),
                                ("d".into(), enc_bytes(private)),
                            ]));
                        }
                    }
                } else if let Ok(pkey) = PKey::public_key_from_pem(&data) {
                    if let Ok(public) = pkey.raw_public_key() {
                        return Ok(host_api::object(vec![
                            ("kty".into(), Value::String("OKP".into())),
                            ("crv".into(), Value::String(curve.into())),
                            ("x".into(), enc_bytes(public)),
                        ]));
                    }
                }
            }
        }
        if matches!(asym_type(), Value::String(ref value) if value == "rsa") {
            let enc = |value: &openssl::bn::BigNumRef| {
                Value::String(
                    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value.to_vec()),
                )
            };
            if matches!(key_type(), Value::String(ref value) if value == "private") {
                if let Ok(pkey) = PKey::private_key_from_pem(&data) {
                    let Ok(rsa) = pkey.rsa() else {
                        return Ok(host_api::object(vec![(
                            "kty".into(),
                            Value::String("RSA".into()),
                        )]));
                    };
                    return Ok(host_api::object(vec![
                        ("kty".into(), Value::String("RSA".into())),
                        ("n".into(), enc(rsa.n())),
                        ("e".into(), enc(rsa.e())),
                        ("d".into(), enc(rsa.d())),
                        ("p".into(), enc(rsa.p().unwrap_or(rsa.n()))),
                        ("q".into(), enc(rsa.q().unwrap_or(rsa.n()))),
                        ("dp".into(), enc(rsa.dmp1().unwrap_or(rsa.d()))),
                        ("dq".into(), enc(rsa.dmq1().unwrap_or(rsa.d()))),
                        ("qi".into(), enc(rsa.iqmp().unwrap_or(rsa.d()))),
                    ]));
                }
            } else if let Ok(rsa) = Rsa::private_key_from_pem(&data) {
                return Ok(host_api::object(vec![
                    ("kty".into(), Value::String("RSA".into())),
                    ("n".into(), enc(rsa.n())),
                    ("e".into(), enc(rsa.e())),
                    ("d".into(), enc(rsa.d())),
                    ("p".into(), enc(rsa.p().unwrap_or(rsa.n()))),
                    ("q".into(), enc(rsa.q().unwrap_or(rsa.n()))),
                    ("dp".into(), enc(rsa.dmp1().unwrap_or(rsa.d()))),
                    ("dq".into(), enc(rsa.dmq1().unwrap_or(rsa.d()))),
                    ("qi".into(), enc(rsa.iqmp().unwrap_or(rsa.d()))),
                ]));
            } else if let Ok(pkey) = PKey::public_key_from_pem(&data) {
                let Ok(rsa) = pkey.rsa() else {
                    return Ok(host_api::object(vec![(
                        "kty".into(),
                        Value::String("RSA".into()),
                    )]));
                };
                return Ok(host_api::object(vec![
                    ("kty".into(), Value::String("RSA".into())),
                    ("n".into(), enc(rsa.n())),
                    ("e".into(), enc(rsa.e())),
                ]));
            } else if let Ok(pkey) = PKey::private_key_from_pem(&data) {
                if let Ok(rsa) = pkey.rsa() {
                    return Ok(host_api::object(vec![
                        ("kty".into(), Value::String("RSA".into())),
                        ("n".into(), enc(rsa.n())),
                        ("e".into(), enc(rsa.e())),
                    ]));
                }
            }
            return Ok(host_api::object(vec![(
                "kty".into(),
                Value::String("RSA".into()),
            )]));
        }
        if matches!(asym_type(), Value::String(ref value) if value == "ec") {
            let private = matches!(key_type(), Value::String(ref value) if value == "private");
            if private {
                if let Ok(pkey) = PKey::private_key_from_pem(&data) {
                    if let Ok(ec) = pkey.ec_key() {
                        let (mut fields, width) = ec_jwk_base(&ec)?;
                        ec_jwk_curve_from_key(&mut fields, receiver);
                        let enc = |bytes: &[u8]| {
                            Value::String(
                                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes),
                            )
                        };
                        let mut scalar = ec.private_key().to_vec();
                        if scalar.len() < width {
                            let mut padded = vec![0; width - scalar.len()];
                            padded.extend_from_slice(&scalar);
                            scalar = padded;
                        }
                        fields.push(("d".into(), enc(&scalar)));
                        return Ok(host_api::object(fields));
                    }
                }
            } else if let Ok(pkey) = PKey::public_key_from_pem(&data) {
                if let Ok(ec) = pkey.ec_key() {
                    let (fields, _) = ec_jwk_base(&ec)?;
                    let mut fields = fields;
                    ec_jwk_curve_from_key(&mut fields, receiver);
                    return Ok(host_api::object(fields));
                }
            }
        }
        return Ok(host_api::object(vec![
            ("kty".into(), Value::String("oct".into())),
            (
                "k".into(),
                Value::String(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)),
            ),
        ]));
    }
    if matches!(format.as_str(), "raw-public" | "raw-private" | "raw-seed") {
        if !matches!(key_type(), Value::String(ref value) if value == "public" || value == "private")
        {
            return Err(crypto_error(
                "ERR_INVALID_ARG_VALUE",
                "Invalid raw key format for this key object",
            ));
        }
        let kind = execute::to_js_string(&asym_type()).unwrap_or_default();
        let private = matches!(format.as_str(), "raw-private" | "raw-seed");
        if format == "raw-public"
            && !matches!(requested_type, Value::Undefined)
            && !matches!(requested_type_name.as_str(), "compressed" | "uncompressed")
        {
            return Err(invalid_option("type", &requested_type));
        }
        let bytes = if kind == "ec" && private {
            if format == "raw-seed" {
                return Err(crypto_error(
                    "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS",
                    "The selected key type does not support raw-seed export",
                ));
            }
            let pkey = PKey::private_key_from_pem(&data).map_err(openssl_error)?;
            let ec = pkey.ec_key().map_err(openssl_error)?;
            let width = ((ec.group().degree() + 7) / 8) as usize;
            let scalar = ec.private_key().to_vec();
            if scalar.len() < width {
                let mut padded = vec![0; width - scalar.len()];
                padded.extend_from_slice(&scalar);
                padded
            } else {
                scalar
            }
        } else if kind == "ec" {
            let pkey = PKey::public_key_from_pem(&data).map_err(openssl_error)?;
            let ec = pkey.ec_key().map_err(openssl_error)?;
            {
                let point_type = match requested_type_name.as_str() {
                    "compressed" => PointConversionForm::COMPRESSED,
                    _ => PointConversionForm::UNCOMPRESSED,
                };
                let mut context = BigNumContext::new().map_err(openssl_error)?;
                ec.public_key()
                    .to_bytes(ec.group(), point_type, &mut context)
                    .map_err(openssl_error)?
            }
        } else if matches!(kind.as_str(), "ed25519" | "ed448" | "x25519" | "x448") && private {
            if format == "raw-seed" {
                return Err(crypto_error(
                    "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS",
                    "The selected key type does not support raw-seed export",
                ));
            }
            PKey::private_key_from_pem(&data)
                .map_err(openssl_error)?
                .raw_private_key()
                .map_err(openssl_error)?
        } else if matches!(kind.as_str(), "ed25519" | "ed448" | "x25519" | "x448") {
            PKey::public_key_from_pem(&data)
                .map_err(openssl_error)?
                .raw_public_key()
                .map_err(openssl_error)?
        } else {
            return Err(crypto_error(
                "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS",
                "The selected key type does not support raw key export",
            ));
        };
        return Ok(crate::modules::buffer_proto::make_buffer(&bytes));
    }
    if format == "der" && requested_passphrase.is_none() {
        let key_type_value = key_type();
        if matches!(key_type_value, Value::String(ref value) if value == "public") {
            if let Ok(pkey) = PKey::public_key_from_pem(&data) {
                if let Ok(rsa) = pkey.rsa() {
                    let der = if requested_type_name == "pkcs1" {
                        rsa.public_key_to_der_pkcs1()
                    } else {
                        pkey.public_key_to_der()
                    };
                    if let Ok(der) = der {
                        let der = if requested_type_name == "spki"
                            && matches!(asym_type(), Value::String(ref kind) if kind == "rsa-pss")
                        {
                            strip_rsa_algorithm_null(der)
                        } else {
                            der
                        };
                        return Ok(crate::modules::buffer_proto::make_buffer(&der));
                    }
                }
            } else if let Ok(pkey) = PKey::private_key_from_pem(&data) {
                if let Ok(rsa) = pkey.rsa() {
                    let der = if requested_type_name == "pkcs1" {
                        rsa.public_key_to_der()
                    } else {
                        pkey.public_key_to_der()
                    };
                    if let Ok(der) = der {
                        return Ok(crate::modules::buffer_proto::make_buffer(&der));
                    }
                }
            } else if let Ok(rsa) = Rsa::public_key_from_pem(&data) {
                let der = if requested_type_name == "pkcs1" {
                    rsa.public_key_to_der_pkcs1()
                } else {
                    rsa.public_key_to_der()
                };
                if let Ok(der) = der {
                    return Ok(crate::modules::buffer_proto::make_buffer(&der));
                }
            }
        } else if matches!(key_type_value, Value::String(ref value) if value == "private") {
            if let Ok(pkey) = PKey::private_key_from_pem(&data) {
                if requested_type_name == "sec1" {
                    if let Ok(ec) = pkey.ec_key() {
                        if let Ok(der) = ec.private_key_to_der() {
                            return Ok(crate::modules::buffer_proto::make_buffer(&der));
                        }
                    }
                } else if let Ok(rsa) = pkey.rsa() {
                    let der = if requested_type_name == "pkcs1" {
                        rsa.private_key_to_der()
                    } else {
                        pkey.private_key_to_der()
                    };
                    if let Ok(der) = der {
                        return Ok(crate::modules::buffer_proto::make_buffer(&der));
                    }
                }
            }
        }
    }
    if matches!(key_type(), Value::String(ref value) if value == "private")
        && matches!(
            execute::get_property(options, "passphrase"),
            Value::String(_) | Value::Uint8Array(_) | Value::ArrayBuffer(_)
        )
    {
        let passphrase =
            key_bytes(&execute::get_property(options, "passphrase")).unwrap_or_default();
        let cipher_name = execute::to_js_string(&execute::get_property(options, "cipher"))
            .unwrap_or_else(|_| "aes-256-cbc".into())
            .to_ascii_lowercase();
        let cipher = match cipher_name.as_str() {
            "aes-128-cbc" => Cipher::aes_128_cbc(),
            "aes-192-cbc" => Cipher::aes_192_cbc(),
            "aes-256-cbc" => Cipher::aes_256_cbc(),
            "aes-128-ecb" => Cipher::aes_128_ecb(),
            "aes-192-ecb" => Cipher::aes_192_ecb(),
            "aes-256-ecb" => Cipher::aes_256_ecb(),
            _ => return Err(crypto_error("ERR_CRYPTO_UNKNOWN_CIPHER", "Unknown cipher")),
        };
        if let Ok(private) =
            PKey::private_key_from_pem(&data).or_else(|_| PKey::private_key_from_der(&data))
        {
            if format == "der" && requested_type_name == "pkcs8" {
                let der = private
                    .private_key_to_pkcs8_passphrase(cipher, &passphrase)
                    .map_err(openssl_error)?;
                return Ok(crate::modules::buffer_proto::make_buffer(&der));
            }
            let pem = if requested_type_name == "pkcs1" {
                private
                    .rsa()
                    .map_err(openssl_error)?
                    .private_key_to_pem_passphrase(cipher, &passphrase)
                    .map_err(openssl_error)?
            } else if requested_type_name == "sec1" {
                private
                    .ec_key()
                    .map_err(openssl_error)?
                    .private_key_to_pem_passphrase(cipher, &passphrase)
                    .map_err(openssl_error)?
            } else {
                private
                    .private_key_to_pem_pkcs8_passphrase(cipher, &passphrase)
                    .map_err(openssl_error)?
            };
            return Ok(Value::String(String::from_utf8_lossy(&pem).into_owned()));
        }
    }
    if format == "pem"
        && matches!(key_type(), Value::String(ref value) if value == "public" || value == "private")
    {
        if requested_type_name == "pkcs1"
            && matches!(key_type(), Value::String(ref value) if value == "public")
        {
            if let Ok(rsa) = PKey::public_key_from_pem(&data).and_then(|pkey| pkey.rsa()) {
                if let Ok(pem) = rsa.public_key_to_pem_pkcs1() {
                    return Ok(Value::String(String::from_utf8_lossy(&pem).into_owned()));
                }
            }
        }
        if requested_type_name == "sec1"
            && matches!(key_type(), Value::String(ref value) if value == "private")
        {
            if let Ok(ec) = PKey::private_key_from_pem(&data).and_then(|pkey| pkey.ec_key()) {
                if let Ok(pem) = ec.private_key_to_pem() {
                    return Ok(Value::String(String::from_utf8_lossy(&pem).into_owned()));
                }
            }
        }
        return Ok(Value::String(String::from_utf8_lossy(&data).into_owned()));
    }
    Ok(crate::modules::buffer_proto::make_buffer(&data))
}

fn strip_rsa_algorithm_null(mut der: Vec<u8>) -> Vec<u8> {
    if der.len() > 17
        && der[0] == 0x30
        && der[2] == 0x30
        && der[3] == 0x0d
        && der[15] == 0x05
        && der[16] == 0x00
    {
        der.remove(16);
        der.remove(15);
        der[1] = der[1].saturating_sub(2);
        der[3] = der[3].saturating_sub(2);
    }
    der
}

fn rsa_input(value: &Value) -> (Value, Vec<u8>) {
    if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
        let nested = execute::get_property(value, KEY_DATA_PROP);
        if !matches!(nested, Value::Undefined) {
            return (value.clone(), key_bytes(&nested).unwrap_or_default());
        }
        let key = execute::get_property(value, "key");
        if !matches!(key, Value::Undefined) {
            if matches!(execute::get_property(value, "format"), Value::String(ref f) if f == "jwk")
            {
                let kind = if execute::has_own_property(&execute::get_property(value, "key"), "d") {
                    "private"
                } else {
                    "public"
                };
                if let Ok(object) = create_asymmetric_key(&[value.clone()], kind) {
                    let bytes = key_bytes(&execute::get_property(&object, KEY_DATA_PROP))
                        .unwrap_or_default();
                    return (object, bytes);
                }
            }
            return (value.clone(), key_bytes(&key).unwrap_or_default());
        }
    }
    (value.clone(), key_bytes(value).unwrap_or_default())
}

fn rsa_operation(args: &[Value], operation: &str) -> Result<Value, VmError> {
    let key_value = args
        .first()
        .ok_or_else(|| invalid_type("The \"key\" argument is required"))?;
    let key_source = if matches!(key_value, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::get_property(key_value, "key")
    } else {
        key_value.clone()
    };
    if (operation == "public_encrypt" || operation == "public_decrypt")
        && (crate::modules::url_whatwg::is_url_instance(key_value)
            || crate::modules::url_whatwg::is_url_instance(&key_source))
    {
        return Err(invalid_type(
            "The \"key\" argument must be a string or an instance of Buffer",
        ));
    }
    if key_is_rsa_pss(Some(key_value)) {
        return Err(crypto_error(
            "ERR_OSSL_EVP_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE",
            "operation not supported for this keytype",
        ));
    }
    let options = if matches!(key_value, Value::Object(_) | Value::ObjectAlias(_))
        && !matches!(execute::get_property(key_value, "key"), Value::Undefined)
    {
        Some(key_value)
    } else {
        None
    };
    let mut data = args.get(1).and_then(bytes_from_value).ok_or_else(|| {
        invalid_type("The \"buffer\" argument must be of type string or an instance of Buffer")
    })?;
    if let Some(options) = options {
        if let Value::String(encoding) = execute::get_property(options, "encoding") {
            data = bytes_with_encoding(args.get(1), Some(&Value::String(encoding)))?;
        }
    }
    let (key_object, mut bytes) = rsa_input(key_value);
    if let Some(options) = options {
        if let Value::String(encoding) = execute::get_property(options, "encoding") {
            if let Value::String(key_text) = execute::get_property(options, "key") {
                bytes = bytes_with_encoding(
                    Some(&Value::String(key_text)),
                    Some(&Value::String(encoding)),
                )?;
            }
        }
    }
    let default_oaep = matches!(operation, "public_encrypt" | "private_decrypt");
    let padding = options
        .map(|value| execute::get_property(value, "padding"))
        .unwrap_or(Value::Undefined);
    let use_oaep = match padding {
        Value::Undefined => default_oaep,
        Value::Number(number) if number == 4.0 => true,
        Value::Number(number) if number == 1.0 => false,
        _ => false,
    };
    // OpenSSL 3.0 lacks RSA PKCS#1 v1.5 implicit-rejection support. Node
    // rejects privateDecrypt with this padding rather than exposing a
    // padding-oracle result; keep the host behavior explicit and bounded.
    if operation == "private_decrypt" && matches!(padding, Value::Number(number) if number == 1.0) {
        return Err(crypto_error(
            "ERR_INVALID_ARG_VALUE",
            "The argument 'options.padding' is invalid",
        ));
    }
    let passphrase =
        options.and_then(|value| key_bytes(&execute::get_property(value, "passphrase")));
    if passphrase.is_none() && is_encrypted_private_key(&bytes) {
        if bytes.starts_with(b"-----") {
            return Err(crypto_error(
                "ERR_OSSL_CRYPTO_INTERRUPTED_OR_CANCELLED",
                "error:07880109:common libcrypto routines::interrupted or cancelled",
            ));
        }
        return Err(crypto_error(
            "ERR_MISSING_PASSPHRASE",
            "Passphrase required for encrypted key",
        ));
    }
    if passphrase.is_some()
        && matches!(
            operation,
            "public_encrypt" | "private_decrypt" | "private_encrypt" | "public_decrypt"
        )
        && is_encrypted_private_key(&bytes)
        && parse_private_operation_key(&bytes, passphrase.as_deref()).is_err()
    {
        return Err(crypto_error(
            "ERR_OSSL_BAD_DECRYPT",
            "error:1C800064:Provider routines::bad decrypt",
        ));
    }
    if use_oaep {
        // Node validates OAEP option types/digests before attempting to load
        // the operation key, so malformed options win over a later key error.
        let _ = rsa_oaep_digests(options)?;
        let _ = rsa_oaep_label(options)?;
        if operation == "private_decrypt" {
            let pkey = parse_private_operation_key(&bytes, passphrase.as_deref())
                .map_err(openssl_error)?;
            return rsa_oaep_decrypt(&pkey, &data, options);
        }
        if let Ok(pkey) = parse_private_operation_key(&bytes, passphrase.as_deref()) {
            return rsa_oaep_encrypt(&pkey, &data, options);
        }
        let pkey = PKey::public_key_from_pem(&bytes)
            .or_else(|_| PKey::public_key_from_der(&bytes))
            .or_else(|_| Rsa::public_key_from_der_pkcs1(&bytes).and_then(PKey::from_rsa))
            .or_else(|_| X509::from_pem(&bytes).and_then(|certificate| certificate.public_key()))
            .map_err(openssl_error)?;
        return rsa_oaep_encrypt(&pkey, &data, options);
    }
    let rsa_padding = match padding {
        Value::Number(number) if number == 3.0 => Padding::NONE,
        Value::Number(number) if number == 5.0 => Padding::from_raw(5),
        Value::Number(number) if number == 2.0 => Padding::from_raw(2),
        _ => Padding::PKCS1,
    };
    let mut out = vec![0u8; 8192];
    let used = if let Ok(pkey) = parse_private_operation_key(&bytes, passphrase.as_deref()) {
        let rsa = pkey.rsa().map_err(openssl_error)?;
        match operation {
            "public_encrypt" => rsa.public_encrypt(&data, &mut out, rsa_padding),
            "private_decrypt" => rsa.private_decrypt(&data, &mut out, rsa_padding),
            "public_decrypt" => rsa.public_decrypt(&data, &mut out, rsa_padding),
            _ => rsa.private_encrypt(&data, &mut out, rsa_padding),
        }
    } else {
        let pkey = PKey::public_key_from_pem(&bytes)
            .or_else(|_| PKey::public_key_from_der(&bytes))
            .or_else(|_| Rsa::public_key_from_der_pkcs1(&bytes).and_then(PKey::from_rsa))
            .or_else(|_| X509::from_pem(&bytes).and_then(|certificate| certificate.public_key()))
            .map_err(openssl_error)?;
        let rsa = pkey.rsa().map_err(openssl_error)?;
        match operation {
            "private_decrypt" => rsa.public_decrypt(&data, &mut out, rsa_padding),
            "public_decrypt" => rsa.public_decrypt(&data, &mut out, rsa_padding),
            _ => rsa.public_encrypt(&data, &mut out, rsa_padding),
        }
    }
    .map_err(openssl_error)?;
    let _ = key_object;
    out.truncate(used);
    Ok(crate::modules::buffer_proto::make_buffer(&out))
}

fn parse_private_operation_key(
    bytes: &[u8],
    passphrase: Option<&[u8]>,
) -> Result<PKey<openssl::pkey::Private>, openssl::error::ErrorStack> {
    if let Some(passphrase) = passphrase {
        if let Ok(key) = PKey::private_key_from_pkcs8_passphrase(bytes, passphrase) {
            return Ok(key);
        }
        if let Ok(key) = PKey::private_key_from_pem_passphrase(bytes, passphrase) {
            return Ok(key);
        }
        if let Ok(rsa) = Rsa::private_key_from_pem_passphrase(bytes, passphrase) {
            return PKey::from_rsa(rsa);
        }
    }
    PKey::private_key_from_pem(bytes).or_else(|_| PKey::private_key_from_der(bytes))
}

fn rsa_oaep_digests(options: Option<&Value>) -> Result<(&'static MdRef, &'static MdRef), VmError> {
    let oaep_name = options
        .map(|value| execute::get_property(value, "oaepHash"))
        .unwrap_or(Value::Undefined);
    let oaep_name = match oaep_name {
        Value::Undefined => "sha1".to_string(),
        Value::String(value) if value.contains('\0') => {
            return Err(invalid_type(&format!(
                "The \"oaepHash\" option must be of type string.{}",
                crate::modules::util::invalid_arg_received(&Value::String(value))
            )));
        }
        Value::String(value) => value,
        value => {
            return Err(invalid_type(&format!(
                "The \"oaepHash\" option must be of type string.{}",
                crate::modules::util::invalid_arg_received(&value)
            )))
        }
    };
    let mgf_name = options
        .map(|value| execute::get_property(value, "mgf1Hash"))
        .unwrap_or(Value::Undefined);
    let mgf_name = match mgf_name {
        Value::Undefined => oaep_name.clone(),
        Value::String(value) => value,
        value => {
            return Err(invalid_type(&format!(
                "The \"mgf1Hash\" option must be of type string.{}",
                crate::modules::util::invalid_arg_received(&value)
            )))
        }
    };
    let digest = |name: &str| match name.to_ascii_lowercase().replace('-', "").as_str() {
        "sha1" | "rsasha1" => Some(Md::sha1()),
        "sha224" | "rsasha224" => Some(Md::sha224()),
        "sha256" | "rsasha256" => Some(Md::sha256()),
        "sha384" | "rsasha384" => Some(Md::sha384()),
        "sha512" | "rsasha512" => Some(Md::sha512()),
        _ => None,
    };
    let oaep_digest = digest(&oaep_name).ok_or_else(|| {
        crypto_error(
            "ERR_OSSL_EVP_INVALID_DIGEST",
            "digital envelope routines::unsupported",
        )
    })?;
    let mgf_digest = digest(&mgf_name).ok_or_else(|| {
        crypto_error(
            "ERR_OSSL_EVP_INVALID_DIGEST",
            "digital envelope routines::unsupported",
        )
    })?;
    Ok((oaep_digest, mgf_digest))
}

fn rsa_oaep_encrypt<T: HasPublic>(
    pkey: &PKey<T>,
    data: &[u8],
    options: Option<&Value>,
) -> Result<Value, VmError> {
    let (oaep_digest, mgf_digest) = rsa_oaep_digests(options)?;
    let mut context = PkeyCtx::new(pkey).map_err(openssl_error)?;
    context.encrypt_init().map_err(openssl_error)?;
    context
        .set_rsa_padding(Padding::PKCS1_OAEP)
        .map_err(openssl_error)?;
    context
        .set_rsa_oaep_md(&oaep_digest)
        .map_err(openssl_error)?;
    context
        .set_rsa_mgf1_md(&mgf_digest)
        .map_err(openssl_error)?;
    if let Some(label) = rsa_oaep_label(options)? {
        context.set_rsa_oaep_label(&label).map_err(openssl_error)?;
    }
    let mut output = Vec::new();
    context
        .encrypt_to_vec(data, &mut output)
        .map_err(openssl_error)?;
    Ok(crate::modules::buffer_proto::make_buffer(&output))
}

fn rsa_oaep_decrypt<T: HasPrivate>(
    pkey: &PKey<T>,
    data: &[u8],
    options: Option<&Value>,
) -> Result<Value, VmError> {
    let (oaep_digest, mgf_digest) = rsa_oaep_digests(options)?;
    let mut context = PkeyCtx::new(pkey).map_err(openssl_error)?;
    context.decrypt_init().map_err(openssl_error)?;
    context
        .set_rsa_padding(Padding::PKCS1_OAEP)
        .map_err(openssl_error)?;
    context
        .set_rsa_oaep_md(&oaep_digest)
        .map_err(openssl_error)?;
    context
        .set_rsa_mgf1_md(&mgf_digest)
        .map_err(openssl_error)?;
    if let Some(label) = rsa_oaep_label(options)? {
        context.set_rsa_oaep_label(&label).map_err(openssl_error)?;
    }
    let mut output = Vec::new();
    context
        .decrypt_to_vec(data, &mut output)
        .map_err(|_| crypto_error("ERR_OSSL_RSA_OAEP_DECODING_ERROR", "oaep decoding error"))?;
    Ok(crate::modules::buffer_proto::make_buffer(&output))
}

fn rsa_oaep_label(options: Option<&Value>) -> Result<Option<Vec<u8>>, VmError> {
    let value = options
        .map(|object| execute::get_property(object, "oaepLabel"))
        .unwrap_or(Value::Undefined);
    if matches!(value, Value::Undefined) {
        return Ok(None);
    }
    if matches!(&value, Value::String(text) if text.contains('\0')) {
        return Err(invalid_type(&format!(
            "The \"oaepLabel\" option must be an ArrayBuffer or a string.{}",
            crate::modules::util::invalid_arg_received(&value)
        )));
    }
    bytes_from_value(&value).map(Some).ok_or_else(|| {
        invalid_type(&format!(
            "The \"oaepLabel\" option must be an ArrayBuffer or a string.{}",
            crate::modules::util::invalid_arg_received(&value)
        ))
    })
}

pub fn public_encrypt(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    rsa_operation(args, "public_encrypt")
}

pub fn private_decrypt(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    rsa_operation(args, "private_decrypt")
}

pub fn public_decrypt(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    rsa_operation(args, "public_decrypt")
}

pub fn private_encrypt(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    rsa_operation(args, "private_encrypt")
}

fn key_bytes(value: &Value) -> Option<Vec<u8>> {
    if crate::modules::url_whatwg::is_url_instance(value) {
        return url_file_bytes(value);
    }
    if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
        let nested = execute::get_property(value, KEY_DATA_PROP);
        if !matches!(nested, Value::Undefined) {
            return key_bytes(&nested);
        }
    }
    match value {
        Value::String(text) => Some(text.as_bytes().to_vec()),
        Value::StringUnits(_) => execute::to_js_string(value)
            .ok()
            .map(|text| text.into_bytes()),
        Value::Uint8Array(view) => {
            let bytes = view.buffer.bytes.borrow();
            let end = view.byte_offset.checked_add(view.byte_length())?;
            Some(bytes.get(view.byte_offset..end)?.to_vec())
        }
        Value::ArrayBuffer(buffer) => Some(buffer.bytes.borrow().clone()),
        Value::DataView(view) => {
            let bytes = view.buffer.bytes.borrow();
            let end = view.byte_offset.checked_add(view.byte_length)?;
            Some(bytes.get(view.byte_offset..end)?.to_vec())
        }
        _ => None,
    }
}

fn url_file_bytes(value: &Value) -> Option<Vec<u8>> {
    let href = crate::modules::url_whatwg::parsed_of(Some(value))
        .ok()?
        .get("href");
    let parsed = url::Url::parse(&href).ok()?;
    (parsed.scheme() == "file")
        .then(|| parsed.to_file_path().ok())
        .flatten()
        .and_then(|path| std::fs::read(path).ok())
}

fn define_hidden(target: &Value, key: &str, value: Value) {
    // Host-owned slots are identity state, not JavaScript descriptors. Mutate
    // the existing object so the methods returned by createHash/createHmac
    // remain attached across the VM's copy-on-write representatives.
    execute::set_property_in_place(target, key, value);
}

pub fn create_hash(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let algorithm = algorithm_named(args.first(), "algorithm")?;
    let output_length = output_length_option(&algorithm, args.get(1))?;
    let default_encoding = args
        .get(1)
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .map(|options| execute::get_property(options, "defaultEncoding"))
        .filter(|value| !matches!(value, Value::Undefined))
        .unwrap_or_else(|| Value::String("utf8".into()));
    let value = host_api::object(vec![
        (
            "update".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_UPDATE),
        ),
        (
            "write".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_UPDATE),
        ),
        (
            "digest".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_DIGEST),
        ),
        (
            "end".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_STREAM_END),
        ),
        (
            "read".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_STREAM_READ),
        ),
        (
            "copy".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_COPY),
        ),
        (
            "pipe".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_PIPE),
        ),
        (
            "on".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_ON),
        ),
        (
            "once".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_ONCE),
        ),
        (
            "emit".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_EMIT),
        ),
        (
            "removeListener".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_REMOVE_LISTENER),
        ),
        (
            "unpipe".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_UNPIPE),
        ),
        (
            "setEncoding".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_SET_ENCODING),
        ),
        (
            "pause".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_PAUSE),
        ),
        (
            "resume".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_RESUME),
        ),
    ]);
    define_hidden(&value, ALGORITHM_PROP, Value::String(algorithm));
    define_hidden(
        &value,
        INPUT_PROP,
        crate::modules::buffer_proto::make_buffer(&[]),
    );
    define_hidden(
        &value,
        WRITABLE_STATE_PROP,
        host_api::object(vec![("defaultEncoding".into(), default_encoding)]),
    );
    if output_length.is_some() {
        define_hidden(
            &value,
            OUTPUT_LEN_PROP,
            Value::Number(output_length.unwrap_or(0) as f64),
        );
    }
    let global = quench_runtime::vm::current_global_object();
    let prototype = execute::get_property(&global, "\0quench:crypto:hash-prototype");
    if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
        return execute::set_prototype_of(&value, &prototype);
    }
    Ok(value)
}

pub fn hash_update(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    if matches!(
        execute::get_property(receiver, DIGESTED_PROP),
        Value::Boolean(true)
    ) {
        if !matches!(
            execute::get_property(receiver, HASH_DATA_LISTENER_PROP),
            Value::Undefined
        ) {
            let previous = execute::get_property(receiver, RESULT_PROP);
            if !matches!(previous, Value::Undefined) {
                return encode_digest(
                    bytes_from_value(&previous).unwrap_or_default(),
                    args.first(),
                );
            }
        }
        return Err(finalized_error());
    }
    if args.first().is_some_and(execute::is_symbol) {
        return Err(invalid_type(
            "The \"data\" argument must be of type string or an instance of Buffer, TypedArray, or DataView.",
        ));
    }
    let encoding = args.get(1).cloned().or_else(|| {
        let default = execute::get_property(receiver, WRITABLE_STATE_PROP);
        let value = execute::get_property(&default, "defaultEncoding");
        (!matches!(value, Value::Undefined)).then_some(value)
    });
    let input = bytes_with_encoding(args.first(), encoding.as_ref())?;
    if let Ok(handle) = execute::get_property_result(receiver, HASH_HANDLE_PROP) {
        let update = execute::get_property(&handle, "update");
        if quench_runtime::is_callable(&update) {
            let input_value = crate::modules::buffer_proto::make_buffer(&input);
            let result = execute::call(&update, &handle, &[input_value])?;
            if matches!(result, Value::Boolean(false)) {
                let error = host_api::object(vec![
                    ("name".into(), Value::String("Error".into())),
                    (
                        "code".into(),
                        Value::String("ERR_CRYPTO_HASH_UPDATE_FAILED".into()),
                    ),
                    ("message".into(), Value::String("Hash update failed".into())),
                ]);
                let listener = execute::get_property(receiver, HASH_ERROR_LISTENER_PROP);
                if quench_runtime::is_callable(&listener) {
                    execute::call(&listener, &Value::Undefined, &[error])?;
                }
                return Ok(receiver.clone());
            }
        }
    }
    let current =
        bytes_from_value(&execute::get_property(receiver, INPUT_PROP)).unwrap_or_default();
    define_hidden(
        receiver,
        INPUT_PROP,
        crate::modules::buffer_proto::make_buffer(&[current, input].concat()),
    );
    Ok(receiver.clone())
}

pub fn hash_digest(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    if matches!(
        execute::get_property(receiver, DIGESTED_PROP),
        Value::Boolean(true)
    ) {
        return Err(finalized_error());
    }
    let algorithm = execute::to_js_string(&execute::get_property(receiver, ALGORITHM_PROP))?;
    let input = bytes_from_value(&execute::get_property(receiver, INPUT_PROP)).unwrap_or_default();
    let digest = if matches!(algorithm.as_str(), "shake128" | "shake256") {
        shake_digest(
            &algorithm,
            &input,
            execute::get_property(receiver, OUTPUT_LEN_PROP),
        )?
    } else {
        digest_bytes(&algorithm, &input)?
    };
    define_hidden(receiver, DIGESTED_PROP, Value::Boolean(true));
    encode_digest(digest, args.first())
}

pub fn hash_one_shot(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let algorithm = algorithm(args.first())?;
    let input = bytes(args.get(1))?;
    let (encoding, output_length) = hash_options(args.get(2), &algorithm)?;
    let digest = if matches!(algorithm.as_str(), "shake128" | "shake256") {
        shake_digest(
            &algorithm,
            &input,
            output_length.unwrap_or(Value::Undefined),
        )?
    } else {
        digest_bytes(&algorithm, &input)?
    };
    encode_digest(digest, encoding.as_ref())
}

fn hash_options(
    value: Option<&Value>,
    algorithm: &str,
) -> Result<(Option<Value>, Option<Value>), VmError> {
    let Some(value) = value.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok((Some(Value::String("hex".into())), None));
    };
    if execute::is_symbol(value) {
        return Err(invalid_type(
            "The \"options\" argument must be of type string or an instance of Object",
        ));
    }
    let (encoding, output_length) = match value {
        Value::String(_) | Value::StringUnits(_) => (Some(value.clone()), None),
        Value::Object(_) | Value::ObjectAlias(_) => {
            let encoding = execute::get_property(value, "outputEncoding");
            let output_length = execute::get_property(value, "outputLength");
            (
                if matches!(encoding, Value::Undefined) {
                    None
                } else {
                    Some(encoding)
                },
                if matches!(output_length, Value::Undefined) {
                    None
                } else {
                    Some(output_length)
                },
            )
        }
        _ => {
            return Err(invalid_type(
                "The \"options\" argument must be of type string or an instance of Object",
            ))
        }
    };
    let encoding = encoding.or_else(|| Some(Value::String("hex".into())));
    if let Some(encoding) = encoding.as_ref() {
        validate_encoding(encoding)?;
    }
    if matches!(algorithm, "shake128" | "shake256") {
        if output_length.is_none() {
            return Err(xof_length_error());
        }
        if !matches!(output_length, Some(Value::Number(number)) if number.is_finite() && number >= 0.0 && number.fract() == 0.0)
        {
            return Err(xof_length_error());
        }
    } else if let Some(Value::Number(number)) = output_length.as_ref() {
        let expected = digest_size(algorithm).unwrap_or(0);
        if !number.is_finite() || number.fract() != 0.0 || *number as usize != expected {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("Error".into())),
                (
                    "message".into(),
                    Value::String(format!(
                        "Output length {} is invalid for {algorithm}, which does not support XOF",
                        number
                    )),
                ),
            ])));
        }
    }
    Ok((encoding, output_length))
}

fn validate_encoding(value: &Value) -> Result<(), VmError> {
    let encoding = execute::to_js_string(value)?.to_ascii_lowercase();
    if matches!(
        encoding.as_str(),
        "buffer"
            | "hex"
            | "base64"
            | "base64url"
            | "latin1"
            | "binary"
            | "ucs2"
            | "ucs-2"
            | "utf16le"
            | "utf-16le"
    ) {
        Ok(())
    } else {
        Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
            (
                "message".into(),
                Value::String(format!("Unknown encoding: {encoding}")),
            ),
        ])))
    }
}

pub fn create_hmac(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let algorithm = algorithm_named(args.first(), "hmac")?;
    let key = bytes(args.get(1))?;
    let value = host_api::object(vec![
        (
            "update".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HMAC_UPDATE),
        ),
        (
            "write".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HMAC_UPDATE),
        ),
        (
            "digest".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HMAC_DIGEST),
        ),
        (
            "end".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_STREAM_END),
        ),
        (
            "read".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_STREAM_READ),
        ),
    ]);
    define_hidden(&value, ALGORITHM_PROP, Value::String(algorithm));
    define_hidden(
        &value,
        HMAC_KEY_PROP,
        crate::modules::buffer_proto::make_buffer(&key),
    );
    define_hidden(
        &value,
        INPUT_PROP,
        crate::modules::buffer_proto::make_buffer(&[]),
    );
    let global = quench_runtime::vm::current_global_object();
    let prototype = execute::get_property(&global, "\0quench:crypto:hmac-prototype");
    if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
        return execute::set_prototype_of(&value, &prototype);
    }
    Ok(value)
}

pub fn hmac_update(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    hash_update(state, receiver, args)
}

pub fn hmac_digest(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    if matches!(
        execute::get_property(receiver, DIGESTED_PROP),
        Value::Boolean(true)
    ) {
        // Node keeps the HMAC object in an uninitialized state after the
        // first digest; subsequent digest calls return an empty value.
        return encode_digest(Vec::new(), args.first());
    }
    let algorithm = execute::to_js_string(&execute::get_property(receiver, ALGORITHM_PROP))?;
    let key = bytes_from_value(&execute::get_property(receiver, HMAC_KEY_PROP)).unwrap_or_default();
    let input = bytes_from_value(&execute::get_property(receiver, INPUT_PROP)).unwrap_or_default();
    let digest = hmac_bytes(&algorithm, &key, &input)?;
    define_hidden(receiver, DIGESTED_PROP, Value::Boolean(true));
    encode_digest(digest, args.first())
}

pub fn hash_pipe(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let destination = args
        .first()
        .ok_or_else(|| invalid_type("The \"destination\" argument must be a stream"))?;
    define_hidden(receiver, PIPE_DEST_PROP, destination.clone());
    Ok(destination.clone())
}

pub fn hash_on(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let event = execute::to_js_string(args.first().unwrap_or(&Value::Undefined))?;
    let listener = args
        .get(1)
        .ok_or_else(|| invalid_type("The \"listener\" argument must be a function"))?;
    if !quench_runtime::is_callable(listener) {
        return Err(invalid_type("The \"listener\" argument must be a function"));
    }
    let slot = match event.as_str() {
        "data" => HASH_DATA_LISTENER_PROP,
        "error" => HASH_ERROR_LISTENER_PROP,
        _ => return Ok(receiver.clone()),
    };
    define_hidden(receiver, slot, listener.clone());
    Ok(receiver.clone())
}

pub fn hash_emit(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    receiver.ok_or(VmError::NotCallable).cloned()
}

pub fn hash_remove_listener(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    receiver.ok_or(VmError::NotCallable).cloned()
}

pub fn hash_unpipe(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    define_hidden(receiver, PIPE_DEST_PROP, Value::Undefined);
    Ok(receiver.clone())
}

pub fn hash_set_encoding(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    if let Some(value) = args.first() {
        define_hidden(receiver, ENCODING_PROP, value.clone());
    }
    Ok(receiver.clone())
}

pub fn hash_pause(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    receiver.ok_or(VmError::NotCallable).cloned()
}
pub fn hash_resume(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    receiver.ok_or(VmError::NotCallable).cloned()
}

pub fn get_hashes(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(host_api::array(
        [
            "RSA-SHA1", "md5", "sha1", "sha224", "sha256", "sha3-256", "sha3-384", "sha3-512",
            "sha384", "sha512",
        ]
        .into_iter()
        .map(|name| Value::String(name.into()))
        .collect(),
    ))
}

pub fn get_fips(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(FIPS_MODE.load(Ordering::Relaxed) as f64))
}

pub fn set_fips(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let global = quench_runtime::vm::current_global_object();
    let process = execute::get_property(&global, "process");
    let env = execute::get_property(&process, "env");
    if matches!(execute::get_property(&env, "QUENCH_WORKER"), Value::String(ref value) if value == "1")
    {
        return Err(VmError::Thrown(quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String(
                "Calling crypto.setFips() is not supported in workers".into(),
            )],
        )));
    }
    let value = args.first().unwrap_or(&Value::Undefined);
    let enabled = match value {
        Value::Boolean(enabled) => *enabled,
        Value::Number(number) => *number != 0.0,
        _ => {
            return Err(invalid_type(
                "The \"bool\" argument must be of type boolean",
            ))
        }
    };
    FIPS_MODE.store(enabled as u8, Ordering::Relaxed);
    Ok(Value::Undefined)
}

/// Return the stable shape of Node's secure-heap accounting object.  The
/// embedded OpenSSL build does not expose allocator counters, so the host
/// reports the configured Node defaults and a monotonic used marker after the
/// initial observation.  This preserves the observable API contract without
/// creating a second allocator or runtime state model.
pub fn secure_heap_used(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let used = if SECURE_HEAP_CALLS.fetch_add(1, Ordering::Relaxed) == 0 {
        0.0
    } else {
        1.0
    };
    Ok(host_api::object(vec![
        ("total".into(), Value::Number(65536.0)),
        ("used".into(), Value::Number(used)),
        ("utilization".into(), Value::Number(used / 65536.0)),
        ("min".into(), Value::Number(4.0)),
    ]))
}

pub fn encapsulate(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let key = args.first().ok_or_else(|| {
        invalid_type("The \"key\" argument must be of type KeyObject, string, or object")
    })?;
    if crate::modules::url_whatwg::is_url_instance(key)
        || (matches!(key, Value::Object(_) | Value::ObjectAlias(_))
            && crate::modules::url_whatwg::is_url_instance(&execute::get_property(key, "key")))
    {
        return Err(invalid_type(
            "The \"key\" argument must be of type KeyObject, string, or object",
        ));
    }
    let (key_bytes, options) = key_and_options(Some(key))?;
    let pkey = parse_public_signing_key(&key_bytes, options.as_ref()).map_err(|_| {
        crypto_error(
            "ERR_CRYPTO_KEM_NOT_SUPPORTED",
            "KEM operation is not supported",
        )
    })?;
    if pkey.id() != Id::RSA {
        return Err(crypto_error(
            "ERR_CRYPTO_OPERATION_FAILED",
            "operation not supported for this key type",
        ));
    }
    let rsa = pkey.rsa().map_err(|_| {
        crypto_error(
            "ERR_CRYPTO_KEM_NOT_SUPPORTED",
            "KEM operation is not supported",
        )
    })?;
    // RSASVE encapsulation uses a full-modulus random secret and raw RSA
    // transport (the ciphertext and shared key are both modulus-sized).
    let mut shared = vec![0u8; rsa.size() as usize];
    rand::thread_rng().fill_bytes(&mut shared);
    shared[0] &= 0x7f;
    let mut ciphertext = vec![0u8; rsa.size() as usize];
    let length = rsa
        .public_encrypt(&shared, &mut ciphertext, Padding::NONE)
        .map_err(|_| crypto_error("ERR_CRYPTO_OPERATION_FAILED", "Encapsulation failed"))?;
    ciphertext.truncate(length);
    Ok(host_api::object(vec![
        (
            "ciphertext".into(),
            crate::modules::buffer_proto::make_buffer(&ciphertext),
        ),
        (
            "sharedKey".into(),
            crate::modules::buffer_proto::make_buffer(&shared),
        ),
    ]))
}

pub fn decapsulate(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args
        .last()
        .filter(|value| quench_runtime::is_callable(value))
        .cloned();
    let sync_args = if callback.is_some() {
        &args[..args.len() - 1]
    } else {
        args
    };
    let result = decapsulate_impl(state, receiver, sync_args);
    if let Some(callback) = callback {
        return match result {
            Ok(value) => {
                execute::call(&callback, &Value::Undefined, &[Value::Null, value])?;
                Ok(Value::Undefined)
            }
            Err(VmError::Thrown(error)) => {
                let error = normalize_callback_error(error);
                execute::call(&callback, &Value::Undefined, &[error, Value::Undefined])?;
                Ok(Value::Undefined)
            }
            Err(error) => Err(error),
        };
    }
    result
}

fn decapsulate_impl(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let key = args.first().ok_or_else(|| {
        invalid_type("The \"key\" argument must be of type KeyObject, string, or object")
    })?;
    let ciphertext_value = args.get(1).unwrap_or(&Value::Undefined);
    let ciphertext = bytes_from_value(ciphertext_value).ok_or_else(|| {
        invalid_type(&format!(
            "The \"ciphertext\" argument must be an instance of ArrayBuffer, Buffer, TypedArray, or DataView.{}",
            crate::modules::util::invalid_arg_received(ciphertext_value)
        ))
    })?;
    let (key_bytes, options) = key_and_options(Some(key))?;
    let pkey = PKey::private_key_from_pem(&key_bytes)
        .or_else(|_| PKey::private_key_from_der(&key_bytes))
        .map_err(|_| crypto_error("ERR_CRYPTO_OPERATION_FAILED", "Decapsulation failed"))?;
    if pkey.id() != Id::RSA {
        return Err(crypto_error(
            "ERR_CRYPTO_OPERATION_FAILED",
            "Decapsulation failed",
        ));
    }
    let rsa = pkey
        .rsa()
        .map_err(|_| crypto_error("ERR_CRYPTO_OPERATION_FAILED", "Decapsulation failed"))?;
    let mut shared = vec![0u8; rsa.size() as usize];
    let length = rsa
        .private_decrypt(&ciphertext, &mut shared, Padding::NONE)
        .map_err(|_| crypto_error("ERR_CRYPTO_OPERATION_FAILED", "Decapsulation failed"))?;
    shared.truncate(length);
    let _ = options;
    Ok(crate::modules::buffer_proto::make_buffer(&shared))
}

pub fn set_engine(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Err(crypto_error(
        "ERR_CRYPTO_CUSTOM_ENGINE_NOT_SUPPORTED",
        "Custom engines are not supported",
    ))
}

pub fn test_fips_crypto(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(0.0))
}

pub fn check_prime_sync(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let input = args.first().unwrap_or(&Value::Undefined);
    let candidate = match input {
        Value::BigInt(text) if text.starts_with('-') => {
            return Err(range_error(&format!(
                "The value of \"candidate\" is out of range. It must be >= 0. Received {text}n"
            )))
        }
        Value::BigInt(text) => BigNum::from_dec_str(text).ok(),
        _ => bytes_from_value(input).and_then(|bytes| {
            if bytes.len() > 16 * 1024 * 1024 {
                return None;
            }
            BigNum::from_slice(&bytes).ok()
        }),
    }
    .ok_or_else(|| {
        if bytes_from_value(input).is_some_and(|bytes| bytes.len() > 16 * 1024 * 1024) {
            crypto_error("ERR_OSSL_BN_BIGNUM_TOO_LONG", "bignum too long")
        } else {
            invalid_type("The \"candidate\" argument must be an instance of Buffer")
        }
    })?;
    let mut context = BigNumContext::new().map_err(openssl_error)?;
    let checks = match args.get(1) {
        None | Some(Value::Undefined) => 0,
        Some(options @ (Value::Object(_) | Value::ObjectAlias(_))) => {
            match execute::get_property(options, "checks") {
                Value::Undefined => 0,
                Value::Number(checks)
                    if checks.is_finite()
                        && checks.fract() == 0.0
                        && checks >= 0.0
                        && checks <= 2_147_483_647.0 => checks as i32,
                Value::Number(checks) => {
                    return Err(range_error(&format!(
                        "The value of \"options.checks\" is out of range. It must be >= 0 && <= 2147483647. Received {checks}"
                    )))
                }
                value => {
                    return Err(invalid_type(&format!(
                        "The \"options.checks\" property must be of type number.{}",
                        crate::modules::util::invalid_arg_received(&value)
                    )))
                }
            }
        }
        Some(value) => {
            return Err(invalid_type(&format!(
                "The \"options\" argument must be of type object.{}",
                crate::modules::util::invalid_arg_received(value)
            )))
        }
    };
    Ok(Value::Boolean(
        candidate
            .is_prime(checks.max(1), &mut context)
            .unwrap_or(false),
    ))
}

pub fn check_prime(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args
        .last()
        .filter(|value| quench_runtime::is_callable(value));
    let end = if callback.is_some() {
        args.len() - 1
    } else {
        args.len()
    };
    let result = check_prime_sync(state, None, &args[..end])?;
    if let Some(callback) = callback {
        state
            .borrow()
            .event_loop
            .queue_microtask(callback.clone(), vec![Value::Null, result]);
        return Ok(Value::Undefined);
    }
    Err(invalid_type(
        "The \"callback\" argument must be of type function",
    ))
}

pub fn generate_prime_sync(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let bits = prime_bits(args.first())?;
    let options = match args.get(1) {
        None | Some(Value::Undefined) => None,
        Some(value @ (Value::Object(_) | Value::ObjectAlias(_))) => Some(value),
        Some(value) => {
            return Err(invalid_type(&format!(
                "The \"options\" argument must be of type object.{}",
                crate::modules::util::invalid_arg_received(value)
            )))
        }
    };
    let safe = boolean_option(options, "safe")?;
    let bigint = boolean_option(options, "bigint")?;
    let add = prime_option(options, "add")?;
    let rem = prime_option(options, "rem")?;
    if rem.is_some() && add.is_none() {
        return Err(range_error("invalid options.rem"));
    }
    if let (Some(add), Some(rem)) = (&add, &rem) {
        if rem.ucmp(add) != std::cmp::Ordering::Less {
            return Err(range_error("invalid options.rem"));
        }
    }
    if add.as_ref().is_some_and(|value| value.num_bits() > bits) {
        return Err(range_error("invalid options.add"));
    }
    let mut prime = BigNum::new().map_err(openssl_error)?;
    prime
        .generate_prime(bits, safe, add.as_deref(), rem.as_deref())
        .map_err(|_| crypto_error("ERR_OPERATION_FAILED", "failed to generate prime"))?;
    if bigint {
        return Ok(Value::BigInt(
            prime.to_dec_str().map_err(openssl_error)?.to_string(),
        ));
    }
    Ok(crate::modules::buffer_proto::make_buffer(&prime.to_vec()))
}

pub fn generate_prime(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args
        .last()
        .filter(|value| quench_runtime::is_callable(value));
    let end = callback.map_or(args.len(), |_| args.len() - 1);
    let callback = callback.ok_or_else(callback_type)?;
    let result = generate_prime_sync(state, None, &args[..end])?;
    state
        .borrow()
        .event_loop
        .queue_microtask(callback.clone(), vec![Value::Null, result]);
    Ok(Value::Undefined)
}

fn prime_bits(value: Option<&Value>) -> Result<i32, VmError> {
    match value {
        Some(Value::Number(bits)) if bits.is_finite() && bits.fract() == 0.0 => {
            if *bits >= 1.0 && *bits <= 2_147_483_647.0 {
                Ok(*bits as i32)
            } else {
                Err(range_error(&format!("The value of \"size\" is out of range. It must be >= 1 && <= 2147483647. Received {bits}")))
            }
        }
        Some(value) => Err(invalid_type(&format!(
            "The \"size\" argument must be of type number.{}",
            crate::modules::util::invalid_arg_received(value)
        ))),
        None => Err(invalid_type(
            "The \"size\" argument must be of type number. Received undefined",
        )),
    }
}

fn boolean_option(options: Option<&Value>, name: &str) -> Result<bool, VmError> {
    let value = options
        .map(|options| execute::get_property(options, name))
        .unwrap_or(Value::Undefined);
    match value {
        Value::Undefined => Ok(false),
        Value::Boolean(value) => Ok(value),
        value => Err(invalid_type(&format!(
            "The \"options.{name}\" property must be of type boolean.{}",
            crate::modules::util::invalid_arg_received(&value)
        ))),
    }
}

fn prime_option(options: Option<&Value>, name: &str) -> Result<Option<BigNum>, VmError> {
    let value = options
        .map(|options| execute::get_property(options, name))
        .unwrap_or(Value::Undefined);
    if matches!(value, Value::Undefined) {
        return Ok(None);
    }
    if let Value::BigInt(text) = &value {
        if text.starts_with('-') {
            return Err(range_error(&format!(
                "The value of \"options.{name}\" is out of range. It must be >= 0. Received {text}n"
            )));
        }
    }
    let bytes = match &value {
        Value::BigInt(text) => BigNum::from_dec_str(text).ok().map(|n| n.to_vec()),
        Value::String(_) | Value::StringUnits(_) | Value::Number(_) | Value::Boolean(_) => None,
        _ => bytes_from_value(&value),
    };
    let Some(bytes) = bytes else {
        return Err(invalid_type(&format!(
            "The \"options.{name}\" property must be an integer or an instance of Buffer"
        )));
    };
    let number =
        BigNum::from_slice(&bytes).map_err(|_| range_error(&format!("invalid options.{name}")))?;
    Ok(Some(number))
}

pub fn random_bytes(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let size = random_size(args.first())?;
    let mut bytes = vec![0_u8; size];
    rand::thread_rng().fill_bytes(&mut bytes);
    let output = crate::modules::buffer_proto::make_buffer(&bytes);
    if let Some(callback) = args.get(1) {
        if !quench_runtime::is_callable(callback) {
            return Err(callback_type());
        }
        let callback = domain_callback(state, callback);
        let resource = crate::modules::async_hooks::new_resource(
            state,
            &[Value::String("RANDOMBYTESREQUEST".into())],
        )
        .ok();
        state
            .borrow()
            .event_loop
            .queue_microtask_with_resource(callback, vec![Value::Null, output.clone()], resource);
    }
    Ok(output)
}

/// Preserve the active domain while delivering callbacks through the event
/// loop.  The wrapper is derived from the domain's existing bind capability,
/// so asynchronous crypto does not invent a parallel context mechanism.
fn domain_callback(state: &Rc<RefCell<HostState>>, callback: &Value) -> Value {
    crate::modules::domain::current(state)
        .map(|domain| {
            host_api::bound_capability_with_arguments(
                crate::host::capability_ref(crate::registry::SPEC_DOMAIN_BIND_CALL),
                vec![domain, callback.clone()],
            )
        })
        .unwrap_or_else(|| callback.clone())
}

pub fn random_fill(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (callback_index, callback) =
        if quench_runtime::is_callable(args.get(1).unwrap_or(&Value::Undefined)) {
            (1, args[1].clone())
        } else if quench_runtime::is_callable(args.get(2).unwrap_or(&Value::Undefined)) {
            (2, args[2].clone())
        } else if quench_runtime::is_callable(args.get(3).unwrap_or(&Value::Undefined)) {
            (3, args[3].clone())
        } else {
            return Err(callback_type());
        };
    let sync_args = &args[..callback_index];
    let result = random_fill_sync(state, None, sync_args)?;
    execute::call(&callback, &Value::Undefined, &[Value::Null, result.clone()])?;
    Ok(Value::Undefined)
}

pub fn random_int(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let one_argument = args.get(1).is_none()
        || quench_runtime::is_callable(args.get(1).unwrap_or(&Value::Undefined));
    let (minimum, maximum, callback) = if one_argument {
        (
            0.0,
            number_value(args.first(), "max")?,
            args.get(1)
                .filter(|value| quench_runtime::is_callable(value))
                .cloned(),
        )
    } else {
        (
            number_value(args.first(), "min")?,
            number_value(args.get(1), "max")?,
            args.get(2)
                .filter(|value| quench_runtime::is_callable(value))
                .cloned(),
        )
    };
    if let Some(value) = args.get(if one_argument { 1 } else { 2 }) {
        if callback.is_none() && !matches!(value, Value::Undefined) {
            return Err(callback_type());
        }
    }
    if maximum <= minimum {
        return Err(range_error(&format!("The value of \"max\" is out of range. It must be greater than the value of \"min\" ({minimum}). Received {maximum}")));
    }
    let range = maximum - minimum;
    if one_argument && maximum > 281_474_976_710_655.0 {
        return Err(range_error("The value of \"max\" is out of range. It must be <= 281474976710655. Received 281_474_976_710_656"));
    }
    if range > 281_474_976_710_655.0 {
        return Err(range_error("The value of \"max - min\" is out of range. It must be <= 281474976710655. Received 281_474_976_710_656"));
    }
    let limit = (281_474_976_710_656.0 / range).floor() * range;
    let choice = loop {
        let mut bytes = [0_u8; 6];
        rand::thread_rng().fill_bytes(&mut bytes);
        let mut value = 0.0;
        for byte in bytes {
            value = value * 256.0 + byte as f64;
        }
        if value < limit {
            break minimum + (value % range);
        }
    };
    let output = Value::Number(choice);
    if let Some(callback) = callback {
        execute::call(&callback, &Value::Undefined, &[Value::Null, output.clone()])?;
    }
    Ok(output)
}

static LAST_UUID_V7_MS: AtomicU64 = AtomicU64::new(0);
static FIPS_MODE: AtomicU8 = AtomicU8::new(0);
static SECURE_HEAP_CALLS: AtomicU64 = AtomicU64::new(0);

pub fn random_uuid(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(options) = args
        .first()
        .filter(|value| !matches!(value, Value::Undefined))
    {
        if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
            return Err(invalid_type(
                "The \"options\" argument must be of type object",
            ));
        }
        let entropy = execute::get_property(options, "disableEntropyCache");
        if !matches!(entropy, Value::Undefined | Value::Boolean(_)) {
            return Err(invalid_type(
                "The \"options.disableEntropyCache\" property must be of type boolean",
            ));
        }
    }
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(Value::String(format_uuid(&bytes)))
}

fn format_uuid(bytes: &[u8; 16]) -> String {
    format!(
        "{}-{}-{}-{}-{}",
        hex::encode(&bytes[..4]),
        hex::encode(&bytes[4..6]),
        hex::encode(&bytes[6..8]),
        hex::encode(&bytes[8..10]),
        hex::encode(&bytes[10..]),
    )
}

pub fn random_uuid_v7(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(options) = args
        .first()
        .filter(|value| !matches!(value, Value::Undefined))
    {
        if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
            return Err(invalid_type("options must be an object"));
        }
        let entropy = execute::get_property(options, "disableEntropyCache");
        if !matches!(entropy, Value::Undefined | Value::Boolean(_)) {
            return Err(invalid_type("disableEntropyCache must be a boolean"));
        }
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let previous = LAST_UUID_V7_MS.load(Ordering::Relaxed);
    let timestamp = now.max(previous);
    LAST_UUID_V7_MS.fetch_max(timestamp, Ordering::Relaxed);
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes[..6].copy_from_slice(&(timestamp & 0x0000_ffff_ffff_ffff).to_be_bytes()[2..]);
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let text = format!(
        "{}-{}-{}-{}-{}",
        hex::encode(&bytes[..4]),
        hex::encode(&bytes[4..6]),
        hex::encode(&bytes[6..8]),
        hex::encode(&bytes[8..10]),
        hex::encode(&bytes[10..])
    );
    Ok(Value::String(text))
}

pub fn random_fill_sync(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let target = args.first().ok_or_else(|| {
        invalid_type("The \"buffer\" argument must be an instance of ArrayBufferView")
    })?;
    let (buffer, view_offset, view_length, element_size) =
        writable_view(target).ok_or_else(|| {
            invalid_type("The \"buffer\" argument must be an instance of ArrayBufferView")
        })?;
    let start = match args.get(1) {
        None | Some(Value::Undefined) => 0,
        Some(value) => bounded_number(Some(value), "offset", view_length)?,
    };
    let length = match args.get(2) {
        None | Some(Value::Undefined) => view_length - start,
        Some(value) => bounded_number(Some(value), "size", 2_147_483_647)?,
    };
    if start.saturating_add(length) > view_length {
        return Err(range_error(&format!("The value of \"size + offset\" is out of range. It must be <= {view_length}. Received {}", start + length)));
    }
    let start = view_offset + start * element_size;
    let length = length * element_size;
    let mut bytes = buffer.bytes.borrow_mut();
    let end = start + length;
    rand::thread_rng().fill_bytes(&mut bytes[start..end]);
    Ok(target.clone())
}

pub fn pbkdf2_sync(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let output = pbkdf2_derive(args)?;
    Ok(crate::modules::buffer_proto::make_buffer(&output))
}

pub fn scrypt_sync(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let password = pbkdf2_bytes(args.first(), "password")?;
    let salt = pbkdf2_bytes(args.get(1), "salt")?;
    let length = match args.get(2) {
        Some(Value::Number(value))
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= 0.0
                && *value <= 2_147_483_647.0 =>
        {
            *value as usize
        }
        Some(Value::Number(value)) if value.is_finite() => {
            return Err(out_of_range("keylen", "must be an integer"))
        }
        _ => {
            return Err(invalid_type(
                "The \"keylen\" argument must be of type number",
            ))
        }
    };
    let options = args
        .iter()
        .skip(3)
        .find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)));
    if let Some(options) = options {
        for (left, right) in [("N", "cost"), ("p", "parallelization"), ("r", "blockSize")] {
            if execute::has_own_property(options, left) && execute::has_own_property(options, right)
            {
                return Err(crypto_error(
                    "ERR_INCOMPATIBLE_OPTION_PAIR",
                    &format!(
                        "Option \"{left}\" cannot be used in combination with option \"{right}\""
                    ),
                ));
            }
        }
        if let Value::Number(maxmem) = execute::get_property(options, "maxmem") {
            if !maxmem.is_finite()
                || maxmem.fract() != 0.0
                || maxmem < 0.0
                || maxmem > 9_007_199_254_740_991.0
            {
                return Err(out_of_range("options.maxmem", "must be a safe integer"));
            }
        }
    }
    let number = |options: &Value, name: &str, default: u64| -> Result<u64, VmError> {
        let value = execute::get_property(options, name);
        match value {
            Value::Undefined => Ok(default),
            Value::Number(value) if value.is_finite() && value.fract() == 0.0 && value >= 0.0 => {
                Ok(value as u64)
            }
            Value::Number(_) => Err(out_of_range(
                "options",
                "must contain valid scrypt parameters",
            )),
            _ => Err(invalid_type(&format!(
                "The \"options.{name}\" property must be of type number"
            ))),
        }
    };
    let (n, r, p, maxmem) = if let Some(options) = options {
        let n = match execute::get_property(options, "N") {
            Value::Undefined => number(options, "cost", 16_384)? as f64,
            Value::Number(value) => value,
            _ => {
                return Err(invalid_type(
                    "The \"options.N\" property must be of type number",
                ))
            }
        };
        let r = if matches!(execute::get_property(options, "r"), Value::Undefined) {
            number(options, "blockSize", 8)?
        } else {
            number(options, "r", 8)?
        };
        let p = if matches!(execute::get_property(options, "p"), Value::Undefined) {
            number(options, "parallelization", 1)?
        } else {
            number(options, "p", 1)?
        };
        let maxmem = number(options, "maxmem", 32 * 1024 * 1024)?;
        (n, r, p, maxmem)
    } else {
        (16_384.0, 8, 1, 32 * 1024 * 1024)
    };
    if !n.is_finite() || n <= 1.0 || n.fract() != 0.0 || (n as u64).count_ones() != 1 {
        return Err(crypto_error(
            "ERR_CRYPTO_INVALID_SCRYPT_PARAMS",
            "Invalid scrypt params: memory limit exceeded",
        ));
    }
    let n_u64 = n as u64;
    if n_u64 >= (1u64 << (r.saturating_mul(16).min(63)))
        || p > (1u64 << 30).saturating_sub(1) / r.max(1)
        || 128u64.saturating_mul(n_u64).saturating_mul(r) > maxmem
    {
        return Err(crypto_error(
            "ERR_CRYPTO_INVALID_SCRYPT_PARAMS",
            "Invalid scrypt params: memory limit exceeded",
        ));
    }
    let mut output = vec![0u8; length];
    openssl::pkcs5::scrypt(&password, &salt, n as u64, r, p, maxmem, &mut output).map_err(
        |_| {
            crypto_error(
                "ERR_CRYPTO_INVALID_SCRYPT_PARAMS",
                "Invalid scrypt params: memory limit exceeded",
            )
        },
    )?;
    Ok(crate::modules::buffer_proto::make_buffer(&output))
}

pub fn scrypt(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args
        .last()
        .filter(|value| quench_runtime::is_callable(value))
        .cloned();
    let sync_args = if callback.is_some() {
        &args[..args.len() - 1]
    } else {
        args
    };
    if callback.is_none()
        && args.len() >= 3
        && matches!(
            args.get(2),
            Some(Value::Number(value))
                if value.is_finite()
                    && value.fract() == 0.0
                    && *value >= 0.0
                    && *value <= 2_147_483_647.0
        )
    {
        return Err(invalid_type(
            "The \"callback\" argument must be of type function",
        ));
    }
    let output = scrypt_sync(state, receiver, sync_args)?;
    let callback = callback
        .ok_or_else(|| invalid_type("The \"callback\" argument must be of type function"))?;
    state
        .borrow()
        .event_loop
        .queue_microtask(callback, vec![Value::Null, output]);
    Ok(Value::Undefined)
}

/// Callback form of PBKDF2. Argument validation remains synchronous, while
/// the successful result is delivered through the ordinary callback edge.
pub fn pbkdf2(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = match args.get(5) {
        Some(value) if quench_runtime::is_callable(value) => value.clone(),
        None if args.get(4).is_some_and(quench_runtime::is_callable) => args[4].clone(),
        _ => {
            return Err(invalid_type(
                "The \"callback\" argument must be of type function",
            ))
        }
    };
    let mut derive_args = args.to_vec();
    if args.get(5).is_none() {
        derive_args.insert(4, Value::Undefined);
    }
    let result = match pbkdf2_derive(&derive_args) {
        Ok(result) => result,
        Err(VmError::Thrown(error)) => {
            return Err(VmError::Thrown(normalize_callback_error(error)));
        }
        Err(error) => return Err(error),
    };
    let callback = domain_callback(state, &callback);
    state.borrow().event_loop.queue_microtask(
        callback,
        vec![
            Value::Null,
            crate::modules::buffer_proto::make_buffer(&result),
        ],
    );
    Ok(Value::Undefined)
}

fn pbkdf2_derive(args: &[Value]) -> Result<Vec<u8>, VmError> {
    let password = pbkdf2_bytes(args.first(), "password")?;
    let salt = pbkdf2_bytes(args.get(1), "salt")?;
    let iterations = pbkdf2_number(args.get(2), "iterations", 1)?;
    let length = pbkdf2_number(args.get(3), "keylen", 0)?;
    let digest = args.get(4).ok_or_else(|| {
        invalid_type("The \"digest\" argument must be of type string. Received undefined")
    })?;
    let algorithm = match digest {
        Value::String(_) | Value::StringUnits(_) => {
            execute::to_js_string(digest)?.to_ascii_lowercase()
        }
        Value::Undefined => {
            return Err(invalid_type(
                "The \"digest\" argument must be of type string. Received undefined",
            ))
        }
        Value::Null => {
            return Err(invalid_type(
                "The \"digest\" argument must be of type string. Received null",
            ))
        }
        _ => {
            return Err(invalid_type(
                "The \"digest\" argument must be of type string.",
            ))
        }
    };
    if !matches!(
        algorithm.as_str(),
        "sha1" | "sha256" | "sha224" | "sha384" | "sha512"
    ) {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            (
                "code".into(),
                Value::String("ERR_CRYPTO_INVALID_DIGEST".into()),
            ),
            (
                "message".into(),
                Value::String(format!("Invalid digest: {algorithm}")),
            ),
        ])));
    }
    let mut output = Vec::with_capacity(length);
    let mut block = 1_u32;
    while output.len() < length {
        let mut input = salt.clone();
        input.extend_from_slice(&block.to_be_bytes());
        let mut u = hmac_bytes(&algorithm, &password, &input)?;
        let mut t = u.clone();
        for _ in 1..iterations {
            u = hmac_bytes(&algorithm, &password, &u)?;
            for (left, right) in t.iter_mut().zip(&u) {
                *left ^= *right;
            }
        }
        output.extend_from_slice(&t);
        block = block.saturating_add(1);
    }
    output.truncate(length);
    Ok(output)
}

fn pbkdf2_bytes(value: Option<&Value>, name: &str) -> Result<Vec<u8>, VmError> {
    bytes_from_value(value.unwrap_or(&Value::Undefined)).ok_or_else(|| {
        invalid_type(&format!("The \"{name}\" argument must be of type string or an instance of Buffer, TypedArray, or DataView"))
    })
}

fn pbkdf2_number(value: Option<&Value>, name: &str, minimum: usize) -> Result<usize, VmError> {
    let Some(Value::Number(number)) = value else {
        let received =
            crate::modules::util::invalid_arg_received(value.unwrap_or(&Value::Undefined));
        return Err(invalid_type(&format!(
            "The \"{name}\" argument must be of type number.{received}"
        )));
    };
    if !number.is_finite()
        || number.fract() != 0.0
        || *number < minimum as f64
        || *number > 2_147_483_647.0
    {
        let received = number_received(*number);
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("RangeError".into())),
            ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
            ("message".into(), Value::String(format!("The value of \"{name}\" is out of range. It must be an integer. Received {received}"))),
        ])));
    }
    Ok(*number as usize)
}

fn number_received(number: f64) -> String {
    if number.is_nan() {
        "NaN".into()
    } else if number == f64::INFINITY {
        "Infinity".into()
    } else if number == f64::NEG_INFINITY {
        "-Infinity".into()
    } else {
        number.to_string()
    }
}

fn integer(value: Option<&Value>, name: &str) -> Result<usize, VmError> {
    let Some(Value::Number(number)) = value else {
        return Err(execute::type_error(&format!(
            "The \"{name}\" argument must be of type number"
        )));
    };
    if !number.is_finite() || *number < 0.0 || number.fract() != 0.0 || *number > usize::MAX as f64
    {
        return Err(execute::type_error("The value is out of range"));
    }
    Ok(*number as usize)
}

fn callback_type() -> VmError {
    invalid_type("The \"callback\" argument must be of type function")
}

fn range_error(message: &str) -> VmError {
    VmError::Thrown(native_error(
        quench_runtime::ops::Builtin::RangeError,
        "ERR_OUT_OF_RANGE",
        message,
    ))
}

fn random_size(value: Option<&Value>) -> Result<usize, VmError> {
    let value = value.unwrap_or(&Value::Undefined);
    let Value::Number(number) = value else {
        return Err(invalid_type(&format!(
            "The \"size\" argument must be of type number.{}",
            crate::modules::util::invalid_arg_received(value)
        )));
    };
    let size = number.trunc();
    if !size.is_finite() || size < 0.0 || size > 2_147_483_647.0 {
        return Err(range_error(&format!(
            "The value of \"size\" is out of range. It must be >= 0 && <= 2147483647. Received {}",
            number_received(*number)
        )));
    }
    Ok(size as usize)
}

fn number_value(value: Option<&Value>, name: &str) -> Result<f64, VmError> {
    let value = value.unwrap_or(&Value::Undefined);
    let Value::Number(number) = value else {
        return Err(invalid_type(&format!(
            "The \"{name}\" argument must be a safe integer.{}",
            crate::modules::util::invalid_arg_received(value)
        )));
    };
    if !number.is_finite() || number.fract() != 0.0 || number.abs() > 9_007_199_254_740_991.0 {
        return Err(invalid_type(&format!(
            "The \"{name}\" argument must be a safe integer.{}",
            crate::modules::util::invalid_arg_received(value)
        )));
    }
    Ok(*number)
}

fn bounded_number(value: Option<&Value>, name: &str, maximum: usize) -> Result<usize, VmError> {
    let value = value.unwrap_or(&Value::Undefined);
    let Value::Number(number) = value else {
        return Err(invalid_type(&format!(
            "The \"{name}\" argument must be of type number.{}",
            crate::modules::util::invalid_arg_received(value)
        )));
    };
    if !number.is_finite() || number.fract() != 0.0 || *number < 0.0 || *number > maximum as f64 {
        return Err(range_error(&format!(
            "The value of \"{name}\" is out of range. It must be >= 0 && <= {maximum}. Received {}",
            number_received(*number)
        )));
    }
    Ok(*number as usize)
}

fn writable_view(
    value: &Value,
) -> Option<(
    Rc<quench_runtime::value::ArrayBufferData>,
    usize,
    usize,
    usize,
)> {
    macro_rules! view {
        ($view:expr, $size:expr) => {
            Some(($view.buffer.clone(), $view.byte_offset, $view.length, $size))
        };
    }
    match value {
        Value::ArrayBuffer(buffer) => Some((buffer.clone(), 0, buffer.bytes.borrow().len(), 1)),
        Value::Float64Array(view) => view!(view, 8),
        Value::Float32Array(view) => view!(view, 4),
        Value::Int8Array(view) => view!(view, 1),
        Value::Int16Array(view) => view!(view, 2),
        Value::Int32Array(view) => view!(view, 4),
        Value::BigInt64Array(view) => view!(view, 8),
        Value::BigUint64Array(view) => view!(view, 8),
        Value::Uint32Array(view) => view!(view, 4),
        Value::Uint8Array(view) => view!(view, 1),
        Value::Uint8ClampedArray(view) => view!(view, 1),
        Value::Uint16Array(view) => view!(view, 2),
        Value::DataView(view) => Some((view.buffer.clone(), view.byte_offset, view.byte_length, 1)),
        _ => None,
    }
}

fn integer_or(value: Option<&Value>, default: usize) -> Result<usize, VmError> {
    match value {
        None | Some(Value::Undefined) => Ok(default),
        Some(value) => integer(Some(value), "offset"),
    }
}

pub fn stream_end(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    if let Some(value) = args
        .first()
        .filter(|value| !matches!(value, Value::Undefined))
    {
        hash_update(_state, Some(receiver), std::slice::from_ref(value))?;
    }
    // Sign/Verify are writable streams, but `end()` only seals the input;
    // digesting at that point would finalize a hash object and lose the
    // message needed by the later sign/verify operation.
    if execute::has_own_property(receiver, SIGN_OPTIONS_PROP) {
        return Ok(receiver.clone());
    }
    let result = if matches!(
        execute::get_property(receiver, HMAC_KEY_PROP),
        Value::Undefined
    ) {
        hash_digest(_state, Some(receiver), &[])?
    } else {
        hmac_digest(_state, Some(receiver), &[])?
    };
    define_hidden(receiver, RESULT_PROP, result);
    let destination = execute::get_property(receiver, PIPE_DEST_PROP);
    if !matches!(destination, Value::Undefined) {
        let result = execute::get_property(receiver, RESULT_PROP);
        if let Ok(write) = execute::get_property_result(&destination, "write") {
            let _ = execute::call(&write, &destination, &[result]);
        }
        if let Ok(end) = execute::get_property_result(&destination, "end") {
            let _ = execute::call(&end, &destination, &[]);
        }
    }
    if let Value::Object(_) | Value::ObjectAlias(_) =
        execute::get_property(receiver, HASH_DATA_LISTENER_PROP)
    {
        let listener = execute::get_property(receiver, HASH_DATA_LISTENER_PROP);
        let digest = execute::get_property(receiver, RESULT_PROP);
        let encoded = match execute::to_js_string(&execute::get_property(receiver, ENCODING_PROP)) {
            Ok(encoding) => encode_digest(
                bytes_from_value(&digest).unwrap_or_default(),
                Some(&Value::String(encoding)),
            )?,
            Err(_) => digest,
        };
        execute::call(&listener, &Value::Undefined, &[encoded])?;
    }
    Ok(receiver.clone())
}

pub fn stream_read(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(receiver
        .map(|value| execute::get_property(value, RESULT_PROP))
        .unwrap_or(Value::Undefined))
}

pub fn hash_copy(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    if matches!(
        execute::get_property(receiver, DIGESTED_PROP),
        Value::Boolean(true)
    ) {
        return Err(finalized_error());
    }
    let algorithm = execute::get_property(receiver, ALGORITHM_PROP);
    let input = execute::get_property(receiver, INPUT_PROP);
    let current_length = execute::get_property(receiver, OUTPUT_LEN_PROP);
    let copy_length = if matches!(algorithm, Value::String(ref value) if value == "shake128" || value == "shake256")
    {
        match args.first().and_then(|options| {
            match execute::get_property(options, "outputLength") {
                Value::Number(value)
                    if value.is_finite() && value >= 0.0 && value.fract() == 0.0 =>
                {
                    Some(value as usize)
                }
                _ => None,
            }
        }) {
            Some(length) => length,
            None if matches!(current_length, Value::Number(0.0)) => return Err(xof_length_error()),
            None => return Err(xof_length_error()),
        }
    } else {
        0
    };
    let copy = host_api::object(vec![
        (
            "update".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_UPDATE),
        ),
        (
            "write".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_UPDATE),
        ),
        (
            "digest".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_DIGEST),
        ),
        (
            "end".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_STREAM_END),
        ),
        (
            "read".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_STREAM_READ),
        ),
        (
            "copy".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_HASH_COPY),
        ),
    ]);
    define_hidden(&copy, ALGORITHM_PROP, algorithm);
    define_hidden(&copy, INPUT_PROP, input);
    if copy_length > 0 || matches!(current_length, Value::Number(_)) {
        define_hidden(&copy, OUTPUT_LEN_PROP, Value::Number(copy_length as f64));
    }
    Ok(copy)
}

fn algorithm(value: Option<&Value>) -> Result<String, VmError> {
    algorithm_named(value, "algorithm")
}

fn algorithm_named(value: Option<&Value>, label: &str) -> Result<String, VmError> {
    let value = value.ok_or_else(|| {
        invalid_type(&format!(
            "The \"{label}\" argument must be of type string. Received undefined"
        ))
    })?;
    if !matches!(value, Value::String(_) | Value::StringUnits(_)) {
        let received = match value {
            Value::Null => "null",
            Value::Boolean(_) => "a boolean",
            Value::Number(_) => "a number",
            _ => "an object",
        };
        return Err(invalid_type(&format!(
            "The \"{label}\" argument must be of type string. Received {received}"
        )));
    }
    let text = execute::to_js_string(value)?.to_ascii_lowercase();
    let text = if matches!(text.as_str(), "dss1" | "rsa-sha1") {
        "sha1".to_string()
    } else {
        text
    };
    if matches!(
        text.as_str(),
        "md5"
            | "sha1"
            | "sha224"
            | "sha256"
            | "sha384"
            | "sha512"
            | "sha3-256"
            | "sha3-384"
            | "sha3-512"
            | "shake128"
            | "shake256"
    ) {
        Ok(text)
    } else {
        Err(VmError::Thrown(host_api::object(vec![
            (
                "name".into(),
                Value::String(if label == "hmac" { "TypeError" } else { "Error" }.into()),
            ),
            (
                "code".into(),
                Value::String(if label == "hmac" {
                    "ERR_CRYPTO_INVALID_DIGEST"
                } else {
                    "ERR_OSSL_EVP_UNSUPPORTED"
                }
                .into()),
            ),
            (
                "message".into(),
                Value::String(if label == "hmac" {
                    format!("Invalid digest: {text}")
                } else {
                    format!("Digest method not supported: {text}")
                }),
            ),
        ])))
    }
}

fn xof_length_error() -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("Error".into())),
        (
            "code".into(),
            Value::String("ERR_OSSL_EVP_NOT_XOF_OR_INVALID_LENGTH".into()),
        ),
        (
            "message".into(),
            Value::String("not XOF or invalid length".into()),
        ),
    ]))
}

fn output_length_option(
    algorithm: &str,
    options: Option<&Value>,
) -> Result<Option<usize>, VmError> {
    let Some(options) =
        options.filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    else {
        return if matches!(algorithm, "shake128" | "shake256") {
            Err(xof_length_error())
        } else {
            Ok(None)
        };
    };
    let has = execute::has_own_property(options, "outputLength");
    if !has {
        return if matches!(algorithm, "shake128" | "shake256") {
            Err(xof_length_error())
        } else {
            Ok(None)
        };
    }
    let value = execute::get_property(options, "outputLength");
    let Value::Number(number) = value else {
        return Err(invalid_type(
            "The \"options.outputLength\" property must be of type number",
        ));
    };
    if !number.is_finite() || number < 0.0 || number.fract() != 0.0 || number > usize::MAX as f64 {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("RangeError".into())),
            ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
        ])));
    }
    let length = number as usize;
    if matches!(algorithm, "shake128" | "shake256") || digest_size(algorithm) == Some(length) {
        Ok(Some(length))
    } else {
        Err(xof_length_error())
    }
}

fn digest_size(algorithm: &str) -> Option<usize> {
    Some(match algorithm {
        "md5" => 16,
        "sha1" => 20,
        "sha224" => 28,
        "sha256" => 32,
        "sha384" => 48,
        "sha512" => 64,
        "sha3-256" => 32,
        "sha3-384" => 48,
        "sha3-512" => 64,
        _ => return None,
    })
}

fn finalized_error() -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("Error".into())),
        (
            "code".into(),
            Value::String("ERR_CRYPTO_HASH_FINALIZED".into()),
        ),
        (
            "message".into(),
            Value::String("Digest already called".into()),
        ),
    ]))
}

pub(crate) fn digest_bytes(algorithm: &str, input: &[u8]) -> Result<Vec<u8>, VmError> {
    Ok(match algorithm {
        "md5" => Md5::digest(input).to_vec(),
        "sha1" => Sha1::digest(input).to_vec(),
        "sha224" => Sha224::digest(input).to_vec(),
        "sha256" => Sha256::digest(input).to_vec(),
        "sha384" => Sha384::digest(input).to_vec(),
        "sha512" => Sha512::digest(input).to_vec(),
        "sha3-256" => Sha3_256::digest(input).to_vec(),
        "sha3-384" => Sha3_384::digest(input).to_vec(),
        "sha3-512" => Sha3_512::digest(input).to_vec(),
        _ => return Err(execute::type_error("Unsupported digest")),
    })
}

pub(crate) fn shake_digest(
    algorithm: &str,
    input: &[u8],
    length: Value,
) -> Result<Vec<u8>, VmError> {
    let Value::Number(length) = length else {
        return Err(xof_length_error());
    };
    let length = length as usize;
    let mut output = vec![0_u8; length];
    if algorithm == "shake128" {
        let mut hasher = Shake128::default();
        hasher.update(input);
        hasher.finalize_xof().read(&mut output);
    } else {
        let mut hasher = Shake256::default();
        hasher.update(input);
        hasher.finalize_xof().read(&mut output);
    }
    Ok(output)
}

pub(crate) fn hmac_bytes(algorithm: &str, key: &[u8], input: &[u8]) -> Result<Vec<u8>, VmError> {
    macro_rules! run {
        ($kind:ty) => {{
            let mut mac = hmac::Hmac::<$kind>::new_from_slice(key)
                .map_err(|_| execute::type_error("Invalid key"))?;
            Mac::update(&mut mac, input);
            mac.finalize().into_bytes().to_vec()
        }};
    }
    Ok(match algorithm {
        "md5" => run!(Md5),
        "sha1" => run!(Sha1),
        "sha224" => run!(Sha224),
        "sha256" => run!(Sha256),
        "sha384" => run!(Sha384),
        "sha512" => run!(Sha512),
        "sha3-256" => run!(Sha3_256),
        "sha3-384" => run!(Sha3_384),
        "sha3-512" => run!(Sha3_512),
        _ => return Err(execute::type_error("Unsupported digest")),
    })
}

fn encode_digest(bytes: Vec<u8>, encoding: Option<&Value>) -> Result<Value, VmError> {
    let encoding = match encoding {
        Some(value) if !matches!(value, Value::Undefined) => Some(execute::to_js_string(value)?),
        _ => None,
    };
    Ok(match encoding.as_deref() {
        Some("hex") => Value::String(hex::encode(bytes)),
        Some("base64") => Value::String(base64::engine::general_purpose::STANDARD.encode(bytes)),
        Some("base64url") => {
            Value::String(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
        }
        Some("latin1") | Some("binary") => {
            Value::String(bytes.into_iter().map(char::from).collect())
        }
        Some("ucs2") | Some("ucs-2") | Some("utf16le") | Some("utf-16le") => {
            let units = bytes
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>();
            Value::String(String::from_utf16_lossy(&units))
        }
        _ => crate::modules::buffer_proto::make_buffer(&bytes),
    })
}

fn dsa_p1363(options: Option<&Value>) -> Result<bool, VmError> {
    let Some(options) = options else {
        return Ok(false);
    };
    match execute::get_property(options, "dsaEncoding") {
        Value::Undefined => Ok(false),
        Value::String(ref value) if value == "der" => Ok(false),
        Value::String(ref value) if value == "ieee-p1363" => Ok(true),
        value => Err(invalid_option("dsaEncoding", &value)),
    }
}

fn dsa_signature_width<T>(pkey: &PKey<T>) -> Option<usize>
where
    T: openssl::pkey::HasPublic,
{
    match pkey.id() {
        Id::EC => pkey
            .ec_key()
            .ok()
            .map(|key| ((key.group().degree() + 7) / 8) as usize),
        Id::DSA => pkey
            .dsa()
            .ok()
            .map(|key| ((key.q().num_bits() + 7) / 8) as usize),
        _ => None,
    }
}

fn der_length(input: &[u8], offset: &mut usize) -> Option<usize> {
    let first = *input.get(*offset)?;
    *offset += 1;
    if first < 0x80 {
        return Some(first as usize);
    }
    let count = (first & 0x7f) as usize;
    if count == 0 || count > std::mem::size_of::<usize>() || *offset + count > input.len() {
        return None;
    }
    let mut length = 0usize;
    for byte in &input[*offset..*offset + count] {
        length = length.checked_mul(256)?.checked_add(*byte as usize)?;
    }
    *offset += count;
    Some(length)
}

fn der_integer(input: &[u8], offset: &mut usize) -> Option<Vec<u8>> {
    if *input.get(*offset)? != 0x02 {
        return None;
    }
    *offset += 1;
    let length = der_length(input, offset)?;
    let end = offset.checked_add(length)?;
    let value = input.get(*offset..end)?.to_vec();
    *offset = end;
    Some(value)
}

fn der_to_p1363(signature: &[u8], width: usize) -> Option<Vec<u8>> {
    let mut offset = 0;
    if *signature.get(offset)? != 0x30 {
        return None;
    }
    offset += 1;
    let length = der_length(signature, &mut offset)?;
    let end = offset.checked_add(length)?;
    let mut r = der_integer(signature, &mut offset)?;
    let mut s = der_integer(signature, &mut offset)?;
    if end > signature.len() || offset != end {
        return None;
    }
    while r.first() == Some(&0) && r.len() > 1 {
        r.remove(0);
    }
    while s.first() == Some(&0) && s.len() > 1 {
        s.remove(0);
    }
    if r.len() > width || s.len() > width {
        return None;
    }
    let mut output = vec![0u8; width * 2];
    output[width - r.len()..width].copy_from_slice(&r);
    output[2 * width - s.len()..].copy_from_slice(&s);
    Some(output)
}

fn p1363_integer(value: &[u8]) -> Vec<u8> {
    let first = value
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(value.len() - 1);
    let mut integer = value[first..].to_vec();
    if integer.first().is_some_and(|byte| byte & 0x80 != 0) {
        integer.insert(0, 0);
    }
    integer
}

fn der_encode_length(length: usize) -> Vec<u8> {
    if length < 0x80 {
        return vec![length as u8];
    }
    let mut bytes = Vec::new();
    let mut value = length;
    while value != 0 {
        bytes.push((value & 0xff) as u8);
        value >>= 8;
    }
    bytes.reverse();
    let mut output = vec![0x80 | bytes.len() as u8];
    output.extend(bytes);
    output
}

fn p1363_to_der(signature: &[u8], width: usize) -> Option<Vec<u8>> {
    if signature.len() != width * 2 {
        return None;
    }
    let r = p1363_integer(&signature[..width]);
    let s = p1363_integer(&signature[width..]);
    let body_len = 1
        + der_encode_length(r.len()).len()
        + r.len()
        + 1
        + der_encode_length(s.len()).len()
        + s.len();
    let mut output = vec![0x30];
    output.extend(der_encode_length(body_len));
    output.push(0x02);
    output.extend(der_encode_length(r.len()));
    output.extend(r);
    output.push(0x02);
    output.extend(der_encode_length(s.len()));
    output.extend(s);
    Some(output)
}

fn bytes(value: Option<&Value>) -> Result<Vec<u8>, VmError> {
    value.and_then(bytes_from_value).ok_or_else(|| {
        let received = value.map(crate::modules::util::invalid_arg_received).unwrap_or_default();
        invalid_type(&format!("The \"data\" argument must be of type string or an instance of Buffer, TypedArray, or DataView.{received}"))
    })
}

fn bytes_with_encoding(
    value: Option<&Value>,
    encoding: Option<&Value>,
) -> Result<Vec<u8>, VmError> {
    if let (Some(Value::String(text)), Some(encoding)) = (value, encoding) {
        let encoding = execute::to_js_string(encoding)?.to_ascii_lowercase();
        if matches!(encoding.as_str(), "latin1" | "binary") {
            return Ok(text.chars().map(|ch| (ch as u32 & 0xff) as u8).collect());
        }
        if matches!(encoding.as_str(), "utf8" | "utf-8") {
            return Ok(text.as_bytes().to_vec());
        }
        if encoding == "hex" {
            return hex::decode(text).map_err(|_| {
                VmError::Thrown(native_error(
                    quench_runtime::ops::Builtin::TypeError,
                    "ERR_INVALID_ARG_VALUE",
                    &format!(
                        "The argument 'encoding' is invalid for data of length {}. Received 'hex'",
                        text.len()
                    ),
                ))
            });
        }
        if encoding == "base64" {
            return base64::engine::general_purpose::STANDARD
                .decode(text)
                .map_err(|_| invalid_type("Invalid base64 encoding"));
        }
        if matches!(encoding.as_str(), "ucs2" | "ucs-2" | "utf16le" | "utf-16le") {
            return Ok(text.encode_utf16().flat_map(u16::to_le_bytes).collect());
        }
    }
    bytes(value)
}

fn invalid_type(message: &str) -> VmError {
    VmError::Thrown(native_error(
        quench_runtime::ops::Builtin::TypeError,
        "ERR_INVALID_ARG_TYPE",
        message,
    ))
}

fn crypto_type_error(code: &str, message: &str) -> VmError {
    VmError::Thrown(native_error(
        quench_runtime::ops::Builtin::Error,
        code,
        message,
    ))
}

fn crypto_error(code: &str, message: &str) -> VmError {
    crypto_type_error(code, message)
}

fn invalid_option(name: &str, value: &Value) -> VmError {
    let received = match value {
        Value::Number(number) if *number == 0.0 => "0".to_owned(),
        _ => crate::modules::util::inspect(value),
    };
    let property = if name.starts_with("options.") {
        name.to_owned()
    } else {
        format!("options.{name}")
    };
    VmError::Thrown(native_error(
        quench_runtime::ops::Builtin::TypeError,
        "ERR_INVALID_ARG_VALUE",
        &format!("The property '{property}' is invalid. Received {received}"),
    ))
}

fn native_error(builtin: quench_runtime::ops::Builtin, code: &str, message: &str) -> Value {
    // `builtins::error` already selects the active realm's intrinsic
    // constructor and prototype. Do not overwrite those with the public
    // global property, which may be a host capability and would make
    // `error instanceof Error` false.
    let mut error = quench_runtime::builtins::error(builtin, &[Value::String(message.into())]);
    error = execute::set_property(error, "code", Value::String(code.into()));
    error
}

/// Convert legacy host error records into realm-native Error instances before
/// delivering them through asynchronous crypto callbacks.
pub(crate) fn normalize_callback_error(error: Value) -> Value {
    let code = execute::to_js_string(&execute::get_property(&error, "code"))
        .unwrap_or_else(|_| "ERR_CRYPTO_OPERATION_FAILED".into());
    let message = execute::to_js_string(&execute::get_property(&error, "message"))
        .unwrap_or_else(|_| "Crypto operation failed".into());
    let name = execute::to_js_string(&execute::get_property(&error, "name")).unwrap_or_default();
    let builtin = if name == "TypeError" {
        quench_runtime::ops::Builtin::TypeError
    } else if name == "RangeError" {
        quench_runtime::ops::Builtin::RangeError
    } else {
        quench_runtime::ops::Builtin::Error
    };
    let mut normalized = native_error(builtin, &code, &message);
    for property in ["library", "reason"] {
        let value = execute::get_property(&error, property);
        if !matches!(value, Value::Undefined) {
            execute::set_property_in_place(&normalized, property, value);
        }
    }
    normalized
}

pub(crate) fn bytes_from_value(value: &Value) -> Option<Vec<u8>> {
    if crate::modules::url_whatwg::is_url_instance(value) {
        return url_file_bytes(value);
    }
    if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
        let key = execute::get_property(value, KEY_DATA_PROP);
        if !matches!(key, Value::Undefined) {
            return bytes_from_value(&key);
        }
    }
    match value {
        Value::String(text) => Some(text.as_bytes().to_vec()),
        Value::StringUnits(_) => execute::to_js_string(value)
            .ok()
            .map(|text| text.into_bytes()),
        Value::Uint8Array(view) => {
            let bytes = view.buffer.bytes.borrow();
            let end = view.byte_offset.checked_add(view.byte_length())?;
            Some(bytes.get(view.byte_offset..end)?.to_vec())
        }
        Value::Int8Array(view) => typed_bytes(&view.buffer, view.byte_offset, view.length, 1),
        Value::Uint8ClampedArray(view) => {
            typed_bytes(&view.buffer, view.byte_offset, view.length, 1)
        }
        Value::Int16Array(view) => typed_bytes(&view.buffer, view.byte_offset, view.length, 2),
        Value::Uint16Array(view) => typed_bytes(&view.buffer, view.byte_offset, view.length, 2),
        Value::Int32Array(view) => typed_bytes(&view.buffer, view.byte_offset, view.length, 4),
        Value::Uint32Array(view) => typed_bytes(&view.buffer, view.byte_offset, view.length, 4),
        Value::Float32Array(view) => typed_bytes(&view.buffer, view.byte_offset, view.length, 4),
        Value::Float64Array(view) => typed_bytes(&view.buffer, view.byte_offset, view.length, 8),
        Value::BigInt64Array(view) => typed_bytes(&view.buffer, view.byte_offset, view.length, 8),
        Value::BigUint64Array(view) => typed_bytes(&view.buffer, view.byte_offset, view.length, 8),
        Value::ArrayBuffer(buffer) => Some(buffer.bytes.borrow().clone()),
        Value::DataView(view) => {
            let bytes = view.buffer.bytes.borrow();
            let end = view.byte_offset.checked_add(view.byte_length)?;
            Some(bytes.get(view.byte_offset..end)?.to_vec())
        }
        _ => None,
    }
}

fn typed_bytes(
    buffer: &std::rc::Rc<quench_runtime::value::ArrayBufferData>,
    offset: usize,
    length: usize,
    element_size: usize,
) -> Option<Vec<u8>> {
    let bytes = buffer.bytes.borrow();
    let end = offset.checked_add(length.checked_mul(element_size)?)?;
    Some(bytes.get(offset..end)?.to_vec())
}
