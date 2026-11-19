//! BigInt words, Dates and ArrayBuffer detaching (`js_native_api.h`), read and written
//! natively: none of them runs guest code.

use super::abi::node_api;
use super::{api, js_api, out, Context, Env, NapiValue, Status, Thrown};
use quench_runtime::{Detach, RootId};

/// A BigInt's sign and magnitude words; `napi_bigint_expected` for any other value.
fn bigint(env: &Env, c: &mut Context<'_>, value: NapiValue) -> Result<(bool, Vec<u64>), Status> {
    let value = env.value(value)?;
    c.bigint_words_rooted(value).napi(env, c)?.ok_or(Status::BigintExpected)
}

/// The low 64 bits of a BigInt in two's complement, and whether they hold it exactly.
fn low_bits(negative: bool, words: &[u64]) -> (u64, bool) {
    let low = words.first().copied().unwrap_or(0);
    let bits = if negative { low.wrapping_neg() } else { low };
    (bits, words.len() <= 1)
}

fn date(env: &Env, c: &mut Context<'_>, value: RootId) -> Result<Option<f64>, Status> {
    c.date_value_rooted(value).napi(env, c)
}

node_api! {
    fn napi_create_bigint_words(env: *mut Env, sign_bit: i32, word_count: usize, words: *const u64, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            if words.is_null() || result.is_null() || word_count > i32::MAX as usize {
                return Err(Status::InvalidArg);
            }
            // SAFETY: Node-API callers pass `word_count` readable words.
            let words = unsafe { std::slice::from_raw_parts(words, word_count) };
            let integer = c.bigint_from_words_rooted(sign_bit != 0, words);
            out(result, env.handle(integer))
        })
    }

    fn napi_get_value_bigint_int64(env: *mut Env, value: NapiValue, result: *mut i64, lossless: *mut bool) -> Status {
        api(env, |env, c| {
            if result.is_null() || lossless.is_null() {
                return Err(Status::InvalidArg);
            }
            let (negative, words) = bigint(env, c, value)?;
            let (bits, whole) = low_bits(negative, &words);
            let signed = bits as i64;
            out(lossless, words.is_empty() || (whole && (signed < 0) == negative))?;
            out(result, signed)
        })
    }

    fn napi_get_value_bigint_uint64(env: *mut Env, value: NapiValue, result: *mut u64, lossless: *mut bool) -> Status {
        api(env, |env, c| {
            if result.is_null() || lossless.is_null() {
                return Err(Status::InvalidArg);
            }
            let (negative, words) = bigint(env, c, value)?;
            let (bits, whole) = low_bits(negative, &words);
            out(lossless, whole && !negative)?;
            out(result, bits)
        })
    }

    fn napi_get_value_bigint_words(env: *mut Env, value: NapiValue, sign_bit: *mut i32, word_count: *mut usize, words: *mut u64) -> Status {
        api(env, |env, c| {
            if word_count.is_null() {
                return Err(Status::InvalidArg);
            }
            let (negative, magnitude) = bigint(env, c, value)?;
            if sign_bit.is_null() && words.is_null() {
                return out(word_count, magnitude.len());
            }
            if sign_bit.is_null() || words.is_null() {
                return Err(Status::InvalidArg);
            }
            // SAFETY: `word_count` is readable; `words` holds that many writable words.
            let capacity = unsafe { *word_count };
            let target = unsafe { std::slice::from_raw_parts_mut(words, capacity) };
            let copied = capacity.min(magnitude.len());
            target[..copied].copy_from_slice(&magnitude[..copied]);
            out(sign_bit, i32::from(negative))?;
            out(word_count, magnitude.len())
        })
    }

    fn napi_create_date(env: *mut Env, time: f64, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            if result.is_null() {
                return Err(Status::InvalidArg);
            }
            let date = c.date_rooted(time);
            out(result, env.handle(date))
        })
    }

    fn napi_is_date(env: *mut Env, value: NapiValue, is_date: *mut bool) -> Status {
        api(env, |env, c| {
            let value = env.value(value)?;
            out(is_date, date(env, c, value)?.is_some())
        })
    }

    fn napi_get_date_value(env: *mut Env, value: NapiValue, result: *mut f64) -> Status {
        js_api(env, |env, c| {
            if result.is_null() {
                return Err(Status::InvalidArg);
            }
            let value = env.value(value)?;
            out(result, date(env, c, value)?.ok_or(Status::DateExpected)?)
        })
    }

    fn napi_detach_arraybuffer(env: *mut Env, arraybuffer: NapiValue) -> Status {
        api(env, |env, c| {
            let value = env.value(arraybuffer)?;
            match c.detach_array_buffer_rooted(value).napi(env, c)? {
                Detach::NotArrayBuffer => Err(Status::ArraybufferExpected),
                Detach::NotDetachable => Err(Status::DetachableArraybufferExpected),
                Detach::Detached => Ok(()),
            }
        })
    }

    fn napi_is_detached_arraybuffer(env: *mut Env, value: NapiValue, result: *mut bool) -> Status {
        api(env, |env, c| {
            let value = env.value(value)?;
            out(result, c.array_buffer_detached_rooted(value).napi(env, c)?)
        })
    }
}
