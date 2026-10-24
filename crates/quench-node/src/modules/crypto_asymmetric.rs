//! Owned asymmetric key material; VM slots retain this owner, never DER bytes.
use foreign_types::{ForeignType, ForeignTypeRef};
use openssl::{
    asn1::Asn1Object,
    bn::{BigNum, BigNumContext, BigNumRef},
    ec::{EcGroup, EcKey, EcPoint, PointConversionForm},
    error::ErrorStack,
    nid::Nid,
    pkey::{Id, PKey, PKeyRef, Private, Public},
    rsa::Rsa,
    symm::Cipher,
    x509::X509,
};
use std::{
    collections::BTreeMap,
    ffi::{c_char, c_int, CStr, CString},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    Public,
    Private,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Der,
    Pem,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Pkcs1,
    Pkcs8,
    Spki,
    Sec1,
}
#[derive(Debug)]
pub enum KeyError {
    OpenSsl(ErrorStack),
    Unsupported(&'static str),
    InvalidRawKey,
    UnsupportedPrivatePkcs1,
    InvalidCurve(String),
    UnsupportedJwkCurve(String),
    UnsupportedJwkAlgorithm,
    UnsupportedJwkKeyType,
    MissingKeySeed,
    InvalidJwk,
    MissingPassphrase,
}
impl From<ErrorStack> for KeyError {
    fn from(e: ErrorStack) -> Self {
        Self::OpenSsl(e)
    }
}
#[derive(Clone)]
pub enum OwnedKey {
    Public(PKey<Public>),
    Private(PKey<Private>),
}
#[derive(Debug, Default)]
pub struct Details {
    pub modulus_length: Option<u32>,
    pub public_exponent: Option<Vec<u8>>,
    pub divisor_length: Option<u32>,
    pub named_curve: Option<String>,
    pub hash_algorithm: Option<String>,
    pub mgf1_hash_algorithm: Option<String>,
    pub salt_length: Option<i32>,
}
#[derive(Clone, Debug)]
pub struct Jwk {
    pub kty: String,
    pub crv: Option<String>,
    pub alg: Option<String>,
    pub fields: BTreeMap<String, Vec<u8>>,
    pub oth: Vec<[Vec<u8>; 3]>,
}
impl Drop for Jwk {
    fn drop(&mut self) {
        for bytes in self.fields.values_mut() {
            super::crypto_secret_material::cleanse(bytes)
        }
        for [r, d, t] in &mut self.oth {
            super::crypto_secret_material::cleanse(r);
            super::crypto_secret_material::cleanse(d);
            super::crypto_secret_material::cleanse(t);
        }
    }
}
fn ec_jwk_coordinate(jwk: &Jwk, name: &'static str, size: usize) -> Result<Vec<u8>, KeyError> {
    let bytes = jwk.fields.get(name).ok_or(KeyError::InvalidJwk)?;
    let leading_zeroes = bytes.iter().take_while(|byte| **byte == 0).count();
    let magnitude = &bytes[leading_zeroes..];
    if magnitude.len() > size {
        return Err(KeyError::InvalidJwk);
    }
    let mut coordinate = vec![0; size];
    let offset = size - magnitude.len();
    coordinate[offset..].copy_from_slice(magnitude);
    Ok(coordinate)
}
fn secure_bn(bytes: &[u8]) -> Result<BigNum, ErrorStack> {
    let mut n = BigNum::new_secure()?;
    n.copy_from_slice(bytes)?;
    Ok(n)
}
unsafe extern "C" {
    fn RSA_set0_multi_prime_params(
        rsa: *mut openssl_sys::RSA,
        primes: *mut *mut openssl_sys::BIGNUM,
        exps: *mut *mut openssl_sys::BIGNUM,
        coeffs: *mut *mut openssl_sys::BIGNUM,
        pnum: c_int,
    ) -> c_int;
    fn RSA_get_multi_prime_extra_count(rsa: *const openssl_sys::RSA) -> c_int;
    fn RSA_get0_multi_prime_factors(
        rsa: *const openssl_sys::RSA,
        primes: *mut *const openssl_sys::BIGNUM,
    ) -> c_int;
    fn RSA_get0_multi_prime_crt_params(
        rsa: *const openssl_sys::RSA,
        exps: *mut *const openssl_sys::BIGNUM,
        coeffs: *mut *const openssl_sys::BIGNUM,
    ) -> c_int;
    fn EVP_PKEY_private_check(ctx: *mut openssl_sys::EVP_PKEY_CTX) -> c_int;
    fn OBJ_txt2nid(name: *const c_char) -> c_int;
    fn EVP_PKEY_eq(a: *const openssl_sys::EVP_PKEY, b: *const openssl_sys::EVP_PKEY) -> c_int;
    fn EVP_PKEY_get_utf8_string_param(
        key: *const openssl_sys::EVP_PKEY,
        name: *const c_char,
        out: *mut c_char,
        max: usize,
        len: *mut usize,
    ) -> c_int;
    fn EVP_PKEY_get_int_param(
        key: *const openssl_sys::EVP_PKEY,
        name: *const c_char,
        out: *mut c_int,
    ) -> c_int;
}
fn curve(name: &str) -> Option<Nid> {
    match name {
        "P-256" => Some(Nid::X9_62_PRIME256V1),
        "P-384" => Some(Nid::SECP384R1),
        "P-521" => Some(Nid::SECP521R1),
        "secp256k1" => Some(Nid::SECP256K1),
        _ => None,
    }
}
fn jwk_curve(nid: Nid) -> Option<&'static str> {
    match nid {
        Nid::X9_62_PRIME256V1 => Some("P-256"),
        Nid::SECP384R1 => Some("P-384"),
        Nid::SECP521R1 => Some("P-521"),
        Nid::SECP256K1 => Some("secp256k1"),
        _ => None,
    }
}
fn okp(name: &str) -> Option<Id> {
    match name {
        "Ed25519" => Some(Id::ED25519),
        "Ed448" => Some(Id::ED448),
        "X25519" => Some(Id::X25519),
        "X448" => Some(Id::X448),
        _ => None,
    }
}
fn cipher(name: &str) -> Result<Cipher, KeyError> {
    let name = CString::new(name.split('\0').next().unwrap()).unwrap();
    let ptr = unsafe { openssl_sys::EVP_get_cipherbyname(name.as_ptr()) };
    if ptr.is_null() {
        Err(KeyError::Unsupported("unknown cipher"))
    } else {
        Ok(unsafe { Cipher::from_ptr(ptr) })
    }
}
struct Password<'a> {
    bytes: Option<&'a [u8]>,
    requested: bool,
}
unsafe extern "C" fn password(
    out: *mut c_char,
    size: c_int,
    _: c_int,
    data: *mut std::ffi::c_void,
) -> c_int {
    let pass = unsafe { &mut *data.cast::<Password<'_>>() };
    pass.requested = true;
    let Some(bytes) = pass.bytes else { return -1 };
    if size < 0 || bytes.len() > size as usize {
        return -1;
    }
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), out.cast(), bytes.len()) };
    bytes.len() as c_int
}
unsafe extern "C" {
    fn PEM_read_bio_PrivateKey(
        bio: *mut openssl_sys::BIO,
        key: *mut *mut openssl_sys::EVP_PKEY,
        callback: Option<
            unsafe extern "C" fn(*mut c_char, c_int, c_int, *mut std::ffi::c_void) -> c_int,
        >,
        data: *mut std::ffi::c_void,
    ) -> *mut openssl_sys::EVP_PKEY;
    fn d2i_PKCS8PrivateKey_bio(
        bio: *mut openssl_sys::BIO,
        key: *mut *mut openssl_sys::EVP_PKEY,
        callback: Option<
            unsafe extern "C" fn(*mut c_char, c_int, c_int, *mut std::ffi::c_void) -> c_int,
        >,
        data: *mut std::ffi::c_void,
    ) -> *mut openssl_sys::EVP_PKEY;
}
fn encrypted_pkcs8(data: &[u8]) -> bool {
    if data.first() != Some(&0x30) {
        return false;
    }
    let Some(&length) = data.get(1) else {
        return false;
    };
    let offset = if length < 128 {
        2
    } else {
        let n = (length & 127) as usize;
        if n == 0 || n > std::mem::size_of::<usize>() || data.len() < 2 + n {
            return false;
        }
        2 + n
    };
    data.get(offset) == Some(&0x30)
}
fn private_reader(data: &[u8], pass: Option<&[u8]>, pem: bool) -> Result<PKey<Private>, KeyError> {
    let length =
        c_int::try_from(data.len()).map_err(|_| KeyError::Unsupported("key input is too large"))?;
    let bio = unsafe { openssl_sys::BIO_new_mem_buf(data.as_ptr().cast(), length) };
    if bio.is_null() {
        return Err(KeyError::OpenSsl(ErrorStack::get()));
    }
    let mut state = Password {
        bytes: pass,
        requested: false,
    };
    let key = unsafe {
        if pem {
            PEM_read_bio_PrivateKey(
                bio,
                std::ptr::null_mut(),
                Some(password),
                (&mut state as *mut Password<'_>).cast(),
            )
        } else {
            d2i_PKCS8PrivateKey_bio(
                bio,
                std::ptr::null_mut(),
                Some(password),
                (&mut state as *mut Password<'_>).cast(),
            )
        }
    };
    unsafe { openssl_sys::BIO_free_all(bio) };
    if key.is_null() {
        let errors = ErrorStack::get();
        if pass.is_none() && !pem && encrypted_pkcs8(data) {
            // Encrypted DER PKCS#8 is identifiable before invoking OpenSSL.
            // Node reports a missing-passphrase error for this encoding on
            // OpenSSL 3 as well, while encrypted PEM retains OpenSSL's native
            // decoder error when no passphrase was supplied.
            Err(KeyError::MissingPassphrase)
        } else if state.requested && pass.is_none() && openssl::version::number() < 0x3000_0000 {
            Err(KeyError::MissingPassphrase)
        } else {
            Err(KeyError::OpenSsl(errors))
        }
    } else {
        Ok(unsafe { PKey::from_ptr(key) })
    }
}
fn private_pem(data: &[u8], pass: Option<&[u8]>) -> Result<PKey<Private>, KeyError> {
    private_reader(data, pass, true)
}
fn private_pkcs8(data: &[u8], pass: Option<&[u8]>) -> Result<PKey<Private>, KeyError> {
    if encrypted_pkcs8(data) {
        private_reader(data, pass, false)
    } else {
        Ok(PKey::private_key_from_pkcs8(data)?)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawFormat {
    Public,
    Private,
    Seed,
}
impl OwnedKey {
    /// Mirrors CheckEcKeyData with actual public/private provider checks.
    pub fn check_ec_key_data(&self) -> bool {
        let _errors = super::crypto_provider_facts::ErrorScope::new();
        if self.key_type() != Some("ec") {
            return false;
        }
        let ctx = unsafe { openssl_sys::EVP_PKEY_CTX_new(self.raw(), std::ptr::null_mut()) };
        if ctx.is_null() {
            return false;
        }
        let ok = unsafe {
            if self.kind() == KeyKind::Private {
                EVP_PKEY_private_check(ctx)
            } else {
                openssl_sys::EVP_PKEY_public_check(ctx)
            }
        };
        unsafe { openssl_sys::EVP_PKEY_CTX_free(ctx) };
        ok == 1
    }
    pub fn kind(&self) -> KeyKind {
        match self {
            Self::Public(_) => KeyKind::Public,
            Self::Private(_) => KeyKind::Private,
        }
    }
    pub fn raw(&self) -> *mut openssl_sys::EVP_PKEY {
        match self {
            Self::Public(k) => k.as_ptr(),
            Self::Private(k) => k.as_ptr(),
        }
    }
    /// Borrow a public-only API view of the same live native owner. The public
    /// marker permits fewer operations; it does not assert private ownership.
    pub fn public_ref(&self) -> &PKeyRef<Public> {
        unsafe { PKeyRef::from_ptr(self.raw()) }
    }
    pub fn private_ref(&self) -> Option<&PKeyRef<Private>> {
        match self {
            Self::Private(k) => Some(k),
            _ => None,
        }
    }
    pub fn to_public(&self) -> Result<Self, KeyError> {
        Ok(Self::Public(PKey::public_key_from_der(
            &self.public_ref().public_key_to_der()?,
        )?))
    }
    pub fn equals(&self, other: &Self) -> bool {
        self.kind() == other.kind() && unsafe { EVP_PKEY_eq(self.raw(), other.raw()) } == 1
    }
    pub fn key_type(&self) -> Option<&'static str> {
        let id = self.public_ref().id();
        Some(if id == Id::RSA {
            "rsa"
        } else if id == Id::RSA_PSS {
            "rsa-pss"
        } else if id == Id::EC {
            "ec"
        } else if id == Id::DSA {
            "dsa"
        } else if id == Id::DH || id == Id::DHX {
            "dh"
        } else if id == Id::ED25519 {
            "ed25519"
        } else if id == Id::ED448 {
            "ed448"
        } else if id == Id::X25519 {
            "x25519"
        } else if id == Id::X448 {
            "x448"
        } else {
            return None;
        })
    }
    /// Native provider algorithm identity, including algorithms without legacy IDs.
    pub fn key_type_name(&self) -> Option<String> {
        super::crypto_provider_facts::key_algorithm_for(self.raw())
            .and_then(|algorithm| algorithm.public_name())
    }
    pub fn import(
        kind: KeyKind,
        data: &[u8],
        format: Format,
        encoding: Option<Encoding>,
        passphrase: Option<&[u8]>,
        named_curve: Option<&str>,
    ) -> Result<Self, KeyError> {
        if named_curve.is_some() {
            return Err(KeyError::Unsupported(
                "raw EC input requires separate named-curve import",
            ));
        }
        let private = || -> Result<PKey<Private>, KeyError> {
            Ok(match format {
                Format::Pem => private_pem(data, passphrase)?,
                Format::Der => match encoding
                    .ok_or(KeyError::Unsupported("DER encoding is required"))?
                {
                    Encoding::Pkcs1 => {
                        let rsa = Rsa::private_key_from_der(data)
                            .map_err(|_| KeyError::UnsupportedPrivatePkcs1)?;
                        PKey::from_rsa(rsa)?
                    }
                    Encoding::Pkcs8 => private_pkcs8(data, passphrase)?,
                    Encoding::Sec1 => PKey::from_ec_key(EcKey::private_key_from_der(data)?)?,
                    Encoding::Spki => {
                        return Err(KeyError::Unsupported("SPKI does not contain a private key"));
                    }
                },
            })
        };
        if kind == KeyKind::Private {
            return Ok(Self::Private(private()?));
        }
        let public = match format {
            Format::Pem => PKey::public_key_from_pem(data)
                .or_else(|_| Rsa::public_key_from_pem_pkcs1(data).and_then(PKey::from_rsa))
                .or_else(|_| X509::from_pem(data).and_then(|x| x.public_key())),
            Format::Der => match encoding {
                Some(Encoding::Pkcs1) => {
                    Rsa::public_key_from_der_pkcs1(data).and_then(PKey::from_rsa)
                }
                Some(Encoding::Spki) => PKey::public_key_from_der(data),
                _ => Err(ErrorStack::get()),
            },
        };
        match public {
            Ok(k) => Ok(Self::Public(k)),
            Err(e) if format == Format::Der && encoding == Some(Encoding::Spki) => {
                Err(KeyError::OpenSsl(e))
            }
            Err(_) => Self::Private(private()?).to_public(),
        }
    }
    pub fn import_raw(
        kind: KeyKind,
        data: &[u8],
        format: RawFormat,
        algorithm: &str,
        named_curve: Option<&str>,
    ) -> Result<Self, KeyError> {
        if let Some(descriptor) = super::crypto_provider_facts::key_algorithm(algorithm)
            .filter(|algorithm| algorithm.is_pqc())
        {
            if format == RawFormat::Public {
                let expected_len = match descriptor.name() {
                    "ML-DSA-44" => Some(1312),
                    "ML-DSA-65" => Some(1952),
                    "ML-DSA-87" => Some(2592),
                    "ML-KEM-512" => Some(800),
                    "ML-KEM-768" => Some(1184),
                    "ML-KEM-1024" => Some(1568),
                    name if name.contains("128") => Some(32),
                    name if name.contains("192") => Some(48),
                    name if name.contains("256") => Some(64),
                    _ => None,
                };
                if expected_len.is_some_and(|expected| data.len() != expected) {
                    return Err(KeyError::InvalidRawKey);
                }
            }
            if format == RawFormat::Private
                && descriptor.family == super::crypto_provider_facts::KeyFamily::SLHDSA
            {
                // The raw SLH-DSA private representation contains four n-byte
                // values. Reject a mismatched parameter set before asking the
                // provider to import it, which can otherwise accept the
                // differently-sized octet string under another algorithm.
                let expected_len = match descriptor.name() {
                    name if name.contains("128") => Some(64),
                    name if name.contains("192") => Some(96),
                    name if name.contains("256") => Some(128),
                    _ => None,
                };
                if expected_len.is_some_and(|expected| data.len() != expected) {
                    return Err(KeyError::InvalidRawKey);
                }
            }
            if format == RawFormat::Private
                && matches!(
                    descriptor.family,
                    super::crypto_provider_facts::KeyFamily::MLDSA
                        | super::crypto_provider_facts::KeyFamily::MLKEM
                )
            {
                return Err(KeyError::Unsupported(
                    "raw private format is not supported for this key family",
                ));
            }
            if format == RawFormat::Seed
                && matches!(
                    descriptor.family,
                    super::crypto_provider_facts::KeyFamily::SLHDSA
                )
            {
                return Err(KeyError::Unsupported(
                    "raw seed is not supported for this key family",
                ));
            }
            let param = match format {
                RawFormat::Public => Some((c"pub", KeyKind::Public)),
                RawFormat::Private => Some((c"priv", KeyKind::Private)),
                RawFormat::Seed => Some((c"seed", KeyKind::Private)),
            };
            let (param, kind) = param.expect("all PQC formats are provider octets");
            return import_akp_params(algorithm, None, Some((param, data)), kind);
        }
        if format == RawFormat::Seed {
            return Err(KeyError::Unsupported(
                "raw seed is not supported for this key family",
            ));
        }
        if kind == KeyKind::Private && format == RawFormat::Public {
            return Err(KeyError::Unsupported(
                "public material cannot initialize private key",
            ));
        }
        let key = if algorithm == "ec" {
            let name = named_curve.ok_or(KeyError::Unsupported("EC named curve is required"))?;
            let nid = curve(name).unwrap_or_else(|| {
                let name = CString::new(name.split('\0').next().unwrap()).unwrap();
                Nid::from_raw(unsafe { OBJ_txt2nid(name.as_ptr()) })
            });
            if nid == Nid::UNDEF {
                return Err(KeyError::InvalidCurve(name.to_owned()));
            }
            let group = EcGroup::from_curve_name(nid)
                .map_err(|_| KeyError::InvalidCurve(name.to_owned()))?;
            let mut ctx = BigNumContext::new()?;
            if format == RawFormat::Public {
                let width = (group.degree() as usize).div_ceil(8);
                if data.len() != width + 1 && data.len() != width * 2 + 1 {
                    return Err(KeyError::InvalidRawKey);
                }
                let point = EcPoint::from_bytes(&group, data, &mut ctx)
                    .map_err(|_| KeyError::InvalidRawKey)?;
                let key = EcKey::from_public_key(&group, &point)?;
                Self::Public(PKey::from_ec_key(key)?)
            } else {
                let mut order = BigNum::new()?;
                group.order(&mut order, &mut ctx)?;
                if data.len() != order.num_bytes() as usize {
                    return Err(KeyError::InvalidRawKey);
                }
                let d = secure_bn(data)?;
                if d.num_bits() == 0 || d.ucmp(&order) != std::cmp::Ordering::Less {
                    return Err(KeyError::InvalidRawKey);
                }
                let mut point = EcPoint::new(&group)?;
                point.mul_generator2(&group, &d, &mut ctx)?;
                let key = EcKey::from_private_components(&group, &d, &point)?;
                Self::Private(PKey::from_ec_key(key)?)
            }
        } else {
            let id = match algorithm {
                "ed25519" => Id::ED25519,
                "ed448" => Id::ED448,
                "x25519" => Id::X25519,
                "x448" => Id::X448,
                _ => return Err(KeyError::Unsupported("raw key family")),
            };
            if format == RawFormat::Public {
                Self::Public(PKey::public_key_from_raw_bytes(data, id)?)
            } else {
                Self::Private(PKey::private_key_from_raw_bytes(data, id)?)
            }
        };
        if kind == KeyKind::Public {
            key.to_public()
        } else {
            Ok(key)
        }
    }
    pub fn export(
        &self,
        format: Format,
        encoding: Encoding,
        cipher_name: Option<&str>,
        passphrase: Option<&[u8]>,
    ) -> Result<Vec<u8>, KeyError> {
        if encoding == Encoding::Pkcs1 && self.key_type() != Some("rsa") {
            return Err(KeyError::Unsupported("PKCS1 requires an RSA key"));
        }
        if encoding == Encoding::Sec1 && self.key_type() != Some("ec") {
            return Err(KeyError::Unsupported("SEC1 requires an EC key"));
        }
        let cipher = cipher_name.map(cipher).transpose()?;
        if cipher.is_some() && passphrase.is_none() {
            return Err(KeyError::MissingPassphrase);
        }
        match self {
            Self::Public(k) => {
                if cipher.is_some() {
                    return Err(KeyError::Unsupported("public key encryption"));
                }
                Ok(match (encoding, format) {
                    (Encoding::Pkcs1, Format::Pem) => k.rsa()?.public_key_to_pem_pkcs1()?,
                    (Encoding::Pkcs1, Format::Der) => k.rsa()?.public_key_to_der_pkcs1()?,
                    (Encoding::Spki, Format::Pem) => k.public_key_to_pem()?,
                    (Encoding::Spki, Format::Der) => k.public_key_to_der()?,
                    _ => return Err(KeyError::Unsupported("public key encoding")),
                })
            }
            Self::Private(k) => Ok(match (encoding, format, cipher) {
                (Encoding::Pkcs8, format, None) => {
                    if let Some(encoded) = pqc_seed_pkcs8(k, format)? {
                        return Ok(encoded);
                    }
                    match format {
                        Format::Pem => k.private_key_to_pem_pkcs8()?,
                        Format::Der => k.private_key_to_pkcs8()?,
                    }
                }
                (Encoding::Pkcs8, Format::Pem, Some(c)) => {
                    k.private_key_to_pem_pkcs8_passphrase(c, passphrase.unwrap())?
                }
                (Encoding::Pkcs8, Format::Der, Some(c)) => {
                    k.private_key_to_pkcs8_passphrase(c, passphrase.unwrap())?
                }
                (Encoding::Pkcs1, Format::Pem, None) => k.rsa()?.private_key_to_pem()?,
                (Encoding::Pkcs1, Format::Der, None) => k.rsa()?.private_key_to_der()?,
                (Encoding::Pkcs1, Format::Pem, Some(c)) => k
                    .rsa()?
                    .private_key_to_pem_passphrase(c, passphrase.unwrap())?,
                (Encoding::Sec1, Format::Pem, None) => k.ec_key()?.private_key_to_pem()?,
                (Encoding::Sec1, Format::Der, None) => k.ec_key()?.private_key_to_der()?,
                (Encoding::Sec1, Format::Pem, Some(c)) => k
                    .ec_key()?
                    .private_key_to_pem_passphrase(c, passphrase.unwrap())?,
                _ => return Err(KeyError::Unsupported("private key encoding or encryption")),
            }),
        }
    }
    pub fn details(&self) -> Result<Details, KeyError> {
        let key = self.public_ref();
        let mut d = Details::default();
        match self.key_type() {
            Some("rsa" | "rsa-pss") => {
                let rsa = key.rsa()?;
                d.modulus_length = Some(rsa.n().num_bits() as u32);
                d.public_exponent = Some(rsa.e().to_vec());
                if self.key_type() == Some("rsa-pss") {
                    let mut n = 0;
                    if unsafe { EVP_PKEY_get_int_param(self.raw(), c"saltlen".as_ptr(), &mut n) }
                        == 1
                    {
                        // OpenSSL uses a negative salt length for an unrestricted
                        // RSA-PSS key. Node omits all PSS restrictions in that case.
                        if n >= 0 {
                            // OpenSSL can leave RFC 3447's empty/default
                            // RSASSA-PSS-params sequence implicit. Node reports
                            // those defaults on constrained PSS keys.
                            d.hash_algorithm = Some(
                                self.text_param(c"digest")
                                    .unwrap_or_else(|| "sha1".to_owned()),
                            );
                            d.mgf1_hash_algorithm = Some(
                                self.text_param(c"mgf1-digest")
                                    .unwrap_or_else(|| "sha1".to_owned()),
                            );
                            d.salt_length = Some(n)
                        }
                    }
                }
            }
            Some("ec") => {
                d.named_curve = key
                    .ec_key()?
                    .group()
                    .curve_name()
                    .and_then(|n| n.short_name().ok())
                    .map(str::to_owned)
            }
            Some("dsa") => {
                let k = key.dsa()?;
                d.modulus_length = Some(k.p().num_bits() as u32);
                d.divisor_length = Some(k.q().num_bits() as u32)
            }
            _ => {}
        }
        Ok(d)
    }
    fn text_param(&self, name: &CStr) -> Option<String> {
        let mut n = 0;
        let mut out = [0i8; 128];
        if unsafe {
            EVP_PKEY_get_utf8_string_param(
                self.raw(),
                name.as_ptr(),
                out.as_mut_ptr(),
                out.len(),
                &mut n,
            )
        } == 1
        {
            Some(
                unsafe { CStr::from_ptr(out.as_ptr()) }
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .replace("sha2-", "sha"),
            )
        } else {
            None
        }
    }
    pub fn import_jwk(kind: KeyKind, jwk: &Jwk) -> Result<Self, KeyError> {
        let field = |n: &'static str| {
            jwk.fields
                .get(n)
                .map(Vec::as_slice)
                .ok_or(KeyError::InvalidJwk)
        };
        let bn = |n| secure_bn(field(n)?).map_err(KeyError::OpenSsl);
        let key = match jwk.kty.as_str() {
            "AKP" => import_akp_jwk(kind, jwk)?,
            "RSA" => {
                if jwk.fields.contains_key("d") {
                    let rsa = Rsa::from_private_components(
                        bn("n")?,
                        bn("e")?,
                        bn("d")?,
                        bn("p")?,
                        bn("q")?,
                        bn("dp")?,
                        bn("dq")?,
                        bn("qi")?,
                    )
                    .map_err(|_| KeyError::InvalidJwk)?;
                    if !jwk.oth.is_empty() {
                        let mut primes = jwk
                            .oth
                            .iter()
                            .map(|[r, _, _]| secure_bn(r).map_err(|_| KeyError::InvalidJwk))
                            .collect::<Result<Vec<_>, _>>()?;
                        let mut exps = jwk
                            .oth
                            .iter()
                            .map(|[_, d, _]| secure_bn(d).map_err(|_| KeyError::InvalidJwk))
                            .collect::<Result<Vec<_>, _>>()?;
                        let mut coeffs = jwk
                            .oth
                            .iter()
                            .map(|[_, _, t]| secure_bn(t).map_err(|_| KeyError::InvalidJwk))
                            .collect::<Result<Vec<_>, _>>()?;
                        let mut prime_ptrs = primes
                            .iter()
                            .map(|value| value.as_ptr())
                            .collect::<Vec<_>>();
                        let mut exp_ptrs =
                            exps.iter().map(|value| value.as_ptr()).collect::<Vec<_>>();
                        let mut coeff_ptrs = coeffs
                            .iter()
                            .map(|value| value.as_ptr())
                            .collect::<Vec<_>>();
                        let count =
                            c_int::try_from(prime_ptrs.len()).map_err(|_| KeyError::InvalidJwk)?;
                        if unsafe {
                            RSA_set0_multi_prime_params(
                                rsa.as_ptr(),
                                prime_ptrs.as_mut_ptr(),
                                exp_ptrs.as_mut_ptr(),
                                coeff_ptrs.as_mut_ptr(),
                                count,
                            )
                        } != 1
                        {
                            return Err(KeyError::InvalidJwk);
                        }
                        for value in primes
                            .drain(..)
                            .chain(exps.drain(..))
                            .chain(coeffs.drain(..))
                        {
                            std::mem::forget(value);
                        }
                    }
                    rsa.check_key().map_err(|_| KeyError::InvalidJwk)?;
                    Self::Private(PKey::from_rsa(rsa)?)
                } else {
                    Self::Public(PKey::from_rsa(Rsa::from_public_components(
                        bn("n")?,
                        bn("e")?,
                    )?)?)
                }
            }
            "EC" => {
                let name = jwk.crv.as_deref().ok_or(KeyError::InvalidJwk)?;
                let nid = curve(name).ok_or_else(|| KeyError::InvalidCurve(name.to_owned()))?;
                let group = EcGroup::from_curve_name(nid)?;
                let coordinate_size = (group.degree() as usize).div_ceil(8);
                let mut point = vec![4];
                point.extend(ec_jwk_coordinate(jwk, "x", coordinate_size)?);
                point.extend(ec_jwk_coordinate(jwk, "y", coordinate_size)?);
                let mut ctx = BigNumContext::new()?;
                let point = EcPoint::from_bytes(&group, &point, &mut ctx)
                    .map_err(|_| KeyError::InvalidJwk)?;
                if jwk.fields.contains_key("d") {
                    let d = bn("d")?;
                    let key = EcKey::from_private_components(&group, &d, &point)
                        .map_err(|_| KeyError::InvalidJwk)?;
                    key.check_key().map_err(|_| KeyError::InvalidJwk)?;
                    Self::Private(PKey::from_ec_key(key)?)
                } else {
                    let key =
                        EcKey::from_public_key(&group, &point).map_err(|_| KeyError::InvalidJwk)?;
                    key.check_key().map_err(|_| KeyError::InvalidJwk)?;
                    Self::Public(PKey::from_ec_key(key)?)
                }
            }
            "OKP" => {
                let id = okp(jwk.crv.as_deref().ok_or(KeyError::InvalidJwk)?)
                    .ok_or(KeyError::InvalidJwk)?;
                if let Some(d) = jwk.fields.get("d") {
                    {
                        let key = PKey::private_key_from_raw_bytes(d, id)?;
                        if key.raw_public_key()?.as_slice() != field("x")? {
                            return Err(KeyError::InvalidJwk);
                        }
                        Self::Private(key)
                    }
                } else {
                    Self::Public(PKey::public_key_from_raw_bytes(field("x")?, id)?)
                }
            }
            _ => return Err(KeyError::Unsupported("JWK key type")),
        };
        match (kind, key) {
            (KeyKind::Private, Self::Public(_)) => Err(KeyError::InvalidJwk),
            (KeyKind::Public, key) => key.to_public(),
            (_, key) => Ok(key),
        }
    }
    pub fn export_jwk_with_flags(&self, allow_rsa_pss: bool) -> Result<Jwk, KeyError> {
        let mut jwk = Jwk {
            kty: String::new(),
            crv: None,
            alg: None,
            fields: BTreeMap::new(),
            oth: Vec::new(),
        };
        let fields = &mut jwk.fields;
        if let Some(algorithm) = self
            .key_type_name()
            .and_then(|name| super::crypto_provider_facts::key_algorithm(&name))
            .filter(|algorithm| algorithm.is_pqc())
        {
            fields.insert("pub".into(), octet_param(self.raw(), c"pub")?);
            if self.kind() == KeyKind::Private {
                // ML-DSA and ML-KEM use their provider seed in AKP JWKs.
                // SLH-DSA has no seed parameter and exposes the full private
                // component instead.
                let private_param = if matches!(
                    algorithm.family,
                    super::crypto_provider_facts::KeyFamily::SLHDSA
                ) {
                    c"priv"
                } else {
                    c"seed"
                };
                fields.insert(
                    "priv".into(),
                    octet_param(self.raw(), private_param).map_err(|_| KeyError::MissingKeySeed)?,
                );
            }
            jwk.kty = "AKP".into();
            jwk.alg = Some(algorithm.name().into());
            return Ok(jwk);
        }
        let (kty, crv) = match self.key_type() {
            Some("rsa" | "rsa-pss") if self.key_type() == Some("rsa") || allow_rsa_pss => {
                let k = self.public_ref().rsa()?;
                fields.insert("n".into(), k.n().to_vec());
                fields.insert("e".into(), k.e().to_vec());
                if let Some(p) = self.private_ref() {
                    let k = p.rsa()?;
                    for (n, v) in [
                        ("d", Some(k.d())),
                        ("p", k.p()),
                        ("q", k.q()),
                        ("dp", k.dmp1()),
                        ("dq", k.dmq1()),
                        ("qi", k.iqmp()),
                    ] {
                        fields.insert(n.into(), v.ok_or(KeyError::InvalidJwk)?.to_vec());
                    }
                    let count = unsafe { RSA_get_multi_prime_extra_count(k.as_ptr()) };
                    if count < 0 || count > 8 {
                        return Err(KeyError::InvalidJwk);
                    }
                    if count > 0 {
                        let count = count as usize;
                        let mut primes = vec![std::ptr::null(); count];
                        let mut exps = vec![std::ptr::null(); count];
                        let mut coeffs = vec![std::ptr::null(); count];
                        if unsafe { RSA_get0_multi_prime_factors(k.as_ptr(), primes.as_mut_ptr()) }
                            != 1
                            || unsafe {
                                RSA_get0_multi_prime_crt_params(
                                    k.as_ptr(),
                                    exps.as_mut_ptr(),
                                    coeffs.as_mut_ptr(),
                                )
                            } != 1
                        {
                            return Err(KeyError::InvalidJwk);
                        }
                        let octets = |value: *const openssl_sys::BIGNUM| {
                            if value.is_null() {
                                return Err(KeyError::InvalidJwk);
                            }
                            Ok(unsafe { BigNumRef::from_ptr(value.cast_mut()) }.to_vec())
                        };
                        for ((r, d), t) in primes.into_iter().zip(exps).zip(coeffs) {
                            jwk.oth.push([octets(r)?, octets(d)?, octets(t)?]);
                        }
                    }
                }
                ("RSA", None)
            }
            Some("ec") => {
                let k = self.public_ref().ec_key()?;
                let group = k.group();
                let curve = group
                    .curve_name()
                    .ok_or(KeyError::Unsupported("unnamed EC curve"))?;
                let crv = jwk_curve(curve).ok_or_else(|| {
                    KeyError::UnsupportedJwkCurve(
                        curve.short_name().unwrap_or("unknown").to_owned(),
                    )
                })?;
                let width = group.degree().div_ceil(8) as i32;
                let mut x = BigNum::new()?;
                let mut y = BigNum::new()?;
                let mut ctx = BigNumContext::new()?;
                k.public_key()
                    .affine_coordinates_gfp(group, &mut x, &mut y, &mut ctx)?;
                fields.insert("x".into(), x.to_vec_padded(width)?);
                fields.insert("y".into(), y.to_vec_padded(width)?);
                if let Some(p) = self.private_ref() {
                    fields.insert("d".into(), p.ec_key()?.private_key().to_vec_padded(width)?);
                }
                ("EC", Some(crv.into()))
            }
            Some(ty @ ("ed25519" | "ed448" | "x25519" | "x448")) => {
                fields.insert("x".into(), self.public_ref().raw_public_key()?);
                if let Some(p) = self.private_ref() {
                    fields.insert("d".into(), p.raw_private_key()?);
                }
                let crv = match ty {
                    "ed25519" => "Ed25519",
                    "ed448" => "Ed448",
                    "x25519" => "X25519",
                    _ => "X448",
                };
                ("OKP", Some(crv.into()))
            }
            _ => return Err(KeyError::UnsupportedJwkKeyType),
        };
        jwk.kty = kty.into();
        jwk.crv = crv;
        Ok(jwk)
    }
    pub fn raw_public(&self, compressed: bool) -> Result<Vec<u8>, KeyError> {
        if self.is_pqc() {
            return octet_param(self.raw(), c"pub");
        }
        if self.key_type() == Some("ec") {
            let key = self.public_ref().ec_key()?;
            let mut ctx = BigNumContext::new()?;
            Ok(key.public_key().to_bytes(
                key.group(),
                if compressed {
                    PointConversionForm::COMPRESSED
                } else {
                    PointConversionForm::UNCOMPRESSED
                },
                &mut ctx,
            )?)
        } else {
            Ok(self.public_ref().raw_public_key()?)
        }
    }
    pub fn raw_private(&self) -> Result<Vec<u8>, KeyError> {
        let key = self
            .private_ref()
            .ok_or(KeyError::Unsupported("private key required"))?;
        if self.is_pqc() {
            let algorithm = self.key_type_name().unwrap_or_default();
            let family = super::crypto_provider_facts::key_algorithm(&algorithm)
                .map(|algorithm| algorithm.family);
            return octet_param(
                self.raw(),
                if matches!(
                    family,
                    Some(super::crypto_provider_facts::KeyFamily::SLHDSA)
                ) {
                    c"priv"
                } else {
                    c"seed"
                },
            );
        }
        if self.key_type() == Some("ec") {
            let ec = key.ec_key()?;
            Ok(ec
                .private_key()
                .to_vec_padded(ec.group().degree().div_ceil(8) as i32)?)
        } else {
            Ok(key.raw_private_key()?)
        }
    }
    fn is_pqc(&self) -> bool {
        self.key_type_name()
            .and_then(|name| super::crypto_provider_facts::key_algorithm(&name))
            .is_some_and(|algorithm| algorithm.is_pqc())
    }
}

fn octet_param(key: *const openssl_sys::EVP_PKEY, name: &CStr) -> Result<Vec<u8>, KeyError> {
    let mut size = 0usize;
    let ok = unsafe {
        openssl_sys::EVP_PKEY_get_octet_string_param(
            key,
            name.as_ptr(),
            std::ptr::null_mut(),
            0,
            &mut size,
        )
    };
    if ok != 1 {
        return Err(KeyError::OpenSsl(ErrorStack::get()));
    }
    let mut value = vec![0; size];
    let ok = unsafe {
        openssl_sys::EVP_PKEY_get_octet_string_param(
            key,
            name.as_ptr(),
            value.as_mut_ptr(),
            value.len(),
            &mut size,
        )
    };
    if ok != 1 {
        return Err(KeyError::OpenSsl(ErrorStack::get()));
    }
    value.truncate(size);
    Ok(value)
}

/// Node's OpenSSL key encoder serializes seeded PQC private keys using the
/// compact PKCS#8 form whose inner private-key OCTET STRING contains only the
/// seed. OpenSSL 3.5's provider encoder emits seed, public key, and expanded
/// private key instead, so preserve Node's stable PEM/DER representation here.
fn pqc_seed_pkcs8(key: &PKeyRef<Private>, format: Format) -> Result<Option<Vec<u8>>, KeyError> {
    let raw = key.as_ptr();
    let Some(algorithm) =
        super::crypto_provider_facts::key_algorithm_for(raw).filter(|algorithm| algorithm.is_pqc())
    else {
        return Ok(None);
    };
    let Ok(seed) = octet_param(raw, c"seed") else {
        // A private-only key has no seed and uses the provider's canonical
        // private-key representation (Node preserves that form as well).
        return Ok(None);
    };
    let oid = Asn1Object::from_str(algorithm.name()).map_err(KeyError::OpenSsl)?;
    let algorithm_identifier = der_tlv(0x30, &der_tlv(0x06, oid.as_slice()));
    // The ML-DSA PKCS#8 profile marks seed-only material with primitive
    // context-specific tag [0] (0x80), followed by the seed bytes.
    let seed = der_tlv(0x80, &seed);
    let private_key = der_tlv(0x04, &seed);
    let mut contents = vec![0x02, 0x01, 0x00];
    contents.extend_from_slice(&algorithm_identifier);
    contents.extend_from_slice(&private_key);
    let der = der_tlv(0x30, &contents);
    if format == Format::Der {
        return Ok(Some(der));
    }

    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(der);
    let mut pem = String::from("-----BEGIN PRIVATE KEY-----\n");
    for line in encoded.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(line).expect("base64 is ASCII"));
        pem.push('\n');
    }
    pem.push_str("-----END PRIVATE KEY-----\n");
    Ok(Some(pem.into_bytes()))
}

fn der_tlv(tag: u8, value: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(value.len() + 6);
    encoded.push(tag);
    if value.len() < 128 {
        encoded.push(value.len() as u8);
    } else {
        let bytes = value.len().to_be_bytes();
        let first = bytes
            .iter()
            .position(|byte| *byte != 0)
            .unwrap_or(bytes.len() - 1);
        encoded.push(0x80 | (bytes.len() - first) as u8);
        encoded.extend_from_slice(&bytes[first..]);
    }
    encoded.extend_from_slice(value);
    encoded
}

fn import_akp_jwk(kind: KeyKind, jwk: &Jwk) -> Result<OwnedKey, KeyError> {
    let algorithm = jwk
        .alg
        .as_deref()
        .ok_or(KeyError::UnsupportedJwkAlgorithm)?;
    let descriptor = super::crypto_provider_facts::key_algorithm(algorithm)
        .filter(|algorithm| algorithm.is_pqc())
        .ok_or(KeyError::UnsupportedJwkAlgorithm)?;
    // AKP `alg` is the canonical OpenSSL algorithm identifier. Lower-case
    // aliases are accepted by key-type lookup, but Node rejects them in JWKs.
    if algorithm != descriptor.name() {
        return Err(KeyError::UnsupportedJwkAlgorithm);
    }
    let public = jwk
        .fields
        .get("pub")
        .map(Vec::as_slice)
        .ok_or(KeyError::InvalidJwk)?;
    let expected_public_len = match descriptor.name() {
        "ML-DSA-44" => Some(1312),
        "ML-DSA-65" => Some(1952),
        "ML-DSA-87" => Some(2592),
        "ML-KEM-512" => Some(800),
        "ML-KEM-768" => Some(1184),
        "ML-KEM-1024" => Some(1568),
        name if name.contains("128") => Some(32),
        name if name.contains("192") => Some(48),
        name if name.contains("256") => Some(64),
        _ => None,
    };
    if expected_public_len.is_some_and(|len| public.len() != len) {
        return Err(KeyError::InvalidJwk);
    }
    let private_name = if matches!(
        descriptor.family,
        super::crypto_provider_facts::KeyFamily::SLHDSA
    ) {
        c"priv"
    } else {
        c"seed"
    };
    let private = jwk
        .fields
        .get("priv")
        .map(|value| {
            let expected_len = match descriptor.family {
                super::crypto_provider_facts::KeyFamily::MLDSA => Some(32),
                super::crypto_provider_facts::KeyFamily::MLKEM => Some(64),
                _ => None,
            };
            if expected_len.is_some_and(|len| value.len() != len) {
                return Err(KeyError::InvalidJwk);
            }
            Ok((private_name, value.as_slice()))
        })
        .transpose()?;
    let imported =
        import_akp_params(algorithm, Some(public), private, kind).map_err(|error| match error {
            KeyError::OpenSsl(_) => KeyError::InvalidJwk,
            error => error,
        })?;
    if private.is_some() && octet_param(imported.raw(), c"pub")? != public {
        return Err(KeyError::InvalidJwk);
    }
    Ok(imported)
}

fn import_akp_params(
    algorithm: &str,
    public: Option<&[u8]>,
    private: Option<(&CStr, &[u8])>,
    kind: KeyKind,
) -> Result<OwnedKey, KeyError> {
    use foreign_types::ForeignType;
    let descriptor = super::crypto_provider_facts::key_algorithm(algorithm)
        .filter(|algorithm| algorithm.is_pqc())
        .ok_or(KeyError::InvalidJwk)?;
    let name = CString::new(descriptor.name()).map_err(|_| KeyError::InvalidJwk)?;
    if kind == KeyKind::Private && private.is_none() {
        return Err(KeyError::InvalidJwk);
    }
    let bld = unsafe { openssl_sys::OSSL_PARAM_BLD_new() };
    if bld.is_null() {
        return Err(KeyError::OpenSsl(ErrorStack::get()));
    }
    let mut params = std::ptr::null_mut();
    let mut ctx = std::ptr::null_mut();
    let mut pkey = std::ptr::null_mut();
    let result = unsafe {
        let mut ok = 1;
        if let Some(public) = public {
            ok = openssl_sys::OSSL_PARAM_BLD_push_octet_string(
                bld,
                c"pub".as_ptr(),
                public.as_ptr().cast(),
                public.len(),
            );
        }
        if ok == 1 {
            if let Some((name, private)) = private {
                ok = openssl_sys::OSSL_PARAM_BLD_push_octet_string(
                    bld,
                    name.as_ptr(),
                    private.as_ptr().cast(),
                    private.len(),
                );
            }
        }
        if ok == 1 {
            params = openssl_sys::OSSL_PARAM_BLD_to_param(bld);
            ok = (!params.is_null()) as i32;
        }
        if ok == 1 {
            ctx = openssl_sys::EVP_PKEY_CTX_new_from_name(
                std::ptr::null_mut(),
                name.as_ptr(),
                std::ptr::null(),
            );
            ok = (!ctx.is_null()) as i32;
        }
        if ok == 1 {
            ok = openssl_sys::EVP_PKEY_fromdata_init(ctx);
        }
        if ok == 1 {
            ok = openssl_sys::EVP_PKEY_fromdata(
                ctx,
                &mut pkey,
                if kind == KeyKind::Private {
                    openssl_sys::EVP_PKEY_KEYPAIR
                } else {
                    openssl_sys::EVP_PKEY_PUBLIC_KEY
                },
                params,
            );
        }
        if !ctx.is_null() {
            openssl_sys::EVP_PKEY_CTX_free(ctx);
        }
        if !params.is_null() {
            openssl_sys::OSSL_PARAM_free(params);
        }
        openssl_sys::OSSL_PARAM_BLD_free(bld);
        ok
    };
    if result != 1 || pkey.is_null() {
        return Err(KeyError::OpenSsl(ErrorStack::get()));
    }
    Ok(if kind == KeyKind::Private {
        OwnedKey::Private(unsafe { PKey::from_ptr(pkey) })
    } else {
        OwnedKey::Public(unsafe { PKey::from_ptr(pkey) })
    })
}
