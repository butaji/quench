//! How JetStream's driver runs an analogue's row, from its `// jetstream-driver:` header: the
//! benchmark class (which runner loop and host helpers it gets), whether `Math.random` is seeded,
//! the preloaded resources the row reads by name, the globals a compressed payload exports, and
//! whether the row's files run as one subtest each (a grouped benchmark).

/// The benchmark classes of JetStreamDriver.js that an analogue's row can be.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// `DefaultBenchmark`: a synchronous `runIteration`.
    #[default]
    Default,
    /// `AsyncBenchmark`: awaited `init`, `prepareForNextIteration` and `runIteration`.
    Async,
    /// `WasmEMCCBenchmark`: an `Async` row whose Emscripten `Module` is set up before its files.
    WasmEmcc,
}

/// One analogue's runner settings. The defaults are a plain synchronous, unseeded row.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Driver {
    pub class: Class,
    pub deterministic: bool,
    /// `name=path` pairs: the path is relative to the JetStream root, as the driver's preloads are.
    pub preload: Vec<(String, String)>,
    /// Globals a compressed (`.z`) file's top-level `let` bindings are exported as.
    pub exports: Vec<String>,
    /// Each file is one subtest with its own `Benchmark` (JetStream's grouped `Sunspider`).
    pub per_file: bool,
    /// `(global, file)` pairs: after `file` (a UMD bundle) the global is its `module.exports`.
    pub namespaces: Vec<(String, String)>,
    /// The row's `Worker`s run their scripts in their own realms (`worker=true`, a browser row).
    pub worker: bool,
    /// `runner=wsl`: each iteration builds a fresh `Benchmark` and times its `buildStdlib` and `run`.
    pub runner: String,
}

impl Class {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "Default" => Ok(Self::Default),
            "Async" => Ok(Self::Async),
            "WasmEMCC" => Ok(Self::WasmEmcc),
            other => Err(format!("unknown benchmark class `{other}`")),
        }
    }
}

/// The `global@file` entries of a `namespace=` value.
fn namespaces(value: &str) -> Result<Vec<(String, String)>, String> {
    value
        .split(',')
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            entry
                .split_once('@')
                .map(|(global, file)| (global.to_owned(), file.to_owned()))
                .ok_or_else(|| format!("namespace `{entry}` is not `global@file`"))
        })
        .collect()
}

/// The `name=path` entries of a `preload=` value, which is `name=path[,name=path...]`.
fn preloads(value: &str) -> Result<Vec<(String, String)>, String> {
    value
        .split(',')
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            entry
                .split_once('=')
                .map(|(name, path)| (name.to_owned(), path.to_owned()))
                .ok_or_else(|| format!("preload `{entry}` is not `name=path`"))
        })
        .collect()
}

impl Driver {
    /// Reads the space-separated `key[=value]` fields of a `// jetstream-driver:` header.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut driver = Self::default();
        for token in text.split_whitespace() {
            let (key, value) = token.split_once('=').unwrap_or((token, "true"));
            match key {
                "class" => driver.class = Class::parse(value)?,
                "deterministic" => driver.deterministic = value == "true",
                "preload" => driver.preload = preloads(value)?,
                "exports" => driver.exports = value.split(',').filter(|name| !name.is_empty()).map(str::to_owned).collect(),
                "group" => driver.per_file = value == "file",
                "namespace" => driver.namespaces = namespaces(value)?,
                "worker" => driver.worker = value == "true",
                "runner" => driver.runner = value.to_owned(),
                other => return Err(format!("unknown driver field `{other}`")),
            }
        }
        Ok(driver)
    }
}
