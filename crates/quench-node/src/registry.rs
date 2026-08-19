//! Canonical Node API surface table.
//!
//! Every Node global, every `node:` module, and every host function
//! is declared as data here. A single `install` function lowers
//! this table into a `VmContext`.
//!
//! Capability ids are stable `u16` values. The runtime's
//! `HostCapabilityKind::Custom(u16)` is the dispatch key. Id 0 is
//! reserved (sentinel).

use crate::envelope::NodeObject;

/// Stable capability id. Stays under `u16` to fit the runtime's
/// `HostCapabilityKind::Custom` representation.
pub type CapId = u16;

/// Canonical Node API surface entry. Each entry is one dispatchable
/// op on the host. The table is the only place that names them.
#[derive(Clone, Copy, Debug)]
pub struct NodeSpec {
    pub name: &'static str,
    pub cap: CapId,
}

impl NodeSpec {
    pub const fn new(name: &'static str, cap: CapId) -> Self {
        Self { name, cap }
    }
}

/// All Node host capabilities. Stable ids; do not reorder.
pub const SPEC_EVENTS_NEW: NodeSpec = NodeSpec::new("events:EventEmitter", 0x0100);
pub const SPEC_EVENTS_FROM: NodeSpec = NodeSpec::new("events:from", 0x0101);
pub const SPEC_EVENTS_ON: NodeSpec = NodeSpec::new("events:on", 0x0102);
pub const SPEC_EVENTS_EMIT: NodeSpec = NodeSpec::new("events:emit", 0x0103);

pub const SPEC_CONSOLE_LOG: NodeSpec = NodeSpec::new("console:log", 0x0200);
pub const SPEC_CONSOLE_INFO: NodeSpec = NodeSpec::new("console:info", 0x0201);
pub const SPEC_CONSOLE_WARN: NodeSpec = NodeSpec::new("console:warn", 0x0202);
pub const SPEC_CONSOLE_ERROR: NodeSpec = NodeSpec::new("console:error", 0x0203);
pub const SPEC_CONSOLE_DEBUG: NodeSpec = NodeSpec::new("console:debug", 0x0204);
pub const SPEC_CONSOLE_TRACE: NodeSpec = NodeSpec::new("console:trace", 0x0205);

pub const SPEC_UTIL_FORMAT: NodeSpec = NodeSpec::new("util:format", 0x0300);
pub const SPEC_UTIL_INSPECT: NodeSpec = NodeSpec::new("util:inspect", 0x0301);
pub const SPEC_UTIL_TYPES: NodeSpec = NodeSpec::new("util:types", 0x0302);
pub const SPEC_UTIL_GETCALLSITES: NodeSpec = NodeSpec::new("util:getCallSites", 0x0303);
pub const SPEC_UTIL_IS: NodeSpec = NodeSpec::new("util:is", 0x0303);
pub const SPEC_UTIL_INHERITS: NodeSpec = NodeSpec::new("util:inherits", 0x0304);

pub const SPEC_PATH_JOIN: NodeSpec = NodeSpec::new("path:join", 0x0400);
pub const SPEC_PATH_RESOLVE: NodeSpec = NodeSpec::new("path:resolve", 0x0401);
pub const SPEC_PATH_NORMALIZE: NodeSpec = NodeSpec::new("path:normalize", 0x0402);
pub const SPEC_PATH_DIRNAME: NodeSpec = NodeSpec::new("path:dirname", 0x0403);
pub const SPEC_PATH_BASENAME: NodeSpec = NodeSpec::new("path:basename", 0x0404);
pub const SPEC_PATH_EXTNAME: NodeSpec = NodeSpec::new("path:extname", 0x0405);
pub const SPEC_PATH_ISABSOLUTE: NodeSpec = NodeSpec::new("path:isAbsolute", 0x0406);
pub const SPEC_PATH_RELATIVE: NodeSpec = NodeSpec::new("path:relative", 0x0409);
pub const SPEC_PATH_SEP: NodeSpec = NodeSpec::new("path:sep", 0x0407);
pub const SPEC_PATH_DELIM: NodeSpec = NodeSpec::new("path:delimiter", 0x0408);

pub const SPEC_URL_PARSE: NodeSpec = NodeSpec::new("url:parse", 0x0500);
pub const SPEC_URL_FORMAT: NodeSpec = NodeSpec::new("url:format", 0x0501);
pub const SPEC_URL_RESOLVE: NodeSpec = NodeSpec::new("url:resolve", 0x0502);
pub const SPEC_URL_NEW: NodeSpec = NodeSpec::new("url:URL", 0x0503);
pub const SPEC_URL_SEARCHPARAMS_NEW: NodeSpec = NodeSpec::new("url:URLSearchParams", 0x0504);

pub const SPEC_QS_PARSE: NodeSpec = NodeSpec::new("querystring:parse", 0x0600);
pub const SPEC_QS_STRINGIFY: NodeSpec = NodeSpec::new("querystring:stringify", 0x0601);
pub const SPEC_QS_ESCAPE: NodeSpec = NodeSpec::new("querystring:escape", 0x0602);
pub const SPEC_QS_UNESCAPE: NodeSpec = NodeSpec::new("querystring:unescape", 0x0603);

pub const SPEC_TIMERS_SETTIMEOUT: NodeSpec = NodeSpec::new("timers:setTimeout", 0x0700);
pub const SPEC_TIMERS_CLEARTIMEOUT: NodeSpec = NodeSpec::new("timers:clearTimeout", 0x0701);
pub const SPEC_TIMERS_SETINTERVAL: NodeSpec = NodeSpec::new("timers:setInterval", 0x0702);
pub const SPEC_TIMERS_CLEARINTERVAL: NodeSpec = NodeSpec::new("timers:clearInterval", 0x0703);
pub const SPEC_TIMERS_SETIMMEDIATE: NodeSpec = NodeSpec::new("timers:setImmediate", 0x0704);
pub const SPEC_TIMERS_CLEARIMMEDIATE: NodeSpec = NodeSpec::new("timers:clearImmediate", 0x0705);
pub const SPEC_TIMERS_TICK: NodeSpec = NodeSpec::new("timers:tick", 0x0706);
pub const SPEC_TIMERS_UNREF: NodeSpec = NodeSpec::new("timers:unref", 0x0708);
pub const SPEC_TIMERS_REF: NodeSpec = NodeSpec::new("timers:ref", 0x0709);
pub const SPEC_TIMERS_HASREF: NodeSpec = NodeSpec::new("timers:hasRef", 0x070A);
pub const SPEC_TIMERS_REFRESH: NodeSpec = NodeSpec::new("timers:refresh", 0x070B);
pub const SPEC_RUN_LOOP: NodeSpec = NodeSpec::new("__quench_run_loop__", 0x070C);
pub const SPEC_RUN_EXIT: NodeSpec = NodeSpec::new("__quench_run_exit__", 0x070D);
pub const SPEC_INTERNAL_UTIL_SLEEP: NodeSpec = NodeSpec::new("internal/util:sleep", 0x070E);
pub const SPEC_TIMERS_CLOSE: NodeSpec = NodeSpec::new("timers:close", 0x070F);

pub const SPEC_BUFFER_FROM: NodeSpec = NodeSpec::new("buffer:from", 0x0800);
pub const SPEC_BUFFER_ALLOC: NodeSpec = NodeSpec::new("buffer:alloc", 0x0801);
pub const SPEC_BUFFER_BYTELENGTH: NodeSpec = NodeSpec::new("buffer:byteLength", 0x0802);
pub const SPEC_BUFFER_ISBUFFER: NodeSpec = NodeSpec::new("buffer:isBuffer", 0x0803);
pub const SPEC_BUFFER_CONCAT: NodeSpec = NodeSpec::new("buffer:concat", 0x0804);
pub const SPEC_BUFFER_NEW: NodeSpec = NodeSpec::new("buffer:Buffer", 0x0805);
pub const SPEC_BUFFER_ATOB: NodeSpec = NodeSpec::new("buffer:atob", 0x0806);
pub const SPEC_BUFFER_BTOA: NodeSpec = NodeSpec::new("buffer:btoa", 0x0807);

pub const SPEC_TTY_ISATTY: NodeSpec = NodeSpec::new("tty:isatty", 0x0900);

pub const SPEC_PROCESS_GET: NodeSpec = NodeSpec::new("process:get", 0x0A00);
pub const SPEC_PROCESS_EXIT: NodeSpec = NodeSpec::new("process:exit", 0x0A01);
pub const SPEC_PROCESS_CWD: NodeSpec = NodeSpec::new("process:cwd", 0x0A02);
pub const SPEC_PROCESS_CHDIR: NodeSpec = NodeSpec::new("process:chdir", 0x0A03);
pub const SPEC_PROCESS_NEXT_TICK: NodeSpec = NodeSpec::new("process:nextTick", 0x0A04);
pub const SPEC_PROCESS_HRTIME: NodeSpec = NodeSpec::new("process:hrtime", 0x0A05);
pub const SPEC_PROCESS_UMASK: NodeSpec = NodeSpec::new("process:umask", 0x0A06);
pub const SPEC_PROCESS_ON: NodeSpec = NodeSpec::new("process:on", 0x0A07);
pub const SPEC_PROCESS_ONCE: NodeSpec = NodeSpec::new("process:once", 0x0A08);

pub const SPEC_OS_PLATFORM: NodeSpec = NodeSpec::new("os:platform", 0x0B00);
pub const SPEC_OS_ARCH: NodeSpec = NodeSpec::new("os:arch", 0x0B01);
pub const SPEC_OS_HOSTNAME: NodeSpec = NodeSpec::new("os:hostname", 0x0B02);
pub const SPEC_OS_TYPE: NodeSpec = NodeSpec::new("os:type", 0x0B03);
pub const SPEC_OS_RELEASE: NodeSpec = NodeSpec::new("os:release", 0x0B04);
pub const SPEC_OS_CPUS: NodeSpec = NodeSpec::new("os:cpus", 0x0B05);
pub const SPEC_OS_TMPDIR: NodeSpec = NodeSpec::new("os:tmpdir", 0x0B06);
pub const SPEC_OS_HOMEDIR: NodeSpec = NodeSpec::new("os:homedir", 0x0B07);
pub const SPEC_OS_EOL: NodeSpec = NodeSpec::new("os:EOL", 0x0B08);
pub const SPEC_OS_UPTIME: NodeSpec = NodeSpec::new("os:uptime", 0x0B09);
pub const SPEC_OS_FREEMEM: NodeSpec = NodeSpec::new("os:freemem", 0x0B0A);
pub const SPEC_OS_TOTALMEM: NodeSpec = NodeSpec::new("os:totalmem", 0x0B0B);
pub const SPEC_OS_LOADAVG: NodeSpec = NodeSpec::new("os:loadavg", 0x0B0C);
pub const SPEC_OS_NETWORKINTERFACES: NodeSpec = NodeSpec::new("os:networkInterfaces", 0x0B0D);

pub const SPEC_STREAM_READABLE: NodeSpec = NodeSpec::new("stream:Readable", 0x0C00);
pub const SPEC_STREAM_WRITABLE: NodeSpec = NodeSpec::new("stream:Writable", 0x0C01);
pub const SPEC_STREAM_DUPLEX: NodeSpec = NodeSpec::new("stream:Duplex", 0x0C02);
pub const SPEC_STREAM_TRANSFORM: NodeSpec = NodeSpec::new("stream:Transform", 0x0C03);
pub const SPEC_STREAM_PIPELINE: NodeSpec = NodeSpec::new("stream:pipeline", 0x0C04);
pub const SPEC_STREAM_FINISHED: NodeSpec = NodeSpec::new("stream:finished", 0x0C05);

pub const SPEC_STRING_DECODER: NodeSpec = NodeSpec::new("string_decoder:StringDecoder", 0x0D00);

pub const SPEC_DNS_LOOKUP: NodeSpec = NodeSpec::new("dns:lookup", 0x0E00);
pub const SPEC_DNS_RESOLVE4: NodeSpec = NodeSpec::new("dns:resolve4", 0x0E01);

pub const SPEC_HTTP_REQUEST: NodeSpec = NodeSpec::new("http:request", 0x0F00);
pub const SPEC_HTTP_GET: NodeSpec = NodeSpec::new("http:get", 0x0F01);
pub const SPEC_HTTP_SERVER: NodeSpec = NodeSpec::new("http:createServer", 0x0F02);

pub const SPEC_NET_CONNECT: NodeSpec = NodeSpec::new("net:connect", 0x1000);
pub const SPEC_NET_SERVER: NodeSpec = NodeSpec::new("net:createServer", 0x1001);
pub const SPEC_NET_ISIP: NodeSpec = NodeSpec::new("net:isIP", 0x1002);
pub const SPEC_NET_ISIPV4: NodeSpec = NodeSpec::new("net:isIPv4", 0x1003);
pub const SPEC_NET_ISIPV6: NodeSpec = NodeSpec::new("net:isIPv6", 0x1004);
pub const SPEC_NET_GET_ASF_TIMEOUT: NodeSpec =
    NodeSpec::new("net:getDefaultAutoSelectFamilyAttemptTimeout", 0x1005);
pub const SPEC_NET_SET_ASF_TIMEOUT: NodeSpec =
    NodeSpec::new("net:setDefaultAutoSelectFamilyAttemptTimeout", 0x1006);

pub const SPEC_FS_READFILE: NodeSpec = NodeSpec::new("fs:readFile", 0x1100);
pub const SPEC_FS_WRITEFILE: NodeSpec = NodeSpec::new("fs:writeFile", 0x1101);
pub const SPEC_FS_STAT: NodeSpec = NodeSpec::new("fs:stat", 0x1102);
pub const SPEC_FS_READDIR: NodeSpec = NodeSpec::new("fs:readdir", 0x1103);
pub const SPEC_FS_EXISTS: NodeSpec = NodeSpec::new("fs:exists", 0x1104);
pub const SPEC_FS_MKDIR: NodeSpec = NodeSpec::new("fs:mkdir", 0x1105);
pub const SPEC_FS_UNLINK: NodeSpec = NodeSpec::new("fs:unlink", 0x1106);
pub const SPEC_FS_READFILESYNC: NodeSpec = NodeSpec::new("fs:readFileSync", 0x1107);
pub const SPEC_FS_WRITEFILESYNC: NodeSpec = NodeSpec::new("fs:writeFileSync", 0x1108);
pub const SPEC_FS_STATSYNC: NodeSpec = NodeSpec::new("fs:statSync", 0x1109);
pub const SPEC_FS_READDIRSYNC: NodeSpec = NodeSpec::new("fs:readdirSync", 0x110A);
pub const SPEC_FS_EXISTSSYNC: NodeSpec = NodeSpec::new("fs:existsSync", 0x110B);
pub const SPEC_FS_REALSYNC: NodeSpec = NodeSpec::new("fs:realpathSync", 0x110C);

pub const SPEC_REQUIRE: NodeSpec = NodeSpec::new("require", 0x1200);
pub const SPEC_READLINE: NodeSpec = NodeSpec::new("readline:createInterface", 0x1300);
pub const SPEC_CJS_WRAP: NodeSpec = NodeSpec::new("__quench_cjs_wrap__", 0x1d00);
pub const SPEC_CP_SPAWNSYNC: NodeSpec = NodeSpec::new("child_process:spawnSync", 0x1e00);
pub const SPEC_CP_EXECSYNC: NodeSpec = NodeSpec::new("child_process:execSync", 0x1e01);
pub const SPEC_CP_EXEC: NodeSpec = NodeSpec::new("child_process:exec", 0x1e02);
pub const SPEC_CP_SPAWN: NodeSpec = NodeSpec::new("child_process:spawn", 0x1e03);
pub const SPEC_URL_PATH_TO_FILE_URL: NodeSpec = NodeSpec::new("url:pathToFileURL", 0x0505);
pub const SPEC_STRUCTURED_CLONE: NodeSpec = NodeSpec::new("structuredClone", 0x1f00);
pub const SPEC_FETCH: NodeSpec = NodeSpec::new("fetch", 0x1f01);
pub const SPEC_ABORT_CONTROLLER: NodeSpec = NodeSpec::new("AbortController", 0x1f02);
pub const SPEC_ABORT_SIGNAL: NodeSpec = NodeSpec::new("AbortSignal", 0x1f03);

pub const SPEC_ASSERT_OK: NodeSpec = NodeSpec::new("assert:ok", 0x1400);
pub const SPEC_ASSERT_STRICT_EQUAL: NodeSpec = NodeSpec::new("assert:strictEqual", 0x1401);
pub const SPEC_ASSERT_NOT_STRICT_EQUAL: NodeSpec = NodeSpec::new("assert:notStrictEqual", 0x1402);
pub const SPEC_ASSERT_EQUAL: NodeSpec = NodeSpec::new("assert:equal", 0x1403);
pub const SPEC_ASSERT_NOT_EQUAL: NodeSpec = NodeSpec::new("assert:notEqual", 0x1404);
pub const SPEC_ASSERT_DEEP_STRICT_EQUAL: NodeSpec = NodeSpec::new("assert:deepStrictEqual", 0x1405);
pub const SPEC_ASSERT_NOT_DEEP_STRICT_EQUAL: NodeSpec =
    NodeSpec::new("assert:notDeepStrictEqual", 0x1406);
pub const SPEC_ASSERT_THROWS: NodeSpec = NodeSpec::new("assert:throws", 0x1407);
pub const SPEC_ASSERT_DOES_NOT_THROW: NodeSpec = NodeSpec::new("assert:doesNotThrow", 0x1408);
pub const SPEC_ASSERT_FAIL: NodeSpec = NodeSpec::new("assert:fail", 0x1409);
pub const SPEC_ASSERT_IF_ERROR: NodeSpec = NodeSpec::new("assert:ifError", 0x140A);
pub const SPEC_ASSERT_MATCH: NodeSpec = NodeSpec::new("assert:match", 0x140B);
pub const SPEC_ASSERT_DOES_NOT_MATCH: NodeSpec = NodeSpec::new("assert:doesNotMatch", 0x140C);

/// Symbolic id for a Node host object stored in a `Value::Object`.
/// The runtime does not interpret this; the host uses it to map
/// `Value::Object` back to the Rust envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeSymbol {
    EventEmitter,
    Stream,
    Buffer,
    Timer,
    URL,
    URLSearchParams,
    Server,
    Socket,
    Process,
    Stats,
    StreamReadable,
    StreamWritable,
    StreamDuplex,
    StreamTransform,
    StringDecoder,
    FsWatcher,
    ChildProcess,
}

/// A bound Node host object: the Rust envelope + its `Value`.
pub struct BoundNode<T: 'static> {
    pub object: NodeObject<T>,
}

impl<T: 'static + crate::envelope::NodeAny> BoundNode<T> {
    pub fn value(&self) -> quench_runtime::value::Value {
        self.object.value()
    }
}

/// Canonical namespace wiring. Returns the `(name, value)` pairs
/// the host installs into the `VmContext` via
/// `with_host_value`. Single source of truth for the global table.
pub fn namespace_bindings(argv: &[String]) -> Vec<(String, quench_runtime::value::Value)> {
    let mut out = Vec::new();
    push_bindings(&mut out, argv);
    out.push(timers_binding(
        "setTimeout",
        crate::registry::SPEC_TIMERS_SETTIMEOUT,
    ));
    out.push(timers_binding(
        "clearTimeout",
        crate::registry::SPEC_TIMERS_CLEARTIMEOUT,
    ));
    out.push(timers_binding(
        "setInterval",
        crate::registry::SPEC_TIMERS_SETINTERVAL,
    ));
    out.push(timers_binding(
        "clearInterval",
        crate::registry::SPEC_TIMERS_CLEARINTERVAL,
    ));
    out.push(timers_binding(
        "setImmediate",
        crate::registry::SPEC_TIMERS_SETIMMEDIATE,
    ));
    out.push(timers_binding(
        "clearImmediate",
        crate::registry::SPEC_TIMERS_CLEARIMMEDIATE,
    ));
    out.push((
        "queueMicrotask".to_string(),
        crate::host::capability(crate::registry::NodeSpec::new("queueMicrotask", 0x0707)),
    ));
    out.push((
        "require".to_string(),
        crate::host::capability(crate::registry::NodeSpec::new("require", 0x1200)),
    ));
    out.push((
        "__quench_cjs_wrap__".to_string(),
        crate::host::capability(crate::registry::SPEC_CJS_WRAP),
    ));
    out.push((
        "__quench_run_loop__".to_string(),
        crate::host::capability(crate::registry::SPEC_RUN_LOOP),
    ));
    out.push((
        "__quench_run_exit__".to_string(),
        crate::host::capability(crate::registry::SPEC_RUN_EXIT),
    ));
    out.push((
        "__quench_uncaught__".to_string(),
        crate::host::capability(crate::registry::NodeSpec::new(
            "__quench_uncaught__",
            0x0117,
        )),
    ));
    out.push((
        "structuredClone".to_string(),
        crate::host::capability(crate::registry::SPEC_STRUCTURED_CLONE),
    ));
    out.push((
        "fetch".to_string(),
        crate::host::capability(crate::registry::SPEC_FETCH),
    ));
    out.push((
        "AbortController".to_string(),
        crate::host::capability(crate::registry::SPEC_ABORT_CONTROLLER),
    ));
    out.push((
        "AbortSignal".to_string(),
        crate::host::capability(crate::registry::SPEC_ABORT_SIGNAL),
    ));
    out.push((
        "EventTarget".to_string(),
        crate::host::capability(crate::registry::NodeSpec::new("events:EventTarget", 0x0116)),
    ));
    out.push((
        "atob".to_string(),
        crate::host::capability(crate::registry::SPEC_BUFFER_ATOB),
    ));
    out.push((
        "btoa".to_string(),
        crate::host::capability(crate::registry::SPEC_BUFFER_BTOA),
    ));
    out.push((
        "global".to_string(),
        crate::host::namespace_object_from_pairs(vec![]),
    ));
    out
}

fn push_bindings(out: &mut Vec<(String, quench_runtime::value::Value)>, argv: &[String]) {
    out.push((
        "console".to_string(),
        crate::modules::console::build_value(),
    ));
    out.push(("process".to_string(), crate::modules::process::build(argv)));
    out.push((
        "Buffer".to_string(),
        crate::host::namespace_object_from_pairs(crate::modules::buffer::build()),
    ));
}

fn timers_binding(
    name: &'static str,
    spec: crate::registry::NodeSpec,
) -> (String, quench_runtime::value::Value) {
    (name.to_string(), crate::host::capability(spec))
}
