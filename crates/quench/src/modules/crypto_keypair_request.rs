//! Native-only keypair request state; no JS roots cross the pool boundary.
use super::{
    crypto_asymmetric::{Encoding, Format, KeyError, OwnedKey},
    crypto_asymmetric_generate::{DhParameters, Generation, Pss},
    crypto_digest::Digest,
    crypto_secret_material::Material,
};
use std::sync::Arc;
pub(crate) enum Family {
    Rsa,
    Ec,
    Dsa,
    Dh,
    Named,
}
pub(crate) enum Parameters {
    Rsa {
        bits: u32,
        exponent: u32,
        pss: Option<PssParameters>,
    },
    Ec {
        curve: String,
        explicit: bool,
    },
    Dsa {
        bits: u32,
        divisor_bits: Option<u32>,
    },
    Dh {
        params: DhOwned,
        generator: u32,
    },
    Named(String),
}
pub(crate) struct PssParameters {
    pub digest: Option<Arc<Digest>>,
    pub mgf1: Option<Arc<Digest>>,
    pub salt: Option<u32>,
}
pub(crate) enum DhOwned {
    Group(String),
    Prime(Vec<u8>),
    Bits(u32),
}
pub(crate) enum OutputEncoding {
    Bytes(Format, Encoding),
    Jwk,
    RawPublic { compressed: bool },
    RawPrivate,
    RawSeed,
}
pub(crate) struct EncodingOptions {
    pub public: Option<OutputEncoding>,
    pub private: Option<OutputEncoding>,
    pub cipher: Option<String>,
    pub passphrase: Option<Material>,
}
pub(crate) struct Request {
    pub parameters: Parameters,
    pub encoding: EncodingOptions,
}
pub(crate) struct Pair {
    pub public: Arc<OwnedKey>,
    pub private: Arc<OwnedKey>,
    pub encoding: EncodingOptions,
}
impl Request {
    pub(crate) fn run(self) -> Result<Pair, KeyError> {
        let native = match &self.parameters {
            Parameters::Rsa {
                bits,
                exponent,
                pss,
            } => Generation::Rsa {
                bits: *bits,
                exponent: *exponent,
                pss: pss.as_ref().map(|p| Pss {
                    digest: p.digest.as_deref(),
                    mgf1_digest: p.mgf1.as_deref(),
                    salt_length: p.salt,
                }),
            },
            Parameters::Ec { curve, explicit } => Generation::Ec {
                curve,
                explicit: *explicit,
            },
            Parameters::Dsa { bits, divisor_bits } => Generation::Dsa {
                bits: *bits,
                divisor_bits: *divisor_bits,
            },
            Parameters::Dh { params, generator } => Generation::Dh {
                params: match params {
                    DhOwned::Group(name) => DhParameters::Group(name),
                    DhOwned::Prime(bytes) => DhParameters::Prime(bytes),
                    DhOwned::Bits(bits) => DhParameters::Bits(*bits),
                },
                generator: *generator,
            },
            Parameters::Named(name) => Generation::Named(name),
        };
        let private = Arc::new(OwnedKey::generate(native)?);
        let public = Arc::new(private.to_public()?);
        Ok(Pair {
            public,
            private,
            encoding: self.encoding,
        })
    }
}
