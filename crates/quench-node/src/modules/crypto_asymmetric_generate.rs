//! Native provider key generation. Owns every context and the resulting EVP_PKEY.
use super::crypto_asymmetric::{KeyError, OwnedKey};
use super::crypto_digest::Digest;
use foreign_types::{ForeignType, ForeignTypeRef};
use openssl::{
    bn::BigNum,
    dh::Dh,
    error::ErrorStack,
    pkey::{PKey, Private},
};
use std::ffi::{c_char, c_int, c_void, CString};
pub struct Pss<'a> {
    pub digest: Option<&'a Digest>,
    pub mgf1_digest: Option<&'a Digest>,
    pub salt_length: Option<u32>,
}
pub enum DhParameters<'a> {
    Group(&'a str),
    Prime(&'a [u8]),
    Bits(u32),
}
pub enum Generation<'a> {
    Rsa {
        bits: u32,
        exponent: u32,
        pss: Option<Pss<'a>>,
    },
    Ec {
        curve: &'a str,
        explicit: bool,
    },
    Dsa {
        bits: u32,
        divisor_bits: Option<u32>,
    },
    Dh {
        params: DhParameters<'a>,
        generator: u32,
    },
    Named(&'a str),
}
unsafe extern "C" {
    fn OBJ_length(object: *const openssl_sys::ASN1_OBJECT) -> usize;
    fn EVP_PKEY_CTX_set_rsa_pss_keygen_md(
        ctx: *mut openssl_sys::EVP_PKEY_CTX,
        md: *const openssl_sys::EVP_MD,
    ) -> c_int;
    fn EVP_PKEY_CTX_set_rsa_pss_keygen_mgf1_md(
        ctx: *mut openssl_sys::EVP_PKEY_CTX,
        md: *const openssl_sys::EVP_MD,
    ) -> c_int;
    fn EVP_PKEY_CTX_new_from_name(
        lib: *mut c_void,
        name: *const c_char,
        props: *const c_char,
    ) -> *mut c_void;
    fn EVP_PKEY_CTX_ctrl_str(
        ctx: *mut openssl_sys::EVP_PKEY_CTX,
        name: *const c_char,
        value: *const c_char,
    ) -> c_int;
    fn BN_get_rfc2409_prime_768(p: *mut openssl_sys::BIGNUM) -> *mut openssl_sys::BIGNUM;
    fn BN_get_rfc2409_prime_1024(p: *mut openssl_sys::BIGNUM) -> *mut openssl_sys::BIGNUM;
    fn BN_get_rfc3526_prime_1536(p: *mut openssl_sys::BIGNUM) -> *mut openssl_sys::BIGNUM;
    fn BN_get_rfc3526_prime_2048(p: *mut openssl_sys::BIGNUM) -> *mut openssl_sys::BIGNUM;
    fn BN_get_rfc3526_prime_3072(p: *mut openssl_sys::BIGNUM) -> *mut openssl_sys::BIGNUM;
    fn BN_get_rfc3526_prime_4096(p: *mut openssl_sys::BIGNUM) -> *mut openssl_sys::BIGNUM;
    fn BN_get_rfc3526_prime_6144(p: *mut openssl_sys::BIGNUM) -> *mut openssl_sys::BIGNUM;
    fn BN_get_rfc3526_prime_8192(p: *mut openssl_sys::BIGNUM) -> *mut openssl_sys::BIGNUM;
}
fn encodable(d: &Digest) -> Result<(), KeyError> {
    let nid = unsafe { openssl_sys::EVP_MD_type(d.native_md().as_ptr()) };
    let obj = unsafe { openssl_sys::OBJ_nid2obj(nid) };
    if nid == 0 || obj.is_null() || unsafe { OBJ_length(obj) } == 0 {
        Err(KeyError::Unsupported("Key generation failed"))
    } else {
        Ok(())
    }
}
fn native(ok: c_int) -> Result<(), KeyError> {
    if ok > 0 {
        Ok(())
    } else {
        Err(ErrorStack::get().into())
    }
}
fn text(s: &str) -> CString {
    CString::new(s.split('\0').next().unwrap()).unwrap()
}
struct Context(*mut openssl_sys::EVP_PKEY_CTX);
impl Drop for Context {
    fn drop(&mut self) {
        unsafe { openssl_sys::EVP_PKEY_CTX_free(self.0) }
    }
}
impl Context {
    fn named(name: &str) -> Result<Self, KeyError> {
        let name = text(name);
        let p = unsafe {
            EVP_PKEY_CTX_new_from_name(std::ptr::null_mut(), name.as_ptr(), std::ptr::null())
        };
        if p.is_null() {
            Err(ErrorStack::get().into())
        } else {
            Ok(Self(p.cast()))
        }
    }
    fn control(&self, name: &str, value: &str) -> Result<(), KeyError> {
        let n = text(name);
        let v = text(value);
        native(unsafe { EVP_PKEY_CTX_ctrl_str(self.0, n.as_ptr(), v.as_ptr()) })
    }
    fn keygen(&self) -> Result<PKey<Private>, KeyError> {
        let mut p = std::ptr::null_mut();
        native(unsafe { openssl_sys::EVP_PKEY_keygen(self.0, &mut p) })?;
        Ok(unsafe { PKey::from_ptr(p) })
    }
    fn from_params(&self) -> Result<PKey<Private>, KeyError> {
        let mut p = std::ptr::null_mut();
        native(unsafe { openssl_sys::EVP_PKEY_paramgen(self.0, &mut p) })?;
        let params = unsafe { PKey::<openssl::pkey::Params>::from_ptr(p) };
        let ctx =
            Self(unsafe { openssl_sys::EVP_PKEY_CTX_new(params.as_ptr(), std::ptr::null_mut()) });
        if ctx.0.is_null() {
            return Err(ErrorStack::get().into());
        }
        native(unsafe { openssl_sys::EVP_PKEY_keygen_init(ctx.0) })?;
        ctx.keygen()
    }
}
pub(crate) fn standard_group(name: &str) -> Result<BigNum, KeyError> {
    let lower = name.to_ascii_lowercase();
    let f = match lower.as_str() {
        "modp1" => BN_get_rfc2409_prime_768,
        "modp2" => BN_get_rfc2409_prime_1024,
        "modp5" => BN_get_rfc3526_prime_1536,
        "modp14" => BN_get_rfc3526_prime_2048,
        "modp15" => BN_get_rfc3526_prime_3072,
        "modp16" => BN_get_rfc3526_prime_4096,
        "modp17" => BN_get_rfc3526_prime_6144,
        "modp18" => BN_get_rfc3526_prime_8192,
        _ => return Err(KeyError::Unsupported("unknown DH group")),
    };
    let p = unsafe { f(std::ptr::null_mut()) };
    if p.is_null() {
        Err(ErrorStack::get().into())
    } else {
        Ok(unsafe { BigNum::from_ptr(p) })
    }
}
impl OwnedKey {
    pub fn generate(config: Generation<'_>) -> Result<Self, KeyError> {
        let key = match config {
            Generation::Named(name) => {
                let c = Context::named(name)?;
                native(unsafe { openssl_sys::EVP_PKEY_keygen_init(c.0) })?;
                c.keygen()?
            }
            Generation::Rsa {
                bits,
                exponent,
                pss,
            } => {
                let c = Context::named(if pss.is_some() { "RSA-PSS" } else { "RSA" })?;
                native(unsafe { openssl_sys::EVP_PKEY_keygen_init(c.0) })?;
                c.control("rsa_keygen_bits", &bits.to_string())?;
                c.control("rsa_keygen_pubexp", &exponent.to_string())?;
                if let Some(p) = pss {
                    if let Some(d) = p.digest {
                        encodable(d)?;
                        native(unsafe {
                            EVP_PKEY_CTX_set_rsa_pss_keygen_md(c.0, d.native_md().as_ptr())
                        })?
                    }
                    if let Some(d) = p.mgf1_digest.or(p.digest) {
                        encodable(d)?;
                        native(unsafe {
                            EVP_PKEY_CTX_set_rsa_pss_keygen_mgf1_md(c.0, d.native_md().as_ptr())
                        })?
                    }
                    if let Some(n) = p.salt_length.or_else(|| p.digest.map(|d| d.size() as u32)) {
                        c.control("rsa_pss_keygen_saltlen", &n.to_string())?
                    }
                }
                c.keygen()?
            }
            Generation::Ec { curve, explicit } => {
                let c = Context::named("EC")?;
                native(unsafe { openssl_sys::EVP_PKEY_keygen_init(c.0) })?;
                c.control("ec_paramgen_curve", curve)?;
                c.control(
                    "ec_param_enc",
                    if explicit { "explicit" } else { "named_curve" },
                )?;
                c.keygen()?
            }
            Generation::Dsa { bits, divisor_bits } => {
                let c = Context::named("DSA")?;
                native(unsafe { openssl_sys::EVP_PKEY_paramgen_init(c.0) })?;
                c.control("dsa_paramgen_bits", &bits.to_string())?;
                if let Some(n) = divisor_bits {
                    c.control("dsa_paramgen_q_bits", &n.to_string())?
                }
                c.from_params()?
            }
            Generation::Dh { params, generator } => {
                let params = match params {
                    DhParameters::Bits(bits) => Dh::generate_params(bits, generator)?,
                    DhParameters::Prime(p) => {
                        Dh::from_pqg(BigNum::from_slice(p)?, None, BigNum::from_u32(generator)?)?
                    }
                    DhParameters::Group(name) => {
                        Dh::from_pqg(standard_group(name)?, None, BigNum::from_u32(2)?)?
                    }
                };
                PKey::from_dh(params.generate_key()?)?
            }
        };
        Ok(Self::Private(key))
    }
}

unsafe extern "C" {
    fn EC_curve_nist2nid(name: *const c_char) -> c_int;
}
pub(crate) fn curve_available(name: &str) -> bool {
    let _errors = super::crypto_provider_facts::ErrorScope::new();
    let name = text(name);
    let nid = unsafe { EC_curve_nist2nid(name.as_ptr()) };
    if nid != 0 || unsafe { openssl_sys::OBJ_sn2nid(name.as_ptr()) } != 0 {
        return true;
    }
    let raw = unsafe {
        EVP_PKEY_CTX_new_from_name(std::ptr::null_mut(), c"EC".as_ptr(), std::ptr::null())
    };
    if raw.is_null() {
        return false;
    }
    let context = Context(raw.cast());
    if unsafe { openssl_sys::EVP_PKEY_paramgen_init(context.0) } <= 0 {
        return false;
    }
    if unsafe { EVP_PKEY_CTX_ctrl_str(context.0, c"ec_paramgen_curve".as_ptr(), name.as_ptr()) }
        <= 0
        || unsafe {
            EVP_PKEY_CTX_ctrl_str(context.0, c"ec_param_enc".as_ptr(), c"named_curve".as_ptr())
        } <= 0
    {
        return false;
    }
    let mut parameters = std::ptr::null_mut();
    let ok = unsafe { openssl_sys::EVP_PKEY_paramgen(context.0, &mut parameters) };
    if !parameters.is_null() {
        unsafe { openssl_sys::EVP_PKEY_free(parameters) }
    }
    ok > 0
}
