//! The key arrives only at finalization: own one incremental EVP digest context,
//! consume it at finalization, and sign/verify the resulting native digest.
use super::{
    crypto_asymmetric::{KeyError, OwnedKey},
    crypto_asymmetric_operations::{sign_prehashed, verify_prehashed, SignatureOptions},
    crypto_digest::{Digest, DigestError},
    crypto_digest_state::HashState,
};
use std::sync::Arc;
#[derive(Debug)]
pub(crate) enum Error {
    UnknownDigest,
    NotInitialized,
    UnsupportedOneShot,
    Digest(DigestError),
    Key(KeyError),
}
impl From<DigestError> for Error {
    fn from(error: DigestError) -> Self {
        Self::Digest(error)
    }
}
impl From<KeyError> for Error {
    fn from(error: KeyError) -> Self {
        Self::Key(error)
    }
}
#[derive(Default)]
pub(crate) struct State {
    digest: Option<Arc<Digest>>,
    context: Option<HashState>,
}
impl State {
    pub(crate) fn initialize(&mut self, name: &str) -> Result<(), Error> {
        let digest = Arc::new(Digest::from_sdk_name(name).map_err(|_| Error::UnknownDigest)?);
        let context = HashState::new(digest.clone(), None, None, None)?;
        self.digest = Some(digest);
        self.context = Some(context);
        Ok(())
    }
    pub(crate) fn update(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let context = self.context.as_mut().ok_or(Error::NotInitialized)?;
        if !context.update(bytes)? {
            return Err(Error::Digest(DigestError::OpenSsl(
                openssl::error::ErrorStack::get(),
            )));
        }
        Ok(())
    }
    fn consume(&mut self, key: &OwnedKey) -> Result<(Arc<Digest>, Vec<u8>), Error> {
        if super::crypto_provider_facts::key_algorithm_for(key.raw())
            .is_some_and(|algorithm| algorithm.is_one_shot())
        {
            return Err(Error::UnsupportedOneShot);
        }
        let mut context = self.context.take().ok_or(Error::NotInitialized)?;
        let digest = self.digest.take().ok_or(Error::NotInitialized)?;
        let data = context.digest()?;
        Ok((digest, data))
    }
    pub(crate) fn sign(
        &mut self,
        key: &OwnedKey,
        options: &SignatureOptions,
    ) -> Result<Vec<u8>, Error> {
        let (digest, data) = self.consume(key)?;
        Ok(sign_prehashed(key, &digest, &data, options)?)
    }
    pub(crate) fn verify(
        &mut self,
        key: &OwnedKey,
        signature: &[u8],
        options: &SignatureOptions,
    ) -> Result<bool, Error> {
        let (digest, data) = self.consume(key)?;
        Ok(verify_prehashed(key, &digest, &data, signature, options)?)
    }
}
