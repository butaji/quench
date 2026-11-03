//! Source map support (lib/internal/source_map): the support switches
//! `--enable-source-maps`, `process.setSourceMapsEnabled` and `module.setSourceMapsSupport`
//! set, the cache of the source maps loaded modules name, and `module.SourceMap`.
use super::event_constructor_shared_vm::get;
use crate::host::shared_vm::{commonjs, operation};
use crate::host::NodeHost;
use quench_runtime::{DescriptorFields, NativeContext, RootId, RootedError};
use std::cell::Cell;

type Context<'a> = NativeContext<'a, NodeHost>;

/// Which source maps are honoured (`module.getSourceMapsSupport()`).
#[derive(Clone, Copy)]
pub(crate) struct Support {
    pub(crate) enabled: bool,
    pub(crate) node_modules: bool,
    pub(crate) generated_code: bool,
}

thread_local! {
    /// The support switches, read from `--enable-source-maps` on first use.
    static SUPPORT: Cell<Option<Support>> = const { Cell::new(None) };
}

/// The memoized `getSourceMapsSupport()` object, replaced when the switches change.
const SUPPORT_OBJECT: &str = "\0native:source-maps-support";
/// The cache of loaded modules' source maps, by module URL.
const MODULE_CACHE: &str = "\0native:source-map-cache";
const MAPPINGS_SLOT: &str = "\0SourceMap.mappings";
const PAYLOAD_SLOT: &str = "\0SourceMap.payload";
const LINE_LENGTHS_SLOT: &str = "\0SourceMap.lineLengths";

pub(crate) fn support(c: &mut Context<'_>) -> Support {
    if let Some(support) = SUPPORT.get() {
        return support;
    }
    let enabled = c.host_mut().shared_state().borrow().cli_options.enabled("--enable-source-maps");
    let support = Support { enabled, node_modules: enabled, generated_code: enabled };
    SUPPORT.set(Some(support));
    support
}

pub(crate) fn set_support(c: &mut Context<'_>, support: Support) {
    SUPPORT.set(Some(support));
    if let Some(cell) = c.host_mut().shared_state().borrow().builtin_cache.get(SUPPORT_OBJECT).copied() {
        let undefined = c.undefined();
        let key = c.string_rooted("0");
        let _ = c.set_property_rooted(cell, key, undefined, cell);
    }
}

fn argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}

fn is_undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined())
}

fn put(c: &mut Context<'_>, target: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(target, key, value, true, true, true)?;
    Ok(())
}

fn validate_boolean(c: &mut Context<'_>, value: RootId, name: &str) -> Result<bool, RootedError> {
    match c.rooted_value(value).and_then(|value| value.as_bool()) {
        Some(value) => Ok(value),
        None => Err(super::util_shared_vm::argument_type_error(c, value, name, "boolean")?),
    }
}

/// `module.getSourceMapsSupport()`: a frozen `{ enabled, nodeModules, generatedCode }`.
pub(crate) fn get_support(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let cell = commonjs::cached_native_binding(c, SUPPORT_OBJECT, |c| c.array_rooted(&[]))?;
    let memoized = get(c, cell, "0")?;
    if !is_undefined(c, memoized) {
        return Ok(memoized);
    }
    let support = support(c);
    let object = c.null_object_rooted()?;
    for (name, value) in [
        ("enabled", support.enabled),
        ("nodeModules", support.node_modules),
        ("generatedCode", support.generated_code),
    ] {
        let value = c.boolean(value);
        put(c, object, name, value)?;
    }
    c.prevent_extensions_rooted(object)?;
    for name in ["enabled", "nodeModules", "generatedCode"] {
        let key = c.string_rooted(name);
        let fields = DescriptorFields { writable: Some(false), configurable: Some(false), ..DescriptorFields::default() };
        c.define_property_fields_rooted(object, key, fields)?;
    }
    put(c, cell, "0", object)?;
    Ok(object)
}

/// `module.setSourceMapsSupport(enabled[, { nodeModules, generatedCode }])`.
pub(crate) fn set_support_operation(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let enabled = argument(c, args, 0);
    let enabled = validate_boolean(c, enabled, "enabled")?;
    let options = argument(c, args, 1);
    let mut switches = [false; 2];
    if !is_undefined(c, options) {
        super::perf_hooks::validate_object(c, options, "options")?;
        for (switch, name) in switches.iter_mut().zip(["nodeModules", "generatedCode"]) {
            let value = get(c, options, name)?;
            if !is_undefined(c, value) {
                *switch = validate_boolean(c, value, &format!("options.{name}"))?;
            }
        }
    }
    set_support(c, Support { enabled, node_modules: switches[0], generated_code: switches[1] });
    Ok(c.undefined())
}

/// The support functions `node:module` and `internal/source_map/source_map_cache` export.
const SUPPORT_FUNCTIONS: [(&str, &str); 3] = [
    ("findSourceMap", "sourceMapFind"),
    ("getSourceMapsSupport", "sourceMapsSupportGet"),
    ("setSourceMapsSupport", "sourceMapsSupportSet"),
];

/// Defines `findSourceMap`, `getSourceMapsSupport`, `setSourceMapsSupport` and, for
/// `node:module`, `SourceMap` on `target`.
pub(crate) fn install(c: &mut Context<'_>, target: RootId, with_class: bool) -> Result<(), RootedError> {
    for (name, operation_name) in SUPPORT_FUNCTIONS {
        let function = c.host_function_named(operation(operation_name), name)?;
        put(c, target, name, function)?;
    }
    if with_class {
        let class = source_map_class(c)?;
        put(c, target, "SourceMap", class)?;
    }
    Ok(())
}

/// `internal/source_map/source_map_cache`.
pub(crate) fn cache_module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let module = c.object_rooted()?;
    install(c, module, false)?;
    Ok(module)
}

/// The last `//# sourceMappingURL=` (or `/*# … */`) comment's URL in `source`.
fn source_mapping_url(source: &str) -> Option<&str> {
    const MARKER: &str = "sourceMappingURL=";
    let mut found = None;
    let mut rest = source;
    while let Some(index) = rest.find("#") {
        let (before, after) = rest.split_at(index);
        let after = &after[1..];
        let introduced = before.ends_with("//") || before.ends_with("/*");
        let spaced = after.trim_start();
        if introduced && spaced.len() < after.len() {
            if let Some(url) = spaced.strip_prefix(MARKER) {
                let end = url.find(char::is_whitespace).unwrap_or(url.len());
                if end > 0 {
                    found = Some(&url[..end]);
                }
            }
        }
        rest = after;
    }
    found
}

/// Node's `lineLengths(content)`: each line's UTF-16 length, lines ending at `\n`, U+2028
/// or U+2029 (a `\r` stays in its line).
fn line_lengths(source: &str) -> Vec<f64> {
    let mut lengths = Vec::new();
    let mut length = 0usize;
    for character in source.chars() {
        if matches!(character, '\n' | '\u{2028}' | '\u{2029}') {
            lengths.push(length as f64);
            length = 0;
        } else {
            length += character.len_utf16();
        }
    }
    lengths.push(length as f64);
    lengths
}

/// The source map `url` names relative to the module at `module_url`, with its sources
/// made absolute (Node's `sourcesToAbsolute`), as JSON.
fn source_map_data(module_url: &url::Url, url: &str) -> Option<String> {
    let mut map: serde_json::Value = match url::Url::parse(url) {
        Ok(parsed) if parsed.scheme() == "data" => {
            let (format, data) = parsed.path().split_once(',')?;
            let mut parts = format.split(';');
            if parts.next() != Some("application/json") {
                return None;
            }
            let text = if format.split(';').next_back() == Some("base64") {
                use base64::Engine;
                String::from_utf8(base64::engine::general_purpose::STANDARD.decode(data).ok()?).ok()?
            } else {
                data.to_owned()
            };
            serde_json::from_str(&text).ok()?
        }
        Ok(_) => return None,
        Err(_) => {
            let path = module_url.join(url).ok()?.to_file_path().ok()?;
            serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?
        }
    };
    let root = map.get("sourceRoot").and_then(|root| root.as_str()).unwrap_or_default().to_owned();
    let sources = map.get_mut("sources")?.as_array_mut()?;
    for source in sources.iter_mut() {
        let relative = format!("{root}{}", source.as_str()?);
        *source = serde_json::Value::String(module_url.join(&relative).ok()?.to_string());
    }
    map.as_object_mut()?.insert("sourceRoot".into(), serde_json::Value::String(String::new()));
    serde_json::to_string(&map).ok()
}

fn under_node_modules(url: &str) -> bool {
    url.contains("/node_modules/")
}

/// Node's `maybeCacheSourceMap` for a CommonJS module compiled from `source`: with source
/// maps enabled, records the map its magic comment names under the module's URL.
pub(crate) fn maybe_cache(c: &mut Context<'_>, filename: &std::path::Path, source: &str) -> Result<(), RootedError> {
    let support = support(c);
    if !support.enabled {
        return Ok(());
    }
    let Ok(module_url) = url::Url::from_file_path(filename) else {
        return Ok(());
    };
    if !support.node_modules && under_node_modules(module_url.as_str()) {
        return Ok(());
    }
    let Some(map_url) = source_mapping_url(source) else {
        return Ok(());
    };
    let entry = c.null_object_rooted()?;
    let lengths = line_lengths(source).into_iter().map(|length| c.number(length)).collect::<Vec<_>>();
    let lengths = c.array_rooted(&lengths)?;
    put(c, entry, "lineLengths", lengths)?;
    let data = match source_map_data(&module_url, map_url) {
        Some(json) => c.parse_json_rooted(&json)?,
        None => c.null(),
    };
    put(c, entry, "data", data)?;
    let cache = commonjs::cached_native_binding(c, MODULE_CACHE, |c| c.null_object_rooted())?;
    put(c, cache, module_url.as_str(), entry)
}

/// `module.findSourceMap(path)`: the `SourceMap` of the loaded module at `path` (a path or
/// URL), made on first request.
pub(crate) fn find_source_map(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let path = argument(c, args, 0);
    let Some(path) = c.string_text(path)? else {
        return Ok(undefined);
    };
    if path.starts_with("node:") || (!support(c).node_modules && under_node_modules(&path)) {
        return Ok(undefined);
    }
    let url = match url::Url::parse(&path) {
        Ok(url) if url.scheme().len() > 1 => url.to_string(),
        _ => match url::Url::from_file_path(&path) {
            Ok(url) => url.to_string(),
            Err(()) => return Ok(undefined),
        },
    };
    let cache = commonjs::cached_native_binding(c, MODULE_CACHE, |c| c.null_object_rooted())?;
    let entry = get(c, cache, &url)?;
    if is_undefined(c, entry) {
        return Ok(undefined);
    }
    let data = get(c, entry, "data")?;
    if c.rooted_value(data).is_some_and(|value| value.is_null()) {
        return Ok(undefined);
    }
    let made = get(c, entry, "sourceMap")?;
    if !is_undefined(c, made) {
        return Ok(made);
    }
    let options = c.null_object_rooted()?;
    let lengths = get(c, entry, "lineLengths")?;
    put(c, options, "lineLengths", lengths)?;
    let constructor = source_map_class(c)?;
    let made = c.construct_rooted(constructor, constructor, &[data, options])?;
    put(c, entry, "sourceMap", made)?;
    Ok(made)
}

/// `module.SourceMap`.
pub(crate) fn source_map_class(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    commonjs::cached_native_binding(c, "\0native:SourceMap", |c| {
        let class = c.host_constructor_named(crate::host::shared_vm::operation!("sourceMapConstruct"), "SourceMap")?;
        let prototype = get(c, class, "prototype")?;
        for (name, operation_name) in [("payload", "sourceMapPayload"), ("lineLengths", "sourceMapLineLengths")] {
            let getter = c.host_function_named(operation(operation_name), &format!("get {name}"))?;
            let key = c.string_rooted(name);
            let undefined = c.undefined();
            let fields = DescriptorFields {
                get: Some(getter),
                set: Some(undefined),
                enumerable: Some(false),
                configurable: Some(true),
                ..DescriptorFields::default()
            };
            c.define_property_fields_rooted(prototype, key, fields)?;
        }
        for (name, operation_name) in [("findEntry", "sourceMapFindEntry"), ("findOrigin", "sourceMapFindOrigin")] {
            let function = c.host_function_named(operation(operation_name), name)?;
            let key = c.string_rooted(name);
            c.define_data_property_rooted(prototype, key, function, true, false, true)?;
        }
        Ok(class)
    })
}

/// Node's `cloneSourceMapV3`: a copy whose array members are copied too.
fn clone_payload(c: &mut Context<'_>, payload: RootId) -> Result<RootId, RootedError> {
    super::perf_hooks::validate_object(c, payload, "payload")?;
    let clone = c.object_rooted()?;
    for key in c.enumerable_own_keys_rooted(payload, true, true)? {
        let mut value = c.get_property_rooted(payload, key)?;
        if c.is_array_rooted(value)? {
            let values = c.array_like_values_rooted(value)?;
            value = c.array_rooted(&values)?;
        }
        c.define_data_property_rooted(clone, key, value, true, true, true)?;
    }
    Ok(clone)
}

/// One decoded mapping: its generated position and, unless it maps nothing, its source,
/// original position and name.
struct Mapping {
    line: f64,
    column: f64,
    origin: Option<[RootId; 4]>,
}

/// Base64 VLQ (source map v3): five value bits per digit, the sixth continues.
const VLQ_BASE_SHIFT: u32 = 5;
const VLQ_BASE_MASK: i32 = (1 << VLQ_BASE_SHIFT) - 1;
const VLQ_CONTINUATION_BIT: i32 = 1 << VLQ_BASE_SHIFT;
const BASE64_DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Node's `StringCharIterator` over a mappings string's UTF-16 units.
struct Units<'a> {
    units: &'a [u16],
    position: usize,
}

impl Units<'_> {
    fn peek(&self) -> Option<u16> {
        self.units.get(self.position).copied()
    }
    fn next(&mut self) -> Option<u16> {
        let unit = self.peek();
        self.position += 1;
        unit
    }
    fn has_next(&self) -> bool {
        self.position < self.units.len()
    }
    fn separator(&self) -> bool {
        matches!(self.peek(), Some(unit) if unit == u16::from(b',') || unit == u16::from(b';'))
    }
    /// Node's `decodeVLQ`, with its int32 arithmetic; an unknown digit reads as zero.
    fn vlq(&mut self) -> f64 {
        let mut result = 0.0;
        let mut shift = 0u32;
        loop {
            let digit = self
                .next()
                .and_then(|unit| BASE64_DIGITS.iter().position(|digit| u16::from(*digit) == unit))
                .map_or(0, |digit| digit as i32);
            result += f64::from((digit & VLQ_BASE_MASK).wrapping_shl(shift));
            shift += VLQ_BASE_SHIFT;
            if digit & VLQ_CONTINUATION_BIT == 0 {
                break;
            }
        }
        let bits = to_uint32(result);
        let magnitude = bits >> 1;
        if bits & 1 == 0 {
            f64::from(magnitude)
        } else {
            f64::from((-(f64::from(magnitude)) as i64 as i32) | i32::MIN)
        }
    }
}

fn to_uint32(value: f64) -> u32 {
    const TWO_TO_32: f64 = 4_294_967_296.0;
    if !value.is_finite() {
        return 0;
    }
    value.trunc().rem_euclid(TWO_TO_32) as u32
}

/// Node's `#parseMap(map, line, column)`, appending to `mappings`.
fn parse_map(c: &mut Context<'_>, map: RootId, mut line: f64, mut column: f64, mappings: &mut Vec<Mapping>) -> Result<(), RootedError> {
    let sources = get(c, map, "sources")?;
    let sources = c.array_like_values_rooted(sources)?;
    let names = get(c, map, "names")?;
    let text = get(c, map, "mappings")?;
    let units = c.to_string_code_units(text)?;
    let mut units = Units { units: &units, position: 0 };
    let undefined = c.undefined();
    let source_at = |index: f64| if index >= 0.0 { sources.get(index as usize).copied() } else { None };
    let (mut source_index, mut source_line, mut source_column, mut name_index) = (0.0, 0.0, 0.0, 0.0);
    let mut source = source_at(source_index).unwrap_or(undefined);
    loop {
        if units.peek() == Some(u16::from(b',')) {
            units.next();
        } else {
            while units.peek() == Some(u16::from(b';')) {
                line += 1.0;
                column = 0.0;
                units.next();
            }
            if !units.has_next() {
                break;
            }
        }
        column += units.vlq();
        if units.separator() {
            mappings.push(Mapping { line, column, origin: None });
            continue;
        }
        let source_delta = units.vlq();
        if source_delta != 0.0 {
            source_index += source_delta;
            source = source_at(source_index).unwrap_or(undefined);
        }
        source_line += units.vlq();
        source_column += units.vlq();
        let mut name = undefined;
        if !units.separator() {
            name_index += units.vlq();
            if c.is_object_rooted(names)? {
                let key = c.number(name_index);
                name = c.get_property_rooted(names, key)?;
            }
        }
        let original_line = c.number(source_line);
        let original_column = c.number(source_column);
        mappings.push(Mapping { line, column, origin: Some([source, original_line, original_column, name]) });
    }
    Ok(())
}

/// `new SourceMap(payload[, { lineLengths }])`.
pub(crate) fn construct(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if c.new_target_rooted().is_none() {
        let error = c.type_error_rooted("Class constructor SourceMap cannot be invoked without 'new'")?;
        return Err(c.throw(error));
    }
    let payload = argument(c, args, 0);
    let options = argument(c, args, 1);
    let line_lengths = if is_undefined(c, options) { c.undefined() } else { get(c, options, "lineLengths")? };
    let payload = clone_payload(c, payload)?;
    let mut mappings = Vec::new();
    let sections = get(c, payload, "sections")?;
    if c.truthy_rooted(sections)? {
        for section in c.array_like_values_rooted(sections)? {
            let offset = get(c, section, "offset")?;
            let line = get(c, offset, "line")?;
            let line = c.to_number(line)?;
            let column = get(c, offset, "column")?;
            let column = c.to_number(column)?;
            let map = get(c, section, "map")?;
            parse_map(c, map, line, column, &mut mappings)?;
        }
    } else {
        parse_map(c, payload, 0.0, 0.0, &mut mappings)?;
    }
    mappings.sort_by(|left, right| left.line.total_cmp(&right.line).then(left.column.total_cmp(&right.column)));
    let mut entries = Vec::with_capacity(mappings.len());
    for mapping in mappings {
        let line = c.number(mapping.line);
        let column = c.number(mapping.column);
        let entry = match mapping.origin {
            Some([source, original_line, original_column, name]) => c.array_rooted(&[line, column, source, original_line, original_column, name])?,
            None => c.array_rooted(&[line, column])?,
        };
        entries.push(entry);
    }
    let entries = c.array_rooted(&entries)?;
    super::private_slots::write(c, this, MAPPINGS_SLOT, entries)?;
    super::private_slots::write(c, this, PAYLOAD_SLOT, payload)?;
    if c.is_array_rooted(line_lengths)? && !c.array_like_values_rooted(line_lengths)?.is_empty() {
        super::private_slots::write(c, this, LINE_LENGTHS_SLOT, line_lengths)?;
    }
    Ok(c.undefined())
}

fn slot(c: &mut Context<'_>, this: RootId, name: &str) -> Result<RootId, RootedError> {
    match super::private_slots::read(c, this, MAPPINGS_SLOT)? {
        Some(_) => Ok(super::private_slots::read(c, this, name)?.unwrap_or_else(|| c.undefined())),
        None => {
            let error = c.type_error_rooted("Receiver must be an instance of class SourceMap")?;
            Err(c.throw(error))
        }
    }
}

pub(crate) fn payload(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let payload = slot(c, this, PAYLOAD_SLOT)?;
    clone_payload(c, payload)
}

pub(crate) fn line_lengths_getter(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let lengths = slot(c, this, LINE_LENGTHS_SLOT)?;
    if is_undefined(c, lengths) {
        return Ok(lengths);
    }
    let values = c.array_like_values_rooted(lengths)?;
    c.array_rooted(&values)
}

/// The fields of a mapping entry, in Node's order.
const ENTRY_FIELDS: [&str; 6] = ["generatedLine", "generatedColumn", "originalSource", "originalLine", "originalColumn", "name"];

/// The entry at or before generated `(line, column)`, by binary search over the sorted
/// mappings, or `None` before the first.
fn entry_at(c: &mut Context<'_>, this: RootId, line: f64, column: f64) -> Result<Option<RootId>, RootedError> {
    let mappings = slot(c, this, MAPPINGS_SLOT)?;
    let mappings = c.array_like_values_rooted(mappings)?;
    let position = |c: &mut Context<'_>, entry: RootId| -> Result<(f64, f64), RootedError> {
        let line = get(c, entry, "0")?;
        let column = get(c, entry, "1")?;
        Ok((c.to_number(line)?, c.to_number(column)?))
    };
    let before = |(entry_line, entry_column): (f64, f64)| line < entry_line || (line == entry_line && column < entry_column);
    let (mut first, mut count) = (0, mappings.len());
    while count > 1 {
        let step = count >> 1;
        let middle = first + step;
        if before(position(c, mappings[middle])?) {
            count = step;
        } else {
            first = middle;
            count -= step;
        }
    }
    let Some(&entry) = mappings.get(first) else {
        return Ok(None);
    };
    if first == 0 && before(position(c, entry)?) {
        return Ok(None);
    }
    Ok(Some(entry))
}

fn entry_field(c: &mut Context<'_>, entry: RootId, index: usize) -> Result<RootId, RootedError> {
    get(c, entry, &index.to_string())
}

/// `findEntry(lineOffset, columnOffset)`, zero-based.
pub(crate) fn find_entry(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let line = argument(c, args, 0);
    let line = c.to_number(line)?;
    let column = argument(c, args, 1);
    let column = c.to_number(column)?;
    let result = c.object_rooted()?;
    if let Some(entry) = entry_at(c, this, line, column)? {
        for (index, name) in ENTRY_FIELDS.iter().enumerate() {
            let value = entry_field(c, entry, index)?;
            put(c, result, name, value)?;
        }
    }
    Ok(result)
}

/// `findOrigin(lineNumber, columnNumber)`, one-based: the original position, offset from
/// the entry's as the request is from the entry's generated position.
pub(crate) fn find_origin(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let line = argument(c, args, 0);
    let line = c.to_number(line)?;
    let column = argument(c, args, 1);
    let column = c.to_number(column)?;
    let result = c.object_rooted()?;
    let Some(entry) = entry_at(c, this, line - 1.0, column - 1.0)? else {
        return Ok(result);
    };
    let mut fields = Vec::with_capacity(ENTRY_FIELDS.len());
    for index in 0..ENTRY_FIELDS.len() {
        fields.push(entry_field(c, entry, index)?);
    }
    if fields[..ENTRY_FIELDS.len() - 1].iter().any(|value| is_undefined(c, *value)) {
        return Ok(result);
    }
    let [generated_line, generated_column, source, original_line, original_column, name] = fields[..] else {
        return Ok(result);
    };
    let numbers = [generated_line, generated_column, original_line, original_column].map(|value| c.rooted_value(value).and_then(|value| value.as_number()).unwrap_or(f64::NAN));
    put(c, result, "name", name)?;
    put(c, result, "fileName", source)?;
    let line = c.number(numbers[2] + (line - numbers[0]));
    put(c, result, "lineNumber", line)?;
    let column = c.number(numbers[3] + (column - numbers[1]));
    put(c, result, "columnNumber", column)?;
    Ok(result)
}
