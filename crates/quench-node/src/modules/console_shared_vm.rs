//! The configured Node SDK owns the Console constructor and global namespace.
//!
//! The SDK console loads inspect, errors, validators, buffer and the primordials, more than the
//! rest of a run usually allocates. The global `console` is therefore the object Node's
//! `internal/console/global.js` makes, built ahead of it: the same prototype shape, the SDK's
//! own keys in its order (`CONSOLE_KEYS`), and `Symbol.toStringTag`. Every key but the writing
//! methods (`WRITERS`) is a lazy member that loads the SDK, whose own properties the console
//! then adopts (`adopt`). Each writer stays a function of the SDK's name and length that
//! writes a call of plain strings and primitives to its stream as the SDK would until the SDK
//! is loaded (`plain_line`), and calls the SDK's method otherwise. Listing the console's keys with its symbols loads the SDK first.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

/// The global console (`facade`).
const CONSOLE: &str = "\0node/console/global";
/// The SDK's own global console, once loaded; its methods are bound to it.
const SDK_CONSOLE: &str = "\0node/console/sdk";
/// Set while the SDK console's properties move to the global console.
const ADOPTING: &str = "\0node/console/adopting";
/// The streams the unloaded console wrote to, which the SDK console then keeps.
const STDOUT: &str = "\0node/console/stdout";
const STDERR: &str = "\0node/console/stderr";
/// Set once `internal/util/inspect.js` has loaded: from then on its options
/// (`inspect.defaultOptions`) may differ from Node's defaults, so `log` leaves every
/// non-string argument to the SDK.
const INSPECT_LOADED: &str = "\0node/console/inspect-loaded";
/// The variable that makes Node's console colorize output that is not a terminal
/// (`internal/util/colors.js` `shouldColorize`).
const FORCE_COLOR: &str = "FORCE_COLOR";

/// The own string keys of the SDK's global console after Node's bootstrap, in order, with
/// whether each is enumerable. The writers (`WRITERS`) come first and stay the global
/// console's own.
const CONSOLE_KEYS: [(&str, bool); 26] = [
    ("log", true),
    ("info", true),
    ("debug", true),
    ("warn", true),
    ("error", true),
    ("dir", true),
    ("time", true),
    ("timeEnd", true),
    ("timeLog", true),
    ("trace", true),
    ("assert", true),
    ("clear", true),
    ("count", true),
    ("countReset", true),
    ("group", true),
    ("groupEnd", true),
    ("table", true),
    ("dirxml", true),
    ("groupCollapsed", true),
    ("_stdoutErrorHandler", false),
    ("_stderrErrorHandler", false),
    ("_ignoreErrors", false),
    ("_times", false),
    ("Console", true),
    ("_stdout", false),
    ("_stderr", false),
];

/// The stream a writing method writes to: the process property and its cache key.
#[derive(Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    fn property(self) -> &'static str {
        match self {
            Stream::Stdout => "stdout",
            Stream::Stderr => "stderr",
        }
    }

    fn cache_key(self) -> &'static str {
        match self {
            Stream::Stdout => STDOUT,
            Stream::Stderr => STDERR,
        }
    }
}

/// The SDK console's writing methods (`consoleMethods` in `internal/console/constructor.js`),
/// the first `CONSOLE_KEYS` in order: `log`, `info` and `debug` format for stdout, `warn` and
/// `error` for stderr.
const WRITERS: [Stream; 5] = [Stream::Stdout, Stream::Stdout, Stream::Stdout, Stream::Stderr, Stream::Stderr];

/// Set once `lib/diagnostics_channel.js` has loaded: a `console.*` channel may then have
/// subscribers, which only the SDK's methods publish to.
const CHANNELS_LOADED: &str = "\0node/console/channels-loaded";

/// The `%` that starts a format specifier in the SDK's `formatWithOptions`.
const FORMAT_SPECIFIER: char = '%';

fn cached(context: &mut NativeContext<'_, NodeHost>, name: &str) -> Option<RootId> {
    context.host_mut().shared_state().borrow().builtin_cache.get(name).copied()
}

fn cache(context: &mut NativeContext<'_, NodeHost>, name: &str, value: RootId) -> Result<(), RootedError> {
    let value = context.retain(value)?;
    context.host_mut().shared_state().borrow_mut().builtin_cache.insert(name.into(), value);
    Ok(())
}

/// The global console, made on first use.
pub(crate) fn facade(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    if let Some(console) = cached(context, CONSOLE) {
        return Ok(console);
    }
    // `{ __proto__: {} }`, as `internal/console/global.js` makes it.
    let console = context.object_rooted()?;
    let prototype = context.object_rooted()?;
    context.set_prototype_rooted(console, prototype)?;
    // Bound to the console, as the SDK binds every method of its global console, and to the
    // writer's index. Called directly: compiling a helper would fault the compiler back in
    // after the entry's compilation released it.
    let write = context.host_function(crate::host::shared_vm::operation("consoleWrite"))?;
    let global = context.global_root()?;
    let function_key = context.string_rooted("Function");
    let function = context.get_property_rooted(global, function_key)?;
    let prototype_key = context.string_rooted("prototype");
    let function_prototype = context.get_property_rooted(function, prototype_key)?;
    let bind_key = context.string_rooted("bind");
    let bind = context.get_property_rooted(function_prototype, bind_key)?;
    let name = context.string_rooted("name");
    let length = context.string_rooted("length");
    let variadic = context.number(0.0);
    for (index, &(writer_key, enumerable)) in CONSOLE_KEYS[..WRITERS.len()].iter().enumerate() {
        let index = context.number(index as f64);
        let method = context.call_rooted(bind, write, &[console, index])?;
        let key = context.string_rooted(writer_key);
        context.define_data_property_rooted(console, key, method, true, enumerable, true)?;
        context.define_data_property_rooted(method, name, key, false, false, true)?;
        context.define_data_property_rooted(method, length, variadic, false, false, true)?;
    }
    let initializer = context.host_function(crate::host::shared_vm::operation("consoleLoad"))?;
    context.define_lazy_holder_keys_rooted(console, &CONSOLE_KEYS[WRITERS.len()..], initializer)?;
    let symbol_key = context.string_rooted("Symbol");
    let symbol = context.get_property_rooted(global, symbol_key)?;
    let tag_key = context.string_rooted("toStringTag");
    let tag = context.get_property_rooted(symbol, tag_key)?;
    let tag_value = context.string_rooted("console");
    context.define_data_property_rooted(console, tag, tag_value, false, false, true)?;
    cache(context, CONSOLE, console)?;
    Ok(console)
}

/// The lazy members' initializer: loads the SDK console, whose load adopts it (`adopt`).
pub(crate) fn load(context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    if cached(context, ADOPTING).is_none() && cached(context, SDK_CONSOLE).is_none() {
        crate::host::shared_vm::commonjs::console_module(context)?;
    }
    Ok(context.undefined())
}

/// Called as `internal/console/global.js` finishes with the SDK's console `sdk`: the global
/// console takes its own properties, in its order, and becomes the module's export.
pub(crate) fn adopt(context: &mut NativeContext<'_, NodeHost>, module: RootId, sdk: RootId) -> Result<(), RootedError> {
    let console = facade(context)?;
    let marker = context.boolean(true);
    cache(context, ADOPTING, marker)?;
    cache(context, SDK_CONSOLE, sdk)?;
    let stdout = cached(context, STDOUT).unwrap_or_else(|| context.undefined());
    let stderr = cached(context, STDERR).unwrap_or_else(|| context.undefined());
    let adopt = context.evaluate_script_rooted(
        "(sdk, console, stdout, stderr, writers) => {\n\
           for (const key of Reflect.ownKeys(sdk)) {\n\
             if (writers.includes(key)) continue;\n\
             if (key === Symbol.toStringTag) delete console[key];\n\
             Reflect.defineProperty(console, key, Reflect.getOwnPropertyDescriptor(sdk, key));\n\
           }\n\
           if (stdout !== undefined) sdk._stdout = stdout;\n\
           if (stderr !== undefined) sdk._stderr = stderr;\n\
           return Reflect.ownKeys(sdk).filter((key) => typeof key === 'string');\n\
         }",
        "node:internal/console/adopt.js",
    )?;
    let writers = CONSOLE_KEYS[..WRITERS.len()].iter().map(|(key, _)| context.string_rooted(key)).collect::<Vec<_>>();
    let writers = context.array_rooted(&writers)?;
    let undefined = context.undefined();
    let keys = context.call_rooted(adopt, undefined, &[sdk, console, stdout, stderr, writers])?;
    debug_assert!(sdk_keys_match(context, keys), "CONSOLE_KEYS follows the SDK console's keys");
    let exports = context.string_rooted("exports");
    context.set_property_rooted(module, exports, console, module)?;
    Ok(())
}

/// Whether the SDK console's string keys `keys` are `CONSOLE_KEYS`.
fn sdk_keys_match(context: &mut NativeContext<'_, NodeHost>, keys: RootId) -> bool {
    let length_key = context.string_rooted("length");
    let Ok(length) = context.get_property_rooted(keys, length_key) else { return false };
    let Ok(length) = context.to_number(length) else { return false };
    length as usize == CONSOLE_KEYS.len()
        && CONSOLE_KEYS.iter().enumerate().all(|(index, (name, _))| {
            let index = context.string_rooted(&index.to_string());
            context
                .get_property_rooted(keys, index)
                .and_then(|key| context.string_text(key))
                .is_ok_and(|key| key.as_deref() == Some(*name))
        })
}

/// A writing method of the global console: `args` are the writer's index (`WRITERS`), bound
/// ahead of the call's arguments.
pub(crate) fn write(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    crate::host::resident_pages::around_output(|| write_line(context, args))
}

fn write_line(context: &mut NativeContext<'_, NodeHost>, args: &[RootId]) -> Result<RootId, RootedError> {
    let (&index, args) = args.split_first().ok_or_else(|| RootedError::host("console writer without its index"))?;
    let index = context.to_number(index)? as usize;
    let stream = *WRITERS.get(index).ok_or_else(|| RootedError::host("console writer index out of range"))?;
    if cached(context, SDK_CONSOLE).is_none() {
        if cached(context, CHANNELS_LOADED).is_none() {
            if let Some(text) = plain_line(context, stream, args)? {
                return write_stream(context, stream, &text);
            }
        }
        let console = facade(context)?;
        context.materialize_lazy_data_property_rooted(console, CONSOLE_KEYS[WRITERS.len()].0)?;
    }
    let sdk = cached(context, SDK_CONSOLE).ok_or_else(|| RootedError::host("SDK console did not load"))?;
    let key = context.string_rooted(CONSOLE_KEYS[index].0);
    let method = context.get_property_rooted(sdk, key)?;
    context.call_rooted(method, sdk, args)
}

/// The line an SDK writer writes to `stream` for `args`, when it needs no format specifier, no
/// colors and no `inspect` beyond what `inspect_primitive` and `inspect_flat` reproduce
/// (`formatWithOptions`: a lone string as it is; strings as they are, every other argument
/// as `inspect` with default options), joined by spaces.
fn plain_line(context: &mut NativeContext<'_, NodeHost>, stream: Stream, args: &[RootId]) -> Result<Option<String>, RootedError> {
    use quench_runtime::ValueType;
    if let [first, _, ..] = args {
        let first = *first;
        if context.string_text(first)?.is_some_and(|text| text.contains(FORMAT_SPECIFIER)) {
            return Ok(None);
        }
    }
    let inspected = args.iter().any(|&arg| context.value_type_rooted(arg) != Some(ValueType::String));
    if inspected && (cached(context, INSPECT_LOADED).is_some() || may_colorize(context, stream)?) {
        return Ok(None);
    }
    let mut parts = Vec::with_capacity(args.len());
    for &arg in args {
        let part = match context.value_type_rooted(arg) {
            Some(ValueType::String) => context.string_text(arg)?,
            Some(ValueType::Object) => inspect_flat(context, arg)?,
            _ => inspect_primitive(context, arg)?,
        };
        let Some(part) = part else { return Ok(None) };
        parts.push(part);
    }
    Ok(Some(parts.join(" ") + "\n"))
}

/// `formatPrimitive` with default options for a primitive other than a string; `None` for
/// any other value.
fn inspect_primitive(context: &mut NativeContext<'_, NodeHost>, value: RootId) -> Result<Option<String>, RootedError> {
    use quench_runtime::ValueType;
    Ok(match context.value_type_rooted(value) {
        Some(ValueType::Number) => {
            let number = context.to_number(value)?;
            // `formatNumber`: `-0` as itself, every other number as `${number}`.
            Some(if number == 0.0 && number.is_sign_negative() { "-0".to_owned() } else { context.to_string(value)? })
        }
        Some(ValueType::BigInt) => Some(context.to_string(value)? + "n"),
        Some(ValueType::Boolean | ValueType::Undefined | ValueType::Null) => Some(context.to_string(value)?),
        _ => None,
    })
}

/// `inspectDefaultOptions.breakLength`: the width a container's entries share one line within.
const BREAK_LENGTH: usize = 80;
/// `reduceToSingleString`'s constant start, "next to all other factors that may reduce
/// `breakLength`".
const SINGLE_LINE_START: usize = 10;
/// `inspectDefaultOptions.maxArrayLength`: longer arrays end in `... n more items`, which this
/// path leaves to `inspect`.
const MAX_ARRAY_LENGTH: usize = 100;
/// Arrays of more entries are grouped into columns (`groupArrayElements`).
const MAX_UNGROUPED_ARRAY_ENTRIES: usize = 6;
/// `groupArrayElements`: the comma and space after each entry.
const SEPARATOR_SPACE: usize = 2;
/// `groupArrayElements` groups only when this many entries fit next to each other...
const MIN_GROUPED_COLUMNS: usize = 3;
/// ...and no entry is longer than this share of all of them, or every entry is at most
/// `SHORT_ENTRY_WIDTH` wide.
const GROUPED_LENGTH_SHARE: f64 = 5.0;
const SHORT_ENTRY_WIDTH: usize = 6;
/// `groupArrayElements`'s expected character height in widths, which makes its grid square.
const APPROX_CHAR_HEIGHTS: f64 = 2.5;
/// `groupArrayElements`'s reduction of the widest entry before its bias.
const BIASED_MAX_OFFSET: f64 = 3.0;
/// `groupArrayElements`'s columns per `compact` level, and its most columns.
const GROUPED_COLUMNS_PER_COMPACT: usize = 4;
const MAX_GROUPED_COLUMNS: usize = 15;
/// The indentation `formatProperty` adds for a container's entries.
const ENTRY_INDENTATION: usize = 2;
/// `formatPrimitive` splits a string into lines past `kMinLineLength` and past
/// `breakLength - indentationLvl - STRING_LINE_RESERVE` characters.
const MIN_LINE_LENGTH: usize = 16;
const STRING_LINE_RESERVE: usize = 4;
/// The internal brands `formatValue` and `formatRaw` test before a value's prototype: each
/// is written its own way whatever its prototype, and a proxy is inspected through its target.
const INSPECTED_BRANDS: [quench_runtime::BuiltinBrand; 18] = {
    use quench_runtime::BuiltinBrand::*;
    [
        Proxy, External, Date, RegExp, NativeError, Promise, Map, Set, WeakMap, WeakSet,
        AnyArrayBuffer, ArrayBufferView, ArgumentsObject, GeneratorObject, MapIterator,
        SetIterator, ModuleNamespaceObject, BoxedPrimitive,
    ]
};
/// The registry key of `util.inspect.custom`.
const INSPECT_CUSTOM: &str = "nodejs.util.inspect.custom";
/// `strEscape`'s default quote; a string holding it, a backslash, or a character outside
/// printable ASCII would be escaped or quoted otherwise, which this path leaves to `inspect`.
const SINGLE_QUOTE: char = '\'';
const BACKSLASH: char = '\\';

/// `inspectDefaultOptions.depth`: deeper containers are written as `[Object]` or `[Array]`.
const DEPTH: usize = 2;
/// `inspectDefaultOptions.compact`: a container whose most recently inspected descendant is
/// fewer levels below it shares one line, when its entries fit.
const COMPACT: usize = 3;

/// The state `inspect` threads through one value's containers (`ctx`).
#[derive(Default)]
struct Inspection {
    /// `ctx.indentationLvl`.
    indentation: usize,
    /// `ctx.currentDepth`: the depth of the container inspected last.
    current_depth: usize,
    /// `ctx.seen`: the containers being inspected, a circular one of which `inspect` labels.
    seen: Vec<RootId>,
}

/// `inspect` with default options for a plain object or an array of at most
/// `MAX_ARRAY_LENGTH` (`formatValue`), whose containers hold only primitives and
/// such containers; `None` for any value whose output depends on more: a proxy or another
/// builtin brand, another prototype or constructor, a `Symbol.toStringTag` or
/// `util.inspect.custom` member, a symbol, accessor or non-identifier key, a hole, a circular
/// reference, or a string `strEscape` would change.
fn inspect_flat(context: &mut NativeContext<'_, NodeHost>, value: RootId) -> Result<Option<String>, RootedError> {
    inspect_container(context, &mut Inspection::default(), value, 0)
}

/// `formatValue` of a property value at `recurse_times`.
fn inspect_value(
    context: &mut NativeContext<'_, NodeHost>, inspection: &mut Inspection, value: RootId, recurse_times: usize,
) -> Result<Option<String>, RootedError> {
    use quench_runtime::ValueType;
    match context.value_type_rooted(value) {
        Some(ValueType::Object) => inspect_container(context, inspection, value, recurse_times),
        Some(ValueType::String) => {
            let text = context.string_text(value)?.unwrap_or_default();
            let split = text.len() > MIN_LINE_LENGTH
                && text.len() + inspection.indentation + STRING_LINE_RESERVE > BREAK_LENGTH;
            let plain = text.chars().all(|character| matches!(character, ' '..='~') && character != SINGLE_QUOTE && character != BACKSLASH);
            Ok((plain && !split).then(|| format!("{SINGLE_QUOTE}{text}{SINGLE_QUOTE}")))
        }
        _ => inspect_primitive(context, value),
    }
}

/// `formatRaw` and `reduceToSingleString` for a plain object or short array.
fn inspect_container(
    context: &mut NativeContext<'_, NodeHost>, inspection: &mut Inspection, value: RootId, recurse_times: usize,
) -> Result<Option<String>, RootedError> {
    use quench_runtime::ValueType;
    for brand in INSPECTED_BRANDS {
        if context.has_builtin_brand_rooted(value, brand)? {
            return Ok(None);
        }
    }
    for &seen in &inspection.seen {
        if context.strict_equal_rooted(seen, value)? {
            return Ok(None);
        }
    }
    let array = context.is_array_rooted(value)?;
    let global = context.global_root()?;
    let Some(constructor) = context.startup_data_property_rooted(global, if array { "Array" } else { "Object" }) else {
        return Ok(None);
    };
    let Some(prototype) = context.startup_data_property_rooted(constructor, "prototype") else { return Ok(None) };
    let actual_prototype = context.prototype_rooted(value)?;
    if !context.strict_equal_rooted(actual_prototype, prototype)? {
        return Ok(None);
    }
    // `getConstructorName`: the first `constructor` data property on the chain names it.
    let constructor_key = context.string_rooted("constructor");
    if context.own_property_fields_rooted(value, constructor_key)?.is_some() {
        return Ok(None);
    }
    let named = context.own_property_fields_rooted(prototype, constructor_key)?.and_then(|fields| fields.value);
    match named {
        Some(named) if context.strict_equal_rooted(named, constructor)? => {}
        _ => return Ok(None),
    }
    let Some(symbol) = context.startup_data_property_rooted(global, "Symbol") else { return Ok(None) };
    let Some(tag) = context.startup_data_property_rooted(symbol, "toStringTag") else { return Ok(None) };
    let Some(symbol_for) = context.startup_data_property_rooted(symbol, "for") else { return Ok(None) };
    let custom_key = context.string_rooted(INSPECT_CUSTOM);
    let custom = context.call_rooted(symbol_for, symbol, &[custom_key])?;
    for holder in [value, prototype] {
        for key in [tag, custom] {
            if context.own_property_fields_rooted(holder, key)?.is_some() {
                return Ok(None);
            }
        }
    }
    // The keys `formatProperty` writes, as (key root, name): an array's elements, then an
    // object's enumerable own string keys.
    let mut properties = Vec::new();
    let length = if array {
        let length_key = context.string_rooted("length");
        let length = context.get_property_rooted(value, length_key)?;
        let length = context.to_number(length)? as usize;
        if length > MAX_ARRAY_LENGTH {
            return Ok(None);
        }
        for index in 0..length {
            properties.push((context.string_rooted(&index.to_string()), None));
        }
        length
    } else {
        0
    };
    for key in context.own_keys_rooted(value)? {
        if context.value_type_rooted(key) != Some(ValueType::String) {
            return Ok(None);
        }
        let name = context.string_text(key)?.unwrap_or_default();
        if array && (name == "length" || name.parse::<usize>().is_ok_and(|index| index < length)) {
            continue;
        }
        let Some(fields) = context.own_property_fields_rooted(value, key)? else { continue };
        if fields.enumerable != Some(true) {
            continue;
        }
        // An array's other enumerable keys follow its elements; the path leaves them to
        // `inspect`, as it leaves keys `keyStrRegExp` quotes and `__proto__`.
        if array || !identifier_key(&name) || name == "__proto__" {
            return Ok(None);
        }
        properties.push((key, Some(name)));
    }
    let (open, close) = if array { ("[", "]") } else { ("{", "}") };
    if properties.is_empty() {
        return Ok(Some(format!("{open}{close}")));
    }
    if recurse_times > DEPTH {
        return Ok(Some(if array { "[Array]" } else { "[Object]" }.to_owned()));
    }
    let recurse_times = recurse_times + 1;
    inspection.seen.push(value);
    inspection.current_depth = recurse_times;
    let mut entries = Vec::with_capacity(properties.len());
    let mut numeric = true;
    for (key, name) in properties {
        let Some(fields) = context.own_property_fields_rooted(value, key)? else { return Ok(None) };
        let (Some(property), None, None) = (fields.value, fields.get, fields.set) else { return Ok(None) };
        numeric &= matches!(context.value_type_rooted(property), Some(ValueType::Number | ValueType::BigInt));
        inspection.indentation += ENTRY_INDENTATION;
        let entry = inspect_value(context, inspection, property, recurse_times)?;
        inspection.indentation -= ENTRY_INDENTATION;
        let Some(entry) = entry else { return Ok(None) };
        entries.push(match name {
            Some(name) => format!("{name}: {entry}"),
            None => entry,
        });
    }
    inspection.seen.pop();
    let count = entries.len();
    if array && count > MAX_UNGROUPED_ARRAY_ENTRIES {
        entries = group_array_elements(entries, numeric, inspection.indentation);
    }
    if inspection.current_depth - recurse_times < COMPACT && entries.len() == count {
        // `isBelowBreakLength` with no base.
        let start = entries.len() + inspection.indentation + open.len() + SINGLE_LINE_START;
        let below = entries.len() + start + entries.len() <= BREAK_LENGTH
            && entries.iter().map(String::len).sum::<usize>() + entries.len() + start <= BREAK_LENGTH;
        let joined = entries.join(", ");
        if below && !joined.contains('\n') {
            return Ok(Some(format!("{open} {joined} {close}")));
        }
    }
    let indentation = format!("\n{}", " ".repeat(inspection.indentation));
    let entry_indentation = " ".repeat(ENTRY_INDENTATION);
    Ok(Some(format!(
        "{open}{indentation}{entry_indentation}{}{indentation}{close}",
        entries.join(&format!(",{indentation}{entry_indentation}"))
    )))
}

/// `groupArrayElements` for an array's `entries`, every one ASCII, when their widths allow:
/// rows of columns, numbers right-aligned (`numeric`) and other values left-aligned.
fn group_array_elements(entries: Vec<String>, numeric: bool, indentation: usize) -> Vec<String> {
    // `getStringWidth` of ASCII: every character from the space up.
    let widths = entries.iter().map(|entry| entry.bytes().filter(|&byte| byte >= b' ').count()).collect::<Vec<_>>();
    let total = widths.iter().map(|width| width + SEPARATOR_SPACE).sum::<usize>();
    let max_width = widths.iter().copied().max().unwrap_or(0);
    let actual_max = max_width + SEPARATOR_SPACE;
    if actual_max * MIN_GROUPED_COLUMNS + indentation >= BREAK_LENGTH
        || !(total as f64 / actual_max as f64 > GROUPED_LENGTH_SHARE || max_width <= SHORT_ENTRY_WIDTH)
    {
        return entries;
    }
    let count = entries.len();
    let average_bias = (actual_max as f64 - total as f64 / count as f64).sqrt();
    let biased_max = (actual_max as f64 - BIASED_MAX_OFFSET - average_bias).max(1.0);
    let columns = ((APPROX_CHAR_HEIGHTS * biased_max * count as f64).sqrt() / biased_max).round() as usize;
    let columns = columns
        .min((BREAK_LENGTH - indentation) / actual_max)
        .min(COMPACT * GROUPED_COLUMNS_PER_COMPACT)
        .min(MAX_GROUPED_COLUMNS);
    if columns <= 1 {
        return entries;
    }
    let column_widths = (0..columns)
        .map(|column| widths.iter().skip(column).step_by(columns).copied().max().unwrap_or(0) + SEPARATOR_SPACE)
        .collect::<Vec<_>>();
    let pad = |text: String, width: usize| {
        let fill = " ".repeat(width.saturating_sub(text.len()));
        if numeric { fill + &text } else { text + &fill }
    };
    let mut rows = Vec::with_capacity(count.div_ceil(columns));
    for start in (0..count).step_by(columns) {
        let end = (start + columns).min(count);
        let mut row = String::new();
        for index in start..end - 1 {
            let width = column_widths[index - start] + entries[index].len() - widths[index];
            row += &pad(format!("{}, ", entries[index]), width);
        }
        let last = end - 1;
        row += &if numeric {
            let width = column_widths[last - start] + entries[last].len() - widths[last] - SEPARATOR_SPACE;
            pad(entries[last].clone(), width)
        } else {
            entries[last].clone()
        };
        rows.push(row);
    }
    rows
}

/// `keyStrRegExp`: a key `formatProperty` writes unquoted.
fn identifier_key(key: &str) -> bool {
    let mut characters = key.chars();
    characters.next().is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// Whether the SDK console could colorize `stream` (`shouldColorize`): `FORCE_COLOR` is set
/// or the stream is a terminal.
fn may_colorize(context: &mut NativeContext<'_, NodeHost>, stream: Stream) -> Result<bool, RootedError> {
    let process = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process is not initialized"))?;
    let key = context.string_rooted("env");
    let env = context.get_property_rooted(process, key)?;
    let key = context.string_rooted(FORCE_COLOR);
    let force = context.get_property_rooted(env, key)?;
    if context.value_type_rooted(force) != Some(quench_runtime::ValueType::Undefined) {
        return Ok(true);
    }
    let key = context.string_rooted(stream.property());
    let stream = context.get_property_rooted(process, key)?;
    let key = context.string_rooted("isTTY");
    let terminal = context.get_property_rooted(stream, key)?;
    context.truthy_rooted(terminal)
}

/// Called as `internal/util/inspect.js` finishes loading (`INSPECT_LOADED`).
pub(crate) fn note_inspect_loaded(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let marker = context.boolean(true);
    cache(context, INSPECT_LOADED, marker)
}

/// Called as `diagnostics_channel.js` finishes loading (`CHANNELS_LOADED`).
pub(crate) fn note_channels_loaded(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let marker = context.boolean(true);
    cache(context, CHANNELS_LOADED, marker)
}

/// Writes `text` to `process[stream]`, the stream cached once first written, as the SDK
/// console does, which ignores write errors.
fn write_stream(context: &mut NativeContext<'_, NodeHost>, stream: Stream, text: &str) -> Result<RootId, RootedError> {
    let stream = match cached(context, stream.cache_key()) {
        Some(cached_stream) => cached_stream,
        None => {
            let process = context
                .host_mut()
                .shared_state()
                .borrow()
                .process_module
                .ok_or_else(|| RootedError::host("process is not initialized"))?;
            let key = context.string_rooted(stream.property());
            let value = context.get_property_rooted(process, key)?;
            cache(context, stream.cache_key(), value)?;
            value
        }
    };
    let key = context.string_rooted("write");
    let write = context.get_property_rooted(stream, key)?;
    let text = context.string_rooted(text);
    let _ = context.call_rooted(write, stream, &[text]);
    Ok(context.undefined())
}

/// The console key Node's `writeOut` (`internal/process/warning.js`) writes warnings with.
const WARNING_KEY: &str = "error";

/// What Node's `writeOut` writes a warning with, read when it first writes one: the global
/// console's `error`, a writer that writes one string without loading the SDK.
pub(crate) fn warning_writer(context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let console = facade(context)?;
    let key = context.string_rooted(WARNING_KEY);
    context.get_property_rooted(console, key)
}

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let console =
        crate::host::shared_vm::commonjs::node_library_module(context, "console.js", "console.js")?;
    let constructor_key = context.string_rooted("Console");
    let constructor = context.get_property_rooted(console, constructor_key)?;
    let prototype_key = context.string_rooted("prototype");
    let prototype = context.get_property_rooted(constructor, prototype_key)?;
    // Node's global console boundary is variadic and names aliases by their
    // namespace keys. Instance methods keep the SDK's original metadata.
    let name = context.string_rooted("name");
    let length = context.string_rooted("length");
    let variadic = context.number(0.0);
    for key in context.own_non_index_keys_rooted(prototype, true)? {
        if context.string_text(key)?.is_none() {
            continue;
        }
        let method = context.get_property_rooted(console, key)?;
        if !context.is_callable_rooted(method)? {
            continue;
        }
        context.define_data_property_rooted(method, name, key, false, false, true)?;
        context.define_data_property_rooted(method, length, variadic, false, false, true)?;
    }
    Ok(console)
}
