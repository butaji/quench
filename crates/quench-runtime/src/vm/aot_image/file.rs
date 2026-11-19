//! The file form of an `AotImage` (it replaces `AheadImages::to_bytes`): magic, build id,
//! fingerprint, then the recipe tables, atom texts and per function its base and lane code
//! with relocations, every number little-endian.
//!
//! Owner: one pipeline.

use super::AotImage;

pub(crate) fn to_bytes(image: &AotImage) -> Vec<u8> {
    let _ = image;
    todo!("one pipeline: file form")
}

pub(crate) fn from_bytes(bytes: &[u8]) -> Result<AotImage, String> {
    let _ = bytes;
    todo!("one pipeline: file form")
}
