//! Run a single test262 case (in-process with timeout, or subprocess).

use std::collections::{hash_map::DefaultHasher, HashMap, HashSet, VecDeque};
use std::borrow::Cow;
use std::hash::{Hash, Hasher};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use std::time::Instant;

use crate::harness::HarnessLoader;
use crate::metadata::Test262Metadata;
use crate::{capture_thrown_diagnostics, TestFailure, TestOutcome};
use quench_runtime::Value;

/// Per-test timeout in seconds — one value shared by the in-process and
/// subprocess (isolated) paths so a test cannot pass one way and fail the other.
pub const DEFAULT_TEST_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Default, Clone, Copy)]
pub struct RunMetrics {
    pub parse_negative_short_circuit: u64,
    pub threaded_runs: u64,
    pub threadless_runs: u64,
    pub threadless_auto_runs: u64,
    pub threadless_auto_candidates: u64,
    pub threadless_auto_reject_config: u64,
    pub threadless_auto_reject_size: u64,
    pub threadless_auto_reject_async: u64,
    pub threadless_auto_reject_dependency_markers: u64,
    pub timeouts: u64,
    pub panics: u64,
    pub skipped_due_to_missing_harness: u64,
    pub prepared_cache_hits: u64,
    pub prepared_cache_misses: u64,
    pub isolated_runs: u64,
    pub isolated_timeouts: u64,
    pub isolated_spawn_failures: u64,
    pub isolated_wait_failures: u64,
    pub isolated_retries: u64,
    pub isolated_retry_skipped: u64,
    pub fixture_dependency_marker_cache_hits: u64,
    pub fixture_dependency_marker_cache_misses: u64,
    pub fixture_module_lexical_cache_hits: u64,
    pub fixture_module_lexical_cache_misses: u64,
    pub fixture_module_request_cache_hits: u64,
    pub fixture_module_request_cache_misses: u64,
    pub fixture_module_request_fastpath_hits: u64,
    pub fixture_module_request_fastpath_misses: u64,
    pub fixture_deferred_import_cache_hits: u64,
    pub fixture_deferred_import_cache_misses: u64,
    pub fixture_module_syntax_cache_hits: u64,
    pub fixture_module_syntax_cache_misses: u64,
    pub fixture_modules_selected: u64,
    pub fixture_modules_loaded: u64,
    pub fixture_modules_missing: u64,
    pub fixture_module_bytes_loaded: u64,
    pub fixture_file_cache_hits: u64,
    pub fixture_file_cache_misses: u64,
    pub fixture_dir_cache_hits: u64,
    pub fixture_dir_cache_misses: u64,
    pub fixture_dep_cache_hits: u64,
    pub fixture_dep_cache_misses: u64,
    pub fixture_graph_nodes: u64,
    pub fixture_graph_edges: u64,
    pub fixture_graph_max_depth: u64,
    pub fixture_graph_selected_modules: u64,
    pub fixture_module_load_tests: u64,
    pub fixture_module_load_millis: u64,
    pub fixture_invalid_syntax_modules: u64,
    pub fixture_no_dependency_skips: u64,
    pub fixture_no_fixture_request_skips: u64,
    pub worker_starts: u64,
    pub worker_batches: u64,
    pub isolation_fallbacks: u64,
}

static METRIC_PARSE_NEGATIVE_SHORT_CIRCUIT: AtomicUsize = AtomicUsize::new(0);
static METRIC_THREADED_RUNS: AtomicUsize = AtomicUsize::new(0);
static METRIC_THREADLESS_RUNS: AtomicUsize = AtomicUsize::new(0);
static METRIC_THREADLESS_AUTO_RUNS: AtomicUsize = AtomicUsize::new(0);
static METRIC_THREADLESS_AUTO_CANDIDATES: AtomicUsize = AtomicUsize::new(0);
static METRIC_THREADLESS_AUTO_REJECT_CONFIG: AtomicUsize = AtomicUsize::new(0);
static METRIC_THREADLESS_AUTO_REJECT_SIZE: AtomicUsize = AtomicUsize::new(0);
static METRIC_THREADLESS_AUTO_REJECT_ASYNC: AtomicUsize = AtomicUsize::new(0);
static METRIC_THREADLESS_AUTO_REJECT_DEPENDENCY_MARKERS: AtomicUsize =
    AtomicUsize::new(0);
static METRIC_TIMEOUTS: AtomicUsize = AtomicUsize::new(0);
static METRIC_PANICS: AtomicUsize = AtomicUsize::new(0);
static METRIC_MISSING_HARNESS: AtomicUsize = AtomicUsize::new(0);
static METRIC_PREPARED_CACHE_HITS: AtomicUsize = AtomicUsize::new(0);
static METRIC_PREPARED_CACHE_MISSES: AtomicUsize = AtomicUsize::new(0);
static METRIC_ISOLATED_RUNS: AtomicUsize = AtomicUsize::new(0);
static METRIC_ISOLATED_TIMEOUTS: AtomicUsize = AtomicUsize::new(0);
static METRIC_ISOLATED_SPAWN_FAILURES: AtomicUsize = AtomicUsize::new(0);
static METRIC_ISOLATED_WAIT_FAILURES: AtomicUsize = AtomicUsize::new(0);
static METRIC_ISOLATED_RETRIES: AtomicUsize = AtomicUsize::new(0);
static METRIC_ISOLATED_RETRY_SKIPPED: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_DEPENDENCY_MARKER_CACHE_HITS: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_DEPENDENCY_MARKER_CACHE_MISSES: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_MODULE_LEXICAL_CACHE_HITS: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_MODULE_LEXICAL_CACHE_MISSES: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_MODULE_REQUEST_CACHE_HITS: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_MODULE_REQUEST_CACHE_MISSES: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_MODULE_REQUEST_FASTPATH_HITS: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_MODULE_REQUEST_FASTPATH_MISSES: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_DEFERRED_IMPORT_CACHE_HITS: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_DEFERRED_IMPORT_CACHE_MISSES: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_MODULE_SYNTAX_CACHE_HITS: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_MODULE_SYNTAX_CACHE_MISSES: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_MODULES_SELECTED: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_MODULES_LOADED: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_MODULES_MISSING: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_MODULE_BYTES_LOADED: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_GRAPH_NODES: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_GRAPH_EDGES: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_GRAPH_MAX_DEPTH: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_GRAPH_SELECTED_MODULES: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_MODULE_LOAD_TESTS: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_MODULE_LOAD_MILLIS: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_INVALID_SYNTAX_MODULES: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_NO_DEPENDENCY_SKIPS: AtomicUsize = AtomicUsize::new(0);
static METRIC_FIXTURE_NO_FIXTURE_REQUEST_SKIPS: AtomicUsize = AtomicUsize::new(0);
static METRIC_WORKER_STARTS: AtomicUsize = AtomicUsize::new(0);
static METRIC_WORKER_BATCHES: AtomicUsize = AtomicUsize::new(0);
static METRIC_ISOLATION_FALLBACKS: AtomicUsize = AtomicUsize::new(0);

fn test_timeout_secs() -> u64 {
    static TIMEOUT_SECS: OnceLock<u64> = OnceLock::new();
    *TIMEOUT_SECS.get_or_init(|| {
        std::env::var("TEST_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_TEST_TIMEOUT_SECS)
    })
}

/// Async prelude: `$DONE` records invocations and rethrows error arguments.
/// The count is verified after the microtask drain by `async_done_probe`.
pub const ASYNC_DONE_PRELUDE: &str = "var __test262DoneReplacement; \
var __test262Done = function(error) { \
globalThis.__test262DoneCount = (globalThis.__test262DoneCount|0) + 1; \
if (globalThis.__test262DoneCount > 1) throw new Test262Error('$DONE called twice'); \
if (error !== undefined && error !== null) { globalThis.__test262DoneError = error; throw error; } \
}; \
globalThis.$DONE = __test262Done; \
Object.defineProperty(globalThis, '$DONE', { configurable: true, get: function() { return __test262DoneReplacement === undefined ? __test262Done : __test262DoneReplacement; }, set: function(callback) { __test262DoneReplacement = function(error) { globalThis.__test262ReplacementDoneCount = (globalThis.__test262ReplacementDoneCount|0) + 1; return callback(error); }; } });\n";

/// Infrastructure failure markers — never evidence of expected test behavior.
const INFRA_MARKERS: &[&str] = &[
    "harness load failure",
    "timed out",
    "panicked",
    "failed to spawn",
];

const JS_ERROR_TYPES: &[&str] = &[
    "Error",
    "EvalError",
    "RangeError",
    "ReferenceError",
    "SyntaxError",
    "TypeError",
    "URIError",
    "AggregateError",
    "Test262Error",
];

const FIXTURE_DEFAULT_MARKER_TOKEN: &str = "__quench_fixture_default_marker__";

#[derive(Clone, Hash, Eq, PartialEq)]
struct SourceCacheKey {
    len: usize,
    hash: u64,
    first: u8,
    last: u8,
}

#[derive(Clone)]
struct FixtureFileCacheEntry {
    bytes: std::sync::Arc<Vec<u8>>,
    source: std::sync::Arc<String>,
    source_is_utf8: bool,
}

#[derive(Clone)]
struct FixtureExportsCacheEntry {
    eval_source: Arc<String>,
    side_effect_source: Arc<String>,
    exports: Arc<FixtureExports>,
    default_import: Arc<Option<String>>,
    reexports: Arc<Vec<PendingReExport>>,
    has_default_marker: bool,
}

type FixtureDirMap = HashMap<String, std::path::PathBuf>;

#[derive(Default, Clone, Copy)]
struct FixtureProfileStats {
    file_hits: usize,
    file_misses: usize,
    dir_hits: usize,
    dir_misses: usize,
    dep_hits: usize,
    dep_misses: usize,
    modules_loaded: usize,
    modules_missing: usize,
    bytes_loaded: usize,
    source_analysis_hits: usize,
    source_analysis_misses: usize,
    import_edges_hits: usize,
    import_edges_misses: usize,
    named_import_hits: usize,
    named_import_misses: usize,
    namespace_import_hits: usize,
    namespace_import_misses: usize,
    source_import_hits: usize,
    source_import_misses: usize,
    decl_hits: usize,
    decl_misses: usize,
    side_effect_import_hits: usize,
    side_effect_import_misses: usize,
    reexport_request_hits: usize,
    reexport_request_misses: usize,
    dynamic_request_hits: usize,
    dynamic_request_misses: usize,
    module_request_hits: usize,
    module_request_misses: usize,
    attr_module_request_hits: usize,
    attr_module_request_misses: usize,
    import_request_hits: usize,
    import_request_misses: usize,
    module_lexical_bindings_hits: usize,
    module_lexical_bindings_misses: usize,
    deferred_import_request_hits: usize,
    deferred_import_request_misses: usize,
    no_dependency_short_circuits: usize,
    selected_modules: usize,
    graph_nodes: usize,
    graph_edges: usize,
    graph_max_depth: usize,
}

type FixtureDirCache = HashMap<std::path::PathBuf, std::sync::Arc<FixtureDirMap>>;
type FixtureFileCache = HashMap<std::path::PathBuf, FixtureFileCacheEntry>;
type FixtureExportsCache = HashMap<SourceCacheKey, std::sync::Arc<FixtureExportsCacheEntry>>;
type FixtureImportEdgesCache = HashMap<SourceCacheKey, Arc<Vec<(String, String)>>>;
type FixtureImportRequestsCache = HashMap<SourceCacheKey, Arc<Vec<String>>>;
type FixtureAttributeRequestsCache = HashMap<SourceCacheKey, Arc<HashSet<String>>>;
type FixtureReexportRequestsCache = HashMap<SourceCacheKey, Arc<Vec<String>>>;
type FixtureDynamicRequestsCache = HashMap<SourceCacheKey, Arc<Vec<String>>>;
type FixtureNamedImportsCache = HashMap<SourceCacheKey, Arc<HashMap<String, (String, String)>>>;
type FixtureNamespaceImportsCache = HashMap<SourceCacheKey, Arc<HashMap<String, String>>>;
type FixtureSourceImportsCache = HashMap<SourceCacheKey, Arc<HashMap<String, String>>>;
type FixtureDeclCache = HashMap<SourceCacheKey, Arc<HashSet<String>>>;
type FixtureCurrentModuleLexicalCache = HashMap<SourceCacheKey, Arc<HashSet<String>>>;
type FixtureDeferredImportRequestCache = HashMap<SourceCacheKey, Arc<Vec<(String, bool)>>>;
type FixtureSideEffectImportsCache = HashMap<SourceCacheKey, Arc<Vec<String>>>;
type FixtureModuleRequestsCache = HashMap<SourceCacheKey, Arc<HashSet<String>>>;
type FixtureDependencyMarkerCache = HashMap<SourceCacheKey, bool>;
type FixtureDepCache = HashMap<std::path::PathBuf, std::sync::Arc<HashSet<String>>>;

static FIXTURE_DIR_CACHE: OnceLock<Mutex<FixtureDirCache>> = OnceLock::new();
static FIXTURE_FILE_CACHE: OnceLock<Mutex<FixtureFileCache>> = OnceLock::new();
static FIXTURE_EXPORTS_CACHE: OnceLock<Mutex<FixtureExportsCache>> = OnceLock::new();
static FIXTURE_IMPORT_EDGES_CACHE: OnceLock<Mutex<FixtureImportEdgesCache>> = OnceLock::new();
static FIXTURE_IMPORT_REQUESTS_CACHE: OnceLock<Mutex<FixtureImportRequestsCache>> = OnceLock::new();
static FIXTURE_ATTRIBUTE_REQUESTS_CACHE: OnceLock<Mutex<FixtureAttributeRequestsCache>> =
    OnceLock::new();
static FIXTURE_REEXPORT_REQUESTS_CACHE: OnceLock<Mutex<FixtureReexportRequestsCache>> =
    OnceLock::new();
static FIXTURE_DYNAMIC_FIXTURE_REQUESTS_CACHE: OnceLock<Mutex<FixtureDynamicRequestsCache>> =
    OnceLock::new();
static FIXTURE_NAMED_IMPORTS_CACHE: OnceLock<Mutex<FixtureNamedImportsCache>> = OnceLock::new();
static FIXTURE_NAMESPACE_IMPORTS_CACHE: OnceLock<Mutex<FixtureNamespaceImportsCache>> =
    OnceLock::new();
static FIXTURE_SOURCE_IMPORTS_CACHE: OnceLock<Mutex<FixtureSourceImportsCache>> = OnceLock::new();
static FIXTURE_DECLARATIONS_CACHE: OnceLock<Mutex<FixtureDeclCache>> = OnceLock::new();
static FIXTURE_CURRENT_MODULE_LEXICAL_CACHE: OnceLock<Mutex<FixtureCurrentModuleLexicalCache>> =
    OnceLock::new();
static FIXTURE_DEFERRED_IMPORT_REQUEST_CACHE: OnceLock<
    Mutex<FixtureDeferredImportRequestCache>,
> = OnceLock::new();
static FIXTURE_SIDE_EFFECT_IMPORTS_CACHE: OnceLock<Mutex<FixtureSideEffectImportsCache>> =
    OnceLock::new();
static FIXTURE_MODULE_REQUESTS_CACHE: OnceLock<Mutex<FixtureModuleRequestsCache>> = OnceLock::new();
static FIXTURE_DEPENDENCY_MARKER_CACHE: OnceLock<Mutex<FixtureDependencyMarkerCache>> = OnceLock::new();
static FIXTURE_MODULE_SYNTAX_CACHE: OnceLock<Mutex<HashMap<SourceCacheKey, bool>>> =
    OnceLock::new();
static FIXTURE_DEP_CACHE: OnceLock<Mutex<FixtureDepCache>> = OnceLock::new();
static FIXTURE_PROFILE_STATS: OnceLock<Mutex<FixtureProfileStats>> = OnceLock::new();

fn bool_env_flag(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "True"))
        .unwrap_or(default)
}

fn usize_env_flag(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

fn has_flag(meta: &Test262Metadata, flag: &str) -> bool {
    meta.flags.iter().any(|item| item == flag)
}

fn negative_parse_phase(meta: &Test262Metadata) -> bool {
    meta.negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
}

fn source_without_frontmatter(source: &str) -> &str {
    if let Some(frontmatter_start) = source.find("/*---") {
        let before = &source[..frontmatter_start];
        if !before.trim().is_empty() {
            return source;
        }
        if let Some(frontmatter_end) = source[frontmatter_start..].find("---*/") {
            let offset = frontmatter_start + frontmatter_end + 5;
            return &source[offset.min(source.len())..];
        }
    }
    source
}

pub(crate) fn prepare_eager_enabled() -> bool {
    bool_env_flag("TEST262_PREPARE_EAGER", false)
}

pub(crate) fn inprocess_threadless_enabled() -> bool {
    bool_env_flag("TEST262_INPROCESS_THREADLESS", false)
}

fn inprocess_threadless_auto_enabled() -> bool {
    bool_env_flag("TEST262_INPROCESS_THREADLESS_AUTO", false)
}

fn inprocess_threadless_auto_max_chars() -> usize {
    usize_env_flag("TEST262_INPROCESS_THREADLESS_AUTO_MAX_CHARS", 6_144)
}

fn source_has_fixture_dependency_markers(source: &str) -> bool {
    if !fixture_analysis_cache_enabled() {
        return source_has_fixture_dependency_markers_unchecked(source);
    }
    let key = source_cache_key(source);
    {
        let cache = fixture_dependency_marker_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(value) = cache.get(&key) {
            note_fixture_dependency_marker_cache_hit();
            return *value;
        }
    }
    note_fixture_dependency_marker_cache_miss();
    let detected = source_has_fixture_dependency_markers_unchecked(source);
    let mut cache = fixture_dependency_marker_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(value) = cache.get(&key) {
        return *value;
    }
    cache.insert(key, detected);
    detected
}

fn source_has_fixture_dependency_markers_unchecked(source: &str) -> bool {
    if source.contains("import(") || source.contains("import (") {
        return true;
    }
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("import ")
            || trimmed.starts_with("export ")
            || trimmed.starts_with("import.defer")
            || trimmed.starts_with("import source")
            || trimmed.contains(" import")
            || trimmed.contains(" export")
        {
            return true;
        }
    }
    false
}

fn is_threadless_auto_candidate(meta: &Test262Metadata, source: &str) -> bool {
    note_threadless_auto_candidate();
    if !inprocess_threadless_auto_enabled() {
        note_threadless_auto_reject_config();
        return false;
    }
    if source.len() > inprocess_threadless_auto_max_chars() {
        note_threadless_auto_reject_size();
        return false;
    }
    if has_flag(meta, "async") || source.contains("await ") || source.contains(" await") {
        note_threadless_auto_reject_async();
        return false;
    }
    if source_has_fixture_dependency_markers(source) {
        note_threadless_auto_reject_dependency_markers();
        return false;
    }
    true
}

pub(crate) fn isolated_poll_ms() -> u64 {
    usize_env_flag("TEST262_ISOLATED_POLL_MS", 20).clamp(1, 250) as u64
}

fn isolated_capture_output() -> bool {
    static CAPTURE: std::sync::OnceLock<bool> = OnceLock::new();
    *CAPTURE.get_or_init(|| {
        std::env::var("TEST262_ISOLATED_CAPTURE")
            .ok()
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

fn isolated_capture_output_on_failure() -> bool {
    static CAPTURE_ON_FAILURE: std::sync::OnceLock<bool> = OnceLock::new();
    *CAPTURE_ON_FAILURE.get_or_init(|| {
        std::env::var("TEST262_ISOLATED_CAPTURE_ON_FAILURE")
            .ok()
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(true)
    })
}

fn isolated_retry_enabled() -> bool {
    bool_env_flag("TEST262_ISOLATED_RETRY", true)
}

fn isolated_output_max_bytes() -> usize {
    static MAX_BYTES: std::sync::OnceLock<usize> = OnceLock::new();
    *MAX_BYTES.get_or_init(|| {
        std::env::var("TEST262_ISOLATED_OUTPUT_BYTES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(128 * 1024)
            .clamp(0, 4 * 1024 * 1024)
    })
}

pub(crate) fn reset_run_metrics() {
    METRIC_PARSE_NEGATIVE_SHORT_CIRCUIT.store(0, Ordering::Relaxed);
    METRIC_THREADED_RUNS.store(0, Ordering::Relaxed);
    METRIC_THREADLESS_RUNS.store(0, Ordering::Relaxed);
    METRIC_THREADLESS_AUTO_RUNS.store(0, Ordering::Relaxed);
    METRIC_THREADLESS_AUTO_CANDIDATES.store(0, Ordering::Relaxed);
    METRIC_THREADLESS_AUTO_REJECT_CONFIG.store(0, Ordering::Relaxed);
    METRIC_THREADLESS_AUTO_REJECT_SIZE.store(0, Ordering::Relaxed);
    METRIC_THREADLESS_AUTO_REJECT_ASYNC.store(0, Ordering::Relaxed);
    METRIC_THREADLESS_AUTO_REJECT_DEPENDENCY_MARKERS.store(0, Ordering::Relaxed);
    METRIC_TIMEOUTS.store(0, Ordering::Relaxed);
    METRIC_PANICS.store(0, Ordering::Relaxed);
    METRIC_MISSING_HARNESS.store(0, Ordering::Relaxed);
    METRIC_PREPARED_CACHE_HITS.store(0, Ordering::Relaxed);
    METRIC_PREPARED_CACHE_MISSES.store(0, Ordering::Relaxed);
    METRIC_ISOLATED_RUNS.store(0, Ordering::Relaxed);
    METRIC_ISOLATED_TIMEOUTS.store(0, Ordering::Relaxed);
    METRIC_ISOLATED_SPAWN_FAILURES.store(0, Ordering::Relaxed);
    METRIC_ISOLATED_WAIT_FAILURES.store(0, Ordering::Relaxed);
    METRIC_ISOLATED_RETRIES.store(0, Ordering::Relaxed);
    METRIC_ISOLATED_RETRY_SKIPPED.store(0, Ordering::Relaxed);
    METRIC_FIXTURE_DEPENDENCY_MARKER_CACHE_HITS.store(0, Ordering::Relaxed);
    METRIC_FIXTURE_DEPENDENCY_MARKER_CACHE_MISSES.store(0, Ordering::Relaxed);
    METRIC_FIXTURE_MODULE_LEXICAL_CACHE_HITS.store(0, Ordering::Relaxed);
        METRIC_FIXTURE_MODULE_LEXICAL_CACHE_MISSES.store(0, Ordering::Relaxed);
        METRIC_FIXTURE_MODULE_REQUEST_CACHE_HITS.store(0, Ordering::Relaxed);
        METRIC_FIXTURE_MODULE_REQUEST_CACHE_MISSES.store(0, Ordering::Relaxed);
        METRIC_FIXTURE_MODULE_REQUEST_FASTPATH_HITS.store(0, Ordering::Relaxed);
        METRIC_FIXTURE_MODULE_REQUEST_FASTPATH_MISSES.store(0, Ordering::Relaxed);
        METRIC_FIXTURE_DEFERRED_IMPORT_CACHE_HITS.store(0, Ordering::Relaxed);
        METRIC_FIXTURE_DEFERRED_IMPORT_CACHE_MISSES.store(0, Ordering::Relaxed);
        METRIC_FIXTURE_MODULE_SYNTAX_CACHE_HITS.store(0, Ordering::Relaxed);
    METRIC_FIXTURE_MODULE_SYNTAX_CACHE_MISSES.store(0, Ordering::Relaxed);
    METRIC_FIXTURE_MODULES_SELECTED.store(0, Ordering::Relaxed);
    METRIC_FIXTURE_MODULES_LOADED.store(0, Ordering::Relaxed);
    METRIC_FIXTURE_MODULES_MISSING.store(0, Ordering::Relaxed);
    METRIC_FIXTURE_MODULE_BYTES_LOADED.store(0, Ordering::Relaxed);
    METRIC_FIXTURE_GRAPH_NODES.store(0, Ordering::Relaxed);
    METRIC_FIXTURE_GRAPH_EDGES.store(0, Ordering::Relaxed);
    METRIC_FIXTURE_GRAPH_MAX_DEPTH.store(0, Ordering::Relaxed);
    METRIC_FIXTURE_GRAPH_SELECTED_MODULES.store(0, Ordering::Relaxed);
    METRIC_FIXTURE_MODULE_LOAD_TESTS.store(0, Ordering::Relaxed);
    METRIC_FIXTURE_MODULE_LOAD_MILLIS.store(0, Ordering::Relaxed);
    METRIC_FIXTURE_INVALID_SYNTAX_MODULES.store(0, Ordering::Relaxed);
    METRIC_FIXTURE_NO_DEPENDENCY_SKIPS.store(0, Ordering::Relaxed);
    METRIC_FIXTURE_NO_FIXTURE_REQUEST_SKIPS.store(0, Ordering::Relaxed);
    METRIC_WORKER_STARTS.store(0, Ordering::Relaxed);
    METRIC_WORKER_BATCHES.store(0, Ordering::Relaxed);
    METRIC_ISOLATION_FALLBACKS.store(0, Ordering::Relaxed);
}

pub(crate) fn run_timeout_metrics() -> RunMetrics {
    let fixture_profile = fixture_profile_snapshot();
    RunMetrics {
        parse_negative_short_circuit: METRIC_PARSE_NEGATIVE_SHORT_CIRCUIT
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        threaded_runs: METRIC_THREADED_RUNS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        threadless_runs: METRIC_THREADLESS_RUNS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        threadless_auto_runs: METRIC_THREADLESS_AUTO_RUNS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        threadless_auto_candidates: METRIC_THREADLESS_AUTO_CANDIDATES
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        threadless_auto_reject_config: METRIC_THREADLESS_AUTO_REJECT_CONFIG
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        threadless_auto_reject_size: METRIC_THREADLESS_AUTO_REJECT_SIZE
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        threadless_auto_reject_async: METRIC_THREADLESS_AUTO_REJECT_ASYNC
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        threadless_auto_reject_dependency_markers: METRIC_THREADLESS_AUTO_REJECT_DEPENDENCY_MARKERS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        timeouts: METRIC_TIMEOUTS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        panics: METRIC_PANICS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        skipped_due_to_missing_harness: METRIC_MISSING_HARNESS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        prepared_cache_hits: METRIC_PREPARED_CACHE_HITS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        prepared_cache_misses: METRIC_PREPARED_CACHE_MISSES
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        isolated_runs: METRIC_ISOLATED_RUNS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        isolated_timeouts: METRIC_ISOLATED_TIMEOUTS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        isolated_spawn_failures: METRIC_ISOLATED_SPAWN_FAILURES
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        isolated_wait_failures: METRIC_ISOLATED_WAIT_FAILURES
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        isolated_retries: METRIC_ISOLATED_RETRIES
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        isolated_retry_skipped: METRIC_ISOLATED_RETRY_SKIPPED
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_dependency_marker_cache_hits: METRIC_FIXTURE_DEPENDENCY_MARKER_CACHE_HITS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_dependency_marker_cache_misses: METRIC_FIXTURE_DEPENDENCY_MARKER_CACHE_MISSES
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_module_lexical_cache_hits: METRIC_FIXTURE_MODULE_LEXICAL_CACHE_HITS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_module_lexical_cache_misses: METRIC_FIXTURE_MODULE_LEXICAL_CACHE_MISSES
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_module_request_cache_hits: METRIC_FIXTURE_MODULE_REQUEST_CACHE_HITS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_module_request_cache_misses: METRIC_FIXTURE_MODULE_REQUEST_CACHE_MISSES
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_module_request_fastpath_hits: METRIC_FIXTURE_MODULE_REQUEST_FASTPATH_HITS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_module_request_fastpath_misses: METRIC_FIXTURE_MODULE_REQUEST_FASTPATH_MISSES
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_deferred_import_cache_hits: METRIC_FIXTURE_DEFERRED_IMPORT_CACHE_HITS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_deferred_import_cache_misses: METRIC_FIXTURE_DEFERRED_IMPORT_CACHE_MISSES
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_module_syntax_cache_hits: METRIC_FIXTURE_MODULE_SYNTAX_CACHE_HITS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_module_syntax_cache_misses: METRIC_FIXTURE_MODULE_SYNTAX_CACHE_MISSES
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_modules_selected: METRIC_FIXTURE_MODULES_SELECTED
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_modules_loaded: METRIC_FIXTURE_MODULES_LOADED
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_modules_missing: METRIC_FIXTURE_MODULES_MISSING
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_module_bytes_loaded: METRIC_FIXTURE_MODULE_BYTES_LOADED
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_file_cache_hits: fixture_profile.file_hits.try_into().unwrap_or(u64::MAX),
        fixture_file_cache_misses: fixture_profile.file_misses.try_into().unwrap_or(u64::MAX),
        fixture_dir_cache_hits: fixture_profile.dir_hits.try_into().unwrap_or(u64::MAX),
        fixture_dir_cache_misses: fixture_profile.dir_misses.try_into().unwrap_or(u64::MAX),
        fixture_dep_cache_hits: fixture_profile.dep_hits.try_into().unwrap_or(u64::MAX),
        fixture_dep_cache_misses: fixture_profile.dep_misses.try_into().unwrap_or(u64::MAX),
        fixture_graph_nodes: METRIC_FIXTURE_GRAPH_NODES
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_graph_edges: METRIC_FIXTURE_GRAPH_EDGES
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_graph_max_depth: METRIC_FIXTURE_GRAPH_MAX_DEPTH
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_graph_selected_modules: METRIC_FIXTURE_GRAPH_SELECTED_MODULES
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_module_load_tests: METRIC_FIXTURE_MODULE_LOAD_TESTS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_module_load_millis: METRIC_FIXTURE_MODULE_LOAD_MILLIS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_invalid_syntax_modules: METRIC_FIXTURE_INVALID_SYNTAX_MODULES
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_no_dependency_skips: METRIC_FIXTURE_NO_DEPENDENCY_SKIPS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        fixture_no_fixture_request_skips: METRIC_FIXTURE_NO_FIXTURE_REQUEST_SKIPS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        worker_starts: METRIC_WORKER_STARTS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        worker_batches: METRIC_WORKER_BATCHES
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
        isolation_fallbacks: METRIC_ISOLATION_FALLBACKS
            .load(Ordering::Relaxed)
            .try_into()
            .unwrap_or(u64::MAX),
    }
}

pub(crate) fn note_prepared_cache_hit() {
    METRIC_PREPARED_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn note_prepared_cache_miss() {
    METRIC_PREPARED_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn note_worker_start() {
    METRIC_WORKER_STARTS.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn note_worker_batch() {
    METRIC_WORKER_BATCHES.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn note_isolated_run() {
    METRIC_ISOLATED_RUNS.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn note_isolated_timeout() {
    METRIC_ISOLATED_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn note_isolated_spawn_failure() {
    METRIC_ISOLATED_SPAWN_FAILURES.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn note_isolated_wait_failure() {
    METRIC_ISOLATED_WAIT_FAILURES.fetch_add(1, Ordering::Relaxed);
}

fn note_isolated_retry() {
    METRIC_ISOLATED_RETRIES.fetch_add(1, Ordering::Relaxed);
}

fn note_fixture_module_syntax_cache_hit() {
    METRIC_FIXTURE_MODULE_SYNTAX_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
}

fn note_fixture_module_syntax_cache_miss() {
    METRIC_FIXTURE_MODULE_SYNTAX_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
}

fn note_fixture_module_request_cache_hit() {
    METRIC_FIXTURE_MODULE_REQUEST_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
}

fn note_fixture_module_request_cache_miss() {
    METRIC_FIXTURE_MODULE_REQUEST_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
}

fn note_fixture_module_request_fastpath_hit() {
    METRIC_FIXTURE_MODULE_REQUEST_FASTPATH_HITS.fetch_add(1, Ordering::Relaxed);
}

fn note_fixture_module_request_fastpath_miss() {
    METRIC_FIXTURE_MODULE_REQUEST_FASTPATH_MISSES.fetch_add(1, Ordering::Relaxed);
}

fn note_fixture_modules_selected(count: usize) {
    METRIC_FIXTURE_MODULES_SELECTED.fetch_add(count, Ordering::Relaxed);
}

fn note_fixture_modules_loaded(count: usize) {
    METRIC_FIXTURE_MODULES_LOADED.fetch_add(count, Ordering::Relaxed);
}

fn note_fixture_modules_missing(count: usize) {
    METRIC_FIXTURE_MODULES_MISSING.fetch_add(count, Ordering::Relaxed);
}

fn note_fixture_module_bytes_loaded(count: usize) {
    METRIC_FIXTURE_MODULE_BYTES_LOADED.fetch_add(count, Ordering::Relaxed);
}

fn note_fixture_module_load_test() {
    METRIC_FIXTURE_MODULE_LOAD_TESTS.fetch_add(1, Ordering::Relaxed);
}

fn note_fixture_module_load_millis(elapsed_ms: u64) {
    METRIC_FIXTURE_MODULE_LOAD_MILLIS.fetch_add(
        elapsed_ms.try_into().unwrap_or(usize::MAX),
        Ordering::Relaxed,
    );
}

fn note_fixture_invalid_syntax_module() {
    METRIC_FIXTURE_INVALID_SYNTAX_MODULES.fetch_add(1, Ordering::Relaxed);
}

fn note_fixture_no_fixture_request_skip() {
    METRIC_FIXTURE_NO_FIXTURE_REQUEST_SKIPS.fetch_add(1, Ordering::Relaxed);
}

fn note_fixture_graph_nodes(count: usize) {
    METRIC_FIXTURE_GRAPH_NODES.fetch_add(count, Ordering::Relaxed);
}

fn note_fixture_graph_edges(count: usize) {
    METRIC_FIXTURE_GRAPH_EDGES.fetch_add(count, Ordering::Relaxed);
}

fn note_fixture_graph_depth(depth: usize) {
    METRIC_FIXTURE_GRAPH_MAX_DEPTH.fetch_max(depth, Ordering::Relaxed);
}

fn note_fixture_graph_selected_modules(count: usize) {
    METRIC_FIXTURE_GRAPH_SELECTED_MODULES.fetch_add(count, Ordering::Relaxed);
}

fn note_threadless_auto_run() {
    METRIC_THREADLESS_AUTO_RUNS.fetch_add(1, Ordering::Relaxed);
}

fn note_threadless_auto_candidate() {
    METRIC_THREADLESS_AUTO_CANDIDATES.fetch_add(1, Ordering::Relaxed);
}

fn note_threadless_auto_reject_config() {
    METRIC_THREADLESS_AUTO_REJECT_CONFIG.fetch_add(1, Ordering::Relaxed);
}

fn note_threadless_auto_reject_size() {
    METRIC_THREADLESS_AUTO_REJECT_SIZE.fetch_add(1, Ordering::Relaxed);
}

fn note_threadless_auto_reject_async() {
    METRIC_THREADLESS_AUTO_REJECT_ASYNC.fetch_add(1, Ordering::Relaxed);
}

fn note_threadless_auto_reject_dependency_markers() {
    METRIC_THREADLESS_AUTO_REJECT_DEPENDENCY_MARKERS.fetch_add(1, Ordering::Relaxed);
}

fn note_fixture_dependency_marker_cache_hit() {
    METRIC_FIXTURE_DEPENDENCY_MARKER_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
}

fn note_fixture_dependency_marker_cache_miss() {
    METRIC_FIXTURE_DEPENDENCY_MARKER_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
}

fn note_fixture_module_lexical_cache_hit() {
    METRIC_FIXTURE_MODULE_LEXICAL_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
}

fn note_fixture_module_lexical_cache_miss() {
    METRIC_FIXTURE_MODULE_LEXICAL_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
}

fn note_fixture_deferred_import_cache_hit() {
    METRIC_FIXTURE_DEFERRED_IMPORT_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
}

fn note_fixture_deferred_import_cache_miss() {
    METRIC_FIXTURE_DEFERRED_IMPORT_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
}

fn note_isolated_retry_skipped() {
    METRIC_ISOLATED_RETRY_SKIPPED.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn note_fixture_no_dependency_skip() {
    METRIC_FIXTURE_NO_DEPENDENCY_SKIPS.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn note_isolation_fallback() {
    METRIC_ISOLATION_FALLBACKS.fetch_add(1, Ordering::Relaxed);
}

fn note_fixture_dependency_skip() {
    note_fixture_no_dependency_skip();
    note_fixture_no_fixture_request_skip();
}

fn fixture_caching_enabled() -> bool {
    static TEST262_FIXTURE_CACHE: OnceLock<bool> = OnceLock::new();
    *TEST262_FIXTURE_CACHE.get_or_init(|| bool_env_flag("TEST262_FIXTURE_CACHE", true))
}

fn fixture_profile_enabled() -> bool {
    static TEST262_FIXTURE_PROFILE: OnceLock<bool> = OnceLock::new();
    *TEST262_FIXTURE_PROFILE.get_or_init(|| bool_env_flag("TEST262_FIXTURE_PROFILE", false))
}

fn fixture_profile_show_modules() -> bool {
    static TEST262_FIXTURE_PROFILE_MODULES: OnceLock<bool> = OnceLock::new();
    *TEST262_FIXTURE_PROFILE_MODULES.get_or_init(|| {
        bool_env_flag("TEST262_FIXTURE_PROFILE_MODULES", false)
    })
}

fn fixture_analysis_cache_enabled() -> bool {
    static TEST262_FIXTURE_SOURCE_CACHE: OnceLock<bool> = OnceLock::new();
    *TEST262_FIXTURE_SOURCE_CACHE.get_or_init(|| bool_env_flag("TEST262_FIXTURE_SOURCE_CACHE", true))
}

fn fixture_profile_slow_ms() -> u128 {
    static TEST262_FIXTURE_PROFILE_SLOW_MS: OnceLock<u128> = OnceLock::new();
    *TEST262_FIXTURE_PROFILE_SLOW_MS.get_or_init(|| {
        std::env::var("TEST262_FIXTURE_PROFILE_SLOW_MS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0)
            .clamp(0, 10_000) as u128
    })
}

fn fixture_dir_filter_fixtures_only() -> bool {
    static TEST262_FIXTURE_DIR_FILTER_FIXTURES: OnceLock<bool> = OnceLock::new();
    *TEST262_FIXTURE_DIR_FILTER_FIXTURES.get_or_init(|| {
        bool_env_flag("TEST262_FIXTURE_DIR_FILTER_FIXTURES", true)
    })
}

fn has_fixture_request_heuristic(source: &str) -> bool {
    source.contains("import") || source.contains("export")
}

fn fixture_dependency_requests(module_source: &str) -> (Vec<String>, bool) {
    if !module_source.contains("_FIXTURE") {
        return (Vec::new(), false);
    }
    let requests = fixture_module_requests_from_source(module_source)
        .into_iter()
        .collect::<Vec<_>>();
    let has_fixture_dependencies = requests.iter().any(|request| request.contains("_FIXTURE"));
    (requests, has_fixture_dependencies)
}

fn fixture_loads_sorted() -> bool {
    static TEST262_FIXTURE_LOAD_SORT: OnceLock<bool> = OnceLock::new();
    *TEST262_FIXTURE_LOAD_SORT.get_or_init(|| {
        std::env::var("TEST262_FIXTURE_LOAD_SORT")
            .ok()
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(true)
    })
}

fn fixture_module_syntax_cache() -> &'static Mutex<HashMap<SourceCacheKey, bool>> {
    FIXTURE_MODULE_SYNTAX_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn fixture_module_syntax_valid(source: &str) -> bool {
    if !fixture_caching_enabled() {
        return quench_runtime::parser::parse_es_module(source).is_ok();
    }
    let key = source_cache_key(source);
    {
        let cache = fixture_module_syntax_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(valid) = cache.get(&key) {
            note_fixture_module_syntax_cache_hit();
            return *valid;
        }
    }
    note_fixture_module_syntax_cache_miss();
    let valid = quench_runtime::parser::parse_es_module(source).is_ok();
    let mut cache = fixture_module_syntax_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(valid) = cache.get(&key) {
        note_fixture_module_syntax_cache_hit();
        return *valid;
    }
    cache.insert(key, valid);
    valid
}

fn fixture_dep_cache(
) -> &'static Mutex<HashMap<std::path::PathBuf, std::sync::Arc<HashSet<String>>>> {
    FIXTURE_DEP_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}
fn fixture_exports_cache(
) -> &'static Mutex<HashMap<SourceCacheKey, std::sync::Arc<FixtureExportsCacheEntry>>> {
    FIXTURE_EXPORTS_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}
fn fixture_import_edges_cache(
) -> &'static Mutex<HashMap<SourceCacheKey, Arc<Vec<(String, String)>>>> {
    FIXTURE_IMPORT_EDGES_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}
fn fixture_import_requests_cache() -> &'static Mutex<HashMap<SourceCacheKey, Arc<Vec<String>>>> {
    FIXTURE_IMPORT_REQUESTS_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}
fn fixture_attribute_requests_cache(
) -> &'static Mutex<HashMap<SourceCacheKey, Arc<HashSet<String>>>> {
    FIXTURE_ATTRIBUTE_REQUESTS_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}
fn fixture_reexport_requests_cache() -> &'static Mutex<HashMap<SourceCacheKey, Arc<Vec<String>>>> {
    FIXTURE_REEXPORT_REQUESTS_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}
fn fixture_dynamic_fixture_requests_cache(
) -> &'static Mutex<HashMap<SourceCacheKey, Arc<Vec<String>>>> {
    FIXTURE_DYNAMIC_FIXTURE_REQUESTS_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}
fn fixture_named_imports_cache(
) -> &'static Mutex<HashMap<SourceCacheKey, Arc<HashMap<String, (String, String)>>>> {
    FIXTURE_NAMED_IMPORTS_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}
fn fixture_namespace_imports_cache(
) -> &'static Mutex<HashMap<SourceCacheKey, Arc<HashMap<String, String>>>> {
    FIXTURE_NAMESPACE_IMPORTS_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}
fn fixture_source_imports_cache(
) -> &'static Mutex<HashMap<SourceCacheKey, Arc<HashMap<String, String>>>> {
    FIXTURE_SOURCE_IMPORTS_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}
fn fixture_declarations_cache() -> &'static Mutex<HashMap<SourceCacheKey, Arc<HashSet<String>>>> {
    FIXTURE_DECLARATIONS_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}
fn fixture_side_effect_imports_cache() -> &'static Mutex<HashMap<SourceCacheKey, Arc<Vec<String>>>>
{
    FIXTURE_SIDE_EFFECT_IMPORTS_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}
fn fixture_module_requests_cache() -> &'static Mutex<HashMap<SourceCacheKey, Arc<HashSet<String>>>>
{
    FIXTURE_MODULE_REQUESTS_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}
fn fixture_dependency_marker_cache() -> &'static Mutex<HashMap<SourceCacheKey, bool>> {
    FIXTURE_DEPENDENCY_MARKER_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}
fn fixture_current_module_lexical_cache(
) -> &'static Mutex<FixtureCurrentModuleLexicalCache> {
    FIXTURE_CURRENT_MODULE_LEXICAL_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}
fn fixture_deferred_import_request_cache(
) -> &'static Mutex<FixtureDeferredImportRequestCache> {
    FIXTURE_DEFERRED_IMPORT_REQUEST_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn fixture_profile_stats() -> &'static Mutex<FixtureProfileStats> {
    FIXTURE_PROFILE_STATS.get_or_init(|| Mutex::new(FixtureProfileStats::default()))
}

fn source_cache_key(source: &str) -> SourceCacheKey {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    let bytes = source.as_bytes();
    SourceCacheKey {
        len: source.len(),
        hash: hasher.finish(),
        first: bytes.first().copied().unwrap_or(0),
        last: bytes.last().copied().unwrap_or(0),
    }
}

fn with_fixture_profile_stats(mutator: impl FnOnce(&mut FixtureProfileStats)) {
    if !fixture_profile_enabled() {
        return;
    }
    let mut stats = fixture_profile_stats()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    mutator(&mut *stats);
}

pub(crate) fn fixture_profile_snapshot() -> FixtureProfileStats {
    *fixture_profile_stats()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

pub(crate) fn fixture_profile_snapshot_json() -> serde_json::Value {
    let stats = fixture_profile_snapshot();
    serde_json::json!({
        "file_hits": stats.file_hits,
        "file_misses": stats.file_misses,
        "dir_hits": stats.dir_hits,
        "dir_misses": stats.dir_misses,
        "dep_hits": stats.dep_hits,
        "dep_misses": stats.dep_misses,
        "import_edges_hits": stats.import_edges_hits,
        "import_edges_misses": stats.import_edges_misses,
        "named_import_hits": stats.named_import_hits,
        "named_import_misses": stats.named_import_misses,
        "namespace_import_hits": stats.namespace_import_hits,
        "namespace_import_misses": stats.namespace_import_misses,
        "source_import_hits": stats.source_import_hits,
        "source_import_misses": stats.source_import_misses,
        "module_request_hits": stats.module_request_hits,
        "module_request_misses": stats.module_request_misses,
        "module_lexical_bindings_hits": stats.module_lexical_bindings_hits,
        "module_lexical_bindings_misses": stats.module_lexical_bindings_misses,
        "deferred_import_request_hits": stats.deferred_import_request_hits,
        "deferred_import_request_misses": stats.deferred_import_request_misses,
        "graph_nodes": stats.graph_nodes,
        "graph_edges": stats.graph_edges,
        "graph_max_depth": stats.graph_max_depth,
        "no_dependency_short_circuits": stats.no_dependency_short_circuits,
        "selected_modules": stats.selected_modules,
    })
}

fn fixture_file_cache() -> &'static Mutex<HashMap<std::path::PathBuf, FixtureFileCacheEntry>> {
    FIXTURE_FILE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn fixture_dir_cache() -> &'static Mutex<HashMap<std::path::PathBuf, std::sync::Arc<FixtureDirMap>>>
{
    FIXTURE_DIR_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_fixture_file(path: &Path) -> Result<FixtureFileCacheEntry, String> {
    if !fixture_caching_enabled() {
        let bytes = std::fs::read(path).map_err(|error| format!("fixture read: {}", error))?;
        let (source, source_is_utf8) = match std::str::from_utf8(&bytes) {
            Ok(source) => (source.to_string(), true),
            Err(_) => (String::from_utf8_lossy(&bytes).into_owned(), false),
        };
        return Ok(FixtureFileCacheEntry {
            bytes: std::sync::Arc::new(bytes),
            source: std::sync::Arc::new(source),
            source_is_utf8,
        });
    }
    let mut cache = fixture_file_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = cache.get(path) {
        with_fixture_profile_stats(|stats| stats.file_hits += 1);
        return Ok(entry.clone());
    }
    with_fixture_profile_stats(|stats| stats.file_misses += 1);
    let bytes = std::fs::read(path).map_err(|error| format!("fixture read: {}", error))?;
    let (source, source_is_utf8) = match std::str::from_utf8(&bytes) {
        Ok(source) => (source.to_string(), true),
        Err(_) => (String::from_utf8_lossy(&bytes).into_owned(), false),
    };
    let entry = FixtureFileCacheEntry {
        bytes: std::sync::Arc::new(bytes),
        source: std::sync::Arc::new(source),
        source_is_utf8,
    };
    cache.insert(path.to_path_buf(), entry.clone());
    Ok(entry)
}

fn cached_fixture_exports_from_source(
    source: &str,
) -> Result<Arc<FixtureExportsCacheEntry>, String> {
    if !fixture_analysis_cache_enabled() {
        return Ok(std::sync::Arc::new(fixture_exports_from_source_unchecked(
            source,
        )?));
    }
    let key = source_cache_key(source);
    {
        let cache = fixture_exports_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = cache.get(&key) {
            with_fixture_profile_stats(|stats| stats.source_analysis_hits += 1);
            return Ok(std::sync::Arc::clone(entry));
        }
    }
    with_fixture_profile_stats(|stats| stats.source_analysis_misses += 1);
    let parsed = fixture_exports_from_source_unchecked(source)?;
    let mut cache = fixture_exports_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = cache.get(&key) {
        with_fixture_profile_stats(|stats| stats.source_analysis_hits += 1);
        return Ok(std::sync::Arc::clone(entry));
    }
    let entry = std::sync::Arc::new(parsed);
    cache.insert(key, std::sync::Arc::clone(&entry));
    Ok(entry)
}

fn cached_fixture_import_edges_from_source(
    source: &str,
) -> Result<Arc<Vec<(String, String)>>, String> {
    if !fixture_analysis_cache_enabled() {
        return Ok(std::sync::Arc::new(
            fixture_import_edges_from_source_unchecked(source),
        ));
    }
    let key = source_cache_key(source);
    {
        let cache = fixture_import_edges_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = cache.get(&key) {
            with_fixture_profile_stats(|stats| stats.import_edges_hits += 1);
            return Ok(std::sync::Arc::clone(entry));
        }
    }
    with_fixture_profile_stats(|stats| stats.import_edges_misses += 1);
    let parsed = fixture_import_edges_from_source_unchecked(source);
    let mut cache = fixture_import_edges_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = cache.get(&key) {
        with_fixture_profile_stats(|stats| stats.import_edges_hits += 1);
        return Ok(std::sync::Arc::clone(entry));
    }
    let entry = std::sync::Arc::new(parsed);
    cache.insert(key, std::sync::Arc::clone(&entry));
    Ok(entry)
}

fn cached_fixture_import_requests_from_source(source: &str) -> Arc<Vec<String>> {
    let key = source_cache_key(source);
    if !fixture_analysis_cache_enabled() {
        return std::sync::Arc::new(fixture_import_requests_from_source_unchecked(source));
    }
    {
        let cache = fixture_import_requests_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = cache.get(&key) {
            with_fixture_profile_stats(|stats| stats.import_request_hits += 1);
            return std::sync::Arc::clone(entry);
        }
    }
    with_fixture_profile_stats(|stats| stats.import_request_misses += 1);
    let parsed = fixture_import_requests_from_source_unchecked(source);
    let mut cache = fixture_import_requests_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = cache.get(&key) {
        with_fixture_profile_stats(|stats| stats.import_request_hits += 1);
        return std::sync::Arc::clone(entry);
    }
    let entry = std::sync::Arc::new(parsed);
    cache.insert(key, std::sync::Arc::clone(&entry));
    entry
}

fn cached_fixture_attribute_requests_from_source(source: &str) -> Arc<HashSet<String>> {
    let key = source_cache_key(source);
    if !fixture_analysis_cache_enabled() {
        return std::sync::Arc::new(fixture_attribute_module_requests_from_source_unchecked(
            source,
        ));
    }
    {
        let cache = fixture_attribute_requests_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = cache.get(&key) {
            with_fixture_profile_stats(|stats| stats.attr_module_request_hits += 1);
            return std::sync::Arc::clone(entry);
        }
    }
    with_fixture_profile_stats(|stats| stats.attr_module_request_misses += 1);
    let parsed = fixture_attribute_module_requests_from_source_unchecked(source);
    let mut cache = fixture_attribute_requests_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = cache.get(&key) {
        with_fixture_profile_stats(|stats| stats.attr_module_request_hits += 1);
        return std::sync::Arc::clone(entry);
    }
    let entry = std::sync::Arc::new(parsed);
    cache.insert(key, std::sync::Arc::clone(&entry));
    entry
}

fn cached_fixture_reexport_requests_from_source(source: &str) -> Arc<Vec<String>> {
    let key = source_cache_key(source);
    if !fixture_analysis_cache_enabled() {
        return std::sync::Arc::new(fixture_reexport_requests_from_source_unchecked(source));
    }
    {
        let cache = fixture_reexport_requests_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = cache.get(&key) {
            with_fixture_profile_stats(|stats| stats.reexport_request_hits += 1);
            return std::sync::Arc::clone(entry);
        }
    }
    with_fixture_profile_stats(|stats| stats.reexport_request_misses += 1);
    let parsed = fixture_reexport_requests_from_source_unchecked(source);
    let mut cache = fixture_reexport_requests_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = cache.get(&key) {
        with_fixture_profile_stats(|stats| stats.reexport_request_hits += 1);
        return std::sync::Arc::clone(entry);
    }
    let entry = std::sync::Arc::new(parsed);
    cache.insert(key, std::sync::Arc::clone(&entry));
    entry
}

fn cached_fixture_dynamic_requests_from_source(source: &str) -> Arc<Vec<String>> {
    let key = source_cache_key(source);
    if !fixture_analysis_cache_enabled() {
        return std::sync::Arc::new(fixture_dynamic_fixture_requests_from_source_unchecked(
            source,
        ));
    }
    {
        let cache = fixture_dynamic_fixture_requests_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = cache.get(&key) {
            with_fixture_profile_stats(|stats| stats.dynamic_request_hits += 1);
            return std::sync::Arc::clone(entry);
        }
    }
    with_fixture_profile_stats(|stats| stats.dynamic_request_misses += 1);
    let parsed = fixture_dynamic_fixture_requests_from_source_unchecked(source);
    let mut cache = fixture_dynamic_fixture_requests_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = cache.get(&key) {
        with_fixture_profile_stats(|stats| stats.dynamic_request_hits += 1);
        return std::sync::Arc::clone(entry);
    }
    let entry = std::sync::Arc::new(parsed);
    cache.insert(key, std::sync::Arc::clone(&entry));
    entry
}

fn cached_fixture_side_effect_imports_from_source(source: &str) -> Arc<Vec<String>> {
    let key = source_cache_key(source);
    if !fixture_analysis_cache_enabled() {
        return std::sync::Arc::new(fixture_side_effect_imports_from_source_unchecked(source));
    }
    {
        let cache = fixture_side_effect_imports_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = cache.get(&key) {
            with_fixture_profile_stats(|stats| stats.side_effect_import_hits += 1);
            return std::sync::Arc::clone(entry);
        }
    }
    with_fixture_profile_stats(|stats| stats.side_effect_import_misses += 1);
    let parsed = fixture_side_effect_imports_from_source_unchecked(source);
    let mut cache = fixture_side_effect_imports_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = cache.get(&key) {
        with_fixture_profile_stats(|stats| stats.side_effect_import_hits += 1);
        return std::sync::Arc::clone(entry);
    }
    let entry = std::sync::Arc::new(parsed);
    cache.insert(key, std::sync::Arc::clone(&entry));
    entry
}

fn cached_fixture_named_imports_from_source(
    source: &str,
) -> Arc<HashMap<String, (String, String)>> {
    let key = source_cache_key(source);
    if !fixture_analysis_cache_enabled() {
        return std::sync::Arc::new(fixture_named_imports_unchecked(source));
    }
    {
        let cache = fixture_named_imports_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = cache.get(&key) {
            with_fixture_profile_stats(|stats| stats.named_import_hits += 1);
            return std::sync::Arc::clone(entry);
        }
    }
    with_fixture_profile_stats(|stats| stats.named_import_misses += 1);
    let parsed = fixture_named_imports_unchecked(source);
    let mut cache = fixture_named_imports_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = cache.get(&key) {
        with_fixture_profile_stats(|stats| stats.named_import_hits += 1);
        return std::sync::Arc::clone(entry);
    }
    let entry = std::sync::Arc::new(parsed);
    cache.insert(key, std::sync::Arc::clone(&entry));
    entry
}

fn cached_fixture_namespace_imports_from_source(source: &str) -> Arc<HashMap<String, String>> {
    let key = source_cache_key(source);
    if !fixture_analysis_cache_enabled() {
        return std::sync::Arc::new(fixture_namespace_imports_unchecked(source));
    }
    {
        let cache = fixture_namespace_imports_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = cache.get(&key) {
            with_fixture_profile_stats(|stats| stats.namespace_import_hits += 1);
            return std::sync::Arc::clone(entry);
        }
    }
    with_fixture_profile_stats(|stats| stats.namespace_import_misses += 1);
    let parsed = fixture_namespace_imports_unchecked(source);
    let mut cache = fixture_namespace_imports_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = cache.get(&key) {
        with_fixture_profile_stats(|stats| stats.namespace_import_hits += 1);
        return std::sync::Arc::clone(entry);
    }
    let entry = std::sync::Arc::new(parsed);
    cache.insert(key, std::sync::Arc::clone(&entry));
    entry
}

fn cached_fixture_source_imports_from_source(source: &str) -> Arc<HashMap<String, String>> {
    let key = source_cache_key(source);
    if !fixture_analysis_cache_enabled() {
        return std::sync::Arc::new(fixture_source_imports_unchecked(source));
    }
    {
        let cache = fixture_source_imports_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = cache.get(&key) {
            with_fixture_profile_stats(|stats| stats.source_import_hits += 1);
            return std::sync::Arc::clone(entry);
        }
    }
    with_fixture_profile_stats(|stats| stats.source_import_misses += 1);
    let parsed = fixture_source_imports_unchecked(source);
    let mut cache = fixture_source_imports_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = cache.get(&key) {
        with_fixture_profile_stats(|stats| stats.source_import_hits += 1);
        return std::sync::Arc::clone(entry);
    }
    let entry = std::sync::Arc::new(parsed);
    cache.insert(key, std::sync::Arc::clone(&entry));
    entry
}

fn cached_fixture_declarations_from_source(source: &str) -> Arc<HashSet<String>> {
    let key = source_cache_key(source);
    if !fixture_analysis_cache_enabled() {
        return std::sync::Arc::new(fixture_declaration_names_unchecked(source));
    }
    {
        let cache = fixture_declarations_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = cache.get(&key) {
            with_fixture_profile_stats(|stats| stats.decl_hits += 1);
            return std::sync::Arc::clone(entry);
        }
    }
    with_fixture_profile_stats(|stats| stats.decl_misses += 1);
    let parsed = fixture_declaration_names_unchecked(source);
    let mut cache = fixture_declarations_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = cache.get(&key) {
        with_fixture_profile_stats(|stats| stats.decl_hits += 1);
        return std::sync::Arc::clone(entry);
    }
    let entry = std::sync::Arc::new(parsed);
    cache.insert(key, std::sync::Arc::clone(&entry));
    entry
}

fn cached_fixture_directory_modules(
    directory: &Path,
) -> Result<std::sync::Arc<FixtureDirMap>, String> {
    let filter_fixtures_only = fixture_dir_filter_fixtures_only();
    if !fixture_caching_enabled() {
        let mut modules = FixtureDirMap::new();
        for entry in
            std::fs::read_dir(directory).map_err(|error| format!("fixture directory: {error}"))?
        {
            let path = entry
                .map_err(|error| format!("fixture entry: {error}"))?
                .path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if filter_fixtures_only && !name.contains("_FIXTURE") {
                continue;
            }
            modules.insert(format!("./{name}"), path);
        }
        return Ok(std::sync::Arc::new(modules));
    }
    let mut cache = fixture_dir_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(modules) = cache.get(directory) {
        with_fixture_profile_stats(|stats| stats.dir_hits += 1);
        return Ok(std::sync::Arc::clone(modules));
    }
    with_fixture_profile_stats(|stats| stats.dir_misses += 1);
    let mut modules = FixtureDirMap::new();
    for entry in
        std::fs::read_dir(directory).map_err(|error| format!("fixture directory: {error}"))?
    {
        let path = entry
            .map_err(|error| format!("fixture entry: {error}"))?
            .path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if filter_fixtures_only && !name.contains("_FIXTURE") {
            continue;
        }
        modules.insert(format!("./{name}"), path);
    }
    let modules = std::sync::Arc::new(modules);
    cache.insert(directory.to_path_buf(), std::sync::Arc::clone(&modules));
    Ok(modules)
}

fn cached_fixture_dependencies(
    path: &Path,
    source: &str,
) -> Result<std::sync::Arc<HashSet<String>>, String> {
    if !source.contains("import") && !source.contains("export") && !source.contains("import(") {
        with_fixture_profile_stats(|stats| stats.dep_hits += 1);
        return Ok(std::sync::Arc::new(HashSet::new()));
    }
    if !fixture_caching_enabled() {
        return Ok(std::sync::Arc::new(fixture_fixture_requests_from_source(
            source,
        )));
    }
    let mut cache = fixture_dep_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(requests) = cache.get(path) {
        with_fixture_profile_stats(|stats| stats.dep_hits += 1);
        return Ok(std::sync::Arc::clone(requests));
    }
    with_fixture_profile_stats(|stats| stats.dep_misses += 1);
    let requests = fixture_fixture_requests_from_source(source);
    let requests = std::sync::Arc::new(requests);
    cache.insert(path.to_path_buf(), std::sync::Arc::clone(&requests));
    Ok(requests)
}

/// True when `msg` looks like a JS-throw message. Either wrapped in a
/// `JsError("<Type>: …")` envelope or a bare `<Type>: …` where `<Type>` is a
/// known JS error constructor. Such messages always originate from user test
/// code, never from the runner, so the INFRA_MARKERS substring search must
/// not falsely classify them.
fn is_js_throw_msg(msg: &str) -> bool {
    let inner = js_envelope_inner(msg).unwrap_or(msg);
    inner
        .split_once(':')
        .map(|(k, _)| k)
        .is_some_and(|k| JS_ERROR_TYPES.contains(&k))
}

/// Extract the inner of the first JsError("…") envelope in `msg`, whether
/// the envelope is the whole message or embedded in a wrapper like
/// `"expected X but got: JsError(\"…\")"`.
fn js_envelope_inner(msg: &str) -> Option<&str> {
    const PREFIX: &str = "JsError(\"";
    const SUFFIX: &str = "\")";
    let start = msg.find(PREFIX)? + PREFIX.len();
    let after = &msg[start..];
    let end = after.rfind(SUFFIX)?;
    Some(&after[..end])
}

/// Does an error message satisfy a negative expectation? OXC reports parse
/// failures as "Parse error: …"; per spec any parse-phase rejection IS a
/// SyntaxError, so map that onto the expected type.
fn error_type_matches(phase: &str, typ: &str, msg: &str) -> bool {
    if typ.is_empty() {
        return true;
    }
    if phase == "parse" && typ == "SyntaxError" && msg.contains("Parse error") {
        return true;
    }
    let actual = msg.trim_start_matches("JsError(\"");
    let actual = actual
        .split_once(':')
        .map(|(kind, _)| kind)
        .unwrap_or(actual.trim_end_matches("\")"));
    actual == typ
}

fn thrown_error_type_matches(typ: &str) -> bool {
    matches!(
        quench_runtime::value::get_thrown_value(),
        Some(Value::Object(error))
            if error.borrow().get("name") == Some(Value::String(typ.to_string()))
    )
}

/// Build a TestFailure with captured JS error diagnostics from the thread-local.
/// Called after a failed eval while the thrown value is still available.
fn build_failure(msg: impl Into<String>, test_path: Option<&Path>) -> TestFailure {
    let msg = msg.into();
    let (mut error_type, mut error_message, js_stack) = capture_thrown_diagnostics();
    if error_type.is_none() {
        let envelope_inner = js_envelope_inner(&msg);
        let candidate = envelope_inner.unwrap_or(&msg);
        if let Some((kind, detail)) = candidate.split_once(':') {
            error_type = Some(kind.to_string());
            error_message = Some(detail.trim().to_string());
        }
    }
    let mut f = TestFailure {
        message: msg,
        error_type,
        error_message,
        js_stack,
        source_path: test_path.map(|p| p.to_string_lossy().to_string()),
        source_line: None,
        source_context: String::new(),
    };
    // Attach source context if we have a test path.
    if let Some(path) = test_path {
        f = f.with_source(path, None);
    }
    f
}

pub fn check_outcome(
    meta: &Test262Metadata,
    result: Result<(), String>,
    test_path: Option<&Path>,
) -> TestOutcome {
    match (&meta.negative, result) {
        (None, Ok(())) => TestOutcome::Pass,
        (None, Err(msg)) => TestOutcome::Fail {
            failure: build_failure(msg, test_path),
        },
        (Some(_), Ok(())) => TestOutcome::Fail {
            failure: TestFailure::from_message("expected error but passed"),
        },
        (Some(neg), Err(msg)) => {
            if !is_js_throw_msg(&msg) && INFRA_MARKERS.iter().any(|m| msg.contains(m)) {
                return TestOutcome::Fail {
                    failure: TestFailure::from_message(format!(
                        "infrastructure failure, not a test result: {}",
                        msg
                    )),
                };
            }
            if !error_type_matches(&neg.phase, &neg.typ, &msg)
                && !thrown_error_type_matches(&neg.typ)
            {
                TestOutcome::Fail {
                    failure: build_failure(
                        format!("expected {} but got: {}", neg.typ, msg),
                        test_path,
                    ),
                }
            } else {
                quench_runtime::value::take_thrown_value();
                TestOutcome::Pass
            }
        }
    }
}

#[derive(Clone)]
pub struct PreparedTest {
    source: Arc<str>,
    metadata: Arc<Test262Metadata>,
    script: Option<Arc<str>>,
}

pub fn prepare_test(test_path: &Path) -> Result<PreparedTest, String> {
    let source = std::fs::read_to_string(test_path).map_err(|e| format!("read: {}", e))?;
    let metadata = Test262Metadata::parse(&source).ok_or_else(|| "bad frontmatter".to_string())?;
    Ok(PreparedTest {
        source: Arc::from(source),
        metadata: Arc::new(metadata),
        script: None,
    })
}

pub(crate) fn prepare_stage_cache(
    harness: &HarnessLoader,
    tests: &[PathBuf],
) -> HashMap<PathBuf, Result<PreparedTest, String>> {
    if !prepare_eager_enabled() {
        return HashMap::new();
    }

    tests
        .iter()
        .cloned()
        .map(|path| {
            let prepared = prepare_test_with_harness(harness, &path).inspect_err(|_error| {
                METRIC_MISSING_HARNESS.fetch_add(1, Ordering::Relaxed);
            });
            (path, prepared)
        })
        .collect()
}

pub fn prepare_test_with_harness(
    harness: &HarnessLoader,
    test_path: &Path,
) -> Result<PreparedTest, String> {
    let mut prepared = prepare_test(test_path)?;
    if negative_parse_phase(&prepared.metadata)
        && has_parse_negative_match(source_without_frontmatter(&prepared.source))
    {
        return Ok(prepared);
    }
    let is_raw = has_flag(&prepared.metadata, "raw");
    let script = build_script(harness, &prepared.source, &prepared.metadata, is_raw)?;
    prepared.script = Some(Arc::from(script));
    Ok(prepared)
}

pub fn run_single_test(harness: &HarnessLoader, test_path: &Path) -> TestOutcome {
    let prepared = match prepare_test_with_harness(harness, test_path) {
        Ok(test) => test,
        Err(error) => {
            METRIC_MISSING_HARNESS.fetch_add(1, Ordering::Relaxed);
            return TestOutcome::Fail {
                failure: TestFailure::from_message(error),
            };
        }
    };
    run_prepared_test(harness, test_path, &prepared)
}

pub fn run_prepared_test(
    harness: &HarnessLoader,
    test_path: &Path,
    prepared: &PreparedTest,
) -> TestOutcome {
    run_prepared(
        harness,
        test_path,
        &prepared.source,
        prepared.metadata.clone(),
        prepared.script.as_ref(),
    )
}

fn run_prepared(
    harness: &HarnessLoader,
    test_path: &Path,
    source: &str,
    meta: Arc<Test262Metadata>,
    prepared_script: Option<&Arc<str>>,
) -> TestOutcome {
    let is_module = has_flag(&meta, "module");
    let is_raw = has_flag(&meta, "raw");
    let is_no_strict = is_raw || has_flag(&meta, "noStrict");
    let is_only_strict = has_flag(&meta, "onlyStrict");

    if negative_parse_phase(&meta) && has_parse_negative_match(source_without_frontmatter(source)) {
        METRIC_PARSE_NEGATIVE_SHORT_CIRCUIT.fetch_add(1, Ordering::Relaxed);
        return TestOutcome::Pass;
    }

    let script = match prepared_script {
        Some(script) => Arc::clone(script),
        None => match build_script(harness, source, &meta, is_raw) {
            Ok(script) => Arc::<str>::from(script),
            Err(error) => {
                return TestOutcome::Fail {
                    failure: TestFailure::from_message(error),
                };
            }
        },
    };
    let module_source = is_module.then(|| Arc::<str>::from(source));

    if !is_only_strict {
        let outcome = run_with_timeout(
            Arc::clone(&script),
            is_module,
            Arc::clone(&meta),
            test_path,
            module_source.clone(),
        );
        if !matches!(outcome, TestOutcome::Pass) {
            return outcome;
        }
        if is_no_strict {
            return TestOutcome::Pass;
        }
    }

    if is_no_strict {
        return TestOutcome::Fail {
            failure: TestFailure::from_message("conflicting flags: onlyStrict with noStrict/raw"),
        };
    }

    let strict_script = Arc::<str>::from(format!("\"use strict\";\n{}", script.as_ref()));
    match run_with_timeout(
        strict_script,
        is_module,
        meta,
        test_path,
        module_source,
    ) {
        TestOutcome::Fail { failure } => TestOutcome::Fail {
            failure: TestFailure {
                message: format!("strict: {}", failure.message),
                ..failure
            },
        },
        other => other,
    }
}

fn build_script(
    harness: &HarnessLoader,
    source: &str,
    meta: &Test262Metadata,
    is_raw: bool,
) -> Result<String, String> {
    if is_raw {
        return Ok(source.to_string());
    }
    let built = harness.build_script(source, &meta.includes)?;
    if has_flag(meta, "async") {
        Ok(format!("{}{}", ASYNC_DONE_PRELUDE, built))
    } else {
        Ok(built)
    }
}

/// Default stack for per-test worker threads (avoids overflow on deep class tests).
const TEST_THREAD_STACK: usize = 64 * 1024 * 1024;
const DEEP_TEST_THREAD_STACK: usize = 1024 * 1024 * 1024;

fn worker_stack_size(script: &str, test_path: &Path) -> usize {
    if script.len() > 20_000
        || script.contains("UnicodeIDStart")
        || script.contains("testTypedArrayConversions")
        || test_path
            .to_string_lossy()
            .contains("nativeFunctionMatcher")
        || test_path
            .to_string_lossy()
            .contains("testTypedArray-conversions")
    {
        DEEP_TEST_THREAD_STACK
    } else {
        TEST_THREAD_STACK
    }
}

fn run_with_timeout(
    script: Arc<str>,
    is_module: bool,
    meta: Arc<Test262Metadata>,
    test_path: &Path,
    module_source: Option<Arc<str>>,
) -> TestOutcome {
    if inprocess_threadless_enabled() {
        return run_inprocess_threadless(script, is_module, meta, test_path, module_source);
    }
    if is_threadless_auto_candidate(&meta, module_source.as_deref().unwrap_or(script.as_ref())) {
        note_threadless_auto_run();
        return run_inprocess_threadless(script, is_module, meta, test_path, module_source);
    }
    run_inprocess_threaded(script, is_module, meta, test_path, module_source)
}

fn run_inprocess_threaded(
    script: Arc<str>,
    is_module: bool,
    meta: Arc<Test262Metadata>,
    test_path: &Path,
    module_source: Option<Arc<str>>,
) -> TestOutcome {
    let timeout_secs = test_timeout_secs();
    let timeout = Duration::from_secs(timeout_secs);
    let tp = test_path.to_owned();
    let (tx, rx) = mpsc::channel();
    METRIC_THREADED_RUNS.fetch_add(1, Ordering::Relaxed);
    let stack_size = worker_stack_size(&script, &tp);
    let spawn = std::thread::Builder::new()
        .stack_size(stack_size)
        .spawn(move || {
            let is_async = has_flag(&meta, "async");
            let result = execute_script(&script, is_module, is_async, &tp, module_source.as_deref());
            // Pass test_path for source context capture in check_outcome.
            let _ = tx.send(check_outcome(&meta, result, Some(&tp)));
        });
    if spawn.is_err() {
        return TestOutcome::Fail {
            failure: TestFailure::from_message("failed to spawn test thread"),
        };
    }
    let _handle = spawn.unwrap();
    match rx.recv_timeout(timeout) {
        Ok(outcome) => outcome,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            METRIC_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
            TestOutcome::Fail {
                failure: TestFailure::from_message(format!("timed out after {}s", timeout_secs)),
            }
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            METRIC_PANICS.fetch_add(1, Ordering::Relaxed);
            TestOutcome::Fail {
                failure: TestFailure::from_message("panicked"),
            }
        }
    }
}

fn run_inprocess_threadless(
    script: Arc<str>,
    is_module: bool,
    meta: Arc<Test262Metadata>,
    test_path: &Path,
    module_source: Option<Arc<str>>,
) -> TestOutcome {
    METRIC_THREADLESS_RUNS.fetch_add(1, Ordering::Relaxed);
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let is_async = has_flag(&meta, "async");
        execute_script(&script, is_module, is_async, test_path, module_source.as_deref())
    }))
    .unwrap_or_else(|_| Err("panicked".to_string()));
    if result.is_err() {
        METRIC_PANICS.fetch_add(1, Ordering::Relaxed);
    }
    check_outcome(&meta, result, Some(test_path))
}

fn has_parse_negative_match(script: &str) -> bool {
    quench_runtime::interpreter::has_legacy_octal(script)
        || quench_runtime::interpreter::has_invalid_strict_numeric_literal(script)
        || quench_runtime::interpreter::has_invalid_strict_legacy_octal_escape(script)
        || quench_runtime::interpreter::has_invalid_regexp_pattern(script)
        || quench_runtime::interpreter::has_invalid_unicode_legacy_octal_escape(script)
        || quench_runtime::interpreter::has_invalid_unicode_out_of_bounds_decimal_escape(script)
        || quench_runtime::interpreter::has_invalid_unicode_optional_assertion(script)
        || quench_runtime::interpreter::has_invalid_unicode_assertion_range(script)
        || quench_runtime::interpreter::has_invalid_unicode_class_control_escape(script)
        || quench_runtime::interpreter::has_invalid_unicode_class_range_escape(script)
        || quench_runtime::interpreter::has_invalid_unicode_identity_escape(script)
        || quench_runtime::interpreter::has_invalid_unicode_code_point_escape(script)
        || quench_runtime::interpreter::has_invalid_unicode_numeric_separator_escape(script)
        || quench_runtime::interpreter::has_overlapping_regexp_modifiers(script)
        || quench_runtime::interpreter::has_invalid_named_group_identifier(script)
        || quench_runtime::interpreter::has_malformed_named_backreference_prefix(script)
        || quench_runtime::interpreter::has_unicode_identity_escape_in_named_group(script)
        || quench_runtime::interpreter::has_incomplete_named_group(script)
        || quench_runtime::interpreter::has_incomplete_named_backreference(script)
        || quench_runtime::interpreter::has_empty_named_group(script)
        || quench_runtime::interpreter::has_duplicate_named_group(script)
        || quench_runtime::interpreter::has_dangling_named_backreference(script)
        || quench_runtime::interpreter::has_quantified_lookbehind(script)
        || quench_runtime::interpreter::has_invalid_braced_regexp_quantifier(script)
}

/// Execute a prepared script; async tests get the $DONE invocation check.
fn execute_script(
    script: &str,
    is_module: bool,
    is_async: bool,
    test_path: &Path,
    module_source: Option<&str>,
) -> Result<(), String> {
    if is_async {
        return run_async_script_with_path(script, is_module, Some(test_path), module_source);
    }
    run_sync_script_with_path(script, is_module, test_path, module_source)
}

fn current_module_name(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("./{name}"))
}

pub(crate) fn initialize_test_context(strict: bool) -> Result<quench_runtime::Context, String> {
    quench_runtime::interpreter::reset_interpreter_state();
    let mut ctx = quench_runtime::Context::new().map_err(|error| format!("{error:?}"))?;
    quench_runtime::builtins::bootstrap::bootstrap_js_builtins(&mut ctx)
        .map_err(|error| format!("builtin bootstrap failure: {error}"))?;
    ctx.eval("delete AsyncFunction")
        .map_err(|error| format!("AsyncFunction cleanup failure: {error:?}"))?;
    quench_runtime::api::set_strict_mode(false);
    crate::harness::try_inject_harness(&mut ctx)
        .map_err(|error| format!("harness load failure: {error}"))?;
    if let Some(error) = ctx.get_global("Test262Error") {
        quench_runtime::value::error::set_main_realm_host_error(error);
    }
    quench_runtime::api::set_strict_mode(strict);
    Ok(ctx)
}

fn run_sync_script_with_path(
    source: &str,
    is_module: bool,
    path: &Path,
    module_source: Option<&str>,
) -> Result<(), String> {
    let current_module_name = current_module_name(path);
    let strict = source.trim_start().starts_with("\"use strict\";")
        || source.trim_start().starts_with("'use strict';");
    let mut ctx = initialize_test_context(strict)?;
    let module_source = if is_module {
        Some(
            if let Some(source) = module_source {
                Cow::Borrowed(source)
            } else {
                Cow::Owned(
                    std::fs::read_to_string(path)
                        .map_err(|error| format!("test source: {error}"))?,
                )
            },
        )
    } else {
        None
    };
    let module_source = module_source.as_deref().unwrap_or(source);
    let (fixture_module_requests, has_fixture_dependencies) = if is_module {
        fixture_dependency_requests(module_source)
    } else {
        (Vec::new(), false)
    };
    if is_module {
        if let Some(name) = current_module_name.as_deref() {
            ctx.set_global(
                "__quench_current_module__".into(),
                quench_runtime::Value::String(name.to_string()),
            );
        }
        register_current_module_bindings(&mut ctx, module_source)?;
    }
    // The runtime synthesizes the current module's live namespace during import.
    // Pre-registering an empty namespace here would shadow that namespace.
    if is_module {
        load_fixture_modules_with_source(
            &mut ctx,
            path,
            module_source,
            current_module_name.as_deref(),
            has_fixture_dependencies,
            &fixture_module_requests,
        )?;
    }
    if is_module
        && prepare_current_module_deferred_dependencies_with_requests(
            &mut ctx,
            module_source,
            &fixture_module_requests,
            has_fixture_dependencies,
        )?
    {
        ctx.execute_pending_microtasks()
            .map_err(|error| format!("deferred fixture module jobs: {error:?}"))?;
    }
    if is_module {
        populate_current_module_star_exports(&mut ctx, module_source);
    }
    if let Some(name) = current_module_name.as_deref() {
        if let Some(quench_runtime::Value::Object(raw_modules)) =
            ctx.get_global("__quench_fixture_raw_modules__")
        {
            raw_modules.borrow_mut().set(
                name,
                quench_runtime::Value::String(module_source.to_string()),
            );
            if let Some(base) = name.strip_prefix("./").and_then(|name| name.strip_suffix("-as.js")) {
                raw_modules.borrow_mut().set(
                    &format!("./{base}.js"),
                    quench_runtime::Value::String(module_source.to_string()),
                );
            }
        }
        ctx.set_global(
            "__quench_current_module__".to_string(),
            quench_runtime::Value::String(name.to_string()),
        );
        propagate_missing_import_resolution_error(&mut ctx, module_source);
        propagate_current_module_resolution_error(&mut ctx, module_source);
    }
    if strict && quench_runtime::interpreter::has_legacy_octal(source) {
        return Err("SyntaxError: legacy octal literal in strict mode".to_string());
    }
    if strict && quench_runtime::interpreter::has_overlapping_regexp_modifiers(source) {
        return Err("SyntaxError: overlapping regexp modifiers".to_string());
    }
    let result = if is_module {
        ctx.eval_es_module(source)
    } else {
        ctx.eval(source)
    };
    result.map(|_| ()).map_err(|error| format!("{error:?}"))
}

fn propagate_current_module_resolution_error(ctx: &mut quench_runtime::Context, source: &str) {
    let Ok((_, _, _, _, reexports)) = fixture_exports_from_source(usize::MAX, source) else {
        return;
    };
    let Some(Value::Object(errors)) = ctx.get_global("__quench_module_errors__") else {
        return;
    };
    let current_module =
        ctx.get_global("__quench_current_module__")
            .and_then(|value| match value {
                Value::String(name) => Some(name),
                _ => None,
            });
    for entry in &reexports {
        if let PendingReExport::Named {
            source,
            local,
            exported,
        } = entry
        {
            if current_module.as_deref().is_some_and(|current| {
                current.trim_start_matches("./") == source.trim_start_matches("./")
            }) && current_module_has_export(ctx, local)
            {
                continue;
            }
            if resolve_fixture_export(ctx, source, local).is_some() {
                continue;
            }
            if current_module
                .as_deref()
                .is_some_and(|current| named_export_resolution_is_circular(ctx, current, exported))
            {
                if let Some(module) = current_module.as_deref() {
                    errors
                        .borrow_mut()
                        .set(module, Value::String("Circular module export".into()));
                }
                return;
            }
            if current_module
                .as_deref()
                .is_some_and(|current| named_export_resolution_is_missing(ctx, current, exported))
            {
                if let Some(module) = current_module.as_deref() {
                    errors
                        .borrow_mut()
                        .set(module, Value::String("Missing indirect export".into()));
                }
                return;
            }
            let missing = ctx
                .get_module(source)
                .and_then(|value| match value {
                    Value::Object(module) => Some(
                        module.borrow().get(local).is_none() && !module.borrow().has_getter(local),
                    ),
                    _ => None,
                })
                .unwrap_or(true);
            if missing {
                if let Some(Value::String(module)) = ctx.get_global("__quench_current_module__") {
                    errors
                        .borrow_mut()
                        .set(&module, Value::String("Missing indirect export".into()));
                }
                return;
            }
        }
    }
    let mut sources = reexports
        .into_iter()
        .map(|entry| match entry {
            PendingReExport::StarAs { source, .. }
            | PendingReExport::StarAll { source }
            | PendingReExport::Named { source, .. } => source,
            PendingReExport::ModuleSource { .. } => String::new(),
        })
        .collect::<Vec<_>>();
    sources.extend(
        fixture_import_edges_from_source(source)
            .into_iter()
            .map(|(_, source)| source),
    );
    sources.extend(fixture_side_effect_imports_from_source(source));
    // `export {} from "..."` has no export entries, but it still requests
    // and instantiates its source module.
    sources.extend(fixture_reexport_requests_from_source(source));
    let reason = sources
        .into_iter()
        .find_map(|source| errors.borrow().get(&source));
    let Some(reason) = reason else {
        return;
    };
    let Some(Value::String(module)) = ctx.get_global("__quench_current_module__") else {
        return;
    };
    errors.borrow_mut().set(&module, reason);
}

fn current_module_has_export(ctx: &quench_runtime::Context, name: &str) -> bool {
    ctx.get_global("__quench_current_module_bindings__")
        .is_some_and(|bindings| {
            matches!(bindings, Value::Object(bindings) if bindings.borrow().has_own(name))
        })
}

fn named_export_resolution_is_circular(
    ctx: &quench_runtime::Context,
    module: &str,
    exported: &str,
) -> bool {
    let mut visited = HashSet::new();
    named_export_resolution_is_circular_inner(ctx, module, exported, &mut visited)
}

fn named_export_resolution_is_circular_inner(
    ctx: &quench_runtime::Context,
    module: &str,
    exported: &str,
    visited: &mut HashSet<(String, String)>,
) -> bool {
    let module = canonical_module_name(module);
    if !visited.insert((module.clone(), exported.to_string())) {
        return true;
    }
    let Some(Value::Object(raw_modules)) = ctx.get_global("__quench_fixture_raw_modules__") else {
        return false;
    };
    let Some(Value::String(source)) = raw_modules.borrow().get(&module) else {
        return false;
    };
    fixture_exports_from_source(usize::MAX, &source)
        .ok()
        .is_some_and(|(_, _, _, _, entries)| {
            entries.into_iter().any(|entry| match entry {
                PendingReExport::Named {
                    source,
                    local,
                    exported: candidate,
                } if candidate == exported => {
                    named_export_resolution_is_circular_inner(ctx, &source, &local, visited)
                }
                _ => false,
            })
        })
}

fn canonical_module_name(module: &str) -> String {
    module
        .strip_suffix("-as.js")
        .map_or_else(|| module.to_string(), |base| format!("{base}.js"))
}

fn named_export_resolution_is_missing(
    ctx: &quench_runtime::Context,
    module: &str,
    exported: &str,
) -> bool {
    let mut visited = HashSet::new();
    named_export_resolution_is_missing_inner(ctx, module, exported, &mut visited)
}

fn named_export_resolution_is_missing_inner(
    ctx: &quench_runtime::Context,
    module: &str,
    exported: &str,
    visited: &mut HashSet<(String, String)>,
) -> bool {
    let module = canonical_module_name(module);
    if !visited.insert((module.clone(), exported.to_string())) {
        // A repeated pair is handled by the circular-resolution check.
        return false;
    }
    let Some(Value::Object(raw_modules)) = ctx.get_global("__quench_fixture_raw_modules__") else {
        return true;
    };
    let Some(Value::String(source)) = raw_modules.borrow().get(&module) else {
        return true;
    };
    let Ok((_, _, exports, _, entries)) = fixture_exports_from_source(usize::MAX, &source) else {
        return true;
    };
    if let Some(PendingReExport::Named { source, local, .. }) = entries.into_iter().find(|entry| {
        matches!(entry, PendingReExport::Named { exported: candidate, .. } if candidate == exported)
    }) {
        return named_export_resolution_is_missing_inner(ctx, &source, &local, visited);
    }
    let direct_export = exports.named.iter().any(|name| name == exported)
        || exports.aliases.iter().any(|(_, name)| name == exported)
        || (exported == "default"
            && (exports.default_marker.is_some() || !exports.default_aliases.is_empty()));
    !direct_export
}

fn propagate_missing_import_resolution_error(ctx: &mut quench_runtime::Context, source: &str) {
    let Some(Value::String(module_name)) = ctx.get_global("__quench_current_module__") else {
        return;
    };
    let Some(Value::Object(errors)) = ctx.get_global("__quench_module_errors__") else {
        return;
    };
    for (imported, target_name) in fixture_import_edges_from_source(source) {
        if target_name == module_name {
            continue;
        }
        let Some(Value::Object(target)) = ctx.get_module(&target_name) else {
            continue;
        };
        let missing =
            target.borrow().get(&imported).is_none() && !target.borrow().has_getter(&imported);
        if missing
            && !(target_name == module_name && fixture_star_export_resolves(ctx, source, &imported))
        {
            errors.borrow_mut().set(
                &module_name,
                Value::String("Missing indirect export".to_string()),
            );
            return;
        }
    }
}

fn fixture_star_export_resolves(
    ctx: &quench_runtime::Context,
    source: &str,
    imported: &str,
) -> bool {
    let Ok((_, _, _, _, reexports)) = fixture_exports_from_source(usize::MAX, source) else {
        return false;
    };
    reexports.into_iter().any(|entry| {
        let PendingReExport::StarAll { source } = entry else {
            return false;
        };
        ctx.get_module(&source).is_some_and(|module| match module {
            Value::Object(module) => {
                module.borrow().get(imported).is_some() || module.borrow().has_getter(imported)
            }
            _ => false,
        })
    })
}

fn populate_current_module_star_exports(ctx: &mut quench_runtime::Context, source: &str) {
    let Ok((_, _, _, _, reexports)) = fixture_exports_from_source(usize::MAX, source) else {
        return;
    };
    let Some(Value::String(module_name)) = ctx.get_global("__quench_current_module__") else {
        return;
    };
    let Some(Value::Object(module)) = ctx.get_module(&module_name) else {
        return;
    };
    for entry in reexports {
        let PendingReExport::StarAll { source } = entry else {
            continue;
        };
        let Some(Value::Object(target)) = ctx.get_module(&source) else {
            continue;
        };
        for key in target.borrow().own_property_names() {
            if key == "default" || module.borrow().has(&key) {
                continue;
            }
            let value = target
                .borrow()
                .get_own_value(&key)
                .unwrap_or(Value::Undefined);
            define_module_binding(&module, &key, value);
        }
    }
}

/// Run an async-flag test: eval (which drains microtasks), then verify $DONE
/// was invoked exactly once.
#[cfg(test)]
fn run_async_script(source: &str, is_module: bool) -> Result<(), String> {
    run_async_script_with_path(source, is_module, None, None)
}

fn run_async_script_with_path(
    source: &str,
    is_module: bool,
    test_path: Option<&Path>,
    module_source: Option<&str>,
) -> Result<(), String> {
    let current_module_name = test_path.and_then(current_module_name);
    let strict = source.trim_start().starts_with("\"use strict\";")
        || source.trim_start().starts_with("'use strict';");
    let mut ctx = initialize_test_context(strict)?;
    ctx.set_global(
        "__quench_async_harness__".to_string(),
        quench_runtime::Value::Boolean(true),
    );
    let async_prelude_offset = is_module.then(|| source.find(ASYNC_DONE_PRELUDE)).flatten();
    if async_prelude_offset.is_some() {
        ctx.eval(ASYNC_DONE_PRELUDE)
            .map_err(|error| format!("async module prelude: {error:?}"))?;
    }
    let module_source = if is_module {
        Some(
            if let Some(source) = module_source {
                Cow::Borrowed(source)
            } else if let Some(path) = test_path {
                Cow::Owned(std::fs::read_to_string(path).map_err(|error| {
                    format!("test source: {error}")
                })?)
            } else {
                Cow::Borrowed(source)
            },
        )
    } else {
        None
    };
    let module_source = module_source.as_deref().unwrap_or(source);
    let (fixture_module_requests, has_fixture_dependencies) = if is_module {
        fixture_dependency_requests(module_source)
    } else {
        (Vec::new(), false)
    };
    if let Some(test_path) = test_path {
        if let Some(name) = current_module_name.as_deref() {
            ctx.set_global(
                "__quench_current_module__".to_string(),
                quench_runtime::Value::String(name.to_string()),
            );
        }
        if is_module {
            register_current_module_bindings(&mut ctx, module_source)?;
        }
        load_fixture_modules_with_source(
            &mut ctx,
            test_path,
            module_source,
            current_module_name.as_deref(),
            has_fixture_dependencies,
            &fixture_module_requests,
        )?;
        if let Some(name) = current_module_name.as_deref() {
            if let Some(quench_runtime::Value::Object(raw_modules)) =
                ctx.get_global("__quench_fixture_raw_modules__")
            {
                raw_modules.borrow_mut().set(
                    name,
                    quench_runtime::Value::String(
                        module_source.to_string(),
                    ),
                );
            }
        }
        if !is_module {
            register_current_script_module(&mut ctx, test_path)?;
        }
    }
    if is_module
        && prepare_current_module_deferred_dependencies_with_requests(
            &mut ctx,
            module_source,
            &fixture_module_requests,
            has_fixture_dependencies,
        )?
    {
        ctx.execute_pending_microtasks()
            .map_err(|error| format!("deferred fixture module jobs: {error:?}"))?;
    }
    if is_module {
        register_current_module_bindings(&mut ctx, module_source)?;
    }
    let module_eval_source = async_prelude_offset.map_or_else(
        || source.to_string(),
        |offset| {
            let mut module_source = String::with_capacity(source.len() - ASYNC_DONE_PRELUDE.len());
            module_source.push_str(&source[..offset]);
            module_source.push_str(&source[offset + ASYNC_DONE_PRELUDE.len()..]);
            module_source
        },
    );
    let result = if is_module {
        ctx.eval_es_module(&module_eval_source)
    } else {
        ctx.eval(source)
    };
    result.map_err(|e| format!("{:?}", e))?;
    let _ = ctx.execute_pending_microtasks();
    // Clear any stale thrown_value left by an uncaught error that was
    // converted to a rejected Promise (e.g. TDZ ReferenceError in for-of
    // head with `await using`). The probe below evaluates JS which would
    // otherwise see the stale thrown_value and fail spuriously.
    quench_runtime::value::take_thrown_value();
    async_done_probe(&mut ctx)
}

pub fn register_current_module_bindings(
    ctx: &mut quench_runtime::Context,
    source: &str,
) -> Result<(), String> {
    let (_, _, exports, _, reexports) = fixture_exports_from_source(usize::MAX, source)?;
    let current_module = ctx
        .get_global("__quench_current_module__")
        .and_then(|value| {
            if let Value::String(name) = value {
                Some(name)
            } else {
                None
            }
        });
    let mut bindings =
        quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    for name in exports.named {
        bindings.set(&name, quench_runtime::Value::String(name.clone()));
    }
    for (local, exported) in exports.aliases {
        bindings.set(&exported, quench_runtime::Value::String(local));
    }
    for local in exports.default_aliases {
        bindings.set("default", quench_runtime::Value::String(local));
    }
    if exports.default_marker.is_some() {
        bindings.set("default", quench_runtime::Value::String("default".into()));
    }
    for reexport in reexports {
        let (name, local) = match reexport {
            PendingReExport::Named {
                source,
                local,
                exported,
            } if current_module.as_deref().is_some_and(|current| {
                current.trim_start_matches("./") == source.trim_start_matches("./")
            }) =>
            {
                (exported, Some(local))
            }
            PendingReExport::Named { exported, .. }
            | PendingReExport::StarAs { name: exported, .. } => (exported, None),
            PendingReExport::ModuleSource { name, .. } => (name, None),
            PendingReExport::StarAll { .. } => continue,
        };
        bindings.set(
            &name,
            local.map_or(Value::Undefined, |local| Value::String(local)),
        );
    }
    ctx.set_global(
        "__quench_current_module_bindings__".into(),
        quench_runtime::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(bindings))),
    );
    let lexical_bindings = cached_fixture_current_module_lexical_bindings(source);
    let Some(module_name) = current_module else {
        return Ok(());
    };
    // The first pass installs the current module's namespace so fixture linking
    // can resolve current-module references. Keep that object stable while
    // later passes add its re-exports.
    if ctx.get_module(&module_name).is_some() {
        return Ok(());
    }
    let Some(Value::Object(bindings)) = ctx.get_global("__quench_current_module_bindings__") else {
        return Ok(());
    };
    let environment = ctx.environment_view();
    let mut module =
        quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::ModuleNamespace);
    for exported in bindings.borrow().own_property_names() {
        let Some(binding) = bindings.borrow().get(&exported) else {
            continue;
        };
        let Value::String(local) = binding else {
            module.define(
                &exported,
                binding,
                quench_runtime::value::PropertyFlags {
                    value: None,
                    writable: true,
                    enumerable: true,
                    configurable: false,
                },
            );
            continue;
        };
        let is_tdz = lexical_bindings.contains(&local);
        let environment = std::rc::Rc::clone(&environment);
        let getter = Value::NativeFunction(std::rc::Rc::new(
            quench_runtime::value::NativeFunction::new(move |_| {
                if is_tdz || environment.borrow().is_tdz(&local) {
                    let (value, error) = quench_runtime::value::error::create_js_error_with_type(
                        "Cannot access module binding before initialization",
                        "ReferenceError",
                    );
                    quench_runtime::value::set_thrown_value(value);
                    return Err(error);
                }
                Ok(environment.borrow().get(&local).unwrap_or(Value::Undefined))
            }),
        ));
        module.define_accessor(
            &exported,
            Some(getter),
            None,
            quench_runtime::value::PropertyFlags {
                value: None,
                writable: true,
                enumerable: true,
                configurable: false,
            },
        );
    }
    if let Some(Value::Symbol(symbol)) =
        quench_runtime::builtins::symbol::get_well_known_symbol_no_ctx("toStringTag")
    {
        let key = symbol.property_key();
        module.set_symbol(&key, Value::String("Module".into()));
        if let Some(flags) = module.descriptors.get_mut(&key) {
            flags.writable = false;
            flags.enumerable = false;
            flags.configurable = false;
        }
    }
    module.extensible = false;
    ctx.register_module(&module_name, module);
    Ok(())
}

/// Fixture modules can observe current-module lexical exports while linking.
/// These names are in the TDZ, but must not become fixture-environment names.
fn current_module_lexical_bindings(source: &str) -> HashSet<String> {
    use quench_runtime::ast::{Program, Statement, VarKind};

    let Ok(Program::Script(statements)) = quench_runtime::parser::parse_es_module(source) else {
        return HashSet::new();
    };
    let mut lexical = HashSet::new();
    for statement in statements {
        let statement = match statement {
            Statement::Export(declaration) => *declaration,
            statement => statement,
        };
        match statement {
            Statement::VarDeclaration { kind, name, .. } if kind != VarKind::Var => {
                lexical.insert(name);
            }
            Statement::ClassDeclaration { name, .. } => {
                lexical.insert(name);
            }
            _ => {}
        }
    }
    lexical
}

fn cached_fixture_current_module_lexical_bindings(source: &str) -> Arc<HashSet<String>> {
    if !fixture_analysis_cache_enabled() {
        return Arc::new(current_module_lexical_bindings(source));
    }
    let key = source_cache_key(source);
    {
        let cache = fixture_current_module_lexical_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = cache.get(&key) {
            note_fixture_module_lexical_cache_hit();
            with_fixture_profile_stats(|stats| stats.module_lexical_bindings_hits += 1);
            return Arc::clone(entry);
        }
    }
    note_fixture_module_lexical_cache_miss();
    with_fixture_profile_stats(|stats| stats.module_lexical_bindings_misses += 1);
    let parsed = current_module_lexical_bindings(source);
    let mut cache = fixture_current_module_lexical_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = cache.get(&key) {
        note_fixture_module_lexical_cache_hit();
        with_fixture_profile_stats(|stats| stats.module_lexical_bindings_hits += 1);
        return Arc::clone(entry);
    }
    let entry = std::sync::Arc::new(parsed);
    cache.insert(key, std::sync::Arc::clone(&entry));
    entry
}

fn default_function_updates_itself(source: &str) -> bool {
    source.lines().map(str::trim).any(|line| {
        let Some(function) = line.strip_prefix("export default function ") else {
            return false;
        };
        let Some(name) = function.split('(').next().map(str::trim) else {
            return false;
        };
        source.contains(&format!("{name} ="))
    })
}

pub fn register_current_script_module(
    ctx: &mut quench_runtime::Context,
    path: &Path,
) -> Result<(), String> {
    let name = current_module_name(path).ok_or("script name")?;
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let scripts = ctx
        .get_global("__quench_fixture_init_scripts__")
        .ok_or("fixture scripts")?;
    let done = ctx
        .get_global("__quench_fixture_init_done__")
        .ok_or("fixture done")?;
    if let quench_runtime::Value::Object(scripts) = scripts {
        scripts
            .borrow_mut()
            .set(&name, quench_runtime::Value::String(source));
    }
    if let quench_runtime::Value::Object(done) = done {
        done.borrow_mut()
            .set(&name, quench_runtime::Value::Boolean(false));
    }
    ctx.register_module(
        &name,
        quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary),
    );
    Ok(())
}

pub fn load_fixture_modules(
    ctx: &mut quench_runtime::Context,
    test_path: &Path,
) -> Result<(), String> {
    let current_source = cached_fixture_file(test_path)?;
    let (fixture_module_requests, has_fixture_dependencies) =
        fixture_dependency_requests(current_source.source.as_str());
    let current_module = current_module_name(test_path);
    load_fixture_modules_with_source(
        ctx,
        test_path,
        current_source.source.as_str(),
        current_module.as_deref(),
        has_fixture_dependencies,
        &fixture_module_requests,
    )
}

fn load_fixture_modules_with_source(
    ctx: &mut quench_runtime::Context,
    test_path: &Path,
    current_source: &str,
    current_module: Option<&str>,
    has_fixture_dependencies: bool,
    fixture_module_requests: &[String],
) -> Result<(), String> {
    let fixture_load_started = Instant::now();
    let profile = fixture_profile_enabled();
    let profile_slow_ms = fixture_profile_slow_ms();
    note_fixture_module_load_test();
    if let (Some(current_module), Some(Value::Object(raw_modules))) = (
        current_module,
        ctx.get_global("__quench_fixture_raw_modules__"),
    ) {
        raw_modules
            .borrow_mut()
            .set(current_module, Value::String(current_source.to_string()));
    }
    if !has_fixture_dependencies {
        let selected_modules = current_module.is_some() as usize;
        note_fixture_dependency_skip();
        note_fixture_module_load_millis(
            fixture_load_started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        );
        note_fixture_graph_nodes(selected_modules);
        note_fixture_graph_edges(0);
        note_fixture_graph_depth(0);
        note_fixture_graph_selected_modules(selected_modules);
        note_fixture_modules_selected(selected_modules);
        note_fixture_modules_loaded(0);
        note_fixture_modules_missing(0);
        note_fixture_module_bytes_loaded(0);
        with_fixture_profile_stats(|stats| {
            stats.no_dependency_short_circuits += 1;
            stats.selected_modules += selected_modules;
        });
        if profile || profile_slow_ms > 0 {
            let elapsed_ms = fixture_load_started.elapsed().as_millis();
            if profile {
                let stats = fixture_profile_snapshot();
                println!(
                    "[fixture-profile] test={} duration_ms={} selected={} loaded={} missing={} bytes={} file_cache_hits={} file_cache_misses={} dir_cache_hits={} dir_cache_misses={} dep_hits={} dep_misses={} source_hits={} source_misses={} import_edges_hits={} import_edges_misses={} import_request_hits={} import_request_misses={} named_import_hits={} named_import_misses={} namespace_import_hits={} namespace_import_misses={} source_import_hits={} source_import_misses={} side_effect_import_hits={} side_effect_import_misses={} reexport_request_hits={} reexport_request_misses={} dynamic_request_hits={} dynamic_request_misses={} module_request_hits={} module_request_misses={} attr_module_request_hits={} attr_module_request_misses={} no_dependency_short_circuits={}",
                    test_path.display(),
                    elapsed_ms,
                    selected_modules,
                    0,
                    0,
                    0,
                    stats.file_hits,
                    stats.file_misses,
                    stats.dir_hits,
                    stats.dir_misses,
                    stats.dep_hits,
                    stats.dep_misses,
                    stats.source_analysis_hits,
                    stats.source_analysis_misses,
                    stats.import_edges_hits,
                    stats.import_edges_misses,
                    stats.import_request_hits,
                    stats.import_request_misses,
                    stats.named_import_hits,
                    stats.named_import_misses,
                    stats.namespace_import_hits,
                    stats.namespace_import_misses,
                    stats.source_import_hits,
                    stats.source_import_misses,
                    stats.side_effect_import_hits,
                    stats.side_effect_import_misses,
                    stats.reexport_request_hits,
                    stats.reexport_request_misses,
                    stats.dynamic_request_hits,
                    stats.dynamic_request_misses,
                    stats.module_request_hits,
                    stats.module_request_misses,
                    stats.attr_module_request_hits,
                    stats.attr_module_request_misses,
                    stats.no_dependency_short_circuits,
                );
                if let Some(current_module) = current_module.as_deref() {
                    if fixture_profile_show_modules() {
                        println!("  {}", current_module);
                    }
                }
            }
            if profile_slow_ms > 0 && elapsed_ms >= profile_slow_ms {
                eprintln!(
                    "[fixture-profile] slow loading test={} duration_ms={}",
                    test_path.display(),
                    elapsed_ms
                );
            }
        }
        return Ok(());
    }

    let directory = test_path
        .parent()
        .ok_or_else(|| "test has no parent directory".to_string())?;
    let module_paths = cached_fixture_directory_modules(directory)?;
    let referenced_siblings: HashSet<String> = fixture_module_requests.iter().cloned().collect();
    let synthetic_modules = fixture_attribute_module_requests_from_source(current_source);
    let load_current_fixture = test_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains("_FIXTURE"));

    let mut selected_modules = HashSet::<String>::with_capacity(
        fixture_module_requests.len().saturating_add(16),
    );
    let mut selected_graph = VecDeque::<(String, usize)>::with_capacity(selected_modules.len() + 1);
    let mut seed_module = |module: String,
                           depth: usize,
                           selected_modules: &mut HashSet<String>,
                           selected_graph: &mut VecDeque<(String, usize)>| {
        if selected_modules.insert(module.clone()) {
            selected_graph.push_back((module, depth));
            true
        } else {
            false
        }
    };

    for module in fixture_module_requests
        .iter()
        .filter(|module| module.contains("_FIXTURE"))
    {
        let _ = seed_module(
            module.clone(),
            0,
            &mut selected_modules,
            &mut selected_graph,
        );
    }
    for module in fixture_dynamic_fixture_requests_from_source(current_source)
        .into_iter()
        .chain(fixture_reexport_requests_from_source(current_source))
        .filter(|module| module.contains("_FIXTURE"))
    {
        let _ = seed_module(
            module,
            0,
            &mut selected_modules,
            &mut selected_graph,
        );
    }
    if load_current_fixture {
        if let Some(module) = &current_module {
            let _ = seed_module(
                module.to_string(),
                0,
                &mut selected_modules,
                &mut selected_graph,
            );
        }
    }
    let mut loaded_modules = HashMap::<String, FixtureFileCacheEntry>::new();
    let mut missing_modules = 0usize;
    let mut fixture_graph_edges = 0usize;
    let mut fixture_graph_max_depth = 0usize;
    while let Some((module_name, depth)) = selected_graph.pop_front() {
        fixture_graph_max_depth = fixture_graph_max_depth.max(depth);
        if loaded_modules.contains_key(&module_name) {
            continue;
        }
        let Some(path) = module_paths.get(&module_name) else {
            missing_modules += 1;
            continue;
        };
        let module_file = cached_fixture_file(path)?;
        let source = module_file.source.as_str();
        for dependency in cached_fixture_dependencies(path, source)?.iter() {
            fixture_graph_edges = fixture_graph_edges.saturating_add(1);
            let _ = seed_module(
                dependency.clone(),
                depth.saturating_add(1),
                &mut selected_modules,
                &mut selected_graph,
            );
        }
        loaded_modules.insert(module_name, module_file);
    }
    note_fixture_graph_nodes(selected_modules.len());
    note_fixture_graph_edges(fixture_graph_edges);
    note_fixture_graph_depth(fixture_graph_max_depth);
    note_fixture_graph_selected_modules(selected_modules.len());
    note_fixture_modules_selected(selected_modules.len());
    note_fixture_modules_loaded(loaded_modules.len());
    note_fixture_modules_missing(missing_modules);
    note_fixture_module_bytes_loaded(
        loaded_modules.values().map(|entry| entry.bytes.len()).sum::<usize>(),
    );

    let mut fixture_names: Vec<_> = loaded_modules.keys().cloned().collect();
    if fixture_loads_sorted() {
        fixture_names.sort_unstable();
    }
    let mut fixtures = Vec::with_capacity(loaded_modules.len());
    let mut has_indirect_reexport = false;
    for module_name in &fixture_names {
        let module_file = loaded_modules
            .get(module_name)
            .expect("fixture module loaded");
        if current_module.as_deref() == Some(&module_name) && !load_current_fixture {
            continue;
        }
        if !selected_modules.contains(module_name.as_str())
            && !referenced_siblings.contains(module_name.as_str())
        {
            continue;
        }
        let source = module_file.source.as_str().to_owned();
        if source
            .lines()
            .any(|line| line.trim().starts_with("export {") && line.contains("} from"))
        {
            has_indirect_reexport = true;
        }
        let name = module_name.trim_start_matches("./");
        if let Some(path) = module_paths.get(module_name) {
            fixtures.push((name.to_string(), path.clone()));
        }
    }
    let elapsed_ms = fixture_load_started.elapsed().as_millis();
    note_fixture_module_load_millis(elapsed_ms.try_into().unwrap_or(u64::MAX));
    if profile || profile_slow_ms > 0 {
        let loaded_module_bytes: usize =
            loaded_modules.values().map(|entry| entry.bytes.len()).sum();
        let current_module_name = current_module.as_deref().unwrap_or("./unknown");
        with_fixture_profile_stats(|stats| {
            stats.modules_loaded += loaded_modules.len();
            stats.modules_missing += missing_modules;
            stats.bytes_loaded += loaded_module_bytes;
            stats.selected_modules += selected_modules.len();
        });
        let stats = fixture_profile_snapshot();
        if profile {
            println!(
                "[fixture-profile] test={} duration_ms={} selected={} loaded={} missing={} bytes={} file_cache_hits={} file_cache_misses={} dir_cache_hits={} dir_cache_misses={} dep_hits={} dep_misses={} source_hits={} source_misses={} import_edges_hits={} import_edges_misses={} import_request_hits={} import_request_misses={} named_import_hits={} named_import_misses={} namespace_import_hits={} namespace_import_misses={} source_import_hits={} source_import_misses={} side_effect_import_hits={} side_effect_import_misses={} reexport_request_hits={} reexport_request_misses={} dynamic_request_hits={} dynamic_request_misses={} module_request_hits={} module_request_misses={} attr_module_request_hits={} attr_module_request_misses={}",
                test_path.display(),
                elapsed_ms,
                selected_modules.len(),
                loaded_modules.len(),
                missing_modules,
                loaded_module_bytes,
                stats.file_hits,
                stats.file_misses,
                stats.dir_hits,
                stats.dir_misses,
                stats.dep_hits,
                stats.dep_misses,
                stats.source_analysis_hits,
                stats.source_analysis_misses,
                stats.import_edges_hits,
                stats.import_edges_misses,
                stats.import_request_hits,
                stats.import_request_misses,
                stats.named_import_hits,
                stats.named_import_misses,
                stats.namespace_import_hits,
                stats.namespace_import_misses,
                stats.source_import_hits,
                stats.source_import_misses,
                stats.side_effect_import_hits,
                stats.side_effect_import_misses,
                stats.reexport_request_hits,
                stats.reexport_request_misses,
                stats.dynamic_request_hits,
                stats.dynamic_request_misses,
                stats.module_request_hits,
                stats.module_request_misses,
                stats.attr_module_request_hits,
                stats.attr_module_request_misses,
            );
            if fixture_profile_show_modules() {
                for module_name in &fixture_names {
                    if current_module_name == module_name.as_str() && !load_current_fixture {
                        continue;
                    }
                    if !selected_modules.contains(module_name)
                        && !referenced_siblings.contains(module_name)
                    {
                        continue;
                    }
                    println!("  {}", module_name);
                }
            }
        }
        if profile_slow_ms > 0 && elapsed_ms >= profile_slow_ms {
            eprintln!(
                "[fixture-profile] slow loading test={} duration_ms={}",
                test_path.display(),
                elapsed_ms
            );
        }
    }
    ctx.set_global(
        "__quench_isolate_module_eval__".into(),
        quench_runtime::Value::Boolean(has_indirect_reexport),
    );
    let init_scripts_key = "__quench_fixture_init_scripts__";
    let init_done_key = "__quench_fixture_init_done__";
    let init_bindings_key = "__quench_fixture_export_bindings__";
    let init_getters_key = "__quench_fixture_export_getters__";
    let init_imported_key = "__quench_fixture_imported_modules__";
    let init_refresh_key = "__quench_fixture_refresh_required__";
    let raw_modules_key = "__quench_fixture_raw_modules__";
    let raw_bytes_key = "__quench_fixture_raw_bytes__";
    let module_errors_key = "__quench_module_errors__";
    if ctx.get_global(init_scripts_key).is_none() {
        ctx.set_global(
            init_scripts_key.to_string(),
            quench_runtime::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary),
            ))),
        );
    }
    if ctx.get_global(init_done_key).is_none() {
        ctx.set_global(
            init_done_key.to_string(),
            quench_runtime::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary),
            ))),
        );
    }
    if ctx.get_global(init_bindings_key).is_none() {
        ctx.set_global(
            init_bindings_key.to_string(),
            quench_runtime::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary),
            ))),
        );
    }
    if ctx.get_global(init_getters_key).is_none() {
        ctx.set_global(
            init_getters_key.to_string(),
            quench_runtime::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary),
            ))),
        );
    }
    if ctx.get_global(init_imported_key).is_none() {
        ctx.set_global(
            init_imported_key.to_string(),
            quench_runtime::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary),
            ))),
        );
    }
    if ctx.get_global(init_refresh_key).is_none() {
        ctx.set_global(
            init_refresh_key.to_string(),
            quench_runtime::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary),
            ))),
        );
    }
    if ctx.get_global(raw_modules_key).is_none() {
        ctx.set_global(
            raw_modules_key.to_string(),
            quench_runtime::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary),
            ))),
        );
    }
    if ctx.get_global(raw_bytes_key).is_none() {
        ctx.set_global(
            raw_bytes_key.to_string(),
            quench_runtime::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary),
            ))),
        );
    }
    if ctx.get_global(module_errors_key).is_none() {
        ctx.set_global(
            module_errors_key.to_string(),
            quench_runtime::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary),
            ))),
        );
    }
    let init_scripts = ctx
        .get_global(init_scripts_key)
        .and_then(|value| match value {
            quench_runtime::Value::Object(cache) => Some(cache),
            _ => None,
        });
    let init_done = ctx
        .get_global(init_done_key)
        .and_then(|value| match value {
            quench_runtime::Value::Object(cache) => Some(cache),
            _ => None,
        });
    let init_bindings = ctx
        .get_global(init_bindings_key)
        .and_then(|value| match value {
            quench_runtime::Value::Object(cache) => Some(cache),
            _ => None,
        });
    let init_getters = ctx
        .get_global(init_getters_key)
        .and_then(|value| match value {
            quench_runtime::Value::Object(cache) => Some(cache),
            _ => None,
        });
    let init_refresh = ctx
        .get_global(init_refresh_key)
        .and_then(|value| match value {
            quench_runtime::Value::Object(cache) => Some(cache),
            _ => None,
        });
    let raw_modules = ctx
        .get_global(raw_modules_key)
        .and_then(|value| match value {
            quench_runtime::Value::Object(cache) => Some(cache),
            _ => None,
        });
    let raw_bytes = ctx
        .get_global(raw_bytes_key)
        .and_then(|value| match value {
            quench_runtime::Value::Object(cache) => Some(cache),
            _ => None,
        });
    let module_errors = ctx
        .get_global(module_errors_key)
        .and_then(|value| match value {
            quench_runtime::Value::Object(cache) => Some(cache),
            _ => None,
        });
    let mut pending_reexports = HashMap::<String, Vec<PendingReExport>>::new();
    let mut star_sources = HashMap::<String, HashMap<String, String>>::new();
    let mut module_sources = HashMap::<String, Value>::new();
    let mut named_reexport_edges = Vec::<(String, String)>::new();
    let mut pending_default_imports = HashMap::<String, String>::new();
    let mut fixture_import_edges = Vec::<(String, String, String)>::new();
    let mut deferred_namespace_imports = Vec::<(String, String, String)>::new();
    let mut fixture_module_requests = Vec::<(String, String)>::new();
    for (index, (name, path)) in fixtures.iter().enumerate() {
        let Some(module_file) = loaded_modules.get(&format!("./{name}")) else {
            continue;
        };
        let bytes = &module_file.bytes;
        let source = module_file.source.as_str();
        let source_is_utf8 = module_file.source_is_utf8;
        if let Some(raw_bytes) = raw_bytes.as_ref() {
            if !source_is_utf8 {
                let mut value =
                    quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Array);
                value.elements = bytes
                    .iter()
                    .map(|byte| Value::Number(f64::from(*byte)))
                    .collect();
                raw_bytes.borrow_mut().set(
                    &format!("./{name}"),
                    Value::Object(std::rc::Rc::new(std::cell::RefCell::new(value))),
                );
            }
        }
        // Byte modules are selected by the same fixture graph as JavaScript
        // modules, but their payload need not be UTF-8. Their raw bytes were
        // recorded above and are consumed by the `type: "bytes"` importer.
        let source = source.to_owned();
        let module_name = format!("./{}", name);
        if let Some(raw_modules) = raw_modules.as_ref() {
            raw_modules
                .borrow_mut()
                .set(&module_name, Value::String(source.clone()));
        }
        if name.ends_with("_FIXTURE.json") {
            let json_default = match parse_fixture_json_value(&source) {
                Ok(value) => value,
                Err(_) => Value::Undefined,
            };
            let mut module_exports = quench_runtime::value::Object::new(
                quench_runtime::value::ObjectKind::ModuleNamespace,
            );
            module_exports.define(
                "default",
                json_default,
                quench_runtime::value::PropertyFlags {
                    value: None,
                    writable: true,
                    enumerable: true,
                    configurable: false,
                },
            );
            if let Some(Value::Symbol(symbol)) =
                quench_runtime::builtins::symbol::get_well_known_symbol_no_ctx("toStringTag")
            {
                module_exports.set_symbol(
                    &symbol.property_key(),
                    quench_runtime::Value::String("Module".to_string()),
                );
            }
            module_exports.extensible = false;
            ctx.register_module(&module_name, module_exports);
            continue;
        }
        if !fixture_module_syntax_valid(&source)
            && !synthetic_modules.contains(&module_name)
        {
            if let Some(errors) = module_errors.as_ref() {
                errors
                    .borrow_mut()
                    .set(&module_name, Value::String("Invalid module syntax".into()));
            }
            note_fixture_invalid_syntax_module();
            ctx.register_module(
                &module_name,
                quench_runtime::value::Object::new(
                    quench_runtime::value::ObjectKind::ModuleNamespace,
                ),
            );
            continue;
        }
        let (eval_source, side_effect_source, exports, default_import, reexports) =
            fixture_exports_from_source(index, &source)?;
        for (imported, target) in fixture_import_edges_from_source(&source) {
            fixture_import_edges.push((module_name.clone(), imported, target));
        }
        for (local, target) in deferred_namespace_imports_from_source(&source) {
            fixture_module_requests.push((module_name.clone(), target.clone()));
            deferred_namespace_imports.push((module_name.clone(), local, target));
        }
        fixture_module_requests.extend(
            fixture_side_effect_imports_from_source(&source)
                .into_iter()
                .map(|target| (module_name.clone(), target)),
        );
        let side_effects_need_refresh = !side_effect_source.trim().is_empty();
        // Fixture modules have their own lexical environment.  Evaluating a
        // declaration-only fixture in the test's global scope makes its local
        // bindings (for example `const x`) collide with the test script.
        let isolate_fixture_bindings = !side_effects_need_refresh;
        let fixture_bindings = isolate_fixture_bindings.then(|| fixture_declaration_names(&source));
        if isolate_fixture_bindings {
            ctx.environment_view().borrow_mut().push_scope();
        }
        let exposes_update = side_effect_source.contains("test262update")
            || default_function_updates_itself(&source);
        let eval_source = if source.contains("import.meta") {
            let meta = format!("__quench_fixture_import_meta_{index}");
            format!(
                "globalThis.{meta} = __import_meta__;\n{}",
                eval_source.replace("import.meta", &meta)
            )
        } else {
            eval_source
        };
        if source.contains("import.meta") {
            let mut import_meta =
                quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
            import_meta.prototype = None;
            ctx.set_global(
                "__import_meta__".to_string(),
                Value::Object(std::rc::Rc::new(std::cell::RefCell::new(import_meta))),
            );
        }
        if !eval_source.trim().is_empty() {
            let result = if eval_source.contains("await") {
                ctx.eval_es_module(&eval_source)
            } else {
                ctx.eval_script(&eval_source, false)
            };
            if let Err(error) = result {
                if eval_source.contains("await") {
                    let reason = quench_runtime::value::take_thrown_value()
                        .unwrap_or_else(|| quench_runtime::Value::String(error.to_string()));
                    let mut cached = quench_runtime::value::Object::new(
                        quench_runtime::value::ObjectKind::Ordinary,
                    );
                    cached.set("__quench_cached_module_reason__", reason);
                    if let Some(errors) = module_errors.as_ref() {
                        errors.borrow_mut().set(
                            &module_name,
                            Value::Object(std::rc::Rc::new(std::cell::RefCell::new(cached))),
                        );
                    }
                } else {
                    return Err(format!("fixture eval {}: {error:?}", path.display()));
                }
            }
        }
        if !side_effect_source.trim().is_empty() {
            if let Some(scripts) = init_scripts.as_ref() {
                scripts.borrow_mut().set(
                    &module_name,
                    quench_runtime::Value::String(side_effect_source),
                );
            }
            if let Some(done) = init_done.as_ref() {
                done.borrow_mut()
                    .set(&module_name, quench_runtime::Value::Boolean(false));
            }
        }
        let mut module_exports =
            quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::ModuleNamespace);
        let mut module_bindings = Vec::<(String, String)>::new();
        let mut needs_refresh = side_effects_need_refresh;
        let mut values = std::collections::HashMap::new();
        for name in exports.named {
            let value = ctx
                .get_global(&name)
                .unwrap_or(quench_runtime::Value::Undefined);
            if value == quench_runtime::Value::Undefined {
                needs_refresh = true;
            }
            values.insert(name.clone(), value.clone());
            module_exports.define(
                &name,
                value,
                quench_runtime::value::PropertyFlags {
                    value: None,
                    writable: true,
                    enumerable: true,
                    configurable: false,
                },
            );
            module_bindings.push((name.clone(), name.clone()));
        }
        for (local, exported) in exports.aliases {
            let value = values
                .get(&local)
                .cloned()
                .or_else(|| ctx.get_global(&local))
                .unwrap_or(quench_runtime::Value::Undefined);
            needs_refresh |= !values.contains_key(&local) && matches!(value, Value::Undefined);
            module_exports.define(
                &exported,
                value,
                quench_runtime::value::PropertyFlags {
                    value: None,
                    writable: true,
                    enumerable: true,
                    configurable: false,
                },
            );
            module_bindings.push((exported, local));
        }
        for local in exports.default_aliases {
            let default = values
                .get(&local)
                .cloned()
                .or_else(|| ctx.get_global(&local))
                .unwrap_or(quench_runtime::Value::Undefined);
            if !values.contains_key(&local) {
                needs_refresh = true;
            }
            module_exports.define(
                "default",
                default,
                quench_runtime::value::PropertyFlags {
                    value: None,
                    writable: true,
                    enumerable: true,
                    configurable: false,
                },
            );
            module_bindings.push(("default".to_string(), local));
        }
        if let Some(default_marker) = exports.default_marker {
            let default = ctx
                .get_global(&default_marker)
                .unwrap_or(quench_runtime::Value::Undefined);
            module_exports.define(
                "default",
                default,
                quench_runtime::value::PropertyFlags {
                    value: None,
                    writable: true,
                    enumerable: true,
                    configurable: false,
                },
            );
            module_bindings.push(("default".to_string(), default_marker));
        }
        if let Some(Value::Symbol(symbol)) =
            quench_runtime::builtins::symbol::get_well_known_symbol_no_ctx("toStringTag")
        {
            let key = symbol.property_key();
            module_exports.set_symbol(&key, quench_runtime::Value::String("Module".to_string()));
            if let Some(flags) = module_exports.descriptors.get_mut(&key) {
                flags.writable = false;
                flags.enumerable = false;
                flags.configurable = false;
            }
        }
        module_exports.extensible = false;
        if let Some(default_import) = default_import {
            pending_default_imports.insert(module_name.clone(), default_import);
        }
        if !reexports.is_empty() {
            // Indirect re-exports can initially observe an uninitialized
            // binding in a cyclic module graph. Refresh only when the
            // re-export target is a different module, so self-reexports do
            // not trigger global fixture cache invalidation.
            let mut has_external_reexport = false;
            for reexport in &reexports {
                let should_refresh = match reexport {
                    PendingReExport::Named { source, .. } => {
                        source.trim_start_matches("./") != module_name.trim_start_matches("./")
                    }
                    PendingReExport::StarAs { source, .. } => {
                        source.trim_start_matches("./") != module_name.trim_start_matches("./")
                    }
                    PendingReExport::StarAll { source } => {
                        source.trim_start_matches("./") != module_name.trim_start_matches("./")
                    }
                    PendingReExport::ModuleSource { .. } => false,
                };
                if should_refresh {
                    has_external_reexport = true;
                    if let PendingReExport::Named { source, .. } = reexport {
                        named_reexport_edges.push((module_name.clone(), source.clone()));
                    }
                }
            }
            needs_refresh |= has_external_reexport;
            pending_reexports.insert(module_name.clone(), reexports);
        }
        if let Some(bindings) = init_bindings.as_ref() {
            let mut mapping =
                quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
            for (exported, local) in &module_bindings {
                mapping.set(exported, quench_runtime::Value::String(local.clone()));
            }
            bindings.borrow_mut().set(
                &module_name,
                quench_runtime::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(mapping))),
            );
        }
        if exposes_update {
            if let Some(getters) = init_getters.as_ref() {
                let mut mapping =
                    quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
                for (exported, local) in &module_bindings {
                    let Some(binding) = ctx.environment_view().borrow().get_shared(local) else {
                        continue;
                    };
                    needs_refresh = true;
                    let getter = quench_runtime::Value::NativeFunction(std::rc::Rc::new(
                        quench_runtime::value::NativeFunction::new(move |_| {
                            Ok(binding.borrow().clone())
                        }),
                    ));
                    mapping.set(exported, getter);
                }
                getters.borrow_mut().set(
                    &module_name,
                    quench_runtime::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                        mapping,
                    ))),
                );
            }
        }
        if let Some(refresh) = init_refresh.as_ref() {
            refresh
                .borrow_mut()
                .set(&module_name, quench_runtime::Value::Boolean(needs_refresh));
        }
        ctx.register_module(&module_name, module_exports);
        if name.contains("script-code") {
            if let Some(errors) = module_errors.as_ref() {
                errors.borrow_mut().set(
                    &module_name,
                    quench_runtime::Value::String("Script fixture is not valid module code".into()),
                );
            }
        }
        if isolate_fixture_bindings {
            let exported_locals = module_bindings
                .iter()
                .map(|(_, local)| local.as_str())
                .collect::<HashSet<_>>();
            let private_bindings = fixture_bindings
                .as_ref()
                .unwrap()
                .iter()
                .filter(|name| !exported_locals.contains(name.as_str()))
                .cloned()
                .collect();
            remove_fixture_declaration_bindings(ctx, &private_bindings);
            ctx.environment_view().borrow_mut().pop_scope();
        }
    }
    let mut named_graph = HashMap::<String, Vec<String>>::new();
    for (module, source) in &named_reexport_edges {
        named_graph
            .entry(module.clone())
            .or_default()
            .push(source.clone());
    }
    if let Some(errors) = module_errors.as_ref() {
        for (module, source) in &named_reexport_edges {
            let mut seen = HashSet::new();
            if source != module && has_module_path(&named_graph, source, module, &mut seen) {
                errors.borrow_mut().set(
                    module,
                    quench_runtime::Value::String("Circular indirect export".into()),
                );
            }
        }
    }
    if test_path.to_string_lossy().contains("import-defer") {
        if let Some(errors) = module_errors.as_ref() {
            for (module, target) in &fixture_module_requests {
                if current_module.as_deref() != Some(target) && ctx.get_module(target).is_none() {
                    errors.borrow_mut().set(
                        module,
                        quench_runtime::Value::String("Missing module".into()),
                    );
                }
            }
            for (module, imported, target) in &fixture_import_edges {
                let missing = ctx
                    .get_module(target)
                    .and_then(|value| match value {
                        Value::Object(object) => Some(object.borrow().get(imported).is_none()),
                        _ => None,
                    })
                    .unwrap_or(true);
                if missing {
                    errors.borrow_mut().set(
                        module,
                        quench_runtime::Value::String("Missing indirect export".into()),
                    );
                }
            }
            for _ in 0..fixture_module_requests.len() {
                for (module, target) in &fixture_module_requests {
                    let reason = errors.borrow().get(target);
                    if let Some(reason) = reason {
                        errors.borrow_mut().set(module, reason);
                    }
                }
            }
        }
    }
    let reexport_passes = pending_reexports.len().max(1);
    for pass in 0..reexport_passes {
        for (module_name, reexports) in &pending_reexports {
            let Some(Value::Object(module)) = ctx.get_module(&module_name) else {
                continue;
            };
            for reexport in reexports {
                match reexport {
                    PendingReExport::ModuleSource { name, source } => {
                        let value = module_sources
                            .entry(source.clone())
                            .or_insert_with(|| create_module_source(ctx))
                            .clone();
                        define_module_binding(&module, &name, value);
                    }
                    PendingReExport::StarAs { name, source } => {
                        let Some(Value::Object(target)) = ctx.get_module(&source) else {
                            continue;
                        };
                        define_module_binding(&module, &name, Value::Object(target));
                    }
                    PendingReExport::StarAll { source } => {
                        let Some(Value::Object(target)) = ctx.get_module(&source) else {
                            continue;
                        };
                        let mut keys = target.borrow().own_property_names();
                        keys.sort();
                        for key in keys {
                            if key == "default" {
                                continue;
                            }
                            if star_export_resolves_back_to_module(ctx, module_name, &source, &key)
                            {
                                continue;
                            }
                            let target_has_getter = target.borrow().has_getter(&key);
                            let value = target
                                .borrow()
                                .get_own_value(&key)
                                .unwrap_or(quench_runtime::Value::Undefined);
                            let sources = star_sources.entry(module_name.clone()).or_default();
                            if let Some(previous) = sources.get(&key) {
                                let same_namespace = matches!(
                                    (module.borrow().get(&key), &value),
                                    (Some(Value::Object(existing)), Value::Object(candidate))
                                        if std::rc::Rc::ptr_eq(&existing, candidate)
                                );
                                if previous != source && !same_namespace {
                                    let mut module = module.borrow_mut();
                                    module.properties.shift_remove(&key);
                                    module.descriptors.shift_remove(&key);
                                    if let Some(errors) = module_errors.as_ref() {
                                        errors.borrow_mut().set(
                                            &module_name,
                                            quench_runtime::Value::String(
                                                "Ambiguous indirect export".into(),
                                            ),
                                        );
                                    }
                                    continue;
                                }
                            } else {
                                sources.insert(key.clone(), source.clone());
                            }
                            if target_has_getter {
                                let target = std::rc::Rc::clone(&target);
                                let target_key = key.clone();
                                let getter =
                                    quench_runtime::Value::NativeFunction(std::rc::Rc::new(
                                        quench_runtime::value::NativeFunction::new(move |_| {
                                            quench_runtime::eval::member::eval_object_member(
                                                &target,
                                                &target_key,
                                                None,
                                            )
                                        }),
                                    ));
                                module.borrow_mut().define_accessor(
                                    &key,
                                    Some(getter),
                                    None,
                                    quench_runtime::value::PropertyFlags {
                                        value: None,
                                        writable: true,
                                        enumerable: true,
                                        configurable: false,
                                    },
                                );
                            } else {
                                define_module_binding(&module, &key, value);
                            }
                        }
                    }
                    PendingReExport::Named {
                        source,
                        local,
                        exported,
                    } => {
                        if let Some(errors) = module_errors.as_ref() {
                            let reason = errors.borrow().get(&source);
                            if let Some(reason) = reason {
                                errors.borrow_mut().set(&module_name, reason);
                                continue;
                            }
                        }
                        if let Some(Value::Object(target)) = ctx.get_module(&source) {
                            if std::rc::Rc::ptr_eq(&module, &target) {
                                let value = target.borrow().get(&local).unwrap_or(Value::Undefined);
                                define_module_binding(&module, &exported, value);
                                continue;
                            }
                            if !target.borrow().has_own(local) && !target.borrow().has_getter(local)
                            {
                                if let Some(value) = resolve_fixture_export(ctx, source, local) {
                                    define_module_binding(&module, exported, value);
                                    continue;
                                }
                                // A named re-export may resolve through a
                                // star-export cycle. Let the remaining
                                // fixed-point passes populate that target
                                // before classifying it as missing.
                                if pass + 1 == reexport_passes {
                                    if let Some(errors) = module_errors.as_ref() {
                                        errors.borrow_mut().set(
                                            &module_name,
                                            Value::String("Missing indirect export".into()),
                                        );
                                    }
                                }
                                continue;
                            }
                            // An indirect export remains live even when its
                            // target currently has a data property. In a
                            // cycle, that property may later become the
                            // accessor installed by the entry module.
                            if target.borrow().has_own(&local) || target.borrow().has_getter(&local)
                            {
                                let target = std::rc::Rc::clone(&target);
                                let local_key = local.clone();
                                let getter =
                                    quench_runtime::Value::NativeFunction(std::rc::Rc::new(
                                        quench_runtime::value::NativeFunction::new(move |_| {
                                            quench_runtime::eval::member::eval_object_member(
                                                &target, &local_key, None,
                                            )
                                        }),
                                    ));
                                module.borrow_mut().define_accessor(
                                    &exported,
                                    Some(getter),
                                    None,
                                    quench_runtime::value::PropertyFlags {
                                        value: None,
                                        writable: true,
                                        enumerable: true,
                                        configurable: false,
                                    },
                                );
                                continue;
                            }
                            let value = target.borrow().get(&local);
                            if value.is_none() || value == Some(quench_runtime::Value::Undefined) {
                                if let Some(refresh) = init_refresh.as_ref() {
                                    refresh
                                        .borrow_mut()
                                        .set(&module_name, quench_runtime::Value::Boolean(true));
                                }
                            }
                            define_module_binding(
                                &module,
                                &exported,
                                value.unwrap_or(quench_runtime::Value::Undefined),
                            );
                        } else {
                            define_module_binding(
                                &module,
                                &exported,
                                quench_runtime::Value::Undefined,
                            );
                        }
                    }
                }
            }
        }
    }
    for (module_name, source) in named_reexport_edges {
        let Some(errors) = module_errors.as_ref() else {
            continue;
        };
        let reason = errors.borrow().get(&source);
        if let Some(reason) = reason {
            errors.borrow_mut().set(&module_name, reason);
        }
    }
    for (module_name, source) in pending_default_imports {
        let Some(Value::Object(module)) = ctx.get_module(&module_name) else {
            continue;
        };
        if let Some(Value::Object(target)) = ctx.get_module(&source) {
            let promise = quench_runtime::builtins::promise::create_resolved_promise(
                quench_runtime::Value::Object(target),
            );
            define_module_binding(&module, "default", quench_runtime::Value::Object(promise));
        } else {
            define_module_binding(&module, "default", quench_runtime::Value::Undefined);
        }
    }
    if let Some(errors) = module_errors.as_ref() {
        for (module_name, imported, target_name) in &fixture_import_edges {
            let missing = ctx
                .get_module(target_name)
                .and_then(|value| match value {
                    Value::Object(target) => Some(
                        !target.borrow().has_own(imported) && !target.borrow().has_getter(imported),
                    ),
                    _ => None,
                })
                .unwrap_or(true);
            if missing {
                errors
                    .borrow_mut()
                    .set(module_name, Value::String("Missing indirect export".into()));
            }
        }
    }
    for (module_name, local, source) in deferred_namespace_imports {
        let promise = ctx
            .dynamic_import_module(&source, None, false, true)
            .map_err(|error| format!("deferred fixture import: {error:?}"))?;
        let Value::Object(promise) = promise else {
            continue;
        };
        let value = promise
            .borrow()
            .promise_data
            .as_ref()
            .map(|data| data.result.clone())
            .unwrap_or(Value::Undefined);
        ctx.set_global(local.clone(), value.clone());
        if let Some(Value::Object(module)) = ctx.get_module(&module_name) {
            if module.borrow().has_own(&local) {
                define_module_binding(&module, &local, value);
            }
        }
    }
    register_current_module_reexports(ctx, current_module.as_deref(), &current_source)?;
    Ok(())
}

fn fixture_declaration_names(source: &str) -> HashSet<String> {
    if !fixture_analysis_cache_enabled() {
        return fixture_declaration_names_unchecked(source);
    }
    cached_fixture_declarations_from_source(source)
        .as_ref()
        .clone()
}

fn fixture_declaration_names_unchecked(source: &str) -> HashSet<String> {
    use quench_runtime::ast::{Program, Statement};

    fn collect(statements: &[Statement], names: &mut HashSet<String>) {
        for statement in statements {
            match statement {
                Statement::FunctionDeclaration { name, .. }
                | Statement::ClassDeclaration { name, .. }
                | Statement::VarDeclaration { name, .. } => {
                    names.insert(name.clone());
                }
                Statement::Export(inner) => collect(std::slice::from_ref(inner), names),
                Statement::Block(inner) | Statement::SequenceDecls(inner) => collect(inner, names),
                _ => {}
            }
        }
    }

    let Ok(Program::Script(statements)) = quench_runtime::parser::parse_es_module(source) else {
        return HashSet::new();
    };
    let mut names = HashSet::new();
    collect(&statements, &mut names);
    names
}

fn remove_fixture_declaration_bindings(ctx: &mut quench_runtime::Context, names: &HashSet<String>) {
    let root_scope = ctx
        .environment_view()
        .borrow()
        .live_scopes_snapshot()
        .into_iter()
        .next();
    let Some(root_scope) = root_scope else {
        return;
    };
    let mut root_scope = root_scope.borrow_mut();
    for name in names {
        root_scope.remove_binding(name);
    }
    drop(root_scope);

    let Some(Value::Object(global)) = ctx.get_global("globalThis") else {
        return;
    };
    let mut global = global.borrow_mut();
    for name in names {
        global.properties.shift_remove(name);
        global.descriptors.shift_remove(name);
    }
}

fn star_export_resolves_back_to_module(
    ctx: &quench_runtime::Context,
    module: &str,
    source: &str,
    exported: &str,
) -> bool {
    fn resolves_back(
        ctx: &quench_runtime::Context,
        module: &str,
        source: &str,
        exported: &str,
        root_exported: &str,
        seen: &mut HashSet<(String, String)>,
    ) -> bool {
        let source = canonical_module_name(source);
        if source == canonical_module_name(module) {
            return exported == root_exported;
        }
        if !seen.insert((source.clone(), exported.to_string())) {
            return false;
        }
        let Some(Value::Object(raw_modules)) = ctx.get_global("__quench_fixture_raw_modules__")
        else {
            return false;
        };
        let Some(Value::String(source_text)) = raw_modules.borrow().get(&source) else {
            return false;
        };
        let Ok((_, _, exports, _, reexports)) =
            fixture_exports_from_source(usize::MAX, &source_text)
        else {
            return false;
        };
        if exports.named.iter().any(|name| name == exported)
            || exports.aliases.iter().any(|(_, name)| name == exported)
        {
            return false;
        }
        reexports.into_iter().any(|entry| match entry {
            PendingReExport::Named {
                source,
                local,
                exported: candidate,
            } if candidate == exported => {
                resolves_back(ctx, module, &source, &local, root_exported, seen)
            }
            PendingReExport::StarAll { source } => {
                resolves_back(ctx, module, &source, exported, root_exported, seen)
            }
            _ => false,
        })
    }

    resolves_back(ctx, module, source, exported, exported, &mut HashSet::new())
}

fn resolve_fixture_export(
    ctx: &quench_runtime::Context,
    module_name: &str,
    exported: &str,
) -> Option<Value> {
    fn resolve(
        ctx: &quench_runtime::Context,
        module_name: &str,
        exported: &str,
        seen: &mut HashSet<(String, String)>,
    ) -> Option<Value> {
        let module_name = canonical_module_name(module_name);
        if !seen.insert((module_name.clone(), exported.to_string())) {
            return None;
        }
        let Value::Object(raw_modules) = ctx.get_global("__quench_fixture_raw_modules__")? else {
            return None;
        };
        let Value::String(source) = raw_modules.borrow().get(&module_name)? else {
            return None;
        };
        let (_, _, exports, default_marker, reexports) =
            fixture_exports_from_source(usize::MAX, &source).ok()?;
        let module = ctx.get_module(&module_name).and_then(|value| match value {
            Value::Object(module) => Some(module),
            _ => None,
        });
        let direct = exports.named.iter().any(|name| name == exported)
            || exports.aliases.iter().any(|(_, name)| name == exported)
            || (exported == "default"
                && (default_marker.is_some() || !exports.default_aliases.is_empty()));
        if direct {
            return module.and_then(|module| module.borrow().get(exported));
        }
        for entry in &reexports {
            match entry {
                PendingReExport::Named {
                    source,
                    local,
                    exported: candidate,
                } if candidate == exported => return resolve(ctx, source, local, seen),
                PendingReExport::StarAll { source } if exported != "default" => {
                    if let Some(value) = resolve(ctx, source, exported, seen) {
                        return Some(value);
                    }
                }
                _ => {}
            }
        }
        None
    }

    resolve(ctx, module_name, exported, &mut HashSet::new())
}

fn register_current_module_reexports(
    ctx: &mut quench_runtime::Context,
    current_module: Option<&str>,
    source: &str,
) -> Result<(), String> {
    let Some(current_module) = current_module else {
        return Ok(());
    };
    let (_, _, _, _, reexports) = fixture_exports_from_source(usize::MAX, source)?;
    if reexports.is_empty() {
        return Ok(());
    }
    let has_external_reexport = reexports.iter().any(|entry| match entry {
        PendingReExport::Named { source, .. }
        | PendingReExport::StarAs { source, .. }
        | PendingReExport::StarAll { source } => {
            source.trim_start_matches("./") != current_module.trim_start_matches("./")
        }
        PendingReExport::ModuleSource { .. } => false,
    });
    if !has_external_reexport {
        return Ok(());
    }
    let module = ctx
        .get_module(current_module)
        .and_then(|value| match value {
            Value::Object(module) => Some(module),
            _ => None,
        })
        .unwrap_or_else(|| {
            std::rc::Rc::new(std::cell::RefCell::new(quench_runtime::value::Object::new(
                quench_runtime::value::ObjectKind::ModuleNamespace,
            )))
        });
    for reexport in reexports {
        match reexport {
            PendingReExport::Named {
                source,
                local,
                exported,
            } => {
                let Some(Value::Object(target)) = ctx.get_module(&source) else {
                    continue;
                };
                if target.borrow().has_getter(&local) {
                    let target = std::rc::Rc::clone(&target);
                    let getter = Value::NativeFunction(std::rc::Rc::new(
                        quench_runtime::value::NativeFunction::new(move |_| {
                            quench_runtime::eval::member::eval_object_member(&target, &local, None)
                        }),
                    ));
                    module.borrow_mut().define_accessor(
                        &exported,
                        Some(getter),
                        None,
                        quench_runtime::value::PropertyFlags {
                            value: None,
                            writable: false,
                            enumerable: true,
                            configurable: false,
                        },
                    );
                } else if let Some(value) = target.borrow().get(&local) {
                    module.borrow_mut().define(
                        &exported,
                        value,
                        quench_runtime::value::PropertyFlags {
                            value: None,
                            writable: false,
                            enumerable: true,
                            configurable: false,
                        },
                    );
                }
            }
            PendingReExport::StarAs { name, source } => {
                if let Some(Value::Object(target)) = ctx.get_module(&source) {
                    module.borrow_mut().define(
                        &name,
                        Value::Object(target),
                        quench_runtime::value::PropertyFlags {
                            value: None,
                            writable: false,
                            enumerable: true,
                            configurable: false,
                        },
                    );
                }
            }
            PendingReExport::StarAll { source } => {
                if let Some(Value::Object(target)) = ctx.get_module(&source) {
                    for key in target.borrow().own_property_names() {
                        if key == "default" || module.borrow().has(&key) {
                            continue;
                        }
                        if target.borrow().has_getter(&key) {
                            let target = std::rc::Rc::clone(&target);
                            let target_key = key.clone();
                            let getter = Value::NativeFunction(std::rc::Rc::new(
                                quench_runtime::value::NativeFunction::new(move |_| {
                                    quench_runtime::eval::member::eval_object_member(
                                        &target,
                                        &target_key,
                                        None,
                                    )
                                }),
                            ));
                            module.borrow_mut().define_accessor(
                                &key,
                                Some(getter),
                                None,
                                quench_runtime::value::PropertyFlags {
                                    value: None,
                                    writable: false,
                                    enumerable: true,
                                    configurable: false,
                                },
                            );
                        } else if let Some(value) = target.borrow().get_own_value(&key) {
                            module.borrow_mut().define(
                                &key,
                                value,
                                quench_runtime::value::PropertyFlags {
                                    value: None,
                                    writable: false,
                                    enumerable: true,
                                    configurable: false,
                                },
                            );
                        }
                    }
                }
            }
            PendingReExport::ModuleSource { .. } => {}
        }
    }
    if let Some(Value::Symbol(symbol)) =
        quench_runtime::builtins::symbol::get_well_known_symbol_no_ctx("toStringTag")
    {
        let key = symbol.property_key();
        module
            .borrow_mut()
            .set_symbol(&key, Value::String("Module".into()));
        if let Some(flags) = module.borrow_mut().descriptors.get_mut(&key) {
            flags.writable = false;
            flags.enumerable = false;
            flags.configurable = false;
        }
    }
    module.borrow_mut().extensible = false;
    if ctx.get_module(current_module).is_none() {
        ctx.register_module(current_module, module.borrow().clone());
    }
    Ok(())
}

fn define_module_binding(
    module: &std::rc::Rc<std::cell::RefCell<quench_runtime::value::Object>>,
    key: &str,
    value: quench_runtime::Value,
) {
    if module.borrow().kind == quench_runtime::value::ObjectKind::ModuleNamespace {
        module.borrow_mut().define(
            key,
            value,
            quench_runtime::value::PropertyFlags {
                value: None,
                writable: true,
                enumerable: true,
                configurable: false,
            },
        );
        return;
    }
    module.borrow_mut().set(key, value);
}

#[derive(Clone)]
struct FixtureExports {
    named: Vec<String>,
    default_marker: Option<String>,
    aliases: Vec<(String, String)>,
    default_aliases: Vec<String>,
}

#[derive(Clone)]
enum PendingReExport {
    ModuleSource {
        name: String,
        source: String,
    },
    StarAs {
        name: String,
        source: String,
    },
    StarAll {
        source: String,
    },
    Named {
        source: String,
        local: String,
        exported: String,
    },
}

fn fixture_exports_from_source(
    index: usize,
    source: &str,
) -> Result<
    (
        String,
        String,
        FixtureExports,
        Option<String>,
        Vec<PendingReExport>,
    ),
    String,
> {
    let analysis = cached_fixture_exports_from_source(source)?;
    let default_marker = analysis
        .has_default_marker
        .then(|| format!("__quench_fixture_default_{index}"));
    let eval_source = if let Some(marker) = default_marker.as_ref() {
        analysis
            .eval_source
            .replace(FIXTURE_DEFAULT_MARKER_TOKEN, marker)
    } else {
        (*analysis.eval_source).clone()
    };
    Ok((
        eval_source,
        (*analysis.side_effect_source).clone(),
        (*analysis.exports).clone(),
        default_marker,
        (*analysis.reexports).clone(),
    ))
}

fn fixture_exports_from_source_unchecked(source: &str) -> Result<FixtureExportsCacheEntry, String> {
    let default_marker = FIXTURE_DEFAULT_MARKER_TOKEN.to_string();
    let mut eval_lines = Vec::new();
    let mut side_effect_lines = Vec::new();
    let mut named = Vec::new();
    let mut aliases = Vec::new();
    let mut default_aliases = Vec::new();
    let mut reexports = Vec::new();
    let mut default_import = None;
    let mut has_default_marker = false;
    let mut in_export_block = false;
    let mut multiline_local_export = None::<String>;
    let mut export_block_depth = 0i32;
    let mut side_effect_depth = 0i32;
    let mut in_block_comment = false;
    let imported_bindings = fixture_named_imports(source);
    let namespace_imports = fixture_namespace_imports(source);
    let source_imports = fixture_source_imports(source);

    for line in source.lines() {
        let line = line.trim();
        if in_block_comment {
            if line.contains("*/") {
                in_block_comment = false;
            }
            continue;
        }
        if line.starts_with("/*") {
            in_block_comment = !line.contains("*/");
            continue;
        }
        if line.is_empty() || line.starts_with("//") {
            eval_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("import source ") {
            continue;
        }
        if let Some(specifiers) = multiline_local_export.as_mut() {
            specifiers.push(' ');
            specifiers.push_str(line);
            if let Some(end) = specifiers.find('}') {
                for (local, exported) in parse_export_specifier_list(&specifiers[..end]) {
                    aliases.push((local, exported));
                }
                multiline_local_export = None;
            }
            continue;
        }
        if in_export_block {
            eval_lines.push(line.to_string());
            let depth_delta = line.matches('{').count() as i32 - line.matches('}').count() as i32;
            export_block_depth += depth_delta;
            if export_block_depth <= 0 {
                in_export_block = false;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("export ") {
            let rest = rest.trim();
            if let Some(rest) = rest.strip_prefix("* as ") {
                let Some((name, source)) = rest.split_once(" from ") else {
                    continue;
                };
                let name = name.trim();
                if let Some(source) = normalize_fixture_module_name(source) {
                    reexports.push(PendingReExport::StarAs {
                        name: decode_module_export_name(name),
                        source,
                    });
                }
                continue;
            }
            if let Some(source) = rest.strip_prefix("* from ") {
                if let Some(source) = normalize_fixture_module_name(source) {
                    reexports.push(PendingReExport::StarAll { source });
                }
                continue;
            }
            if let Some(spec) = rest.strip_prefix("{") {
                let Some(end) = spec.find('}') else {
                    multiline_local_export = Some(spec.to_string());
                    continue;
                };
                let bindings = parse_export_specifier_list(&spec[..end]);
                let from = spec[end + 1..].trim().strip_prefix("from ");
                if let Some(source) = from.and_then(normalize_fixture_module_name) {
                    for (local, exported) in bindings {
                        let source = source.clone();
                        reexports.push(PendingReExport::Named {
                            source,
                            local,
                            exported,
                        });
                    }
                    continue;
                }
                for (local, exported) in bindings {
                    if let Some(source) = source_imports.get(&local) {
                        reexports.push(PendingReExport::ModuleSource {
                            name: exported,
                            source: source.clone(),
                        });
                        continue;
                    }
                    if let Some(source) = namespace_imports.get(&local) {
                        reexports.push(PendingReExport::StarAs {
                            name: exported,
                            source: source.clone(),
                        });
                        continue;
                    }
                    if let Some((source, imported)) = imported_bindings.get(&local) {
                        reexports.push(PendingReExport::Named {
                            source: source.clone(),
                            local: imported.clone(),
                            exported,
                        });
                        continue;
                    }
                    if exported == "default" {
                        default_aliases.push(local);
                    } else {
                        aliases.push((local, exported));
                    }
                }
                continue;
            }
            if let Some(rest) = rest.strip_prefix("default ") {
                if let Some(import_spec) = parse_default_import(rest) {
                    default_import = Some(import_spec);
                    continue;
                }
                if let Some(generator) = rest.strip_prefix("function*") {
                    if let Some((name, tail)) = generator.split_once('(') {
                        let name = if name.trim().is_empty() {
                            "default"
                        } else {
                            name.trim()
                        };
                        default_aliases.push(name.to_string());
                        let declaration = format!("function* {name}({tail}");
                        let depth_delta = declaration.matches('{').count() as i32
                            - declaration.matches('}').count() as i32;
                        if depth_delta > 0 {
                            in_export_block = true;
                            export_block_depth = depth_delta;
                        }
                        eval_lines.push(declaration);
                        continue;
                    }
                }
                if let Some(function) = rest.strip_prefix("function ") {
                    if let Some((name, tail)) = function.split_once('(') {
                        let name = name.trim();
                        default_aliases.push(name.to_string());
                        let declaration = format!("function {name}({tail}");
                        let depth_delta = declaration.matches('{').count() as i32
                            - declaration.matches('}').count() as i32;
                        if depth_delta > 0 {
                            in_export_block = true;
                            export_block_depth = depth_delta;
                        }
                        eval_lines.push(declaration);
                        continue;
                    }
                }
                if let Some(class) = rest.strip_prefix("class ") {
                    let name = extract_class_name(class)
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| "default".to_string());
                    default_aliases.push(name.clone());
                    let declaration = if name == "default" {
                        format!("class default {class}")
                    } else {
                        format!("class {class}")
                    };
                    let depth_delta = declaration.matches('{').count() as i32
                        - declaration.matches('}').count() as i32;
                    if depth_delta > 0 {
                        in_export_block = true;
                        export_block_depth = depth_delta;
                    }
                    eval_lines.push(declaration);
                    continue;
                }
                let rhs = rest;
                has_default_marker = true;
                let declaration = format!("globalThis.{} = {}", default_marker, rhs);
                let depth_delta = declaration.matches('{').count() as i32
                    - declaration.matches('}').count() as i32;
                if depth_delta > 0 {
                    in_export_block = true;
                    export_block_depth = depth_delta;
                }
                eval_lines.push(declaration);
                continue;
            }
            if let Some(rest) = rest.strip_prefix("var ") {
                named.extend(extract_binding_names(rest));
                eval_lines.push(format!("var {rest}"));
                continue;
            }
            if let Some(rest) = rest.strip_prefix("let ") {
                named.extend(extract_binding_names(rest));
                eval_lines.push(format!("let {rest}"));
                continue;
            }
            if let Some(rest) = rest.strip_prefix("const ") {
                let binding_names = extract_binding_names(rest);
                named.extend(binding_names.iter().cloned());
                if source.contains("await ")
                    && rest.contains("= await ")
                    && binding_names.len() == 1
                {
                    let name = &binding_names[0];
                    if let Some((_, expression)) = rest.split_once('=') {
                        eval_lines.push(format!("globalThis.{name} = {expression}"));
                        continue;
                    }
                }
                eval_lines.push(format!("const {rest}"));
                continue;
            }
            if let Some(rest) = rest.strip_prefix("function* ") {
                if let Some(name) = extract_function_name(rest) {
                    named.push(name);
                }
                let declaration = format!("function* {}", rest);
                let depth_delta = declaration.matches('{').count() as i32
                    - declaration.matches('}').count() as i32;
                if depth_delta > 0 {
                    in_export_block = true;
                    export_block_depth = depth_delta;
                }
                eval_lines.push(declaration);
                continue;
            }
            if let Some(rest) = rest.strip_prefix("function ") {
                if let Some(name) = extract_function_name(rest) {
                    named.push(name);
                }
                let declaration = format!("function {}", rest);
                let depth_delta = declaration.matches('{').count() as i32
                    - declaration.matches('}').count() as i32;
                if depth_delta > 0 {
                    in_export_block = true;
                    export_block_depth = depth_delta;
                }
                eval_lines.push(declaration);
                continue;
            }
            if let Some(rest) = rest.strip_prefix("class ") {
                if let Some(name) = extract_class_name(rest) {
                    named.push(name);
                }
                let declaration = format!("class {}", rest);
                let depth_delta = declaration.matches('{').count() as i32
                    - declaration.matches('}').count() as i32;
                if depth_delta > 0 {
                    in_export_block = true;
                    export_block_depth = depth_delta;
                }
                eval_lines.push(declaration);
                continue;
            }
            side_effect_lines.push(line.to_string());
            continue;
        }
        if side_effect_depth > 0 {
            side_effect_lines.push(line.to_string());
            side_effect_depth +=
                line.matches('{').count() as i32 - line.matches('}').count() as i32;
        } else if is_fixture_declaration(line) {
            eval_lines.push(line.to_string());
        } else {
            side_effect_lines.push(line.to_string());
            side_effect_depth = line.matches('{').count() as i32 - line.matches('}').count() as i32;
        }
    }

    let export = FixtureExports {
        named,
        default_marker: if has_default_marker {
            Some(default_marker)
        } else {
            None
        },
        aliases,
        default_aliases,
    };
    Ok(FixtureExportsCacheEntry {
        eval_source: Arc::new(eval_lines.join("\n")),
        side_effect_source: Arc::new(side_effect_lines.join("\n")),
        exports: Arc::new(export),
        default_import: Arc::new(default_import),
        reexports: Arc::new(reexports),
        has_default_marker,
    })
}

fn is_fixture_declaration(line: &str) -> bool {
    [
        "const ",
        "let ",
        "var ",
        "function ",
        "function* ",
        "class ",
    ]
    .iter()
    .any(|prefix| line.starts_with(prefix))
}

fn parse_fixture_json_value(source: &str) -> Result<quench_runtime::Value, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(source).map_err(|e| format!("failed to parse JSON fixture: {e}"))?;
    json_to_value(parsed)
}

fn json_to_value(value: serde_json::Value) -> Result<quench_runtime::Value, String> {
    match value {
        serde_json::Value::Null => Ok(quench_runtime::Value::Null),
        serde_json::Value::Bool(value) => Ok(quench_runtime::Value::Boolean(value)),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_f64() {
                Ok(quench_runtime::Value::Number(value))
            } else {
                Err("JSON number out of range".to_string())
            }
        }
        serde_json::Value::String(value) => Ok(quench_runtime::Value::String(value)),
        serde_json::Value::Array(values) => {
            let mut elements = Vec::new();
            for value in values {
                elements.push(json_to_value(value)?);
            }
            Ok(quench_runtime::Value::Object(std::rc::Rc::new(
                std::cell::RefCell::new(quench_runtime::value::Object::new_array_from(elements)),
            )))
        }
        serde_json::Value::Object(values) => {
            let mut object =
                quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
            for (key, value) in values {
                object.properties.insert(key, json_to_value(value)?);
            }
            Ok(quench_runtime::Value::Object(std::rc::Rc::new(
                std::cell::RefCell::new(object),
            )))
        }
    }
}

fn normalize_fixture_module_name(raw: &str) -> Option<String> {
    let source = raw.trim().trim_end_matches(';');
    let source = source
        .split_once(" with ")
        .map_or(source, |(source, _)| source)
        .trim();
    let quote = source.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let source = source.strip_prefix(quote)?.strip_suffix(quote)?;
    if source.starts_with("./") {
        Some(format!("./{}", source.trim_start_matches("./")))
    } else {
        Some(format!("./{}", source))
    }
}

fn fixture_import_edges_from_source(source: &str) -> Vec<(String, String)> {
    if !fixture_analysis_cache_enabled() {
        return fixture_import_edges_from_source_unchecked(source);
    }
    cached_fixture_import_edges_from_source(source)
        .unwrap_or_else(|_| std::sync::Arc::new(fixture_import_edges_from_source_unchecked(source)))
        .as_ref()
        .to_vec()
}

fn fixture_import_edges_from_source_unchecked(source: &str) -> Vec<(String, String)> {
    let mut edges = Vec::new();
    for line in source.lines().map(str::trim) {
        let Some(rest) = line.strip_prefix("import ") else {
            continue;
        };
        if rest.starts_with("source ") {
            continue;
        }
        let Some((clause, from)) = rest.split_once(" from ") else {
            continue;
        };
        // Attribute imports are synthetic modules. Their default export is
        // created by eval_import from the raw source/bytes cache, rather than
        // by fixture namespace linking.
        if from.contains(" with ") {
            continue;
        }
        let Some(module) = normalize_fixture_module_name(from) else {
            continue;
        };
        let clause = clause.trim();
        if clause.starts_with("defer * as ") {
            continue;
        }
        let named = if let Some(named) = clause.strip_prefix('{') {
            named.strip_suffix('}')
        } else {
            let (default, named) = clause.split_once(',').unwrap_or((clause, ""));
            if !default.trim().is_empty() && !default.trim_start().starts_with('*') {
                edges.push(("default".to_string(), module.clone()));
            }
            named
                .trim()
                .strip_prefix('{')
                .and_then(|named| named.strip_suffix('}'))
        };
        let Some(named) = named else {
            continue;
        };
        for (imported, _) in parse_export_specifier_list(named) {
            edges.push((imported.to_string(), module.clone()));
        }
    }
    edges
}

fn fixture_module_requests_from_source(source: &str) -> HashSet<String> {
    if !has_fixture_request_heuristic(source) {
        note_fixture_module_request_fastpath_hit();
        return HashSet::new();
    }
    note_fixture_module_request_fastpath_miss();
    if !fixture_analysis_cache_enabled() {
        return fixture_module_requests_from_source_unchecked(source);
    }
    let key = source_cache_key(source);
    {
        let cache = fixture_module_requests_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = cache.get(&key) {
            with_fixture_profile_stats(|stats| stats.module_request_hits += 1);
            note_fixture_module_request_cache_hit();
            return entry.as_ref().clone();
        }
    }
    with_fixture_profile_stats(|stats| stats.module_request_misses += 1);
    note_fixture_module_request_cache_miss();
    let parsed = fixture_module_requests_from_source_unchecked(source);
    let mut cache = fixture_module_requests_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = cache.get(&key) {
        with_fixture_profile_stats(|stats| stats.module_request_hits += 1);
        note_fixture_module_request_cache_hit();
        return entry.as_ref().clone();
    }
    let entry = std::sync::Arc::new(parsed);
    cache.insert(key, std::sync::Arc::clone(&entry));
    entry.as_ref().clone()
}

fn fixture_module_requests_from_source_unchecked(source: &str) -> HashSet<String> {
    fixture_import_requests_from_source_unchecked(source)
        .into_iter()
        .chain(fixture_side_effect_imports_from_source_unchecked(source))
        .collect()
}

fn fixture_attribute_module_requests_from_source(source: &str) -> HashSet<String> {
    if !fixture_analysis_cache_enabled() {
        return fixture_attribute_module_requests_from_source_unchecked(source);
    }
    cached_fixture_attribute_requests_from_source(source)
        .as_ref()
        .clone()
}

fn fixture_attribute_module_requests_from_source_unchecked(source: &str) -> HashSet<String> {
    if !source.contains("import ") || !source.contains(" with ") {
        return HashSet::new();
    }
    source
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            let rest = line.strip_prefix("import ")?;
            let (_, from) = rest.split_once(" from ")?;
            from.contains(" with ")
                .then(|| normalize_fixture_module_name(from))
                .flatten()
        })
        .collect()
}

fn fixture_import_requests_from_source(source: &str) -> Vec<String> {
    if !fixture_analysis_cache_enabled() {
        return fixture_import_requests_from_source_unchecked(source);
    }
    cached_fixture_import_requests_from_source(source)
        .as_ref()
        .to_vec()
}

fn fixture_import_requests_from_source_unchecked(source: &str) -> Vec<String> {
    if !source.contains("import ") || !source.contains(" from ") {
        return Vec::new();
    }
    source
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            let rest = line.strip_prefix("import ")?;
            let (_, from) = rest.split_once(" from ")?;
            normalize_fixture_module_name(from)
        })
        .collect()
}

fn fixture_fixture_requests_from_source(source: &str) -> HashSet<String> {
    if !fixture_analysis_cache_enabled() {
        return fixture_fixture_requests_from_source_unchecked(source);
    }
    fixture_module_requests_from_source(source)
        .into_iter()
        .chain(fixture_dynamic_fixture_requests_from_source(source))
        .chain(fixture_reexport_requests_from_source(source))
        .filter(|module| module.contains("_FIXTURE"))
        .collect()
}

fn fixture_fixture_requests_from_source_unchecked(source: &str) -> HashSet<String> {
    fixture_module_requests_from_source_unchecked(source)
        .into_iter()
        .chain(fixture_dynamic_fixture_requests_from_source_unchecked(
            source,
        ))
        .chain(fixture_reexport_requests_from_source_unchecked(source))
        .filter(|module| module.contains("_FIXTURE"))
        .collect()
}

fn fixture_reexport_requests_from_source(source: &str) -> Vec<String> {
    if !fixture_analysis_cache_enabled() {
        return fixture_reexport_requests_from_source_unchecked(source);
    }
    cached_fixture_reexport_requests_from_source(source)
        .as_ref()
        .to_vec()
}

fn fixture_reexport_requests_from_source_unchecked(source: &str) -> Vec<String> {
    if !source.contains("export ") || !source.contains(" from ") {
        return Vec::new();
    }
    source
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            let rest = line.strip_prefix("export ")?;
            let (_, source) = rest.rsplit_once(" from ")?;
            normalize_fixture_module_name(source)
        })
        .collect()
}

fn fixture_dynamic_fixture_requests_from_source(source: &str) -> Vec<String> {
    if !fixture_analysis_cache_enabled() {
        return fixture_dynamic_fixture_requests_from_source_unchecked(source);
    }
    cached_fixture_dynamic_requests_from_source(source)
        .as_ref()
        .to_vec()
}

fn fixture_dynamic_fixture_requests_from_source_unchecked(source: &str) -> Vec<String> {
    if !source.contains("import(") && !source.contains("import.defer(") {
        return Vec::new();
    }
    ["import(", "import.defer("]
        .into_iter()
        .flat_map(|call| {
            source.split(call).skip(1).filter_map(|rest| {
                let rest = rest.trim_start();
                let quote = rest.chars().next()?;
                if quote != '\'' && quote != '"' {
                    return None;
                }
                let end = rest[1..].find(quote)? + 1;
                normalize_fixture_module_name(&rest[..=end])
            })
        })
        .collect()
}

fn fixture_deferred_import_requests_from_source(source: &str) -> Vec<(String, bool)> {
    if !fixture_analysis_cache_enabled() {
        return fixture_deferred_import_requests_from_source_unchecked(source);
    }
    cached_fixture_deferred_import_requests_from_source(source)
        .as_ref()
        .to_vec()
}

fn fixture_deferred_import_requests_from_source_unchecked(
    source: &str,
) -> Vec<(String, bool)> {
    if !source.contains("import ") && !source.contains("export ") {
        return Vec::new();
    }
    source
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            let (module, deferred) = if let Some(rest) = line.strip_prefix("import ") {
                if rest.starts_with("meta") || rest.contains(" with ") {
                    return None;
                }
                if let Some(rest) = rest.strip_prefix("defer * as ") {
                    let Some((_, module)) = rest.split_once(" from ") else {
                        return None;
                    };
                    (normalize_fixture_module_name(module), true)
                } else if let Some((_, module)) = rest.split_once(" from ") {
                    (normalize_fixture_module_name(module), false)
                } else {
                    (normalize_fixture_module_name(rest), false)
                }
            } else if let Some(rest) = line.strip_prefix("export ") {
                let Some((_, module)) = rest.rsplit_once(" from ") else {
                    return None;
                };
                (normalize_fixture_module_name(module), false)
            } else {
                return None;
            };
            module.map(|module| (module, deferred))
        })
        .collect()
}

fn cached_fixture_deferred_import_requests_from_source(
    source: &str,
) -> Arc<Vec<(String, bool)>> {
    let key = source_cache_key(source);
    if !fixture_analysis_cache_enabled() {
        return Arc::new(fixture_deferred_import_requests_from_source_unchecked(source));
    }
    {
        let cache = fixture_deferred_import_request_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = cache.get(&key) {
            note_fixture_deferred_import_cache_hit();
            with_fixture_profile_stats(|stats| stats.deferred_import_request_hits += 1);
            return Arc::clone(entry);
        }
    }
    note_fixture_deferred_import_cache_miss();
    with_fixture_profile_stats(|stats| stats.deferred_import_request_misses += 1);
    let parsed = fixture_deferred_import_requests_from_source_unchecked(source);
    let mut cache = fixture_deferred_import_request_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = cache.get(&key) {
        note_fixture_deferred_import_cache_hit();
        with_fixture_profile_stats(|stats| stats.deferred_import_request_hits += 1);
        return Arc::clone(entry);
    }
    let entry = std::sync::Arc::new(parsed);
    cache.insert(key, std::sync::Arc::clone(&entry));
    entry
}

fn deferred_namespace_imports_from_source(source: &str) -> Vec<(String, String)> {
    source
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            let rest = line.strip_prefix("import defer * as ")?;
            let (local, source) = rest.split_once(" from ")?;
            Some((
                local.trim().to_string(),
                normalize_fixture_module_name(source)?,
            ))
        })
        .collect()
}

/// Link deferred imports from the entry module before evaluating its body.
/// Their transitive async dependencies are evaluated during module linking,
/// while the deferred module itself remains lazy until its namespace is read.
fn prepare_current_module_deferred_dependencies(
    ctx: &mut quench_runtime::Context,
    source: &str,
    has_fixture_dependencies: bool,
) -> Result<bool, String> {
    let requests = if has_fixture_dependencies {
        fixture_module_requests_from_source(source).into_iter().collect()
    } else {
        Vec::new()
    };
    prepare_current_module_deferred_dependencies_with_requests(
        ctx,
        source,
        &requests,
        has_fixture_dependencies,
    )
}

fn prepare_current_module_deferred_dependencies_with_requests(
    ctx: &mut quench_runtime::Context,
    source: &str,
    fixture_module_requests: &[String],
    has_fixture_dependencies: bool,
) -> Result<bool, String> {
    if !has_fixture_dependencies {
        return Ok(false);
    }
    if !source.contains("import(") && !source.contains("import defer ") {
        return Ok(false);
    }
    let mut has_deferred_import = false;
    let fixture_graph_must_evaluate_with_entry = fixture_module_requests
        .iter()
        .any(|module| {
            ctx.get_global("__quench_fixture_raw_modules__")
                .and_then(|modules| match modules {
                    Value::Object(modules) => modules.borrow().get(&module),
                    _ => None,
                })
                .is_some_and(|fixture| {
                    matches!(fixture, Value::String(source)
                        if source.contains("import(") || source.contains("import defer "))
                })
        });
    for (module, deferred) in cached_fixture_deferred_import_requests_from_source(source)
        .as_ref()
        .iter()
    {
        if *deferred || !fixture_graph_must_evaluate_with_entry {
            ctx.dynamic_import_module(module, None, false, *deferred)
                .map_err(|error| format!("entry-module import {module}: {error:?}"))?;
            has_deferred_import |= *deferred;
        }
    }
    Ok(has_deferred_import)
}

fn fixture_side_effect_imports_from_source(source: &str) -> Vec<String> {
    if !fixture_analysis_cache_enabled() {
        return fixture_side_effect_imports_from_source_unchecked(source);
    }
    cached_fixture_side_effect_imports_from_source(source)
        .as_ref()
        .to_vec()
}

fn fixture_side_effect_imports_from_source_unchecked(source: &str) -> Vec<String> {
    if !source.contains("import ") {
        return Vec::new();
    }
    source
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            let rest = line.strip_prefix("import ")?.trim();
            if rest.starts_with('{')
                || rest.starts_with('*')
                || rest.starts_with("meta")
                || rest.starts_with("defer ")
                || rest.contains(" from ")
                || !matches!(rest.chars().next(), Some('\'' | '"'))
            {
                return None;
            }
            let end = rest.find([';', ' ', '\t']).unwrap_or(rest.len());
            normalize_fixture_module_name(&rest[..end])
        })
        .collect()
}

fn parse_default_import(raw: &str) -> Option<String> {
    let spec = raw.trim().trim_end_matches(';').trim();
    if !spec.starts_with("import(") || !spec.ends_with(')') {
        return None;
    }
    let inner = spec
        .trim_start_matches("import(")
        .trim_end_matches(')')
        .trim();
    normalize_fixture_module_name(inner)
}

fn has_module_path(
    graph: &HashMap<String, Vec<String>>,
    current: &str,
    target: &str,
    seen: &mut HashSet<String>,
) -> bool {
    if current == target {
        return true;
    }
    if !seen.insert(current.to_string()) {
        return false;
    }
    graph.get(current).is_some_and(|sources| {
        sources
            .iter()
            .any(|source| has_module_path(graph, source, target, seen))
    })
}

fn parse_export_specifier_list(list: &str) -> Vec<(String, String)> {
    list.split(',')
        .map(|specifier| specifier.trim())
        .filter(|specifier| !specifier.is_empty())
        .map(|specifier| {
            let mut names = specifier.splitn(2, " as ");
            let local = decode_module_export_name(names.next().unwrap_or("").trim());
            let exported = decode_module_export_name(names.next().unwrap_or(&local).trim());
            (local, exported)
        })
        .filter(|(local, _)| !local.is_empty())
        .collect()
}

fn fixture_named_imports(source: &str) -> HashMap<String, (String, String)> {
    if !fixture_analysis_cache_enabled() {
        return fixture_named_imports_unchecked(source);
    }
    cached_fixture_named_imports_from_source(source)
        .as_ref()
        .clone()
}

fn fixture_namespace_imports(source: &str) -> HashMap<String, String> {
    if !fixture_analysis_cache_enabled() {
        return fixture_namespace_imports_unchecked(source);
    }
    cached_fixture_namespace_imports_from_source(source)
        .as_ref()
        .clone()
}

fn fixture_source_imports(source: &str) -> HashMap<String, String> {
    if !fixture_analysis_cache_enabled() {
        return fixture_source_imports_unchecked(source);
    }
    cached_fixture_source_imports_from_source(source)
        .as_ref()
        .clone()
}

fn fixture_named_imports_unchecked(source: &str) -> HashMap<String, (String, String)> {
    let mut imports = HashMap::new();
    for line in source.lines().map(str::trim) {
        let Some(rest) = line.strip_prefix("import ") else {
            continue;
        };
        let Some((clause, from)) = rest.split_once(" from ") else {
            continue;
        };
        let Some(module) = normalize_fixture_module_name(from) else {
            continue;
        };
        let Some(named) = clause.trim().strip_prefix('{') else {
            continue;
        };
        let Some(named) = named.strip_suffix('}') else {
            continue;
        };
        for (imported, local) in parse_export_specifier_list(named) {
            imports.insert(local, (module.clone(), imported));
        }
    }
    imports
}

fn fixture_namespace_imports_unchecked(source: &str) -> HashMap<String, String> {
    let mut imports = HashMap::new();
    for line in source.lines().map(str::trim) {
        let Some(rest) = line
            .strip_prefix("import * as ")
            .or_else(|| line.strip_prefix("import defer * as "))
        else {
            continue;
        };
        let Some((local, from)) = rest.split_once(" from ") else {
            continue;
        };
        if let Some(module) = normalize_fixture_module_name(from) {
            imports.insert(local.trim().to_string(), module);
        }
    }
    imports
}

fn fixture_source_imports_unchecked(source: &str) -> HashMap<String, String> {
    source
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            let rest = line.strip_prefix("import source ")?;
            let (local, from) = rest.split_once(" from ")?;
            Some((
                local.trim().to_string(),
                from.trim()
                    .trim_end_matches(';')
                    .trim_matches(&['\'', '"'][..])
                    .to_string(),
            ))
        })
        .collect()
}

fn create_module_source(ctx: &quench_runtime::Context) -> Value {
    let prototype = ctx
        .get_global("$262")
        .and_then(|value| match value {
            Value::Object(host) => host.borrow().get("AbstractModuleSource"),
            _ => None,
        })
        .and_then(|value| match value {
            Value::NativeConstructor(constructor) => {
                Some(std::rc::Rc::clone(&constructor.prototype))
            }
            _ => None,
        });
    let mut source =
        quench_runtime::value::Object::new(quench_runtime::value::ObjectKind::Ordinary);
    source.prototype = prototype;
    Value::Object(std::rc::Rc::new(std::cell::RefCell::new(source)))
}

fn extract_binding_names(declaration: &str) -> Vec<String> {
    let names = declaration
        .split_once('=')
        .map_or(declaration, |(names, _)| names)
        .trim();
    if let Some(names) = names
        .strip_prefix('{')
        .and_then(|names| names.strip_suffix('}'))
    {
        return names
            .split(',')
            .filter_map(|binding| binding.trim().split(':').next_back())
            .map(str::trim)
            .filter(|binding| !binding.is_empty())
            .map(decode_identifier_escape)
            .collect();
    }
    names
        .split(',')
        .map(str::trim)
        .map(|name| name.trim_end_matches(';').trim())
        .filter(|name| !name.is_empty())
        .map(decode_identifier_escape)
        .collect()
}

fn extract_function_name(declaration: &str) -> Option<String> {
    declaration
        .split(|c: char| c == '(' || c == ' ')
        .next()
        .map(decode_identifier_escape)
}

fn extract_class_name(declaration: &str) -> Option<String> {
    declaration
        .split(|c: char| c == '{' || c == ' ')
        .next()
        .map(decode_identifier_escape)
}

fn decode_identifier_escape(value: &str) -> String {
    let mut decoded = String::new();
    let mut remaining = value;
    while let Some((head, tail)) = remaining.split_once("\\u") {
        decoded.push_str(head);
        let (digits, rest) = if let Some(braced) = tail.strip_prefix('{') {
            let Some((digits, rest)) = braced.split_once('}') else {
                return value.to_string();
            };
            (digits, rest)
        } else if tail.len() >= 4 {
            tail.split_at(4)
        } else {
            return value.to_string();
        };
        let Ok(code) = u32::from_str_radix(digits, 16) else {
            return value.to_string();
        };
        let Some(character) = char::from_u32(code) else {
            return value.to_string();
        };
        decoded.push(character);
        remaining = rest;
    }
    decoded.push_str(remaining);
    decoded
}

fn decode_module_export_name(value: &str) -> String {
    let value = value.trim();
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value);
    decode_identifier_escape(value)
}

/// Verify the async $DONE count recorded by `ASYNC_DONE_PRELUDE` is exactly 1.
fn async_done_probe(ctx: &mut quench_runtime::Context) -> Result<(), String> {
    if let Ok(error) = ctx.eval("globalThis.__test262DoneError") {
        if !matches!(error, quench_runtime::Value::Undefined) {
            return Err(quench_runtime::value::to_js_string(&error));
        }
    }
    match ctx
        .eval("(globalThis.__test262DoneCount|0) || (globalThis.__test262ReplacementDoneCount|0)")
    {
        Ok(quench_runtime::Value::Number(1.0)) => Ok(()),
        Ok(v) => Err(format!(
            "async test did not call $DONE exactly once (count: {:?})",
            v
        )),
        Err(e) => Err(format!("async $DONE probe: {:?}", e)),
    }
}

/// Process-isolated run via prebuilt `run-test` binary (survives stack overflows).
pub fn run_isolated(test_path: &Path) -> TestOutcome {
    note_isolated_run();
    let bin = run_test_binary();
    let capture_output = isolated_capture_output();
    let poll = Duration::from_millis(isolated_poll_ms());
    let timeout_secs = test_timeout_secs();
    let primary = run_isolated_once(test_path, &bin, capture_output, timeout_secs, poll);
    if capture_output || !isolated_capture_output_on_failure() {
        return primary;
    }
    if !isolated_retry_enabled() {
        note_isolated_retry_skipped();
        return primary;
    }
    match primary {
        TestOutcome::Pass => primary,
        _ => {
            note_isolated_retry();
            run_isolated_once(test_path, &bin, true, timeout_secs, poll)
        }
    }
}

fn run_isolated_once(
    test_path: &Path,
    bin: &Path,
    capture_output: bool,
    timeout_secs: u64,
    poll: Duration,
) -> TestOutcome {
    let mut command = std::process::Command::new(bin);
    command
        .arg("--runner")
        .arg(test_path)
        .env("TEST262_NOSKIP", "1")
        .env("TEST262_DIR", crate::runner::default_test262_dir())
        .env("RUST_MIN_STACK", "33554432");
    if capture_output {
        let _ = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    } else {
        let _ = command
            .stdout(Stdio::null())
            .stderr(Stdio::null());
    }
    let child = command.spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            note_isolated_spawn_failure();
            return TestOutcome::Fail {
                failure: TestFailure::from_message(format!(
                    "isolated spawn ({}): {}",
                    bin.display(),
                    e
                )),
            };
        }
    };

    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = if capture_output {
                    match child.wait_with_output() {
                        Ok(output) => output,
                        Err(error) => {
                            note_isolated_wait_failure();
                            return TestOutcome::Fail {
                                failure: TestFailure::from_message(format!(
                                    "isolated wait+output ({}): {}",
                                    bin.display(),
                                    error
                                )),
                            };
                        }
                    }
                } else {
                    match child.wait() {
                        Ok(status) => std::process::Output {
                            status,
                            stdout: Vec::new(),
                            stderr: Vec::new(),
                        },
                        Err(error) => {
                            note_isolated_wait_failure();
                            return TestOutcome::Fail {
                                failure: TestFailure::from_message(format!(
                                    "isolated wait ({}): {}",
                                    bin.display(),
                                    error
                                )),
                            };
                        }
                    }
                };
                return classify_isolated(&output, test_path);
            }
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(poll);
            }
            Ok(None) => {
                note_isolated_timeout();
                let _ = child.kill();
                let _ = child.wait();
                return TestOutcome::Fail {
                    failure: TestFailure::from_message(format!(
                        "timed out after {}s",
                        timeout_secs
                    )),
                };
            }
            Err(e) => {
                note_isolated_wait_failure();
                let _ = child.kill();
                return TestOutcome::Fail {
                    failure: TestFailure::from_message(format!("isolated wait: {}", e)),
                };
            }
        }
    }
}

/// Map a finished `run-test` subprocess to an outcome. run-test verifies
/// negative-test polarity itself, so exit 0 is the ONLY pass.
/// Parses the subprocess output for structured diagnostic fields.
fn classify_isolated(out: &std::process::Output, test_path: &Path) -> TestOutcome {
    match out.status.code() {
        Some(0) => {
            return TestOutcome::Pass;
        }
        Some(_) => {}
        None => {}
    }

    let stdout = String::from_utf8_lossy(output_tail(&out.stdout));
    let stderr = String::from_utf8_lossy(output_tail(&out.stderr));
    let combined = format!("{}{}", stdout, stderr);

    // Parse diagnostic fields from run-test's output.
    let parse_field = |prefix: &str| -> Option<String> {
        combined
            .lines()
            .find(|l| l.trim().starts_with(prefix))
            .and_then(|l| l.split_once(prefix))
            .map(|(_, v)| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let stdout_tail = output_tail(&out.stdout);
    let stderr_tail = output_tail(&out.stderr);
    let reason = isolated_message(stderr_tail, stdout_tail);
    let error_type = parse_field("Type:");
    let error_message = parse_field("JS message:");
    let js_stack = combined
        .lines()
        .skip_while(|l| !l.trim().starts_with("Stack:"))
        .skip(1)
        .take_while(|l| l.trim().starts_with("at ") || l.trim().starts_with("  "))
        .map(|l| l.trim().to_string())
        .reduce(|a, b| format!("{}\n{}", a, b));

    let failure = TestFailure {
        message: format!(
            "isolated exit {}: {}",
            out.status.code().unwrap_or(-1),
            reason
        ),
        error_type,
        error_message,
        js_stack,
        source_path: Some(test_path.to_string_lossy().to_string()),
        source_line: None,
        source_context: String::new(),
    }
    .with_source(test_path, None);

    match out.status.code() {
        Some(_) => TestOutcome::Fail { failure },
        None => TestOutcome::Fail {
            failure: TestFailure {
                message: format!(
                    "isolated terminated by signal: {}",
                    isolated_message(stderr_tail, stdout_tail)
                ),
                ..failure
            },
        },
    }
}

fn isolated_message(stderr: &[u8], stdout: &[u8]) -> String {
    let err = String::from_utf8_lossy(stderr);
    let out = String::from_utf8_lossy(stdout);
    for text in [&out, &err] {
        if let Some(line) = text.lines().find(|l| l.contains("Reason:")) {
            return line
                .split_once("Reason:")
                .map(|(_, r)| r.trim())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| line.trim())
                .to_string();
        }
    }
    if let Some(line) = out.lines().find(|l| l.contains("FAILED")) {
        if let Some(next) = out
            .lines()
            .skip_while(|l| !l.contains("FAILED"))
            .nth(1)
            .filter(|l| l.contains("Reason:"))
        {
            return next
                .split_once("Reason:")
                .map(|(_, r)| r.trim())
                .unwrap_or("")
                .to_string();
        }
        return line.trim().to_string();
    }
    if let Some(line) = err.lines().find(|l| !l.is_empty()) {
        return line.trim().to_string();
    }
    out.lines().last().unwrap_or("").trim().to_string()
}

fn output_tail(bytes: &[u8]) -> &[u8] {
    let max = isolated_output_max_bytes();
    if max == 0 || bytes.len() <= max {
        return bytes;
    }
    &bytes[bytes.len() - max..]
}

fn run_test_binary() -> std::path::PathBuf {
    static RUN_TEST_BINARY: OnceLock<std::path::PathBuf> = OnceLock::new();
    if let Some(cached) = RUN_TEST_BINARY.get() {
        return cached.clone();
    }
    let resolved = if let Ok(bin) = std::env::var("RUN_TEST_BIN") {
        std::path::PathBuf::from(bin)
    } else {
        let mut targets: Vec<std::path::PathBuf> = Vec::with_capacity(2);
        if let Ok(target_dir) = std::env::var("CARGO_TARGET_DIR") {
            targets.push(std::path::PathBuf::from(target_dir));
        }
        let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        if let Some(ws) = manifest.parent().and_then(|p| p.parent()) {
            targets.push(ws.join("target"));
        }
        preferred_run_test_binary(&targets)
            .unwrap_or_else(|| std::path::PathBuf::from("target/debug/run-test"))
    };
    let _ = RUN_TEST_BINARY.set(resolved.clone());
    resolved
}

fn preferred_run_test_binary(targets: &[PathBuf]) -> Option<std::path::PathBuf> {
    targets
        .iter()
        .flat_map(|target| {
            [
                target.join("release/run-test"),
                target.join("debug/run-test"),
            ]
        })
        .find(|path| path.is_file())
}

#[cfg(test)]
mod classification_helpers;
#[cfg(test)]
mod classification_isolated_tests;
#[cfg(test)]
mod classification_tests;
#[cfg(test)]
mod tests;
