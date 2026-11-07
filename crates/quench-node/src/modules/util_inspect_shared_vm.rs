//! Node's `util.inspect`, `util.format` and `util.formatWithOptions` (lib/internal/util/inspect.js)
//! in Rust. One formatter serves the `util` module, the console and every coded error that
//! inspects a received value.
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, BuiltinData, DescriptorFields, NativeContext, PromiseSnapshot, RootId, RootedError, ValueType};

type Context<'a> = NativeContext<'a, NodeHost>;

/// `kMinLineWidth`: strings at most this long are never split into lines.
const MIN_LINE_WIDTH: usize = 16;
use crate::modules::console_shared_vm::STRING_LINE_RESERVE;
use crate::modules::console_shared_vm::ENTRY_INDENTATION;
/// `formatProperty` indents object properties by three columns in `compact: true` mode.
const SINGLE_LINE_PROPERTY_INDENTATION: usize = 3;
use crate::modules::console_shared_vm::SINGLE_LINE_START;
/// `reduceToSingleString` groups arrays with more output entries than this.
const GROUPING_MIN_ENTRIES: usize = 6;
use crate::modules::console_shared_vm::SEPARATOR_SPACE;
/// `groupArrayElements`: a character is about this many times as high as wide.
const APPROXIMATE_CHARACTER_HEIGHT: f64 = 2.5;
/// `groupArrayElements`: at least this many columns must fit next to each other.
const MIN_GROUP_COLUMNS: usize = 3;
/// `groupArrayElements`: an entry longer than this share of the total prevents grouping.
const GROUP_LENGTH_SHARE: f64 = 5.0;
/// `groupArrayElements`: entries at most this long group whatever their share.
const GROUP_SHORT_ENTRY: usize = 6;
/// `groupArrayElements`: columns per `compact` level.
const COLUMNS_PER_COMPACT_LEVEL: f64 = 4.0;
/// `groupArrayElements`: the most columns of a grouped array.
const MAX_GROUP_COLUMNS: f64 = 15.0;
/// `formatRaw`: output past this many units at one indentation stops further recursion.
const OUTPUT_BUDGET: usize = 1 << 27;
/// `formatSpecialArray`: arrays hold at most 2^32 - 1 elements.
const MAX_ARRAY_INDEX: f64 = 4_294_967_294.0;
/// `identicalSequenceRange`: shared frame runs shorter than this are not elided.
const MIN_IDENTICAL_FRAMES: usize = 3;
/// The frames an elided run keeps visible around its message.
const KEPT_IDENTICAL_FRAMES: usize = 2;
pub(crate) use crate::modules::keys::INSPECT_CUSTOM;
/// `inspectDefaultOptions`, in Node's order.
pub(crate) const DEFAULT_OPTIONS: [(&str, DefaultOption); 12] = [
    ("showHidden", DefaultOption::Boolean(false)),
    ("depth", DefaultOption::Number(2.0)),
    ("colors", DefaultOption::Boolean(false)),
    ("customInspect", DefaultOption::Boolean(true)),
    ("showProxy", DefaultOption::Boolean(false)),
    ("maxArrayLength", DefaultOption::Number(100.0)),
    ("maxStringLength", DefaultOption::Number(10_000.0)),
    ("breakLength", DefaultOption::Number(80.0)),
    ("compact", DefaultOption::Number(3.0)),
    ("sorted", DefaultOption::Boolean(false)),
    ("getters", DefaultOption::Boolean(false)),
    ("numericSeparator", DefaultOption::Boolean(false)),
];

#[derive(Clone, Copy)]
pub(crate) enum DefaultOption {
    Boolean(bool),
    Number(f64),
}

/// The SGR foreground and background resets the color table closes with.
const DEFAULT_FOREGROUND: u8 = 39;
const DEFAULT_BACKGROUND: u8 = 49;
/// `inspect.colors`: each name's SGR open and close codes.
pub(crate) const COLORS: &[(&str, u8, u8)] = &[
    ("reset", 0, 0),
    ("bold", 1, 22),
    ("dim", 2, 22),
    ("italic", 3, 23),
    ("underline", 4, 24),
    ("blink", 5, 25),
    ("inverse", 7, 27),
    ("hidden", 8, 28),
    ("strikethrough", 9, 29),
    ("doubleunderline", 21, 24),
    ("black", 30, DEFAULT_FOREGROUND),
    ("red", 31, DEFAULT_FOREGROUND),
    ("green", 32, DEFAULT_FOREGROUND),
    ("yellow", 33, DEFAULT_FOREGROUND),
    ("blue", 34, DEFAULT_FOREGROUND),
    ("magenta", 35, DEFAULT_FOREGROUND),
    ("cyan", 36, DEFAULT_FOREGROUND),
    ("white", 37, DEFAULT_FOREGROUND),
    ("bgBlack", 40, DEFAULT_BACKGROUND),
    ("bgRed", 41, DEFAULT_BACKGROUND),
    ("bgGreen", 42, DEFAULT_BACKGROUND),
    ("bgYellow", 43, DEFAULT_BACKGROUND),
    ("bgBlue", 44, DEFAULT_BACKGROUND),
    ("bgMagenta", 45, DEFAULT_BACKGROUND),
    ("bgCyan", 46, DEFAULT_BACKGROUND),
    ("bgWhite", 47, DEFAULT_BACKGROUND),
    ("framed", 51, 54),
    ("overlined", 53, 55),
    ("gray", 90, DEFAULT_FOREGROUND),
    ("redBright", 91, DEFAULT_FOREGROUND),
    ("greenBright", 92, DEFAULT_FOREGROUND),
    ("yellowBright", 93, DEFAULT_FOREGROUND),
    ("blueBright", 94, DEFAULT_FOREGROUND),
    ("magentaBright", 95, DEFAULT_FOREGROUND),
    ("cyanBright", 96, DEFAULT_FOREGROUND),
    ("whiteBright", 97, DEFAULT_FOREGROUND),
    ("bgGray", 100, DEFAULT_BACKGROUND),
    ("bgRedBright", 101, DEFAULT_BACKGROUND),
    ("bgGreenBright", 102, DEFAULT_BACKGROUND),
    ("bgYellowBright", 103, DEFAULT_BACKGROUND),
    ("bgBlueBright", 104, DEFAULT_BACKGROUND),
    ("bgMagentaBright", 105, DEFAULT_BACKGROUND),
    ("bgCyanBright", 106, DEFAULT_BACKGROUND),
    ("bgWhiteBright", 107, DEFAULT_BACKGROUND),
];
/// `defineColorAlias(target, alias)` calls, as (alias, target).
pub(crate) const COLOR_ALIASES: &[(&str, &str)] = &[
    ("grey", "gray"),
    ("blackBright", "gray"),
    ("bgGrey", "bgGray"),
    ("bgBlackBright", "bgGray"),
    ("faint", "dim"),
    ("crossedout", "strikethrough"),
    ("strikeThrough", "strikethrough"),
    ("crossedOut", "strikethrough"),
    ("conceal", "hidden"),
    ("swapColors", "inverse"),
    ("swapcolors", "inverse"),
    ("doubleUnderline", "doubleunderline"),
];
/// `inspect.styles`.
pub(crate) const STYLES: &[(&str, &str)] = &[
    ("special", "cyan"),
    ("number", "yellow"),
    ("bigint", "yellow"),
    ("boolean", "yellow"),
    ("undefined", "grey"),
    ("null", "bold"),
    ("string", "green"),
    ("symbol", "green"),
    ("date", "magenta"),
    ("regexp", "red"),
    ("module", "underline"),
];
/// `builtInObjects`: global constructors whose prototype `toString` counts as built in.
const BUILT_IN_OBJECTS: &[&str] = &[
    "Object", "Function", "Array", "Number", "Boolean", "String", "Symbol", "Date", "Promise", "RegExp", "Error",
    "AggregateError", "EvalError", "RangeError", "ReferenceError", "SyntaxError", "TypeError", "URIError",
    "ArrayBuffer", "Uint8Array", "Int8Array", "Uint16Array", "Int16Array", "Uint32Array", "Int32Array",
    "Float32Array", "Float64Array", "Uint8ClampedArray", "BigUint64Array", "BigInt64Array", "DataView", "Map",
    "BigInt", "Set", "WeakMap", "WeakSet", "Proxy", "Reflect", "SharedArrayBuffer", "WeakRef",
    "FinalizationRegistry", "Float16Array", "Iterator",
];

/// The original functions and symbols the formatter reads, captured once (`primordials`).
pub(crate) const PRIMORDIALS: &[(&str, &[&str])] = &[
    ("functionToString", &["Function", "prototype", "toString"]),
    ("regexpToString", &["RegExp", "prototype", "toString"]),
    ("dateToISOString", &["Date", "prototype", "toISOString"]),
    ("dateToString", &["Date", "prototype", "toString"]),
    ("symbolToString", &["Symbol", "prototype", "toString"]),
    ("errorToString", &["Error", "prototype", "toString"]),
    ("objectToString", &["Object", "prototype", "toString"]),
    ("toStringTag", &["Symbol", "toStringTag"]),
    ("iterator", &["Symbol", "iterator"]),
    ("toPrimitive", &["Symbol", "toPrimitive"]),
    ("symbolFor", &["Symbol", "for"]),
    ("Error", &["Error"]),
    ("jsonStringify", &["JSON", "stringify"]),
    ("parseInt", &["Number", "parseInt"]),
    ("parseFloat", &["Number", "parseFloat"]),
    ("Number", &["Number"]),
    ("String", &["String"]),
    ("Uint8Array", &["Uint8Array"]),
    ("setHas", &["Set", "prototype", "has"]),
    ("mapHas", &["Map", "prototype", "has"]),
    ("mapGet", &["Map", "prototype", "get"]),
];

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

fn type_of(c: &Context<'_>, value: RootId) -> ValueType {
    c.value_type_rooted(value).unwrap_or(ValueType::Undefined)
}

fn number_of(c: &Context<'_>, value: RootId) -> Option<f64> {
    c.rooted_value(value).and_then(|value| value.as_number())
}

fn is_nullish(c: &Context<'_>, value: RootId) -> bool {
    matches!(type_of(c, value), ValueType::Undefined | ValueType::Null)
}

/// The formatter's captured originals and the inspect tables (`UtilIntrinsics::primordials`).
pub(crate) fn primordial(c: &mut Context<'_>, name: &str) -> Result<RootId, RootedError> {
    let primordials = super::util_shared_vm::primordials(c)?;
    get(c, primordials, name)
}

fn call_primordial(c: &mut Context<'_>, name: &str, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let function = primordial(c, name)?;
    c.call_rooted(function, this, args)
}

/// The UTF-16 length JavaScript reports for `text`.
pub(crate) fn js_length(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// `strEscape`'s quoting: pick a delimiter, escape controls and lone surrogates, keep pairs.
pub(crate) fn quote_units(units: &[u16]) -> Vec<u16> {
    let contains = |unit: u8| units.contains(&u16::from(unit));
    let delimiter = if !contains(b'\'') {
        b'\''
    } else if !contains(b'"') {
        b'"'
    } else if !contains(b'`') && !units.windows(2).any(|pair| pair == [u16::from(b'$'), u16::from(b'{')]) {
        b'`'
    } else {
        b'\''
    };
    let mut output = vec![u16::from(delimiter)];
    output.extend(escape_units(units, delimiter == b'\''));
    output.push(u16::from(delimiter));
    output
}

/// `strEscapeSequencesReplacer`'s `meta` table: controls, backslash, optionally the single
/// quote, and lone surrogates become escapes.
pub(crate) fn escape_units(units: &[u16], escape_single_quote: bool) -> Vec<u16> {
    const BACKSPACE: u16 = 8;
    const TAB: u16 = 9;
    const LINE_FEED: u16 = 10;
    const FORM_FEED: u16 = 12;
    const CARRIAGE_RETURN: u16 = 13;
    const C0_END: u16 = 31;
    const DELETE: u16 = 127;
    const C1_END: u16 = 159;
    let mut output = Vec::with_capacity(units.len());
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        let escape = match unit {
            BACKSPACE => Some("\\b".to_owned()),
            TAB => Some("\\t".to_owned()),
            LINE_FEED => Some("\\n".to_owned()),
            FORM_FEED => Some("\\f".to_owned()),
            CARRIAGE_RETURN => Some("\\r".to_owned()),
            _ if unit == u16::from(b'\\') => Some("\\\\".to_owned()),
            _ if unit == u16::from(b'\'') && escape_single_quote => Some("\\'".to_owned()),
            0..=C0_END | DELETE..=C1_END => Some(format!("\\x{unit:02X}")),
            0xd800..=0xdbff if units.get(index + 1).is_some_and(|next| (0xdc00..=0xdfff).contains(next)) => {
                output.extend_from_slice(&units[index..index + 2]);
                index += 2;
                continue;
            }
            0xd800..=0xdfff => Some(format!("\\u{unit:04x}")),
            _ => None,
        };
        match escape {
            Some(escape) => output.extend(escape.encode_utf16()),
            None => output.push(unit),
        }
        index += 1;
    }
    output
}

fn quote(text: &[u16]) -> String {
    String::from_utf16_lossy(&quote_units(text))
}

/// `removeColors`: the text without its SGR sequences.
fn remove_colors(text: &str) -> String {
    super::util_strip::strip_vt_control_characters(text)
}

/// `compact`: `true` writes every container on one line where it fits; a number `n` lets the
/// `n` innermost levels share a line; `false` (`Lines(0)`) never does.
#[derive(Clone, Copy, PartialEq)]
enum Compact {
    Single,
    Lines(f64),
    Never,
}

#[derive(Clone, Copy)]
enum Sorted {
    No,
    Yes,
    By(RootId),
}

#[derive(Clone, Copy, PartialEq)]
enum Getters {
    No,
    All,
    Get,
    Set,
}

#[derive(Clone, Copy)]
enum Stylize {
    Plain,
    Colors,
    User(RootId),
}

/// The inspect options of one call (`ctx` minus its traversal state).
#[derive(Clone, Copy)]
pub(crate) struct Options {
    show_hidden: bool,
    /// `None` for `depth: null`, which never stops.
    depth: Option<f64>,
    colors: bool,
    custom_inspect: bool,
    show_proxy: bool,
    max_array_length: f64,
    max_string_length: f64,
    break_length: f64,
    compact: Compact,
    sorted: Sorted,
    getters: Getters,
    numeric_separator: bool,
    stylize: Stylize,
    /// The caller's options object when it names keys beyond Node's (`ctx.userOptions`).
    user_options: Option<RootId>,
}

impl Options {
    /// The options `inspectDefaultOptions` holds now.
    pub(crate) fn defaults(c: &mut Context<'_>) -> Result<Self, RootedError> {
        let mut options = Self {
            show_hidden: false,
            depth: Some(2.0),
            colors: false,
            custom_inspect: true,
            show_proxy: false,
            max_array_length: 100.0,
            max_string_length: 10_000.0,
            break_length: 80.0,
            compact: Compact::Lines(3.0),
            sorted: Sorted::No,
            getters: Getters::No,
            numeric_separator: false,
            stylize: Stylize::Plain,
            user_options: None,
        };
        let defaults = primordial(c, "defaultOptions")?;
        for (name, _) in DEFAULT_OPTIONS {
            let value = get(c, defaults, name)?;
            options.set(c, name, value)?;
        }
        Ok(options)
    }

    /// `ctx[key] = opts[key]`, read as the formatter later reads the key.
    fn set(&mut self, c: &mut Context<'_>, name: &str, value: RootId) -> Result<(), RootedError> {
        let number = number_of(c, value);
        let truthy = c.truthy_rooted(value)?;
        let limit = |number: Option<f64>, nullish: bool| if nullish { f64::INFINITY } else { number.unwrap_or(f64::NAN) };
        match name {
            "showHidden" => self.show_hidden = truthy,
            "depth" => {
                self.depth = match type_of(c, value) {
                    ValueType::Null => None,
                    ValueType::Undefined => Some(f64::INFINITY),
                    _ => Some(number.unwrap_or(f64::NAN)),
                }
            }
            "colors" => {
                self.colors = truthy;
                if !matches!(self.stylize, Stylize::User(_)) {
                    self.stylize = if truthy { Stylize::Colors } else { Stylize::Plain };
                }
            }
            "customInspect" => self.custom_inspect = truthy,
            "showProxy" => self.show_proxy = truthy,
            "maxArrayLength" => self.max_array_length = limit(number, type_of(c, value) == ValueType::Null),
            "maxStringLength" => self.max_string_length = limit(number, type_of(c, value) == ValueType::Null),
            "breakLength" => self.break_length = number.unwrap_or(f64::NAN),
            "compact" => {
                self.compact = match (c.rooted_value(value).and_then(|value| value.as_bool()), number) {
                    (Some(true), _) => Compact::Single,
                    (_, Some(levels)) => Compact::Lines(levels),
                    _ => Compact::Never,
                }
            }
            "sorted" => {
                self.sorted = if c.is_callable_rooted(value)? {
                    Sorted::By(value)
                } else if truthy {
                    Sorted::Yes
                } else {
                    Sorted::No
                }
            }
            "getters" => {
                self.getters = match c.string_text(value)?.as_deref() {
                    Some("get") => Getters::Get,
                    Some("set") => Getters::Set,
                    _ if c.rooted_value(value).and_then(|value| value.as_bool()) == Some(true) => Getters::All,
                    _ => Getters::No,
                }
            }
            "numericSeparator" => self.numeric_separator = truthy,
            "stylize" if c.is_callable_rooted(value)? => self.stylize = Stylize::User(value),
            _ => {}
        }
        Ok(())
    }

    /// Applies an options object as `inspect(value, opts)` does.
    fn apply(&mut self, c: &mut Context<'_>, options: RootId) -> Result<(), RootedError> {
        if let Some(show_hidden) = c.rooted_value(options).and_then(|value| value.as_bool()) {
            self.show_hidden = show_hidden;
            return Ok(());
        }
        if !c.truthy_rooted(options)? || !c.is_object_rooted(options)? {
            return Ok(());
        }
        for key in c.enumerable_own_keys_rooted(options, false, true)? {
            let Some(name) = c.string_text(key)? else { continue };
            if name == "stylize" || DEFAULT_OPTIONS.iter().any(|(default, _)| *default == name) {
                let value = c.get_property_rooted(options, key)?;
                self.set(c, &name, value)?;
            } else if self.user_options.is_none() {
                self.user_options = Some(options);
            }
        }
        Ok(())
    }

    /// The options `inspect(value, opts)` formats with: the defaults, then `opts`.
    pub(crate) fn from_arguments(c: &mut Context<'_>, args: &[RootId]) -> Result<Self, RootedError> {
        let mut options = Self::defaults(c)?;
        if args.len() > 1 {
            let depth = args.get(2).copied().filter(|depth| type_of(c, *depth) != ValueType::Undefined);
            if let Some(depth) = depth {
                options.set(c, "depth", depth)?;
            }
            let colors = args.get(3).copied().filter(|colors| type_of(c, *colors) != ValueType::Undefined);
            if let Some(colors) = colors {
                options.set(c, "colors", colors)?;
            }
            options.apply(c, args[1])?;
        }
        Ok(options)
    }

    /// `getUserOptions`: what a custom inspector receives as its options.
    fn user_object(&self, c: &mut Context<'_>) -> Result<RootId, RootedError> {
        let object = c.object_rooted()?;
        let stylize = match self.stylize {
            Stylize::User(function) => function,
            Stylize::Plain | Stylize::Colors => {
                let colors = c.boolean(self.colors);
                c.host_function_named_with_data(crate::host::shared_vm::operation!("utilInspectStylize"), "stylize", colors)?
            }
        };
        c.assign_named_property_rooted(object, "stylize", stylize)?;
        let bool = |c: &mut Context<'_>, value: bool| c.boolean(value);
        let entries: [(&str, RootId); 12] = [
            ("showHidden", bool(c, self.show_hidden)),
            ("depth", match self.depth { Some(depth) => c.number(depth), None => c.null() }),
            ("colors", bool(c, self.colors)),
            ("customInspect", bool(c, self.custom_inspect)),
            ("showProxy", bool(c, self.show_proxy)),
            ("maxArrayLength", c.number(self.max_array_length)),
            ("maxStringLength", c.number(self.max_string_length)),
            ("breakLength", c.number(self.break_length)),
            (
                "compact",
                match self.compact {
                    Compact::Single => bool(c, true),
                    Compact::Lines(levels) => c.number(levels),
                    Compact::Never => bool(c, false),
                },
            ),
            ("sorted", match self.sorted { Sorted::By(function) => function, Sorted::Yes => bool(c, true), Sorted::No => bool(c, false) }),
            (
                "getters",
                match self.getters {
                    Getters::No => bool(c, false),
                    Getters::All => bool(c, true),
                    Getters::Get => c.string_rooted("get"),
                    Getters::Set => c.string_rooted("set"),
                },
            ),
            ("numericSeparator", bool(c, self.numeric_separator)),
        ];
        for (name, value) in entries {
            c.assign_named_property_rooted(object, name, value)?;
        }
        if let Some(user) = self.user_options {
            for key in c.enumerable_own_keys_rooted(user, true, true)? {
                let value = c.get_property_rooted(user, key)?;
                c.set_property_rooted(object, key, value, object)?;
            }
        }
        Ok(object)
    }

    fn depth_exceeded(&self, recurse_times: usize) -> bool {
        self.depth.is_some_and(|depth| recurse_times as f64 > depth)
    }
}

/// What a formatted entry is (`kObjectType`, `kArrayType`, `kArrayExtrasType`): an object's
/// property, an array element, which carries no key, or an array's other property. A
/// container's extras type is `Object` or `Array`.
#[derive(Clone, Copy, PartialEq)]
enum Extras {
    Object,
    Element,
    Array,
}

/// The traversal state of one `inspect` call (`ctx`).
struct Inspector {
    options: Options,
    seen: Vec<RootId>,
    circular: Vec<RootId>,
    indentation: usize,
    current_depth: usize,
    budget: std::collections::HashMap<usize, usize>,
}

/// A container's formatter: what `formatRaw` writes before the object's own keys.
enum Formatter {
    None,
    Array,
    TypedArray { length: usize },
    Set(Vec<RootId>),
    Map(Vec<(RootId, RootId)>),
    Iterator,
    ArrayBuffer,
    Promise,
    WeakCollection { map: bool },
}

/// `inspect(value, options)` with prepared options.
pub(crate) fn inspect_value(c: &mut Context<'_>, value: RootId, options: Options) -> Result<String, RootedError> {
    let mut inspector = Inspector {
        options,
        seen: Vec::new(),
        circular: Vec::new(),
        indentation: 0,
        current_depth: 0,
        budget: std::collections::HashMap::new(),
    };
    inspector.format_value(c, value, 0, false)
}

impl Inspector {
    fn stylize(&self, c: &mut Context<'_>, text: &str, style: &str) -> Result<String, RootedError> {
        match self.options.stylize {
            Stylize::Plain => Ok(text.to_owned()),
            Stylize::Colors => stylize_with_color(c, text, style),
            Stylize::User(function) => {
                let text = c.string_rooted(text);
                let style = c.string_rooted(style);
                let undefined = c.undefined();
                let result = c.call_rooted(function, undefined, &[text, style])?;
                c.to_string(result)
            }
        }
    }

    fn visible_length(&self, text: &str) -> usize {
        if self.options.colors {
            js_length(&remove_colors(text))
        } else {
            js_length(text)
        }
    }

    fn format_value(&mut self, c: &mut Context<'_>, value: RootId, recurse_times: usize, typed_array: bool) -> Result<String, RootedError> {
        match type_of(c, value) {
            ValueType::Object | ValueType::Function => {}
            ValueType::Null => return self.stylize(c, "null", "null"),
            _ => return self.format_primitive(c, value),
        }
        let context = value;
        let mut value = value;
        let proxy = c.proxy_details_rooted(value)?;
        if let Some((target, handler)) = proxy {
            if type_of(c, target) == ValueType::Null {
                return self.stylize(c, "<Revoked Proxy>", "special");
            }
            if self.options.show_proxy {
                return self.format_proxy(c, target, handler, recurse_times);
            }
            value = target;
            while let Some((target, _)) = c.proxy_details_rooted(value)? {
                if type_of(c, target) == ValueType::Null {
                    return self.stylize(c, "<Revoked Proxy>", "special");
                }
                value = target;
            }
        }
        if self.options.custom_inspect {
            let custom_key = primordial(c, "custom")?;
            let custom = c.get_property_rooted(value, custom_key)?;
            let inspect = primordial(c, "inspect")?;
            if c.is_callable_rooted(custom)? && !c.strict_equal_rooted(custom, inspect)? && !is_prototype_object(c, value)? {
                let depth = match self.options.depth {
                    Some(depth) => c.number(depth - recurse_times as f64),
                    None => c.null(),
                };
                let options = self.options.user_object(c)?;
                let result = c.call_rooted(custom, context, &[depth, options, inspect])?;
                if !c.strict_equal_rooted(result, context)? {
                    let Some(text) = c.string_text(result)? else {
                        return self.format_value(c, result, recurse_times, false);
                    };
                    return Ok(text.replace('\n', &format!("\n{}", " ".repeat(self.indentation))));
                }
            }
        }
        for seen in self.seen.clone() {
            if c.strict_equal_rooted(seen, value)? {
                let index = match self.circular_index(c, value)? {
                    Some(index) => index,
                    None => {
                        self.circular.push(value);
                        self.circular.len()
                    }
                };
                return self.stylize(c, &format!("[Circular *{index}]"), "special");
            }
        }
        self.format_raw(c, value, recurse_times, typed_array)
    }

    fn circular_index(&self, c: &mut Context<'_>, value: RootId) -> Result<Option<usize>, RootedError> {
        for (index, entry) in self.circular.iter().enumerate() {
            if c.strict_equal_rooted(*entry, value)? {
                return Ok(Some(index + 1));
            }
        }
        Ok(None)
    }

    fn format_proxy(&mut self, c: &mut Context<'_>, target: RootId, handler: RootId, recurse_times: usize) -> Result<String, RootedError> {
        if self.options.depth_exceeded(recurse_times) {
            return self.stylize(c, "Proxy [Array]", "special");
        }
        let recurse_times = recurse_times + 1;
        self.indentation += ENTRY_INDENTATION;
        let output = vec![
            self.format_value(c, target, recurse_times, false)?,
            self.format_value(c, handler, recurse_times, false)?,
        ];
        self.indentation -= ENTRY_INDENTATION;
        let braces = ("Proxy [".to_owned(), "]".to_owned());
        Ok(self.reduce_to_single_string(output, "", &braces, Extras::Array, recurse_times, None))
    }


    fn format_primitive(&mut self, c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
        if type_of(c, value) != ValueType::String {
            let (text, style) = primitive_text(c, value, self.options.numeric_separator)?;
            return self.stylize(c, &text, style);
        }
        let mut units = c.string_code_units(value)?.unwrap_or_default();
        let mut trailer = String::new();
        if units.len() as f64 > self.options.max_string_length {
            let keep = self.options.max_string_length.max(0.0) as usize;
            let remaining = units.len() - keep;
            units.truncate(keep);
            trailer = format!("... {remaining} more character{}", plural(remaining));
        }
        if self.options.compact != Compact::Single
            && units.len() > MIN_LINE_WIDTH
            && units.len() as f64 > self.options.break_length - (self.indentation + STRING_LINE_RESERVE) as f64
        {
            let mut lines = Vec::new();
            for line in units.split_inclusive(|unit| *unit == u16::from(b'\n')) {
                lines.push(self.stylize(c, &quote(line), "string")?);
            }
            let separator = format!(" +\n{}", " ".repeat(self.indentation + ENTRY_INDENTATION));
            return Ok(lines.join(&separator) + &trailer);
        }
        Ok(self.stylize(c, &quote(&units), "string")? + &trailer)
    }

    fn format_raw(&mut self, c: &mut Context<'_>, value: RootId, recurse_times: usize, typed_array: bool) -> Result<String, RootedError> {
        let mut proto_props = (self.options.show_hidden && !self.options.depth_exceeded(recurse_times)).then(Vec::new);
        let constructor = self.constructor_name(c, value, recurse_times, &mut proto_props)?;
        let proto_props = proto_props.filter(|props| !props.is_empty());
        let bare = |keys: &Vec<RootId>| keys.is_empty() && proto_props.is_none();
        let tag_key = primordial(c, "toStringTag")?;
        let tag_value = c.get_property_rooted(value, tag_key)?;
        let mut tag = c.string_text(tag_value)?.unwrap_or_default();
        if !tag.is_empty() {
            let shown_twice = match c.own_property_fields_rooted(value, tag_key)? {
                Some(fields) => self.options.show_hidden || fields.enumerable == Some(true),
                None => false,
            };
            if shown_twice {
                tag.clear();
            }
        }
        let constructor = constructor.as_deref();
        let mut base = String::new();
        let mut formatter = Formatter::None;
        let mut braces = ("{".to_owned(), "}".to_owned());
        let mut extras = Extras::Object;
        let mut keys = Vec::new();
        let iterator_key = primordial(c, "iterator")?;
        let mut no_iterator = true;
        if constructor.is_none() || c.has_property_rooted(value, iterator_key)? {
            no_iterator = false;
            if c.is_array_rooted(value)? {
                let length = array_length(c, value)?;
                let prefix = if constructor != Some("Array") || !tag.is_empty() {
                    prefix(constructor, &tag, "Array", &format!("({length})"))
                } else {
                    String::new()
                };
                keys = c.own_non_index_keys_rooted(value, !self.options.show_hidden)?;
                braces = (format!("{prefix}["), "]".to_owned());
                if length == 0 && bare(&keys) {
                    return Ok(format!("{}]", braces.0));
                }
                extras = Extras::Array;
                formatter = Formatter::Array;
            } else if let Some(BuiltinData::Set(entries)) = brand_data(c, value, BuiltinBrand::Set)? {
                let prefix = prefix(constructor, &tag, "Set", &format!("({})", entries.len()));
                keys = self.keys(c, value)?;
                if entries.is_empty() && bare(&keys) {
                    return Ok(format!("{prefix}{{}}"));
                }
                braces = (format!("{prefix}{{"), "}".to_owned());
                formatter = Formatter::Set(entries);
            } else if let Some(BuiltinData::Map(entries)) = brand_data(c, value, BuiltinBrand::Map)? {
                let prefix = prefix(constructor, &tag, "Map", &format!("({})", entries.len()));
                keys = self.keys(c, value)?;
                if entries.is_empty() && bare(&keys) {
                    return Ok(format!("{prefix}{{}}"));
                }
                braces = (format!("{prefix}{{"), "}".to_owned());
                formatter = Formatter::Map(entries);
            } else if c.has_builtin_brand_rooted(value, BuiltinBrand::TypedArray)? {
                keys = c.own_non_index_keys_rooted(value, !self.options.show_hidden)?;
                let length = c.buffer_storage_rooted(value)?.map(|storage| storage.length).unwrap_or_default();
                let fallback = if constructor.is_none() { typed_array_name(c, value)? } else { "" };
                let prefix = prefix(constructor, &tag, fallback, &format!("({length})"));
                braces = (format!("{prefix}["), "]".to_owned());
                if length == 0 && keys.is_empty() && !self.options.show_hidden {
                    return Ok(format!("{}]", braces.0));
                }
                formatter = Formatter::TypedArray { length };
                extras = Extras::Array;
            } else if c.has_builtin_brand_rooted(value, BuiltinBrand::MapIterator)? {
                keys = self.keys(c, value)?;
                braces = iterator_braces("Map", &tag);
                formatter = Formatter::Iterator;
            } else if c.has_builtin_brand_rooted(value, BuiltinBrand::SetIterator)? {
                keys = self.keys(c, value)?;
                braces = iterator_braces("Set", &tag);
                formatter = Formatter::Iterator;
            } else {
                no_iterator = true;
            }
        }
        if no_iterator {
            keys = self.keys(c, value)?;
            if type_of(c, value) == ValueType::Function {
                base = function_base(c, value, constructor, &tag)?;
                if bare(&keys) {
                    return self.stylize(c, &base, "special");
                }
            } else if constructor == Some("Object") {
                if c.has_builtin_brand_rooted(value, BuiltinBrand::ArgumentsObject)? {
                    "[Arguments] {".clone_into(&mut braces.0);
                } else if !tag.is_empty() {
                    braces.0 = format!("{}{{", prefix(constructor, &tag, "Object", ""));
                }
                if bare(&keys) {
                    return Ok(format!("{}}}", braces.0));
                }
            } else if c.has_builtin_brand_rooted(value, BuiltinBrand::RegExp)? {
                let text = call_primordial(c, "regexpToString", value, &[])?;
                base = c.to_string(text)?;
                let prefix = prefix(constructor, &tag, "RegExp", "");
                if prefix != "RegExp " {
                    base = format!("{prefix}{base}");
                }
                if bare(&keys) || self.options.depth_exceeded(recurse_times) {
                    return self.stylize(c, &base, "regexp");
                }
            } else if let Some(BuiltinData::Primitive { brand: BuiltinBrand::Date, value: time }) = brand_data(c, value, BuiltinBrand::Date)? {
                let valid = number_of(c, time).is_some_and(|time| !time.is_nan());
                let text = call_primordial(c, if valid { "dateToISOString" } else { "dateToString" }, value, &[])?;
                base = c.to_string(text)?;
                let prefix = prefix(constructor, &tag, "Date", "");
                if prefix != "Date " {
                    base = format!("{prefix}{base}");
                }
                if bare(&keys) {
                    return self.stylize(c, &base, "date");
                }
            } else if is_error(c, value)? {
                base = self.format_error(c, value, constructor, &tag, &mut keys)?;
                if bare(&keys) {
                    return Ok(base);
                }
            } else if c.has_builtin_brand_rooted(value, BuiltinBrand::AnyArrayBuffer)? {
                let shared = c.has_builtin_brand_rooted(value, BuiltinBrand::SharedArrayBuffer)?;
                let prefix = prefix(constructor, &tag, if shared { "SharedArrayBuffer" } else { "ArrayBuffer" }, "");
                if !typed_array {
                    formatter = Formatter::ArrayBuffer;
                } else if bare(&keys) {
                    let length = get(c, value, "byteLength")?;
                    let (length, _) = primitive_text(c, length, false)?;
                    let length = self.stylize(c, &length, "number")?;
                    return Ok(format!("{prefix}{{ byteLength: {length} }}"));
                }
                braces.0 = format!("{prefix}{{");
                let key = c.string_rooted("byteLength");
                keys.insert(0, key);
            } else if c.has_builtin_brand_rooted(value, BuiltinBrand::DataView)? {
                braces.0 = format!("{}{{", prefix(constructor, &tag, "DataView", ""));
                let mut front = ["byteLength", "byteOffset", "buffer"].map(|name| c.string_rooted(name)).to_vec();
                front.append(&mut keys);
                keys = front;
            } else if c.has_builtin_brand_rooted(value, BuiltinBrand::Promise)? {
                braces.0 = format!("{}{{", prefix(constructor, &tag, "Promise", ""));
                formatter = Formatter::Promise;
            } else if c.has_builtin_brand_rooted(value, BuiltinBrand::WeakSet)? {
                braces.0 = format!("{}{{", prefix(constructor, &tag, "WeakSet", ""));
                formatter = Formatter::WeakCollection { map: false };
            } else if c.has_builtin_brand_rooted(value, BuiltinBrand::WeakMap)? {
                braces.0 = format!("{}{{", prefix(constructor, &tag, "WeakMap", ""));
                formatter = Formatter::WeakCollection { map: true };
            } else if c.has_builtin_brand_rooted(value, BuiltinBrand::ModuleNamespaceObject)? {
                braces.0 = format!("{}{{", prefix(constructor, &tag, "Module", ""));
            } else if c.has_builtin_brand_rooted(value, BuiltinBrand::BoxedPrimitive)? {
                base = self.boxed_base(c, value, &mut keys, constructor, &tag)?;
                if bare(&keys) {
                    return Ok(base);
                }
            } else {
                if bare(&keys) {
                    if let Some(address) = c.external_address_rooted(value) {
                        return self.stylize(c, &format!("[External: {address:x}]"), "special");
                    }
                    return Ok(format!("{}{{}}", prefix(constructor, &tag, "Object", "")));
                }
                braces.0 = format!("{}{{", prefix(constructor, &tag, "Object", ""));
            }
        }
        if self.options.depth_exceeded(recurse_times) {
            let name = prefix(constructor, &tag, "Object", "");
            let name = &name[..name.len() - 1];
            let name = if constructor.is_some() { format!("[{name}]") } else { name.to_owned() };
            return self.stylize(c, &name, "special");
        }
        let recurse_times = recurse_times + 1;
        self.seen.push(value);
        self.current_depth = recurse_times;
        let mut output = self.format_entries(c, value, recurse_times, &formatter, &mut braces)?;
        for &key in &keys {
            output.push(self.format_property(c, value, recurse_times, key, extras)?);
        }
        output.extend(proto_props.into_iter().flatten());
        if let Some(index) = self.circular_index(c, value)? {
            let reference = self.stylize(c, &format!("<ref *{index}>"), "special")?;
            if self.options.compact != Compact::Single {
                base = if base.is_empty() { reference } else { format!("{reference} {base}") };
            } else {
                braces.0 = format!("{reference} {}", braces.0);
            }
        }
        self.seen.pop();
        if !matches!(self.options.sorted, Sorted::No) {
            if extras == Extras::Object {
                sort_output(c, &mut output, self.options.sorted)?;
            } else if keys.len() > 1 {
                let start = output.len() - keys.len();
                let mut tail = output.split_off(start);
                sort_output(c, &mut tail, self.options.sorted)?;
                output.append(&mut tail);
            }
        }
        let numeric = match formatter {
            Formatter::Array | Formatter::TypedArray { .. } => Some(all_numeric(c, value, output.len())?),
            _ => None,
        };
        let result = self.reduce_to_single_string(output, &base, &braces, extras, recurse_times, numeric);
        let budget = self.budget.entry(self.indentation).or_default();
        *budget += js_length(&result);
        if *budget > OUTPUT_BUDGET {
            self.options.depth = Some(-1.0);
        }
        Ok(result)
    }

    /// `getConstructorName`: the first constructor on the chain the value is an instance of,
    /// `None` for a null prototype, or the internal name with its prototype's.
    /// With `showHidden`, `proto_props` collects the prototype properties
    /// `addPrototypeProperties` writes.
    fn constructor_name(
        &mut self, c: &mut Context<'_>, value: RootId, recurse_times: usize, proto_props: &mut Option<Vec<String>>,
    ) -> Result<Option<String>, RootedError> {
        let mut object = value;
        let mut first_prototype = None;
        let constructor_key = c.string_rooted("constructor");
        loop {
            if let Some(constructor) = c.own_property_fields_rooted(object, constructor_key)?.and_then(|fields| fields.value) {
                if type_of(c, constructor) == ValueType::Function {
                    let name = get(c, constructor, "name")?;
                    let name = c.to_string(name)?;
                    if !name.is_empty() && c.instance_of_rooted(value, constructor).unwrap_or(false) {
                        let first_is_object = match first_prototype {
                            Some(first) => c.strict_equal_rooted(first, object)?,
                            None => false,
                        };
                        if let Some(output) = proto_props.as_mut() {
                            if !first_is_object || !BUILT_IN_OBJECTS.contains(&name.as_str()) {
                                let start = first_prototype.unwrap_or(value);
                                self.add_prototype_properties(c, value, start, recurse_times, output)?;
                            }
                        }
                        return Ok(Some(name));
                    }
                }
            }
            object = c.prototype_rooted(object)?;
            if first_prototype.is_none() {
                first_prototype = Some(object);
            }
            if type_of(c, object) == ValueType::Null {
                break;
            }
        }
        let first_prototype = first_prototype.expect("the loop reads at least one prototype");
        if type_of(c, first_prototype) == ValueType::Null {
            return Ok(None);
        }
        let internal = match c.constructor_name_rooted(value)? {
            Some(name) => c.to_string(name)?,
            None => "Object".to_owned(),
        };
        if self.options.depth_exceeded(recurse_times) {
            return Ok(Some(format!("{internal} <Complex prototype>")));
        }
        match self.constructor_name(c, first_prototype, recurse_times + 1, proto_props)? {
            Some(prototype) => Ok(Some(format!("{internal} <{prototype}>"))),
            None => {
                let mut options = self.options;
                options.custom_inspect = false;
                options.depth = Some(-1.0);
                let prototype = inspect_value(c, first_prototype, options)?;
                Ok(Some(format!("{internal} <{prototype}>")))
            }
        }
    }

    /// `addPrototypeProperties`: the non-method properties of up to three prototypes above
    /// `main`, before a built-in one, that `main` does not shadow.
    fn add_prototype_properties(
        &mut self, c: &mut Context<'_>, main: RootId, start: RootId, recurse_times: usize, output: &mut Vec<String>,
    ) -> Result<(), RootedError> {
        const PROTOTYPE_LAYERS: usize = 3;
        let constructor_key = c.string_rooted("constructor");
        let mut object = start;
        let mut seen_keys: Vec<RootId> = Vec::new();
        let mut keys: Vec<RootId> = Vec::new();
        for depth in 0..PROTOTYPE_LAYERS {
            if depth != 0 || c.strict_equal_rooted(main, object)? {
                object = c.prototype_rooted(object)?;
                if type_of(c, object) == ValueType::Null {
                    return Ok(());
                }
                if let Some(constructor) = c.own_property_fields_rooted(object, constructor_key)?.and_then(|fields| fields.value) {
                    if type_of(c, constructor) == ValueType::Function {
                        let name = get(c, constructor, "name")?;
                        if BUILT_IN_OBJECTS.contains(&c.to_string(name)?.as_str()) {
                            return Ok(());
                        }
                    }
                }
            }
            if depth != 0 {
                seen_keys.append(&mut keys);
            }
            keys = c.own_keys_rooted(object)?;
            self.seen.push(main);
            for &key in &keys {
                if c.string_text(key)?.as_deref() == Some("constructor") || c.own_property_fields_rooted(main, key)?.is_some() {
                    continue;
                }
                let mut shadowed = false;
                if depth != 0 {
                    for &seen in &seen_keys {
                        if c.strict_equal_rooted(seen, key)? {
                            shadowed = true;
                            break;
                        }
                    }
                }
                if shadowed {
                    continue;
                }
                let Some(fields) = c.own_property_fields_rooted(object, key)? else { continue };
                if fields.value.is_some_and(|value| type_of(c, value) == ValueType::Function) {
                    continue;
                }
                let text = self.format_described(c, recurse_times, key, Extras::Object, fields, main)?;
                output.push(if self.options.colors { format!("\u{1b}[2m{text}\u{1b}[22m") } else { text });
            }
            self.seen.pop();
        }
        Ok(())
    }

    /// `getKeys`: own enumerable keys and symbols, or every own key with `showHidden`.
    fn keys(&self, c: &mut Context<'_>, value: RootId) -> Result<Vec<RootId>, RootedError> {
        if self.options.show_hidden {
            c.own_keys_rooted(value)
        } else {
            c.enumerable_own_keys_rooted(value, true, true)
        }
    }

    fn format_entries(
        &mut self, c: &mut Context<'_>, value: RootId, recurse_times: usize, formatter: &Formatter, braces: &mut (String, String),
    ) -> Result<Vec<String>, RootedError> {
        match formatter {
            Formatter::None => Ok(Vec::new()),
            Formatter::Array => self.format_array(c, value, recurse_times),
            Formatter::TypedArray { length } => self.format_typed_array(c, value, *length, recurse_times),
            Formatter::Set(entries) => self.format_set_entries(c, entries, recurse_times, false),
            Formatter::Map(entries) => {
                let shown = self.shown(entries.len());
                self.indentation += ENTRY_INDENTATION;
                let mut output = Vec::with_capacity(shown + 1);
                for &(key, entry) in &entries[..shown] {
                    let key = self.format_value(c, key, recurse_times, false)?;
                    let entry = self.format_value(c, entry, recurse_times, false)?;
                    output.push(format!("{key} => {entry}"));
                }
                self.indentation -= ENTRY_INDENTATION;
                push_remaining(&mut output, entries.len() - shown);
                Ok(output)
            }
            Formatter::Iterator => {
                let Some(preview) = c.collection_preview_rooted(value)? else { return Ok(Vec::new()) };
                if preview.is_key_value {
                    if let Some(start) = braces.0.strip_suffix(" Iterator] {") {
                        braces.0 = format!("{start} Entries] {{");
                    }
                    self.format_map_entries(c, &preview.entries, recurse_times, false)
                } else {
                    self.format_set_entries(c, &preview.entries, recurse_times, false)
                }
            }
            Formatter::ArrayBuffer => {
                let detached = get(c, value, "detached")?;
                if c.rooted_value(detached).and_then(|value| value.as_bool()) == Some(true) {
                    return Ok(vec![self.stylize(c, "(detached)", "special")?]);
                }
                let bytes = match c.builtin_data_rooted(value)? {
                    Some(BuiltinData::Bytes { bytes, .. }) => bytes,
                    _ => Vec::new(),
                };
                let shown = self.shown(bytes.len());
                let mut text = bytes[..shown].iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join(" ");
                let remaining = bytes.len() - shown;
                if remaining > 0 {
                    text.push_str(&format!(" ... {remaining} more byte{}", plural(remaining)));
                }
                let contents = self.stylize(c, "[Uint8Contents]", "special")?;
                Ok(vec![format!("{contents}: <{text}>")])
            }
            Formatter::Promise => {
                let (result, rejected) = match c.promise_snapshot_rooted(value)? {
                    None | Some(PromiseSnapshot::Pending) => return Ok(vec![self.stylize(c, "<pending>", "special")?]),
                    Some(PromiseSnapshot::Fulfilled(result)) => (result, false),
                    Some(PromiseSnapshot::Rejected(result)) => (result, true),
                };
                self.indentation += ENTRY_INDENTATION;
                let text = self.format_value(c, result, recurse_times, false)?;
                self.indentation -= ENTRY_INDENTATION;
                Ok(vec![if rejected { format!("{} {text}", self.stylize(c, "<rejected>", "special")?) } else { text }])
            }
            Formatter::WeakCollection { map } => {
                if !self.options.show_hidden {
                    return Ok(vec![self.stylize(c, "<items unknown>", "special")?]);
                }
                let entries = c.collection_preview_rooted(value)?.map(|preview| preview.entries).unwrap_or_default();
                if *map {
                    self.format_map_entries(c, &entries, recurse_times, true)
                } else {
                    self.format_set_entries(c, &entries, recurse_times, true)
                }
            }
        }
    }

    fn shown(&self, length: usize) -> usize {
        self.options.max_array_length.max(0.0).min(length as f64) as usize
    }

    /// `formatSet` and `formatSetIterInner`.
    fn format_set_entries(&mut self, c: &mut Context<'_>, entries: &[RootId], recurse_times: usize, weak: bool) -> Result<Vec<String>, RootedError> {
        let shown = self.shown(entries.len());
        self.indentation += ENTRY_INDENTATION;
        let mut output = Vec::with_capacity(shown + 1);
        for &entry in &entries[..shown] {
            output.push(self.format_value(c, entry, recurse_times, false)?);
        }
        self.indentation -= ENTRY_INDENTATION;
        if weak && matches!(self.options.sorted, Sorted::No) {
            output.sort_by(|left, right| left.encode_utf16().cmp(right.encode_utf16()));
        }
        push_remaining(&mut output, entries.len() - shown);
        Ok(output)
    }

    /// `formatMapIterInner`: entries alternate keys and values.
    fn format_map_entries(&mut self, c: &mut Context<'_>, entries: &[RootId], recurse_times: usize, weak: bool) -> Result<Vec<String>, RootedError> {
        let pairs = entries.len() / 2;
        let shown = self.shown(pairs);
        self.indentation += ENTRY_INDENTATION;
        let mut output = Vec::with_capacity(shown + 1);
        for [key, entry] in entries.as_chunks::<2>().0.iter().take(shown) {
            let key = self.format_value(c, *key, recurse_times, false)?;
            let entry = self.format_value(c, *entry, recurse_times, false)?;
            output.push(if weak {
                format!("{key} => {entry}")
            } else {
                let braces = ("[".to_owned(), "]".to_owned());
                self.reduce_to_single_string(vec![key, entry], "", &braces, Extras::Array, recurse_times, None)
            });
        }
        self.indentation -= ENTRY_INDENTATION;
        if weak && matches!(self.options.sorted, Sorted::No) {
            output.sort_by(|left, right| left.encode_utf16().cmp(right.encode_utf16()));
        }
        push_remaining(&mut output, pairs - shown);
        Ok(output)
    }

    /// `formatArray`.
    fn format_array(&mut self, c: &mut Context<'_>, value: RootId, recurse_times: usize) -> Result<Vec<String>, RootedError> {
        let length = array_length(c, value)?;
        let shown = self.shown(length);
        let mut output = Vec::with_capacity(shown + 1);
        for index in 0..shown {
            let key = c.string_rooted(&index.to_string());
            if c.own_property_fields_rooted(value, key)?.is_none() {
                return self.format_sparse_array(c, value, recurse_times, shown, output, index, length);
            }
            output.push(self.format_property(c, value, recurse_times, key, Extras::Element)?);
        }
        push_remaining(&mut output, length - shown);
        Ok(output)
    }

    /// `formatSpecialArray`: holes become `<n empty items>`.
    #[allow(clippy::too_many_arguments)]
    fn format_sparse_array(
        &mut self, c: &mut Context<'_>, value: RootId, recurse_times: usize, max_length: usize, mut output: Vec<String>, start: usize,
        length: usize,
    ) -> Result<Vec<String>, RootedError> {
        let keys = c.enumerable_own_keys_rooted(value, false, true)?;
        let mut index = start;
        let mut position = start;
        while position < keys.len() && output.len() < max_length {
            let key = keys[position];
            let text = c.string_text(key)?.unwrap_or_default();
            let Some(number) = array_index(&text) else { break };
            if number > MAX_ARRAY_INDEX {
                break;
            }
            let number = number as usize;
            if number != index {
                let empty = number - index;
                output.push(self.stylize(c, &format!("<{empty} empty item{}>", plural(empty)), "undefined")?);
                index = number;
                if output.len() == max_length {
                    break;
                }
            }
            output.push(self.format_property(c, value, recurse_times, key, Extras::Element)?);
            index += 1;
            position += 1;
        }
        let remaining = length - index;
        if output.len() != max_length {
            if remaining > 0 {
                output.push(self.stylize(c, &format!("<{remaining} empty item{}>", plural(remaining)), "undefined")?);
            }
        } else {
            push_remaining(&mut output, remaining);
        }
        Ok(output)
    }

    /// `formatTypedArray`.
    fn format_typed_array(&mut self, c: &mut Context<'_>, value: RootId, length: usize, recurse_times: usize) -> Result<Vec<String>, RootedError> {
        let shown = self.shown(length);
        let mut output = Vec::with_capacity(shown + 1);
        for index in 0..shown {
            let element = get(c, value, &index.to_string())?;
            let (text, style) = primitive_text(c, element, self.options.numeric_separator)?;
            output.push(self.stylize(c, &text, style)?);
        }
        push_remaining(&mut output, length - shown);
        if self.options.show_hidden {
            self.indentation += ENTRY_INDENTATION;
            for name in ["BYTES_PER_ELEMENT", "length", "byteLength", "byteOffset", "buffer"] {
                let property = get(c, value, name)?;
                let text = self.format_value(c, property, recurse_times, true)?;
                output.push(format!("[{name}]: {text}"));
            }
            self.indentation -= ENTRY_INDENTATION;
        }
        Ok(output)
    }

    /// `formatProperty`.
    fn format_property(&mut self, c: &mut Context<'_>, value: RootId, recurse_times: usize, key: RootId, extras: Extras) -> Result<String, RootedError> {
        let fields = match c.own_property_fields_rooted(value, key)? {
            Some(fields) => fields,
            None => {
                let value = c.get_property_rooted(value, key)?;
                DescriptorFields { value: Some(value), enumerable: Some(false), ..DescriptorFields::default() }
            }
        };
        self.format_described(c, recurse_times, key, extras, fields, value)
    }

    /// `formatProperty` with the property's descriptor; getters run on `receiver`.
    fn format_described(
        &mut self, c: &mut Context<'_>, recurse_times: usize, key: RootId, extras: Extras, fields: DescriptorFields, receiver: RootId,
    ) -> Result<String, RootedError> {
        let defined = |c: &Context<'_>, root: Option<RootId>| root.filter(|root| type_of(c, *root) != ValueType::Undefined);
        let mut separator = " ".to_owned();
        let text = if let Some(property) = defined(c, fields.value) {
            let indentation = if self.options.compact != Compact::Single || extras != Extras::Object {
                ENTRY_INDENTATION
            } else {
                SINGLE_LINE_PROPERTY_INDENTATION
            };
            self.indentation += indentation;
            let text = self.format_value(c, property, recurse_times, false)?;
            if indentation == SINGLE_LINE_PROPERTY_INDENTATION && self.options.break_length < self.visible_length(&text) as f64 {
                separator = format!("\n{}", " ".repeat(self.indentation));
            }
            self.indentation -= indentation;
            text
        } else if let Some(getter) = defined(c, fields.get) {
            let setter = defined(c, fields.set);
            let label = if setter.is_some() { "Getter/Setter" } else { "Getter" };
            let invoke = match self.options.getters {
                Getters::No => false,
                Getters::All => true,
                Getters::Get => setter.is_none(),
                Getters::Set => setter.is_some(),
            };
            if invoke {
                self.format_getter(c, getter, receiver, label, recurse_times)?
            } else {
                self.stylize(c, &format!("[{label}]"), "special")?
            }
        } else if defined(c, fields.set).is_some() {
            self.stylize(c, "[Setter]", "special")?
        } else {
            self.stylize(c, "undefined", "undefined")?
        };
        if extras == Extras::Element {
            return Ok(text);
        }
        let name = if c.is_symbol_rooted(key)? {
            let symbol = call_primordial(c, "symbolToString", key, &[])?;
            let units = c.string_code_units(symbol)?.unwrap_or_default();
            let escaped = String::from_utf16_lossy(&escape_units(&units, true));
            format!("[{}]", self.stylize(c, &escaped, "symbol")?)
        } else {
            let units = c.string_code_units(key)?.unwrap_or_default();
            let text = String::from_utf16_lossy(&units);
            if text == "__proto__" {
                "['__proto__']".to_owned()
            } else if fields.enumerable == Some(false) {
                format!("[{}]", String::from_utf16_lossy(&escape_units(&units, true)))
            } else if identifier_key(&text) {
                self.stylize(c, &text, "name")?
            } else {
                self.stylize(c, &quote(&units), "string")?
            }
        };
        Ok(format!("{name}:{separator}{text}"))
    }

    fn format_getter(&mut self, c: &mut Context<'_>, getter: RootId, receiver: RootId, label: &str, recurse_times: usize) -> Result<String, RootedError> {
        let open = self.stylize(c, &format!("[{label}:"), "special")?;
        let close = self.stylize(c, "]", "special")?;
        let result = match c.call_rooted(getter, receiver, &[]) {
            Ok(result) => result,
            Err(error) => {
                let Some(exception) = error.exception else { return Err(error) };
                let message = get(c, exception, "message")?;
                let message = c.to_string(message)?;
                return Ok(format!("{open} <Inspection threw ({message})>{close}"));
            }
        };
        self.indentation += ENTRY_INDENTATION;
        let text = match type_of(c, result) {
            ValueType::Null => format!("{open} {}{close}", self.stylize(c, "null", "null")?),
            ValueType::Object => {
                let label = self.stylize(c, &format!("[{label}]"), "special")?;
                format!("{label} {}", self.format_value(c, result, recurse_times, false)?)
            }
            _ => format!("{open} {}{close}", self.format_primitive(c, result)?),
        };
        self.indentation -= ENTRY_INDENTATION;
        Ok(text)
    }

    /// `reduceToSingleString`. `numeric` says whether an array's entries are all numbers,
    /// which right-aligns grouped columns; `None` when there is no array.
    fn reduce_to_single_string(
        &self, mut output: Vec<String>, base: &str, braces: &(String, String), extras: Extras, recurse_times: usize, numeric: Option<bool>,
    ) -> String {
        if self.options.compact != Compact::Single {
            if let Compact::Lines(levels) = self.options.compact {
                if levels >= 1.0 {
                    let entries = output.len();
                    if extras == Extras::Array && entries > GROUPING_MIN_ENTRIES {
                        output = self.group_array_elements(output, numeric);
                    }
                    if ((self.current_depth - recurse_times) as f64) < levels && entries == output.len() {
                        let start = output.len() + self.indentation + js_length(&braces.0) + js_length(base) + SINGLE_LINE_START;
                        if self.is_below_break_length(&output, start, base) {
                            let joined = output.join(", ");
                            if !joined.contains('\n') {
                                let base = if base.is_empty() { String::new() } else { format!("{base} ") };
                                return format!("{base}{} {joined} {}", braces.0, braces.1);
                            }
                        }
                    }
                }
            }
            let indentation = format!("\n{}", " ".repeat(self.indentation));
            let base = if base.is_empty() { String::new() } else { format!("{base} ") };
            return format!("{base}{}{indentation}  {}{indentation}{}", braces.0, output.join(&format!(",{indentation}  ")), braces.1);
        }
        let spaced_base = if base.is_empty() { String::new() } else { format!(" {base}") };
        if self.is_below_break_length(&output, 0, base) {
            return format!("{}{spaced_base} {} {}", braces.0, output.join(", "), braces.1);
        }
        let indentation = " ".repeat(self.indentation);
        let line = if base.is_empty() && js_length(&braces.0) == 1 { " ".to_owned() } else { format!("{spaced_base}\n{indentation}  ") };
        format!("{}{line}{} {}", braces.0, output.join(&format!(",\n{indentation}  ")), braces.1)
    }

    fn is_below_break_length(&self, output: &[String], start: usize, base: &str) -> bool {
        let mut total = output.len() + start;
        if (total + output.len()) as f64 > self.options.break_length {
            return false;
        }
        for entry in output {
            total += self.visible_length(entry);
            if total as f64 > self.options.break_length {
                return false;
            }
        }
        base.is_empty() || !base.contains('\n')
    }

    /// `groupArrayElements`: lines short entries up in columns.
    fn group_array_elements(&self, output: Vec<String>, numeric: Option<bool>) -> Vec<String> {
        let mut output_length = output.len();
        if self.options.max_array_length < output.len() as f64 {
            output_length -= 1;
        }
        let data_length = output[..output_length].iter().map(|entry| self.visible_length(entry)).collect::<Vec<_>>();
        let total_length: usize = data_length.iter().map(|length| length + SEPARATOR_SPACE).sum();
        let max_length = data_length.iter().copied().max().unwrap_or_default();
        let actual_max = max_length + SEPARATOR_SPACE;
        let fits = ((actual_max * MIN_GROUP_COLUMNS + self.indentation) as f64) < self.options.break_length;
        if !fits || !(total_length as f64 / actual_max as f64 > GROUP_LENGTH_SHARE || max_length <= GROUP_SHORT_ENTRY) {
            return output;
        }
        let average_bias = (actual_max as f64 - total_length as f64 / output.len() as f64).sqrt();
        let biased_max = (actual_max as f64 - MIN_GROUP_COLUMNS as f64 - average_bias).max(1.0);
        let levels = match self.options.compact {
            Compact::Lines(levels) => levels,
            Compact::Single | Compact::Never => 0.0,
        };
        let columns = ((APPROXIMATE_CHARACTER_HEIGHT * biased_max * output_length as f64).sqrt() / biased_max)
            .round()
            .min(((self.options.break_length - self.indentation as f64) / actual_max as f64).floor())
            .min(levels * COLUMNS_PER_COMPACT_LEVEL)
            .min(MAX_GROUP_COLUMNS);
        if columns <= 1.0 || columns.is_nan() {
            return output;
        }
        let columns = columns as usize;
        let max_line_length = (0..columns)
            .map(|column| data_length.iter().skip(column).step_by(columns).copied().max().unwrap_or_default() + SEPARATOR_SPACE)
            .collect::<Vec<_>>();
        let pad_start = numeric.unwrap_or(true);
        let mut grouped = Vec::new();
        for row in (0..output_length).step_by(columns) {
            let end = (row + columns).min(output_length);
            let mut line = String::new();
            for index in row..end - 1 {
                let padding = max_line_length[index - row] + js_length(&output[index]) - data_length[index];
                line.push_str(&pad(&format!("{}, ", output[index]), padding, pad_start));
            }
            let last = end - 1;
            if pad_start {
                let padding = max_line_length[last - row] + js_length(&output[last]) - data_length[last] - SEPARATOR_SPACE;
                line.push_str(&pad(&output[last], padding, true));
            } else {
                line.push_str(&output[last]);
            }
            grouped.push(line);
        }
        if self.options.max_array_length < output.len() as f64 {
            grouped.push(output[output_length].clone());
        }
        grouped
    }

    /// `formatError`.
    fn format_error(&mut self, c: &mut Context<'_>, error: RootId, constructor: Option<&str>, tag: &str, keys: &mut Vec<RootId>) -> Result<String, RootedError> {
        let name_value = get(c, error, "name")?;
        let name = if is_nullish(c, name_value) { "Error".to_owned() } else { c.to_string(name_value)? };
        let stack = stack_string(c, error)?;
        if !self.options.show_hidden && !keys.is_empty() {
            for property in ["name", "message", "stack"] {
                if let Some(index) = key_position(c, keys, property)? {
                    let value = get(c, error, property)?;
                    let text = c.to_string(value)?;
                    if stack.contains(&text) {
                        keys.remove(index);
                    }
                }
            }
        }
        let cause = c.string_rooted("cause");
        if c.has_property_rooted(error, cause)? && key_position(c, keys, "cause")?.is_none() {
            keys.push(cause);
        }
        let errors = get(c, error, "errors")?;
        if c.is_array_rooted(errors)? && key_position(c, keys, "errors")?.is_none() {
            let key = c.string_rooted("errors");
            keys.push(key);
        }
        let mut stack = improve_stack(&stack, constructor, &name, tag);
        let message = get(c, error, "message")?;
        let message = if c.truthy_rooted(message)? { Some(c.to_string(message)?) } else { None };
        let position = message
            .as_deref()
            .and_then(|message| stack.find(message).map(|at| at + message.len()))
            .unwrap_or(0);
        match stack[position..].find("\n    at").map(|at| at + position) {
            None => stack = format!("[{stack}]"),
            Some(start) => {
                let frames = self.stack_frames(c, error, &stack[start + 1..])?;
                stack = format!("{}\n{}", &stack[..start], frames.join("\n"));
            }
        }
        if self.indentation != 0 {
            stack = stack.replace('\n', &format!("\n{}", " ".repeat(self.indentation)));
        }
        Ok(stack)
    }

    /// `getStackFrames`: the frames, with a run shared with the cause's stack elided.
    fn stack_frames(&mut self, c: &mut Context<'_>, error: RootId, stack: &str) -> Result<Vec<String>, RootedError> {
        let mut frames = stack.split('\n').map(str::to_owned).collect::<Vec<_>>();
        let Ok(cause) = get(c, error, "cause") else { return Ok(frames) };
        if is_nullish(c, cause) || !is_error(c, cause)? {
            return Ok(frames);
        }
        let cause_stack = stack_string(c, cause)?;
        if let Some(start) = cause_stack.find("\n    at") {
            let cause_frames = cause_stack[start + 1..].split('\n').collect::<Vec<_>>();
            if let Some((length, offset)) = identical_sequence_range(&frames, &cause_frames) {
                let skipped = length - KEPT_IDENTICAL_FRAMES;
                let message = self.stylize(c, &format!("    ... {skipped} lines matching cause stack trace ..."), "undefined")?;
                frames.splice(offset + 1..offset + 1 + skipped, [message]);
            }
        }
        Ok(frames)
    }

    /// `getBoxedBase`.
    fn boxed_base(&mut self, c: &mut Context<'_>, value: RootId, keys: &mut Vec<RootId>, constructor: Option<&str>, tag: &str) -> Result<String, RootedError> {
        let Some(BuiltinData::Primitive { brand, value: primitive }) = c.builtin_data_rooted(value)? else {
            return Ok(String::new());
        };
        let kind = match brand {
            BuiltinBrand::NumberObject => "Number",
            BuiltinBrand::StringObject => "String",
            BuiltinBrand::BooleanObject => "Boolean",
            BuiltinBrand::BigIntObject => "BigInt",
            _ => "Symbol",
        };
        if brand == BuiltinBrand::StringObject {
            let length = c.string_code_units(primitive)?.map(|units| units.len()).unwrap_or_default();
            let mut kept = Vec::with_capacity(keys.len());
            for &key in keys.iter() {
                let index = c.string_text(key)?.as_deref().and_then(array_index);
                if !index.is_some_and(|index| (index as usize) < length) {
                    kept.push(key);
                }
            }
            *keys = kept;
        }
        let mut base = format!("[{kind}");
        if Some(kind) != constructor {
            match constructor {
                None => base.push_str(" (null prototype)"),
                Some(constructor) => base.push_str(&format!(" ({constructor})")),
            }
        }
        let stylize = std::mem::replace(&mut self.options.stylize, Stylize::Plain);
        let primitive = self.format_primitive(c, primitive);
        self.options.stylize = stylize;
        base.push_str(&format!(": {}]", primitive?));
        if !tag.is_empty() && Some(tag) != constructor {
            base.push_str(&format!(" [{tag}]"));
        }
        if !keys.is_empty() || matches!(self.options.stylize, Stylize::Plain) {
            return Ok(base);
        }
        self.stylize(c, &base, &kind.to_lowercase())
    }
}

fn plural(count: usize) -> &'static str {
    if count > 1 {
        "s"
    } else {
        ""
    }
}

fn push_remaining(output: &mut Vec<String>, remaining: usize) {
    if remaining > 0 {
        output.push(format!("... {remaining} more item{}", plural(remaining)));
    }
}

fn pad(text: &str, width: usize, start: bool) -> String {
    let fill = " ".repeat(width.saturating_sub(js_length(text)));
    if start {
        fill + text
    } else {
        format!("{text}{fill}")
    }
}

/// `keyStrRegExp`: a key `formatProperty` writes unquoted.
pub(super) fn identifier_key(key: &str) -> bool {
    let mut characters = key.chars();
    characters.next().is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// `numberRegExp`: a canonical array index.
fn array_index(text: &str) -> Option<f64> {
    let canonical = text == "0" || (!text.starts_with('0') && !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()));
    canonical.then(|| text.parse().ok()).flatten()
}

/// `getPrefix`.
fn prefix(constructor: Option<&str>, tag: &str, fallback: &str, size: &str) -> String {
    match constructor {
        None if !tag.is_empty() && fallback != tag => format!("[{fallback}{size}: null prototype] [{tag}] "),
        None => format!("[{fallback}{size}: null prototype] "),
        Some(constructor) if !tag.is_empty() && constructor != tag => format!("{constructor}{size} [{tag}] "),
        Some(constructor) => format!("{constructor}{size} "),
    }
}

/// `getIteratorBraces`.
fn iterator_braces(kind: &str, tag: &str) -> (String, String) {
    let iterator = format!("{kind} Iterator");
    let tag = if tag == iterator || tag.is_empty() {
        iterator
    } else {
        format!("{tag}] [{iterator}")
    };
    (format!("[{tag}] {{"), "}".to_owned())
}

fn brand_data(c: &mut Context<'_>, value: RootId, brand: BuiltinBrand) -> Result<Option<BuiltinData>, RootedError> {
    if !c.has_builtin_brand_rooted(value, brand)? {
        return Ok(None);
    }
    c.builtin_data_rooted(value)
}

fn array_length(c: &mut Context<'_>, value: RootId) -> Result<usize, RootedError> {
    let length = get(c, value, "length")?;
    Ok(number_of(c, length).unwrap_or_default().max(0.0) as usize)
}

/// The constructor name a typed array's `Symbol.toStringTag` getter reports.
fn typed_array_name(c: &mut Context<'_>, value: RootId) -> Result<&'static str, RootedError> {
    use BuiltinBrand as B;
    let brand = c.buffer_storage_rooted(value)?.map(|storage| storage.kind);
    Ok(match brand {
        Some(B::Int8Array) => "Int8Array",
        Some(B::Uint8ClampedArray) => "Uint8ClampedArray",
        Some(B::Int16Array) => "Int16Array",
        Some(B::Uint16Array) => "Uint16Array",
        Some(B::Int32Array) => "Int32Array",
        Some(B::Uint32Array) => "Uint32Array",
        Some(B::Float16Array) => "Float16Array",
        Some(B::Float32Array) => "Float32Array",
        Some(B::Float64Array) => "Float64Array",
        Some(B::BigInt64Array) => "BigInt64Array",
        Some(B::BigUint64Array) => "BigUint64Array",
        _ => "Uint8Array",
    })
}

/// Whether the first `count` elements are numbers or bigints (`groupArrayElements`' order).
fn all_numeric(c: &mut Context<'_>, value: RootId, count: usize) -> Result<bool, RootedError> {
    for index in 0..count {
        let element = get(c, value, &index.to_string())?;
        if !matches!(type_of(c, element), ValueType::Number | ValueType::BigInt) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn key_position(c: &mut Context<'_>, keys: &[RootId], name: &str) -> Result<Option<usize>, RootedError> {
    for (index, key) in keys.iter().enumerate() {
        if c.string_text(*key)?.as_deref() == Some(name) {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

/// `isError`: the native error brand, or an instance of `Error`.
pub(crate) fn is_error(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    if c.has_builtin_brand_rooted(value, BuiltinBrand::NativeError)? {
        return Ok(true);
    }
    let error = primordial(c, "Error")?;
    Ok(c.instance_of_rooted(value, error).unwrap_or(false))
}

/// Whether `value` is its own constructor's prototype (`value.constructor.prototype === value`).
fn is_prototype_object(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    let constructor = get(c, value, "constructor")?;
    if !c.truthy_rooted(constructor)? {
        return Ok(false);
    }
    let prototype = get(c, constructor, "prototype")?;
    c.strict_equal_rooted(prototype, value)
}

/// `getStackString`.
fn stack_string(c: &mut Context<'_>, error: RootId) -> Result<String, RootedError> {
    let stack = get(c, error, "stack")?;
    if c.truthy_rooted(stack)? {
        return c.to_string(stack);
    }
    let text = call_primordial(c, "errorToString", error, &[])?;
    c.to_string(text)
}

/// `improveStack`: names the constructor and tag in a regular error's first line.
fn improve_stack(stack: &str, constructor: Option<&str>, name: &str, tag: &str) -> String {
    let mut length = name.len();
    let regular = name.ends_with("Error")
        && stack.starts_with(name)
        && (stack.len() == length || matches!(stack.as_bytes()[length], b':' | b'\n'));
    if constructor.is_some() && !regular {
        return stack.to_owned();
    }
    let mut fallback = "Error".to_owned();
    if constructor.is_none() {
        let start = null_prototype_error_name(stack);
        length = start.len();
        if !start.is_empty() {
            fallback = start;
        }
    }
    let prefix = prefix(constructor, tag, &fallback, "");
    let prefix = &prefix[..prefix.len() - 1];
    if name == prefix {
        return stack.to_owned();
    }
    if prefix.contains(name) {
        if length == 0 {
            format!("{prefix}: {stack}")
        } else {
            format!("{prefix}{}", &stack[length..])
        }
    } else {
        format!("{prefix} [{name}]{}", &stack[length..])
    }
}

/// `/^([A-Z][a-z_ A-Z0-9[\]()-]+)(?::|\n\s+at)/` or `/^([a-z_A-Z0-9-]*Error)$/` on a stack.
fn null_prototype_error_name(stack: &str) -> String {
    let bytes = stack.as_bytes();
    if bytes.first().is_some_and(u8::is_ascii_uppercase) {
        let end = bytes[1..]
            .iter()
            .position(|byte| !(byte.is_ascii_alphanumeric() || b"_ []()-".contains(byte)))
            .map_or(bytes.len(), |end| end + 1);
        let rest = &stack[end..];
        let at_frames = rest.strip_prefix('\n').is_some_and(|rest| {
            let trimmed = rest.trim_start();
            trimmed.len() < rest.len() && trimmed.starts_with("at")
        });
        if end > 1 && (rest.starts_with(':') || at_frames) {
            return stack[..end].to_owned();
        }
    }
    if stack.ends_with("Error") && stack.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
        return stack.to_owned();
    }
    String::new()
}

/// `identicalSequenceRange`: the longest run of `frames` the cause's frames share, as
/// (length, offset), when it is long enough to elide.
fn identical_sequence_range(frames: &[String], cause: &[&str]) -> Option<(usize, usize)> {
    for offset in 0..frames.len().saturating_sub(MIN_IDENTICAL_FRAMES) {
        let Some(position) = cause.iter().position(|frame| *frame == frames[offset]) else { continue };
        let rest = cause.len() - position;
        if rest > MIN_IDENTICAL_FRAMES {
            let maximum = (frames.len() - offset).min(rest);
            let mut length = 1;
            while maximum > length && frames[offset + length] == cause[position + length] {
                length += 1;
            }
            if length > MIN_IDENTICAL_FRAMES {
                return Some((length, offset));
            }
        }
    }
    None
}

/// `getFunctionBase` and `getClassBase`.
fn function_base(c: &mut Context<'_>, value: RootId, constructor: Option<&str>, tag: &str) -> Result<String, RootedError> {
    let source = call_primordial(c, "functionToString", value, &[])?;
    let source = c.to_string(source)?;
    if source.starts_with("class") && source.ends_with('}') {
        let body = &source[5..source.len() - 1];
        if body.find('{').is_some_and(|bracket| !body[..bracket].contains('(')) {
            return class_base(c, value, constructor, tag);
        }
    }
    let mut kind = "Function".to_owned();
    if c.has_builtin_brand_rooted(value, BuiltinBrand::GeneratorFunction)? {
        kind = format!("Generator{kind}");
    }
    if c.has_builtin_brand_rooted(value, BuiltinBrand::AsyncFunction)? {
        kind = format!("Async{kind}");
    }
    let mut base = format!("[{kind}");
    if constructor.is_none() {
        base.push_str(" (null prototype)");
    }
    let name = get(c, value, "name")?;
    match c.string_text(name)? {
        Some(name) if name.is_empty() => base.push_str(" (anonymous)"),
        _ => {
            let name = c.to_string(name)?;
            base.push_str(&format!(": {name}"));
        }
    }
    base.push(']');
    if let Some(constructor) = constructor {
        if constructor != kind {
            base.push_str(&format!(" {constructor}"));
        }
    }
    if !tag.is_empty() && constructor != Some(tag) {
        base.push_str(&format!(" [{tag}]"));
    }
    Ok(base)
}

fn class_base(c: &mut Context<'_>, value: RootId, constructor: Option<&str>, tag: &str) -> Result<String, RootedError> {
    let name_key = c.string_rooted("name");
    let name = if c.own_property_fields_rooted(value, name_key)?.is_some() {
        let name = c.get_property_rooted(value, name_key)?;
        if c.truthy_rooted(name)? { c.to_string(name)? } else { String::new() }
    } else {
        String::new()
    };
    let name = if name.is_empty() { "(anonymous)".to_owned() } else { name };
    let mut base = format!("class {name}");
    if let Some(constructor) = constructor {
        if constructor != "Function" {
            base.push_str(&format!(" [{constructor}]"));
        }
    }
    if !tag.is_empty() && constructor != Some(tag) {
        base.push_str(&format!(" [{tag}]"));
    }
    if constructor.is_some() {
        let parent = c.prototype_rooted(value)?;
        let parent_name = get(c, parent, "name")?;
        if c.truthy_rooted(parent_name)? {
            base.push_str(&format!(" extends {}", c.to_string(parent_name)?));
        }
    } else {
        base.push_str(" extends [null prototype]");
    }
    Ok(format!("[{base}]"))
}

/// `formatNumber`, `formatBigInt` and the other primitives' text, with their style.
fn primitive_text(c: &mut Context<'_>, value: RootId, numeric_separator: bool) -> Result<(String, &'static str), RootedError> {
    Ok(match type_of(c, value) {
        ValueType::Number => (number_text(c, value, numeric_separator)?, "number"),
        ValueType::BigInt => {
            let digits = c.to_string(value)?;
            (format!("{}n", if numeric_separator { add_numeric_separator(&digits) } else { digits }), "bigint")
        }
        ValueType::Boolean => (c.to_string(value)?, "boolean"),
        ValueType::Undefined => ("undefined".to_owned(), "undefined"),
        ValueType::Null => ("null".to_owned(), "null"),
        ValueType::String => (c.to_string(value)?, "string"),
        _ => {
            let text = call_primordial(c, "symbolToString", value, &[])?;
            (c.to_string(text)?, "symbol")
        }
    })
}

/// `formatNumber` without its style.
fn number_text(c: &mut Context<'_>, value: RootId, numeric_separator: bool) -> Result<String, RootedError> {
    let number = number_of(c, value).unwrap_or(f64::NAN);
    let text = c.to_string(value)?;
    if !numeric_separator {
        return Ok(if number == 0.0 && number.is_sign_negative() { "-0".to_owned() } else { text });
    }
    let integer = number.trunc();
    let integer_root = c.number(integer);
    let integer_text = c.to_string(integer_root)?;
    if integer == number {
        if !number.is_finite() || integer_text.contains('e') {
            return Ok(integer_text);
        }
        return Ok(add_numeric_separator(&integer_text));
    }
    if number.is_nan() {
        return Ok(integer_text);
    }
    let fraction = text.get(integer_text.len() + 1..).unwrap_or_default();
    Ok(format!("{}.{}", add_numeric_separator(&integer_text), add_numeric_separator_end(fraction)))
}

fn add_numeric_separator(integer: &str) -> String {
    let start = usize::from(integer.starts_with('-'));
    let mut result = String::new();
    let mut index = integer.len();
    while index >= start + 4 {
        result = format!("_{}{result}", &integer[index - 3..index]);
        index -= 3;
    }
    if index == integer.len() {
        integer.to_owned()
    } else {
        format!("{}{result}", &integer[..index])
    }
}

fn add_numeric_separator_end(fraction: &str) -> String {
    let mut result = String::new();
    let mut index = 0;
    while index + 3 < fraction.len() {
        result.push_str(&fraction[index..index + 3]);
        result.push('_');
        index += 3;
    }
    if index == 0 {
        fraction.to_owned()
    } else {
        format!("{result}{}", &fraction[index..])
    }
}

/// Sorts entries by UTF-16 code units, or by the caller's comparator.
fn sort_output(c: &mut Context<'_>, output: &mut [String], sorted: Sorted) -> Result<(), RootedError> {
    let Sorted::By(comparator) = sorted else {
        output.sort_by(|left, right| left.encode_utf16().cmp(right.encode_utf16()));
        return Ok(());
    };
    for index in 1..output.len() {
        let mut position = index;
        while position > 0 {
            let left = c.string_rooted(&output[position - 1]);
            let right = c.string_rooted(&output[position]);
            let undefined = c.undefined();
            let order = c.call_rooted(comparator, undefined, &[left, right])?;
            if c.to_number(order)? > 0.0 {
                output.swap(position - 1, position);
                position -= 1;
            } else {
                break;
            }
        }
    }
    Ok(())
}

/// `stylizeWithColor`: reads the current `inspect.styles` and `inspect.colors`.
fn stylize_with_color(c: &mut Context<'_>, text: &str, style: &str) -> Result<String, RootedError> {
    let styles = primordial(c, "styles")?;
    let style = get(c, styles, style)?;
    if type_of(c, style) == ValueType::Undefined {
        return Ok(text.to_owned());
    }
    let colors = primordial(c, "colors")?;
    let key = c.to_string(style)?;
    let color = get(c, colors, &key)?;
    if type_of(c, color) == ValueType::Undefined {
        return Ok(text.to_owned());
    }
    let open = get(c, color, "0")?;
    let close = get(c, color, "1")?;
    let open = c.to_string(open)?;
    let close = c.to_string(close)?;
    Ok(format!("\u{1b}[{open}m{text}\u{1b}[{close}m"))
}

/// `hasBuiltInToString`: whether `%s` inspects an object rather than calling its `toString`.
fn has_built_in_to_string(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    let mut value = value;
    if let Some((target, _)) = c.proxy_details_rooted(value)? {
        if type_of(c, target) == ValueType::Null {
            return Ok(true);
        }
        value = target;
    }
    let to_primitive = primordial(c, "toPrimitive")?;
    let to_string = c.string_rooted("toString");
    let primitive_function = c.get_property_rooted(value, to_primitive)?;
    let string_function = c.get_property_rooted(value, to_string)?;
    if !c.is_callable_rooted(string_function)? && !c.is_callable_rooted(primitive_function)? {
        return Ok(true);
    }
    let converts = |c: &mut Context<'_>, holder: RootId| -> Result<bool, RootedError> {
        Ok(c.own_property_fields_rooted(holder, to_string)?.is_some() || c.own_property_fields_rooted(holder, to_primitive)?.is_some())
    };
    if converts(c, value)? {
        return Ok(false);
    }
    let mut pointer = value;
    loop {
        pointer = c.prototype_rooted(pointer)?;
        if type_of(c, pointer) == ValueType::Null {
            return Ok(true);
        }
        if converts(c, pointer)? {
            break;
        }
    }
    let constructor_key = c.string_rooted("constructor");
    let Some(constructor) = c.own_property_fields_rooted(pointer, constructor_key)?.and_then(|fields| fields.value) else {
        return Ok(false);
    };
    if type_of(c, constructor) != ValueType::Function {
        return Ok(false);
    }
    let name = get(c, constructor, "name")?;
    let name = c.string_text(name)?.unwrap_or_default();
    Ok(BUILT_IN_OBJECTS.contains(&name.as_str()))
}

/// `formatWithOptionsInternal`: the `util.format` algorithm over UTF-16 code units.
pub(crate) fn format_with_options(c: &mut Context<'_>, options: Option<RootId>, args: &[RootId]) -> Result<Vec<u16>, RootedError> {
    use crate::modules::querystring_shared_vm::PERCENT;
    let mut prepared = None;
    let mut output = Vec::new();
    let mut join: &[u16] = &[];
    let mut next = 0;
    if let Some(first) = args.first().copied().filter(|first| type_of(c, *first) == ValueType::String) {
        let units = c.string_code_units(first)?.unwrap_or_default();
        if args.len() == 1 {
            return Ok(units);
        }
        let mut last = 0;
        let mut index = 0;
        while index + 1 < units.len() {
            if units[index] != PERCENT {
                index += 1;
                continue;
            }
            index += 1;
            let specifier = units[index];
            if next + 1 == args.len() {
                if specifier == PERCENT {
                    output.extend_from_slice(&units[last..index]);
                    last = index + 1;
                }
                index += 1;
                continue;
            }
            let text = match u8::try_from(specifier).unwrap_or_default() {
                b's' => {
                    next += 1;
                    let argument = args[next];
                    match type_of(c, argument) {
                        ValueType::Number | ValueType::BigInt => {
                            let separator = base(c, options, &mut prepared)?.numeric_separator;
                            primitive_text(c, argument, separator)?.0
                        }
                        ValueType::Object if has_built_in_to_string(c, argument)? => {
                            let mut options = base(c, options, &mut prepared)?;
                            options.depth = Some(0.0);
                            options.colors = false;
                            options.stylize = Stylize::Plain;
                            options.compact = Compact::Lines(3.0);
                            inspect_value(c, argument, options)?
                        }
                        _ => {
                            let string = primordial(c, "String")?;
                            let undefined = c.undefined();
                            let text = c.call_rooted(string, undefined, &[argument])?;
                            c.string_text(text)?.unwrap_or_default()
                        }
                    }
                }
                b'j' => {
                    next += 1;
                    json_text(c, args[next])?
                }
                b'd' | b'i' | b'f' => {
                    next += 1;
                    let argument = args[next];
                    match type_of(c, argument) {
                        ValueType::BigInt if specifier != u16::from(b'f') => {
                            let separator = base(c, options, &mut prepared)?.numeric_separator;
                            primitive_text(c, argument, separator)?.0
                        }
                        ValueType::Symbol => "NaN".to_owned(),
                        _ => {
                            let convert = match u8::try_from(specifier).unwrap_or_default() {
                                b'd' => "Number",
                                b'i' => "parseInt",
                                _ => "parseFloat",
                            };
                            let undefined = c.undefined();
                            let number = call_primordial(c, convert, undefined, &[argument])?;
                            let separator = base(c, options, &mut prepared)?.numeric_separator;
                            number_text(c, number, separator)?
                        }
                    }
                }
                b'O' => {
                    next += 1;
                    let options = base(c, options, &mut prepared)?;
                    inspect_value(c, args[next], options)?
                }
                b'o' => {
                    next += 1;
                    let mut options = base(c, options, &mut prepared)?;
                    options.show_hidden = true;
                    options.show_proxy = true;
                    options.depth = Some(4.0);
                    inspect_value(c, args[next], options)?
                }
                b'c' => {
                    next += 1;
                    String::new()
                }
                b'%' => {
                    output.extend_from_slice(&units[last..index]);
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
                output.extend_from_slice(&units[last..index - 1]);
            }
            output.extend(text.encode_utf16());
            last = index + 1;
            index += 1;
        }
        if last != 0 {
            next += 1;
            join = &[b' ' as u16];
            if last < units.len() {
                output.extend_from_slice(&units[last..]);
            }
        }
    }
    for &argument in &args[next..] {
        output.extend_from_slice(join);
        if type_of(c, argument) == ValueType::String {
            output.extend(c.string_code_units(argument)?.unwrap_or_default());
        } else {
            let options = base(c, options, &mut prepared)?;
            output.extend(inspect_value(c, argument, options)?.encode_utf16());
        }
        join = &[b' ' as u16];
    }
    Ok(output)
}

/// The options `formatWithOptions(options)` inspects with, prepared on first use.
fn base(c: &mut Context<'_>, options: Option<RootId>, prepared: &mut Option<Options>) -> Result<Options, RootedError> {
    if let Some(prepared) = prepared {
        return Ok(*prepared);
    }
    let mut base = Options::defaults(c)?;
    if let Some(options) = options {
        base.apply(c, options)?;
    }
    *prepared = Some(base);
    Ok(base)
}

/// `tryStringify`: `JSON.stringify`, or `[Circular]` for a circular structure.
fn json_text(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let undefined = c.undefined();
    match call_primordial(c, "jsonStringify", undefined, &[value]) {
        Ok(text) => {
            if type_of(c, text) == ValueType::Undefined {
                Ok("undefined".to_owned())
            } else {
                c.to_string(text)
            }
        }
        Err(error) => {
            let Some(exception) = error.exception else { return Err(error) };
            let message = get(c, exception, "message")?;
            let message = c.to_string(message)?;
            if message.contains("circular structure") {
                Ok("[Circular]".to_owned())
            } else {
                Err(error)
            }
        }
    }
}

/// `util.inspect(value, opts)`.
pub(crate) fn inspect(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| c.undefined());
    let options = Options::from_arguments(c, args)?;
    let text = inspect_value(c, value, options)?;
    Ok(c.string_rooted(&text))
}

/// `util.format(...args)`.
pub(crate) fn format(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let units = format_with_options(c, None, args)?;
    Ok(c.string_units_rooted(&units))
}

/// `util.formatWithOptions(inspectOptions, ...args)`.
pub(crate) fn format_with_options_operation(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let options = args.first().copied().unwrap_or_else(|| c.undefined());
    if !c.is_object_rooted(options)? {
        return Err(super::util_shared_vm::argument_type_error(c, options, "inspectOptions", "Object")?);
    }
    let units = format_with_options(c, Some(options), &args[1..])?;
    Ok(c.string_units_rooted(&units))
}

/// The `stylize` a custom inspector receives; its data says whether colors are on.
pub(crate) fn stylize_operation(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let colors = c.host_function_data()?;
    let undefined = c.undefined();
    let text = args.first().copied().unwrap_or(undefined);
    let text = c.to_string(text)?;
    if !c.truthy_rooted(colors)? {
        return Ok(c.string_rooted(&text));
    }
    let style = args.get(1).copied().unwrap_or(undefined);
    let style = c.to_string(style)?;
    let styled = stylize_with_color(c, &text, &style)?;
    Ok(c.string_rooted(&styled))
}
