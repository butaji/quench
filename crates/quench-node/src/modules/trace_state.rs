//! Process-owned trace categories and real Chrome trace file output.

use std::collections::{BTreeSet, HashMap};
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use quench_runtime::{HostWeakRoot, RootId};
use serde_json::{json, Value};

const DEFAULT_FILE_PATTERN: &str = "node_trace.${rotation}.log";
#[cfg(target_os = "linux")]
const MICROSECONDS_PER_SECOND: u64 = 1_000_000;
#[cfg(target_os = "linux")]
const NANOSECONDS_PER_MICROSECOND: u64 = 1000;
const FIRST_FILE_ROTATION: &str = "1";

#[derive(Clone, Copy)]
pub(crate) enum Phase {
    AsyncBegin,
    AsyncInstant,
    AsyncEnd,
    Counter,
    Raw(u8),
}
impl Phase {
    pub(crate) fn from_code(code: u8) -> Self {
        [
            Self::AsyncBegin,
            Self::AsyncInstant,
            Self::AsyncEnd,
            Self::Counter,
        ]
        .into_iter()
        .find(|phase| phase.byte() == code)
        .unwrap_or(Self::Raw(code))
    }
    fn byte(self) -> u8 {
        match self {
            Self::AsyncBegin => b'b',
            Self::AsyncInstant => b'n',
            Self::AsyncEnd => b'e',
            Self::Counter => b'C',
            Self::Raw(code) => code,
        }
    }
    fn code(self) -> String {
        char::from(self.byte()).to_string()
    }
}

pub(crate) struct EnabledTracing {
    pub(crate) retained: RootId,
    pub(crate) categories: Vec<String>,
}

pub(crate) struct TraceState {
    started: Instant,
    cli_categories: Vec<String>,
    pub(crate) enabled: HashMap<u64, EnabledTracing>,
    pub(crate) next_handle: u64,
    path: Option<PathBuf>,
    file_pattern: String,
    pub(crate) category_buffers: Vec<(String, HostWeakRoot)>,
    events: Vec<Value>,
    environment_id: Option<u64>,
}

impl Default for TraceState {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            cli_categories: Vec::new(),
            enabled: HashMap::new(),
            next_handle: 0,
            path: None,
            file_pattern: DEFAULT_FILE_PATTERN.into(),
            category_buffers: Vec::new(),
            events: Vec::new(),
            environment_id: None,
        }
    }
}

impl TraceState {
    fn output_path(directory: &std::path::Path, pattern: &str) -> PathBuf {
        let filename = pattern
            .replace("${pid}", &std::process::id().to_string())
            .replace("${rotation}", FIRST_FILE_ROTATION);
        directory.join(filename)
    }

    pub(crate) fn configure(&mut self, invocation: &[String]) {
        // Node accepts trace settings from NODE_OPTIONS as well as argv.
        // Apply both here so tracing is initialized before bootstrap touches
        // the environment or performs other instrumented work.
        let mut args = std::env::var("NODE_OPTIONS")
            .ok()
            .and_then(|options| crate::modules::cli_options::parse_node_options(&options).ok())
            .unwrap_or_default();
        args.extend_from_slice(invocation);
        let mut categories = None;
        let mut enabled = false;
        let mut pattern = DEFAULT_FILE_PATTERN.to_owned();
        let mut index = 0;
        while index < args.len() {
            let argument = &args[index];
            if argument == "--trace-events-enabled" {
                enabled = true;
            } else if let Some(value) = argument.strip_prefix("--trace-event-categories=") {
                categories = Some(value.to_owned());
            } else if argument == "--trace-event-categories" {
                index += 1;
                categories = args.get(index).cloned();
            } else if let Some(value) = argument.strip_prefix("--trace-event-file-pattern=") {
                pattern = value.to_owned();
            } else if argument == "--trace-event-file-pattern" {
                index += 1;
                if let Some(value) = args.get(index) {
                    pattern = value.clone();
                }
            }
            index += 1;
        }
        if enabled && categories.is_none() {
            categories = Some("v8,node,node.async_hooks".into());
        }
        self.cli_categories = categories
            .unwrap_or_default()
            .split(',')
            .filter(|category| !category.is_empty())
            .map(str::to_owned)
            .collect();
        self.file_pattern = pattern;
        if self.enabled_categories().is_some() {
            self.activate(&std::env::current_dir().unwrap_or_default());
        }
    }

    pub(crate) fn activate(&mut self, directory: &std::path::Path) {
        if self.path.is_none() {
            self.path = Some(Self::output_path(directory, &self.file_pattern));
        }
        self.ensure_metadata();
    }

    pub(crate) fn enabled_categories(&self) -> Option<String> {
        let names: BTreeSet<&str> = self
            .cli_categories
            .iter()
            .map(String::as_str)
            .chain(
                self.enabled
                    .values()
                    .flat_map(|entry| entry.categories.iter().map(String::as_str)),
            )
            .collect();
        (!names.is_empty()).then(|| names.into_iter().collect::<Vec<_>>().join(","))
    }

    pub(crate) fn category_enabled(&self, group: &str) -> bool {
        group.split(',').any(|category| {
            self.cli_categories.iter().any(|name| name == category)
                || self
                    .enabled
                    .values()
                    .any(|entry| entry.categories.iter().any(|name| name == category))
        })
    }

    fn timestamp(&self) -> u64 {
        #[cfg(target_os = "linux")]
        if let Some(value) = Self::clock_timestamp(libc::CLOCK_MONOTONIC) {
            return value;
        }
        u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX)
    }
    fn cpu_timestamp(&self) -> u64 {
        #[cfg(target_os = "linux")]
        if let Some(value) = Self::clock_timestamp(libc::CLOCK_THREAD_CPUTIME_ID) {
            return value;
        }
        0
    }
    #[cfg(target_os = "linux")]
    fn clock_timestamp(clock: libc::clockid_t) -> Option<u64> {
        let mut time = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        if unsafe { libc::clock_gettime(clock, &mut time) } != 0 {
            return None;
        }
        u64::try_from(time.tv_sec)
            .ok()?
            .checked_mul(MICROSECONDS_PER_SECOND)?
            .checked_add(u64::try_from(time.tv_nsec).ok()? / NANOSECONDS_PER_MICROSECOND)
    }

    pub(crate) fn record(
        &mut self,
        group: &str,
        name: &str,
        phase: Phase,
        id: Option<u64>,
        args: Value,
    ) {
        if !self.category_enabled(group) {
            return;
        }
        self.ensure_metadata();
        #[cfg(target_os = "linux")]
        let tid = unsafe { libc::syscall(libc::SYS_gettid) as u64 };
        #[cfg(not(target_os = "linux"))]
        let tid = u64::from(std::process::id());
        let mut event = json!({"pid":std::process::id(),"tid":tid,"ts":self.timestamp(),"tts":self.cpu_timestamp(),"dur":0,"tdur":0,"ph":phase.code(),"cat":group,"name":name,"args":args});
        if let Some(id) = id {
            event["id"] = json!(format!("0x{id:x}"));
        }
        self.events.push(event);
    }

    pub(crate) fn ensure_metadata(&mut self) {
        if !self.events.is_empty() {
            return;
        }
        let executable = std::env::current_exe().ok();
        let process_name = executable
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.events.push(json!({"pid":std::process::id(),"tid":std::process::id(),"ts":0,"ph":"M","cat":"__metadata","name":"process_name","args":{"name":process_name}}));
    }

    pub(crate) fn open_environment(&mut self, argv: &[String], exec_argv: &[String]) {
        let id = self as *const Self as usize as u64;
        self.environment_id = Some(id);
        self.record(
            "node,node.environment",
            "Environment",
            Phase::AsyncBegin,
            Some(id),
            json!({"args":{"args":argv,"exec_args":exec_argv}}),
        );
    }
    fn begin_span(&mut self, name: &str) -> Option<usize> {
        if !self.category_enabled("node,node.environment") {
            return None;
        }
        self.record(
            "node,node.environment",
            name,
            Phase::Raw(b'X'),
            None,
            json!({}),
        );
        let index = self.events.len() - 1;
        self.events[index]["dur"] = json!(0);
        Some(index)
    }
    fn end_span(&mut self, index: usize) {
        let elapsed = self.timestamp();
        let cpu = self.cpu_timestamp();
        if let Some(event) = self.events.get_mut(index) {
            if let Some(start) = event["ts"].as_u64() {
                event["dur"] = json!(elapsed.saturating_sub(start));
            }
            if let Some(start) = event["tts"].as_u64() {
                event["tdur"] = json!(cpu.saturating_sub(start));
            }
        }
    }

    pub(crate) fn flush(&self) -> std::io::Result<()> {
        if self.events.is_empty() {
            return Ok(());
        }
        let Some(path) = &self.path else {
            return Ok(());
        };
        let file = std::fs::File::create(path)?;
        let mut writer = std::io::BufWriter::new(file);
        writer.write_all(b"{\"traceEvents\":[")?;
        for (index, event) in self.events.iter().enumerate() {
            if index != 0 {
                writer.write_all(b",")?;
            }
            writer.write_all(b"{")?;
            for (index, (key, value)) in event
                .as_object()
                .expect("trace event object")
                .iter()
                .enumerate()
            {
                if index != 0 {
                    writer.write_all(b",")?;
                }
                serde_json::to_writer(&mut writer, key)?;
                writer.write_all(b":")?;
                // The SDK/V8 JSONTraceWriter receives already escaped category
                // and name payloads and writes them directly inside quotes.
                // Escaping again changes original SDK safeTraceLabel values.
                if matches!(key.as_str(), "name" | "cat") {
                    writer.write_all(b"\"")?;
                    writer.write_all(value.as_str().expect("trace string field").as_bytes())?;
                    writer.write_all(b"\"")?;
                } else {
                    serde_json::to_writer(&mut writer, value)?;
                }
            }
            writer.write_all(b"}")?;
        }
        writer.write_all(b"]}")?;
        writer.flush()
    }
}

impl Drop for TraceState {
    fn drop(&mut self) {
        if let Some(id) = self.environment_id {
            self.record(
                "node,node.environment",
                "Environment",
                Phase::AsyncEnd,
                Some(id),
                json!({}),
            );
        }
        if let Err(error) = self.flush() {
            eprintln!("Unable to write trace file: {error}");
        }
    }
}

/// A trace span surrounds a real host operation and closes on every result path.
pub(crate) struct EnvironmentSpan {
    state: std::rc::Rc<std::cell::RefCell<crate::host::SharedNodeState>>,
    index: usize,
}
impl EnvironmentSpan {
    pub(crate) fn enter(
        state: &std::rc::Rc<std::cell::RefCell<crate::host::SharedNodeState>>,
        name: &'static str,
    ) -> Option<Self> {
        let index = state.borrow_mut().trace.begin_span(name)?;
        Some(Self {
            state: state.clone(),
            index,
        })
    }
}
impl Drop for EnvironmentSpan {
    fn drop(&mut self) {
        self.state.borrow_mut().trace.end_span(self.index);
    }
}
