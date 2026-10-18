//! Shared owned streaming ABI behind `internalBinding('zlib')`; policy stays in Node's
//! lib/zlib.js. Each engine follows its node_zlib.cc context.
use crate::modules::zlib_constants::Mode;
#[path = "zlib_native_codec_zlib.rs"]
mod zlib;
#[cfg(quench_node_brotli)]
#[path = "zlib_native_brotli.rs"]
mod brotli;
#[cfg(not(quench_node_brotli))]
#[path = "zlib_native_brotli_rust.rs"]
mod brotli;
pub(crate) use zlib::version;
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
/// What a Brotli stream was created from; `ResetStream` recreates it with the same
/// dictionary and replays the same parameters.
pub(crate) struct BrotliRecipe {
    mode: Mode,
    params: Vec<u32>,
    dictionary: Option<Vec<u8>>,
}
pub(crate) enum Codec {
    Zlib(zlib::Codec),
    Zstd(zstd::Codec),
    Brotli(brotli::Codec, BrotliRecipe),
}
impl Codec {
    pub(in crate::modules) fn new(
        mode: Mode,
        bits: i32,
        level: i32,
        memory: i32,
        strategy: i32,
        dictionary: Option<Vec<u8>>,
        reject_garbage_after_end: bool,
    ) -> Result<Self, Failure> {
        zlib::Codec::new(mode, bits, level, memory, strategy, dictionary, reject_garbage_after_end).map(Self::Zlib)
    }
    pub(in crate::modules) fn brotli(
        mode: Mode,
        params: &[u32],
        dictionary: Option<Vec<u8>>,
    ) -> Result<Self, Failure> {
        let recipe = BrotliRecipe { mode, params: params.to_vec(), dictionary };
        brotli::Codec::new(mode, params, recipe.dictionary.clone()).map(|codec| Self::Brotli(codec, recipe))
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
            Self::Brotli(codec, _) => codec.write(flush, input, output),
        }
    }
    /// `ResetStream`: start a new stream with the same configuration.
    pub(crate) fn reset(&mut self) -> Result<(), Failure> {
        match self {
            Self::Zlib(codec) => codec.reset(),
            Self::Zstd(codec) => codec.reset(),
            Self::Brotli(codec, recipe) => {
                *codec = brotli::Codec::new(recipe.mode, &recipe.params, recipe.dictionary.clone())?;
                Ok(())
            }
        }
    }
    /// `params(level, strategy)`: zlib's `SetParams`; a no-op for Brotli and Zstd streams.
    pub(crate) fn params(&mut self, level: i32, strategy: i32) -> Result<(), Failure> {
        match self {
            Self::Zlib(codec) => codec.params(level, strategy),
            Self::Zstd(_) | Self::Brotli(..) => Ok(()),
        }
    }
}
