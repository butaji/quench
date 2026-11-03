//! Parsed startup values for implemented Node feature switches.

/// Compare schema names at Rust binding time, without allocating a runtime index.
const fn option_name_order(left: &str, right: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering::{Equal, Greater, Less};
    let (left, right) = (left.as_bytes(), right.as_bytes());
    let mut index = 0;
    while index < left.len() && index < right.len() {
        if left[index] < right[index] { return Less; }
        if left[index] > right[index] { return Greater; }
        index += 1;
    }
    if left.len() < right.len() { Less } else if left.len() > right.len() { Greater } else { Equal }
}

const fn option_index<T>(schema: &[(&str, T)], name: &str) -> usize {
    let mut index = 0;
    while index < schema.len() {
        if matches!(option_name_order(schema[index].0, name), std::cmp::Ordering::Equal) { return index; }
        index += 1;
    }
    panic!("option belongs to its declared schema")
}

/// Preserve the old public lexical iteration order; the declaration owns each default.
const fn option_order<T, const N: usize>(schema: &[(&str, T)]) -> [u16; N] {
    assert!(N == schema.len() && N <= u16::MAX as usize);
    let mut order = [0; N];
    let mut index = 0;
    while index < N {
        let mut slot = index;
        while slot > 0 && matches!(option_name_order(schema[order[slot - 1] as usize].0, schema[index].0), std::cmp::Ordering::Greater) {
            order[slot] = order[slot - 1];
            slot -= 1;
        }
        if slot > 0 { assert!(!matches!(option_name_order(schema[order[slot - 1] as usize].0, schema[index].0), std::cmp::Ordering::Equal)); }
        order[slot] = index as u16;
        index += 1;
    }
    order
}

macro_rules! option_schema {
    ($table:ident, $lookup:ident, $order:ident, $ty:ty, [$(($name:literal, $default:expr)),* $(,)?]) => {
        const $table: &[(&str, $ty)] = &[$(($name, $default)),*];
        const $order: [u16; $table.len()] = option_order($table);
        #[inline(always)]
        fn $lookup(name: &str) -> Option<usize> {
            match name { $($name => Some(const { option_index($table, $name) }),)* _ => None }
        }
    };
}

include!("node_option_tables.rs");
option_schema!(BOOLEAN_OPTIONS, boolean_index, BOOLEAN_ORDER, bool, [
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
    // Quench's opt-in hot module replacement (`crate::host::hmr`).
    ("--hmr", false),
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
    // Node 26 exposes WebSocket and CloseEvent unless this is negated.
    ("--experimental-websocket", true),
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
]);
option_schema!(STRING_OPTIONS, string_index, STRING_ORDER, &str, [
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
]);
option_schema!(NUMBER_OPTIONS, number_index, NUMBER_ORDER, &str, [
    ("--test-timeout", "0"),
    ("--test-concurrency", "0"),
    ("--test-random-seed", "0"),
    ("--test-coverage-branches", "0"),
    ("--test-coverage-lines", "0"),
    ("--test-coverage-functions", "0"),
]);
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

/// Node's `OptionType` (src/node_options.h) for the kinds of option this parser knows.
#[derive(Clone, Copy)]
pub(crate) enum OptionType {
    Boolean = 2,
    UInteger = 4,
    String = 5,
    StringList = 7,
}

/// Every parsed option with its Node type and whether it defaults to true; a repeated
/// option is a string list whatever its single-value entry says.
pub(crate) fn option_types() -> impl Iterator<Item = (&'static str, OptionType, bool)> {
    let list = |name: &&str| repeated_index(name).is_some();
    BOOLEAN_OPTIONS
        .iter()
        .map(|&(name, default)| (name, OptionType::Boolean, default))
        .chain(STRING_OPTIONS.iter().filter(move |(name, _)| !list(name)).map(|&(name, _)| (name, OptionType::String, false)))
        .chain(NUMBER_OPTIONS.iter().map(|&(name, _)| (name, OptionType::UInteger, false)))
        .chain(EMPTY_STRING_LIST_OPTIONS.iter().map(|&(name, ())| (name, OptionType::StringList, false)))
}

/// Node's option aliases this parser accepts, each with the option it stands for.
pub(crate) const OPTION_ALIASES: &[(&str, &str)] = &[("-r", "--require"), ("--report-directory", "--report-dir")];

fn canonical_option(option: &str) -> &str {
    OPTION_ALIASES.iter().find(|(alias, _)| *alias == option).map_or(option, |(_, name)| name)
}
pub(crate) fn takes_string_value(option: &str) -> bool {
    STRING_OPTIONS
        .iter()
        .any(|(name, _)| *name == canonical_option(option))
        || NUMBER_OPTIONS
            .iter()
            .any(|(name, _)| *name == canonical_option(option))
        || repeated_index(canonical_option(option)).is_some()
}

const OPTION_WORD_BITS: usize = u64::BITS as usize;
const BOOLEAN_WORDS: usize = BOOLEAN_OPTIONS.len().div_ceil(OPTION_WORD_BITS);
const PROVIDED_WORDS: usize = NUMBER_OPTIONS.len().div_ceil(OPTION_WORD_BITS);
const DEFAULT_BOOLEANS: [u64; BOOLEAN_WORDS] = {
    let mut words = [0; BOOLEAN_WORDS];
    let mut index = 0;
    while index < BOOLEAN_OPTIONS.len() {
        if BOOLEAN_OPTIONS[index].1 { words[index / OPTION_WORD_BITS] |= 1 << (index % OPTION_WORD_BITS); }
        index += 1;
    }
    words
};

fn option_bit(words: &[u64], index: usize) -> bool {
    words[index / OPTION_WORD_BITS] & (1 << (index % OPTION_WORD_BITS)) != 0
}

fn set_option_bit(words: &mut [u64], index: usize, value: bool) {
    let bit = 1 << (index % OPTION_WORD_BITS);
    let word = &mut words[index / OPTION_WORD_BITS];
    if value { *word |= bit; } else { *word &= !bit; }
}

#[derive(Clone, Debug)]
pub(crate) struct CliOptions {
    booleans: [u64; BOOLEAN_WORDS],
    /// Only explicit text values are owned; unchanged defaults stay in the schema.
    strings: [Option<String>; STRING_OPTIONS.len()],
    numbers: [Option<String>; NUMBER_OPTIONS.len()],
    provided: [u64; PROVIDED_WORDS],
    repeated: [Vec<String>; EMPTY_STRING_LIST_OPTIONS.len()],
    /// Accepted Node booleans outside the implemented schema, in public lexical order.
    extra_booleans: Vec<(&'static str, bool)>,
}
impl Default for CliOptions {
    fn default() -> Self {
        Self {
            booleans: DEFAULT_BOOLEANS,
            strings: [const { None }; STRING_OPTIONS.len()],
            numbers: [const { None }; NUMBER_OPTIONS.len()],
            provided: [0; PROVIDED_WORDS],
            repeated: [const { Vec::new() }; EMPTY_STRING_LIST_OPTIONS.len()],
            extra_booleans: Vec::new(),
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
        options.set_boolean("--pending-deprecation", pending_deprecation);
        if let Some(environment) = environment {
            options.apply(&parse_node_options(environment).map_err(str::to_owned)?)?;
        }
        options.apply(invocation)?;
        if options.enabled("--trace-env-js-stack")
            || options.enabled("--trace-env-native-stack")
        {
            options.set_boolean("--trace-env", true);
        }
        if options.enabled("--tls-min-v1.3") && options.enabled("--tls-max-v1.2") {
            return Err("either --tls-min-v1.3 or --tls-max-v1.2 can be used, not both".to_owned());
        }
        Ok(options)
    }
    pub(crate) fn enabled(&self, name: &str) -> bool {
        match boolean_index(name) {
            Some(index) => option_bit(&self.booleans, index),
            None => self.extra_booleans.binary_search_by_key(&name, |&(name, _)| name)
                .ok().is_some_and(|index| self.extra_booleans[index].1),
        }
    }
    pub(crate) fn values(&self) -> impl Iterator<Item = (&'static str, bool)> + '_ {
        let mut declared = BOOLEAN_ORDER.iter().map(|&index| {
            let index = usize::from(index);
            (BOOLEAN_OPTIONS[index].0, option_bit(&self.booleans, index))
        }).peekable();
        let mut extra = self.extra_booleans.iter().copied().peekable();
        std::iter::from_fn(move || match (declared.peek(), extra.peek()) {
            (Some((left, _)), Some((right, _))) if right < left => extra.next(),
            (Some(_), _) => declared.next(),
            _ => extra.next(),
        })
    }
    pub(crate) fn string_value(&self, name: &str) -> Option<&str> {
        let index = string_index(canonical_option(name))?;
        Some(self.strings[index].as_deref().unwrap_or(STRING_OPTIONS[index].1))
    }
    pub(crate) fn strings(&self) -> impl Iterator<Item = (&'static str, &str)> + '_ {
        STRING_ORDER.iter().map(|&index| {
            let index = usize::from(index);
            (STRING_OPTIONS[index].0, self.strings[index].as_deref().unwrap_or(STRING_OPTIONS[index].1))
        })
    }
    pub(crate) fn numbers(&self) -> impl Iterator<Item = (&'static str, &str)> + '_ {
        NUMBER_ORDER.iter().map(|&index| {
            let index = usize::from(index);
            (NUMBER_OPTIONS[index].0, self.numbers[index].as_deref().unwrap_or(NUMBER_OPTIONS[index].1))
        })
    }
    pub(crate) fn was_provided(&self, name: &str) -> bool {
        number_index(name).is_some_and(|index| option_bit(&self.provided, index))
    }
    pub(crate) fn repeated_values(&self) -> impl Iterator<Item = (&'static str, &[String])> + '_ {
        REPEATED_ORDER.iter().map(|&index| {
            let index = usize::from(index);
            (EMPTY_STRING_LIST_OPTIONS[index].0, self.repeated[index].as_slice())
        })
    }

    /// Serialize effective values; string-list declarations own repeated values.
    pub(crate) fn flags(&self) -> impl Iterator<Item = String> + '_ {
        let booleans = BOOLEAN_OPTIONS.iter().enumerate().filter_map(|(index, &(name, default))| {
            let enabled = option_bit(&self.booleans, index);
            (enabled != default).then(|| if enabled { name.to_owned() }
                else { format!("--no-{}", name.strip_prefix("--").expect("long option")) })
        });
        let strings = self.strings().filter(|(name, value)| repeated_index(name).is_none() && !value.is_empty())
            .map(|(name, value)| format!("{name}={value}"));
        let numbers = self.numbers().map(|(name, value)| format!("{name}={value}"));
        let lists = self.repeated_values().flat_map(|(name, values)| values.iter().map(move |value| format!("{name}={value}")));
        let benches = self.enabled("--experimental-bench");
        booleans.chain(strings).chain(numbers).chain(lists).filter(move |flag| {
            benches || !flag.starts_with("--bench-")
        })
    }

    fn set_boolean(&mut self, name: &str, value: bool) -> bool {
        if let Some(index) = boolean_index(name) { set_option_bit(&mut self.booleans, index, value); return true; }
        let Ok(index) = self.extra_booleans.binary_search_by_key(&name, |&(name, _)| name) else { return false };
        self.extra_booleans[index].1 = value;
        true
    }
    fn apply(&mut self, arguments: &[String]) -> Result<(), String> {
        let mut arguments = arguments.iter().peekable();
        while let Some(argument) = arguments.next() {
            let flag = canonical_option(
                argument
                    .split_once('=')
                    .map_or(argument.as_str(), |(flag, _)| flag),
            );
            if let Some(index) = repeated_index(flag) {
                let value = argument
                    .split_once('=')
                    .map(|(_, value)| value.to_owned())
                    .or_else(|| arguments.next().cloned())
                    .ok_or_else(|| format!("{flag} requires an argument"))?;
                self.repeated[index].push(value);
                continue;
            }
            if let Some(index) = string_index(flag) {
                self.strings[index] = Some(argument
                    .split_once('=')
                    .map(|(_, value)| value.to_owned())
                    .or_else(|| arguments.next().cloned())
                    .ok_or_else(|| format!("{flag} requires an argument"))?);
                if flag == "--report-signal" {
                    self.set_boolean("--report-on-signal", true);
                }
                continue;
            }
            if let Some(index) = number_index(flag) {
                set_option_bit(&mut self.provided, index, true);
                self.numbers[index] = Some(argument
                    .split_once('=')
                    .map(|(_, value)| value.to_owned())
                    .or_else(|| arguments.next().cloned())
                    .ok_or_else(|| format!("{flag} requires an argument"))?);
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
                    if self.set_boolean(flag, true) { continue; }
                    if let Some(positive) = flag.strip_prefix("--no-") {
                        let positive = format!("--{positive}");
                        if self.set_boolean(positive.as_str(), false) { continue; }
                        if VALUE_OPTIONS.contains(&positive.as_str())
                            || string_index(positive.as_str()).is_some()
                            || number_index(positive.as_str()).is_some()
                            || repeated_index(positive.as_str()).is_some()
                        {
                            return Err(format!(
                                "{flag} is an invalid negation because it is not a boolean option"
                            ));
                        } else if let Some(listed) = node_listed_flag(&positive) {
                            // A Node boolean this host keeps no default for: its negation is
                            // what `getOptionValue` reads.
                            let index = self.extra_booleans.binary_search_by_key(&listed, |&(name, _)| name).unwrap_err();
                            self.extra_booleans.insert(index, (listed, false));
                        } else if node_listed_flag(flag).is_none() {
                            return Err(format!("bad option: {flag}"));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// `flag`, when it is one of Node's own CLI options (`process.allowedNodeEnvironmentFlags`'s
/// source list), which Node accepts even where this host keeps no value for it.
fn node_listed_flag(flag: &str) -> Option<&'static str> {
    crate::modules::process_surface::NODE_FLAGS.iter().copied().find(|listed| *listed == flag)
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

#[cfg(test)]
mod node_flag_list_tests {
    #[test]
    fn invocation_overrides_environment_and_keeps_repeated_input_order() {
        let arguments = ["--warnings", "--conditions=invocation", "--report-directory=chosen", "--test-random-seed=0"]
            .map(str::to_owned);
        let options = super::CliOptions::parse(Some("--no-warnings --conditions=environment"), &arguments, true).unwrap();
        assert!(options.enabled("--warnings"));
        assert!(options.enabled("--pending-deprecation"));
        assert_eq!(options.string_value("--report-dir"), Some("chosen"));
        assert!(options.was_provided("--test-random-seed"));
        let conditions = options.repeated_values().find(|(name, _)| *name == "--conditions").unwrap().1;
        assert_eq!(conditions, ["environment", "invocation"]);
        let names: Vec<_> = options.values().map(|(name, _)| name).collect();
        assert!(names.windows(2).all(|names| names[0] < names[1]));
    }

    #[test]
    fn listed_extra_booleans_keep_precedence_and_public_order() {
        let arguments = ["--no-node-snapshot", "--node-snapshot"].map(str::to_owned);
        let options = super::CliOptions::parse(None, &arguments, false).unwrap();
        assert!(options.enabled("--node-snapshot"));
        let values: Vec<_> = options.values().collect();
        assert!(values.windows(2).all(|values| values[0].0 < values[1].0));
        assert_eq!(values.iter().filter(|(name, _)| *name == "--node-snapshot").count(), 1);
        assert!(super::CliOptions::parse(None, &["--no-conditions".into()], false).unwrap_err()
            .contains("not a boolean option"));
    }

    #[test]
    fn node_lists_its_negatable_options() {
        assert!(super::node_listed_flag("--no-node-snapshot").is_some());
        assert!(super::node_listed_flag("--experimental-websocket").is_some());
        assert!(super::node_listed_flag("--no-bogus-flag").is_none());
    }
}
