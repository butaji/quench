//! `internalBinding('zlib')`'s mode ABI and `internalBinding('constants').zlib`
//! (src/node_zlib.cc `node_zlib_mode`, `DefineZlibConstants`).
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

/// node_zlib.cc `node_zlib_mode`: the numbers lib/zlib.js passes to the handles.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum Mode {
    Deflate = 1,
    Inflate = 2,
    Gzip = 3,
    Gunzip = 4,
    DeflateRaw = 5,
    InflateRaw = 6,
    Unzip = 7,
    BrotliDecompress = 8,
    BrotliCompress = 9,
    ZstdCompress = 10,
    ZstdDecompress = 11,
}

impl Mode {
    /// The modes a `binding.Zlib` handle accepts.
    pub(crate) const ZLIB: [Self; 7] =
        [Self::Deflate, Self::Inflate, Self::Gzip, Self::Gunzip, Self::DeflateRaw, Self::InflateRaw, Self::Unzip];
}

/// Marks the entry whose value comes from the linked zlib's version, not the table.
const LINKED_ZLIB_VERNUM: f64 = f64::NAN;

/// Node's constants in Node's order.
const ZLIB_CONSTANTS: &[(&str, f64)] = &[
    ("Z_NO_FLUSH", 0.0),
    ("Z_PARTIAL_FLUSH", 1.0),
    ("Z_SYNC_FLUSH", 2.0),
    ("Z_FULL_FLUSH", 3.0),
    ("Z_FINISH", 4.0),
    ("Z_BLOCK", 5.0),
    ("Z_OK", 0.0),
    ("Z_STREAM_END", 1.0),
    ("Z_NEED_DICT", 2.0),
    ("Z_ERRNO", -1.0),
    ("Z_STREAM_ERROR", -2.0),
    ("Z_DATA_ERROR", -3.0),
    ("Z_MEM_ERROR", -4.0),
    ("Z_BUF_ERROR", -5.0),
    ("Z_VERSION_ERROR", -6.0),
    ("Z_NO_COMPRESSION", 0.0),
    ("Z_BEST_SPEED", 1.0),
    ("Z_BEST_COMPRESSION", 9.0),
    ("Z_DEFAULT_COMPRESSION", -1.0),
    ("Z_FILTERED", 1.0),
    ("Z_HUFFMAN_ONLY", 2.0),
    ("Z_RLE", 3.0),
    ("Z_FIXED", 4.0),
    ("Z_DEFAULT_STRATEGY", 0.0),
    ("ZLIB_VERNUM", LINKED_ZLIB_VERNUM),
    ("DEFLATE", 1.0),
    ("INFLATE", 2.0),
    ("GZIP", 3.0),
    ("GUNZIP", 4.0),
    ("DEFLATERAW", 5.0),
    ("INFLATERAW", 6.0),
    ("UNZIP", 7.0),
    ("BROTLI_DECODE", 8.0),
    ("BROTLI_ENCODE", 9.0),
    ("ZSTD_DECOMPRESS", 11.0),
    ("ZSTD_COMPRESS", 10.0),
    ("Z_MIN_WINDOWBITS", 8.0),
    ("Z_MAX_WINDOWBITS", 15.0),
    ("Z_DEFAULT_WINDOWBITS", 15.0),
    ("Z_MIN_CHUNK", 64.0),
    ("Z_MAX_CHUNK", f64::INFINITY),
    ("Z_DEFAULT_CHUNK", 16384.0),
    ("Z_MIN_MEMLEVEL", 1.0),
    ("Z_MAX_MEMLEVEL", 9.0),
    ("Z_DEFAULT_MEMLEVEL", 8.0),
    ("Z_MIN_LEVEL", -1.0),
    ("Z_MAX_LEVEL", 9.0),
    ("Z_DEFAULT_LEVEL", -1.0),
    ("BROTLI_OPERATION_PROCESS", 0.0),
    ("BROTLI_OPERATION_FLUSH", 1.0),
    ("BROTLI_OPERATION_FINISH", 2.0),
    ("BROTLI_OPERATION_EMIT_METADATA", 3.0),
    ("BROTLI_PARAM_MODE", 0.0),
    ("BROTLI_MODE_GENERIC", 0.0),
    ("BROTLI_MODE_TEXT", 1.0),
    ("BROTLI_MODE_FONT", 2.0),
    ("BROTLI_DEFAULT_MODE", 0.0),
    ("BROTLI_PARAM_QUALITY", 1.0),
    ("BROTLI_MIN_QUALITY", 0.0),
    ("BROTLI_MAX_QUALITY", 11.0),
    ("BROTLI_DEFAULT_QUALITY", 11.0),
    ("BROTLI_PARAM_LGWIN", 2.0),
    ("BROTLI_MIN_WINDOW_BITS", 10.0),
    ("BROTLI_MAX_WINDOW_BITS", 24.0),
    ("BROTLI_LARGE_MAX_WINDOW_BITS", 30.0),
    ("BROTLI_DEFAULT_WINDOW", 22.0),
    ("BROTLI_PARAM_LGBLOCK", 3.0),
    ("BROTLI_MIN_INPUT_BLOCK_BITS", 16.0),
    ("BROTLI_MAX_INPUT_BLOCK_BITS", 24.0),
    ("BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING", 4.0),
    ("BROTLI_PARAM_SIZE_HINT", 5.0),
    ("BROTLI_PARAM_LARGE_WINDOW", 6.0),
    ("BROTLI_PARAM_NPOSTFIX", 7.0),
    ("BROTLI_PARAM_NDIRECT", 8.0),
    ("BROTLI_DECODER_RESULT_ERROR", 0.0),
    ("BROTLI_DECODER_RESULT_SUCCESS", 1.0),
    ("BROTLI_DECODER_RESULT_NEEDS_MORE_INPUT", 2.0),
    ("BROTLI_DECODER_RESULT_NEEDS_MORE_OUTPUT", 3.0),
    ("BROTLI_DECODER_PARAM_DISABLE_RING_BUFFER_REALLOCATION", 0.0),
    ("BROTLI_DECODER_PARAM_LARGE_WINDOW", 1.0),
    ("BROTLI_DECODER_NO_ERROR", 0.0),
    ("BROTLI_DECODER_SUCCESS", 1.0),
    ("BROTLI_DECODER_NEEDS_MORE_INPUT", 2.0),
    ("BROTLI_DECODER_NEEDS_MORE_OUTPUT", 3.0),
    ("BROTLI_DECODER_ERROR_FORMAT_EXUBERANT_NIBBLE", -1.0),
    ("BROTLI_DECODER_ERROR_FORMAT_RESERVED", -2.0),
    ("BROTLI_DECODER_ERROR_FORMAT_EXUBERANT_META_NIBBLE", -3.0),
    ("BROTLI_DECODER_ERROR_FORMAT_SIMPLE_HUFFMAN_ALPHABET", -4.0),
    ("BROTLI_DECODER_ERROR_FORMAT_SIMPLE_HUFFMAN_SAME", -5.0),
    ("BROTLI_DECODER_ERROR_FORMAT_CL_SPACE", -6.0),
    ("BROTLI_DECODER_ERROR_FORMAT_HUFFMAN_SPACE", -7.0),
    ("BROTLI_DECODER_ERROR_FORMAT_CONTEXT_MAP_REPEAT", -8.0),
    ("BROTLI_DECODER_ERROR_FORMAT_BLOCK_LENGTH_1", -9.0),
    ("BROTLI_DECODER_ERROR_FORMAT_BLOCK_LENGTH_2", -10.0),
    ("BROTLI_DECODER_ERROR_FORMAT_TRANSFORM", -11.0),
    ("BROTLI_DECODER_ERROR_FORMAT_DICTIONARY", -12.0),
    ("BROTLI_DECODER_ERROR_FORMAT_WINDOW_BITS", -13.0),
    ("BROTLI_DECODER_ERROR_FORMAT_PADDING_1", -14.0),
    ("BROTLI_DECODER_ERROR_FORMAT_PADDING_2", -15.0),
    ("BROTLI_DECODER_ERROR_FORMAT_DISTANCE", -16.0),
    ("BROTLI_DECODER_ERROR_DICTIONARY_NOT_SET", -19.0),
    ("BROTLI_DECODER_ERROR_INVALID_ARGUMENTS", -20.0),
    ("BROTLI_DECODER_ERROR_ALLOC_CONTEXT_MODES", -21.0),
    ("BROTLI_DECODER_ERROR_ALLOC_TREE_GROUPS", -22.0),
    ("BROTLI_DECODER_ERROR_ALLOC_CONTEXT_MAP", -25.0),
    ("BROTLI_DECODER_ERROR_ALLOC_RING_BUFFER_1", -26.0),
    ("BROTLI_DECODER_ERROR_ALLOC_RING_BUFFER_2", -27.0),
    ("BROTLI_DECODER_ERROR_ALLOC_BLOCK_TYPE_TREES", -30.0),
    ("BROTLI_DECODER_ERROR_UNREACHABLE", -31.0),
    ("ZSTD_e_continue", 0.0),
    ("ZSTD_e_flush", 1.0),
    ("ZSTD_e_end", 2.0),
    ("ZSTD_fast", 1.0),
    ("ZSTD_dfast", 2.0),
    ("ZSTD_greedy", 3.0),
    ("ZSTD_lazy", 4.0),
    ("ZSTD_lazy2", 5.0),
    ("ZSTD_btlazy2", 6.0),
    ("ZSTD_btopt", 7.0),
    ("ZSTD_btultra", 8.0),
    ("ZSTD_btultra2", 9.0),
    ("ZSTD_c_compressionLevel", 100.0),
    ("ZSTD_c_windowLog", 101.0),
    ("ZSTD_c_hashLog", 102.0),
    ("ZSTD_c_chainLog", 103.0),
    ("ZSTD_c_searchLog", 104.0),
    ("ZSTD_c_minMatch", 105.0),
    ("ZSTD_c_targetLength", 106.0),
    ("ZSTD_c_strategy", 107.0),
    ("ZSTD_c_enableLongDistanceMatching", 160.0),
    ("ZSTD_c_ldmHashLog", 161.0),
    ("ZSTD_c_ldmMinMatch", 162.0),
    ("ZSTD_c_ldmBucketSizeLog", 163.0),
    ("ZSTD_c_ldmHashRateLog", 164.0),
    ("ZSTD_c_contentSizeFlag", 200.0),
    ("ZSTD_c_checksumFlag", 201.0),
    ("ZSTD_c_dictIDFlag", 202.0),
    ("ZSTD_c_nbWorkers", 400.0),
    ("ZSTD_c_jobSize", 401.0),
    ("ZSTD_c_overlapLog", 402.0),
    ("ZSTD_d_windowLogMax", 100.0),
    ("ZSTD_CLEVEL_DEFAULT", 3.0),
    ("ZSTD_error_no_error", 0.0),
    ("ZSTD_error_GENERIC", 1.0),
    ("ZSTD_error_prefix_unknown", 10.0),
    ("ZSTD_error_version_unsupported", 12.0),
    ("ZSTD_error_frameParameter_unsupported", 14.0),
    ("ZSTD_error_frameParameter_windowTooLarge", 16.0),
    ("ZSTD_error_corruption_detected", 20.0),
    ("ZSTD_error_checksum_wrong", 22.0),
    ("ZSTD_error_literals_headerWrong", 24.0),
    ("ZSTD_error_dictionary_corrupted", 30.0),
    ("ZSTD_error_dictionary_wrong", 32.0),
    ("ZSTD_error_dictionaryCreation_failed", 34.0),
    ("ZSTD_error_parameter_unsupported", 40.0),
    ("ZSTD_error_parameter_combination_unsupported", 41.0),
    ("ZSTD_error_parameter_outOfBound", 42.0),
    ("ZSTD_error_tableLog_tooLarge", 44.0),
    ("ZSTD_error_maxSymbolValue_tooLarge", 46.0),
    ("ZSTD_error_maxSymbolValue_tooSmall", 48.0),
    ("ZSTD_error_stabilityCondition_notRespected", 50.0),
    ("ZSTD_error_stage_wrong", 60.0),
    ("ZSTD_error_init_missing", 62.0),
    ("ZSTD_error_memory_allocation", 64.0),
    ("ZSTD_error_workSpace_tooSmall", 66.0),
    ("ZSTD_error_dstSize_tooSmall", 70.0),
    ("ZSTD_error_srcSize_wrong", 72.0),
    ("ZSTD_error_dstBuffer_null", 74.0),
    ("ZSTD_error_noForwardProgress_destFull", 80.0),
    ("ZSTD_error_noForwardProgress_inputEmpty", 82.0),
];

/// zlib.h `ZLIB_VERNUM` (0xMmrs) of a `major.minor.revision[.subrevision]` version.
fn vernum(version: &str) -> f64 {
    const NIBBLE_BITS: u32 = 4;
    const NIBBLES: usize = 4;
    let digits = version.split(|c: char| !c.is_ascii_digit()).filter(|part| !part.is_empty()).take(NIBBLES);
    let mut number = 0u32;
    let mut count = 0;
    for digit in digits {
        number = (number << NIBBLE_BITS) | digit.parse::<u32>().unwrap_or(0).min((1 << NIBBLE_BITS) - 1);
        count += 1;
    }
    f64::from(number << (NIBBLE_BITS * (NIBBLES - count) as u32))
}

/// `internalBinding('constants').zlib`: read-only, non-deletable (NODE_DEFINE_CONSTANT).
pub(crate) fn constants(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    const CACHE_KEY: &str = "\0builtin:zlib/constants";
    let shared = context.host_mut().shared_state();
    if let Some(value) = shared.borrow().builtin_cache.get(CACHE_KEY).copied() {
        return Ok(value);
    }
    let object = context.object_rooted()?;
    for (name, number) in ZLIB_CONSTANTS {
        let number = if number.is_nan() { vernum(&super::zlib_native_codec::version()) } else { *number };
        let key = context.string_rooted(name);
        let value = context.number(number);
        context.define_data_property_rooted(object, key, value, false, true, false)?;
    }
    let retained = context.retain(object)?;
    shared.borrow_mut().builtin_cache.insert(CACHE_KEY.to_owned(), retained);
    Ok(object)
}

#[cfg(test)]
mod tests {
    #[test]
    fn vernum_packs_version_nibbles() {
        assert_eq!(super::vernum("1.3.1"), f64::from(0x1310));
        assert_eq!(super::vernum("1.3.1.1-motley"), f64::from(0x1311));
        assert_eq!(super::vernum("1.3.1-zlib-rs-0.6.8"), f64::from(0x1310));
    }
}
