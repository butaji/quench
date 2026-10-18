//! Owned asymmetric key material; VM slots retain this owner, never DER bytes.
use foreign_types::{ForeignType, ForeignTypeRef};
use openssl::{
    bn::{BigNum, BigNumContext},
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
    ffi::{CStr, CString, c_char, c_int},
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
    pub fields: BTreeMap<String, Vec<u8>>,
}
impl Drop for Jwk {
    fn drop(&mut self) {
        for bytes in self.fields.values_mut() {
            super::crypto_secret_material::cleanse(bytes)
        }
    }
}
fn secure_bn(bytes: &[u8]) -> Result<BigNum, ErrorStack> {
    let mut n = BigNum::new_secure()?;
    n.copy_from_slice(bytes)?;
    Ok(n)
}
unsafe extern "C" {
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
        if !pem && state.requested && pass.is_none() {
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
                    Encoding::Pkcs1 => PKey::from_rsa(Rsa::private_key_from_der(data)?)?,
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
            let group = EcGroup::from_curve_name(nid)?;
            let mut ctx = BigNumContext::new()?;
            if format == RawFormat::Public {
                let point = EcPoint::from_bytes(&group, data, &mut ctx)?;
                let key = EcKey::from_public_key(&group, &point)?;
                Self::Public(PKey::from_ec_key(key)?)
            } else {
                let mut order = BigNum::new()?;
                group.order(&mut order, &mut ctx)?;
                if data.len() != order.num_bytes() as usize {
                    return Err(KeyError::Unsupported("invalid EC private key length"));
                }
                let d = secure_bn(data)?;
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
                (Encoding::Pkcs8, Format::Pem, None) => k.private_key_to_pem_pkcs8()?,
                (Encoding::Pkcs8, Format::Der, None) => k.private_key_to_pkcs8()?,
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
                    d.hash_algorithm = self.text_param(c"digest");
                    d.mgf1_hash_algorithm = self.text_param(c"mgf1-digest");
                    let mut n = 0;
                    if unsafe { EVP_PKEY_get_int_param(self.raw(), c"saltlen".as_ptr(), &mut n) }
                        == 1
                    {
                        d.salt_length = Some(n)
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
            "RSA" => {
                if jwk.fields.contains_key("d") {
                    Self::Private(PKey::from_rsa(Rsa::from_private_components(
                        bn("n")?,
                        bn("e")?,
                        bn("d")?,
                        bn("p")?,
                        bn("q")?,
                        bn("dp")?,
                        bn("dq")?,
                        bn("qi")?,
                    )?)?)
                } else {
                    Self::Public(PKey::from_rsa(Rsa::from_public_components(
                        bn("n")?,
                        bn("e")?,
                    )?)?)
                }
            }
            "EC" => {
                let nid = curve(jwk.crv.as_deref().ok_or(KeyError::InvalidJwk)?)
                    .ok_or(KeyError::Unsupported("JWK curve"))?;
                let group = EcGroup::from_curve_name(nid)?;
                let mut point = vec![4];
                point.extend(field("x")?);
                point.extend(field("y")?);
                let mut ctx = BigNumContext::new()?;
                let point = EcPoint::from_bytes(&group, &point, &mut ctx)?;
                if jwk.fields.contains_key("d") {
                    let d = bn("d")?;
                    let key = EcKey::from_private_components(&group, &d, &point)?;
                    key.check_key()?;
                    Self::Private(PKey::from_ec_key(key)?)
                } else {
                    let key = EcKey::from_public_key(&group, &point)?;
                    key.check_key()?;
                    Self::Public(PKey::from_ec_key(key)?)
                }
            }
            "OKP" => {
                let id = okp(jwk.crv.as_deref().ok_or(KeyError::InvalidJwk)?)
                    .ok_or(KeyError::Unsupported("JWK curve"))?;
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
            fields: BTreeMap::new(),
        };
        let fields = &mut jwk.fields;
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
                }
                ("RSA", None)
            }
            Some("ec") => {
                let k = self.public_ref().ec_key()?;
                let group = k.group();
                let crv = jwk_curve(
                    group
                        .curve_name()
                        .ok_or(KeyError::Unsupported("unnamed EC curve"))?,
                )
                .ok_or(KeyError::Unsupported("JWK curve"))?;
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
            _ => return Err(KeyError::Unsupported("JWK key type")),
        };
        jwk.kty = kty.into();
        jwk.crv = crv;
        Ok(jwk)
    }
    pub fn raw_public(&self, compressed: bool) -> Result<Vec<u8>, KeyError> {
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
        if self.key_type() == Some("ec") {
            let ec = key.ec_key()?;
            Ok(ec
                .private_key()
                .to_vec_padded(ec.group().degree().div_ceil(8) as i32)?)
        } else {
            Ok(key.raw_private_key()?)
        }
    }
}
