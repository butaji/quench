//! One validation owner for codec construction and synchronous requests.
use super::zlib_shared_vm::{Codec, Family};
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, NativeContext, RootId, RootedError};
use std::collections::HashMap;

const UNSET_PARAMETER: i32 = -1;

#[derive(Clone, Copy)]
pub(super) struct Options {
    pub level: Option<f64>,
    pub strategy: f64,
    pub chunk_size: usize,
    pub max_output: f64,
    pub flush: f64,
    pub finish_flush: f64,
}
pub(super) fn fact(name: &str) -> f64 {
    super::zlib_constants::ZLIB_CONSTANTS
        .iter()
        .find_map(|(key, value)| (*key == name).then_some(*value))
        .expect("zlib constant authority")
}
pub(super) fn parse(
    context: &mut NativeContext<'_, NodeHost>,
    mode: Codec,
    options: Option<RootId>,
    default_max: RootId,
) -> Result<Options, RootedError> {
    let undefined = context.undefined();
    let options = options.unwrap_or(undefined);
    let has_options = context.truthy_rooted(options)?;
    let mut level = None;
    let mut strategy = fact("Z_DEFAULT_STRATEGY");
    match mode.family() {
        Family::Zlib => {
            if has_options {
                let window = get(context, options, "windowBits")?;
                let accepts_header = matches!(mode, Codec::Inflate | Codec::Gunzip | Codec::Unzip);
                let nullish = context
                    .rooted_value(window)
                    .is_some_and(|value| value.is_null() || value.is_undefined());
                let header_value = if nullish {
                    true
                } else {
                    let window = get(context, options, "windowBits")?;
                    context
                        .rooted_value(window)
                        .and_then(|value| value.as_number())
                        == Some(0.0)
                };
                if !(accepts_header && header_value) {
                    let window = get(context, options, "windowBits")?;
                    range(
                        context,
                        window,
                        "options.windowBits",
                        fact("Z_MIN_WINDOWBITS") + f64::from(matches!(mode, Codec::Gzip)),
                        fact("Z_MAX_WINDOWBITS"),
                        fact("Z_DEFAULT_WINDOWBITS"),
                    )?;
                }
                let value = get(context, options, "level")?;
                level = Some(range(
                    context,
                    value,
                    "options.level",
                    fact("Z_MIN_LEVEL"),
                    fact("Z_MAX_LEVEL"),
                    fact("Z_DEFAULT_COMPRESSION"),
                )?);
                let value = get(context, options, "memLevel")?;
                range(
                    context,
                    value,
                    "options.memLevel",
                    fact("Z_MIN_MEMLEVEL"),
                    fact("Z_MAX_MEMLEVEL"),
                    fact("Z_DEFAULT_MEMLEVEL"),
                )?;
                let value = get(context, options, "strategy")?;
                strategy = range(
                    context,
                    value,
                    "options.strategy",
                    fact("Z_DEFAULT_STRATEGY"),
                    fact("Z_FIXED"),
                    fact("Z_DEFAULT_STRATEGY"),
                )?;
                dictionary(context, options)?;
            }
            // The native zlib handle reads this flag before the base stream.
            if !context
                .rooted_value(options)
                .is_some_and(|value| value.is_null() || value.is_undefined())
            {
                get(context, options, "rejectGarbageAfterEnd")?;
            }
            if !has_options {
                level = Some(fact("Z_DEFAULT_COMPRESSION"));
            }
        }
        Family::Brotli => {
            let params = parameters(
                context,
                options,
                "BROTLI_PARAM_",
                "Brotli",
                "ERR_BROTLI_INVALID_PARAM",
            )?;
            if matches!(mode, Codec::BrotliCompress) {
                let quality = params
                    .get(&(fact("BROTLI_PARAM_QUALITY") as u32))
                    .copied()
                    .unwrap_or(fact("BROTLI_DEFAULT_QUALITY") as i32);
                let encoder_mode = params
                    .get(&(fact("BROTLI_PARAM_MODE") as u32))
                    .copied()
                    .unwrap_or(fact("BROTLI_DEFAULT_MODE") as i32);
                let disable = params
                    .get(&(fact("BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING") as u32))
                    .copied()
                    .unwrap_or_default();
                if !(fact("BROTLI_MIN_QUALITY")..=fact("BROTLI_MAX_QUALITY"))
                    .contains(&f64::from(quality))
                    || !(fact("BROTLI_MODE_GENERIC")..=fact("BROTLI_MODE_FONT"))
                        .contains(&f64::from(encoder_mode))
                    || !(0..=1).contains(&disable)
                {
                    return Err(initialization_error(context, "Initialization failed")?);
                }
                level = Some(f64::from(quality));
            }
            if has_options {
                dictionary(context, options)?;
            }
        }
        Family::Zstd => {
            let compress = matches!(mode, Codec::ZstdCompress);
            let params = parameters(
                context,
                options,
                if compress { "ZSTD_c_" } else { "ZSTD_d_" },
                "zstd",
                "ERR_ZSTD_INVALID_PARAM",
            )?;
            if compress {
                level = Some(f64::from(
                    params
                        .get(&(fact("ZSTD_c_compressionLevel") as u32))
                        .copied()
                        .unwrap_or(fact("ZSTD_CLEVEL_DEFAULT") as i32),
                ));
                if let Some(strategy) = params.get(&(fact("ZSTD_c_strategy") as u32)) {
                    if !(0.0..=fact("ZSTD_btultra2")).contains(&f64::from(*strategy)) {
                        return Err(initialization_error(context, "Setting parameter failed")?);
                    }
                }
            } else if let Some(window) = params.get(&(fact("ZSTD_d_windowLogMax") as u32)) {
                let mut decoder = zstd::zstd_safe::DCtx::create();
                if decoder
                    .set_parameter(zstd::zstd_safe::DParameter::WindowLogMax(*window as u32))
                    .is_err()
                {
                    return Err(initialization_error(context, "Setting parameter failed")?);
                }
            }
        }
    }
    let chunk_size = if has_options {
        super::zlib_stream_shared_vm::chunk_size(context, Some(options))?
    } else {
        super::zlib_constants::DEFAULT_ZLIB_CHUNK_BYTES
    };
    let (lower, upper, default_flush, finish) = match mode {
        Codec::BrotliCompress | Codec::BrotliDecompress => (
            "BROTLI_OPERATION_PROCESS",
            "BROTLI_OPERATION_EMIT_METADATA",
            "BROTLI_OPERATION_PROCESS",
            "BROTLI_OPERATION_FINISH",
        ),
        Codec::ZstdCompress | Codec::ZstdDecompress => (
            "ZSTD_e_continue",
            "ZSTD_e_end",
            "ZSTD_e_continue",
            "ZSTD_e_end",
        ),
        _ => ("Z_NO_FLUSH", "Z_BLOCK", "Z_NO_FLUSH", "Z_FINISH"),
    };
    let flush = if has_options {
        let value = get(context, options, "flush")?;
        range(
            context,
            value,
            "options.flush",
            fact(lower),
            fact(upper),
            fact(default_flush),
        )?
    } else {
        fact(default_flush)
    };
    let finish_flush = if has_options {
        let value = get(context, options, "finishFlush")?;
        range(
            context,
            value,
            "options.finishFlush",
            fact(lower),
            fact(upper),
            fact(finish),
        )?
    } else {
        fact(finish)
    };
    let max_output = super::zlib_shared_vm::output_limit(context, Some(options), default_max)?;
    if has_options {
        let reject = get(context, options, "rejectGarbageAfterEnd")?;
        if context
            .rooted_value(reject)
            .is_some_and(|value| !value.is_undefined())
        {
            let reject = get(context, options, "rejectGarbageAfterEnd")?;
            if context
                .rooted_value(reject)
                .is_some_and(|value| value.as_bool().is_none())
            {
                return Err(super::util_shared_vm::argument_type_error(
                    context,
                    reject,
                    "options.rejectGarbageAfterEnd",
                    "boolean",
                )?);
            }
        }
    }
    Ok(Options {
        level,
        strategy,
        chunk_size,
        max_output,
        flush,
        finish_flush,
    })
}
fn dictionary(
    context: &mut NativeContext<'_, NodeHost>,
    options: RootId,
) -> Result<(), RootedError> {
    let value = get(context, options, "dictionary")?;
    if context
        .rooted_value(value)
        .is_some_and(|value| value.is_undefined())
        || context.has_builtin_brand_rooted(value, BuiltinBrand::ArrayBufferView)?
        || context.has_builtin_brand_rooted(value, BuiltinBrand::AnyArrayBuffer)?
    {
        return Ok(());
    }
    Err(super::util_shared_vm::argument_type_error_with_expected(
        context,
        value,
        "options.dictionary",
        "an instance of Buffer, TypedArray, DataView, or ArrayBuffer",
    )?)
}
fn parameters(
    context: &mut NativeContext<'_, NodeHost>,
    options: RootId,
    prefix: &str,
    label: &str,
    code: &str,
) -> Result<HashMap<u32, i32>, RootedError> {
    let mut assigned = HashMap::new();
    if context
        .rooted_value(options)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        return Ok(assigned);
    }
    let params = get(context, options, "params")?;
    if !context.truthy_rooted(params)? {
        return Ok(assigned);
    }
    let max = super::zlib_constants::ZLIB_CONSTANTS
        .iter()
        .filter_map(|(name, value)| name.starts_with(prefix).then_some(*value))
        .fold(0.0, f64::max);
    let zero = context.number(0.0);
    let params = get(context, options, "params")?;
    for key in context.enumerable_own_keys_rooted(params, false, true)? {
        let integer = context.bitwise_or_rooted(key, zero)?;
        let index = context
            .rooted_value(integer)
            .and_then(|value| value.as_number())
            .unwrap_or_default();
        if !context.equal_rooted(key, integer)?
            || index < 0.0
            || index > max
            || assigned
                .get(&(index as u32))
                .is_some_and(|value| *value != UNSET_PARAMETER)
        {
            let key = context.to_string(key)?;
            let error =
                context.range_error_rooted(&format!("{key} is not a valid {label} parameter"))?;
            let code = context.string_rooted(code);
            set(context, error, "code", code)?;
            return Err(context.throw(error));
        }
        let params = get(context, options, "params")?;
        let value = context.get_property_rooted(params, key)?;
        if context
            .rooted_value(value)
            .is_some_and(|value| value.as_number().is_none() && value.as_bool().is_none())
        {
            let params = get(context, options, "params")?;
            let value = context.get_property_rooted(params, key)?;
            return Err(super::util_shared_vm::argument_type_error(
                context,
                value,
                "options.params[key]",
                "number",
            )?);
        }
        let value = context.bitwise_or_rooted(value, zero)?;
        assigned.insert(
            index as u32,
            context
                .rooted_value(value)
                .and_then(|value| value.as_number())
                .unwrap_or_default() as i32,
        );
    }
    Ok(assigned)
}
pub(super) fn range(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
    lower: f64,
    upper: f64,
    default: f64,
) -> Result<f64, RootedError> {
    if context
        .rooted_value(value)
        .is_some_and(|value| value.is_undefined())
    {
        return Ok(default);
    }
    let Some(number) = context
        .rooted_value(value)
        .and_then(|value| value.as_number())
    else {
        return Err(super::util_shared_vm::argument_type_error(
            context, value, name, "number",
        )?);
    };
    if number.is_nan() {
        return Ok(default);
    }
    let constraint = if !number.is_finite() {
        Some("a finite number".to_owned())
    } else if number < lower || number > upper {
        Some(format!(">= {lower} and <= {upper}"))
    } else {
        None
    };
    if let Some(constraint) = constraint {
        let received = super::util_shared_vm::range_error_received(context, value)?;
        let error = context.range_error_rooted(&format!(
            "The value of \"{name}\" is out of range. It must be {constraint}. Received {received}"
        ))?;
        let code = context.string_rooted("ERR_OUT_OF_RANGE");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    Ok(number)
}
fn initialization_error(
    context: &mut NativeContext<'_, NodeHost>,
    message: &str,
) -> Result<RootedError, RootedError> {
    let error = context.error_rooted(message)?;
    let code = context.string_rooted("ERR_ZLIB_INITIALIZATION_FAILED");
    set(context, error, "code", code)?;
    Ok(context.throw(error))
}
fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    context.get_named_property_rooted(object, name)
}
fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.set_property_rooted(object, key, value, object)?;
    Ok(())
}

#[repr(usize)]
enum Field {
    Level,
    Strategy,
    ChunkSize,
    MaxOutput,
    Flush,
    FinishFlush,
}
impl Options {
    pub(super) fn project(
        self,
        context: &mut NativeContext<'_, NodeHost>,
    ) -> Result<RootId, RootedError> {
        let undefined = context.undefined();
        let level = self
            .level
            .map(|level| context.number(level))
            .unwrap_or(undefined);
        let values = [
            level,
            context.number(self.strategy),
            context.number(self.chunk_size as f64),
            context.number(self.max_output),
            context.number(self.flush),
            context.number(self.finish_flush),
        ];
        context.array_rooted(&values)
    }
    pub(super) fn from_projection(
        context: &mut NativeContext<'_, NodeHost>,
        projection: RootId,
    ) -> Result<Self, RootedError> {
        let level = get(context, projection, &(Field::Level as usize).to_string())?;
        let level = context
            .rooted_value(level)
            .and_then(|value| value.as_number())
            .map(|number| number);
        let mut number = |field: Field| -> Result<f64, RootedError> {
            let value = get(context, projection, &(field as usize).to_string())?;
            context
                .rooted_value(value)
                .and_then(|value| value.as_number())
                .ok_or_else(|| RootedError::host("zlib private options projection invalid"))
        };
        Ok(Self {
            level,
            strategy: number(Field::Strategy)?,
            chunk_size: number(Field::ChunkSize)? as usize,
            max_output: number(Field::MaxOutput)?,
            flush: number(Field::Flush)?,
            finish_flush: number(Field::FinishFlush)?,
        })
    }
}
pub(crate) fn prepare(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let mode = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("zlib preparation mode missing"))?;
    let mode = context.to_string(mode)?;
    let mode = super::zlib_shared_vm::codec(&mode)
        .ok_or_else(|| RootedError::host("zlib preparation mode invalid"))?;
    let default_max = context.host_function_data()?;
    let config = parse(context, mode, args.get(1).copied(), default_max)?.project(context)?;
    let undefined = context.undefined();
    let options = args.get(1).copied().unwrap_or(undefined);
    let info_options = normalize_options(context, options)?;
    let stream_options = context.object_rooted()?;
    let auto_destroy = context.boolean(true);
    set(context, stream_options, "autoDestroy", auto_destroy)?;
    copy_options(context, info_options, stream_options)?;
    let result = context.null_object_rooted()?;
    set(context, result, "config", config)?;
    set(context, result, "streamOptions", stream_options)?;
    set(context, result, "infoOptions", info_options)?;
    Ok(result)
}

fn normalize_options(
    context: &mut NativeContext<'_, NodeHost>,
    options: RootId,
) -> Result<RootId, RootedError> {
    if !context.truthy_rooted(options)? {
        return Ok(options);
    }
    let encoding = get(context, options, "encoding")?;
    let clone = if context.truthy_rooted(encoding)? {
        true
    } else {
        let object_mode = get(context, options, "objectMode")?;
        if context.truthy_rooted(object_mode)? {
            true
        } else {
            let writable_mode = get(context, options, "writableObjectMode")?;
            context.truthy_rooted(writable_mode)?
        }
    };
    if !clone {
        return Ok(options);
    }
    let normalized = context.object_rooted()?;
    copy_options(context, options, normalized)?;
    let null = context.null();
    let boolean = context.boolean(false);
    set(context, normalized, "encoding", null)?;
    set(context, normalized, "objectMode", boolean)?;
    set(context, normalized, "writableObjectMode", boolean)?;
    Ok(normalized)
}
fn copy_options(
    context: &mut NativeContext<'_, NodeHost>,
    source: RootId,
    target: RootId,
) -> Result<(), RootedError> {
    if context
        .rooted_value(source)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        return Ok(());
    }
    for key in context.enumerable_own_keys_rooted(source, true, true)? {
        let value = context.get_property_rooted(source, key)?;
        context.define_data_property_rooted(target, key, value, true, true, true)?;
    }
    Ok(())
}
