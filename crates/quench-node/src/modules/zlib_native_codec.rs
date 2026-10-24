//! Shared owned streaming ABI; policy stays in configured Node source.
use crate::modules::zlib_shared_vm::Codec as Mode;
#[cfg(quench_node_zlib)]
#[path = "zlib_native_codec_c.rs"]
mod backend;
#[cfg(not(quench_node_zlib))]
#[path = "zlib_native_codec_rust.rs"]
mod backend;
#[cfg(quench_node_brotli)]
#[path = "zlib_native_brotli.rs"]
mod brotli;
#[cfg(quench_node_zlib)]
pub(crate) use backend::version;
#[path = "zlib_native_zstd.rs"]
mod zstd;
pub(crate) struct Failure {
    pub(crate) errno: i32,
    pub(crate) code: String,
    pub(crate) message: String,
}
pub(crate) struct Step {
    pub(crate) consumed: usize,
    pub(crate) produced: usize,
}
pub(crate) enum Codec {
    Zlib(backend::Codec),
    Zstd(zstd::Codec),
    #[cfg(quench_node_brotli)]
    Brotli(brotli::Codec),
}
impl Codec {
    pub(crate) fn params(&mut self, level: i32, strategy: i32) -> Result<(), Failure> {
        match self {
            Self::Zlib(codec) => codec.params(level, strategy),
            Self::Zstd(_) => Err(Failure {
                errno: -2,
                code: "Z_STREAM_ERROR".into(),
                message: "stream error".into(),
            }),
            #[cfg(quench_node_brotli)]
            Self::Brotli(_) => Err(Failure {
                errno: -2,
                code: "Z_STREAM_ERROR".into(),
                message: "stream error".into(),
            }),
        }
    }

    pub(in crate::modules) fn new(
        mode: Mode,
        bits: i32,
        level: i32,
        memory: i32,
        strategy: i32,
        dictionary: Option<Vec<u8>>,
        reject_trailing: bool,
    ) -> Result<Self, Failure> {
        backend::Codec::new(
            mode,
            bits,
            level,
            memory,
            strategy,
            dictionary,
            reject_trailing,
        )
        .map(Self::Zlib)
    }
    #[cfg(quench_node_brotli)]
    pub(in crate::modules) fn brotli(
        mode: Mode,
        params: &[u32],
        dictionary: Option<Vec<u8>>,
    ) -> Result<Self, Failure> {
        brotli::Codec::new(mode, params, dictionary).map(Self::Brotli)
    }
    pub(in crate::modules) fn zstd(
        mode: Mode,
        params: &[u32],
        pledge: Option<u64>,
        dictionary: Option<Vec<u8>>,
        reject_trailing: bool,
    ) -> Result<Self, Failure> {
        zstd::Codec::new(mode, params, pledge, dictionary, reject_trailing).map(Self::Zstd)
    }
    pub(crate) fn write(
        &mut self,
        flush: i32,
        input: &[u8],
        output: &mut [u8],
    ) -> Result<Step, Failure> {
        match self {
            Self::Zlib(codec) => codec.write(flush, input, output),
            Self::Zstd(codec) => codec.write(flush, input, output),
            #[cfg(quench_node_brotli)]
            Self::Brotli(codec) => codec.write(flush, input, output),
        }
    }
}
