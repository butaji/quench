//! `util.format` and `util.inspect` as the console uses them.
//!
//! `inspect` prefers Node's own (`node:util`) when it loads; otherwise default-option
//! inspection of plain data is the console inspector's exact rendering
//! (`console_shared_vm::inspect_flat`), and anything else is written structurally here:
//! primitives, functions, errors (their stack), symbols, custom inspectors, dates, regular
//! expressions, maps, sets, arrays and objects with their constructor's name, to a depth.
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, NativeContext, RootId, RootedError, ValueType};

type Context<'a> = NativeContext<'a, NodeHost>;

/// `inspectDefaultOptions.depth`.
pub(crate) const DEFAULT_DEPTH: f64 = 2.0;
/// `inspectDefaultOptions.breakLength`.
const BREAK_LENGTH: usize = 80;
/// `reduceToSingleString`'s start allowance.
const SINGLE_LINE_START: usize = 10;
/// The indentation of a container's entries.
const ENTRY_INDENTATION: usize = 2;
/// `inspectDefaultOptions.maxArrayLength`.
const DEFAULT_MAX_ARRAY_LENGTH: usize = 100;
/// `%s` inspects objects to this depth.
const STRING_SPECIFIER_DEPTH: f64 = 0.0;
/// `%o` inspects to this depth.
const OBJECT_SPECIFIER_DEPTH: f64 = 4.0;
const FORMAT_SPECIFIER: u16 = b'%' as u16;
const INSPECT_CUSTOM: &str = "nodejs.util.inspect.custom";
/// Node's escapes of the C0 controls, by code.
const CONTROL_ESCAPES: [&str; 32] = [
    "\\x00", "\\x01", "\\x02", "\\x03", "\\x04", "\\x05", "\\x06", "\\x07", "\\b", "\\t", "\\n", "\\x0B", "\\f", "\\r",
    "\\x0E", "\\x0F", "\\x10", "\\x11", "\\x12", "\\x13", "\\x14", "\\x15", "\\x16", "\\x17", "\\x18", "\\x19", "\\x1A",
    "\\x1B", "\\x1C", "\\x1D", "\\x1E", "\\x1F",
];
const DELETE: char = '\u{7f}';

/// The options of one inspection.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Options {
    /// Containers deeper than this are written as `[Type]`; `None` is unlimited.
    pub(crate) depth: Option<f64>,
    pub(crate) max_array_length: usize,
    pub(crate) layout: Layout,
    /// Calls `[util.inspect.custom]` methods.
    pub(crate) custom_inspect: bool,
    /// Writes hidden members: an array's `[length]`.
    pub(crate) show_hidden: bool,
    /// Writes a container's entries sorted.
    pub(crate) sorted: bool,
    /// Writes an error as its stack; otherwise as `[Name: message]`.
    pub(crate) error_stacks: bool,
}

/// How containers break over lines.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    /// On one line when they fit `breakLength` (the default `compact: 3`).
    Fitted,
    /// Never broken (`breakLength: Infinity`).
    SingleLine,
    /// Every non-empty container one entry per line (`compact: false`).
    Expanded,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            depth: Some(DEFAULT_DEPTH),
            max_array_length: DEFAULT_MAX_ARRAY_LENGTH,
            layout: Layout::Fitted,
            custom_inspect: true,
            show_hidden: false,
            sorted: false,
            error_stacks: true,
        }
    }
}

impl Options {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }

    fn with_depth(depth: f64) -> Self {
        Self { depth: Some(depth), ..Self::default() }
    }

    /// The JavaScript options object Node's `inspect` takes.
    fn object(&self, c: &mut Context<'_>) -> Result<RootId, RootedError> {
        let options = c.object_rooted()?;
        let depth = match self.depth {
            Some(depth) => c.number(depth),
            None => c.null(),
        };
        c.assign_named_property_rooted(options, "depth", depth)?;
        let maximum = c.number(self.max_array_length as f64);
        c.assign_named_property_rooted(options, "maxArrayLength", maximum)?;
        match self.layout {
            Layout::Fitted => {}
            Layout::SingleLine => {
                let infinity = c.number(f64::INFINITY);
                c.assign_named_property_rooted(options, "breakLength", infinity)?;
            }
            Layout::Expanded => {
                let no = c.boolean(false);
                c.assign_named_property_rooted(options, "compact", no)?;
            }
        }
        for (name, value) in [("customInspect", self.custom_inspect), ("showHidden", self.show_hidden), ("sorted", self.sorted)] {
            let value = c.boolean(value);
            c.assign_named_property_rooted(options, name, value)?;
        }
        Ok(options)
    }
}

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

/// `util.inspect(value, options)`.
pub(crate) fn inspect(c: &mut Context<'_>, value: RootId, options: Options) -> Result<String, RootedError> {
    if options.is_default() {
        if let Some(text) = super::console_shared_vm::inspect_flat(c, value)? {
            return Ok(text);
        }
    }
    if let Ok(inspect) = super::util_shared_vm::internal_inspect(c) {
        let object = options.object(c)?;
        let undefined = c.undefined();
        let text = c.call_rooted(inspect, undefined, &[value, object])?;
        return c.to_string(text);
    }
    let mut inspection = Inspection { options, seen: Vec::new(), indentation: 0 };
    inspection.value(c, value, 0)
}

/// Node's `strEscape`: the string quoted with the first of `'`, `"` and `` ` `` it does not
/// hold, controls and the quote escaped.
pub(crate) fn quote(text: &str) -> String {
    let quote = if !text.contains('\'') {
        '\''
    } else if !text.contains('"') {
        '"'
    } else if !text.contains('`') && !text.contains("${") {
        '`'
    } else {
        '\''
    };
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push(quote);
    for character in text.chars() {
        match character {
            '\\' => quoted.push_str("\\\\"),
            character if character == quote => {
                quoted.push('\\');
                quoted.push(character);
            }
            character if (character as u32) < CONTROL_ESCAPES.len() as u32 => quoted.push_str(CONTROL_ESCAPES[character as usize]),
            DELETE => quoted.push_str("\\x7F"),
            character => quoted.push(character),
        }
    }
    quoted.push(quote);
    quoted
}

/// `formatNumber`: `-0` as itself.
fn number_text(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let number = c.to_number(value)?;
    Ok(if number == 0.0 && number.is_sign_negative() { "-0".to_owned() } else { c.to_string(value)? })
}

/// One structural inspection's state.
struct Inspection {
    options: Options,
    seen: Vec<RootId>,
    indentation: usize,
}

impl Inspection {
    fn value(&mut self, c: &mut Context<'_>, value: RootId, depth: usize) -> Result<String, RootedError> {
        match c.value_type_rooted(value) {
            Some(ValueType::String) => {
                let text = c.string_text(value)?.unwrap_or_default();
                Ok(quote(&text))
            }
            Some(ValueType::Number) => number_text(c, value),
            Some(ValueType::BigInt) => Ok(c.to_string(value)? + "n"),
            Some(ValueType::Symbol) => symbol_text(c, value),
            Some(ValueType::Object | ValueType::Function) => self.object(c, value, depth),
            _ => c.to_string(value),
        }
    }

    fn object(&mut self, c: &mut Context<'_>, value: RootId, depth: usize) -> Result<String, RootedError> {
        if self.options.custom_inspect {
            if let Some(text) = self.custom(c, value, depth)? {
                return Ok(text);
            }
        }
        if c.is_callable_rooted(value)? {
            return function_text(c, value);
        }
        if c.is_error_rooted(value)? && !self.options.error_stacks {
            return error_header(c, value);
        }
        if c.is_error_rooted(value)? {
            let stack = get(c, value, "stack")?;
            return match c.string_text(stack)? {
                Some(stack) if !stack.is_empty() => Ok(stack),
                _ => error_header(c, value),
            };
        }
        if c.has_builtin_brand_rooted(value, BuiltinBrand::Date)? {
            let iso = get(c, value, "toISOString")?;
            let text = c.call_rooted(iso, value, &[]);
            return match text {
                Ok(text) => c.to_string(text),
                Err(_) => Ok("Invalid Date".to_owned()),
            };
        }
        if c.has_builtin_brand_rooted(value, BuiltinBrand::RegExp)? {
            return c.to_string(value);
        }
        for &seen in &self.seen {
            if c.strict_equal_rooted(seen, value)? {
                return Ok("[Circular *1]".to_owned());
            }
        }
        if c.has_builtin_brand_rooted(value, BuiltinBrand::BoxedPrimitive)? {
            if let Some(quench_runtime::BuiltinData::Primitive { value: primitive, .. }) = c.builtin_data_rooted(value)? {
                let name = constructor_name(c, value)?.unwrap_or_else(|| "Object".to_owned());
                let inner = self.value(c, primitive, depth)?;
                return Ok(format!("[{name}: {inner}]"));
            }
        }
        let typed = c.has_builtin_brand_rooted(value, BuiltinBrand::TypedArray)?;
        let array = typed || c.is_array_rooted(value)?;
        let name = constructor_name(c, value)?;
        let collection = c.collection_preview_rooted(value)?;
        let kind = if array && !typed { "Array".to_owned() } else { name.clone().unwrap_or_else(|| "Object".to_owned()) };
        if self.options.depth.is_some_and(|limit| depth as f64 > limit) {
            return Ok(format!("[{kind}]"));
        }
        self.seen.push(value);
        self.indentation += ENTRY_INDENTATION;
        let mut entries = Vec::new();
        let (open, close, prefix) = if let Some(preview) = collection {
            let size = if preview.is_key_value { preview.entries.len() / 2 } else { preview.entries.len() };
            if preview.is_key_value {
                for pair in preview.entries.chunks(2) {
                    let key = self.value(c, pair[0], depth + 1)?;
                    let entry = self.value(c, pair[1], depth + 1)?;
                    entries.push(format!("{key} => {entry}"));
                }
            } else {
                for &entry in &preview.entries {
                    entries.push(self.value(c, entry, depth + 1)?);
                }
            }
            ("{", "}", format!("{kind}({size}) "))
        } else if array {
            let values = c.array_like_values_rooted(value)?;
            let shown = values.len().min(self.options.max_array_length);
            let mut holes = 0;
            for (index, &entry) in values[..shown].iter().enumerate() {
                let key = c.number(index as f64);
                if !typed && c.own_property_fields_rooted(value, key)?.is_none() {
                    holes += 1;
                    continue;
                }
                if holes > 0 {
                    entries.push(empty_items(holes));
                    holes = 0;
                }
                entries.push(self.value(c, entry, depth + 1)?);
            }
            if holes > 0 {
                entries.push(empty_items(holes));
            }
            if values.len() > shown {
                let more = values.len() - shown;
                entries.push(format!("... {more} more item{}", if more == 1 { "" } else { "s" }));
            }
            if self.options.show_hidden {
                entries.push(format!("[length]: {}", values.len()));
            }
            let prefix = match &name {
                Some(name) if name != "Array" => format!("{name}({}) ", values.len()),
                _ => String::new(),
            };
            ("[", "]", prefix)
        } else {
            let prefix = match &name {
                None => "[Object: null prototype] ".to_owned(),
                Some(name) if name == "Object" => String::new(),
                Some(name) => format!("{name} "),
            };
            ("{", "}", prefix)
        };
        if !array {
            for key in c.enumerable_own_keys_rooted(value, false, true)? {
                let Some(name) = c.string_text(key)? else { continue };
                let property = c.get_property_rooted(value, key)?;
                let entry = self.value(c, property, depth + 1)?;
                let key = if super::console_shared_vm::identifier_key(&name) { name } else { quote(&name) };
                entries.push(format!("{key}: {entry}"));
            }
        }
        self.indentation -= ENTRY_INDENTATION;
        self.seen.pop();
        if entries.is_empty() {
            return Ok(format!("{prefix}{open}{close}"));
        }
        if self.options.sorted && !array {
            entries.sort();
        }
        let start = entries.len() + self.indentation + open.len() + prefix.len() + SINGLE_LINE_START;
        let total = entries.iter().map(String::len).sum::<usize>() + entries.len() + start;
        let joined = entries.join(", ");
        let single = match self.options.layout {
            Layout::SingleLine => true,
            Layout::Fitted => total <= BREAK_LENGTH && !joined.contains('\n'),
            Layout::Expanded => false,
        };
        if single {
            return Ok(format!("{prefix}{open} {joined} {close}"));
        }
        let indentation = format!("\n{}", " ".repeat(self.indentation));
        let entry_indentation = " ".repeat(ENTRY_INDENTATION);
        Ok(format!(
            "{prefix}{open}{indentation}{entry_indentation}{}{indentation}{close}",
            entries.join(&format!(",{indentation}{entry_indentation}"))
        ))
    }

    /// `value[util.inspect.custom](depth, options, inspect)`, when it has one.
    fn custom(&mut self, c: &mut Context<'_>, value: RootId, depth: usize) -> Result<Option<String>, RootedError> {
        let global = c.global_root()?;
        let symbol = get(c, global, "Symbol")?;
        let registry = get(c, symbol, "for")?;
        let description = c.string_rooted(INSPECT_CUSTOM);
        let key = c.call_rooted(registry, symbol, &[description])?;
        let method = c.get_property_rooted(value, key)?;
        if !c.is_callable_rooted(method)? {
            return Ok(None);
        }
        let remaining = match self.options.depth {
            Some(limit) => c.number(limit - depth as f64),
            None => c.number(f64::INFINITY),
        };
        let options = self.options.object(c)?;
        let inspect = c.host_function_named(crate::host::shared_vm::operation("consoleInspect"), "inspect")?;
        let result = c.call_rooted(method, value, &[remaining, options, inspect])?;
        if c.strict_equal_rooted(result, value)? {
            let name = constructor_name(c, value)?.unwrap_or_else(|| "Object".to_owned());
            return Ok(Some(format!("[{name}]")));
        }
        if let Some(text) = c.string_text(result)? {
            return Ok(Some(text));
        }
        self.value(c, result, depth).map(Some)
    }
}

/// `<n empty items>`: a run of array holes.
fn empty_items(count: usize) -> String {
    format!("<{count} empty item{}>", if count == 1 { "" } else { "s" })
}

/// `inspect(value, options)` handed to custom inspectors.
pub(crate) fn inspect_operation(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| c.undefined());
    let mut options = Options::default();
    if let Some(&object) = args.get(1) {
        if c.is_object_rooted(object)? {
            let depth = get(c, object, "depth")?;
            match c.rooted_value(depth) {
                Some(depth) if depth.is_null() => options.depth = None,
                Some(depth) => {
                    if let Some(depth) = depth.as_number() {
                        options.depth = Some(depth);
                    }
                }
                None => {}
            }
        }
    }
    let text = inspect(c, value, options)?;
    Ok(c.string_rooted(&text))
}

/// The name a container is labeled with: its constructor's, or `None` for a null prototype.
fn constructor_name(c: &mut Context<'_>, value: RootId) -> Result<Option<String>, RootedError> {
    let mut prototype = c.prototype_rooted(value)?;
    while c.is_object_rooted(prototype)? {
        let key = c.string_rooted("constructor");
        if let Some(fields) = c.own_property_fields_rooted(prototype, key)? {
            if let Some(constructor) = fields.value {
                if c.is_callable_rooted(constructor)? {
                    let name = get(c, constructor, "name")?;
                    if let Some(name) = c.string_text(name)? {
                        if !name.is_empty() {
                            return Ok(Some(name));
                        }
                    }
                }
            }
        }
        prototype = c.prototype_rooted(prototype)?;
    }
    Ok(None)
}

fn symbol_text(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let global = c.global_root()?;
    let string = get(c, global, "String")?;
    let undefined = c.undefined();
    let text = c.call_rooted(string, undefined, &[value])?;
    c.to_string(text)
}

/// `[Function: name]`, `[Function (anonymous)]` or `[class Name]`.
fn function_text(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let name = get(c, value, "name")?;
    let name = c.string_text(name)?.unwrap_or_default();
    let global = c.global_root()?;
    let function = get(c, global, "Function")?;
    let prototype = get(c, function, "prototype")?;
    let to_string = get(c, prototype, "toString")?;
    let source = c.call_rooted(to_string, value, &[])?;
    let class = c.string_text(source)?.is_some_and(|source| source.starts_with("class"));
    Ok(match (class, name.is_empty()) {
        (true, true) => "[class (anonymous)]".to_owned(),
        (true, false) => format!("[class {name}]"),
        (false, true) => "[Function (anonymous)]".to_owned(),
        (false, false) => format!("[Function: {name}]"),
    })
}

/// `[Name: message]`, an error without a stack.
fn error_header(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let name = get(c, value, "name")?;
    let name = c.to_string(name)?;
    let message = get(c, value, "message")?;
    let message = c.to_string(message)?;
    Ok(if message.is_empty() { format!("[{name}]") } else { format!("[{name}: {message}]") })
}


/// Whether `%s` writes `value` with `String(value)`: a primitive, or an object whose
/// `toString` is not one of the builtin ones (`hasBuiltInToString`).
fn has_builtin_to_string(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    let method = get(c, value, "toString")?;
    let global = c.global_root()?;
    for owner in ["Object", "Error", "Array", "Date", "RegExp", "Function", "Number", "Boolean", "String", "Symbol", "BigInt"] {
        let constructor = get(c, global, owner)?;
        let prototype = get(c, constructor, "prototype")?;
        let builtin = get(c, prototype, "toString")?;
        if c.strict_equal_rooted(method, builtin)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// A global function of one argument (`parseInt`, `parseFloat`) applied to `value`.
fn global_call(c: &mut Context<'_>, name: &str, value: RootId) -> Result<RootId, RootedError> {
    let global = c.global_root()?;
    let function = get(c, global, name)?;
    let undefined = c.undefined();
    c.call_rooted(function, undefined, &[value])
}

/// `%d`, `%i` and `%f`: a number from `value`, `NaN` for a symbol, a bigint as itself.
fn numeric_specifier(c: &mut Context<'_>, value: RootId, conversion: Option<&str>) -> Result<String, RootedError> {
    match c.value_type_rooted(value) {
        Some(ValueType::BigInt) if conversion != Some("parseFloat") => Ok(c.to_string(value)? + "n"),
        Some(ValueType::Symbol) => Ok("NaN".to_owned()),
        _ => {
            let number = match conversion {
                Some(name) => global_call(c, name, value)?,
                None => {
                    let number = c.to_number(value)?;
                    c.number(number)
                }
            };
            number_text(c, number)
        }
    }
}

/// `%j`: `JSON.stringify`, `[Circular]` for a cycle.
fn json_specifier(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let global = c.global_root()?;
    let json = get(c, global, "JSON")?;
    let stringify = get(c, json, "stringify")?;
    match c.call_rooted(stringify, json, &[value]) {
        Ok(text) => c.to_string(text),
        Err(error) => {
            let circular = match error.exception {
                Some(exception) => {
                    let message = get(c, exception, "message")?;
                    c.to_string(message)?.contains("circular")
                }
                None => false,
            };
            if circular { Ok("[Circular]".to_owned()) } else { Err(error) }
        }
    }
}

/// `util.format(...args)` with default inspection options (`formatWithOptionsInternal`).
pub(crate) fn format(c: &mut Context<'_>, args: &[RootId]) -> Result<String, RootedError> {
    let mut output = String::new();
    let mut next = 0;
    let mut join = "";
    if let Some(&first) = args.first() {
        if let Some(units) = c.string_code_units(first)? {
            if args.len() == 1 {
                return Ok(String::from_utf16_lossy(&units));
            }
            let mut last = 0;
            let mut index = 0;
            while index + 1 < units.len() {
                if units[index] != FORMAT_SPECIFIER {
                    index += 1;
                    continue;
                }
                index += 1;
                let specifier = char::from_u32(u32::from(units[index])).unwrap_or_default();
                if next + 1 == args.len() {
                    if specifier == '%' {
                        output += &String::from_utf16_lossy(&units[last..index]);
                        last = index + 1;
                    }
                    index += 1;
                    continue;
                }
                let text = match specifier {
                    's' => {
                        next += 1;
                        let value = args[next];
                        match c.value_type_rooted(value) {
                            Some(ValueType::Number) => number_text(c, value)?,
                            Some(ValueType::BigInt) => c.to_string(value)? + "n",
                            Some(ValueType::Object) if has_builtin_to_string(c, value)? => {
                                inspect(c, value, Options::with_depth(STRING_SPECIFIER_DEPTH))?
                            }
                            Some(ValueType::Symbol) => symbol_text(c, value)?,
                            _ => c.to_string(value)?,
                        }
                    }
                    'j' => {
                        next += 1;
                        json_specifier(c, args[next])?
                    }
                    'd' => {
                        next += 1;
                        numeric_specifier(c, args[next], None)?
                    }
                    'O' => {
                        next += 1;
                        inspect(c, args[next], Options::default())?
                    }
                    'o' => {
                        next += 1;
                        inspect(c, args[next], Options { show_hidden: true, ..Options::with_depth(OBJECT_SPECIFIER_DEPTH) })?
                    }
                    'i' => {
                        next += 1;
                        numeric_specifier(c, args[next], Some("parseInt"))?
                    }
                    'f' => {
                        next += 1;
                        numeric_specifier(c, args[next], Some("parseFloat"))?
                    }
                    'c' => {
                        next += 1;
                        String::new()
                    }
                    '%' => {
                        output += &String::from_utf16_lossy(&units[last..index]);
                        last = index + 1;
                        index += 1;
                        continue;
                    }
                    _ => {
                        index += 1;
                        continue;
                    }
                };
                if last != index - 1 {
                    output += &String::from_utf16_lossy(&units[last..index - 1]);
                }
                output += &text;
                last = index + 1;
                index += 1;
            }
            if last != 0 {
                next += 1;
                join = " ";
                if last < units.len() {
                    output += &String::from_utf16_lossy(&units[last..]);
                }
            }
        }
    }
    while next < args.len() {
        let value = args[next];
        output += join;
        match c.string_text(value)? {
            Some(text) => output += &text,
            None => output += &inspect(c, value, Options::default())?,
        }
        join = " ";
        next += 1;
    }
    Ok(output)
}
