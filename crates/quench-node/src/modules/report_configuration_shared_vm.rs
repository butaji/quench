//! Diagnostic reports (`process.report`, lib/internal/process/report.js over
//! src/node_report.cc): the native settings, the report document, and the public API.
//!
//! A report is one document of plain objects (`document`): `getReport` returns it, and the
//! binding's `getReport` and `writeReport` serialize it.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, Value};
type Context<'a> = NativeContext<'a, NodeHost>;

/// How a setting's value is stored and how its public setter validates it.
#[derive(Clone, Copy)]
enum Kind {
    /// A boolean; the public setter requires one.
    Boolean,
    /// The binding stores truthiness; the public setter requires a boolean.
    Truthiness,
    String,
    /// A signal name, which the public setter checks against the platform's signals.
    Signal,
}
struct Setting {
    /// The `process.report` accessor.
    property: &'static str,
    /// The name the public setter validates its value under.
    argument: &'static str,
    /// The `internalBinding('report')` getter and setter.
    get: &'static str,
    set: &'static str,
    option: &'static str,
    kind: Kind,
}
/// The settings, in `process.report`'s accessor order.
const SETTINGS: &[Setting] = &[
    Setting { property: "directory", argument: "directory", get: "getDirectory", set: "setDirectory", option: "--report-dir", kind: Kind::String },
    Setting { property: "filename", argument: "filename", get: "getFilename", set: "setFilename", option: "--report-filename", kind: Kind::String },
    Setting { property: "compact", argument: "compact", get: "getCompact", set: "setCompact", option: "--report-compact", kind: Kind::Truthiness },
    Setting { property: "excludeNetwork", argument: "excludeNetwork", get: "getExcludeNetwork", set: "setExcludeNetwork", option: "--report-exclude-network", kind: Kind::Boolean },
    Setting { property: "signal", argument: "signal", get: "getSignal", set: "setSignal", option: "--report-signal", kind: Kind::Signal },
    Setting { property: "reportOnFatalError", argument: "trigger", get: "shouldReportOnFatalError", set: "setReportOnFatalError", option: "--report-on-fatalerror", kind: Kind::Boolean },
    Setting { property: "reportOnSignal", argument: "trigger", get: "shouldReportOnSignal", set: "setReportOnSignal", option: "--report-on-signal", kind: Kind::Boolean },
    Setting { property: "reportOnUncaughtException", argument: "trigger", get: "shouldReportOnUncaughtException", set: "setReportOnUncaughtException", option: "--report-uncaught-exception", kind: Kind::Boolean },
    Setting { property: "excludeEnv", argument: "excludeEnv", get: "getExcludeEnv", set: "setExcludeEnv", option: "--report-exclude-env", kind: Kind::Boolean },
];
#[derive(Clone)]
enum SettingValue {
    Boolean(bool),
    String(String),
}
pub(crate) struct State {
    values: Vec<SettingValue>,
    /// Reports written with a generated name, which counts them.
    sequence: u32,
}
impl Default for State {
    fn default() -> Self {
        Self::from_options(&super::cli_options::CliOptions::default())
    }
}
impl State {
    pub(crate) fn from_options(options: &super::cli_options::CliOptions) -> Self {
        Self {
            values: SETTINGS
                .iter()
                .map(|setting| match setting.kind {
                    Kind::Boolean | Kind::Truthiness => SettingValue::Boolean(options.enabled(setting.option)),
                    Kind::String | Kind::Signal => SettingValue::String(
                        options.string_value(setting.option).expect("declared report string option").to_owned(),
                    ),
                })
                .collect(),
            sequence: 0,
        }
    }
}

fn setting_position(option: &str) -> usize {
    SETTINGS.iter().position(|setting| setting.option == option).expect("declared report setting")
}
fn value(context: &mut Context<'_>, option: &str) -> SettingValue {
    context.host_mut().shared_state().borrow().report_config.values[setting_position(option)].clone()
}
fn flag(context: &mut Context<'_>, option: &str) -> bool {
    matches!(value(context, option), SettingValue::Boolean(true))
}
fn text(context: &mut Context<'_>, option: &str) -> String {
    match value(context, option) {
        SettingValue::String(text) => text,
        SettingValue::Boolean(_) => unreachable!("report setting {option} is a string"),
    }
}
fn store(context: &mut Context<'_>, index: usize, value: SettingValue) {
    context.host_mut().shared_state().borrow_mut().report_config.values[index] = value;
}

const DIRECTORY: &str = "--report-dir";
const FILENAME: &str = "--report-filename";
const COMPACT: &str = "--report-compact";
const EXCLUDE_NETWORK: &str = "--report-exclude-network";
const EXCLUDE_ENV: &str = "--report-exclude-env";
const SIGNAL: &str = "--report-signal";
const REPORT_ON_SIGNAL: &str = "--report-on-signal";
const REPORT_ON_UNCAUGHT_EXCEPTION: &str = "--report-uncaught-exception";

/// `process.report`: an enumerable, configurable getter of the one report object.
pub(crate) fn install(context: &mut Context<'_>, process: RootId) -> Result<(), RootedError> {
    let getter = context.host_function_named(crate::host::shared_vm::operation("processReport"), "get")?;
    accessor(context, process, "report", getter, None)
}

fn accessor(context: &mut Context<'_>, object: RootId, name: &str, get: RootId, set: Option<RootId>) -> Result<(), RootedError> {
    let fields = quench_runtime::DescriptorFields {
        get: Some(get),
        set,
        enumerable: Some(true),
        configurable: Some(true),
        ..quench_runtime::DescriptorFields::default()
    };
    let key = context.string_rooted(name);
    context.define_property_fields_rooted(object, key, fields)?;
    Ok(())
}

fn put(context: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}

/// The `builtin_cache` keys of the report object and of the report signal's listener.
const REPORT: &str = "\0node/process/report";
const SIGNAL_LISTENER: &str = "\0node/process/report/signal";

fn cached(context: &mut Context<'_>, key: &str, build: fn(&mut Context<'_>) -> Result<RootId, RootedError>) -> Result<RootId, RootedError> {
    let state = context.host_mut().shared_state();
    if let Some(value) = state.borrow().builtin_cache.get(key).copied() {
        return Ok(value);
    }
    let value = build(context)?;
    let value = context.retain(value)?;
    state.borrow_mut().builtin_cache.insert(key.into(), value);
    Ok(value)
}

/// The `process.report` getter.
pub(crate) fn report(context: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    cached(context, REPORT, build_report)
}

fn build_report(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let report = context.object_rooted()?;
    for (name, operation) in [("writeReport", "reportWrite"), ("getReport", "reportGet")] {
        let function = context.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        put(context, report, name, function)?;
    }
    for (index, setting) in SETTINGS.iter().enumerate() {
        let data = context.number(index as f64);
        let get = context.host_function_named_with_data(
            crate::host::shared_vm::operation("reportConfigGet"),
            &format!("get {}", setting.property),
            data,
        )?;
        let set = context.host_function_named_with_data(
            crate::host::shared_vm::operation("reportPropertySet"),
            &format!("set {}", setting.property),
            data,
        )?;
        accessor(context, report, setting.property, get, Some(set))?;
    }
    Ok(report)
}

pub(crate) fn binding(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(context, "\0binding:report", create_binding)
}
fn create_binding(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    for (name, operation) in [("writeReport", "reportBindingWrite"), ("getReport", "reportBindingGet")] {
        let function = context.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        put(context, module, name, function)?;
    }
    for (index, setting) in SETTINGS.iter().enumerate() {
        let data = context.number(index as f64);
        for (name, operation) in [(setting.get, "reportConfigGet"), (setting.set, "reportConfigSet")] {
            let function = context.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, data)?;
            put(context, module, name, function)?;
        }
    }
    Ok(module)
}

fn setting_index(context: &mut Context<'_>) -> Result<usize, RootedError> {
    let data = context.host_function_data()?;
    context
        .rooted_value(data)
        .and_then(Value::as_number)
        .map(|index| index as usize)
        .filter(|index| *index < SETTINGS.len())
        .ok_or_else(|| RootedError::host("report configuration capture is invalid"))
}
fn argument(context: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| context.undefined())
}

pub(crate) fn get(context: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let index = setting_index(context)?;
    Ok(match context.host_mut().shared_state().borrow().report_config.values[index].clone() {
        SettingValue::Boolean(value) => context.boolean(value),
        SettingValue::String(value) => context.string_rooted(&value),
    })
}

/// The binding's setter: the value as its kind stores it.
pub(crate) fn set(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let index = setting_index(context)?;
    let value = argument(context, args, 0);
    let parsed = match SETTINGS[index].kind {
        Kind::Boolean => context.rooted_value(value).and_then(Value::as_bool).map(SettingValue::Boolean),
        Kind::Truthiness => Some(SettingValue::Boolean(context.truthy_rooted(value)?)),
        Kind::String | Kind::Signal => context.string_text(value)?.map(SettingValue::String),
    }
    .ok_or_else(|| RootedError::host("invalid native report setting value"))?;
    store(context, index, parsed);
    Ok(context.undefined())
}

/// A `process.report` setter: validates as lib/internal/process/report.js does, and
/// moves the report signal's listener when the signal or `reportOnSignal` changes.
pub(crate) fn set_property(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let index = setting_index(context)?;
    let setting = &SETTINGS[index];
    let value = argument(context, args, 0);
    let parsed = match setting.kind {
        Kind::Boolean | Kind::Truthiness => context.rooted_value(value).and_then(Value::as_bool).map(SettingValue::Boolean),
        Kind::String | Kind::Signal => context.string_text(value)?.map(SettingValue::String),
    };
    let expected = match setting.kind {
        Kind::Boolean | Kind::Truthiness => "boolean",
        Kind::String | Kind::Signal => "string",
    };
    let Some(parsed) = parsed else {
        return Err(super::util_shared_vm::argument_type_error(context, value, setting.argument, expected)?);
    };
    match (setting.kind, &parsed) {
        (Kind::Signal, SettingValue::String(name)) => {
            validate_signal(context, name)?;
            remove_signal_listener(context)?;
            add_signal_listener(context, Some(name))?;
            store(context, index, parsed);
        }
        _ if setting.option == REPORT_ON_SIGNAL => {
            store(context, index, parsed);
            remove_signal_listener(context)?;
            add_signal_listener(context, None)?;
        }
        _ => store(context, index, parsed),
    }
    Ok(context.undefined())
}

/// `validateSignalName`: `ERR_UNKNOWN_SIGNAL`, hinting at capitals for a lowercase name.
fn validate_signal(context: &mut Context<'_>, name: &str) -> Result<(), RootedError> {
    if super::os_binding_shared_vm::SIGNAL_CONSTANTS.iter().any(|(signal, _)| *signal == name) {
        return Ok(());
    }
    let upper = name.to_uppercase();
    let hint = if super::os_binding_shared_vm::SIGNAL_CONSTANTS.iter().any(|(signal, _)| *signal == upper) {
        " (signals must use all capital letters)"
    } else {
        ""
    };
    let error = context.type_error_rooted(&format!("Unknown signal: {name}{hint}"))?;
    super::node_errors_shared_vm::decorate_type_error(context, error, "ERR_UNKNOWN_SIGNAL")?;
    Err(context.throw(error))
}

fn process(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    context
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process is not initialized"))
}

fn build_signal_listener(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    context.host_function_named(crate::host::shared_vm::operation("reportSignal"), "signalHandler")
}

/// `addSignalHandler(sig)`: with `reportOnSignal`, listen for `sig`, or the configured signal.
fn add_signal_listener(context: &mut Context<'_>, signal: Option<&str>) -> Result<(), RootedError> {
    if !flag(context, REPORT_ON_SIGNAL) {
        return Ok(());
    }
    let signal = signal.map_or_else(|| text(context, SIGNAL), str::to_owned);
    listen(context, "on", &signal)
}

/// `removeSignalHandler()`: stop listening for the configured signal.
fn remove_signal_listener(context: &mut Context<'_>) -> Result<(), RootedError> {
    let signal = text(context, SIGNAL);
    if signal.is_empty() {
        return Ok(());
    }
    listen(context, "removeListener", &signal)
}

fn listen(context: &mut Context<'_>, method: &str, signal: &str) -> Result<(), RootedError> {
    let process = process(context)?;
    let listener = cached(context, SIGNAL_LISTENER, build_signal_listener)?;
    let function = context.get_named_property_rooted(process, method)?;
    let signal = context.string_rooted(signal);
    context.call_rooted(function, process, &[signal, listener])?;
    Ok(())
}

/// With `--report-on-signal`, the report signal's listener is installed before user code.
pub(crate) fn initialize_signal(context: &mut Context<'_>) -> Result<(), RootedError> {
    add_signal_listener(context, None)
}

/// The report signal's listener: a report for the signal it received.
pub(crate) fn signal_report(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let signal = argument(context, args, 0);
    let signal = context.to_string(signal)?;
    write(context, &signal, Trigger::Signal, None, None)?;
    Ok(context.undefined())
}

pub(crate) fn report_on_uncaught_exception(context: &mut Context<'_>) -> bool {
    flag(context, REPORT_ON_UNCAUGHT_EXCEPTION)
}

pub(crate) fn set_report_on_uncaught_exception(context: &mut Context<'_>, enabled: bool) {
    store(context, setting_position(REPORT_ON_UNCAUGHT_EXCEPTION), SettingValue::Boolean(enabled));
}

/// What made a report (`header.trigger`).
#[derive(Clone, Copy)]
enum Trigger {
    GetReport,
    Api,
    Signal,
}
impl Trigger {
    fn name(self) -> &'static str {
        match self {
            Self::GetReport => "GetReport",
            Self::Api => "API",
            Self::Signal => "Signal",
        }
    }
}
/// The event a script's report names (`header.event`).
const API_EVENT: &str = "JavaScript API";

/// `ERR_SYNTHETIC`, the error a report without one describes the stack of.
fn synthetic_error(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let error = context.error_rooted("JavaScript Callstack")?;
    super::node_errors_shared_vm::decorate_error(context, error, "ERR_SYNTHETIC")?;
    Ok(error)
}

/// `validateObject(err, 'err')`, or `ERR_SYNTHETIC` when there is none.
fn report_error(context: &mut Context<'_>, error: RootId) -> Result<RootId, RootedError> {
    if context.rooted_value(error).is_some_and(|value| value.is_undefined()) {
        return synthetic_error(context);
    }
    if !context.is_object_rooted(error)? || context.is_callable_rooted(error)? || context.is_array_rooted(error)? {
        return Err(super::util_shared_vm::argument_type_error(context, error, "err", "object")?);
    }
    Ok(error)
}

/// `process.report.getReport([err])`.
pub(crate) fn get_report(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let error = argument(context, args, 0);
    let error = report_error(context, error)?;
    document(context, API_EVENT, Trigger::GetReport, None, Some(error))
}

/// The binding's `getReport(err)`: the report's JSON text.
pub(crate) fn binding_get_report(context: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let document = get_report(context, receiver, args)?;
    context.stringify_json_rooted(document)
}

/// `process.report.writeReport([file][, err])`: the name of the file written.
pub(crate) fn write_report(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (mut file, mut error) = (argument(context, args, 0), argument(context, args, 1));
    if context.is_object_rooted(file)? {
        error = file;
        file = context.undefined();
    }
    let file = if context.rooted_value(file).is_some_and(|value| value.is_undefined()) {
        None
    } else {
        let Some(name) = context.string_text(file)? else {
            return Err(super::util_shared_vm::argument_type_error(context, file, "file", "string")?);
        };
        if name.contains('\0') {
            return Err(super::util_shared_vm::argument_value_error(
                context,
                file,
                "path",
                "must be a string, Uint8Array, or URL without null bytes",
            )?);
        }
        Some(name)
    };
    let error = report_error(context, error)?;
    let name = write(context, API_EVENT, Trigger::Api, file, Some(error))?;
    Ok(context.string_rooted(&name))
}

/// The binding's `writeReport(event, trigger, file, err)`.
pub(crate) fn binding_write_report(context: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let file = argument(context, args, 2);
    let error = argument(context, args, 3);
    write_report(context, receiver, &[file, error])
}

/// The special report filenames that name the process's streams.
const STDOUT: &str = "stdout";
const STDERR: &str = "stderr";

/// Writes a report to `file`, the configured filename, or a generated name, in the report
/// directory or the working directory: returns the name.
fn write(context: &mut Context<'_>, event: &str, trigger: Trigger, file: Option<String>, error: Option<RootId>) -> Result<String, RootedError> {
    let configured = text(context, FILENAME);
    let name = match file {
        Some(file) => file,
        None if !configured.is_empty() => configured,
        None => generated_name(context),
    };
    let document = document(context, event, trigger, Some(&name), error)?;
    let json = context.stringify_json_rooted(document)?;
    let json = context.to_string(json)?;
    let json = if flag(context, COMPACT) {
        json
    } else {
        let parsed: serde_json::Value = serde_json::from_str(&json).map_err(|error| RootedError::host(error.to_string()))?;
        serde_json::to_string_pretty(&parsed).map_err(|error| RootedError::host(error.to_string()))?
    };
    match name.as_str() {
        STDOUT => println!("{json}"),
        STDERR => eprintln!("{json}"),
        _ => {
            let directory = std::path::PathBuf::from(text(context, DIRECTORY));
            let path = context.host_mut().shared_state().borrow().cwd.resolve_io_path(&directory.join(&name));
            eprintln!("\nWriting Node.js report to file: {name}");
            if let Err(failure) = std::fs::write(&path, format!("{json}\n")) {
                eprintln!("\nFailed to open Node.js report file: {name} directory: {} (errno: {})", directory.display(), failure.raw_os_error().unwrap_or_default());
                return Ok(name);
            }
            eprintln!("Node.js report completed");
        }
    }
    Ok(name)
}

/// `report.YYYYMMDD.HHMMSS.<pid>.<thread>.<sequence>.json`.
fn generated_name(context: &mut Context<'_>) -> String {
    let sequence = {
        let state = context.host_mut().shared_state();
        let mut state = state.borrow_mut();
        state.report_config.sequence += 1;
        state.report_config.sequence
    };
    let time = LocalTime::now();
    format!(
        "report.{:04}{:02}{:02}.{:02}{:02}{:02}.{}.{}.{sequence:03}.json",
        time.year, time.month, time.day, time.hour, time.minute, time.second,
        std::process::id(),
        thread_id(context),
    )
}

fn thread_id(context: &mut Context<'_>) -> u64 {
    context.host_mut().shared_state().borrow().messaging.context.as_ref().map_or(0, |worker| worker.thread_id)
}

/// The wall clock as local time, with the epoch milliseconds it was read at.
struct LocalTime {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    epoch_millis: u128,
}
impl LocalTime {
    fn now() -> Self {
        let elapsed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        let seconds = elapsed.as_secs() as libc::time_t;
        // SAFETY: localtime_r fills the zeroed tm it is given.
        let mut tm = unsafe { std::mem::zeroed::<libc::tm>() };
        unsafe { libc::localtime_r(&seconds, &mut tm) };
        const TM_YEAR_BASE: i32 = 1900;
        Self {
            year: tm.tm_year + TM_YEAR_BASE,
            month: (tm.tm_mon + 1) as u32,
            day: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
            second: tm.tm_sec as u32,
            epoch_millis: elapsed.as_millis(),
        }
    }
}

/// src/node_report.cc's `NODE_REPORT_VERSION`.
const REPORT_VERSION: f64 = 5.0;

/// The report document, as plain objects.
fn document(context: &mut Context<'_>, event: &str, trigger: Trigger, filename: Option<&str>, error: Option<RootId>) -> Result<RootId, RootedError> {
    let report = context.object_rooted()?;
    let header = header(context, event, trigger, filename)?;
    put(context, report, "header", header)?;
    let stack = javascript_stack(context, error)?;
    put(context, report, "javascriptStack", stack)?;
    let heap = javascript_heap(context)?;
    put(context, report, "javascriptHeap", heap)?;
    let native_stack = context.array_rooted(&[])?;
    put(context, report, "nativeStack", native_stack)?;
    let usage = resource_usage(context)?;
    put(context, report, "resourceUsage", usage)?;
    let thread = rusage(super::process_metrics_shared_vm::thread_rusage())?;
    let thread = cpu_usage(context, &thread)?;
    put(context, report, "uvthreadResourceUsage", thread)?;
    for name in ["libuv", "workers"] {
        let empty = context.array_rooted(&[])?;
        put(context, report, name, empty)?;
    }
    if !flag(context, EXCLUDE_ENV) {
        let environment = environment(context)?;
        put(context, report, "environmentVariables", environment)?;
    }
    let limits = user_limits(context)?;
    put(context, report, "userLimits", limits)?;
    let names: Vec<RootId> = shared_objects().iter().map(|name| context.string_rooted(name)).collect();
    let objects = context.array_rooted(&names)?;
    put(context, report, "sharedObjects", objects)?;
    Ok(report)
}

fn string(context: &mut Context<'_>, object: RootId, name: &str, value: &str) -> Result<(), RootedError> {
    let value = context.string_rooted(value);
    put(context, object, name, value)
}
fn number(context: &mut Context<'_>, object: RootId, name: &str, value: f64) -> Result<(), RootedError> {
    let value = context.number(value);
    put(context, object, name, value)
}

fn header(context: &mut Context<'_>, event: &str, trigger: Trigger, filename: Option<&str>) -> Result<RootId, RootedError> {
    let header = context.object_rooted()?;
    number(context, header, "reportVersion", REPORT_VERSION)?;
    string(context, header, "event", event)?;
    string(context, header, "trigger", trigger.name())?;
    let filename = match filename {
        Some(name) => context.string_rooted(name),
        None => context.null(),
    };
    put(context, header, "filename", filename)?;
    let time = LocalTime::now();
    let stamp = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        time.year, time.month, time.day, time.hour, time.minute, time.second
    );
    string(context, header, "dumpEventTime", &stamp)?;
    string(context, header, "dumpEventTimeStamp", &time.epoch_millis.to_string())?;
    number(context, header, "processId", f64::from(std::process::id()))?;
    let thread = thread_id(context);
    number(context, header, "threadId", thread as f64)?;
    let cwd = context.host_mut().shared_state().borrow().cwd.path();
    string(context, header, "cwd", &cwd.to_string_lossy())?;
    let command_line: Vec<RootId> = std::env::args().map(|argument| context.string_rooted(&argument)).collect();
    let command_line = context.array_rooted(&command_line)?;
    put(context, header, "commandLine", command_line)?;
    string(context, header, "nodejsVersion", &format!("v{}", super::process_state::NODE_VERSION))?;
    if let Some(version) = glibc_version() {
        string(context, header, "glibcVersionRuntime", &version)?;
    }
    number(context, header, "wordSize", f64::from(usize::BITS))?;
    string(context, header, "arch", super::process_state::architecture())?;
    string(context, header, "platform", &super::process_state::platform())?;
    let versions = context.object_rooted()?;
    for (name, version) in super::process_state::version_facts() {
        string(context, versions, name, version)?;
    }
    put(context, header, "componentVersions", versions)?;
    let release = context.object_rooted()?;
    for member in RELEASE_ORDER {
        let (name, value) = crate::host::shared_vm::commonjs::RELEASE_MEMBERS
            .into_iter()
            .find(|(name, _)| *name == member)
            .expect("listed release member");
        string(context, release, name, value)?;
    }
    put(context, header, "release", release)?;
    let uname = super::os_shared_vm::checked(context, super::os_binding_shared_vm::information, &[])?;
    for (name, index) in UNAME_MEMBERS {
        let value = context.get_named_property_rooted(uname, &index.to_string())?;
        put(context, header, name, value)?;
    }
    let cpus = cpus(context)?;
    put(context, header, "cpus", cpus)?;
    if !flag(context, EXCLUDE_NETWORK) {
        let interfaces = network_interfaces(context)?;
        put(context, header, "networkInterfaces", interfaces)?;
    }
    let host = super::os_shared_vm::checked(context, super::os_binding_shared_vm::hostname, &[])?;
    put(context, header, "host", host)?;
    Ok(header)
}

/// `process.release`'s members in the report's order.
const RELEASE_ORDER: [&str; 3] = ["name", "headersUrl", "sourceUrl"];
/// The report's OS members and their positions in the binding's `uname` record
/// (`[sysname, version, release, machine]`).
const UNAME_MEMBERS: [(&str, usize); 4] = [("osName", 0), ("osRelease", 2), ("osVersion", 1), ("osMachine", 3)];

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn glibc_version() -> Option<String> {
    // SAFETY: glibc returns a static NUL-terminated version string.
    let version = unsafe { std::ffi::CStr::from_ptr(libc::gnu_get_libc_version()) };
    Some(version.to_string_lossy().into_owned())
}
#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn glibc_version() -> Option<String> {
    None
}

/// `{ model, speed, user, nice, sys, idle, irq }` per CPU.
fn cpus(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let mut records = Vec::new();
    for cpu in super::os_facts::cpus() {
        let record = context.object_rooted()?;
        string(context, record, "model", &cpu.model)?;
        for (name, value) in [
            ("speed", cpu.speed),
            ("user", cpu.times.user),
            ("nice", cpu.times.nice),
            ("sys", cpu.times.sys),
            ("idle", cpu.times.idle),
            ("irq", cpu.times.irq),
        ] {
            number(context, record, name, value as f64)?;
        }
        records.push(record);
    }
    context.array_rooted(&records)
}

/// `{ name, internal, mac, address, netmask, family[, scopeid] }` per interface address.
fn network_interfaces(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let fields = super::os_shared_vm::checked(context, super::os_binding_shared_vm::interfaces, &[])?;
    let fields = context.array_like_values_rooted(fields)?;
    let mut records = Vec::new();
    for [name, address, netmask, family, mac, internal, scope] in fields.as_chunks::<{ super::os_shared_vm::INTERFACE_FIELDS }>().0 {
        let record = context.object_rooted()?;
        for (key, value) in [("name", *name), ("internal", *internal), ("mac", *mac), ("address", *address), ("netmask", *netmask), ("family", *family)] {
            put(context, record, key, value)?;
        }
        if context.to_number(*scope)? != super::os_shared_vm::NO_SCOPE {
            put(context, record, "scopeid", *scope)?;
        }
        records.push(record);
    }
    context.array_rooted(&records)
}

/// The report's description of `error`'s stack: its first line, the following lines but
/// the last, and its other enumerable own properties as strings.
fn javascript_stack(context: &mut Context<'_>, error: Option<RootId>) -> Result<RootId, RootedError> {
    let result = context.object_rooted()?;
    let stack = match error {
        Some(error) => context.get_named_property_rooted(error, "stack")?,
        None => context.undefined(),
    };
    let stack = context.string_text(stack)?.filter(|stack| !stack.is_empty());
    match stack {
        None => {
            string(context, result, "message", "No stack.")?;
            let unavailable = context.string_rooted("Unavailable.");
            let lines = context.array_rooted(&[unavailable])?;
            put(context, result, "stack", lines)?;
        }
        Some(stack) => match stack.split_once('\n') {
            None => string(context, result, "message", &stack)?,
            Some((message, rest)) => {
                string(context, result, "message", message)?;
                let mut lines = Vec::new();
                let mut rest = rest;
                while let Some((line, next)) = rest.split_once('\n') {
                    lines.push(context.string_rooted(line.trim_start()));
                    rest = next;
                }
                let lines = context.array_rooted(&lines)?;
                put(context, result, "stack", lines)?;
            }
        },
    }
    let properties = context.object_rooted()?;
    if let Some(error) = error {
        for key in context.enumerable_own_keys_rooted(error, false, true)? {
            let name = context.to_string(key)?;
            if matches!(name.as_str(), "stack" | "message") {
                continue;
            }
            let value = context.get_property_rooted(error, key)?;
            let value = context.to_string(value)?;
            string(context, properties, &name, &value)?;
        }
    }
    put(context, result, "errorProperties", properties)?;
    Ok(result)
}

/// Report heap members and the `getHeapStatistics()` members they report.
const HEAP_MEMBERS: [(&str, &str); 14] = [
    ("totalMemory", "total_heap_size"),
    ("executableMemory", "total_heap_size_executable"),
    ("totalCommittedMemory", "total_physical_size"),
    ("availableMemory", "total_available_size"),
    ("totalGlobalHandlesMemory", "total_global_handles_size"),
    ("usedGlobalHandlesMemory", "used_global_handles_size"),
    ("usedMemory", "used_heap_size"),
    ("memoryLimit", "heap_size_limit"),
    ("mallocedMemory", "malloced_memory"),
    ("externalMemory", "external_memory"),
    ("peakMallocedMemory", "peak_malloced_memory"),
    ("nativeContextCount", "number_of_native_contexts"),
    ("detachedContextCount", "number_of_detached_contexts"),
    ("doesZapGarbage", "does_zap_garbage"),
];

fn javascript_heap(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let statistics = super::v8_shared_vm::get_heap_statistics(context, undefined, &[])?;
    let heap = context.object_rooted()?;
    for (member, statistic) in HEAP_MEMBERS {
        let value = context.get_named_property_rooted(statistics, statistic)?;
        put(context, heap, member, value)?;
    }
    let spaces = super::v8_shared_vm::get_heap_space_statistics(context, undefined, &[])?;
    let report_spaces = context.object_rooted()?;
    for space in context.array_like_values_rooted(spaces)? {
        let read = |context: &mut Context<'_>, name: &str| -> Result<f64, RootedError> {
            let value = context.get_named_property_rooted(space, name)?;
            context.to_number(value)
        };
        let [size, used, available, physical] =
            ["space_size", "space_used_size", "space_available_size", "physical_space_size"].map(|name| read(context, name));
        let (used, available) = (used?, available?);
        let record = context.object_rooted()?;
        for (name, value) in [
            ("memorySize", size?),
            ("committedMemory", physical?),
            ("capacity", used + available),
            ("used", used),
            ("available", available),
        ] {
            number(context, record, name, value)?;
        }
        let name = context.get_named_property_rooted(space, "space_name")?;
        let name = context.to_string(name)?;
        put(context, report_spaces, &name, record)?;
    }
    put(context, heap, "heapSpaces", report_spaces)?;
    Ok(heap)
}

/// What `getrusage` reports, for the process or its calling thread.
struct Usage {
    user_seconds: f64,
    kernel_seconds: f64,
    max_rss_bytes: f64,
    major_faults: f64,
    minor_faults: f64,
    reads: f64,
    writes: f64,
}
/// `ru_maxrss` is in kibibytes.
const RSS_UNIT_BYTES: f64 = 1024.0;
const MICROS_PER_SECOND: f64 = 1_000_000.0;
const PERCENT: f64 = 100.0;

fn rusage(who: libc::c_int) -> Result<Usage, RootedError> {
    // SAFETY: getrusage fills the zeroed rusage it is given.
    let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
    if unsafe { libc::getrusage(who, &mut usage) } != 0 {
        return Err(RootedError::host(std::io::Error::last_os_error().to_string()));
    }
    let seconds = |time: libc::timeval| time.tv_sec as f64 + time.tv_usec as f64 / MICROS_PER_SECOND;
    Ok(Usage {
        user_seconds: seconds(usage.ru_utime),
        kernel_seconds: seconds(usage.ru_stime),
        max_rss_bytes: usage.ru_maxrss as f64 * RSS_UNIT_BYTES,
        major_faults: usage.ru_majflt as f64,
        minor_faults: usage.ru_minflt as f64,
        reads: usage.ru_inblock as f64,
        writes: usage.ru_oublock as f64,
    })
}

/// CPU seconds and their share of the process's uptime, with file system activity.
fn cpu_usage(context: &mut Context<'_>, usage: &Usage) -> Result<RootId, RootedError> {
    let uptime = context.host_mut().shared_state().borrow().process_control.uptime();
    let share = |seconds: f64| if uptime > 0.0 { seconds / uptime * PERCENT } else { 0.0 };
    let record = context.object_rooted()?;
    for (name, value) in [
        ("userCpuSeconds", usage.user_seconds),
        ("kernelCpuSeconds", usage.kernel_seconds),
        ("cpuConsumptionPercent", share(usage.user_seconds + usage.kernel_seconds)),
        ("userCpuConsumptionPercent", share(usage.user_seconds)),
        ("kernelCpuConsumptionPercent", share(usage.kernel_seconds)),
    ] {
        number(context, record, name, value)?;
    }
    let activity = context.object_rooted()?;
    number(context, activity, "reads", usage.reads)?;
    number(context, activity, "writes", usage.writes)?;
    put(context, record, "fsActivity", activity)?;
    Ok(record)
}

fn resource_usage(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let record = context.object_rooted()?;
    let rss = super::process_metrics_shared_vm::resident_bytes()?;
    for (name, value) in [
        ("free_memory", super::os_facts::free_memory_bytes() as f64),
        ("total_memory", super::os_facts::total_memory_bytes() as f64),
        ("rss", rss),
        ("constrained_memory", super::memory_facts::constrained() as f64),
        ("available_memory", super::memory_facts::available() as f64),
    ] {
        number(context, record, name, value)?;
    }
    let usage = rusage(libc::RUSAGE_SELF)?;
    let cpu = cpu_usage(context, &usage)?;
    let activity = context.get_named_property_rooted(cpu, "fsActivity")?;
    for name in ["userCpuSeconds", "kernelCpuSeconds", "cpuConsumptionPercent", "userCpuConsumptionPercent", "kernelCpuConsumptionPercent"] {
        let value = context.get_named_property_rooted(cpu, name)?;
        put(context, record, name, value)?;
    }
    number(context, record, "maxRss", usage.max_rss_bytes)?;
    let faults = context.object_rooted()?;
    number(context, faults, "IORequired", usage.major_faults)?;
    number(context, faults, "IONotRequired", usage.minor_faults)?;
    put(context, record, "pageFaults", faults)?;
    put(context, record, "fsActivity", activity)?;
    Ok(record)
}

/// `process.env` as the script sees it.
fn environment(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let process = process(context)?;
    let env = context.get_named_property_rooted(process, "env")?;
    let result = context.object_rooted()?;
    for key in context.enumerable_own_keys_rooted(env, false, true)? {
        let value = context.get_property_rooted(env, key)?;
        let value = context.to_string(value)?;
        let name = context.to_string(key)?;
        string(context, result, &name, &value)?;
    }
    Ok(result)
}

/// The report's resource limits, as src/node_report.cc names them.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
type Resource = libc::__rlimit_resource_t;
#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
type Resource = libc::c_int;
const USER_LIMITS: [(&str, Resource); 10] = [
    ("core_file_size_blocks", libc::RLIMIT_CORE),
    ("data_seg_size_bytes", libc::RLIMIT_DATA),
    ("file_size_blocks", libc::RLIMIT_FSIZE),
    ("max_locked_memory_bytes", libc::RLIMIT_MEMLOCK),
    ("max_memory_size_bytes", libc::RLIMIT_RSS),
    ("open_files", libc::RLIMIT_NOFILE),
    ("stack_size_bytes", libc::RLIMIT_STACK),
    ("cpu_time_seconds", libc::RLIMIT_CPU),
    ("max_user_processes", libc::RLIMIT_NPROC),
    ("virtual_memory_bytes", libc::RLIMIT_AS),
];
const UNLIMITED: &str = "unlimited";

fn user_limits(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let result = context.object_rooted()?;
    for (name, resource) in USER_LIMITS {
        // SAFETY: getrlimit fills the zeroed rlimit it is given.
        let mut limit = unsafe { std::mem::zeroed::<libc::rlimit>() };
        if unsafe { libc::getrlimit(resource, &mut limit) } != 0 {
            continue;
        }
        let record = context.object_rooted()?;
        for (field, value) in [("soft", limit.rlim_cur), ("hard", limit.rlim_max)] {
            let value = if value == libc::RLIM_INFINITY {
                context.string_rooted(UNLIMITED)
            } else {
                context.number(value as f64)
            };
            put(context, record, field, value)?;
        }
        put(context, result, name, record)?;
    }
    Ok(result)
}

/// The shared objects loaded into the process, by the names the loader knows them by.
#[cfg(target_os = "linux")]
fn shared_objects() -> Vec<String> {
    unsafe extern "C" fn visit(info: *mut libc::dl_phdr_info, _: usize, data: *mut libc::c_void) -> libc::c_int {
        // SAFETY: the loader passes a valid record, and `data` is the vector below.
        let (info, names) = unsafe { (&*info, &mut *data.cast::<Vec<String>>()) };
        if !info.dlpi_name.is_null() {
            let name = unsafe { std::ffi::CStr::from_ptr(info.dlpi_name) }.to_string_lossy();
            if !name.is_empty() {
                names.push(name.into_owned());
            }
        }
        0
    }
    let mut names = Vec::new();
    // SAFETY: `visit` only reads the loader's records and appends to `names`.
    unsafe { libc::dl_iterate_phdr(Some(visit), (&mut names as *mut Vec<String>).cast()) };
    names
}
#[cfg(not(target_os = "linux"))]
fn shared_objects() -> Vec<String> {
    Vec::new()
}
