//! Portable Brotli streaming engine for hosts without the matching C library. The
//! decoder accepts Node's raw dictionaries; this encoder has only the legacy prefix
//! dictionary, whose output Node's compound-dictionary decoder rejects, so encoding with
//! a dictionary reports Node's dictionary load failure instead.
use super::{Failure, Step};
use crate::modules::zlib_constants::Mode;
use brotli::enc::encode::{BrotliEncoderOperation, BrotliEncoderParameter, BrotliEncoderStateStruct};
use brotli::enc::StandardAlloc;
use brotli::{Allocator, BrotliDecompressStream, BrotliResult, BrotliState, HeapAlloc, HuffmanCode, SliceWrapperMut};

/// Node's `kNoParam` marker for unset entries of the parameter array.
const PARAM_UNSET: u32 = u32::MAX;
/// `BROTLI_OPERATION_FINISH`, the flush Node passes for the final write.
const FINISH: i32 = BrotliEncoderOperation::BROTLI_OPERATION_FINISH as i32;
/// `BROTLI_DECODER_PARAM_LARGE_WINDOW`.
const DECODER_PARAM_LARGE_WINDOW: usize = 1;
/// The library's error-code names carry this prefix; Node's `ERR_` codes follow the C
/// `BrotliDecoderErrorString`, which keeps the rest (`_ERROR_FORMAT_PADDING_2`).
const DECODER_ERROR_PREFIX: &str = "BROTLI_DECODER";
/// Node reports a truncated stream as zlib's `Z_BUF_ERROR`.
const Z_BUF_ERROR: i32 = -5;
/// The encoder parameters, by the index Node's parameter array uses.
const ENCODER_PARAMETERS: [BrotliEncoderParameter; 7] = [
    BrotliEncoderParameter::BROTLI_PARAM_MODE,
    BrotliEncoderParameter::BROTLI_PARAM_QUALITY,
    BrotliEncoderParameter::BROTLI_PARAM_LGWIN,
    BrotliEncoderParameter::BROTLI_PARAM_LGBLOCK,
    BrotliEncoderParameter::BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING,
    BrotliEncoderParameter::BROTLI_PARAM_SIZE_HINT,
    BrotliEncoderParameter::BROTLI_PARAM_LARGE_WINDOW,
];

type Decoder = BrotliState<HeapAlloc<u8>, HeapAlloc<u32>, HeapAlloc<HuffmanCode>>;

pub(crate) enum Codec {
    Encoder(Box<BrotliEncoderStateStruct<StandardAlloc>>),
    Decoder(Box<Decoder>),
}

fn failure(message: &str, code: &str) -> Failure {
    Failure {
        errno: -1,
        code: code.into(),
        message: message.into(),
    }
}

fn operation(flush: i32) -> Option<BrotliEncoderOperation> {
    [
        BrotliEncoderOperation::BROTLI_OPERATION_PROCESS,
        BrotliEncoderOperation::BROTLI_OPERATION_FLUSH,
        BrotliEncoderOperation::BROTLI_OPERATION_FINISH,
        BrotliEncoderOperation::BROTLI_OPERATION_EMIT_METADATA,
    ]
    .into_iter()
    .find(|operation| *operation as i32 == flush)
}

impl Codec {
    pub(in crate::modules) fn new(
        mode: Mode,
        params: &[u32],
        dictionary: Option<Vec<u8>>,
    ) -> Result<Self, Failure> {
        let dictionary = dictionary.filter(|dictionary| !dictionary.is_empty());
        let set = params
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, value)| *value != PARAM_UNSET);
        if matches!(mode, Mode::BrotliCompress) {
            let mut encoder = Box::new(BrotliEncoderStateStruct::new(StandardAlloc::default()));
            for (index, value) in set {
                let accepted = ENCODER_PARAMETERS
                    .get(index)
                    .is_some_and(|parameter| encoder.set_parameter(*parameter, value));
                if !accepted {
                    return Err(failure("Setting parameter failed", "ERR_BROTLI_PARAM_SET_FAILED"));
                }
            }
            if dictionary.is_some() {
                return Err(failure("Failed to attach brotli dictionary", "ERR_ZLIB_DICTIONARY_LOAD_FAILED"));
            }
            return Ok(Self::Encoder(encoder));
        }
        let mut decoder = Box::new(match dictionary {
            Some(dictionary) => {
                let mut prefix = HeapAlloc::<u8>::default().alloc_cell(dictionary.len());
                prefix.slice_mut().copy_from_slice(&dictionary);
                Decoder::new_with_custom_dictionary(HeapAlloc::default(), HeapAlloc::default(), HeapAlloc::default(), prefix)
            }
            None => Decoder::new(HeapAlloc::default(), HeapAlloc::default(), HeapAlloc::default()),
        });
        for (index, value) in set {
            // Ring-buffer reallocation is a memory policy with no observable output.
            if index == DECODER_PARAM_LARGE_WINDOW {
                decoder.large_window = value != 0;
            } else if index > DECODER_PARAM_LARGE_WINDOW {
                return Err(failure("Setting parameter failed", "ERR_BROTLI_PARAM_SET_FAILED"));
            }
        }
        Ok(Self::Decoder(decoder))
    }

    pub(crate) fn write(
        &mut self,
        flush: i32,
        input: &[u8],
        output: &mut [u8],
    ) -> Result<Step, Failure> {
        let (mut available_in, mut input_offset) = (input.len(), 0);
        let (mut available_out, mut output_offset) = (output.len(), 0);
        match self {
            Self::Encoder(encoder) => {
                let operation = operation(flush)
                    .ok_or_else(|| failure("Compression failed", "ERR_BROTLI_COMPRESSION_FAILED"))?;
                let compressed = encoder.compress_stream(
                    operation,
                    &mut available_in,
                    input,
                    &mut input_offset,
                    &mut available_out,
                    output,
                    &mut output_offset,
                    &mut None,
                    &mut |_, _, _, _| (),
                );
                if !compressed {
                    return Err(failure("Compression failed", "ERR_BROTLI_COMPRESSION_FAILED"));
                }
            }
            Self::Decoder(decoder) => {
                let mut total_out = 0;
                let result = BrotliDecompressStream(
                    &mut available_in,
                    &mut input_offset,
                    input,
                    &mut available_out,
                    &mut output_offset,
                    output,
                    &mut total_out,
                    decoder,
                );
                match result {
                    BrotliResult::ResultFailure => {
                        let error = decoder.error_code;
                        let name = format!("{error:?}");
                        return Err(Failure {
                            errno: error as i32,
                            code: format!("ERR_{}", name.trim_start_matches(DECODER_ERROR_PREFIX)),
                            message: "Decompression failed".into(),
                        });
                    }
                    BrotliResult::NeedsMoreInput if flush == FINISH => {
                        return Err(Failure {
                            errno: Z_BUF_ERROR,
                            code: "Z_BUF_ERROR".into(),
                            message: "unexpected end of file".into(),
                        });
                    }
                    _ => {}
                }
            }
        }
        Ok(Step {
            consumed: input_offset,
            produced: output_offset,
        })
    }
}
