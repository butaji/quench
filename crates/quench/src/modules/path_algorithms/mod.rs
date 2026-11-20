//! One runtime-neutral owner for Node POSIX and Win32 path string semantics.

pub(crate) mod common;
pub(crate) mod parts;
pub(crate) mod posix;
pub(crate) mod win32;
pub(crate) mod win32_extra;
pub(crate) mod win32_normalize;
