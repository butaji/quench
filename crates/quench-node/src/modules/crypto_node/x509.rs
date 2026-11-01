//! `X509Certificate`.

use super::errors::{self, CryptoError};
use super::{arg, attach, define_methods, get, is_undefined, with_native, Class, Context, Members, Native, Outcome};
use crate::modules::tls_shared_vm::certificate;
use foreign_types::ForeignTypeRef;
use openssl::hash::MessageDigest;
use openssl::x509::{X509, X509Ref};
use quench_runtime::{RootId, RootedError};

unsafe extern "C" {
    fn X509_check_private_key(certificate: *const openssl_sys::X509, key: *const openssl_sys::EVP_PKEY) -> std::ffi::c_int;
}

/// A parsed certificate.
pub(crate) struct Certificate(X509);

/// `X509_CHECK_FLAG_*` for `checkHost`'s `subject` option, from openssl/x509v3.h.
const CHECK_FLAG_ALWAYS_CHECK_SUBJECT: u32 = 0x1;
const CHECK_FLAG_NEVER_CHECK_SUBJECT: u32 = 0x20;
const CHECK_FLAG_NO_WILDCARDS: u32 = 0x2;
const CHECK_FLAG_NO_PARTIAL_WILDCARDS: u32 = 0x4;
const CHECK_FLAG_MULTI_LABEL_WILDCARDS: u32 = 0x8;
const CHECK_FLAG_SINGLE_LABEL_SUBDOMAINS: u32 = 0x10;

/// `new X509Certificate(buffer)`: PEM or DER.
pub(crate) fn constructor(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let buffer = arg(c, args, 0);
    let bytes = if super::is_string(c, buffer) {
        let undefined = c.undefined();
        super::string_bytes(c, buffer, undefined)?
    } else if super::is_view(c, buffer)? {
        super::buffer_bytes(c, buffer).unwrap_or_default()
    } else {
        return Err(errors::invalid_type(c, buffer, "buffer", super::accepts::DATA));
    };
    let parsed = if bytes.trim_ascii_start().starts_with(b"-----") { X509::from_pem(&bytes) } else { X509::from_der(&bytes).or_else(|_| X509::from_pem(&bytes)) };
    let certificate = parsed.map_err(|stack| errors::openssl(c, stack))?;
    attach(c, this, Native::Certificate(Certificate(certificate)))?;
    Ok(c.undefined())
}

fn certificate(c: &mut Context<'_>, this: RootId) -> Result<X509, RootedError> {
    let found = with_native(c, this, |native| match native {
        Native::Certificate(Certificate(certificate)) => Some(certificate.clone()),
        _ => None,
    })?;
    found.flatten().ok_or_else(|| errors::throw(c, CryptoError::InvalidThis("X509Certificate")))
}

fn text(c: &mut Context<'_>, value: Result<String, openssl::error::ErrorStack>) -> Outcome {
    match value {
        Ok(text) => Ok(c.string_rooted(&text)),
        Err(stack) => Err(errors::openssl(c, stack)),
    }
}

fn fingerprint(certificate: &X509Ref, digest: MessageDigest) -> Result<String, openssl::error::ErrorStack> {
    let bytes = certificate.digest(digest)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02X}")).collect::<Vec<_>>().join(":"))
}

/// A member of the legacy object (`subjectaltname`, `infoaccess`, `ca`).
fn legacy_member(c: &mut Context<'_>, certificate: &X509Ref, name: &str) -> Outcome {
    let legacy = certificate::legacy(c, certificate)?;
    get(c, legacy, name)
}

/// The certificate's validity bound as a `Date`.
fn date(c: &mut Context<'_>, text: String) -> Outcome {
    let global = c.global_root()?;
    let constructor = get(c, global, "Date")?;
    let text = c.string_rooted(&text);
    c.construct_rooted(constructor, constructor, &[text])
}

/// The getters, by the field each projects.
#[derive(Clone, Copy)]
enum Field {
    Subject,
    SubjectAltName,
    Issuer,
    IssuerCertificate,
    InfoAccess,
    ValidFrom,
    ValidTo,
    ValidFromDate,
    ValidToDate,
    Fingerprint,
    Fingerprint256,
    Fingerprint512,
    KeyUsage,
    SerialNumber,
    Raw,
    PublicKey,
    Ca,
}

const FIELDS: [(&str, Field); 17] = [
    ("subject", Field::Subject),
    ("subjectAltName", Field::SubjectAltName),
    ("issuer", Field::Issuer),
    ("issuerCertificate", Field::IssuerCertificate),
    ("infoAccess", Field::InfoAccess),
    ("validFrom", Field::ValidFrom),
    ("validTo", Field::ValidTo),
    ("validFromDate", Field::ValidFromDate),
    ("validToDate", Field::ValidToDate),
    ("fingerprint", Field::Fingerprint),
    ("fingerprint256", Field::Fingerprint256),
    ("fingerprint512", Field::Fingerprint512),
    ("keyUsage", Field::KeyUsage),
    ("serialNumber", Field::SerialNumber),
    ("raw", Field::Raw),
    ("publicKey", Field::PublicKey),
    ("ca", Field::Ca),
];

/// One getter: the field its data names.
pub(crate) fn field(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let index = c.host_function_data()?;
    let index = c.to_number(index)? as usize;
    let cert = certificate(c, this)?;
    match FIELDS[index].1 {
        Field::Subject => text(c, certificate::multiline_name(cert.subject_name())),
        Field::Issuer => text(c, certificate::multiline_name(cert.issuer_name())),
        Field::SubjectAltName => legacy_member(c, &cert, "subjectaltname"),
        Field::InfoAccess => legacy_member(c, &cert, "infoaccess"),
        Field::Ca => {
            let ca = legacy_member(c, &cert, "ca")?;
            let ca = c.truthy_rooted(ca)?;
            Ok(c.boolean(ca))
        }
        Field::IssuerCertificate => Ok(c.undefined()),
        Field::ValidFrom => Ok(c.string_rooted(&cert.not_before().to_string())),
        Field::ValidTo => Ok(c.string_rooted(&cert.not_after().to_string())),
        Field::ValidFromDate => date(c, cert.not_before().to_string()),
        Field::ValidToDate => date(c, cert.not_after().to_string()),
        Field::Fingerprint => text(c, fingerprint(&cert, MessageDigest::sha1())),
        Field::Fingerprint256 => text(c, fingerprint(&cert, MessageDigest::sha256())),
        Field::Fingerprint512 => text(c, fingerprint(&cert, MessageDigest::sha512())),
        Field::KeyUsage => key_usage(c, &cert),
        Field::SerialNumber => text(c, cert.serial_number().to_bn().and_then(|number| number.to_hex_str().map(|text| text.to_string()))),
        Field::Raw => {
            let der = cert.to_der().map_err(|stack| errors::openssl(c, stack))?;
            super::buffer(c, der)
        }
        Field::PublicKey => {
            let key = cert.public_key().map_err(|stack| errors::openssl(c, stack))?;
            let key = crate::modules::crypto_asymmetric::OwnedKey::Public(key);
            super::keys::key_object(c, std::sync::Arc::new(super::keys::Key::Asymmetric(key)))
        }
    }
}

/// The extended key usage OIDs, or `undefined` without the extension.
fn key_usage(c: &mut Context<'_>, cert: &X509Ref) -> Outcome {
    // SAFETY: X509_get_ext_d2i returns a fresh owned stack of ASN1_OBJECTs, or null.
    let usages = unsafe {
        openssl_sys::X509_get_ext_d2i(cert.as_ptr(), openssl_sys::NID_ext_key_usage, std::ptr::null_mut(), std::ptr::null_mut())
    } as *mut openssl_sys::stack_st_ASN1_OBJECT;
    if usages.is_null() {
        return Ok(c.undefined());
    }
    let mut values = Vec::new();
    // SAFETY: the stack is live until freed below; each element is an ASN1_OBJECT.
    unsafe {
        let count = openssl_sys::OPENSSL_sk_num(usages.cast());
        for index in 0..count {
            let object = openssl_sys::OPENSSL_sk_value(usages.cast(), index) as *const openssl_sys::ASN1_OBJECT;
            let mut buffer = [0 as std::ffi::c_char; 128];
            let length = openssl_sys::OBJ_obj2txt(buffer.as_mut_ptr(), buffer.len() as i32, object, 1);
            if length > 0 {
                let text = std::ffi::CStr::from_ptr(buffer.as_ptr()).to_string_lossy().into_owned();
                values.push(text);
            }
        }
        openssl_sys::OPENSSL_sk_pop_free(usages.cast(), Some(std::mem::transmute::<unsafe extern "C" fn(*mut openssl_sys::ASN1_OBJECT), unsafe extern "C" fn(*mut std::ffi::c_void)>(openssl_sys::ASN1_OBJECT_free)));
    }
    super::string_array(c, &values)
}

/// `toString()`/`toJSON()`: the PEM text.
pub(crate) fn to_string(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let cert = certificate(c, this)?;
    let pem = cert.to_pem().map_err(|stack| errors::openssl(c, stack))?;
    Ok(c.string_rooted(&String::from_utf8_lossy(&pem)))
}

/// `toLegacyObject()`.
pub(crate) fn to_legacy_object(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let cert = certificate(c, this)?;
    certificate::legacy(c, &cert)
}

/// `checkHost`'s flags from its options.
fn check_flags(c: &mut Context<'_>, options: RootId) -> Result<u32, RootedError> {
    if is_undefined(c, options) {
        return Ok(0);
    }
    super::object_argument(c, options, "options")?;
    let subject = get(c, options, "subject")?;
    let mut flags = match c.string_text(subject)?.as_deref() {
        _ if is_undefined(c, subject) => 0,
        Some("default") => 0,
        Some("always") => CHECK_FLAG_ALWAYS_CHECK_SUBJECT,
        Some("never") => CHECK_FLAG_NEVER_CHECK_SUBJECT,
        _ => return Err(errors::invalid_value(c, subject, "options.subject", "must be one of: 'default', 'always', 'never'")),
    };
    for (name, flag, when) in [
        ("wildcards", CHECK_FLAG_NO_WILDCARDS, false),
        ("partialWildcards", CHECK_FLAG_NO_PARTIAL_WILDCARDS, false),
        ("multiLabelWildcards", CHECK_FLAG_MULTI_LABEL_WILDCARDS, true),
        ("singleLabelSubdomains", CHECK_FLAG_SINGLE_LABEL_SUBDOMAINS, true),
    ] {
        let value = get(c, options, name)?;
        if is_undefined(c, value) {
            continue;
        }
        if c.value_type_rooted(value) != Some(quench_runtime::ValueType::Boolean) {
            return Err(errors::invalid_type(c, value, &format!("options.{name}"), "of type boolean"));
        }
        if c.truthy_rooted(value)? == when {
            flags |= flag;
        }
    }
    Ok(flags)
}

/// The kinds of name a certificate is checked against.
#[derive(Clone, Copy)]
enum Check {
    Host,
    Email,
    Ip,
}

fn check(c: &mut Context<'_>, this: RootId, args: &[RootId], kind: Check) -> Outcome {
    let (name, options) = (arg(c, args, 0), arg(c, args, 1));
    let argument = match kind {
        Check::Ip => "ip",
        Check::Email => "email",
        Check::Host => "name",
    };
    let text = super::string_argument(c, name, argument)?;
    let flags = match kind {
        Check::Ip => 0,
        _ => check_flags(c, options)?,
    };
    let cert = certificate(c, this)?;
    // SAFETY: each check reads the live certificate and the given text.
    let matched = unsafe {
        match kind {
            Check::Host => {
                let mut peer = std::ptr::null_mut();
                let result = openssl_sys::X509_check_host(cert.as_ptr(), text.as_ptr().cast(), text.len(), flags, &mut peer);
                let peer_name = (!peer.is_null()).then(|| std::ffi::CStr::from_ptr(peer).to_string_lossy().into_owned());
                if !peer.is_null() {
                    openssl_sys::OPENSSL_free(peer.cast());
                }
                (result == 1).then(|| peer_name.unwrap_or_else(|| text.clone()))
            }
            Check::Email => (openssl_sys::X509_check_email(cert.as_ptr(), text.as_ptr().cast(), text.len(), flags) == 1).then(|| text.clone()),
            Check::Ip => {
                let address = std::ffi::CString::new(text.clone()).unwrap_or_default();
                (openssl_sys::X509_check_ip_asc(cert.as_ptr(), address.as_ptr(), flags) == 1).then(|| text.clone())
            }
        }
    };
    match matched {
        Some(name) => Ok(c.string_rooted(&name)),
        None => Ok(c.undefined()),
    }
}

/// `checkHost(name[, options])`.
pub(crate) fn check_host(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    check(c, this, args, Check::Host)
}

/// `checkEmail(email[, options])`.
pub(crate) fn check_email(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    check(c, this, args, Check::Email)
}

/// `checkIP(ip)`.
pub(crate) fn check_ip(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    check(c, this, args, Check::Ip)
}

/// `checkIssued(otherCert)`.
pub(crate) fn check_issued(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let other = arg(c, args, 0);
    let cert = certificate(c, this)?;
    let Some(issuer) = with_native(c, other, |native| match native {
        Native::Certificate(Certificate(certificate)) => Some(certificate.clone()),
        _ => None,
    })?
    .flatten() else {
        return Err(errors::invalid_type(c, other, "otherCert", "an instance of X509Certificate"));
    };
    let issued = issuer.issued(&cert) == openssl::x509::X509VerifyResult::OK;
    Ok(c.boolean(issued))
}

fn asymmetric(c: &mut Context<'_>, value: RootId, expected: &'static str) -> Result<crate::modules::crypto_asymmetric::OwnedKey, RootedError> {
    let key = super::keys::key_object_key(c, value)?;
    match key.as_ref().and_then(|key| key.asymmetric()).filter(|_| key.as_ref().is_some_and(|key| key.type_name() == expected)) {
        Some(owned) => Ok(owned.clone()),
        None => match key {
            Some(key) => Err(errors::throw(c, CryptoError::InvalidKeyObjectType(key.type_name(), expected))),
            None => Err(errors::invalid_type(c, value, "pkey", "an instance of KeyObject")),
        },
    }
}

/// `checkPrivateKey(privateKey)`.
pub(crate) fn check_private_key(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let key = arg(c, args, 0);
    let key = asymmetric(c, key, "private")?;
    let cert = certificate(c, this)?;
    let Some(private) = key.private_ref() else {
        return Ok(c.boolean(false));
    };
    let _errors = crate::modules::crypto_provider_facts::ErrorScope::new();
    // SAFETY: X509_check_private_key reads the live certificate and key.
    let matches = unsafe { X509_check_private_key(cert.as_ptr(), private.as_ptr()) } == 1;
    Ok(c.boolean(matches))
}

/// `verify(publicKey)`.
pub(crate) fn verify(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let key = arg(c, args, 0);
    let key = asymmetric(c, key, "public")?;
    let cert = certificate(c, this)?;
    let _errors = crate::modules::crypto_provider_facts::ErrorScope::new();
    let verified = cert.verify(key.public_ref()).unwrap_or(false);
    Ok(c.boolean(verified))
}

pub(crate) fn build(c: &mut Context<'_>) -> Result<Vec<(Class, RootId)>, RootedError> {
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation("cryptoX509Constructor"), "X509Certificate")?;
    let prototype = get(c, constructor, "prototype")?;
    for (index, (name, _)) in FIELDS.iter().enumerate() {
        let data = c.number(index as f64);
        let getter = c.host_function_named_with_data(crate::host::shared_vm::operation("cryptoX509Field"), &format!("get {name}"), data)?;
        let key = c.string_rooted(name);
        let fields = quench_runtime::DescriptorFields {
            get: Some(getter),
            set: Some(c.undefined()),
            enumerable: Some(false),
            configurable: Some(true),
            ..quench_runtime::DescriptorFields::default()
        };
        c.define_property_fields_rooted(prototype, key, fields)?;
    }
    define_methods(
        c,
        prototype,
        &[
            ("toString", "cryptoX509ToString"),
            ("toJSON", "cryptoX509ToString"),
            ("checkHost", "cryptoX509CheckHostApi"),
            ("checkEmail", "cryptoX509CheckEmail"),
            ("checkIP", "cryptoX509CheckIp"),
            ("checkIssued", "cryptoX509CheckIssued"),
            ("checkPrivateKey", "cryptoX509CheckPrivateKey"),
            ("verify", "cryptoX509Verify"),
            ("toLegacyObject", "cryptoX509ToLegacyObject"),
        ],
        Members::Declared,
    )?;
    let certificate = c.host_constructor_named(crate::host::shared_vm::operation("cryptoCertificate"), "Certificate")?;
    let prototype = get(c, certificate, "prototype")?;
    let spkac = [
        ("verifySpkac", "cryptoSpkacVerify"),
        ("exportPublicKey", "cryptoSpkacExportPublicKey"),
        ("exportChallenge", "cryptoSpkacExportChallenge"),
    ];
    define_methods(c, prototype, &spkac, Members::Assigned)?;
    let mut statics = spkac;
    statics.reverse();
    define_methods(c, certificate, &statics, Members::Assigned)?;
    Ok(vec![(Class::X509Certificate, constructor), (Class::Certificate, certificate)])
}

// ---- Certificate (SPKAC) -------------------------------------------------------------

/// `NETSCAPE_SPKI` and `NETSCAPE_SPKAC`'s leading members, from openssl/x509.h.
#[repr(C)]
struct Spki {
    spkac: *mut Spkac,
}

#[repr(C)]
struct Spkac {
    public_key: *mut std::ffi::c_void,
    challenge: *mut openssl_sys::ASN1_STRING,
}

unsafe extern "C" {
    fn NETSCAPE_SPKI_b64_decode(text: *const std::ffi::c_char, length: std::ffi::c_int) -> *mut Spki;
    fn NETSCAPE_SPKI_free(spki: *mut Spki);
    fn NETSCAPE_SPKI_get_pubkey(spki: *mut Spki) -> *mut openssl_sys::EVP_PKEY;
    fn NETSCAPE_SPKI_verify(spki: *mut Spki, key: *mut openssl_sys::EVP_PKEY) -> std::ffi::c_int;
}

/// A decoded SPKAC, freed on drop.
struct DecodedSpkac(*mut Spki);

impl Drop for DecodedSpkac {
    fn drop(&mut self) {
        // SAFETY: the pointer came from NETSCAPE_SPKI_b64_decode and is freed once.
        unsafe { NETSCAPE_SPKI_free(self.0) }
    }
}

fn decode_spkac(c: &mut Context<'_>, args: &[RootId]) -> Result<Option<DecodedSpkac>, RootedError> {
    let (spkac, encoding) = (arg(c, args, 0), arg(c, args, 1));
    let bytes = super::binary_like(c, spkac, "spkac", encoding)?;
    let text = bytes.trim_ascii();
    let Ok(length) = std::ffi::c_int::try_from(text.len()) else {
        return Ok(None);
    };
    let _errors = crate::modules::crypto_provider_facts::ErrorScope::new();
    // SAFETY: the decoder reads `length` bytes of `text`.
    let spki = unsafe { NETSCAPE_SPKI_b64_decode(text.as_ptr().cast(), length) };
    Ok((!spki.is_null()).then_some(DecodedSpkac(spki)))
}

fn spkac_public_key(spkac: &DecodedSpkac) -> Option<openssl::pkey::PKey<openssl::pkey::Public>> {
    // SAFETY: the SPKI is live; the returned key is a new reference the PKey owns.
    let key = unsafe { NETSCAPE_SPKI_get_pubkey(spkac.0) };
    (!key.is_null()).then(|| unsafe { <openssl::pkey::PKey<openssl::pkey::Public> as foreign_types::ForeignType>::from_ptr(key) })
}

/// `new Certificate()` (callable without `new`).
pub(crate) fn certificate_constructor(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    if let Some(instance) = super::construct_if_called(c, this, Class::Certificate, args)? {
        return Ok(instance);
    }
    Ok(c.undefined())
}

/// `Certificate.verifySpkac(spkac[, encoding])`.
pub(crate) fn verify_spkac(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let verified = decode_spkac(c, args)?.is_some_and(|spkac| {
        spkac_public_key(&spkac).is_some_and(|key| {
            // SAFETY: both the SPKI and the key are live.
            (unsafe { NETSCAPE_SPKI_verify(spkac.0, key.as_ptr()) }) > 0
        })
    });
    Ok(c.boolean(verified))
}

/// `Certificate.exportPublicKey(spkac[, encoding])`: the key as PEM, or `''`.
pub(crate) fn spkac_public_key_pem(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let pem = decode_spkac(c, args)?.and_then(|spkac| spkac_public_key(&spkac)).and_then(|key| key.public_key_to_pem().ok());
    match pem {
        Some(pem) => super::buffer(c, pem),
        None => Ok(c.string_rooted("")),
    }
}

/// `Certificate.exportChallenge(spkac[, encoding])`: the challenge, or `''`.
pub(crate) fn spkac_challenge(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let challenge = decode_spkac(c, args)?.and_then(|spkac| {
        // SAFETY: a decoded SPKI has an SPKAC whose challenge is an ASN1 string or null.
        unsafe {
            let spkac = (*spkac.0).spkac;
            if spkac.is_null() || (*spkac).challenge.is_null() {
                return None;
            }
            let challenge = (*spkac).challenge;
            let data = openssl_sys::ASN1_STRING_get0_data(challenge);
            let length = openssl_sys::ASN1_STRING_length(challenge) as usize;
            Some(std::slice::from_raw_parts(data, length).to_vec())
        }
    });
    match challenge {
        Some(bytes) => super::buffer(c, bytes),
        None => Ok(c.string_rooted("")),
    }
}

