//! Parsed startup values for implemented Node feature switches.
include!(concat!(env!("OUT_DIR"), "/node_option_defaults.rs"));
const BOOLEAN_OPTIONS: &[(&str, bool)] = &[
    // The default Node condition includes native addon exports. Workers can
    // opt out with --no-addons, which is also a valid boolean negation.
    ("--addons", true),
    // Node exposes the async context-frame switch even when the runtime's
    // AsyncLocalStorage implementation does not depend on V8 context frames.
    ("--async-context-frame", true),
    // These switches are consumed by the unchanged SDK harness even when
    // tests are run directly rather than through the file-runner CLI.
    ("--test-only", false),
    ("--test", false),
    ("--test-force-exit", false),
    ("--test-randomize", false),
    ("--test-update-snapshots", false),
    // The bundled Node 26 test runner gates MockTracker.module() on this
    // experimental startup flag; preserving it also lets child processes
    // inherit the option through process.execArgv.
    ("--experimental-test-module-mocks", false),
    // Node exposes the browser-compatible Worker global when this option is
    // active; bootstrap and WPT child workers need the parsed flag, not only
    // the original spelling in process.execArgv.
    ("--experimental-web-worker", false),
    ("--experimental-vfs", false),
    // Node's pre-execution setup always reads this option and passes its
    // value to source-map support, even when source maps are disabled.
    ("--enable-source-maps", false),
    // The VFS allow-list is part of the permission model and is only valid
    // when permission handling is explicitly enabled.
    ("--allow-fs-vfs", false),
    ("--experimental-bench", false),
    ("--experimental-stream-iter", false),
    // Node enables automatic IPv4/IPv6 connection racing by default and
    // accepts the negated flag to restore serial address attempts.
    ("--network-family-autoselection", true),
    // The Node 26 V8 build accepts this boolean flag, but the embedded V8
    // build may not expose it. Keep it in the CLI compatibility surface so
    // Node test fixtures can pass it through; it has no effect when absent
    // from the engine.
    ("--concurrent-array-buffer-sweeping", true),
    // Node 26 enables syntax detection for eval input by default; accepting
    // this switch is also required for workers inheriting its negated form.
    ("--experimental-detect-module", true),
    // Node accepts both forms of this V8 switch on startup. The embedded V8
    // build does not expose it, but parsing the negative spelling must still
    // succeed for Node's CLI compatibility surface.
    ("--extra-info-on-fatal-exception", false),
    ("--expose-internals", false),
    ("--warnings", true),
    ("--deprecation", true),
    ("--pending-deprecation", false),
    // Permission switches are read during process warning setup and must be
    // available through CliOptions as well as preserved in execArgv.
    ("--permission", false),
    ("--permission-audit", false),
    ("--allow-addons", false),
    ("--allow-child-process", false),
    ("--allow-inspector", false),
    ("--allow-wasi", false),
    ("--allow-worker", false),
    ("--allow-openssl-store", false),
    ("--allow-ffi", false),
    // Trace startup switches are queried by Node's trace and environment
    // bootstrap code as well as retained in process.execArgv.
    ("--trace-events-enabled", false),
    ("--trace-env", false),
    ("--trace-env-js-stack", false),
    ("--trace-env-native-stack", false),
    ("--trace-warnings", false),
    ("--trace-deprecation", false),
    ("--throw-deprecation", false),
    ("--report-compact", false),
    ("--report-exclude-env", false),
    ("--report-exclude-network", false),
    ("--report-on-fatalerror", false),
    ("--report-on-signal", false),
    ("--report-uncaught-exception", false),
    // Node 26 registers these TLS protocol selectors and CA-store selectors
    // as booleans, including their automatically supported --no-* forms.
    ("--tls-min-v1.0", false),
    ("--tls-min-v1.1", false),
    ("--tls-min-v1.2", false),
    ("--tls-min-v1.3", false),
    ("--tls-max-v1.2", false),
    ("--tls-max-v1.3", false),
    ("--use-openssl-ca", false),
    ("--use-system-ca", false),
    ("--use-bundled-ca", false),
];
const STRING_OPTIONS: &[(&str, &str)] = &[
    // Node 26 bootstrap reads this unconditionally; an empty value disables it.
    ("--vfs-load", ""),
    ("--input-type", "commonjs"),
    ("--tls-cipher-list", ""),
    // This is a Node startup mode, not a bare boolean: the selected behavior
    // changes the legacy Object.prototype.__proto__ accessor.
    ("--disable-proto", ""),
    ("--dns-result-order", "verbatim"),
    ("--title", ""),
    ("--report-dir", ""),
    ("--report-filename", ""),
    ("--report-signal", "SIGUSR2"),
    ("--redirect-warnings", ""),
    ("--diagnostic-dir", ""),
    // These are consumed by the CLI test runner. Keeping their values in the
    // startup parser prevents a separated value from being mistaken for the
    // test entry path.
    ("--test-isolation", "process"),
    ("--test-reporter", "spec"),
    ("--test-reporter-destination", ""),
    ("--test-shard", ""),
    ("--test-name-pattern", ""),
    ("--test-skip-pattern", ""),
    ("--test-global-setup", ""),
    ("--test-rerun-failures", ""),
    ("--test-coverage-include", ""),
    ("--test-coverage-exclude", ""),
    ("--experimental-test-tag-filter", ""),
    ("--bench-isolation", "process"),
    ("--bench-name-pattern", ""),
    ("--bench-reporter", "spec"),
    ("--bench-reporter-destination", ""),
    ("--bench-samples", ""),
    ("--bench-warmup", ""),
];
const NUMBER_OPTIONS: &[(&str, &str)] = &[
    ("--test-timeout", "0"),
    ("--test-concurrency", "0"),
    ("--test-random-seed", "0"),
    ("--test-coverage-branches", "0"),
    ("--test-coverage-lines", "0"),
    ("--test-coverage-functions", "0"),
];
const VALUE_OPTIONS: &[&str] = &["--max-http-header-size"];

/// Return the original spelling of the first option Node forbids in
/// `NODE_OPTIONS`. These are command-line modes, not startup settings.
pub(crate) fn disallowed_node_option(environment: &str) -> Option<String> {
    const DISALLOWED: &[&str] = &[
        "--version",
        "-v",
        "--help",
        "-h",
        "--eval",
        "-e",
        "--print",
        "-p",
        "-pe",
        "-ep",
        "--check",
        "-c",
        "--interactive",
        "-i",
        "--v8-options",
        "--",
        "--test",
    ];
    parse_node_options(environment)
        .ok()?
        .into_iter()
        .find(|argument| {
            let flag = argument
                .split_once('=')
                .map_or(argument.as_str(), |(flag, _)| flag)
                .replace('_', "-");
            DISALLOWED.contains(&flag.as_str())
        })
}

fn canonical_option(option: &str) -> &str {
    if option == "--report-directory" {
        "--report-dir"
    } else {
        option
    }
}
pub(crate) fn takes_string_value(option: &str) -> bool {
    STRING_OPTIONS
        .iter()
        .any(|(name, _)| *name == canonical_option(option))
        || NUMBER_OPTIONS
            .iter()
            .any(|(name, _)| *name == canonical_option(option))
        || EMPTY_STRING_LIST_OPTIONS.contains(&canonical_option(option))
}
#[derive(Clone, Debug)]
pub(crate) struct CliOptions {
    booleans: std::collections::BTreeMap<&'static str, bool>,
    strings: std::collections::BTreeMap<&'static str, String>,
    numbers: std::collections::BTreeMap<&'static str, String>,
    provided: std::collections::BTreeSet<String>,
    repeated: std::collections::BTreeMap<&'static str, Vec<String>>,
}
impl Default for CliOptions {
    fn default() -> Self {
        Self {
            booleans: BOOLEAN_OPTIONS.iter().copied().collect(),
            strings: STRING_OPTIONS
                .iter()
                .map(|(name, value)| (*name, (*value).to_owned()))
                .collect(),
            numbers: NUMBER_OPTIONS
                .iter()
                .map(|(name, value)| (*name, (*value).to_owned()))
                .collect(),
            provided: std::collections::BTreeSet::new(),
            repeated: EMPTY_STRING_LIST_OPTIONS
                .iter()
                .map(|name| (*name, Vec::new()))
                .collect(),
        }
    }
}
impl CliOptions {
    pub(crate) fn parse(
        environment: Option<&str>,
        invocation: &[String],
        pending_deprecation: bool,
    ) -> Result<Self, String> {
        let mut options = Self::default();
        options
            .booleans
            .insert("--pending-deprecation", pending_deprecation);
        if let Some(environment) = environment {
            options.apply(&parse_node_options(environment).map_err(str::to_owned)?)?;
        }
        options.apply(invocation)?;
        if options.enabled("--trace-env-js-stack")
            || options.enabled("--trace-env-native-stack")
        {
            options.booleans.insert("--trace-env", true);
        }
        if options.enabled("--tls-min-v1.3") && options.enabled("--tls-max-v1.2") {
            return Err("either --tls-min-v1.3 or --tls-max-v1.2 can be used, not both".to_owned());
        }
        Ok(options)
    }
    pub(crate) fn enabled(&self, name: &str) -> bool {
        self.booleans.get(name).copied().unwrap_or(false)
    }
    pub(crate) fn values(&self) -> impl Iterator<Item = (&'static str, bool)> + '_ {
        self.booleans.iter().map(|(name, value)| (*name, *value))
    }
    pub(crate) fn string_value(&self, name: &str) -> Option<&str> {
        self.strings.get(canonical_option(name)).map(String::as_str)
    }
    pub(crate) fn strings(&self) -> impl Iterator<Item = (&'static str, &str)> + '_ {
        self.strings
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
    }
    pub(crate) fn numbers(&self) -> impl Iterator<Item = (&'static str, &str)> + '_ {
        self.numbers
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
    }
    pub(crate) fn was_provided(&self, name: &str) -> bool {
        self.provided.contains(name)
    }
    pub(crate) fn repeated_values(&self) -> impl Iterator<Item = (&'static str, &[String])> + '_ {
        self.repeated
            .iter()
            .map(|(name, values)| (*name, values.as_slice()))
    }
    fn apply(&mut self, arguments: &[String]) -> Result<(), String> {
        let mut arguments = arguments.iter().peekable();
        while let Some(argument) = arguments.next() {
            let flag = canonical_option(
                argument
                    .split_once('=')
                    .map_or(argument.as_str(), |(flag, _)| flag),
            );
            let flag = if flag == "-r" { "--require" } else { flag };
            if let Some(values) = self.repeated.get_mut(flag) {
                let value = argument
                    .split_once('=')
                    .map(|(_, value)| value.to_owned())
                    .or_else(|| arguments.next().cloned())
                    .ok_or_else(|| format!("{flag} requires an argument"))?;
                values.push(value);
                continue;
            }
            if let Some(value) = self.strings.get_mut(flag) {
                *value = argument
                    .split_once('=')
                    .map(|(_, value)| value.to_owned())
                    .or_else(|| arguments.next().cloned())
                    .ok_or_else(|| format!("{flag} requires an argument"))?;
                if flag == "--report-signal" {
                    self.booleans.insert("--report-on-signal", true);
                }
                continue;
            }
            if let Some(value) = self.numbers.get_mut(flag) {
                self.provided.insert(flag.to_owned());
                *value = argument
                    .split_once('=')
                    .map(|(_, value)| value.to_owned())
                    .or_else(|| arguments.next().cloned())
                    .ok_or_else(|| format!("{flag} requires an argument"))?;
                continue;
            }
            match flag {
                "--trace-event-categories" | "--trace-event-file-pattern" | "-r" | "--require"
                    if !argument.contains('=') =>
                {
                    arguments.next();
                }
                "-e" | "--eval" | "-pe" | "-ep" if !argument.contains('=') => {
                    arguments.next();
                }
                "-p" | "--print"
                    if !argument.contains('=')
                        && arguments.peek().is_some_and(|next| !next.starts_with('-')) =>
                {
                    arguments.next();
                }
                _ => {
                    if let Some(value) = self.booleans.get_mut(flag) {
                        *value = true;
                    } else if let Some(positive) = flag.strip_prefix("--no-") {
                        let positive = format!("--{positive}");
                        if let Some(value) = self.booleans.get_mut(positive.as_str()) {
                            *value = false;
                        } else if VALUE_OPTIONS.contains(&positive.as_str())
                            || self.strings.contains_key(positive.as_str())
                            || self.numbers.contains_key(positive.as_str())
                            || self.repeated.contains_key(positive.as_str())
                        {
                            return Err(format!(
                                "{flag} is an invalid negation because it is not a boolean option"
                            ));
                        } else {
                            return Err(format!("bad option: {flag}"));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// Node's double-quote tokenizer; only unquoted ASCII spaces separate arguments.
pub(crate) fn parse_node_options(text: &str) -> Result<Vec<String>, &'static str> {
    let mut arguments = Vec::<String>::new();
    let mut quoted = false;
    let mut new_argument = true;
    let mut characters = text.chars();
    while let Some(mut character) = characters.next() {
        if character == '\\' && quoted {
            character = characters
                .next()
                .ok_or("invalid value for NODE_OPTIONS (invalid escape)\n")?;
        } else if character == ' ' && !quoted {
            new_argument = true;
            continue;
        } else if character == '"' {
            quoted = !quoted;
            continue;
        }
        if new_argument {
            arguments.push(String::new());
            new_argument = false;
        }
        arguments
            .last_mut()
            .expect("argument created before character")
            .push(character);
    }
    if quoted {
        return Err("invalid value for NODE_OPTIONS (unterminated string)\n");
    }
    Ok(arguments)
}
