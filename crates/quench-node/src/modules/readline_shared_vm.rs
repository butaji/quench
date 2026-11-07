//! The `readline` and `readline/promises` modules: line interfaces over a readable input,
//! the terminal cursor commands, and the keypress decoder `emitKeypressEvents` installs.
//!
//! An interface keeps its state where Node keeps it, as properties of the instance (`line`,
//! `cursor`, `history`, ...) and its private fields under the symbols Node names them with;
//! the legacy `readline.Interface` exposes those as `_`-prefixed members.

use super::stream_shared_vm::{
    inherit,
    arg, call_method, captures, closure, emit, get, is_nullish, is_string, is_undefined, next_tick, number, on, once,
    put, put_bool, put_number, remove_listener, truthy_member, Class, Context, Outcome,
};
use super::string_decoder_codec::{DecoderState, Layout, ENCODINGS};
use quench_runtime::{DescriptorFields, RootId, RootedError, ValueType};

const MODULE_CACHE: &str = "\0builtin:readline";
const PROMISES_CACHE: &str = "\0builtin:readline/promises";

/// The symbols an interface keys its private state and internal methods with.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    SawReturnAt,
    SawKeyPress,
    PreviousKey,
    SubstringSearch,
    UndoStack,
    RedoStack,
    KillRing,
    KillRingCursor,
    Prompt,
    LineBuffer,
    Decoder,
    LineObjectStream,
    QuestionCallback,
    OldPrompt,
    QuestionReject,
    Question,
    QuestionCancel,
    SetRawMode,
    OnLine,
    WriteToOutput,
    AddHistory,
    RefreshLine,
    NormalWrite,
    InsertString,
    Line,
    HistoryNext,
    HistoryPrev,
    GetDisplayPos,
    MoveCursor,
    TtyWrite,
    DeleteLeft,
    DeleteRight,
    DeleteLineLeft,
    DeleteLineRight,
    KeypressDecoder,
    EscapeDecoder,
}

impl Slot {
    /// The symbol's description, which for the legacy-aliased slots is also the alias.
    fn description(self) -> &'static str {
        match self {
            Self::SawReturnAt => "_sawReturnAt",
            Self::SawKeyPress => "_sawKeyPress",
            Self::PreviousKey => "_previousKey",
            Self::SubstringSearch => "kSubstringSearch",
            Self::UndoStack => "_undoStack",
            Self::RedoStack => "_redoStack",
            Self::KillRing => "_killRing",
            Self::KillRingCursor => "_killRingCursor",
            Self::Prompt => "_prompt",
            Self::LineBuffer => "_line_buffer",
            Self::Decoder => "_decoder",
            Self::LineObjectStream => "line object stream",
            Self::QuestionCallback => "_questionCallback",
            Self::OldPrompt => "_oldPrompt",
            Self::QuestionReject => "_questionReject",
            Self::Question => "kQuestion",
            Self::QuestionCancel => "kQuestionCancel",
            Self::SetRawMode => "_setRawMode",
            Self::OnLine => "_onLine",
            Self::WriteToOutput => "_writeToOutput",
            Self::AddHistory => "_addHistory",
            Self::RefreshLine => "_refreshLine",
            Self::NormalWrite => "_normalWrite",
            Self::InsertString => "_insertString",
            Self::Line => "_line",
            Self::HistoryNext => "_historyNext",
            Self::HistoryPrev => "_historyPrev",
            Self::GetDisplayPos => "_getDisplayPos",
            Self::MoveCursor => "_moveCursor",
            Self::TtyWrite => "_ttyWrite",
            Self::DeleteLeft => "_deleteLeft",
            Self::DeleteRight => "_deleteRight",
            Self::DeleteLineLeft => "_deleteLineLeft",
            Self::DeleteLineRight => "_deleteLineRight",
            Self::KeypressDecoder => "keypress-decoder",
            Self::EscapeDecoder => "escape-decoder",
        }
    }

    fn cache_key(self) -> String {
        format!("\0readline:{}", self.description())
    }
}

/// The internal methods: their slot, operation and arity, in Node's class order.
const METHODS: [(Slot, &str); 17] = [
    (Slot::SetRawMode, "readlineSetRawMode"),
    (Slot::Question, "readlineQuestionInternal"),
    (Slot::OnLine, "readlineOnLine"),
    (Slot::QuestionCancel, "readlineQuestionCancel"),
    (Slot::WriteToOutput, "readlineWriteToOutput"),
    (Slot::AddHistory, "readlineAddHistory"),
    (Slot::RefreshLine, "readlineRefreshLine"),
    (Slot::NormalWrite, "readlineNormalWrite"),
    (Slot::InsertString, "readlineInsertString"),
    (Slot::DeleteLeft, "readlineDeleteLeft"),
    (Slot::DeleteRight, "readlineDeleteRight"),
    (Slot::DeleteLineLeft, "readlineDeleteLineLeft"),
    (Slot::DeleteLineRight, "readlineDeleteLineRight"),
    (Slot::Line, "readlineLine"),
    (Slot::HistoryNext, "readlineHistoryNext"),
    (Slot::HistoryPrev, "readlineHistoryPrev"),
    (Slot::GetDisplayPos, "readlineGetDisplayPos"),
];

/// The internal methods after `getCursorPos`, in Node's class order.
const LATE_METHODS: [(Slot, &str); 2] = [(Slot::MoveCursor, "readlineMoveCursor"), (Slot::TtyWrite, "readlineTtyWrite")];

/// The private fields the legacy interface exposes as `_`-prefixed accessors.
const LEGACY_FIELDS: [Slot; 8] = [
    Slot::Decoder,
    Slot::LineBuffer,
    Slot::OldPrompt,
    Slot::PreviousKey,
    Slot::Prompt,
    Slot::QuestionCallback,
    Slot::SawKeyPress,
    Slot::SawReturnAt,
];

/// `kHistorySize`: the history kept by default.
const HISTORY_SIZE: f64 = 30.0;
/// `kMincrlfDelay`: the shortest delay a `\r\n` pair may span.
const MIN_CRLF_DELAY: f64 = 100.0;
/// `ESCAPE_CODE_TIMEOUT`, in milliseconds.
const ESCAPE_CODE_TIMEOUT: f64 = 500.0;
/// The columns a tab stop spans by default.
const TAB_SIZE: f64 = 8.0;
pub(crate) use crate::modules::keys::DEFAULT_PROMPT;
/// The lines the async iterator buffers before pausing the input.
const ITERATOR_HIGH_WATER_MARK: f64 = 1024.0;
/// `historyIndex` when no history entry is shown.
const NO_HISTORY_ENTRY: f64 = -1.0;

use quench_runtime::units::{CARRIAGE_RETURN, LINE_FEED};
const TAB: u16 = b'\t' as u16;
/// The first code point outside the Basic Multilingual Plane: two UTF-16 units.
const SUPPLEMENTARY_PLANE: u32 = 0x10000;

/// The CSI sequences the cursor commands write.
const CLEAR_TO_LINE_BEGINNING: &str = "\x1b[1K";
const CLEAR_TO_LINE_END: &str = "\x1b[0K";
const CLEAR_LINE: &str = "\x1b[2K";
const CLEAR_SCREEN_DOWN: &str = "\x1b[0J";

// ---------------------------------------------------------------------------------------
// Shared roots
// ---------------------------------------------------------------------------------------

fn slot(c: &mut Context<'_>, which: Slot) -> Outcome {
    let shared = c.host_mut().shared_state();
    let found = shared.borrow().builtin_cache.get(&which.cache_key()).copied();
    if let Some(symbol) = found {
        return Ok(symbol);
    }
    let symbol = c.symbol_rooted(Some(which.description()));
    let retained = c.retain(symbol)?;
    shared.borrow_mut().builtin_cache.insert(which.cache_key(), retained);
    Ok(symbol)
}

fn get_slot(c: &mut Context<'_>, object: RootId, which: Slot) -> Outcome {
    let key = slot(c, which)?;
    c.get_property_rooted(object, key)
}

fn put_slot(c: &mut Context<'_>, object: RootId, which: Slot, value: RootId) -> Result<(), RootedError> {
    let key = slot(c, which)?;
    c.set_property_rooted(object, key, value, object).map(drop)
}

/// `this[slot](...args)`, through whatever the slot resolves to.
fn call_slot(c: &mut Context<'_>, object: RootId, which: Slot, args: &[RootId]) -> Outcome {
    let method = get_slot(c, object, which)?;
    c.call_rooted(method, object, args)
}

fn text(c: &mut Context<'_>, value: &str) -> RootId {
    c.string_rooted(value)
}

fn units_of(c: &mut Context<'_>, value: RootId) -> Result<Vec<u16>, RootedError> {
    match c.string_code_units(value)? {
        Some(units) => Ok(units),
        None => c.to_string_code_units(value),
    }
}

fn member_units(c: &mut Context<'_>, object: RootId, name: &str) -> Result<Vec<u16>, RootedError> {
    let value = get(c, object, name)?;
    units_of(c, value)
}

fn put_units(c: &mut Context<'_>, object: RootId, name: &str, units: &[u16]) -> Result<(), RootedError> {
    let value = c.string_units_rooted(units);
    put(c, object, name, value)
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_millis() as f64)
}

fn is_function(c: &Context<'_>, value: RootId) -> bool {
    matches!(c.value_type_rooted(value), Some(ValueType::Function))
}

fn is_number(c: &Context<'_>, value: RootId) -> bool {
    matches!(c.value_type_rooted(value), Some(ValueType::Number))
}

fn number_of(c: &Context<'_>, value: RootId) -> Option<f64> {
    c.rooted_value(value).and_then(|value| value.as_number())
}

// ---------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------

fn thrown(c: &mut Context<'_>, built: Outcome) -> RootedError {
    match built {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

fn invalid_type(c: &mut Context<'_>, value: RootId, name: &str, expected: &str) -> RootedError {
    match super::util_shared_vm::argument_type_error_with_expected(c, value, name, expected) {
        Ok(error) | Err(error) => error,
    }
}

fn invalid_value(c: &mut Context<'_>, value: RootId, name: &str) -> RootedError {
    match super::util_shared_vm::argument_value_error(c, value, name, "is invalid") {
        Ok(error) | Err(error) => error,
    }
}

fn require_function(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if c.is_callable_rooted(value)? { Ok(()) } else { Err(invalid_type(c, value, name, "of type function")) }
}

/// `new AbortError(message)`.
fn abort_error(c: &mut Context<'_>, message: &str) -> Outcome {
    let error = c.error_rooted(message)?;
    let code = text(c, "ABORT_ERR");
    put(c, error, "code", code)?;
    let name = text(c, "AbortError");
    put(c, error, "name", name)?;
    Ok(error)
}

/// `ERR_USE_AFTER_CLOSE('readline')`.
fn use_after_close(c: &mut Context<'_>) -> RootedError {
    let built = c.error_rooted("readline was closed").and_then(|error| {
        super::node_errors_shared_vm::decorate_error(c, error, "ERR_USE_AFTER_CLOSE")?;
        Ok(error)
    });
    thrown(c, built)
}

// ---------------------------------------------------------------------------------------
// Cursor commands (internal/readline/callbacks)
// ---------------------------------------------------------------------------------------

/// Reports a no-op command: `process.nextTick(callback, null)` and `true`.
fn no_command(c: &mut Context<'_>, callback: RootId) -> Outcome {
    if is_function(c, callback) {
        let null = c.null();
        next_tick(c, callback, &[null])?;
    }
    Ok(c.boolean(true))
}

fn write_command(c: &mut Context<'_>, stream: RootId, data: &str, callback: RootId) -> Outcome {
    let data = text(c, data);
    call_method(c, stream, "write", &[data, callback])
}

fn optional_callback(c: &mut Context<'_>, callback: RootId) -> Result<(), RootedError> {
    if is_undefined(c, callback) { Ok(()) } else { require_function(c, callback, "callback") }
}

/// The integer text a CSI parameter prints, as JavaScript prints a number.
fn parameter(c: &mut Context<'_>, value: f64) -> Result<String, RootedError> {
    let value = c.number(value);
    c.to_string(value)
}

/// `cursorTo(stream, x[, y][, callback])`.
pub(crate) fn cursor_to(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (stream, x, mut y, mut callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2), arg(c, args, 3));
    optional_callback(c, callback)?;
    if is_function(c, y) {
        callback = y;
        y = c.undefined();
    }
    for (value, name) in [(x, "x"), (y, "y")] {
        if number_of(c, value).is_some_and(f64::is_nan) {
            return Err(invalid_value(c, value, name));
        }
    }
    if is_nullish(c, stream) || (!is_number(c, x) && !is_number(c, y)) {
        return no_command(c, callback);
    }
    let Some(column) = number_of(c, x) else {
        let built = c.type_error_rooted("Cannot set cursor row without setting its column").and_then(|error| {
            super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_CURSOR_POS")?;
            Ok(error)
        });
        return Err(thrown(c, built));
    };
    let column = parameter(c, column + 1.0)?;
    let data = match number_of(c, y) {
        Some(row) => format!("\x1b[{};{column}H", parameter(c, row + 1.0)?),
        None => format!("\x1b[{column}G"),
    };
    write_command(c, stream, &data, callback)
}

/// `moveCursor(stream, dx, dy[, callback])`.
pub(crate) fn move_cursor(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (stream, dx, dy, callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2), arg(c, args, 3));
    optional_callback(c, callback)?;
    if is_nullish(c, stream) || !(c.truthy_rooted(dx)? || c.truthy_rooted(dy)?) {
        return no_command(c, callback);
    }
    let dx = c.to_number(dx)?;
    let dy = c.to_number(dy)?;
    let mut data = String::new();
    for (delta, backward, forward) in [(dx, 'D', 'C'), (dy, 'A', 'B')] {
        if delta < 0.0 {
            data.push_str(&format!("\x1b[{}{backward}", parameter(c, -delta)?));
        } else if delta > 0.0 {
            data.push_str(&format!("\x1b[{}{forward}", parameter(c, delta)?));
        }
    }
    write_command(c, stream, &data, callback)
}

/// `clearLine(stream, dir[, callback])`.
pub(crate) fn clear_line(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (stream, direction, callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    optional_callback(c, callback)?;
    if is_nullish(c, stream) {
        return no_command(c, callback);
    }
    let direction = c.to_number(direction)?;
    let data = if direction < 0.0 {
        CLEAR_TO_LINE_BEGINNING
    } else if direction > 0.0 {
        CLEAR_TO_LINE_END
    } else {
        CLEAR_LINE
    };
    write_command(c, stream, data, callback)
}

/// `clearScreenDown(stream[, callback])`.
pub(crate) fn clear_screen_down(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (stream, callback) = (arg(c, args, 0), arg(c, args, 1));
    optional_callback(c, callback)?;
    if is_nullish(c, stream) {
        return no_command(c, callback);
    }
    write_command(c, stream, CLEAR_SCREEN_DOWN, callback)
}

fn command(c: &mut Context<'_>, operation: fn(&mut Context<'_>, RootId, &[RootId]) -> Outcome, args: &[RootId]) -> Result<(), RootedError> {
    let undefined = c.undefined();
    operation(c, undefined, args).map(drop)
}

// ---------------------------------------------------------------------------------------
// Keypress decoding (internal/readline/utils `emitKeys`)
// ---------------------------------------------------------------------------------------

/// A decoded key, as the `keypress` event reports it.
struct Key {
    name: Option<String>,
    ctrl: bool,
    meta: bool,
    shift: bool,
    code: Option<String>,
}

/// What a prefix of pending characters decodes to.
enum Decoded {
    /// The prefix is the start of an escape sequence.
    Incomplete,
    /// A key spanning the first `length` characters, and whether it was escaped.
    Key { key: Key, length: usize, escaped: bool },
}

/// The names of the escape sequences, by code, with any modifier they imply.
const ESCAPE_CODES: &[(&str, &str, Modifier)] = &[
    ("[P", "f1", Modifier::None), ("[Q", "f2", Modifier::None), ("[R", "f3", Modifier::None), ("[S", "f4", Modifier::None),
    ("OP", "f1", Modifier::None), ("OQ", "f2", Modifier::None), ("OR", "f3", Modifier::None), ("OS", "f4", Modifier::None),
    ("[11~", "f1", Modifier::None), ("[12~", "f2", Modifier::None), ("[13~", "f3", Modifier::None), ("[14~", "f4", Modifier::None),
    ("[200~", "paste-start", Modifier::None), ("[201~", "paste-end", Modifier::None),
    ("[[A", "f1", Modifier::None), ("[[B", "f2", Modifier::None), ("[[C", "f3", Modifier::None), ("[[D", "f4", Modifier::None),
    ("[[E", "f5", Modifier::None),
    ("[15~", "f5", Modifier::None), ("[17~", "f6", Modifier::None), ("[18~", "f7", Modifier::None), ("[19~", "f8", Modifier::None),
    ("[20~", "f9", Modifier::None), ("[21~", "f10", Modifier::None), ("[23~", "f11", Modifier::None), ("[24~", "f12", Modifier::None),
    ("[A", "up", Modifier::None), ("[B", "down", Modifier::None), ("[C", "right", Modifier::None), ("[D", "left", Modifier::None),
    ("[E", "clear", Modifier::None), ("[F", "end", Modifier::None), ("[H", "home", Modifier::None),
    ("OA", "up", Modifier::None), ("OB", "down", Modifier::None), ("OC", "right", Modifier::None), ("OD", "left", Modifier::None),
    ("OE", "clear", Modifier::None), ("OF", "end", Modifier::None), ("OH", "home", Modifier::None),
    ("[1~", "home", Modifier::None), ("[2~", "insert", Modifier::None), ("[3~", "delete", Modifier::None), ("[4~", "end", Modifier::None),
    ("[5~", "pageup", Modifier::None), ("[6~", "pagedown", Modifier::None),
    ("[[5~", "pageup", Modifier::None), ("[[6~", "pagedown", Modifier::None),
    ("[7~", "home", Modifier::None), ("[8~", "end", Modifier::None),
    ("[a", "up", Modifier::Shift), ("[b", "down", Modifier::Shift), ("[c", "right", Modifier::Shift), ("[d", "left", Modifier::Shift),
    ("[e", "clear", Modifier::Shift),
    ("[2$", "insert", Modifier::Shift), ("[3$", "delete", Modifier::Shift), ("[5$", "pageup", Modifier::Shift),
    ("[6$", "pagedown", Modifier::Shift), ("[7$", "home", Modifier::Shift), ("[8$", "end", Modifier::Shift),
    ("Oa", "up", Modifier::Ctrl), ("Ob", "down", Modifier::Ctrl), ("Oc", "right", Modifier::Ctrl), ("Od", "left", Modifier::Ctrl),
    ("Oe", "clear", Modifier::Ctrl),
    ("[2^", "insert", Modifier::Ctrl), ("[3^", "delete", Modifier::Ctrl), ("[5^", "pageup", Modifier::Ctrl),
    ("[6^", "pagedown", Modifier::Ctrl), ("[7^", "home", Modifier::Ctrl), ("[8^", "end", Modifier::Ctrl),
    ("[Z", "tab", Modifier::Shift),
];

#[derive(Clone, Copy)]
enum Modifier {
    None,
    Shift,
    Ctrl,
}

/// The modifier bits of an xterm sequence: `1 + shift + 2 alt + 4 ctrl + 8 meta`.
const MODIFIER_SHIFT: u32 = 1;
const MODIFIER_META: u32 = 2 | 8;
const MODIFIER_CTRL: u32 = 4;
/// The last control character that is a Ctrl+letter (`\x1a`, Ctrl+Z).
const LAST_CONTROL_LETTER: char = '\x1a';
const BACKSPACE: char = '\x7f';

/// `/^(?:(\d\d?)(?:;(\d))?([~^$])|(\d{3}~))$/` then `/^((\d;)?(\d))?([A-Za-z])$/`: the
/// code suffix and modifier a CSI command spells.
fn csi_command(command: &[char]) -> (String, u32) {
    let digits = |part: &[char]| !part.is_empty() && part.iter().all(char::is_ascii_digit);
    let text: String = command.iter().collect();
    let modifier_of = |digit: Option<&char>| digit.and_then(|d| d.to_digit(10)).unwrap_or(1).saturating_sub(1);
    if let [number @ .., last] = command {
        if matches!(last, '~' | '^' | '$') {
            let (number, modifier) = match number.iter().position(|ch| *ch == ';') {
                Some(split) => (&number[..split], Some(&number[split + 1..])),
                None => (number, None),
            };
            let modifier_ok = modifier.is_none_or(|m| m.len() == 1 && digits(m));
            if (1..=2).contains(&number.len()) && digits(number) && modifier_ok {
                let code: String = number.iter().chain(std::iter::once(last)).collect();
                return (code, modifier_of(modifier.and_then(|m| m.first())));
            }
            if modifier.is_none() && number.len() == 3 && digits(number) && *last == '~' {
                return (text, 0);
            }
        }
        if last.is_ascii_alphabetic() {
            let modifier = match number {
                [] => Some(None),
                [digit] if digit.is_ascii_digit() => Some(Some(digit)),
                [first, ';', digit] if first.is_ascii_digit() && digit.is_ascii_digit() => Some(Some(digit)),
                _ => None,
            };
            if let Some(modifier) = modifier {
                return (last.to_string(), modifier_of(modifier));
            }
        }
    }
    (text, 0)
}

/// Decode the first key of `pending`, as `emitKeys` reads characters one by one.
fn decode(pending: &[char]) -> Decoded {
    let at = |index: usize| pending.get(index).copied();
    let Some(mut ch) = at(0) else { return Decoded::Incomplete };
    let mut index = 0;
    let mut escaped = false;
    if ch == '\x1b' {
        escaped = true;
        index += 1;
        let Some(next) = at(index) else { return Decoded::Incomplete };
        ch = next;
        if ch == '\x1b' {
            index += 1;
            let Some(next) = at(index) else { return Decoded::Incomplete };
            ch = next;
        }
    }
    let mut key = Key { name: None, ctrl: false, meta: false, shift: false, code: None };
    if escaped && (ch == 'O' || ch == '[') {
        let mut code = ch.to_string();
        let mut modifier = 0;
        if ch == 'O' {
            index += 1;
            let Some(next) = at(index) else { return Decoded::Incomplete };
            ch = next;
            if let Some(digit) = ch.to_digit(10) {
                modifier = digit.saturating_sub(1);
                index += 1;
                let Some(next) = at(index) else { return Decoded::Incomplete };
                ch = next;
            }
            code.push(ch);
        } else {
            index += 1;
            let Some(next) = at(index) else { return Decoded::Incomplete };
            ch = next;
            if ch == '[' {
                code.push(ch);
                index += 1;
                let Some(next) = at(index) else { return Decoded::Incomplete };
                ch = next;
            }
            let start = index;
            for _ in 0..3 {
                if !ch.is_ascii_digit() {
                    break;
                }
                index += 1;
                let Some(next) = at(index) else { return Decoded::Incomplete };
                ch = next;
            }
            if ch == ';' {
                index += 1;
                let Some(next) = at(index) else { return Decoded::Incomplete };
                ch = next;
                if ch.is_ascii_digit() {
                    index += 1;
                    if at(index).is_none() {
                        return Decoded::Incomplete;
                    }
                }
            }
            let (suffix, implied) = csi_command(&pending[start..=index]);
            code.push_str(&suffix);
            modifier = implied;
        }
        key.ctrl = modifier & MODIFIER_CTRL != 0;
        key.meta = modifier & MODIFIER_META != 0;
        key.shift = modifier & MODIFIER_SHIFT != 0;
        match ESCAPE_CODES.iter().find(|(candidate, ..)| *candidate == code) {
            Some(&(_, name, implied)) => {
                key.name = Some(name.to_owned());
                match implied {
                    Modifier::None => {}
                    Modifier::Shift => key.shift = true,
                    Modifier::Ctrl => key.ctrl = true,
                }
            }
            None => key.name = Some("undefined".to_owned()),
        }
        key.code = Some(code);
    } else {
        let named = match ch {
            '\r' => Some("return"),
            '\n' => Some("enter"),
            '\t' => Some("tab"),
            '\x08' | BACKSPACE => Some("backspace"),
            '\x1b' => Some("escape"),
            ' ' => Some("space"),
            _ => None,
        };
        if let Some(name) = named {
            key.name = Some(name.to_owned());
            key.meta = escaped;
        } else if !escaped && ch <= LAST_CONTROL_LETTER {
            key.name = char::from_u32(u32::from(ch) + u32::from('a') - 1).map(String::from);
            key.ctrl = true;
        } else if ch.is_ascii_alphanumeric() {
            key.name = Some(ch.to_ascii_lowercase().to_string());
            key.shift = ch.is_ascii_uppercase();
            key.meta = escaped;
        } else if escaped {
            key.meta = true;
        }
    }
    Decoded::Key { key, length: index + 1, escaped }
}

/// `emitKeypressEvents(stream[, interface])`.
pub(crate) fn emit_keypress_events(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let stream = arg(c, args, 0);
    let interface = arg(c, args, 1);
    let interface = if is_undefined(c, interface) { c.object_rooted()? } else { interface };
    install_keypress_decoder(c, stream, interface)?;
    Ok(c.undefined())
}

fn install_keypress_decoder(c: &mut Context<'_>, stream: RootId, interface: RootId) -> Result<(), RootedError> {
    if truthy_slot(c, stream, Slot::KeypressDecoder)? {
        return Ok(());
    }
    let decoder = utf8_decoder(c)?;
    put_slot(c, stream, Slot::KeypressDecoder, decoder)?;
    let pending = c.string_rooted("");
    put_slot(c, stream, Slot::EscapeDecoder, pending)?;
    let undefined = c.undefined();
    let environment = c.array_rooted(&[stream, interface, undefined, undefined])?;
    let data = c.host_function_named_with_data(crate::host::shared_vm::operation!("readlineKeypressData"), "onData", environment)?;
    let new_listener =
        c.host_function_named_with_data(crate::host::shared_vm::operation!("readlineKeypressNewListener"), "onNewListener", environment)?;
    put(c, environment, "2", data)?;
    put(c, environment, "3", new_listener)?;
    if super::stream_shared_vm::listener_count(c, stream, "keypress")? > 0.0 {
        on(c, stream, "data", data)
    } else {
        on(c, stream, "newListener", new_listener)
    }
}

fn truthy_slot(c: &mut Context<'_>, object: RootId, which: Slot) -> Result<bool, RootedError> {
    if !c.is_object_rooted(object)? {
        return Ok(false);
    }
    let value = get_slot(c, object, which)?;
    c.truthy_rooted(value)
}

/// The keypress `data` listener: decode keys while anyone listens for them.
pub(crate) fn keypress_data(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [stream, interface, data, on_new_listener] = captures(c)?;
    let input = arg(c, args, 0);
    if super::stream_shared_vm::listener_count(c, stream, "keypress")? <= 0.0 {
        remove_listener(c, stream, "data", data)?;
        on(c, stream, "newListener", on_new_listener)?;
        return Ok(c.undefined());
    }
    let decoder = get_slot(c, stream, Slot::KeypressDecoder)?;
    let units = decode_chunk(c, decoder, input)?;
    if units.is_empty() {
        return Ok(c.undefined());
    }
    let first_length = char_length_at(&units, 0);
    let saw = c.boolean(first_length == units.len());
    put_slot(c, interface, Slot::SawKeyPress, saw)?;
    put_bool(c, interface, "isCompletionEnabled", false)?;
    let pending = get_slot(c, stream, Slot::EscapeDecoder)?;
    let mut pending: Vec<char> = units_of(c, pending).map(|units| String::from_utf16_lossy(&units).chars().collect())?;
    let characters: Vec<char> = String::from_utf16_lossy(&units).chars().collect();
    let count = characters.len();
    for (position, character) in characters.into_iter().enumerate() {
        if position + 1 == count {
            put_bool(c, interface, "isCompletionEnabled", true)?;
        }
        pending.push(character);
        while let Decoded::Key { key, length, escaped } = decode(&pending) {
            let sequence: String = pending.drain(..length).collect();
            let remaining: String = pending.iter().collect();
            let remaining = text(c, &remaining);
            put_slot(c, stream, Slot::EscapeDecoder, remaining)?;
            emit_key(c, stream, key, &sequence, escaped)?;
        }
    }
    let remaining: String = pending.iter().collect();
    let remaining = text(c, &remaining);
    put_slot(c, stream, Slot::EscapeDecoder, remaining)?;
    Ok(c.undefined())
}

fn emit_key(c: &mut Context<'_>, stream: RootId, key: Key, sequence: &str, escaped: bool) -> Result<(), RootedError> {
    let named = key.name.is_some();
    let object = c.object_rooted()?;
    let sequence_value = text(c, sequence);
    put(c, object, "sequence", sequence_value)?;
    let name = match &key.name {
        Some(name) => text(c, name),
        None => c.undefined(),
    };
    put(c, object, "name", name)?;
    put_bool(c, object, "ctrl", key.ctrl)?;
    put_bool(c, object, "meta", key.meta)?;
    put_bool(c, object, "shift", key.shift)?;
    if let Some(code) = &key.code {
        let code = text(c, code);
        put(c, object, "code", code)?;
    }
    let units: Vec<u16> = sequence.encode_utf16().collect();
    if !units.is_empty() && (named || escaped) {
        let shown = if escaped { c.undefined() } else { sequence_value };
        emit(c, stream, "keypress", &[shown, object])?;
    } else if char_length_at(&units, 0) == units.len() {
        emit(c, stream, "keypress", &[sequence_value, object])?;
    }
    Ok(())
}

/// The keypress `newListener` listener: start decoding once a `keypress` listener comes.
pub(crate) fn keypress_new_listener(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [stream, _, on_data, listener] = captures(c)?;
    let event = arg(c, args, 0);
    if c.string_text(event)?.as_deref() == Some("keypress") {
        on(c, stream, "data", on_data)?;
        remove_listener(c, stream, "newListener", listener)?;
    }
    Ok(c.undefined())
}

// ---------------------------------------------------------------------------------------
// UTF-8 decoding and code unit helpers
// ---------------------------------------------------------------------------------------

/// A UTF-8 `StringDecoder` state, as the bytes the shared codec keeps.
fn utf8_decoder(c: &mut Context<'_>) -> Outcome {
    let mut state = vec![0; Layout::SIZE];
    state[Layout::ENCODING] = ENCODINGS.iter().position(|name| *name == "utf8").unwrap_or_default() as u8;
    c.uint8_array_from_rooted(state)
}

/// `decoder.write(chunk)`: a string chunk as it is, a buffer's complete characters.
fn decode_chunk(c: &mut Context<'_>, decoder: RootId, chunk: RootId) -> Result<Vec<u16>, RootedError> {
    if is_string(c, chunk) {
        return units_of(c, chunk);
    }
    let Some(bytes) = c.view_bytes_rooted(chunk) else {
        return units_of(c, chunk);
    };
    let state = c.view_bytes_rooted(decoder).and_then(|state| DecoderState::from_bytes(&state));
    let Some(mut state) = state else {
        return Ok(String::from_utf8_lossy(&bytes).encode_utf16().collect());
    };
    let units = state.decode(&bytes).map_err(|_| RootedError::host("readline input line exceeds the string limit"))?;
    c.with_buffer_bytes_mut_rooted(decoder, |target| target.copy_from_slice(&state.0));
    Ok(units)
}

/// `charLengthAt(str, i)`: the code units of the character at `index`.
fn char_length_at(units: &[u16], index: usize) -> usize {
    if units.len() <= index {
        return 1;
    }
    if code_point_at(units, index) >= SUPPLEMENTARY_PLANE { 2 } else { 1 }
}

/// `charLengthLeft(str, i)`: the code units of the character before `index`.
fn char_length_left(units: &[u16], index: usize) -> usize {
    if index == 0 {
        return 0;
    }
    if (index > 1 && code_point_at(units, index - 2) >= SUPPLEMENTARY_PLANE) || code_point_at(units, index - 1) >= SUPPLEMENTARY_PLANE {
        2
    } else {
        1
    }
}

fn code_point_at(units: &[u16], index: usize) -> u32 {
    char::decode_utf16(units[index..].iter().copied())
        .next()
        .map_or(0, |decoded| decoded.map_or_else(|error| u32::from(error.unpaired_surrogate()), u32::from))
}

/// The columns one character occupies, as `getStringWidth(char, false)` measures it.
#[cfg(quench_icu)]
fn character_width(units: &[u16]) -> u32 {
    super::text_decoder_icu::string_width(units, false, true)
}

/// The code points Node's width table counts as two columns.
#[cfg(not(quench_icu))]
const FULL_WIDTH: &[(u32, u32)] = &[
    (0x1100, 0x115f),
    (0x2329, 0x232a),
    (0x2e80, 0x303e),
    (0x3040, 0x3247),
    (0x3250, 0x4dbf),
    (0x4e00, 0xa4c6),
    (0xa960, 0xa97c),
    (0xac00, 0xd7a3),
    (0xf900, 0xfaff),
    (0xfe10, 0xfe19),
    (0xfe30, 0xfe6b),
    (0xff01, 0xff60),
    (0xffe0, 0xffe6),
    (0x1b000, 0x1b001),
    (0x1f200, 0x1f251),
    (0x1f300, 0x1f64f),
    (0x20000, 0x3fffd),
];

/// The code points Node's width table counts as no columns.
#[cfg(not(quench_icu))]
const ZERO_WIDTH: &[(u32, u32)] = &[
    (0x0, 0x1f),
    (0x7f, 0x9f),
    (0x300, 0x36f),
    (0x200b, 0x200f),
    (0x20d0, 0x20ff),
    (0xfe00, 0xfe0f),
    (0xfe20, 0xfe2f),
    (0xe0100, 0xe01ef),
];

#[cfg(not(quench_icu))]
fn character_width(units: &[u16]) -> u32 {
    let code = code_point_at(units, 0);
    let within = |ranges: &[(u32, u32)]| ranges.iter().any(|(low, high)| (*low..=*high).contains(&code));
    if within(FULL_WIDTH) {
        2
    } else if within(ZERO_WIDTH) {
        0
    } else {
        1
    }
}

/// The ranges of the first line ending (`/\r?\n|\r(?!\n)/`) at or after `from`.
fn line_ending(units: &[u16], from: usize) -> Option<(usize, usize)> {
    let mut index = from;
    while index < units.len() {
        match units[index] {
            LINE_FEED => return Some((index, index + 1)),
            CARRIAGE_RETURN if units.get(index + 1) == Some(&LINE_FEED) => return Some((index, index + 2)),
            CARRIAGE_RETURN => return Some((index, index + 1)),
            _ => index += 1,
        }
    }
    None
}

// ---------------------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------------------

/// `InterfaceConstructor(input, output, completer, terminal)`.
fn initialize(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<(), RootedError> {
    let (mut input, mut output, mut completer, mut terminal) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2), arg(c, args, 3));
    let zero = c.number(0.0);
    put_slot(c, this, Slot::SawReturnAt, zero)?;
    put_bool(c, this, "isCompletionEnabled", true)?;
    let no = c.boolean(false);
    put_slot(c, this, Slot::SawKeyPress, no)?;
    let null = c.null();
    put_slot(c, this, Slot::PreviousKey, null)?;
    put_number(c, this, "escapeCodeTimeout", ESCAPE_CODE_TIMEOUT)?;
    put_number(c, this, "tabSize", TAB_SIZE)?;
    super::stream_shared_vm::module(c)?;
    let emitter = super::stream_shared_vm::intrinsics(c)?.class(Class::Emitter);
    c.call_rooted(emitter, this, &[])?;
    let undefined = c.undefined();
    let (mut history, mut history_size, mut remove_duplicates, mut crlf_delay, mut signal) =
        (undefined, undefined, undefined, undefined, undefined);
    let mut prompt = text(c, DEFAULT_PROMPT);
    let options = input;
    if !is_nullish(c, options) && truthy_member(c, options, "input")? {
        output = get(c, options, "output")?;
        completer = get(c, options, "completer")?;
        terminal = get(c, options, "terminal")?;
        history = get(c, options, "history")?;
        history_size = get(c, options, "historySize")?;
        signal = get(c, options, "signal")?;
        let tab_size = get(c, options, "tabSize")?;
        if !is_undefined(c, tab_size) {
            let maximum = c.number(f64::from(u32::MAX));
            super::util_shared_vm::validate_integer(c, tab_size, "tabSize", 1.0, maximum)?;
            put(c, this, "tabSize", tab_size)?;
        }
        remove_duplicates = get(c, options, "removeHistoryDuplicates")?;
        let given_prompt = get(c, options, "prompt")?;
        if !is_undefined(c, given_prompt) {
            prompt = given_prompt;
        }
        let timeout = get(c, options, "escapeCodeTimeout")?;
        if !is_undefined(c, timeout) {
            if number_of(c, timeout).is_some_and(f64::is_finite) {
                put(c, this, "escapeCodeTimeout", timeout)?;
            } else {
                let current = get(c, this, "escapeCodeTimeout")?;
                return Err(invalid_value(c, current, "input.escapeCodeTimeout"));
            }
        }
        if c.truthy_rooted(signal)? {
            super::util_shared_vm::validate_abort_signal(c, signal, "options.signal")?;
        }
        crlf_delay = get(c, options, "crlfDelay")?;
        input = get(c, options, "input")?;
    }
    if !is_undefined(c, completer) && !is_function(c, completer) {
        return Err(invalid_value(c, completer, "completer"));
    }
    if is_undefined(c, history) {
        history = c.array_rooted(&[])?;
    } else if !c.is_array_rooted(history)? {
        return Err(invalid_type(c, history, "history", "an instance of Array"));
    }
    let history_size = if is_undefined(c, history_size) { c.number(HISTORY_SIZE) } else { history_size };
    match number_of(c, history_size) {
        None => return Err(invalid_type(c, history_size, "historySize", "of type number")),
        Some(size) if size < 0.0 || size.is_nan() => {
            let received = super::util_shared_vm::range_error_received(c, history_size)?;
            let message = format!("The value of \"historySize\" is out of range. It must be >= 0. Received {received}");
            let built = c.range_error_rooted(&message).and_then(|error| {
                super::node_errors_shared_vm::decorate_range_error(c, error, "ERR_OUT_OF_RANGE")?;
                Ok(error)
            });
            return Err(thrown(c, built));
        }
        Some(_) => {}
    }
    if is_undefined(c, terminal) && !is_nullish(c, output) {
        let tty = truthy_member(c, output, "isTTY")?;
        terminal = c.boolean(tty);
    }
    let empty = text(c, "");
    put(c, this, "line", empty)?;
    put_slot(c, this, Slot::SubstringSearch, null)?;
    put(c, this, "output", output)?;
    put(c, this, "input", input)?;
    for which in [Slot::UndoStack, Slot::RedoStack] {
        let stack = c.array_rooted(&[])?;
        put_slot(c, this, which, stack)?;
    }
    put(c, this, "history", history)?;
    put(c, this, "historySize", history_size)?;
    let ring = c.array_rooted(&[])?;
    put_slot(c, this, Slot::KillRing, ring)?;
    put_slot(c, this, Slot::KillRingCursor, zero)?;
    let remove = c.truthy_rooted(remove_duplicates)?;
    put_bool(c, this, "removeHistoryDuplicates", remove)?;
    let delay = if c.truthy_rooted(crlf_delay)? { c.to_number(crlf_delay)?.max(MIN_CRLF_DELAY) } else { MIN_CRLF_DELAY };
    put_number(c, this, "crlfDelay", delay)?;
    put(c, this, "completer", completer)?;
    call_method(c, this, "setPrompt", &[prompt])?;
    let terminal = c.truthy_rooted(terminal)?;
    put_bool(c, this, "terminal", terminal)?;
    let on_error = closure(c, "readlineInputError", "onerror", &[this])?;
    put_slot(c, this, Slot::LineObjectStream, undefined)?;
    on(c, input, "error", on_error)?;
    if terminal {
        let on_keypress = closure(c, "readlineInputKeypress", "onkeypress", &[this])?;
        let on_end = closure(c, "readlineInputTermEnd", "ontermend", &[this])?;
        let on_resize = closure(c, "readlineOutputResize", "onresize", &[this])?;
        install_keypress_decoder(c, input, this)?;
        on(c, input, "keypress", on_keypress)?;
        on(c, input, "end", on_end)?;
        let yes = c.boolean(true);
        call_slot(c, this, Slot::SetRawMode, &[yes])?;
        put_bool(c, this, "terminal", true)?;
        put_number(c, this, "cursor", 0.0)?;
        put_number(c, this, "historyIndex", NO_HISTORY_ENTRY)?;
        if !is_nullish(c, output) {
            on(c, output, "resize", on_resize)?;
        }
        let detach = closure(
            c,
            "readlineDetach",
            "onSelfCloseWithTerminal",
            &[input, output, on_keypress, on_error, on_end, on_resize],
        )?;
        once(c, this, "close", detach)?;
    } else {
        let on_data = closure(c, "readlineInputData", "ondata", &[this])?;
        let on_end = closure(c, "readlineInputEnd", "onend", &[this])?;
        on(c, input, "data", on_data)?;
        on(c, input, "end", on_end)?;
        let detach = closure(c, "readlineDetach", "onSelfCloseWithoutTerminal", &[input, undefined, on_data, on_error, on_end])?;
        once(c, this, "close", detach)?;
        let decoder = utf8_decoder(c)?;
        put_slot(c, this, Slot::Decoder, decoder)?;
    }
    if c.truthy_rooted(signal)? {
        let aborted = closure(c, "readlineAborted", "onAborted", &[this])?;
        if truthy_member(c, signal, "aborted")? {
            next_tick(c, aborted, &[])?;
        } else {
            let options = c.object_rooted()?;
            put_bool(c, options, "once", true)?;
            let abort = text(c, "abort");
            call_method(c, signal, "addEventListener", &[abort, aborted, options])?;
            let unlisten = closure(c, "readlineUnlistenAbort", "", &[signal, aborted])?;
            once(c, this, "close", unlisten)?;
        }
    }
    put(c, this, "line", empty)?;
    call_method(c, input, "resume", &[])?;
    Ok(())
}

/// `onSelfClose*`: remove the listeners an interface added to its input and output.
pub(crate) fn detach(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let environment = c.host_function_data()?;
    let values = c.array_like_values_rooted(environment)?;
    let (input, output) = (values[0], values[1]);
    let terminal = values.len() > 5;
    let events: &[&str] = if terminal { &["keypress", "error", "end"] } else { &["data", "error", "end"] };
    for (event, listener) in events.iter().zip(&values[2..]) {
        remove_listener(c, input, event, *listener)?;
    }
    if terminal && !is_nullish(c, output) {
        remove_listener(c, output, "resize", values[5])?;
    }
    Ok(c.undefined())
}

pub(crate) fn input_error(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [interface] = captures(c)?;
    let error = arg(c, args, 0);
    emit(c, interface, "error", &[error])?;
    Ok(c.undefined())
}

pub(crate) fn input_data(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [interface] = captures(c)?;
    let data = arg(c, args, 0);
    call_slot(c, interface, Slot::NormalWrite, &[data])?;
    Ok(c.undefined())
}

pub(crate) fn input_end(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [interface] = captures(c)?;
    let rest = get_slot(c, interface, Slot::LineBuffer)?;
    if is_string(c, rest) && !units_of(c, rest)?.is_empty() {
        emit(c, interface, "line", &[rest])?;
    }
    call_method(c, interface, "close", &[])?;
    Ok(c.undefined())
}

pub(crate) fn input_term_end(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [interface] = captures(c)?;
    let line = get(c, interface, "line")?;
    if is_string(c, line) && !units_of(c, line)?.is_empty() {
        emit(c, interface, "line", &[line])?;
    }
    call_method(c, interface, "close", &[])?;
    Ok(c.undefined())
}

pub(crate) fn input_keypress(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [interface] = captures(c)?;
    let (s, key) = (arg(c, args, 0), arg(c, args, 1));
    call_slot(c, interface, Slot::TtyWrite, &[s, key])?;
    if c.truthy_rooted(key)? {
        let sequence = get(c, key, "sequence")?;
        if is_string(c, sequence) {
            let units = units_of(c, sequence)?;
            if units.first().is_some_and(|unit| (0xd800..=0xdfff).contains(unit)) {
                call_slot(c, interface, Slot::RefreshLine, &[])?;
            }
        }
    }
    Ok(c.undefined())
}

pub(crate) fn output_resize(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [interface] = captures(c)?;
    call_slot(c, interface, Slot::RefreshLine, &[])?;
    Ok(c.undefined())
}

pub(crate) fn aborted(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [interface] = captures(c)?;
    call_method(c, interface, "close", &[])?;
    Ok(c.undefined())
}

pub(crate) fn unlisten_abort(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [signal, listener] = captures(c)?;
    let abort = text(c, "abort");
    call_method(c, signal, "removeEventListener", &[abort, listener])?;
    Ok(c.undefined())
}

/// `InterfaceConstructor`, called by the classes that extend it.
pub(crate) fn interface_constructor(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    initialize(c, this, args)?;
    Ok(c.undefined())
}

/// `new readline.Interface(input, output, completer, terminal)`: a legacy completer
/// (one argument) is adapted to the callback form.
pub(crate) fn legacy_constructor(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let [class] = captures(c)?;
    let mut args: Vec<RootId> = (0..4).map(|index| arg(c, args, index)).collect();
    if c.new_target_rooted().is_none() && !(c.is_object_rooted(this)? && c.instance_of_rooted(this, class)?) {
        return c.construct_rooted(class, class, &args);
    }
    let options = args[0];
    let options_completer = if !is_nullish(c, options) && truthy_member(c, options, "input")? {
        Some(get(c, options, "completer")?)
    } else {
        None
    };
    match options_completer {
        Some(completer) if is_function(c, completer) && number(c, completer, "length")? != 2.0 => {
            let adapted = closure(c, "readlineCompleterAdapter", "", &[completer])?;
            put(c, options, "completer", adapted)?;
        }
        Some(_) => {}
        None => {
            if is_function(c, args[2]) && number(c, args[2], "length")? != 2.0 {
                args[2] = closure(c, "readlineCompleterAdapter", "", &[args[2]])?;
            }
        }
    }
    initialize(c, this, &args)?;
    Ok(c.undefined())
}

/// `(v, cb) => cb(null, completer(v))`.
pub(crate) fn completer_adapter(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [completer] = captures(c)?;
    let (value, callback) = (arg(c, args, 0), arg(c, args, 1));
    let undefined = c.undefined();
    let result = c.call_rooted(completer, undefined, &[value])?;
    let null = c.null();
    c.call_rooted(callback, undefined, &[null, result])
}

/// `new readlinePromises.Interface(...)`.
pub(crate) fn promises_constructor(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    if c.new_target_rooted().is_none() {
        let built = c.type_error_rooted("Class constructor Interface cannot be invoked without 'new'");
        return Err(thrown(c, built));
    }
    initialize(c, this, args)?;
    Ok(c.undefined())
}

// ---------------------------------------------------------------------------------------
// Interface.prototype
// ---------------------------------------------------------------------------------------

/// The `columns` getter: the output's, or `Infinity`.
pub(crate) fn columns(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let output = get(c, this, "output")?;
    if !is_nullish(c, output) {
        let columns = get(c, output, "columns")?;
        if c.truthy_rooted(columns)? {
            return Ok(columns);
        }
    }
    Ok(c.number(f64::INFINITY))
}

pub(crate) fn set_prompt(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let prompt = arg(c, args, 0);
    put_slot(c, this, Slot::Prompt, prompt)?;
    Ok(c.undefined())
}

pub(crate) fn get_prompt(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    get_slot(c, this, Slot::Prompt)
}

/// `[kSetRawMode](mode)`: the input's previous raw mode.
pub(crate) fn set_raw_mode(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let mode = arg(c, args, 0);
    let input = get(c, this, "input")?;
    let was = get(c, input, "isRaw")?;
    let set = get(c, input, "setRawMode")?;
    if is_function(c, set) {
        c.call_rooted(set, input, &[mode])?;
    }
    Ok(was)
}

fn dumb_terminal(c: &mut Context<'_>) -> Result<bool, RootedError> {
    let global = c.global_root()?;
    let process = get(c, global, "process")?;
    let env = get(c, process, "env")?;
    let term = get(c, env, "TERM")?;
    Ok(c.string_text(term)?.as_deref() == Some("dumb"))
}

/// `prompt(preserveCursor)`.
pub(crate) fn prompt(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let preserve = arg(c, args, 0);
    if truthy_member(c, this, "paused")? {
        call_method(c, this, "resume", &[])?;
    }
    if truthy_member(c, this, "terminal")? && !dumb_terminal(c)? {
        if !c.truthy_rooted(preserve)? {
            put_number(c, this, "cursor", 0.0)?;
        }
        call_slot(c, this, Slot::RefreshLine, &[])?;
    } else {
        let prompt = get_slot(c, this, Slot::Prompt)?;
        call_slot(c, this, Slot::WriteToOutput, &[prompt])?;
    }
    Ok(c.undefined())
}

/// `[kQuestion](query, cb)`.
pub(crate) fn question_internal(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (query, callback) = (arg(c, args, 0), arg(c, args, 1));
    if truthy_member(c, this, "closed")? {
        return Err(use_after_close(c));
    }
    if !truthy_slot(c, this, Slot::QuestionCallback)? {
        let prompt = get_slot(c, this, Slot::Prompt)?;
        put_slot(c, this, Slot::OldPrompt, prompt)?;
        call_method(c, this, "setPrompt", &[query])?;
        put_slot(c, this, Slot::QuestionCallback, callback)?;
    }
    call_method(c, this, "prompt", &[])?;
    Ok(c.undefined())
}

/// `[kOnLine](line)`: answer the pending question, else emit `line`.
pub(crate) fn on_line(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let line = arg(c, args, 0);
    let callback = get_slot(c, this, Slot::QuestionCallback)?;
    if c.truthy_rooted(callback)? {
        let null = c.null();
        put_slot(c, this, Slot::QuestionCallback, null)?;
        let old = get_slot(c, this, Slot::OldPrompt)?;
        call_method(c, this, "setPrompt", &[old])?;
        let undefined = c.undefined();
        c.call_rooted(callback, undefined, &[line])?;
    } else {
        emit(c, this, "line", &[line])?;
    }
    Ok(c.undefined())
}

/// `[kQuestionCancel]()`.
pub(crate) fn question_cancel(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    if truthy_slot(c, this, Slot::QuestionCallback)? {
        let null = c.null();
        put_slot(c, this, Slot::QuestionCallback, null)?;
        let old = get_slot(c, this, Slot::OldPrompt)?;
        call_method(c, this, "setPrompt", &[old])?;
        call_method(c, this, "clearLine", &[])?;
    }
    Ok(c.undefined())
}

/// `[kWriteToOutput](stringToWrite)`.
pub(crate) fn write_to_output(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    if !is_string(c, value) {
        return Err(invalid_type(c, value, "stringToWrite", "of type string"));
    }
    let output = get(c, this, "output")?;
    if !is_nullish(c, output) {
        call_method(c, output, "write", &[value])?;
    }
    Ok(c.undefined())
}

fn trimmed_is_empty(units: &[u16]) -> bool {
    String::from_utf16_lossy(units).trim().is_empty()
}

/// `[kAddHistory]()`: record the line and return it as history now holds it.
pub(crate) fn add_history(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let line = get(c, this, "line")?;
    let units = units_of(c, line)?;
    if units.is_empty() {
        return Ok(text(c, ""));
    }
    if number(c, this, "historySize")? == 0.0 || trimmed_is_empty(&units) {
        return Ok(line);
    }
    let history = get(c, this, "history")?;
    let first = get(c, history, "0")?;
    if number(c, history, "length")? == 0.0 || !c.strict_equal_rooted(first, line)? {
        if truthy_member(c, this, "removeHistoryDuplicates")? {
            let index = call_method(c, history, "indexOf", &[line])?;
            if c.to_number(index)? != -1.0 {
                let one = c.number(1.0);
                call_method(c, history, "splice", &[index, one])?;
            }
        }
        call_method(c, history, "unshift", &[line])?;
        if number(c, history, "length")? > number(c, this, "historySize")? {
            call_method(c, history, "pop", &[])?;
        }
    }
    put_number(c, this, "historyIndex", NO_HISTORY_ENTRY)?;
    let line = get(c, history, "0")?;
    emit(c, this, "history", &[history])?;
    Ok(line)
}

/// `{ cols, rows }`.
fn position(c: &mut Context<'_>, columns: f64, rows: f64) -> Outcome {
    let object = c.object_rooted()?;
    put_number(c, object, "cols", columns)?;
    put_number(c, object, "rows", rows)?;
    Ok(object)
}

/// `[kGetDisplayPos](str)`: where the last character of `str` lands.
pub(crate) fn get_display_pos(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    let units = units_of(c, value)?;
    let units = super::util_strip::strip_vt_control_units(&units);
    let columns = number(c, this, "columns")?;
    let tab_size = number(c, this, "tabSize")?;
    let mut offset = 0.0_f64;
    let mut rows = 0.0_f64;
    let mut index = 0;
    while index < units.len() {
        let length = char_length_at(&units, index);
        let character = &units[index..(index + length).min(units.len())];
        index += length;
        if character == [LINE_FEED] {
            let wrapped = (offset / columns).ceil();
            rows += if wrapped == 0.0 || wrapped.is_nan() { 1.0 } else { wrapped };
            offset = 0.0;
            continue;
        }
        if character == [TAB] {
            offset += tab_size - (offset % tab_size);
            continue;
        }
        let width = character_width(character);
        if width <= 1 {
            offset += f64::from(width);
        } else {
            if (offset + 1.0) % columns == 0.0 {
                offset += 1.0;
            }
            offset += 2.0;
        }
    }
    let cols = offset % columns;
    rows += (offset - cols) / columns;
    position(c, cols, rows)
}

/// `getCursorPos()`.
pub(crate) fn get_cursor_pos(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let prompt = get_slot(c, this, Slot::Prompt)?;
    let mut units = units_of(c, prompt)?;
    let line = member_units(c, this, "line")?;
    let cursor = (number(c, this, "cursor")?.max(0.0) as usize).min(line.len());
    units.extend_from_slice(&line[..cursor]);
    let before = c.string_units_rooted(&units);
    call_slot(c, this, Slot::GetDisplayPos, &[before])
}

fn cursor_position(c: &mut Context<'_>, this: RootId) -> Result<(f64, f64), RootedError> {
    let position = call_method(c, this, "getCursorPos", &[])?;
    Ok((number(c, position, "cols")?, number(c, position, "rows")?))
}

/// `[kRefreshLine]()`: redraw the prompt and line, then put the cursor back.
pub(crate) fn refresh_line(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let prompt = get_slot(c, this, Slot::Prompt)?;
    let mut units = units_of(c, prompt)?;
    units.extend(member_units(c, this, "line")?);
    let line = c.string_units_rooted(&units);
    let display = call_slot(c, this, Slot::GetDisplayPos, &[line])?;
    let line_columns = number(c, display, "cols")?;
    let line_rows = number(c, display, "rows")?;
    let (cursor_columns, cursor_rows) = cursor_position(c, this)?;
    let output = get(c, this, "output")?;
    let previous = get(c, this, "prevRows")?;
    let previous = if c.truthy_rooted(previous)? { c.to_number(previous)? } else { 0.0 };
    let zero = c.number(0.0);
    if previous > 0.0 {
        let up = c.number(-previous);
        command(c, move_cursor, &[output, zero, up])?;
    }
    command(c, cursor_to, &[output, zero])?;
    command(c, clear_screen_down, &[output])?;
    call_slot(c, this, Slot::WriteToOutput, &[line])?;
    if line_columns == 0.0 {
        let space = text(c, " ");
        call_slot(c, this, Slot::WriteToOutput, &[space])?;
    }
    let column = c.number(cursor_columns);
    command(c, cursor_to, &[output, column])?;
    let difference = line_rows - cursor_rows;
    if difference > 0.0 {
        let up = c.number(-difference);
        command(c, move_cursor, &[output, zero, up])?;
    }
    put_number(c, this, "prevRows", cursor_rows)?;
    Ok(c.undefined())
}

/// `close()`.
pub(crate) fn close(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    if truthy_member(c, this, "closed")? {
        return Ok(c.undefined());
    }
    call_method(c, this, "pause", &[])?;
    if truthy_member(c, this, "terminal")? {
        let no = c.boolean(false);
        call_slot(c, this, Slot::SetRawMode, &[no])?;
    }
    put_bool(c, this, "closed", true)?;
    emit(c, this, "close", &[])?;
    Ok(c.undefined())
}

/// `pause()`.
pub(crate) fn pause(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    if truthy_member(c, this, "paused")? {
        return Ok(c.undefined());
    }
    let input = get(c, this, "input")?;
    call_method(c, input, "pause", &[])?;
    put_bool(c, this, "paused", true)?;
    emit(c, this, "pause", &[])?;
    Ok(this)
}

/// `resume()`.
pub(crate) fn resume(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    if !truthy_member(c, this, "paused")? {
        return Ok(c.undefined());
    }
    let input = get(c, this, "input")?;
    call_method(c, input, "resume", &[])?;
    put_bool(c, this, "paused", false)?;
    emit(c, this, "resume", &[])?;
    Ok(this)
}

/// `write(d, key)`.
pub(crate) fn write(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (data, key) = (arg(c, args, 0), arg(c, args, 1));
    if truthy_member(c, this, "paused")? {
        call_method(c, this, "resume", &[])?;
    }
    if truthy_member(c, this, "terminal")? {
        call_slot(c, this, Slot::TtyWrite, &[data, key])?;
    } else {
        call_slot(c, this, Slot::NormalWrite, &[data])?;
    }
    Ok(c.undefined())
}

/// `[kNormalWrite](b)`: split decoded input into lines; keep the unfinished rest.
pub(crate) fn normal_write(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let chunk = arg(c, args, 0);
    if is_undefined(c, chunk) {
        return Ok(c.undefined());
    }
    let decoder = get_slot(c, this, Slot::Decoder)?;
    let mut string = decode_chunk(c, decoder, chunk)?;
    let saw_return = saw_return_at(c, this)?;
    if saw_return != 0.0 && now() - saw_return <= number(c, this, "crlfDelay")? {
        if string.first() == Some(&LINE_FEED) {
            string.remove(0);
        }
        let zero = c.number(0.0);
        put_slot(c, this, Slot::SawReturnAt, zero)?;
    }
    let buffered = get_slot(c, this, Slot::LineBuffer)?;
    if line_ending(&string, 0).is_some() {
        if c.truthy_rooted(buffered)? {
            let mut joined = units_of(c, buffered)?;
            joined.extend_from_slice(&string);
            string = joined;
            let null = c.null();
            put_slot(c, this, Slot::LineBuffer, null)?;
        }
        let saw = if string.last() == Some(&CARRIAGE_RETURN) { now() } else { 0.0 };
        let saw = c.number(saw);
        put_slot(c, this, Slot::SawReturnAt, saw)?;
        let mut lines = Vec::new();
        let mut start = 0;
        while let Some((end, next)) = line_ending(&string, start) {
            lines.push(start..end);
            start = next;
        }
        let rest = c.string_units_rooted(&string[start..]);
        put_slot(c, this, Slot::LineBuffer, rest)?;
        for range in lines {
            let line = c.string_units_rooted(&string[range]);
            call_slot(c, this, Slot::OnLine, &[line])?;
        }
    } else if !string.is_empty() {
        let mut joined = if c.truthy_rooted(buffered)? { units_of(c, buffered)? } else { Vec::new() };
        joined.extend_from_slice(&string);
        let joined = c.string_units_rooted(&joined);
        put_slot(c, this, Slot::LineBuffer, joined)?;
    }
    Ok(c.undefined())
}

/// When the last `\r` arrived, or `0`.
fn saw_return_at(c: &mut Context<'_>, this: RootId) -> Result<f64, RootedError> {
    let value = get_slot(c, this, Slot::SawReturnAt)?;
    Ok(number_of(c, value).unwrap_or(0.0))
}

fn line_and_cursor(c: &mut Context<'_>, this: RootId) -> Result<(Vec<u16>, usize), RootedError> {
    let line = member_units(c, this, "line")?;
    let cursor = (number(c, this, "cursor")?.max(0.0) as usize).min(line.len());
    Ok((line, cursor))
}

/// `[kInsertString](c)`: while a keypress chunk is still being decoded
/// (`isCompletionEnabled` false) the text is echoed as typed.
pub(crate) fn insert_string(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let inserted = arg(c, args, 0);
    let inserted = units_of(c, inserted)?;
    let (mut line, cursor) = line_and_cursor(c, this)?;
    if !truthy_member(c, this, "isCompletionEnabled")? {
        line.splice(cursor..cursor, inserted.iter().copied());
        put_units(c, this, "line", &line)?;
        put_number(c, this, "cursor", (cursor + inserted.len()) as f64)?;
        let value = c.string_units_rooted(&inserted);
        call_slot(c, this, Slot::WriteToOutput, &[value])?;
        return Ok(c.undefined());
    }
    if cursor < line.len() {
        line.splice(cursor..cursor, inserted.iter().copied());
        put_units(c, this, "line", &line)?;
        put_number(c, this, "cursor", (cursor + inserted.len()) as f64)?;
        call_slot(c, this, Slot::RefreshLine, &[])?;
    } else {
        let (_, old_rows) = cursor_position(c, this)?;
        line.extend_from_slice(&inserted);
        put_units(c, this, "line", &line)?;
        put_number(c, this, "cursor", (cursor + inserted.len()) as f64)?;
        let (_, new_rows) = cursor_position(c, this)?;
        if old_rows < new_rows {
            call_slot(c, this, Slot::RefreshLine, &[])?;
        } else {
            let value = c.string_units_rooted(&inserted);
            call_slot(c, this, Slot::WriteToOutput, &[value])?;
        }
    }
    Ok(c.undefined())
}

fn edit_line(c: &mut Context<'_>, this: RootId, line: &[u16], cursor: usize) -> Outcome {
    put_units(c, this, "line", line)?;
    put_number(c, this, "cursor", cursor as f64)?;
    call_slot(c, this, Slot::RefreshLine, &[])
}

pub(crate) fn delete_left(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let (mut line, cursor) = line_and_cursor(c, this)?;
    if cursor > 0 && !line.is_empty() {
        let size = char_length_left(&line, cursor);
        line.drain(cursor - size..cursor);
        edit_line(c, this, &line, cursor - size)?;
    }
    Ok(c.undefined())
}

pub(crate) fn delete_right(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let (mut line, cursor) = line_and_cursor(c, this)?;
    if cursor < line.len() {
        let size = char_length_at(&line, cursor);
        line.drain(cursor..(cursor + size).min(line.len()));
        edit_line(c, this, &line, cursor)?;
    }
    Ok(c.undefined())
}

pub(crate) fn delete_line_left(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let (line, cursor) = line_and_cursor(c, this)?;
    edit_line(c, this, &line[cursor..], 0)?;
    Ok(c.undefined())
}

pub(crate) fn delete_line_right(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let (line, cursor) = line_and_cursor(c, this)?;
    edit_line(c, this, &line[..cursor], cursor)?;
    Ok(c.undefined())
}

/// `clearLine()`: end the line on the output and start an empty one.
pub(crate) fn clear_current_line(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let end = c.number(f64::INFINITY);
    call_slot(c, this, Slot::MoveCursor, &[end])?;
    let newline = text(c, "\r\n");
    call_slot(c, this, Slot::WriteToOutput, &[newline])?;
    let empty = text(c, "");
    put(c, this, "line", empty)?;
    put_number(c, this, "cursor", 0.0)?;
    put_number(c, this, "prevRows", 0.0)?;
    Ok(c.undefined())
}

/// `[kLine]()`.
pub(crate) fn line(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let line = call_slot(c, this, Slot::AddHistory, &[])?;
    for which in [Slot::UndoStack, Slot::RedoStack] {
        let stack = c.array_rooted(&[])?;
        put_slot(c, this, which, stack)?;
    }
    call_method(c, this, "clearLine", &[])?;
    call_slot(c, this, Slot::OnLine, &[line])?;
    Ok(c.undefined())
}

/// Show history entry `index` (`history.length` and `-1` show the search text).
fn show_history(c: &mut Context<'_>, this: RootId, step: f64) -> Result<(), RootedError> {
    let history = get(c, this, "history")?;
    let length = number(c, history, "length")?;
    let search = get_slot(c, this, Slot::SubstringSearch)?;
    let search = if c.truthy_rooted(search)? { units_of(c, search)? } else { Vec::new() };
    let line = get(c, this, "line")?;
    let mut index = number(c, this, "historyIndex")? + step;
    while index >= 0.0 && index < length {
        let key = c.number(index);
        let entry = c.get_property_rooted(history, key)?;
        let units = units_of(c, entry)?;
        if units.starts_with(&search) && !c.strict_equal_rooted(line, entry)? {
            break;
        }
        index += step;
    }
    let shown = if index < 0.0 || index >= length {
        c.string_units_rooted(&search)
    } else {
        let key = c.number(index);
        c.get_property_rooted(history, key)?
    };
    put(c, this, "line", shown)?;
    put_number(c, this, "historyIndex", index)?;
    let length = units_of(c, shown)?.len();
    put_number(c, this, "cursor", length as f64)?;
    call_slot(c, this, Slot::RefreshLine, &[]).map(drop)
}

/// `[kHistoryNext]()`: a newer entry.
pub(crate) fn history_next(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    if number(c, this, "historyIndex")? >= 0.0 {
        show_history(c, this, -1.0)?;
    }
    Ok(c.undefined())
}

/// `[kHistoryPrev]()`: an older entry.
pub(crate) fn history_prev(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let history = get(c, this, "history")?;
    let length = number(c, history, "length")?;
    if number(c, this, "historyIndex")? < length && length > 0.0 {
        show_history(c, this, 1.0)?;
    }
    Ok(c.undefined())
}

/// `[kMoveCursor](dx)`.
pub(crate) fn move_cursor_by(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let delta = arg(c, args, 0);
    let delta = c.to_number(delta)?;
    if delta == 0.0 {
        return Ok(c.undefined());
    }
    let (old_columns, old_rows) = cursor_position(c, this)?;
    let length = member_units(c, this, "line")?.len() as f64;
    let cursor = (number(c, this, "cursor")? + delta).clamp(0.0, length);
    put_number(c, this, "cursor", cursor)?;
    let (new_columns, new_rows) = cursor_position(c, this)?;
    if old_rows == new_rows {
        let output = get(c, this, "output")?;
        let difference = c.number(new_columns - old_columns);
        let zero = c.number(0.0);
        command(c, move_cursor, &[output, difference, zero])?;
    } else {
        call_slot(c, this, Slot::RefreshLine, &[])?;
    }
    Ok(c.undefined())
}

fn key_text(c: &mut Context<'_>, key: RootId, name: &str) -> Result<Option<String>, RootedError> {
    let value = get(c, key, name)?;
    c.string_text(value)
}

/// `[kTtyWrite](s, key)`: edit the line as the key asks.
pub(crate) fn tty_write(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (s, key) = (arg(c, args, 0), arg(c, args, 1));
    let key = if c.truthy_rooted(key)? { key } else { c.object_rooted()? };
    put_slot(c, this, Slot::PreviousKey, key)?;
    let name = key_text(c, key, "name")?;
    let ctrl = truthy_member(c, key, "ctrl")?;
    let meta = truthy_member(c, key, "meta")?;
    let shift = truthy_member(c, key, "shift")?;
    let null = c.null();
    if matches!(name.as_deref(), Some("up" | "down")) && !ctrl && !meta && !shift {
        let search = get_slot(c, this, Slot::SubstringSearch)?;
        if super::stream_shared_vm::is_null(c, search) {
            let (line, cursor) = line_and_cursor(c, this)?;
            let prefix = c.string_units_rooted(&line[..cursor]);
            put_slot(c, this, Slot::SubstringSearch, prefix)?;
        }
    } else {
        let search = get_slot(c, this, Slot::SubstringSearch)?;
        if !super::stream_shared_vm::is_null(c, search) {
            put_slot(c, this, Slot::SubstringSearch, null)?;
            let history = get(c, this, "history")?;
            if number(c, history, "length")? == number(c, this, "historyIndex")? {
                put_number(c, this, "historyIndex", NO_HISTORY_ENTRY)?;
            }
        }
    }
    if name.as_deref() == Some("escape") {
        return Ok(c.undefined());
    }
    let infinity = |c: &mut Context<'_>, sign: f64| c.number(sign * f64::INFINITY);
    if ctrl && shift {
        match name.as_deref() {
            Some("backspace") => drop(call_slot(c, this, Slot::DeleteLineLeft, &[])?),
            Some("delete") => drop(call_slot(c, this, Slot::DeleteLineRight, &[])?),
            _ => {}
        }
    } else if ctrl {
        match name.as_deref() {
            Some("c") => {
                if super::stream_shared_vm::listener_count(c, this, "SIGINT")? > 0.0 {
                    emit(c, this, "SIGINT", &[])?;
                } else {
                    call_method(c, this, "close", &[])?;
                    reject_question(c, this, "Aborted with Ctrl+C")?;
                }
            }
            Some("h") => drop(call_slot(c, this, Slot::DeleteLeft, &[])?),
            Some("d") => {
                let (line, cursor) = line_and_cursor(c, this)?;
                if cursor == 0 && line.is_empty() {
                    call_method(c, this, "close", &[])?;
                    reject_question(c, this, "Aborted with Ctrl+D")?;
                } else if cursor < line.len() {
                    call_slot(c, this, Slot::DeleteRight, &[])?;
                }
            }
            Some("u") => drop(call_slot(c, this, Slot::DeleteLineLeft, &[])?),
            Some("k") => drop(call_slot(c, this, Slot::DeleteLineRight, &[])?),
            Some("a") => {
                let delta = infinity(c, -1.0);
                call_slot(c, this, Slot::MoveCursor, &[delta])?;
            }
            Some("e") => {
                let delta = infinity(c, 1.0);
                call_slot(c, this, Slot::MoveCursor, &[delta])?;
            }
            Some("b") => step_cursor(c, this, false)?,
            Some("f") => step_cursor(c, this, true)?,
            Some("l") => {
                let output = get(c, this, "output")?;
                let zero = c.number(0.0);
                command(c, cursor_to, &[output, zero, zero])?;
                command(c, clear_screen_down, &[output])?;
                call_slot(c, this, Slot::RefreshLine, &[])?;
            }
            Some("n") => drop(call_slot(c, this, Slot::HistoryNext, &[])?),
            Some("p") => drop(call_slot(c, this, Slot::HistoryPrev, &[])?),
            _ => {}
        }
    } else if !meta {
        let saw = saw_return_at(c, this)?;
        if saw != 0.0 && name.as_deref() != Some("enter") {
            let zero = c.number(0.0);
            put_slot(c, this, Slot::SawReturnAt, zero)?;
        }
        match name.as_deref() {
            Some("return") => {
                let at = c.number(now());
                put_slot(c, this, Slot::SawReturnAt, at)?;
                call_slot(c, this, Slot::Line, &[])?;
            }
            Some("enter") => {
                let saw = saw_return_at(c, this)?;
                if saw == 0.0 || now() - saw > number(c, this, "crlfDelay")? {
                    call_slot(c, this, Slot::Line, &[])?;
                }
                let zero = c.number(0.0);
                put_slot(c, this, Slot::SawReturnAt, zero)?;
            }
            Some("backspace") => drop(call_slot(c, this, Slot::DeleteLeft, &[])?),
            Some("delete") => drop(call_slot(c, this, Slot::DeleteRight, &[])?),
            Some("left") => step_cursor(c, this, false)?,
            Some("right") => step_cursor(c, this, true)?,
            Some("home") => {
                let delta = infinity(c, -1.0);
                call_slot(c, this, Slot::MoveCursor, &[delta])?;
            }
            Some("end") => {
                let delta = infinity(c, 1.0);
                call_slot(c, this, Slot::MoveCursor, &[delta])?;
            }
            Some("up") => drop(call_slot(c, this, Slot::HistoryPrev, &[])?),
            Some("down") => drop(call_slot(c, this, Slot::HistoryNext, &[])?),
            _ => insert_lines(c, this, s)?,
        }
    }
    Ok(c.undefined())
}

fn step_cursor(c: &mut Context<'_>, this: RootId, forward: bool) -> Result<(), RootedError> {
    let (line, cursor) = line_and_cursor(c, this)?;
    let delta = if forward { char_length_at(&line, cursor) as f64 } else { -(char_length_left(&line, cursor) as f64) };
    let delta = c.number(delta);
    call_slot(c, this, Slot::MoveCursor, &[delta]).map(drop)
}

/// Insert typed text, ending a line at each line ending in it.
fn insert_lines(c: &mut Context<'_>, this: RootId, s: RootId) -> Result<(), RootedError> {
    if !is_string(c, s) {
        return Ok(());
    }
    let units = units_of(c, s)?;
    if units.is_empty() {
        return Ok(());
    }
    let mut start = 0;
    while let Some((end, next)) = line_ending(&units, start) {
        let part = c.string_units_rooted(&units[start..end]);
        call_slot(c, this, Slot::InsertString, &[part])?;
        call_slot(c, this, Slot::Line, &[])?;
        start = next;
    }
    let rest = c.string_units_rooted(&units[start..]);
    call_slot(c, this, Slot::InsertString, &[rest]).map(drop)
}

/// `this[kQuestionReject]?.(new AbortError(message))`.
fn reject_question(c: &mut Context<'_>, this: RootId, message: &str) -> Result<(), RootedError> {
    let reject = get_slot(c, this, Slot::QuestionReject)?;
    if !is_nullish(c, reject) {
        let error = abort_error(c, message)?;
        let undefined = c.undefined();
        c.call_rooted(reject, undefined, &[error])?;
    }
    Ok(())
}

/// `[Symbol.dispose]()`.
pub(crate) fn dispose(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    call_method(c, this, "close", &[])?;
    Ok(c.undefined())
}

// ---------------------------------------------------------------------------------------
// The async iterator over lines (`events.on(rl, 'line', { close: ['close'] })`)
// ---------------------------------------------------------------------------------------

/// The iterator's state, as the captures of its methods and listeners.
#[derive(Clone, Copy)]
enum IteratorField {
    Interface,
    Lines,
    Waiting,
    Error,
    Finished,
    Listeners,
}

const ITERATOR_FIELDS: usize = 6;

fn iterator_state(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let [state] = captures(c)?;
    Ok(state)
}

fn field(c: &mut Context<'_>, state: RootId, which: IteratorField) -> Outcome {
    get(c, state, &(which as usize).to_string())
}

fn set_field(c: &mut Context<'_>, state: RootId, which: IteratorField, value: RootId) -> Result<(), RootedError> {
    put(c, state, &(which as usize).to_string(), value)
}

/// `rl[Symbol.asyncIterator]()`: one iterator per interface.
pub(crate) fn async_iterator(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let existing = get_slot(c, this, Slot::LineObjectStream)?;
    if !is_undefined(c, existing) {
        return Ok(existing);
    }
    let undefined = c.undefined();
    let state = c.array_rooted(&[undefined; ITERATOR_FIELDS])?;
    set_field(c, state, IteratorField::Interface, this)?;
    let lines = c.array_rooted(&[])?;
    set_field(c, state, IteratorField::Lines, lines)?;
    let waiting = c.array_rooted(&[])?;
    set_field(c, state, IteratorField::Waiting, waiting)?;
    let no = c.boolean(false);
    set_field(c, state, IteratorField::Finished, no)?;
    let on_line = closure(c, "readlineIteratorLine", "eventHandler", &[state])?;
    let on_error = closure(c, "readlineIteratorError", "errorHandler", &[state])?;
    let on_close = closure(c, "readlineIteratorClose", "closeHandler", &[state])?;
    let listeners = c.array_rooted(&[on_line, on_error, on_close])?;
    set_field(c, state, IteratorField::Listeners, listeners)?;
    on(c, this, "line", on_line)?;
    on(c, this, "error", on_error)?;
    on(c, this, "close", on_close)?;
    let iterator = c.object_rooted()?;
    for (name, operation) in [("next", "readlineIteratorNext"), ("return", "readlineIteratorReturn"), ("throw", "readlineIteratorThrow")] {
        let method = closure(c, operation, name, &[state])?;
        put(c, iterator, name, method)?;
    }
    let itself = c.host_function_named(crate::host::shared_vm::operation!("readlineIteratorSelf"), "[Symbol.asyncIterator]")?;
    let key = super::stream_shared_vm::intrinsics(c)?.slot(super::stream_shared_vm::Slot::AsyncIterator);
    c.set_property_rooted(iterator, key, itself, iterator)?;
    put_slot(c, this, Slot::LineObjectStream, iterator)?;
    Ok(iterator)
}

pub(crate) fn iterator_self(_: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    Ok(this)
}

fn settled(c: &mut Context<'_>, value: RootId, done: bool) -> Outcome {
    let result = super::stream_shared_vm::iterator_result(c, value, done)?;
    let promise = c.pending_promise_rooted()?;
    c.resolve_promise_rooted(promise, result)?;
    Ok(promise)
}

fn rejected(c: &mut Context<'_>, error: RootId) -> Outcome {
    let promise = c.pending_promise_rooted()?;
    c.reject_promise_rooted(promise, error)?;
    Ok(promise)
}

pub(crate) fn iterator_line(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let state = iterator_state(c)?;
    let line = arg(c, args, 0);
    let waiting = field(c, state, IteratorField::Waiting)?;
    if number(c, waiting, "length")? > 0.0 {
        let promise = call_method(c, waiting, "shift", &[])?;
        let result = super::stream_shared_vm::iterator_result(c, line, false)?;
        c.resolve_promise_rooted(promise, result)?;
    } else {
        let lines = field(c, state, IteratorField::Lines)?;
        call_method(c, lines, "push", &[line])?;
        if number(c, lines, "length")? > ITERATOR_HIGH_WATER_MARK {
            let interface = field(c, state, IteratorField::Interface)?;
            call_method(c, interface, "pause", &[])?;
        }
    }
    Ok(c.undefined())
}

pub(crate) fn iterator_error(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let state = iterator_state(c)?;
    let error = arg(c, args, 0);
    let waiting = field(c, state, IteratorField::Waiting)?;
    if number(c, waiting, "length")? > 0.0 {
        let promise = call_method(c, waiting, "shift", &[])?;
        c.reject_promise_rooted(promise, error)?;
    } else {
        set_field(c, state, IteratorField::Error, error)?;
    }
    finish_iterator(c, state)?;
    Ok(c.undefined())
}

pub(crate) fn iterator_close(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let state = iterator_state(c)?;
    finish_iterator(c, state)?;
    Ok(c.undefined())
}

/// Stop listening, and end every waiting `next()`.
fn finish_iterator(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    let yes = c.boolean(true);
    set_field(c, state, IteratorField::Finished, yes)?;
    let interface = field(c, state, IteratorField::Interface)?;
    let listeners = field(c, state, IteratorField::Listeners)?;
    let listeners = c.array_like_values_rooted(listeners)?;
    for (event, listener) in ["line", "error", "close"].into_iter().zip(listeners) {
        remove_listener(c, interface, event, listener)?;
    }
    let waiting = field(c, state, IteratorField::Waiting)?;
    let undefined = c.undefined();
    for promise in c.array_like_values_rooted(waiting)? {
        let result = super::stream_shared_vm::iterator_result(c, undefined, true)?;
        c.resolve_promise_rooted(promise, result)?;
    }
    let empty = c.array_rooted(&[])?;
    set_field(c, state, IteratorField::Waiting, empty)
}

/// `next()`: a buffered line, the pending error, the end, or a promise of the next line.
pub(crate) fn iterator_next(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let state = iterator_state(c)?;
    let lines = field(c, state, IteratorField::Lines)?;
    if number(c, lines, "length")? > 0.0 {
        let line = call_method(c, lines, "shift", &[])?;
        let interface = field(c, state, IteratorField::Interface)?;
        if truthy_member(c, interface, "paused")? && number(c, lines, "length")? < ITERATOR_HIGH_WATER_MARK {
            call_method(c, interface, "resume", &[])?;
        }
        return settled(c, line, false);
    }
    let error = field(c, state, IteratorField::Error)?;
    if !is_undefined(c, error) {
        let undefined = c.undefined();
        set_field(c, state, IteratorField::Error, undefined)?;
        return rejected(c, error);
    }
    let finished = field(c, state, IteratorField::Finished)?;
    if c.truthy_rooted(finished)? {
        let undefined = c.undefined();
        return settled(c, undefined, true);
    }
    let promise = c.pending_promise_rooted()?;
    let waiting = field(c, state, IteratorField::Waiting)?;
    call_method(c, waiting, "push", &[promise])?;
    Ok(promise)
}

/// `return()`.
pub(crate) fn iterator_return(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let state = iterator_state(c)?;
    finish_iterator(c, state)?;
    let undefined = c.undefined();
    settled(c, undefined, true)
}

/// `throw(err)`.
pub(crate) fn iterator_throw(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let state = iterator_state(c)?;
    let error = arg(c, args, 0);
    if !c.is_error_rooted(error)? {
        return Err(invalid_type(c, error, "EventEmitter.AsyncIterator", "an instance of Error"));
    }
    set_field(c, state, IteratorField::Error, error)?;
    let interface = field(c, state, IteratorField::Interface)?;
    let listeners = field(c, state, IteratorField::Listeners)?;
    let listeners = c.array_like_values_rooted(listeners)?;
    for (event, listener) in ["line", "error", "close"].into_iter().zip(listeners) {
        remove_listener(c, interface, event, listener)?;
    }
    let yes = c.boolean(true);
    set_field(c, state, IteratorField::Finished, yes)?;
    Ok(c.undefined())
}

// ---------------------------------------------------------------------------------------
// The legacy `question` and the promise forms
// ---------------------------------------------------------------------------------------

/// `readline.Interface.prototype.question(query[, options], cb)`.
pub(crate) fn question(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (query, options, callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let mut callback = if is_function(c, options) { options } else { callback };
    let options = if c.is_object_rooted(options)? && !is_function(c, options) { Some(options) } else { None };
    if let Some(options) = options {
        let signal = get(c, options, "signal")?;
        if c.truthy_rooted(signal)? {
            super::util_shared_vm::validate_abort_signal(c, signal, "options.signal")?;
            if truthy_member(c, signal, "aborted")? {
                return Ok(c.undefined());
            }
            let cancel = closure(c, "readlineQuestionAbort", "onAbort", &[this])?;
            let listen = c.object_rooted()?;
            put_bool(c, listen, "once", true)?;
            let abort = text(c, "abort");
            call_method(c, signal, "addEventListener", &[abort, cancel, listen])?;
            callback = closure(c, "readlineQuestionAnswered", "", &[signal, cancel, callback])?;
        }
    }
    if is_function(c, callback) {
        call_slot(c, this, Slot::Question, &[query, callback])?;
    }
    Ok(c.undefined())
}

pub(crate) fn question_abort(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [interface] = captures(c)?;
    call_slot(c, interface, Slot::QuestionCancel, &[])?;
    Ok(c.undefined())
}

/// The answer callback of a question with a signal: stop listening, then answer.
pub(crate) fn question_answered(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [signal, listener, callback] = captures(c)?;
    let abort = text(c, "abort");
    call_method(c, signal, "removeEventListener", &[abort, listener])?;
    if is_function(c, callback) {
        let answer = arg(c, args, 0);
        let undefined = c.undefined();
        c.call_rooted(callback, undefined, &[answer])?;
    }
    Ok(c.undefined())
}

/// `question[util.promisify.custom](query, options)` and `readlinePromises.Interface.prototype.question`.
pub(crate) fn question_promise(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (query, options) = (arg(c, args, 0), arg(c, args, 1));
    let signal = if c.is_object_rooted(options)? { get(c, options, "signal")? } else { c.undefined() };
    let promise = c.pending_promise_rooted()?;
    if c.truthy_rooted(signal)? {
        super::util_shared_vm::validate_abort_signal(c, signal, "options.signal")?;
        if truthy_member(c, signal, "aborted")? {
            let error = super::stream_shared_vm::errors::abort_error_for(c, signal)?;
            c.reject_promise_rooted(promise, error)?;
            return Ok(promise);
        }
    }
    let resolve = closure(c, "readlineQuestionResolve", "", &[promise])?;
    let reject = closure(c, "readlineQuestionReject", "", &[promise])?;
    put_slot(c, this, Slot::QuestionReject, reject)?;
    let answer = if c.truthy_rooted(signal)? {
        let cancel = closure(c, "readlineQuestionAbortReject", "onAbort", &[this, promise, signal])?;
        let listen = c.object_rooted()?;
        put_bool(c, listen, "once", true)?;
        let abort = text(c, "abort");
        call_method(c, signal, "addEventListener", &[abort, cancel, listen])?;
        closure(c, "readlineQuestionAnswered", "", &[signal, cancel, resolve])?
    } else {
        resolve
    };
    call_slot(c, this, Slot::Question, &[query, answer])?;
    Ok(promise)
}

pub(crate) fn question_resolve(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [promise] = captures(c)?;
    let answer = arg(c, args, 0);
    c.resolve_promise_rooted(promise, answer)?;
    Ok(c.undefined())
}

pub(crate) fn question_reject(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [promise] = captures(c)?;
    let error = arg(c, args, 0);
    c.reject_promise_rooted(promise, error)?;
    Ok(c.undefined())
}

pub(crate) fn question_abort_reject(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [interface, promise, signal] = captures(c)?;
    call_slot(c, interface, Slot::QuestionCancel, &[])?;
    let error = super::stream_shared_vm::errors::abort_error_for(c, signal)?;
    c.reject_promise_rooted(promise, error)?;
    Ok(c.undefined())
}

/// A legacy alias getter: `this[name]`, the `_`-prefixed member the getter closes over.
pub(crate) fn alias_get(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let name = c.host_function_data()?;
    c.get_property_rooted(this, name)
}

/// A private field's legacy accessor pair: the getter and setter close over the symbol.
pub(crate) fn field_get(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let key = c.host_function_data()?;
    c.get_property_rooted(this, key)
}

pub(crate) fn field_set(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let key = c.host_function_data()?;
    let value = arg(c, args, 0);
    c.set_property_rooted(this, key, value, this)?;
    Ok(c.undefined())
}

/// `createInterface(input, output, completer, terminal)`, closing over the class.
pub(crate) fn create_interface(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let class = c.host_function_data()?;
    let args: Vec<RootId> = (0..4).map(|index| arg(c, args, index)).collect();
    c.construct_rooted(class, class, &args)
}

// ---------------------------------------------------------------------------------------
// readline/promises Readline
// ---------------------------------------------------------------------------------------

/// `new Readline(stream, { autoCommit })`.
pub(crate) fn readline_constructor(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (stream, options) = (arg(c, args, 0), arg(c, args, 1));
    if c.new_target_rooted().is_none() {
        let built = c.type_error_rooted("Class constructor Readline cannot be invoked without 'new'");
        return Err(thrown(c, built));
    }
    let writable = if c.is_object_rooted(stream)? { get(c, stream, "write")? } else { c.undefined() };
    if !is_function(c, writable) {
        return Err(invalid_type(c, stream, "stream", "an instance of Writable"));
    }
    put_slot_named(c, this, "#stream", stream)?;
    let auto = if c.is_object_rooted(options)? { truthy_member(c, options, "autoCommit")? } else { false };
    put_bool_named(c, this, "#autoCommit", auto)?;
    let todo = c.array_rooted(&[])?;
    put_slot_named(c, this, "#todo", todo)?;
    Ok(c.undefined())
}

fn private_key(c: &mut Context<'_>, name: &'static str) -> Outcome {
    let cache = format!("\0readline:{name}");
    let shared = c.host_mut().shared_state();
    if let Some(key) = shared.borrow().builtin_cache.get(&cache).copied() {
        return Ok(key);
    }
    let key = c.private_symbol_rooted(Some(name));
    let retained = c.retain(key)?;
    shared.borrow_mut().builtin_cache.insert(cache, retained);
    Ok(key)
}

fn put_slot_named(c: &mut Context<'_>, object: RootId, name: &'static str, value: RootId) -> Result<(), RootedError> {
    let key = private_key(c, name)?;
    c.set_property_rooted(object, key, value, object).map(drop)
}

fn put_bool_named(c: &mut Context<'_>, object: RootId, name: &'static str, value: bool) -> Result<(), RootedError> {
    let value = c.boolean(value);
    put_slot_named(c, object, name, value)
}

fn get_named(c: &mut Context<'_>, object: RootId, name: &'static str) -> Outcome {
    let key = private_key(c, name)?;
    c.get_property_rooted(object, key)
}

/// Queue or write one command's text, as `autoCommit` asks; returns `this`.
fn readline_command(c: &mut Context<'_>, this: RootId, data: &str) -> Outcome {
    let stream = get_named(c, this, "#stream")?;
    let auto = get_named(c, this, "#autoCommit")?;
    let data = text(c, data);
    if c.truthy_rooted(auto)? {
        call_method(c, stream, "write", &[data])?;
    } else {
        let todo = get_named(c, this, "#todo")?;
        call_method(c, todo, "push", &[data])?;
    }
    Ok(this)
}

fn integer_argument(c: &mut Context<'_>, value: RootId, name: &str) -> Result<f64, RootedError> {
    let minimum = f64::from(i32::MIN);
    let maximum = c.number(f64::from(i32::MAX));
    super::util_shared_vm::validate_integer(c, value, name, minimum, maximum)
}

/// `readline.clearLine(dir)`.
pub(crate) fn readline_clear_line(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let direction = arg(c, args, 0);
    let direction = integer_argument(c, direction, "dir")?;
    let data = if direction < 0.0 {
        CLEAR_TO_LINE_BEGINNING
    } else if direction > 0.0 {
        CLEAR_TO_LINE_END
    } else {
        CLEAR_LINE
    };
    readline_command(c, this, data)
}

pub(crate) fn readline_clear_screen_down(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    readline_command(c, this, CLEAR_SCREEN_DOWN)
}

/// `readline.cursorTo(x[, y])`.
pub(crate) fn readline_cursor_to(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (x, y) = (arg(c, args, 0), arg(c, args, 1));
    let column = integer_argument(c, x, "x")?;
    let column = parameter(c, column + 1.0)?;
    let data = if is_undefined(c, y) {
        format!("\x1b[{column}G")
    } else {
        let row = integer_argument(c, y, "y")?;
        format!("\x1b[{};{column}H", parameter(c, row + 1.0)?)
    };
    readline_command(c, this, &data)
}

/// `readline.moveCursor(dx, dy)`.
pub(crate) fn readline_move_cursor(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (dx, dy) = (arg(c, args, 0), arg(c, args, 1));
    let dx = integer_argument(c, dx, "dx")?;
    let dy = integer_argument(c, dy, "dy")?;
    if dx == 0.0 && dy == 0.0 {
        return Ok(this);
    }
    let mut data = String::new();
    for (delta, backward, forward) in [(dx, 'D', 'C'), (dy, 'A', 'B')] {
        if delta < 0.0 {
            data.push_str(&format!("\x1b[{}{backward}", parameter(c, -delta)?));
        } else if delta > 0.0 {
            data.push_str(&format!("\x1b[{}{forward}", parameter(c, delta)?));
        }
    }
    readline_command(c, this, &data)
}

/// `readline.commit()`: write the queued commands; a promise of the write.
pub(crate) fn readline_commit(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let stream = get_named(c, this, "#stream")?;
    let todo = get_named(c, this, "#todo")?;
    let commands = c.array_like_values_rooted(todo)?;
    let mut joined = Vec::new();
    for command in commands {
        joined.extend(units_of(c, command)?);
    }
    let empty = c.array_rooted(&[])?;
    put_slot_named(c, this, "#todo", empty)?;
    let promise = c.pending_promise_rooted()?;
    let data = c.string_units_rooted(&joined);
    let resolve = closure(c, "readlineQuestionResolve", "", &[promise])?;
    call_method(c, stream, "write", &[data, resolve])?;
    Ok(promise)
}

/// `readline.rollback()`: drop the queued commands.
pub(crate) fn readline_rollback(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let empty = c.array_rooted(&[])?;
    put_slot_named(c, this, "#todo", empty)?;
    Ok(this)
}

// ---------------------------------------------------------------------------------------
// The modules
// ---------------------------------------------------------------------------------------

fn method(c: &mut Context<'_>, target: RootId, key: RootId, operation: &str, name: &str, enumerable: bool) -> Result<(), RootedError> {
    let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
    c.define_data_property_rooted(target, key, function, true, enumerable, true)?;
    Ok(())
}

fn named_method(c: &mut Context<'_>, target: RootId, name: &str, operation: &str, enumerable: bool) -> Result<(), RootedError> {
    let key = text(c, name);
    method(c, target, key, operation, name, enumerable)
}

fn slot_method(c: &mut Context<'_>, target: RootId, which: Slot, operation: &str) -> Result<(), RootedError> {
    let key = slot(c, which)?;
    let name = format!("[{}]", which.description());
    method(c, target, key, operation, &name, false)
}

fn accessor(c: &mut Context<'_>, target: RootId, key: RootId, getter: RootId, setter: Option<RootId>) -> Result<(), RootedError> {
    let undefined = c.undefined();
    let fields = DescriptorFields {
        get: Some(getter),
        set: Some(setter.unwrap_or(undefined)),
        enumerable: Some(false),
        configurable: Some(true),
        ..DescriptorFields::default()
    };
    c.define_property_fields_rooted(target, key, fields)?;
    Ok(())
}

/// `internal/readline/interface`'s `Interface`, over `InterfaceConstructor`.
fn base_interface(c: &mut Context<'_>) -> Outcome {
    let cache = "\0readline:Interface";
    let shared = c.host_mut().shared_state();
    if let Some(class) = shared.borrow().builtin_cache.get(cache).copied() {
        return Ok(class);
    }
    super::stream_shared_vm::module(c)?;
    let emitter = super::stream_shared_vm::intrinsics(c)?.class(Class::Emitter);
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation!("readlineInterfaceConstructor"), "InterfaceConstructor")?;
    inherit(c, constructor, emitter)?;
    let class = c.host_constructor_named(crate::host::shared_vm::operation!("readlineInterfaceConstructor"), "Interface")?;
    let prototype = inherit(c, class, constructor)?;
    let getter = c.host_function_named(crate::host::shared_vm::operation!("readlineColumns"), "get columns")?;
    let key = text(c, "columns");
    accessor(c, prototype, key, getter, None)?;
    named_method(c, prototype, "setPrompt", "readlineSetPrompt", false)?;
    named_method(c, prototype, "getPrompt", "readlineGetPrompt", false)?;
    named_method(c, prototype, "prompt", "readlinePrompt", false)?;
    for (which, operation) in METHODS {
        slot_method(c, prototype, which, operation)?;
    }
    named_method(c, prototype, "close", "readlineClose", false)?;
    named_method(c, prototype, "pause", "readlinePause", false)?;
    named_method(c, prototype, "resume", "readlineResume", false)?;
    named_method(c, prototype, "write", "readlineWrite", false)?;
    named_method(c, prototype, "clearLine", "readlineClearCurrentLine", false)?;
    named_method(c, prototype, "getCursorPos", "readlineGetCursorPos", false)?;
    for (which, operation) in LATE_METHODS {
        slot_method(c, prototype, which, operation)?;
    }
    let iterator = super::stream_shared_vm::intrinsics(c)?.slot(super::stream_shared_vm::Slot::AsyncIterator);
    method(c, prototype, iterator, "readlineAsyncIterator", "[Symbol.asyncIterator]", false)?;
    let dispose = super::stream_shared_vm::intrinsics(c)?.slot(super::stream_shared_vm::Slot::Dispose);
    method(c, prototype, dispose, "readlineDispose", "[Symbol.dispose]", true)?;
    let retained = c.retain(class)?;
    shared.borrow_mut().builtin_cache.insert(cache.to_owned(), retained);
    Ok(class)
}

/// The legacy `readline.Interface`: the internal methods also as `_`-prefixed members.
fn legacy_interface(c: &mut Context<'_>, base: RootId) -> Outcome {
    let data = c.array_rooted(&[])?;
    let class = c.host_constructor_named_with_data(crate::host::shared_vm::operation!("readlineLegacyConstructor"), "Interface", data)?;
    let prototype = inherit(c, class, base)?;
    let question = c.host_function_named(crate::host::shared_vm::operation!("readlineQuestion"), "question")?;
    let key = text(c, "question");
    c.define_data_property_rooted(prototype, key, question, true, true, true)?;
    let custom = promisify_custom(c)?;
    let promised = c.host_function_named(crate::host::shared_vm::operation!("readlineQuestionPromise"), "question")?;
    c.define_data_property_rooted(question, custom, promised, true, true, true)?;
    let base_prototype = get(c, base, "prototype")?;
    for (which, _) in METHODS.iter().chain(LATE_METHODS.iter()) {
        if matches!(which, Slot::Question | Slot::QuestionCancel) {
            continue;
        }
        let name = text(c, which.description());
        let getter = c.host_function_named_with_data(crate::host::shared_vm::operation!("readlineAliasGet"), "get", name)?;
        let key = slot(c, *which)?;
        accessor(c, prototype, key, getter, None)?;
    }
    for which in LEGACY_FIELDS {
        let key = slot(c, which)?;
        let getter = c.host_function_named_with_data(crate::host::shared_vm::operation!("readlineFieldGet"), "get", key)?;
        let setter = c.host_function_named_with_data(crate::host::shared_vm::operation!("readlineFieldSet"), "set", key)?;
        let name = text(c, which.description());
        accessor(c, prototype, name, getter, Some(setter))?;
    }
    for (which, _) in METHODS.iter().chain(LATE_METHODS.iter()) {
        if matches!(which, Slot::Question | Slot::QuestionCancel) {
            continue;
        }
        let key = slot(c, *which)?;
        let original = c.get_property_rooted(base_prototype, key)?;
        let name = text(c, which.description());
        c.define_data_property_rooted(prototype, name, original, true, true, true)?;
        if *which == Slot::GetDisplayPos {
            let cursor = get(c, base_prototype, "getCursorPos")?;
            let name = text(c, "_getCursorPos");
            c.define_data_property_rooted(prototype, name, cursor, true, true, true)?;
        }
    }
    let index = c.number(0.0);
    c.set_property_rooted(data, index, class, data)?;
    Ok(class)
}

fn promisify_custom(c: &mut Context<'_>) -> Outcome {
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let description = text(c, "nodejs.util.promisify.custom");
    call_method(c, symbol, "for", &[description])
}

fn define_function(c: &mut Context<'_>, target: RootId, name: &str, operation: &str, data: Option<RootId>) -> Result<(), RootedError> {
    let function = match data {
        Some(data) => c.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, data)?,
        None => c.host_function_named(crate::host::shared_vm::operation(operation), name)?,
    };
    let key = text(c, name);
    c.define_data_property_rooted(target, key, function, true, true, true)?;
    Ok(())
}

/// `readline`.
pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    let shared = c.host_mut().shared_state();
    if let Some(module) = shared.borrow().builtin_cache.get(MODULE_CACHE).copied() {
        return Ok(module);
    }
    let base = base_interface(c)?;
    let interface = legacy_interface(c, base)?;
    let module = c.object_rooted()?;
    let key = text(c, "Interface");
    c.define_data_property_rooted(module, key, interface, true, true, true)?;
    define_function(c, module, "clearLine", "readlineClearLine", None)?;
    define_function(c, module, "clearScreenDown", "readlineClearScreenDown", None)?;
    define_function(c, module, "createInterface", "readlineCreateInterface", Some(interface))?;
    define_function(c, module, "cursorTo", "readlineCursorTo", None)?;
    define_function(c, module, "emitKeypressEvents", "readlineEmitKeypressEvents", None)?;
    define_function(c, module, "moveCursor", "readlineMoveCursorCommand", None)?;
    let promises = promises_module(c)?;
    let key = text(c, "promises");
    c.define_data_property_rooted(module, key, promises, true, true, true)?;
    let retained = c.retain(module)?;
    shared.borrow_mut().builtin_cache.insert(MODULE_CACHE.to_owned(), retained);
    Ok(module)
}

/// `readline/promises`.
pub(crate) fn promises_module(c: &mut Context<'_>) -> Outcome {
    let shared = c.host_mut().shared_state();
    if let Some(module) = shared.borrow().builtin_cache.get(PROMISES_CACHE).copied() {
        return Ok(module);
    }
    let base = base_interface(c)?;
    let interface = c.host_constructor_named(crate::host::shared_vm::operation!("readlinePromisesConstructor"), "Interface")?;
    let prototype = inherit(c, interface, base)?;
    named_method(c, prototype, "question", "readlineQuestionPromise", false)?;
    let readline = c.host_constructor_named(crate::host::shared_vm::operation!("readlineReadline"), "Readline")?;
    let readline_prototype = get(c, readline, "prototype")?;
    for (name, operation) in [
        ("cursorTo", "readlineReadlineCursorTo"),
        ("moveCursor", "readlineReadlineMoveCursor"),
        ("clearLine", "readlineReadlineClearLine"),
        ("clearScreenDown", "readlineReadlineClearScreenDown"),
        ("commit", "readlineReadlineCommit"),
        ("rollback", "readlineReadlineRollback"),
    ] {
        named_method(c, readline_prototype, name, operation, false)?;
    }
    let module = c.object_rooted()?;
    let key = text(c, "Interface");
    c.define_data_property_rooted(module, key, interface, true, true, true)?;
    let key = text(c, "Readline");
    c.define_data_property_rooted(module, key, readline, true, true, true)?;
    define_function(c, module, "createInterface", "readlineCreateInterface", Some(interface))?;
    let retained = c.retain(module)?;
    shared.borrow_mut().builtin_cache.insert(PROMISES_CACHE.to_owned(), retained);
    Ok(module)
}

#[cfg(test)]
mod tests {
    use super::{decode, Decoded};

    fn name(input: &str) -> (Option<String>, bool, bool, bool, usize) {
        let characters: Vec<char> = input.chars().collect();
        match decode(&characters) {
            Decoded::Key { key, length, .. } => (key.name, key.ctrl, key.meta, key.shift, length),
            Decoded::Incomplete => (None, false, false, false, 0),
        }
    }

    #[test]
    fn decodes_terminal_keys() {
        assert_eq!(name("a"), (Some("a".into()), false, false, false, 1));
        assert_eq!(name("A"), (Some("a".into()), false, false, true, 1));
        assert_eq!(name("\x03"), (Some("c".into()), true, false, false, 1));
        assert_eq!(name("\r"), (Some("return".into()), false, false, false, 1));
        assert_eq!(name("\x1b[A"), (Some("up".into()), false, false, false, 3));
        assert_eq!(name("\x1b[1;5C"), (Some("right".into()), true, false, false, 6));
        assert_eq!(name("\x1b[3~"), (Some("delete".into()), false, false, false, 4));
        assert_eq!(name("\x1bOH"), (Some("home".into()), false, false, false, 3));
        assert_eq!(name("\x1bb"), (Some("b".into()), false, true, false, 2));
        assert_eq!(name("."), (None, false, false, false, 1));
        assert!(matches!(decode(&['\x1b', '[']), Decoded::Incomplete));
    }
}
