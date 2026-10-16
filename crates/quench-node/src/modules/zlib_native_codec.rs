//! Codec backends share the same owned streaming ABI.
#[cfg(quench_node_zlib)]
#[path = "zlib_native_codec_c.rs"]
mod backend;
#[cfg(not(quench_node_zlib))]
#[path = "zlib_native_codec_rust.rs"]
mod backend;
pub(crate) use backend::{Codec, Failure, Step};
#[cfg(quench_node_zlib)]
pub(crate) use backend::version;
