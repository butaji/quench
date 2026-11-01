//! `node:repl`: `REPLServer`, a readline `Interface` that evaluates each line (or the
//! lines a recoverable syntax error buffers) in its context and writes the result with
//! its writer; `.`-commands; and the interactive REPL `-i` starts.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

const DEFAULT_PROMPT: &str = "> ";
/// The prompt while lines are buffered (Node's `...` with its trailing space).
const CONTINUATION_PROMPT: &str = "... ";
const RESOURCE_PREFIX: &str = "REPL";
/// The module name the REPL's `require` resolves from (Node's `<repl>`).
const REPL_MODULE: &str = "<repl>";
const BUFFERED: &str = "\0quench:repl:buffered";
const INITIAL_PROMPT: &str = "_initialPrompt";
/// Node's message for `.`-input that names no command.
const INVALID_KEYWORD: &str = "Invalid REPL keyword\n";
const HELP_FOOTER: &str = "\nPress Ctrl+C to abort current expression, Ctrl+D to exit the REPL\n";
const UNDERSCORE_DISABLED: &str = "Expression assignment to _ now disabled.\n";
const UNDERSCORE_ERROR_DISABLED: &str = "Expression assignment to _error now disabled.\n";
/// The gap between a command's name and its help in `.help`.
const HELP_GAP: usize = 3;
/// SyntaxError messages of input that more lines can complete (Node's `isRecoverableError`).
const RECOVERABLE_MESSAGES: [&str; 3] = ["Unexpected end of input", "Unterminated template literal", "missing ) after argument list"];

/// The commands every REPL has: (name, help, operation).
const DEFAULT_COMMANDS: [(&str, &str, &str); 6] = [
    ("break", "Sometimes you get stuck, this gets you out", "replCommandBreak"),
    ("clear", "Break, and also clear the local context", "replCommandClear"),
    ("exit", "Exit the REPL", "replCommandExit"),
    ("help", "Print this help message", "replCommandHelp"),
    ("save", "Save all evaluated commands in this REPL session to a file", "replCommandSave"),
    ("load", "Load JS from a file into the REPL session", "replCommandLoad"),
];
const CLEAR_ALIAS_HELP: &str = "Alias for .break";

const PROTOTYPE_METHODS: [(&str, &str); 10] = [
    ("setupHistory", "replSetupHistory"),
    ("clearBufferedCommand", "replClearBufferedCommand"),
    ("close", "replClose"),
    ("createContext", "replCreateContext"),
    ("resetContext", "replResetContext"),
    ("displayPrompt", "replDisplayPrompt"),
    ("setPrompt", "replSetPrompt"),
    ("complete", "replComplete"),
    ("completeOnEditorMode", "replCompleteOnEditorMode"),
    ("defineCommand", "replDefineCommand"),
];

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.set_property_rooted(object, key, value, object)?;
    Ok(())
}

fn hidden(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, false, true)?;
    Ok(())
}

fn call(c: &mut Context<'_>, object: RootId, method: &str, args: &[RootId]) -> Result<RootId, RootedError> {
    let function = get(c, object, method)?;
    c.call_rooted(function, object, args)
}

fn function(c: &mut Context<'_>, operation: &str, name: &str, data: RootId) -> Result<RootId, RootedError> {
    c.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, data)
}

fn write(c: &mut Context<'_>, server: RootId, text: &str) -> Result<(), RootedError> {
    let output = get(c, server, "output")?;
    let text = c.string_rooted(text);
    call(c, output, "write", &[text])?;
    Ok(())
}

fn has_own(c: &mut Context<'_>, object: RootId, key: RootId) -> Result<bool, RootedError> {
    let descriptor = c.own_property_descriptor_rooted(object, key)?;
    Ok(!c.rooted_value(descriptor).is_some_and(|value| value.is_undefined()))
}

fn nullish(c: &mut Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_null() || value.is_undefined())
}

fn interface_constructor(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let readline = crate::host::shared_vm::commonjs::readline_module(c)?;
    get(c, readline, "Interface")
}

fn cache(c: &mut Context<'_>, name: &str) -> Option<RootId> {
    c.host_mut().shared_state().borrow().builtin_cache.get(name).copied()
}

fn remember(c: &mut Context<'_>, name: &str, value: RootId) -> Result<RootId, RootedError> {
    let value = c.retain(value)?;
    c.host_mut().shared_state().borrow_mut().builtin_cache.insert(name.to_owned(), value);
    Ok(value)
}

const MODULE_CACHE: &str = "\0quench:repl:module";
const SERVER_CACHE: &str = "\0quench:repl:server";
const RECOVERABLE_CACHE: &str = "\0quench:repl:recoverable";

pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    if let Some(module) = cache(c, MODULE_CACHE) {
        return Ok(module);
    }
    let module = c.object_rooted()?;
    let undefined = c.undefined();
    let start = function(c, "replStart", "start", undefined)?;
    put(c, module, "start", start)?;
    let writer = writer(c)?;
    put(c, module, "writer", writer)?;
    let server = server_class(c)?;
    put(c, module, "REPLServer", server)?;
    for (name, description) in [("REPL_MODE_SLOPPY", "repl-sloppy"), ("REPL_MODE_STRICT", "repl-strict")] {
        let symbol = c.symbol_rooted(Some(description));
        put(c, module, name, symbol)?;
    }
    let recoverable = recoverable_class(c)?;
    put(c, module, "Recoverable", recoverable)?;
    let valid = function(c, "replIsValidSyntax", "isValidSyntax", undefined)?;
    put(c, module, "isValidSyntax", valid)?;
    remember(c, MODULE_CACHE, module)
}

/// `repl.writer`: `util.inspect` with `writer.options` (Node's defaults plus `showProxy`).
fn writer(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let writer = function(c, "replWriter", "writer", undefined)?;
    let util = crate::host::shared_vm::commonjs::util_module(c)?;
    let inspect = get(c, util, "inspect")?;
    let defaults = get(c, inspect, "defaultOptions")?;
    let options = c.object_rooted()?;
    assign(c, options, defaults)?;
    let yes = c.boolean(true);
    put(c, options, "showProxy", yes)?;
    put(c, writer, "options", options)?;
    Ok(writer)
}

fn assign(c: &mut Context<'_>, target: RootId, source: RootId) -> Result<(), RootedError> {
    let global = c.global_root()?;
    let object = get(c, global, "Object")?;
    call(c, object, "assign", &[target, source])?;
    Ok(())
}

pub(crate) fn writer_call(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let module = module(c)?;
    let writer = get(c, module, "writer")?;
    let options = get(c, writer, "options")?;
    inspect(c, args.first().copied(), options)
}

fn inspect(c: &mut Context<'_>, value: Option<RootId>, options: RootId) -> Result<RootId, RootedError> {
    let value = value.unwrap_or_else(|| c.undefined());
    let util = crate::host::shared_vm::commonjs::util_module(c)?;
    call(c, util, "inspect", &[value, options])
}

/// A server's writer when it uses `repl.writer` with colours: the same options, coloured.
pub(crate) fn colored_writer(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let options = c.host_function_data()?;
    inspect(c, args.first().copied(), options)
}

fn recoverable_class(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation("replRecoverable"), "Recoverable")?;
    let prototype = get(c, constructor, "prototype")?;
    let global = c.global_root()?;
    let syntax_error = get(c, global, "SyntaxError")?;
    let parent = get(c, syntax_error, "prototype")?;
    c.set_prototype_rooted(prototype, parent)?;
    c.set_prototype_rooted(constructor, syntax_error)?;
    remember(c, RECOVERABLE_CACHE, constructor)
}

pub(crate) fn recoverable(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let error = args.first().copied().unwrap_or_else(|| c.undefined());
    put(c, this, "err", error)?;
    Ok(this)
}

fn is_recoverable(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    let Some(constructor) = cache(c, RECOVERABLE_CACHE) else {
        return Ok(false);
    };
    if !c.is_object_rooted(value)? {
        return Ok(false);
    }
    c.instance_of_rooted(value, constructor)
}

fn server_class(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation("replServer"), "REPLServer")?;
    let prototype = get(c, constructor, "prototype")?;
    let interface = interface_constructor(c)?;
    let parent = get(c, interface, "prototype")?;
    c.set_prototype_rooted(prototype, parent)?;
    c.set_prototype_rooted(constructor, interface)?;
    let undefined = c.undefined();
    for (name, operation) in PROTOTYPE_METHODS {
        let method = function(c, operation, name, undefined)?;
        hidden(c, prototype, name, method)?;
    }
    remember(c, SERVER_CACHE, constructor)
}

/// `repl.start([options])`.
pub(crate) fn start(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let constructor = match cache(c, SERVER_CACHE) {
        Some(constructor) => constructor,
        None => {
            module(c)?;
            cache(c, SERVER_CACHE).ok_or_else(|| RootedError::host("REPLServer missing"))?
        }
    };
    c.construct_rooted(constructor, constructor, args)
}

fn option(c: &mut Context<'_>, options: RootId, name: &str) -> Result<RootId, RootedError> {
    get(c, options, name)
}

/// `new REPLServer(prompt | options[, stream, eval, useGlobal, ignoreUndefined, replMode])`.
pub(crate) fn construct(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let argument = |index: usize| args.get(index).copied().unwrap_or(undefined);
    let options = c.object_rooted()?;
    let first = argument(0);
    let (mut stream, prompt) = if c.is_object_rooted(first)? {
        assign(c, options, first)?;
        let stream = option(c, options, "stream")?;
        let stream = if c.truthy_rooted(stream)? { stream } else { option(c, options, "socket")? };
        let prompt = option(c, options, "prompt")?;
        (stream, prompt)
    } else {
        for (index, name) in [(2, "eval"), (3, "useGlobal"), (4, "ignoreUndefined"), (5, "replMode")] {
            let value = argument(index);
            put(c, options, name, value)?;
        }
        (argument(1), first)
    };
    let input = option(c, options, "input")?;
    let output = option(c, options, "output")?;
    if !c.truthy_rooted(input)? && !c.truthy_rooted(output)? {
        if !c.truthy_rooted(stream)? {
            stream = c.host_mut().shared_state().borrow().process_module.ok_or_else(|| RootedError::host("process missing"))?;
        }
        let stdin = get(c, stream, "stdin")?;
        let stdout = get(c, stream, "stdout")?;
        if c.truthy_rooted(stdin)? && c.truthy_rooted(stdout)? {
            put(c, options, "input", stdin)?;
            put(c, options, "output", stdout)?;
        } else {
            put(c, options, "input", stream)?;
            put(c, options, "output", stream)?;
        }
    }
    let output = option(c, options, "output")?;
    let terminal = option(c, options, "terminal")?;
    let terminal = if nullish(c, terminal) { get(c, output, "isTTY")? } else { terminal };
    let terminal = c.truthy_rooted(terminal)?;
    let use_colors = option(c, options, "useColors")?;
    let use_colors = if nullish(c, use_colors) { terminal } else { c.truthy_rooted(use_colors)? };
    let prompt = if nullish(c, prompt) { c.string_rooted(DEFAULT_PROMPT) } else { prompt };
    let interface_options = c.object_rooted()?;
    for name in ["input", "output", "completer", "historySize"] {
        let value = option(c, options, name)?;
        put(c, interface_options, name, value)?;
    }
    let terminal_value = c.boolean(terminal);
    put(c, interface_options, "terminal", terminal_value)?;
    put(c, interface_options, "prompt", prompt)?;
    let interface = interface_constructor(c)?;
    let target = c.new_target_rooted().unwrap_or(match cache(c, SERVER_CACHE) {
        Some(constructor) => constructor,
        None => interface,
    });
    let server = c.construct_rooted(interface, target, &[interface_options])?;
    let use_colors = c.boolean(use_colors);
    put(c, server, "useColors", use_colors)?;
    for name in ["useGlobal", "ignoreUndefined", "replMode", "breakEvalOnSigint"] {
        let value = option(c, options, name)?;
        let value = if name == "replMode" && nullish(c, value) {
            let module = module(c)?;
            get(c, module, "REPL_MODE_SLOPPY")?
        } else if name == "replMode" {
            value
        } else {
            let flag = c.truthy_rooted(value)?;
            c.boolean(flag)
        };
        put(c, server, name, value)?;
    }
    let no = c.boolean(false);
    for name in ["underscoreAssigned", "underscoreErrAssigned", "editorMode"] {
        put(c, server, name, no)?;
    }
    for name in ["last", "lastError"] {
        put(c, server, name, undefined)?;
    }
    let evaluate = option(c, options, "eval")?;
    let evaluate = if c.is_callable_rooted(evaluate)? { evaluate } else { function(c, "replDefaultEval", "defaultEval", undefined)? };
    put(c, server, "eval", evaluate)?;
    let requested_writer = option(c, options, "writer")?;
    let module = module(c)?;
    let default_writer = get(c, module, "writer")?;
    let writer = if c.is_callable_rooted(requested_writer)? { requested_writer } else { default_writer };
    let writer = if c.strict_equal_rooted(writer, default_writer)? && c.truthy_rooted(use_colors)? {
        let defaults = get(c, default_writer, "options")?;
        let colored = c.object_rooted()?;
        assign(c, colored, defaults)?;
        let yes = c.boolean(true);
        put(c, colored, "colors", yes)?;
        let writer = function(c, "replColoredWriter", "writer", colored)?;
        put(c, writer, "options", colored)?;
        writer
    } else {
        writer
    };
    put(c, server, "writer", writer)?;
    let commands = c.null_object_rooted()?;
    put(c, server, "commands", commands)?;
    hidden(c, server, INITIAL_PROMPT, prompt)?;
    let lines = c.array_rooted(&[])?;
    put(c, server, "lines", lines)?;
    let empty = c.string_rooted("");
    crate::modules::private_slots::write(c, server, BUFFERED, empty)?;
    reset_context(c, server, &[])?;
    define_default_commands(c, server)?;
    for (event, operation) in [("line", "replLine"), ("close", "replEmitExit")] {
        let listener = function(c, operation, "", server)?;
        let event = c.string_rooted(event);
        call(c, server, "on", &[event, listener])?;
    }
    display_prompt(c, server, &[])?;
    Ok(server)
}

fn define_default_commands(c: &mut Context<'_>, server: RootId) -> Result<(), RootedError> {
    let use_global = get(c, server, "useGlobal")?;
    let use_global = c.truthy_rooted(use_global)?;
    let commands = get(c, server, "commands")?;
    for (name, help, operation) in DEFAULT_COMMANDS {
        let help = if name == "clear" && use_global { CLEAR_ALIAS_HELP } else { help };
        let command = c.object_rooted()?;
        let help = c.string_rooted(help);
        put(c, command, "help", help)?;
        let undefined = c.undefined();
        let action = function(c, operation, "action", undefined)?;
        put(c, command, "action", action)?;
        put(c, commands, name, command)?;
    }
    Ok(())
}

pub(crate) fn emit_exit(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let server = c.host_function_data()?;
    let event = c.string_rooted("exit");
    call(c, server, "emit", &[event])?;
    Ok(c.undefined())
}

fn buffered(c: &mut Context<'_>, server: RootId) -> Result<String, RootedError> {
    match crate::modules::private_slots::read(c, server, BUFFERED)? {
        Some(value) => Ok(c.string_text(value)?.unwrap_or_default()),
        None => Ok(String::new()),
    }
}

fn set_buffered(c: &mut Context<'_>, server: RootId, text: &str) -> Result<(), RootedError> {
    let text = c.string_rooted(text);
    crate::modules::private_slots::write(c, server, BUFFERED, text)
}

/// A line of input: a `.`-command, or code to evaluate with what is buffered.
pub(crate) fn on_line(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let server = c.host_function_data()?;
    let undefined = c.undefined();
    let line = match args.first().copied() {
        Some(line) => c.string_text(line)?.unwrap_or_default(),
        None => String::new(),
    };
    let trimmed = line.trim();
    let pending = buffered(c, server)?;
    if trimmed.starts_with('.') && !trimmed[1..].starts_with('.') && trimmed.parse::<f64>().is_err() {
        let body = &trimmed[1..];
        let (keyword, rest) = body.split_once(char::is_whitespace).map_or((body, ""), |(keyword, rest)| (keyword, rest.trim_start()));
        let commands = get(c, server, "commands")?;
        let command = get(c, commands, keyword)?;
        if c.is_object_rooted(command)? {
            let action = get(c, command, "action")?;
            let rest = c.string_rooted(rest);
            c.call_rooted(action, server, &[rest])?;
            return Ok(undefined);
        }
        if pending.is_empty() {
            write(c, server, INVALID_KEYWORD)?;
            let null = c.null();
            let finish = finisher(c, server, &line)?;
            c.call_rooted(finish, undefined, &[null])?;
            return Ok(undefined);
        }
    }
    let code = format!("{pending}{line}\n");
    let evaluate = get(c, server, "eval")?;
    let context = get(c, server, "context")?;
    let code_value = c.string_rooted(&code);
    let resource = resource_name(c)?;
    let finish = finisher(c, server, &line)?;
    c.call_rooted(evaluate, server, &[code_value, context, resource, finish])?;
    Ok(undefined)
}

/// The `finish` callback of one input line.
fn finisher(c: &mut Context<'_>, server: RootId, line: &str) -> Result<RootId, RootedError> {
    let line = c.string_rooted(line);
    let data = c.array_rooted(&[server, line])?;
    function(c, "replFinish", "finish", data)
}

/// The next `REPLn` resource name, numbered across every REPL of the process.
fn resource_name(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let shared = c.host_mut().shared_state();
    let number = {
        let mut state = shared.borrow_mut();
        state.repl_resources += 1;
        state.repl_resources
    };
    Ok(c.string_rooted(&format!("{RESOURCE_PREFIX}{number}")))
}

/// An evaluation's callback: buffer a recoverable error, report another, or write the result.
pub(crate) fn finish(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let server = get(c, data, "0")?;
    let line = get(c, data, "1")?;
    let line = c.string_text(line)?.unwrap_or_default();
    let undefined = c.undefined();
    let lines = get(c, server, "lines")?;
    let line_value = c.string_rooted(&line);
    call(c, lines, "push", &[line_value])?;
    let source = format!("{}{line}\n", buffered(c, server)?);
    if let Some(error) = args.first().copied().filter(|error| !nullish(c, *error)) {
        if is_recoverable(c, error)? {
            set_buffered(c, server, &source)?;
            display_prompt(c, server, &[])?;
            return Ok(undefined);
        }
        let error = match get_recoverable_cause(c, error)? {
            Some(cause) => cause,
            None => error,
        };
        return report_error(c, server, error, Some(&source)).map(|()| undefined);
    }
    set_buffered(c, server, "")?;
    // Only an evaluation's completion carries a result; `finish(null)` prints nothing.
    if let [_, result] = args {
        let ignore = get(c, server, "ignoreUndefined")?;
        if !c.truthy_rooted(ignore)? || !c.rooted_value(*result).is_some_and(|value| value.is_undefined()) {
            let assigned = get(c, server, "underscoreAssigned")?;
            if !c.truthy_rooted(assigned)? {
                put(c, server, "last", *result)?;
            }
            let writer = get(c, server, "writer")?;
            let text = c.call_rooted(writer, server, &[*result])?;
            let text = c.to_string(text)?;
            write(c, server, &format!("{text}\n"))?;
        }
    }
    display_prompt(c, server, &[])?;
    Ok(undefined)
}

/// Node's REPL error report: the stack without the REPL's own frames, `Uncaught ` before
/// its first error line.
fn report_error(c: &mut Context<'_>, server: RootId, shown: RootId, source: Option<&str>) -> Result<(), RootedError> {
    if c.is_object_rooted(shown)? {
        let stack = get(c, shown, "stack")?;
        if let Some(stack) = c.string_text(stack)? {
            let name = get(c, shown, "name")?;
            let syntax = c.string_text(name)?.as_deref() == Some("SyntaxError");
            let mut stack = repl_stack(&stack, syntax);
            if let Some(source) = source.filter(|_| syntax && !stack.contains("\n\n")) {
                stack = format!("{}{stack}", source_arrow(source));
            }
            let stack = c.string_rooted(&stack);
            let key = c.string_rooted("stack");
            c.define_data_property_rooted(shown, key, stack, true, false, true)?;
        }
    }
    let assigned = get(c, server, "underscoreErrAssigned")?;
    if !c.truthy_rooted(assigned)? {
        put(c, server, "lastError", shown)?;
    }
    let writer = get(c, server, "writer")?;
    let text = c.call_rooted(writer, server, &[shown])?;
    let mut text = c.to_string(text)?;
    if text.starts_with('[') && text.ends_with(']') {
        text = text[1..text.len() - 1].to_owned();
    }
    let lines = text.split_inclusive('\n').collect::<Vec<_>>();
    let mut matched = false;
    let mut report = String::new();
    for line in &lines {
        if !matched && is_error_line(line) {
            report.push_str("Uncaught ");
            matched = true;
        }
        report.push_str(line);
    }
    if !matched {
        report = format!("Uncaught{}{text}", if lines.len() == 1 { " " } else { ":\n" });
    }
    if !report.ends_with('\n') {
        report.push('\n');
    }
    write(c, server, &report)?;
    set_buffered(c, server, "")?;
    display_prompt(c, server, &[])?;
    Ok(())
}

/// Node's `decorateErrorStack` arrow for a syntax error in REPL input: the offending
/// source line and a caret under the column the compiler reports.
fn source_arrow(source: &str) -> String {
    let request = quench_runtime::ExecutionRequest { source, name: RESOURCE_PREFIX, kind: quench_runtime::SourceKind::Script };
    let Err(diagnostics) = quench_runtime::Engine::compile(request) else {
        return String::new();
    };
    let Some(offset) = diagnostics.iter().find(|diagnostic| diagnostic.is_syntax_error()).map(|diagnostic| diagnostic.offset() as usize) else {
        return String::new();
    };
    let offset = offset.min(source.len());
    let start = source[..offset].rfind('\n').map_or(0, |newline| newline + 1);
    let end = source[offset..].find('\n').map_or(source.len(), |newline| offset + newline);
    let padding = " ".repeat(source[start..offset].chars().count());
    format!("{}\n{padding}^\n\n", &source[start..end])
}

fn get_recoverable_cause(c: &mut Context<'_>, error: RootId) -> Result<Option<RootId>, RootedError> {
    if !is_recoverable(c, error)? {
        return Ok(None);
    }
    get(c, error, "err").map(Some)
}

/// `^\[?([A-Z][a-z0-9_]*)*Error`: a line naming an error class.
fn is_error_line(line: &str) -> bool {
    let line = line.strip_prefix('[').unwrap_or(line);
    let word = line.split(|character: char| !character.is_ascii_alphanumeric() && character != '_').next().unwrap_or_default();
    word.ends_with("Error") && word.starts_with(|character: char| character.is_ascii_uppercase())
}

/// The stack Node's REPL shows: no source-arrow header, and no frames from the REPL
/// line's own top level down (a SyntaxError keeps its arrow but shows no frames).
fn repl_stack(stack: &str, syntax: bool) -> String {
    let mut lines = stack.split('\n').collect::<Vec<_>>();
    let header_end = lines.iter().position(|line| line.is_empty());
    let has_header = lines.first().is_some_and(|line| line.starts_with(RESOURCE_PREFIX) && line.contains(':'));
    if let (true, Some(end)) = (has_header, header_end) {
        if syntax {
            lines.remove(0);
        } else {
            lines.drain(..=end);
        }
    }
    let frames_end = lines
        .iter()
        .position(|line| {
            let frame = line.trim_start();
            frame.starts_with("at ") && (syntax || frame[3..].starts_with(RESOURCE_PREFIX))
        })
        .unwrap_or(lines.len());
    lines.truncate(frames_end);
    lines.join("\n")
}

/// Node's `defaultEval`: an object-literal-looking line is tried as an expression first;
/// a syntax error more input can complete is a `Recoverable`.
pub(crate) fn default_eval(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let code = args.first().copied().unwrap_or(undefined);
    let code = c.to_string(code)?;
    let context = args.get(1).copied().unwrap_or(undefined);
    let file = args.get(2).copied().unwrap_or(undefined);
    let callback = args.get(3).copied().unwrap_or(undefined);
    let null = c.null();
    if code == "\n" {
        c.call_rooted(callback, undefined, &[null])?;
        return Ok(undefined);
    }
    let trimmed = code.trim();
    let wrapped = trimmed.starts_with('{') && !trimmed.ends_with(';');
    let strict = {
        let mode = get(c, this, "replMode")?;
        let module = module(c)?;
        let strict = get(c, module, "REPL_MODE_STRICT")?;
        c.strict_equal_rooted(mode, strict)?
    };
    let vm = crate::host::shared_vm::commonjs::vm_module(c)?;
    let script_class = get(c, vm, "Script")?;
    let options = c.object_rooted()?;
    put(c, options, "filename", file)?;
    let yes = c.boolean(true);
    put(c, options, "displayErrors", yes)?;
    let mut candidates = Vec::new();
    if wrapped {
        candidates.push(format!("({trimmed})\n"));
    }
    candidates.push(code.clone());
    let mut compiled = None;
    let mut failure = None;
    for (index, candidate) in candidates.iter().enumerate() {
        let source = if strict { format!("'use strict'; void 0;\n{candidate}") } else { candidate.clone() };
        let source = c.string_rooted(&source);
        match c.construct_rooted(script_class, script_class, &[source, options]) {
            Ok(script) => {
                compiled = Some(script);
                break;
            }
            Err(RootedError { exception: Some(error), .. }) if index + 1 == candidates.len() => failure = Some(error),
            Err(RootedError { exception: Some(_), .. }) => {}
            Err(error) => return Err(error),
        }
    }
    let completion = match (compiled, failure) {
        (Some(script), _) => {
            let use_global = get(c, this, "useGlobal")?;
            let result = if c.truthy_rooted(use_global)? {
                call(c, script, "runInThisContext", &[options])
            } else {
                call(c, script, "runInContext", &[context, options])
            };
            match result {
                Ok(value) => (null, value),
                // Node's domain reports a thrown evaluation without its callback.
                Err(RootedError { exception: Some(error), .. }) => {
                    report_error(c, this, error, None)?;
                    return Ok(undefined);
                }
                Err(error) => return Err(error),
            }
        }
        (None, Some(error)) => {
            if recoverable_syntax(c, error, &code)? {
                let constructor = cache(c, RECOVERABLE_CACHE).ok_or_else(|| RootedError::host("Recoverable missing"))?;
                (c.construct_rooted(constructor, constructor, &[error])?, undefined)
            } else {
                (error, undefined)
            }
        }
        (None, None) => (null, undefined),
    };
    c.call_rooted(callback, undefined, &[completion.0, completion.1])?;
    Ok(undefined)
}

fn recoverable_syntax(c: &mut Context<'_>, error: RootId, code: &str) -> Result<bool, RootedError> {
    let message = get(c, error, "message")?;
    let message = c.string_text(message)?.unwrap_or_default();
    let unclosed_template = code.matches('`').count() % 2 == 1;
    Ok(RECOVERABLE_MESSAGES.contains(&message.as_str()) || unclosed_template)
}

pub(crate) fn is_valid_syntax(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let code = args.first().copied().unwrap_or(undefined);
    let vm = crate::host::shared_vm::commonjs::vm_module(c)?;
    let script_class = get(c, vm, "Script")?;
    let valid = match c.construct_rooted(script_class, script_class, &[code]) {
        Ok(_) => true,
        Err(RootedError { exception: Some(_), .. }) => false,
        Err(error) => return Err(error),
    };
    Ok(c.boolean(valid))
}

/// `displayPrompt()`: the initial prompt, or the continuation prompt while lines are buffered.
pub(crate) fn display_prompt(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let prompt = if buffered(c, this)?.is_empty() {
        get(c, this, INITIAL_PROMPT)?
    } else {
        c.string_rooted(CONTINUATION_PROMPT)
    };
    let interface = interface_constructor(c)?;
    let prototype = get(c, interface, "prototype")?;
    let set_prompt = get(c, prototype, "setPrompt")?;
    c.call_rooted(set_prompt, this, &[prompt])?;
    let preserve = args.first().copied().unwrap_or_else(|| c.undefined());
    call(c, this, "prompt", &[preserve])?;
    Ok(c.undefined())
}

pub(crate) fn set_prompt(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let prompt = args.first().copied().unwrap_or_else(|| c.undefined());
    hidden(c, this, INITIAL_PROMPT, prompt)?;
    let interface = interface_constructor(c)?;
    let prototype = get(c, interface, "prototype")?;
    let set_prompt = get(c, prototype, "setPrompt")?;
    c.call_rooted(set_prompt, this, &[prompt])
}

pub(crate) fn clear_buffered_command(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    set_buffered(c, this, "")?;
    Ok(c.undefined())
}

pub(crate) fn close(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let interface = interface_constructor(c)?;
    let prototype = get(c, interface, "prototype")?;
    let close = get(c, prototype, "close")?;
    super::shared_event_loop::queue_next_tick(c, close, this, &[])?;
    Ok(c.undefined())
}

pub(crate) fn setup_history(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let callback = args.iter().copied().rev().find(|value| c.is_callable_rooted(*value).unwrap_or(false));
    if let Some(callback) = callback {
        let null = c.null();
        let undefined = c.undefined();
        super::shared_event_loop::queue_next_tick(c, callback, undefined, &[null, this])?;
    }
    Ok(c.undefined())
}

pub(crate) fn complete(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let line = args.first().copied().unwrap_or(undefined);
    let callback = args.get(1).copied().unwrap_or(undefined);
    let completions = c.array_rooted(&[])?;
    let result = c.array_rooted(&[completions, line])?;
    let null = c.null();
    if c.is_callable_rooted(callback)? {
        c.call_rooted(callback, undefined, &[null, result])?;
    }
    Ok(undefined)
}

/// `defineCommand(keyword, cmd)`: a function is the command's action.
pub(crate) fn define_command(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let keyword = args.first().copied().unwrap_or(undefined);
    let keyword = c.to_string(keyword)?;
    let command = args.get(1).copied().unwrap_or(undefined);
    let command = if c.is_callable_rooted(command)? {
        let object = c.object_rooted()?;
        put(c, object, "action", command)?;
        object
    } else {
        let action = if c.is_object_rooted(command)? { get(c, command, "action")? } else { undefined };
        if !c.is_callable_rooted(action)? {
            return Err(super::util_shared_vm::argument_type_error(c, action, "cmd.action", "function")?);
        }
        command
    };
    let commands = get(c, this, "commands")?;
    put(c, commands, &keyword, command)?;
    Ok(undefined)
}

/// `createContext()`: the global object, or a fresh vm context with the globals, a
/// console on the output, `require` and the built-in modules.
pub(crate) fn create_context(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let global = c.global_root()?;
    let use_global = get(c, this, "useGlobal")?;
    let context = if c.truthy_rooted(use_global)? {
        global
    } else {
        let vm = crate::host::shared_vm::commonjs::vm_module(c)?;
        let context = call(c, vm, "createContext", &[])?;
        let names = c.own_keys_rooted(global)?;
        for name in names {
            if c.string_text(name)?.is_some_and(|name| name == "console" || name == "global") {
                continue;
            }
            if has_own(c, context, name)? {
                continue;
            }
            // A lazily installed global materializes when read; one whose installer fails
            // is absent from the context as it would be from a script reading it.
            let Ok(descriptor) = c.own_property_descriptor_rooted(global, name) else {
                continue;
            };
            if c.is_object_rooted(descriptor)? {
                c.define_property_descriptor_rooted(context, name, descriptor)?;
            }
        }
        hidden(c, context, "global", context)?;
        let console = crate::host::shared_vm::commonjs::console_module(c)?;
        let console_class = get(c, console, "Console")?;
        let output = get(c, this, "output")?;
        let console = c.construct_rooted(console_class, console_class, &[output])?;
        hidden(c, context, "console", console)?;
        let require = repl_require(c)?;
        hidden(c, context, "require", require)?;
        context
    };
    builtin_accessors(c, context)?;
    for (name, getter, setter) in [("_", "replUnderscoreGet", "replUnderscoreSet"), ("_error", "replUnderscoreErrorGet", "replUnderscoreErrorSet")] {
        let descriptor = c.object_rooted()?;
        let get_function = function(c, getter, &format!("get {name}"), this)?;
        let set_function = function(c, setter, &format!("set {name}"), this)?;
        put(c, descriptor, "get", get_function)?;
        put(c, descriptor, "set", set_function)?;
        let yes = c.boolean(true);
        put(c, descriptor, "configurable", yes)?;
        let key = c.string_rooted(name);
        c.define_property_descriptor_rooted(context, key, descriptor)?;
    }
    Ok(context)
}

/// Node's `addBuiltinLibsToObject`: each public built-in, required on first read.
fn builtin_accessors(c: &mut Context<'_>, context: RootId) -> Result<(), RootedError> {
    for name in super::module_shared_vm::BUILTIN_MODULES {
        if name.starts_with('_') || name.contains('/') || name.starts_with("node:") {
            continue;
        }
        let key = c.string_rooted(name);
        // An existing global (even one whose lazy installer fails) keeps its own value.
        if !matches!(has_own(c, context, key), Ok(false)) {
            continue;
        }
        let data = c.array_rooted(&[context, key])?;
        let descriptor = c.object_rooted()?;
        let getter = function(c, "replBuiltinGet", "get", data)?;
        let setter = function(c, "replBuiltinSet", "set", data)?;
        put(c, descriptor, "get", getter)?;
        put(c, descriptor, "set", setter)?;
        let yes = c.boolean(true);
        put(c, descriptor, "configurable", yes)?;
        c.define_property_descriptor_rooted(context, key, descriptor)?;
    }
    Ok(())
}

fn replace_with_value(c: &mut Context<'_>, value: RootId) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let context = get(c, data, "0")?;
    let key = get(c, data, "1")?;
    c.define_data_property_rooted(context, key, value, true, false, true)?;
    Ok(value)
}

pub(crate) fn builtin_get(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let key = get(c, data, "1")?;
    let require = repl_require(c)?;
    let undefined = c.undefined();
    let module = c.call_rooted(require, undefined, &[key])?;
    replace_with_value(c, module)
}

/// The REPL's `require`: Node's, relative to a `<repl>` module in the working directory.
fn repl_require(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let module = crate::host::shared_vm::commonjs::module_module(c)?;
    let path = std::env::current_dir().unwrap_or_default().join(REPL_MODULE);
    let path = c.string_rooted(&path.to_string_lossy());
    call(c, module, "createRequire", &[path])
}

pub(crate) fn builtin_set(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| c.undefined());
    replace_with_value(c, value)?;
    Ok(c.undefined())
}

fn underscore_get(c: &mut Context<'_>, field: &str) -> Result<RootId, RootedError> {
    let server = c.host_function_data()?;
    get(c, server, field)
}

fn underscore_set(c: &mut Context<'_>, args: &[RootId], flag: &str, field: &str, warning: &str) -> Result<RootId, RootedError> {
    let server = c.host_function_data()?;
    let value = args.first().copied().unwrap_or_else(|| c.undefined());
    put(c, server, field, value)?;
    let assigned = get(c, server, flag)?;
    if !c.truthy_rooted(assigned)? {
        let yes = c.boolean(true);
        put(c, server, flag, yes)?;
        write(c, server, warning)?;
    }
    Ok(c.undefined())
}

pub(crate) fn underscore_get_last(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    underscore_get(c, "last")
}

pub(crate) fn underscore_set_last(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    underscore_set(c, args, "underscoreAssigned", "last", UNDERSCORE_DISABLED)
}

pub(crate) fn underscore_get_error(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    underscore_get(c, "lastError")
}

pub(crate) fn underscore_set_error(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    underscore_set(c, args, "underscoreErrAssigned", "lastError", UNDERSCORE_ERROR_DISABLED)
}

/// `resetContext()`: a fresh context, `_` unassigned, and the `reset` event.
pub(crate) fn reset_context(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let context = create_context(c, this, &[])?;
    put(c, this, "context", context)?;
    let no = c.boolean(false);
    put(c, this, "underscoreAssigned", no)?;
    let lines = c.array_rooted(&[])?;
    put(c, this, "lines", lines)?;
    let event = c.string_rooted("reset");
    call(c, this, "emit", &[event, context])?;
    Ok(c.undefined())
}

pub(crate) fn command_break(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    set_buffered(c, this, "")?;
    display_prompt(c, this, &[])
}

pub(crate) fn command_clear(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    set_buffered(c, this, "")?;
    let use_global = get(c, this, "useGlobal")?;
    if !c.truthy_rooted(use_global)? {
        write(c, this, "Clearing context...\n")?;
        reset_context(c, this, &[])?;
    }
    display_prompt(c, this, &[])
}

pub(crate) fn command_exit(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    call(c, this, "close", &[])
}

pub(crate) fn command_help(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let commands = get(c, this, "commands")?;
    let mut names = Vec::new();
    for key in c.own_keys_rooted(commands)? {
        if let Some(name) = c.string_text(key)? {
            names.push(name);
        }
    }
    names.sort();
    let longest = names.iter().map(|name| name.encode_utf16().count()).max().unwrap_or_default();
    let mut text = String::new();
    for name in &names {
        let command = get(c, commands, name)?;
        let help = get(c, command, "help")?;
        let help = if c.truthy_rooted(help)? { Some(c.to_string(help)?) } else { None };
        let spaces = " ".repeat(longest - name.encode_utf16().count() + HELP_GAP);
        text.push_str(&format!(".{name}{}\n", help.map(|help| format!("{spaces}{help}")).unwrap_or_default()));
    }
    text.push_str(HELP_FOOTER);
    write(c, this, &text)?;
    display_prompt(c, this, &[])
}

pub(crate) fn command_save(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let file = match args.first().copied() {
        Some(file) => c.to_string(file)?,
        None => String::new(),
    };
    let lines = get(c, this, "lines")?;
    let separator = c.string_rooted("\n");
    let text = call(c, lines, "join", &[separator])?;
    let text = c.to_string(text)?;
    let message = match std::fs::write(&file, text) {
        Ok(()) => format!("Session saved to: {file}\n"),
        Err(_) => format!("Failed to save: {file}\n"),
    };
    write(c, this, &message)?;
    display_prompt(c, this, &[])
}

/// `.load file`: the file evaluated as one input, as Node's editor mode does.
pub(crate) fn command_load(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let file = match args.first().copied() {
        Some(file) => c.to_string(file)?,
        None => String::new(),
    };
    match std::fs::metadata(&file) {
        Ok(metadata) if metadata.is_file() => match std::fs::read_to_string(&file) {
            Ok(source) => {
                let evaluate = get(c, this, "eval")?;
                let context = get(c, this, "context")?;
                let code = c.string_rooted(&format!("{source}\n"));
                let resource = resource_name(c)?;
                let finish = finisher(c, this, source.trim_end())?;
                c.call_rooted(evaluate, this, &[code, context, resource, finish])?;
            }
            Err(_) => write(c, this, &format!("Failed to load: {file}\n"))?,
        },
        Ok(_) => write(c, this, &format!("Failed to load: {file} is not a valid file\n"))?,
        Err(_) => write(c, this, &format!("Failed to load: {file}\n"))?,
    }
    display_prompt(c, this, &[])
}

/// `node -i`: Node's welcome, then a REPL on stdin/stdout over the global object that
/// ends the process when it exits.
pub(crate) fn start_interactive(c: &mut Context<'_>) -> Result<(), RootedError> {
    let welcome = format!(
        "Welcome to Node.js v{}.\nType \".help\" for more information.\n",
        crate::modules::process_state::NODE_VERSION
    );
    let process = c.host_mut().shared_state().borrow().process_module.ok_or_else(|| RootedError::host("process missing"))?;
    let stdout = get(c, process, "stdout")?;
    let welcome = c.string_rooted(&welcome);
    call(c, stdout, "write", &[welcome])?;
    let options = c.object_rooted()?;
    let yes = c.boolean(true);
    put(c, options, "useGlobal", yes)?;
    let undefined = c.undefined();
    let server = start(c, undefined, &[options])?;
    let exit = get(c, process, "exit")?;
    let leave = call(c, exit, "bind", &[process])?;
    let event = c.string_rooted("exit");
    call(c, server, "on", &[event, leave])?;
    Ok(())
}
