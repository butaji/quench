//! Node crypto_keys.h/ncrypto.h native key ABI. Public policy remains in SDK JS.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Secret = 0,
    Public = 1,
    Private = 2,
}
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    Der = 0,
    Pem = 1,
    Jwk = 2,
    RawPublic = 3,
    RawPrivate = 4,
    RawSeed = 5,
    Store = 6,
}
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Encoding {
    Pkcs1 = 0,
    Pkcs8 = 1,
    Spki = 2,
    Sec1 = 3,
}
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WebCryptoFormat {
    Raw = 0,
    Pkcs8 = 1,
    Spki = 2,
    Jwk = 3,
}
impl Format {
    pub(crate) fn from_native(value: u32) -> Option<Self> {
        Some(match value {
            0 => Self::Der,
            1 => Self::Pem,
            2 => Self::Jwk,
            3 => Self::RawPublic,
            4 => Self::RawPrivate,
            5 => Self::RawSeed,
            6 => Self::Store,
            _ => return None,
        })
    }
}
impl Encoding {
    pub(crate) fn from_native(value: u32) -> Option<Self> {
        Some(match value {
            0 => Self::Pkcs1,
            1 => Self::Pkcs8,
            2 => Self::Spki,
            3 => Self::Sec1,
            _ => return None,
        })
    }
    #[cfg(quench_openssl_provider)]
    pub(crate) fn provider(self) -> super::crypto_asymmetric::Encoding {
        use super::crypto_asymmetric::Encoding as E;
        match self {
            Self::Pkcs1 => E::Pkcs1,
            Self::Pkcs8 => E::Pkcs8,
            Self::Spki => E::Spki,
            Self::Sec1 => E::Sec1,
        }
    }
}
pub(crate) const CONSTANTS: &[(&str, u32)] = &[
    ("kKeyTypeSecret", Kind::Secret as u32),
    ("kKeyTypePublic", Kind::Public as u32),
    ("kKeyTypePrivate", Kind::Private as u32),
    ("kWebCryptoKeyFormatRaw", WebCryptoFormat::Raw as u32),
    ("kWebCryptoKeyFormatPKCS8", WebCryptoFormat::Pkcs8 as u32),
    ("kWebCryptoKeyFormatSPKI", WebCryptoFormat::Spki as u32),
    ("kWebCryptoKeyFormatJWK", WebCryptoFormat::Jwk as u32),
    ("kKeyFormatDER", Format::Der as u32),
    ("kKeyFormatPEM", Format::Pem as u32),
    ("kKeyFormatJWK", Format::Jwk as u32),
    ("kKeyFormatRawPublic", Format::RawPublic as u32),
    ("kKeyFormatRawPrivate", Format::RawPrivate as u32),
    ("kKeyFormatRawSeed", Format::RawSeed as u32),
    ("kKeyFormatStore", Format::Store as u32),
    ("kKeyEncodingPKCS1", Encoding::Pkcs1 as u32),
    ("kKeyEncodingPKCS8", Encoding::Pkcs8 as u32),
    ("kKeyEncodingSPKI", Encoding::Spki as u32),
    ("kKeyEncodingSEC1", Encoding::Sec1 as u32),
];
