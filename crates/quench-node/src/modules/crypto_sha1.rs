//! Shared SHA-1 digest primitive for the legacy and shared Node crypto adapters.

use sha1::{Digest, Sha1};

pub(crate) fn digest(input: &[u8]) -> Vec<u8> {
    Sha1::digest(input).to_vec()
}
