//! Stateful native zlib byte engine. Policy and iteration remain in configured Node.
use zlib_rs::{
    Deflate, DeflateConfig, DeflateFlush, Inflate, InflateError, InflateFlush, Method, Status,
    Strategy,
};

use crate::modules::zlib_shared_vm::Codec as Mode;
impl Mode {
    fn native_encoding(self) -> bool {
        matches!(self, Self::Deflate | Self::Gzip | Self::DeflateRaw)
    }
    fn native_window(self, bits: i32) -> i32 {
        const GZIP_WINDOW: i32 = 16;
        match self {
            Self::Gzip | Self::Gunzip => bits + GZIP_WINDOW,
            // zlib-rs' Inflate accepts a zlib-vs-raw flag and a window size,
            // but has no gzip/zlib auto-detection mode equivalent to
            // inflateInit2(windowBits + 32). Use the zlib window for this
            // fallback backend; the native zlib backend retains full auto
            // detection semantics.
            Self::Unzip => bits,
            Self::DeflateRaw | Self::InflateRaw => -bits,
            _ => bits,
        }
    }
}
use super::{Failure, Step};
enum Engine {
    Encode(Deflate),
    Decode(Inflate),
}
pub(crate) struct Codec {
    mode: Mode,
    window: i32,
    dictionary: Option<Vec<u8>>,
    engine: Engine,
    ended: bool,
}
impl Codec {
    pub(crate) fn params(&mut self, _level: i32, _strategy: i32) -> Result<(), Failure> {
        Err(stream_error())
    }

    pub(in crate::modules) fn new(
        mode: Mode,
        bits: i32,
        level: i32,
        memory: i32,
        strategy: i32,
        dictionary: Option<Vec<u8>>,
        _reject_trailing: bool,
    ) -> Result<Self, Failure> {
        // Node permits windowBits: 0 for decompressors that can read the
        // window size from the stream header. zlib-rs requires an explicit
        // 8..=15 value, so select the largest supported window for the
        // fallback engine. Compression must continue to reject zero.
        let bits = if bits == 0
            && matches!(mode, Mode::Inflate | Mode::Gunzip | Mode::Unzip)
        {
            15
        } else {
            bits
        };
        let window = mode.native_window(bits);
        let strategy = match strategy {
            0 => Strategy::Default,
            1 => Strategy::Filtered,
            2 => Strategy::HuffmanOnly,
            3 => Strategy::Rle,
            4 => Strategy::Fixed,
            _ => return Err(stream_error()),
        };
        if !(8..=15).contains(&bits) || !(-1..=9).contains(&level) || !(1..=9).contains(&memory) {
            return Err(stream_error());
        }
        let mut engine = if mode.native_encoding() {
            Engine::Encode(Deflate::new_with_config(DeflateConfig {
                level,
                method: Method::Deflated,
                window_bits: window,
                mem_level: memory,
                strategy,
            }))
        } else {
            Engine::Decode(Inflate::new(window >= 0, window.unsigned_abs() as u8))
        };
        if let Some(dict) = dictionary.as_deref().filter(|dict| !dict.is_empty()) {
            match &mut engine {
                Engine::Encode(encoder) if matches!(mode, Mode::Deflate | Mode::DeflateRaw) => {
                    encoder.set_dictionary(dict).map_err(|e| Failure {
                        errno: e as i32,
                        code: "Z_STREAM_ERROR".into(),
                        message: e.as_str().into(),
                    })?;
                }
                Engine::Decode(decoder) if matches!(mode, Mode::InflateRaw) => {
                    decoder.set_dictionary(dict).map_err(|e| Failure {
                        errno: zlib_rs::ReturnCode::from(e) as i32,
                        code: "Z_STREAM_ERROR".into(),
                        message: e.as_str().into(),
                    })?;
                }
                _ => {}
            }
        }
        Ok(Self {
            mode,
            window,
            dictionary,
            engine,
            ended: false,
        })
    }
    pub(crate) fn write(
        &mut self,
        flush: i32,
        input: &[u8],
        output: &mut [u8],
    ) -> Result<Step, Failure> {
        let mut step = Step {
            consumed: 0,
            produced: 0,
        };
        loop {
            if self.ended
                && matches!(self.mode, Mode::Gunzip | Mode::Unzip)
                && step.consumed < input.len()
            {
                // Native gunzip accepts concatenated members and ignores trailing zero padding.
                if input[step.consumed] == 0 {
                    step.consumed = input.len();
                    return Ok(step);
                }
                self.engine = Engine::Decode(Inflate::new(
                    self.window >= 0,
                    self.window.unsigned_abs() as u8,
                ));
                self.ended = false;
            }
            let remaining = &input[step.consumed..];
            let available = &mut output[step.produced..];
            let status = match &mut self.engine {
                Engine::Encode(encoder) => {
                    let flush = DeflateFlush::try_from(flush).map_err(|_| stream_error())?;
                    let before = (encoder.total_in(), encoder.total_out());
                    let status = encoder.compress(remaining, available, flush);
                    step.consumed += (encoder.total_in() - before.0) as usize;
                    step.produced += (encoder.total_out() - before.1) as usize;
                    status.map_err(|e| Failure {
                        errno: e as i32,
                        code: "Z_STREAM_ERROR".into(),
                        message: encoder.error_message().unwrap_or(e.as_str()).into(),
                    })?
                }
                Engine::Decode(decoder) => {
                    let flush = InflateFlush::try_from(flush).map_err(|_| stream_error())?;
                    let before = (decoder.total_in(), decoder.total_out());
                    let result = decoder.decompress(remaining, available, flush);
                    step.consumed += (decoder.total_in() - before.0) as usize;
                    step.produced += (decoder.total_out() - before.1) as usize;
                    match result {
                        Ok(status) => status,
                        Err(InflateError::NeedDict { .. }) => {
                            let Some(dictionary) = self.dictionary.as_deref() else {
                                return Err(Failure {
                                    errno: 2,
                                    code: "Z_NEED_DICT".into(),
                                    message: "Missing dictionary".into(),
                                });
                            };
                            decoder.set_dictionary(dictionary).map_err(|_| Failure {
                                errno: 2,
                                code: "Z_NEED_DICT".into(),
                                message: "Bad dictionary".into(),
                            })?;
                            continue;
                        }
                        Err(e) => {
                            return Err(Failure {
                                errno: zlib_rs::ReturnCode::from(e) as i32,
                                code: "Z_DATA_ERROR".into(),
                                message: decoder.error_message().unwrap_or(e.as_str()).into(),
                            });
                        }
                    }
                }
            };
            self.ended = status == Status::StreamEnd;
            if !self.ended && flush == DeflateFlush::Finish as i32 && step.produced < output.len() {
                let code = if status == Status::BufError {
                    zlib_rs::ReturnCode::BufError
                } else {
                    zlib_rs::ReturnCode::Ok
                };
                return Err(Failure {
                    errno: code as i32,
                    code: if status == Status::BufError {
                        "Z_BUF_ERROR".into()
                    } else {
                        "Z_OK".into()
                    },
                    message: "unexpected end of file".into(),
                });
            }
            if !self.ended
                || !matches!(self.mode, Mode::Gunzip | Mode::Unzip)
                || step.consumed == input.len()
                || step.produced == output.len()
            {
                return Ok(step);
            }
        }
    }
}
fn stream_error() -> Failure {
    Failure {
        errno: -2,
        code: "Z_STREAM_ERROR".into(),
        message: "stream error".into(),
    }
}
