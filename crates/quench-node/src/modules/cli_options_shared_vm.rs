//! Native option queries project the same startup state used by feature gates.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
const CACHE_KEY: &str = "\0native:options";
pub(crate) fn binding(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    if let Some(root) = shared.borrow().builtin_cache.get(CACHE_KEY).copied() {
        return Ok(root);
    }
    let module = context.object_rooted()?;
    for (name, operation) in [
        ("getCLIOptionsValues", "cliOptionsValues"),
        ("getCLIOptionsInfo", "cliOptionsInfo"),
        ("getOptionsAsFlags", "cliOptionsFlags"),
        ("getEmbedderOptions", "cliEmbedderOptions"),
        ("parseNodeOptionsEnvVar", "cliParseNodeOptions"),
    ] {
        let function =
            context.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(module, key, function, true, true, true)?;
    }
    let root = context.retain(module)?;
    shared
        .borrow_mut()
        .builtin_cache
        .insert(CACHE_KEY.into(), root);
    Ok(module)
}
pub(crate) fn info(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    // Node's benchmark runner uses CLI option metadata to distinguish flags
    // that require a value from boolean flags. Preserve the shape and types
    // needed by that validation path, including the -r alias for --require.
    let factory = context.evaluate_script_rooted(
        r#"(() => {
          const options = new Map([
            ["--bench", { type: 2 }],
            ["--experimental-bench", { type: 2 }],
            ["--bench-name-pattern", { type: 5 }],
            ["--require", { type: 5 }],
          ]);
          const aliases = new Map([["-r", ["--require"]]]);
          return { options, aliases };
        })()"#,
        "internal/options/cli-info.js",
    )?;
    Ok(factory)
}
pub(crate) fn flags(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let (mut flags, exec_argv) = {
        let shared = context.host_mut().shared_state();
        let shared = shared.borrow();
        (Vec::new(), shared.exec_argv.clone())
    };
    if let Ok(node_options) = std::env::var("NODE_OPTIONS") {
        if let Ok(arguments) = crate::modules::cli_options::parse_node_options(&node_options) {
            flags.extend(arguments);
        }
    }
    append_env_file_options(&exec_argv, &mut flags);
    append_config_file_options(&exec_argv, &mut flags);
    flags.extend(exec_argv);
    let values = flags
        .iter()
        .map(|flag| context.string_rooted(flag))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

fn option_file_paths(arguments: &[String], option: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        if let Some(path) = arguments[index].strip_prefix(&format!("{option}=")) {
            paths.push(path.to_owned());
        } else if arguments[index] == option {
            if let Some(path) = arguments.get(index + 1) {
                paths.push(path.clone());
                index += 1;
            }
        }
        index += 1;
    }
    paths
}

fn append_env_file_options(arguments: &[String], flags: &mut Vec<String>) {
    if std::env::var_os("NODE_OPTIONS").is_some() {
        return;
    }
    for path in option_file_paths(arguments, "--env-file") {
        let Ok(contents) = std::fs::read_to_string(path) else {
            continue;
        };
        let values = crate::modules::util_env::parse_env_file(&contents);
        if let Some(node_options) = values.get("NODE_OPTIONS") {
            if let Ok(arguments) = crate::modules::cli_options::parse_node_options(node_options) {
                flags.extend(arguments);
            }
        }
    }
}

fn append_config_file_options(arguments: &[String], flags: &mut Vec<String>) {
    for path in option_file_paths(arguments, "--config-file") {
        let Ok(contents) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(config) = serde_json::from_str::<serde_json::Value>(&contents) else {
            continue;
        };
        let Some(config) = config.as_object() else {
            continue;
        };
        for options in config.values() {
            let Some(options) = options.as_object() else {
                continue;
            };
            for (name, value) in options {
                match value {
                    serde_json::Value::Bool(true) => flags.push(format!("--{name}")),
                    serde_json::Value::Bool(false) | serde_json::Value::Null => {}
                    serde_json::Value::Array(values) => {
                        for value in values {
                            if let Some(value) = config_value_as_string(value) {
                                flags.push(format!("--{name}={value}"));
                            }
                        }
                    }
                    value => {
                        if let Some(value) = config_value_as_string(value) {
                            flags.push(format!("--{name}={value}"));
                        }
                    }
                }
            }
        }
    }
}

fn config_value_as_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}
pub(crate) fn embedder_options(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    // Quench runs without Node's embedders, so all embedder-specific switches
    // are false. Node's startup modules still query this binding during
    // bootstrap, including the benchmark runner.
    let result = context.null_object_rooted()?;
    for name in [
        "shouldNotRegisterESMLoader",
        "noGlobalSearchPaths",
        "noBrowserGlobals",
        "hasEmbedderPreload",
    ] {
        let key = context.string_rooted(name);
        let value = context.boolean(false);
        context.define_data_property_rooted(result, key, value, true, true, true)?;
    }
    Ok(result)
}
pub(crate) fn values(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let options = context
        .host_mut()
        .shared_state()
        .borrow()
        .cli_options
        .clone();
    let result = context.null_object_rooted()?;
    for (name, enabled) in options.values() {
        for (name, enabled) in [
            (name.to_owned(), enabled),
            (
                format!("--no-{}", name.strip_prefix("--").expect("long option")),
                !enabled,
            ),
        ] {
            let key = context.string_rooted(&name);
            let value = context.boolean(enabled);
            context.define_data_property_rooted(result, key, value, true, true, true)?;
        }
    }
    for (name, value) in options.strings() {
        let key = context.string_rooted(name);
        let value = context.string_rooted(value);
        context.define_data_property_rooted(result, key, value, true, true, true)?;
    }
    for (name, value) in options.numbers() {
        let key = context.string_rooted(name);
        let value = match value.parse::<f64>() {
            Ok(value) => context.number(value),
            Err(_) => context.string_rooted(value),
        };
        context.define_data_property_rooted(result, key, value, true, true, true)?;
    }
    let key = context.string_rooted("[has_test_random_seed]");
    let value = context.boolean(options.was_provided("--test-random-seed"));
    context.define_data_property_rooted(result, key, value, true, true, true)?;
    for (name, values) in options.repeated_values() {
        let values = values
            .iter()
            .map(|value| context.string_rooted(value))
            .collect::<Vec<_>>();
        let array = context.array_rooted(&values)?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(result, key, array, true, true, true)?;
    }
    Ok(result)
}
pub(crate) fn parse_environment(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let text = context.to_string(value)?;
    match super::cli_options::parse_node_options(&text) {
        Ok(arguments) => {
            let arguments = arguments
                .iter()
                .map(|arg| context.string_rooted(arg))
                .collect::<Vec<_>>();
            context.array_rooted(&arguments)
        }
        Err(message) => {
            let error = context.error_rooted(message)?;
            Err(context.throw(error))
        }
    }
}
