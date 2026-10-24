//! Native diagnostic-report configuration; public policy uses the configured SDK.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, Value};
type Context<'a> = NativeContext<'a, NodeHost>;
#[derive(Clone, Copy)]
enum Kind {
    Boolean,
    Truthiness,
    String,
}
struct Setting {
    get: &'static str,
    set: &'static str,
    option: &'static str,
    kind: Kind,
}
const SETTINGS: &[Setting] = &[
    Setting {
        get: "getCompact",
        set: "setCompact",
        option: "--report-compact",
        kind: Kind::Truthiness,
    },
    Setting {
        get: "getExcludeNetwork",
        set: "setExcludeNetwork",
        option: "--report-exclude-network",
        kind: Kind::Boolean,
    },
    Setting {
        get: "getExcludeEnv",
        set: "setExcludeEnv",
        option: "--report-exclude-env",
        kind: Kind::Boolean,
    },
    Setting {
        get: "getDirectory",
        set: "setDirectory",
        option: "--report-dir",
        kind: Kind::String,
    },
    Setting {
        get: "getFilename",
        set: "setFilename",
        option: "--report-filename",
        kind: Kind::String,
    },
    Setting {
        get: "getSignal",
        set: "setSignal",
        option: "--report-signal",
        kind: Kind::String,
    },
    Setting {
        get: "shouldReportOnFatalError",
        set: "setReportOnFatalError",
        option: "--report-on-fatalerror",
        kind: Kind::Boolean,
    },
    Setting {
        get: "shouldReportOnSignal",
        set: "setReportOnSignal",
        option: "--report-on-signal",
        kind: Kind::Boolean,
    },
    Setting {
        get: "shouldReportOnUncaughtException",
        set: "setReportOnUncaughtException",
        option: "--report-uncaught-exception",
        kind: Kind::Boolean,
    },
];
#[derive(Clone)]
enum SettingValue {
    Boolean(bool),
    String(String),
}
pub(crate) struct State {
    values: Vec<SettingValue>,
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
                    Kind::Boolean | Kind::Truthiness => {
                        SettingValue::Boolean(options.enabled(setting.option))
                    }
                    Kind::String => SettingValue::String(
                        options
                            .string_value(setting.option)
                            .expect("declared report string option")
                            .to_owned(),
                    ),
                })
                .collect(),
        }
    }
}
pub(crate) fn install(context: &mut Context<'_>, process: RootId) -> Result<(), RootedError> {
    let getter =
        context.host_function_named(crate::host::shared_vm::operation("processReport"), "get")?;
    let descriptor = context.null_object_rooted()?;
    for (name, value) in [
        ("get", getter),
        ("enumerable", context.boolean(true)),
        ("configurable", context.boolean(true)),
    ] {
        let key = context.string_rooted(name);
        context.define_data_property_rooted(descriptor, key, value, true, true, true)?;
    }
    let key = context.string_rooted("report");
    context.define_property_descriptor_rooted(process, key, descriptor)?;
    Ok(())
}
pub(crate) fn report(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::node_library_export(
        context,
        "internal/process/pre_execution.js",
        "internal/process/report.js",
        "report",
    )
}
pub(crate) fn binding(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(
        context,
        "\0binding:report",
        create_binding,
    )
}
fn create_binding(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    let get_report = context.host_function_named(
        crate::host::shared_vm::operation("reportBindingGetReport"),
        "getReport",
    )?;
    let key = context.string_rooted("getReport");
    context.define_data_property_rooted(module, key, get_report, true, true, true)?;
    for (index, setting) in SETTINGS.iter().enumerate() {
        let data = context.number(index as f64);
        for (name, operation) in [
            (setting.get, "reportConfigGet"),
            (setting.set, "reportConfigSet"),
        ] {
            let function = context.host_function_named_with_data(
                crate::host::shared_vm::operation(operation),
                name,
                data,
            )?;
            let key = context.string_rooted(name);
            context.define_data_property_rooted(module, key, function, true, true, true)?;
        }
    }
    Ok(module)
}

pub(crate) fn binding_get_report(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    Ok(context.string_rooted(
        r#"{"header":{"event":"JavaScript API","pid":0,"commandLine":[]},"javascriptStack":{"message":""},"resourceUsage":{},"libuv":[],"sharedObjects":[]}"#,
    ))
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
pub(crate) fn get(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let index = setting_index(context)?;
    let value = context
        .host_mut()
        .shared_state()
        .borrow()
        .report_config
        .values[index]
        .clone();
    Ok(match value {
        SettingValue::Boolean(value) => context.boolean(value),
        SettingValue::String(value) => context.string_rooted(&value),
    })
}
pub(crate) fn set(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let index = setting_index(context)?;
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let parsed = match SETTINGS[index].kind {
        Kind::Boolean => context
            .rooted_value(value)
            .and_then(Value::as_bool)
            .map(SettingValue::Boolean),
        Kind::Truthiness => Some(SettingValue::Boolean(context.truthy_rooted(value)?)),
        Kind::String => context.string_text(value)?.map(SettingValue::String),
    }
    .ok_or_else(|| RootedError::host("invalid native report setting value"))?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .report_config
        .values[index] = parsed;
    Ok(context.undefined())
}
pub(crate) fn initialize_signal(context: &mut Context<'_>) -> Result<(), RootedError> {
    if context
        .host_mut()
        .shared_state()
        .borrow()
        .cli_options
        .enabled("--report-on-signal")
    {
        let function = crate::host::shared_vm::commonjs::node_library_export(
            context,
            "internal/process/pre_execution.js",
            "internal/process/report.js",
            "addSignalHandler",
        )?;
        let undefined = context.undefined();
        context.call_rooted(function, undefined, &[])?;
    }
    Ok(())
}
