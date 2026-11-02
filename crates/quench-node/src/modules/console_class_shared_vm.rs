//! The `Console` class (`lib/internal/console/constructor.js`) and the global console's
//! members beyond its writers.
//!
//! A console's state is one private slot: its group indentation, counters and timers. Its
//! streams are its `_stdout` and `_stderr`; the global console writes to the process's.
use super::console_format_shared_vm::{self as format, Options};
use super::console_shared_vm::Stream;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

/// The cache key of the private symbol naming a console's slot.
const SLOT_KEY: &str = "\0node/console/slot";
/// The slot's fields.
const INDENT: &str = "indent";
const INDENT_WIDTH: &str = "indentWidth";
const COUNTS: &str = "counts";
const TIMERS: &str = "timers";
const IGNORE_ERRORS: &str = "ignoreErrors";
/// `kMaxGroupIndentation`, and the default group indentation.
const MAX_GROUP_INDENTATION: f64 = 1000.0;
const DEFAULT_GROUP_INDENTATION: f64 = 2.0;
const DEFAULT_LABEL: &str = "default";
const TRACE_NAME: &str = "Trace";
/// `console.clear`'s `cursorTo(0, 0)` and `clearScreenDown`.
const CLEAR_SCREEN: &str = "\x1b[1;1H\x1b[0J";
const TABLE_INDEX: &str = "(index)";
const TABLE_VALUES: &str = "Values";
/// `console.table` inspects cell values with this array limit.
const TABLE_MAX_ARRAY_LENGTH: usize = 3;
/// A cell object with more keys than this is written at depth -1.
const TABLE_SHALLOW_KEYS: usize = 2;
/// Milliseconds per second, minute and hour (`formatTime`).
const SECOND_MS: f64 = 1000.0;
const MINUTE_MS: f64 = 60.0 * SECOND_MS;
const HOUR_MS: f64 = 60.0 * MINUTE_MS;
/// `formatTime`'s fraction digits.
const TIME_DIGITS: usize = 3;

/// A console method (the function data of its host function, by position in `ALL`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Method {
    Log,
    Warn,
    Dir,
    Time,
    TimeEnd,
    TimeLog,
    Trace,
    Assert,
    Clear,
    Count,
    CountReset,
    Group,
    GroupEnd,
    Table,
}

impl Method {
    const ALL: [Method; 14] = [
        Method::Log,
        Method::Warn,
        Method::Dir,
        Method::Time,
        Method::TimeEnd,
        Method::TimeLog,
        Method::Trace,
        Method::Assert,
        Method::Clear,
        Method::Count,
        Method::CountReset,
        Method::Group,
        Method::GroupEnd,
        Method::Table,
    ];

    fn code(self) -> f64 {
        Self::ALL.iter().position(|method| *method == self).unwrap_or_default() as f64
    }
}

/// `Console.prototype`'s keys in Node's order: (key, function name, method, length). The
/// `dirxml` and `groupCollapsed` aliases keep their target's name.
const PROTOTYPE_KEYS: [(&str, &str, Method, f64); 19] = [
    ("log", "log", Method::Log, 0.0),
    ("info", "info", Method::Log, 0.0),
    ("debug", "debug", Method::Log, 0.0),
    ("warn", "warn", Method::Warn, 0.0),
    ("error", "error", Method::Warn, 0.0),
    ("dir", "dir", Method::Dir, 2.0),
    ("time", "time", Method::Time, 0.0),
    ("timeEnd", "timeEnd", Method::TimeEnd, 0.0),
    ("timeLog", "timeLog", Method::TimeLog, 0.0),
    ("trace", "trace", Method::Trace, 0.0),
    ("assert", "assert", Method::Assert, 1.0),
    ("clear", "clear", Method::Clear, 0.0),
    ("count", "count", Method::Count, 0.0),
    ("countReset", "countReset", Method::CountReset, 0.0),
    ("group", "group", Method::Group, 0.0),
    ("groupEnd", "groupEnd", Method::GroupEnd, 0.0),
    ("table", "table", Method::Table, 2.0),
    ("dirxml", "log", Method::Log, 0.0),
    ("groupCollapsed", "group", Method::Group, 0.0),
];

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    c.assign_named_property_rooted(object, name, value)
}

fn hidden(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    c.define_named_data_property_rooted(object, name, value, true, false, true)?;
    Ok(())
}

fn argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}

fn slot_key(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let shared = c.host_mut().shared_state();
    if let Some(key) = shared.borrow().builtin_cache.get(SLOT_KEY).copied() {
        return Ok(key);
    }
    let key = c.private_symbol_rooted(Some("kConsoleState"));
    let retained = c.retain(key)?;
    shared.borrow_mut().builtin_cache.insert(SLOT_KEY.into(), retained);
    Ok(key)
}

/// Gives `console` a fresh state.
fn attach_state(c: &mut Context<'_>, console: RootId, ignore_errors: bool, indent_width: f64) -> Result<RootId, RootedError> {
    let state = c.null_object_rooted()?;
    let empty = c.string_rooted("");
    put(c, state, INDENT, empty)?;
    let width = c.number(indent_width);
    put(c, state, INDENT_WIDTH, width)?;
    for name in [COUNTS, TIMERS] {
        let table = c.null_object_rooted()?;
        put(c, state, name, table)?;
    }
    let ignore = c.boolean(ignore_errors);
    put(c, state, IGNORE_ERRORS, ignore)?;
    let key = slot_key(c)?;
    c.define_data_property_rooted(console, key, state, true, false, true)?;
    Ok(state)
}

/// `console`'s state; the global console's is made on first use.
fn state(c: &mut Context<'_>, console: RootId) -> Result<RootId, RootedError> {
    let key = slot_key(c)?;
    if c.is_object_rooted(console)? {
        if let Some(state) = c.own_property_fields_rooted(console, key)?.and_then(|fields| fields.value) {
            return Ok(state);
        }
    }
    let global = super::console_shared_vm::facade(c)?;
    if c.strict_equal_rooted(console, global)? {
        return attach_state(c, console, true, DEFAULT_GROUP_INDENTATION);
    }
    let error = c.type_error_rooted("Illegal invocation")?;
    Err(c.throw(error))
}

/// The group indentation `console` writes lines with.
pub(super) fn indentation(c: &mut Context<'_>, console: RootId) -> Result<String, RootedError> {
    let state = state(c, console)?;
    let indent = get(c, state, INDENT)?;
    c.to_string(indent)
}

/// `kWriteToConsole`: `text` indented by the group indentation, written with a newline to
/// the console's stream; write errors are ignored unless the console was told otherwise.
pub(super) fn write_text(c: &mut Context<'_>, console: RootId, stream: Stream, text: &str) -> Result<RootId, RootedError> {
    let indent = indentation(c, console)?;
    let mut line = if indent.is_empty() { text.to_owned() } else { format!("{indent}{}", text.replace('\n', &format!("\n{indent}"))) };
    line.push('\n');
    let global = super::console_shared_vm::facade(c)?;
    if c.strict_equal_rooted(console, global)? {
        return super::console_shared_vm::write_stream(c, stream, &line);
    }
    let target = get(c, console, if matches!(stream, Stream::Stdout) { "_stdout" } else { "_stderr" })?;
    let text = c.string_rooted(&line);
    let state = state(c, console)?;
    let ignore = get(c, state, IGNORE_ERRORS)?;
    if !c.truthy_rooted(ignore)? {
        let write = get(c, target, "write")?;
        return c.call_rooted(write, target, &[text]);
    }
    // Node guards the write with a no-op `error` listener while the stream has none, and
    // ignores what the write throws; removing the guard can still throw.
    let noop = c.host_function_named(crate::host::shared_vm::operation("consoleNoop"), "noop")?;
    let error_event = c.string_rooted("error");
    let _ = guarded_write(c, target, text, noop, error_event);
    let remove = get(c, target, "removeListener")?;
    c.call_rooted(remove, target, &[error_event, noop])?;
    Ok(c.undefined())
}

fn guarded_write(c: &mut Context<'_>, target: RootId, text: RootId, noop: RootId, error_event: RootId) -> Result<(), RootedError> {
    let count = get(c, target, "listenerCount")?;
    let count = c.call_rooted(count, target, &[error_event])?;
    if c.to_number(count)? == 0.0 {
        let once = get(c, target, "once")?;
        c.call_rooted(once, target, &[error_event, noop])?;
    }
    let write = get(c, target, "write")?;
    c.call_rooted(write, target, &[text])?;
    Ok(())
}

/// The console a method acts on: its bound console, else its receiver.
fn console_of(c: &mut Context<'_>, receiver: RootId) -> Result<(Method, RootId), RootedError> {
    let data = c.host_function_data()?;
    let code = get(c, data, "method")?;
    let method = Method::ALL[c.to_number(code)? as usize];
    let bound = get(c, data, "console")?;
    let console = if c.rooted_value(bound).is_some_and(|value| value.is_undefined()) { receiver } else { bound };
    Ok((method, console))
}

/// A method's host function, bound to `console` when given.
fn method_function(c: &mut Context<'_>, name: &str, method: Method, console: Option<RootId>) -> Result<RootId, RootedError> {
    let data = c.null_object_rooted()?;
    let code = c.number(method.code());
    put(c, data, "method", code)?;
    if let Some(console) = console {
        put(c, data, "console", console)?;
    }
    c.host_function_named_with_data(crate::host::shared_vm::operation("consoleMethod"), name, data)
}

/// Every console method: the bound console's, else the receiver's.
pub(crate) fn invoke(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (method, console) = console_of(c, receiver)?;
    run(c, console, method, args)
}

fn label(c: &mut Context<'_>, args: &[RootId]) -> Result<String, RootedError> {
    match args.first() {
        Some(&label) if !c.rooted_value(label).is_some_and(|value| value.is_undefined()) => c.to_string(label),
        _ => Ok(DEFAULT_LABEL.to_owned()),
    }
}

fn warn_process(c: &mut Context<'_>, message: &str) -> Result<(), RootedError> {
    let warning = c.string_rooted(message);
    super::event_target_shared_vm::emit_warning(c, warning)
}

fn now(c: &mut Context<'_>) -> Result<f64, RootedError> {
    let undefined = c.undefined();
    let now = super::perf_hooks::now(c, undefined, &[])?;
    c.to_number(now)
}

/// `formatTime(ms)`.
fn format_time(milliseconds: f64) -> String {
    let mut ms = milliseconds;
    let (mut hours, mut minutes, mut seconds) = (0.0, 0.0, 0.0);
    if ms >= SECOND_MS {
        if ms >= MINUTE_MS {
            if ms >= HOUR_MS {
                hours = (ms / HOUR_MS).floor();
                ms %= HOUR_MS;
            }
            minutes = (ms / MINUTE_MS).floor();
            ms %= MINUTE_MS;
        }
        seconds = ms / SECOND_MS;
    }
    if hours != 0.0 || minutes != 0.0 {
        let fixed = format!("{seconds:.TIME_DIGITS$}");
        let (whole, fraction) = fixed.split_once('.').unwrap_or((&fixed, ""));
        let lead = if hours != 0.0 { format!("{hours}:{minutes:02}") } else { format!("{minutes}") };
        let unit = if hours != 0.0 { "h:m" } else { "" };
        return format!("{lead}:{whole:0>2}.{fraction} ({unit}m:ss.mmm)");
    }
    if seconds != 0.0 {
        return format!("{seconds:.TIME_DIGITS$}s");
    }
    let rounded: f64 = format!("{ms:.TIME_DIGITS$}").parse().unwrap_or(ms);
    format!("{rounded}ms")
}

fn run(c: &mut Context<'_>, console: RootId, method: Method, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    match method {
        Method::Log | Method::Warn => {
            let text = format::format(c, args)?;
            let stream = if method == Method::Log { Stream::Stdout } else { Stream::Stderr };
            write_text(c, console, stream, &text)?;
        }
        Method::Dir => {
            let value = argument(c, args, 0);
            let mut options = Options { custom_inspect: false, ..Options::default() };
            let given = argument(c, args, 1);
            if c.is_object_rooted(given)? {
                let depth = get(c, given, "depth")?;
                match c.rooted_value(depth) {
                    Some(depth) if depth.is_null() => options.depth = None,
                    Some(depth) => {
                        if let Some(depth) = depth.as_number() {
                            options.depth = Some(depth);
                        }
                    }
                    None => {}
                }
                let custom = get(c, given, "customInspect")?;
                if let Some(custom) = c.rooted_value(custom).and_then(|value| value.as_bool()) {
                    options.custom_inspect = custom;
                }
            }
            let text = format::inspect(c, value, options)?;
            write_text(c, console, Stream::Stdout, &text)?;
        }
        Method::Time => {
            let label = label(c, args)?;
            let timers = table(c, console, TIMERS)?;
            if !absent(c, timers, &label)? {
                warn_process(c, &format!("Label '{label}' already exists for console.time()"))?;
                return Ok(undefined);
            }
            let start = now(c)?;
            let start = c.number(start);
            put(c, timers, &label, start)?;
        }
        Method::TimeEnd | Method::TimeLog => {
            let label = label(c, args)?;
            let timers = table(c, console, TIMERS)?;
            if absent(c, timers, &label)? {
                let name = if method == Method::TimeEnd { "timeEnd" } else { "timeLog" };
                warn_process(c, &format!("No such label '{label}' for console.{name}()"))?;
                return Ok(undefined);
            }
            let start = get(c, timers, &label)?;
            let elapsed = now(c)? - c.to_number(start)?;
            let mut line = vec![c.string_rooted("%s: %s"), c.string_rooted(&label), c.string_rooted(&format_time(elapsed))];
            if method == Method::TimeEnd {
                let key = c.string_rooted(&label);
                c.delete_property_rooted(timers, key)?;
            } else {
                line.extend_from_slice(args.get(1..).unwrap_or_default());
            }
            run(c, console, Method::Log, &line)?;
        }
        Method::Trace => {
            let message = format::format(c, args)?;
            let trace = c.object_rooted()?;
            let name = c.string_rooted(TRACE_NAME);
            put(c, trace, "name", name)?;
            let message = c.string_rooted(&message);
            put(c, trace, "message", message)?;
            let global = c.global_root()?;
            let error = get(c, global, "Error")?;
            let capture = get(c, error, "captureStackTrace")?;
            c.call_rooted(capture, error, &[trace])?;
            let stack = get(c, trace, "stack")?;
            run(c, console, Method::Warn, &[stack])?;
        }
        Method::Assert => {
            let expression = argument(c, args, 0);
            if !c.truthy_rooted(expression)? {
                let mut rest = args.get(1..).unwrap_or_default().to_vec();
                let head = match rest.first() {
                    Some(&first) => {
                        let first = c.to_string(first)?;
                        format!("Assertion failed: {first}")
                    }
                    None => "Assertion failed".to_owned(),
                };
                let head = c.string_rooted(&head);
                match rest.first_mut() {
                    Some(first) => *first = head,
                    None => rest.push(head),
                }
                run(c, console, Method::Warn, &rest)?;
            }
        }
        Method::Clear => {
            let global = super::console_shared_vm::facade(c)?;
            let stream = if c.strict_equal_rooted(console, global)? {
                let process = c
                    .host_mut()
                    .shared_state()
                    .borrow()
                    .process_module
                    .ok_or_else(|| RootedError::host("process is not initialized"))?;
                get(c, process, "stdout")?
            } else {
                get(c, console, "_stdout")?
            };
            let terminal = get(c, stream, "isTTY")?;
            if c.truthy_rooted(terminal)? {
                let state = state(c, console)?;
                let empty = c.string_rooted("");
                put(c, state, INDENT, empty)?;
                let write = get(c, stream, "write")?;
                let text = c.string_rooted(CLEAR_SCREEN);
                c.call_rooted(write, stream, &[text])?;
            }
        }
        Method::Count => {
            let label = label(c, args)?;
            let counts = table(c, console, COUNTS)?;
            let count = get(c, counts, &label)?;
            let count = if c.rooted_value(count).is_some_and(|value| value.is_undefined()) { 1.0 } else { c.to_number(count)? + 1.0 };
            let value = c.number(count);
            put(c, counts, &label, value)?;
            let line = c.string_rooted(&format!("{label}: {count}"));
            run(c, console, Method::Log, &[line])?;
        }
        Method::CountReset => {
            let label = label(c, args)?;
            let counts = table(c, console, COUNTS)?;
            if absent(c, counts, &label)? {
                warn_process(c, &format!("Count for '{label}' does not exist"))?;
                return Ok(undefined);
            }
            let key = c.string_rooted(&label);
            c.delete_property_rooted(counts, key)?;
        }
        Method::Group => {
            if !args.is_empty() {
                run(c, console, Method::Log, args)?;
            }
            let state = state(c, console)?;
            let indent = get(c, state, INDENT)?;
            let indent = c.to_string(indent)?;
            let width = get(c, state, INDENT_WIDTH)?;
            let width = c.to_number(width)? as usize;
            let deeper = c.string_rooted(&format!("{indent}{}", " ".repeat(width)));
            put(c, state, INDENT, deeper)?;
        }
        Method::GroupEnd => {
            let state = state(c, console)?;
            let indent = get(c, state, INDENT)?;
            let indent = c.to_string(indent)?;
            let width = get(c, state, INDENT_WIDTH)?;
            let width = c.to_number(width)? as usize;
            let shallower = c.string_rooted(&indent[..indent.len().saturating_sub(width)]);
            put(c, state, INDENT, shallower)?;
        }
        Method::Table => {
            let data = argument(c, args, 0);
            let properties = argument(c, args, 1);
            if !c.rooted_value(properties).is_some_and(|value| value.is_undefined()) && !c.is_array_rooted(properties)? {
                return Err(super::util_shared_vm::argument_type_error(c, properties, "properties", "Array")?);
            }
            if !c.is_object_rooted(data)? {
                return run(c, console, Method::Log, &[data]);
            }
            let rendered = table_text(c, data, properties)?;
            let line = c.string_rooted(&rendered);
            run(c, console, Method::Log, &[line])?;
        }
    }
    Ok(undefined)
}

fn table(c: &mut Context<'_>, console: RootId, name: &str) -> Result<RootId, RootedError> {
    let state = state(c, console)?;
    get(c, state, name)
}

/// `console.table`'s cell: `inspect` on one line, three array entries, shallow for an object
/// of more than two keys.
fn cell(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let shallow = c.is_object_rooted(value)?
        && !c.is_array_rooted(value)?
        && c.enumerable_own_keys_rooted(value, false, true)?.len() > TABLE_SHALLOW_KEYS;
    let options = Options {
        depth: Some(if shallow { -1.0 } else { 0.0 }),
        max_array_length: TABLE_MAX_ARRAY_LENGTH,
        layout: format::Layout::SingleLine,
        ..Options::default()
    };
    format::inspect(c, value, options)
}

/// The rows and columns of `console.table(data, properties)` for an array or object.
fn table_text(c: &mut Context<'_>, data: RootId, properties: RootId) -> Result<String, RootedError> {
    let filter = if c.rooted_value(properties).is_some_and(|value| value.is_undefined()) {
        None
    } else {
        Some(c.array_like_values_rooted(properties)?)
    };
    let index_keys = c.enumerable_own_keys_rooted(data, false, true)?;
    let mut columns: Vec<(String, Vec<Option<String>>)> = Vec::new();
    let mut values = vec![None; index_keys.len()];
    let mut has_primitives = false;
    let mut index_column = Vec::with_capacity(index_keys.len());
    for (row, &key) in index_keys.iter().enumerate() {
        index_column.push(Some(c.to_string(key)?));
        let item = c.get_property_rooted(data, key)?;
        let primitive = !c.is_object_rooted(item)? && !c.is_callable_rooted(item)?;
        if filter.is_none() && primitive {
            has_primitives = true;
            values[row] = Some(cell(c, item)?);
            continue;
        }
        let keys = match &filter {
            Some(keys) => keys.clone(),
            None => c.enumerable_own_keys_rooted(item, false, true)?,
        };
        for key in keys {
            let name = c.to_string(key)?;
            let position = match columns.iter().position(|(column, _)| *column == name) {
                Some(position) => position,
                None => {
                    columns.push((name.clone(), vec![None; index_keys.len()]));
                    columns.len() - 1
                }
            };
            let own = !primitive && c.own_property_fields_rooted(item, key)?.is_some();
            let text = if own {
                let value = c.get_property_rooted(item, key)?;
                cell(c, value)?
            } else {
                String::new()
            };
            columns[position].1[row] = Some(text);
        }
    }
    let mut head = vec![TABLE_INDEX.to_owned()];
    let mut body = vec![index_column];
    for (name, column) in columns {
        head.push(name);
        body.push(column);
    }
    if has_primitives {
        head.push(TABLE_VALUES.to_owned());
        body.push(values);
    }
    Ok(render_table(&head, &body))
}

/// `getStringWidth` of a cell, counting characters.
fn width(text: &str) -> usize {
    text.chars().count()
}

/// Node's `cli_table` box drawing.
fn render_table(head: &[String], columns: &[Vec<Option<String>>]) -> String {
    let rows = columns.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = head.iter().map(|name| width(name)).collect::<Vec<_>>();
    for (index, column) in columns.iter().enumerate() {
        for value in column.iter().flatten() {
            widths[index] = widths[index].max(width(value));
        }
    }
    let render_row = |cells: &[&str]| {
        let mut out = String::from("│ ");
        for (index, cell) in cells.iter().enumerate() {
            out += cell;
            out += &" ".repeat(widths[index] - width(cell));
            if index != cells.len() - 1 {
                out += " │ ";
            }
        }
        out + " │"
    };
    let divider = widths.iter().map(|width| "─".repeat(width + 2)).collect::<Vec<_>>();
    let head_cells = head.iter().map(String::as_str).collect::<Vec<_>>();
    let mut result = format!("┌{}┐\n{}\n├{}┤\n", divider.join("┬"), render_row(&head_cells), divider.join("┼"));
    for row in 0..rows {
        let cells = columns.iter().map(|column| column.get(row).cloned().flatten().unwrap_or_default()).collect::<Vec<_>>();
        let cells = cells.iter().map(String::as_str).collect::<Vec<_>>();
        result += &render_row(&cells);
        result.push('\n');
    }
    result + &format!("└{}┘", divider.join("┴"))
}

/// Defines `console`'s members after its writers: the methods bound to it, the streams,
/// the error handlers and `Console`, in `CONSOLE_KEYS` order.
pub(super) fn install_global_members(c: &mut Context<'_>, console: RootId, keys: &[(&str, bool)]) -> Result<(), RootedError> {
    let process = c
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process is not initialized"))?;
    for &(key, enumerable) in keys {
        let value = match key {
            "_stdout" => get(c, process, "stdout")?,
            "_stderr" => get(c, process, "stderr")?,
            "_stdoutErrorHandler" | "_stderrErrorHandler" => {
                c.host_function_named(crate::host::shared_vm::operation("consoleNoop"), "")?
            }
            "_ignoreErrors" => c.boolean(true),
            "_times" => {
                let global = c.global_root()?;
                let map = get(c, global, "Map")?;
                c.construct_rooted(map, map, &[])?
            }
            "Console" => class(c)?,
            "profile" | "profileEnd" | "timeStamp" => noop_named(c, key)?,
            "context" => c.host_function_named(crate::host::shared_vm::operation("consoleContext"), key)?,
            "createTask" => c.host_function_named(crate::host::shared_vm::operation("consoleCreateTask"), key)?,
            name => {
                let method = PROTOTYPE_KEYS
                    .iter()
                    .find(|(candidate, _, _, _)| *candidate == name)
                    .map(|&(_, _, method, _)| method)
                    .ok_or_else(|| RootedError::host(format!("unknown console member {name}")))?;
                method_function(c, name, method, Some(console))?
            }
        };
        let key = c.string_rooted(key);
        c.define_data_property_rooted(console, key, value, true, enumerable, true)?;
    }
    Ok(())
}

/// A function that does nothing: a stream write's error handler, and the inspector's console
/// methods without an inspector session.
pub(crate) fn noop(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(c.undefined())
}

fn noop_named(c: &mut Context<'_>, name: &str) -> Result<RootId, RootedError> {
    c.host_function_named(crate::host::shared_vm::operation("consoleNoop"), name)
}

/// The members of an inspector console context, which write only to an inspector session.
const CONTEXT_MEMBERS: [&str; 22] = [
    "dir", "dirXml", "table", "groupEnd", "clear", "count", "countReset", "profile", "profileEnd", "timeLog", "debug",
    "error", "info", "log", "warn", "trace", "group", "groupCollapsed", "assert", "time", "timeEnd", "timeStamp",
];

/// `console.context(name)`: a console of no-op methods.
pub(crate) fn context(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let context = c.object_rooted()?;
    for name in CONTEXT_MEMBERS {
        let method = noop_named(c, name)?;
        put(c, context, name, method)?;
    }
    Ok(context)
}

/// `console.createTask(name)`: a task whose `run(fn)` calls `fn`.
pub(crate) fn create_task(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let task = c.object_rooted()?;
    let run = c.host_function_named(crate::host::shared_vm::operation("consoleTaskRun"), "run")?;
    put(c, task, "run", run)?;
    Ok(task)
}

pub(crate) fn task_run(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let function = argument(c, args, 0);
    let undefined = c.undefined();
    c.call_rooted(function, undefined, &[])
}

/// The `Console` class, built once.
pub(crate) fn class(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    const CLASS_KEY: &str = "\0node/console/class";
    let shared = c.host_mut().shared_state();
    if let Some(class) = shared.borrow().builtin_cache.get(CLASS_KEY).copied() {
        return Ok(class);
    }
    let class = c.host_constructor_named(crate::host::shared_vm::operation("consoleConstructor"), "Console")?;
    let prototype = get(c, class, "prototype")?;
    for &(key, name, method, length) in &PROTOTYPE_KEYS {
        let function = method_function(c, name, method, None)?;
        let length = c.number(length);
        c.define_named_data_property_rooted(function, "length", length, false, false, true)?;
        c.define_named_data_property_rooted(prototype, key, function, true, true, true)?;
    }
    let retained = c.retain(class)?;
    shared.borrow_mut().builtin_cache.insert(CLASS_KEY.into(), retained);
    Ok(class)
}

fn writable(c: &mut Context<'_>, stream: RootId) -> Result<bool, RootedError> {
    if !c.is_object_rooted(stream)? {
        return Ok(false);
    }
    let write = get(c, stream, "write")?;
    c.is_callable_rooted(write)
}

fn writable_stream_error(c: &mut Context<'_>, name: &str) -> RootedError {
    match c.type_error_rooted(&format!("Console expects a writable stream instance for {name}")) {
        Ok(error) => match super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_CONSOLE_WRITABLE_STREAM") {
            Ok(()) => c.throw(error),
            Err(error) => error,
        },
        Err(error) => error,
    }
}

/// `new Console(stdout, stderr, ignoreErrors)` or `new Console({ stdout, stderr,
/// ignoreErrors, groupIndentation })`; without `new`, constructs.
pub(crate) fn constructor(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if c.new_target_rooted().is_none() {
        let class = class(c)?;
        return c.construct_rooted(class, class, args);
    }
    let options = argument(c, args, 0);
    let by_position = !c.truthy_rooted(options)? || writable(c, options)?;
    let undefined = c.undefined();
    let (stdout, stderr, ignore, indentation) = if by_position {
        (options, argument(c, args, 1), argument(c, args, 2), undefined)
    } else {
        let stdout = get(c, options, "stdout")?;
        let stderr = get(c, options, "stderr")?;
        let ignore = get(c, options, "ignoreErrors")?;
        let indentation = get(c, options, "groupIndentation")?;
        (stdout, stderr, ignore, indentation)
    };
    let stderr = if c.rooted_value(stderr).is_some_and(|value| value.is_undefined()) { stdout } else { stderr };
    if !writable(c, stdout)? {
        return Err(writable_stream_error(c, "stdout"));
    }
    if !writable(c, stderr)? {
        return Err(writable_stream_error(c, "stderr"));
    }
    let indentation = if c.rooted_value(indentation).is_some_and(|value| value.is_undefined()) {
        DEFAULT_GROUP_INDENTATION
    } else {
        let maximum = c.number(MAX_GROUP_INDENTATION);
        super::util_shared_vm::validate_integer(c, indentation, "groupIndentation", 0.0, maximum)?
    };
    let ignore_errors = c.rooted_value(ignore).is_none_or(|value| value.is_undefined()) || c.truthy_rooted(ignore)?;
    for &(name, _, method, _) in &PROTOTYPE_KEYS {
        let function = method_function(c, name, method, Some(receiver))?;
        put(c, receiver, name, function)?;
    }
    hidden(c, receiver, "_stdout", stdout)?;
    hidden(c, receiver, "_stderr", stderr)?;
    for name in ["_stdoutErrorHandler", "_stderrErrorHandler"] {
        let handler = c.host_function_named(crate::host::shared_vm::operation("consoleNoop"), "")?;
        hidden(c, receiver, name, handler)?;
    }
    let ignore = c.boolean(ignore_errors);
    hidden(c, receiver, "_ignoreErrors", ignore)?;
    let global = c.global_root()?;
    let map = get(c, global, "Map")?;
    let times = c.construct_rooted(map, map, &[])?;
    hidden(c, receiver, "_times", times)?;
    attach_state(c, receiver, ignore_errors, indentation)?;
    Ok(undefined)
}

/// Whether `table` has no entry `name`.
fn absent(c: &mut Context<'_>, table: RootId, name: &str) -> Result<bool, RootedError> {
    let value = get(c, table, name)?;
    Ok(c.rooted_value(value).is_some_and(|value| value.is_undefined()))
}
