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
pub const SPEC_UTIL_STRIP_VT: NodeSpec = NodeSpec::new("util:stripVTControlCharacters", 0x0305);
pub const SPEC_UTIL_FORMAT_WITH_OPTIONS: NodeSpec = NodeSpec::new("util:formatWithOptions", 0x0306);
pub const SPEC_UTIL_STYLE_TEXT: NodeSpec = NodeSpec::new("util:styleText", 0x0307);
pub const SPEC_UTIL_IS_DEEP_STRICT_EQUAL: NodeSpec =
    NodeSpec::new("util:isDeepStrictEqual", 0x0308);
pub const SPEC_TEXT_DECODER_NEW: NodeSpec = NodeSpec::new("TextDecoder:new", 0x0809);
pub const SPEC_TEXT_DECODER_DECODE: NodeSpec = NodeSpec::new("TextDecoder:decode", 0x080A);
pub const SPEC_TEXT_ENCODER_NEW: NodeSpec = NodeSpec::new("TextEncoder:new", 0x084C);
pub const SPEC_TEXT_ENCODER_ENCODE: NodeSpec = NodeSpec::new("TextEncoder:encode", 0x084D);
pub const SPEC_TEXT_ENCODER_ENCODE_INTO: NodeSpec = NodeSpec::new("TextEncoder:encodeInto", 0x084E);
pub const SPEC_TEST: NodeSpec = NodeSpec::new("test:test", 0x1b00);
pub const SPEC_TEST_SKIP: NodeSpec = NodeSpec::new("test:skip", 0x1b01);

pub const SPEC_PATH_JOIN: NodeSpec = NodeSpec::new("path:join", 0x0400);
pub const SPEC_PATH_RESOLVE: NodeSpec = NodeSpec::new("path:resolve", 0x0401);
pub const SPEC_PATH_NORMALIZE: NodeSpec = NodeSpec::new("path:normalize", 0x0402);
pub const SPEC_PATH_DIRNAME: NodeSpec = NodeSpec::new("path:dirname", 0x0403);
pub const SPEC_PATH_BASENAME: NodeSpec = NodeSpec::new("path:basename", 0x0404);
pub const SPEC_PATH_EXTNAME: NodeSpec = NodeSpec::new("path:extname", 0x0405);
pub const SPEC_PATH_ISABSOLUTE: NodeSpec = NodeSpec::new("path:isAbsolute", 0x0406);
pub const SPEC_PATH_RELATIVE: NodeSpec = NodeSpec::new("path:relative", 0x0409);
pub const SPEC_PATH_PARSE: NodeSpec = NodeSpec::new("path:parse", 0x040A);
pub const SPEC_PATH_FORMAT: NodeSpec = NodeSpec::new("path:format", 0x040B);
pub const SPEC_PATH_TO_NAMESPACED: NodeSpec = NodeSpec::new("path:toNamespacedPath", 0x040C);
pub const SPEC_PATH_MATCHES_GLOB: NodeSpec = NodeSpec::new("path:matchesGlob", 0x040D);

pub const SPEC_PATH_WIN32_JOIN: NodeSpec = NodeSpec::new("path.win32:join", 0x0410);
pub const SPEC_PATH_WIN32_RESOLVE: NodeSpec = NodeSpec::new("path.win32:resolve", 0x0411);
pub const SPEC_PATH_WIN32_NORMALIZE: NodeSpec = NodeSpec::new("path.win32:normalize", 0x0412);
pub const SPEC_PATH_WIN32_DIRNAME: NodeSpec = NodeSpec::new("path.win32:dirname", 0x0413);
pub const SPEC_PATH_WIN32_BASENAME: NodeSpec = NodeSpec::new("path.win32:basename", 0x0414);
pub const SPEC_PATH_WIN32_EXTNAME: NodeSpec = NodeSpec::new("path.win32:extname", 0x0415);
pub const SPEC_PATH_WIN32_ISABSOLUTE: NodeSpec = NodeSpec::new("path.win32:isAbsolute", 0x0416);
pub const SPEC_PATH_WIN32_RELATIVE: NodeSpec = NodeSpec::new("path.win32:relative", 0x0417);
pub const SPEC_PATH_WIN32_PARSE: NodeSpec = NodeSpec::new("path.win32:parse", 0x0418);
pub const SPEC_PATH_WIN32_FORMAT: NodeSpec = NodeSpec::new("path.win32:format", 0x0419);
pub const SPEC_PATH_WIN32_TO_NAMESPACED: NodeSpec =
    NodeSpec::new("path.win32:toNamespacedPath", 0x041A);
pub const SPEC_PATH_WIN32_MATCHES_GLOB: NodeSpec = NodeSpec::new("path.win32:matchesGlob", 0x041B);

pub const SPEC_URL_PARSE: NodeSpec = NodeSpec::new("url:parse", 0x0500);
pub const SPEC_URL_FORMAT: NodeSpec = NodeSpec::new("url:format", 0x0501);
pub const SPEC_URL_RESOLVE: NodeSpec = NodeSpec::new("url:resolve", 0x0502);
pub const SPEC_URL_NEW: NodeSpec = NodeSpec::new("url:URL", 0x0503);
pub const SPEC_URL_SEARCHPARAMS_NEW: NodeSpec = NodeSpec::new("url:URLSearchParams", 0x0504);

pub const SPEC_QS_PARSE: NodeSpec = NodeSpec::new("querystring:parse", 0x0600);
pub const SPEC_QS_STRINGIFY: NodeSpec = NodeSpec::new("querystring:stringify", 0x0601);
pub const SPEC_QS_ESCAPE: NodeSpec = NodeSpec::new("querystring:escape", 0x0602);
pub const SPEC_QS_UNESCAPE: NodeSpec = NodeSpec::new("querystring:unescape", 0x0603);
pub const SPEC_QS_UNESCAPE_BUFFER: NodeSpec = NodeSpec::new("querystring:unescapeBuffer", 0x0604);

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
pub const SPEC_BUFFER_TOSTRING: NodeSpec = NodeSpec::new("buffer:toString", 0x0808);
pub const SPEC_BUFFER_ALLOC_UNSAFE: NodeSpec = NodeSpec::new("buffer:allocUnsafe", 0x080B);
pub const SPEC_BUFFER_ALLOC_UNSAFE_SLOW: NodeSpec = NodeSpec::new("buffer:allocUnsafeSlow", 0x080C);
pub const SPEC_BUFFER_ISENCODING: NodeSpec = NodeSpec::new("buffer:isEncoding", 0x080D);
pub const SPEC_BUFFER_ISUTF8: NodeSpec = NodeSpec::new("buffer:isUtf8", 0x080E);
pub const SPEC_BUFFER_ISASCII: NodeSpec = NodeSpec::new("buffer:isAscii", 0x080F);
pub const SPEC_BUFFER_COMPARE_STATIC: NodeSpec = NodeSpec::new("buffer:compare", 0x0810);
pub const SPEC_BUFFER_EQUALS: NodeSpec = NodeSpec::new("buffer.prototype:equals", 0x0811);
pub const SPEC_BUFFER_COMPARE: NodeSpec = NodeSpec::new("buffer.prototype:compare", 0x0812);
pub const SPEC_BUFFER_COPY: NodeSpec = NodeSpec::new("buffer.prototype:copy", 0x0813);
pub const SPEC_BUFFER_FILL: NodeSpec = NodeSpec::new("buffer.prototype:fill", 0x0814);
pub const SPEC_BUFFER_SLICE: NodeSpec = NodeSpec::new("buffer.prototype:slice", 0x0815);
pub const SPEC_BUFFER_SWAP16: NodeSpec = NodeSpec::new("buffer.prototype:swap16", 0x0816);
pub const SPEC_BUFFER_SWAP32: NodeSpec = NodeSpec::new("buffer.prototype:swap32", 0x0817);
pub const SPEC_BUFFER_SWAP64: NodeSpec = NodeSpec::new("buffer.prototype:swap64", 0x0818);
pub const SPEC_BUFFER_TOJSON: NodeSpec = NodeSpec::new("buffer.prototype:toJSON", 0x0819);
pub const SPEC_BUFFER_INDEX_OF: NodeSpec = NodeSpec::new("buffer.prototype:indexOf", 0x081A);
pub const SPEC_BUFFER_LAST_INDEX_OF: NodeSpec =
    NodeSpec::new("buffer.prototype:lastIndexOf", 0x081B);
pub const SPEC_BUFFER_INCLUDES: NodeSpec = NodeSpec::new("buffer.prototype:includes", 0x081C);
pub const SPEC_BUFFER_WRITE: NodeSpec = NodeSpec::new("buffer.prototype:write", 0x081D);
pub const SPEC_BUFFER_INSPECT: NodeSpec = NodeSpec::new("buffer.prototype:inspect", 0x081E);
pub const SPEC_BUFFER_COPY_BYTES_FROM: NodeSpec = NodeSpec::new("buffer:copyBytesFrom", 0x081F);

macro_rules! buffer_num_specs {
    ($(($name:ident, $id:expr)),* $(,)?) => {
        $(pub const $name: NodeSpec = NodeSpec::new(stringify!($name), $id);)*
    };
}

buffer_num_specs! {
    (SPEC_BUF_READ_UINT8, 0x0820), (SPEC_BUF_WRITE_UINT8, 0x0821),
    (SPEC_BUF_READ_UINT16_LE, 0x0822), (SPEC_BUF_WRITE_UINT16_LE, 0x0823),
    (SPEC_BUF_READ_UINT16_BE, 0x0824), (SPEC_BUF_WRITE_UINT16_BE, 0x0825),
    (SPEC_BUF_READ_UINT32_LE, 0x0826), (SPEC_BUF_WRITE_UINT32_LE, 0x0827),
    (SPEC_BUF_READ_UINT32_BE, 0x0828), (SPEC_BUF_WRITE_UINT32_BE, 0x0829),
    (SPEC_BUF_READ_INT8, 0x082A), (SPEC_BUF_WRITE_INT8, 0x082B),
    (SPEC_BUF_READ_INT16_LE, 0x082C), (SPEC_BUF_WRITE_INT16_LE, 0x082D),
    (SPEC_BUF_READ_INT16_BE, 0x082E), (SPEC_BUF_WRITE_INT16_BE, 0x082F),
    (SPEC_BUF_READ_INT32_LE, 0x0830), (SPEC_BUF_WRITE_INT32_LE, 0x0831),
    (SPEC_BUF_READ_INT32_BE, 0x0832), (SPEC_BUF_WRITE_INT32_BE, 0x0833),
    (SPEC_BUF_READ_FLOAT_LE, 0x0834), (SPEC_BUF_WRITE_FLOAT_LE, 0x0835),
    (SPEC_BUF_READ_FLOAT_BE, 0x0836), (SPEC_BUF_WRITE_FLOAT_BE, 0x0837),
    (SPEC_BUF_READ_DOUBLE_LE, 0x0838), (SPEC_BUF_WRITE_DOUBLE_LE, 0x0839),
    (SPEC_BUF_READ_DOUBLE_BE, 0x083A), (SPEC_BUF_WRITE_DOUBLE_BE, 0x083B),
    (SPEC_BUF_READ_BIGINT64_LE, 0x083C), (SPEC_BUF_WRITE_BIGINT64_LE, 0x083D),
    (SPEC_BUF_READ_BIGINT64_BE, 0x083E), (SPEC_BUF_WRITE_BIGINT64_BE, 0x083F),
    (SPEC_BUF_READ_BIGUINT64_LE, 0x0840), (SPEC_BUF_WRITE_BIGUINT64_LE, 0x0841),
    (SPEC_BUF_READ_BIGUINT64_BE, 0x0842), (SPEC_BUF_WRITE_BIGUINT64_BE, 0x0843),
    (SPEC_BUF_READ_UINT_LE, 0x0844), (SPEC_BUF_WRITE_UINT_LE, 0x0845),
    (SPEC_BUF_READ_UINT_BE, 0x0846), (SPEC_BUF_WRITE_UINT_BE, 0x0847),
    (SPEC_BUF_READ_INT_LE, 0x0848), (SPEC_BUF_WRITE_INT_LE, 0x0849),
    (SPEC_BUF_READ_INT_BE, 0x084A), (SPEC_BUF_WRITE_INT_BE, 0x084B),
}

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
// http response methods (dispatched with the res receiver).
pub const SPEC_HTTP_RES_SET_HEADER: NodeSpec = NodeSpec::new("http:res:setHeader", 0x0F03);
pub const SPEC_HTTP_RES_WRITE_HEAD: NodeSpec = NodeSpec::new("http:res:writeHead", 0x0F04);
pub const SPEC_HTTP_RES_WRITE: NodeSpec = NodeSpec::new("http:res:write", 0x0F05);
pub const SPEC_HTTP_RES_END: NodeSpec = NodeSpec::new("http:res:end", 0x0F06);
// http ClientRequest methods (dispatched with the req receiver).
pub const SPEC_HTTP_REQ_WRITE: NodeSpec = NodeSpec::new("http:req:write", 0x0F09);
pub const SPEC_HTTP_REQ_END: NodeSpec = NodeSpec::new("http:req:end", 0x0F0A);

pub const SPEC_NET_CONNECT: NodeSpec = NodeSpec::new("net:connect", 0x1000);
pub const SPEC_NET_SERVER: NodeSpec = NodeSpec::new("net:createServer", 0x1001);
pub const SPEC_NET_ISIP: NodeSpec = NodeSpec::new("net:isIP", 0x1002);
pub const SPEC_NET_ISIPV4: NodeSpec = NodeSpec::new("net:isIPv4", 0x1003);
pub const SPEC_NET_ISIPV6: NodeSpec = NodeSpec::new("net:isIPv6", 0x1004);
pub const SPEC_NET_GET_ASF_TIMEOUT: NodeSpec =
    NodeSpec::new("net:getDefaultAutoSelectFamilyAttemptTimeout", 0x1005);
pub const SPEC_NET_SET_ASF_TIMEOUT: NodeSpec =
    NodeSpec::new("net:setDefaultAutoSelectFamilyAttemptTimeout", 0x1006);

// net socket / server methods (dispatched with the JS receiver).
pub const SPEC_NET_SERVER_LISTEN: NodeSpec = NodeSpec::new("net:server:listen", 0x1007);
pub const SPEC_NET_SERVER_CLOSE: NodeSpec = NodeSpec::new("net:server:close", 0x1008);
pub const SPEC_NET_SERVER_ADDRESS: NodeSpec = NodeSpec::new("net:server:address", 0x1009);
pub const SPEC_NET_SOCKET_WRITE: NodeSpec = NodeSpec::new("net:socket:write", 0x100A);
pub const SPEC_NET_SOCKET_END: NodeSpec = NodeSpec::new("net:socket:end", 0x100B);
pub const SPEC_NET_SOCKET_DESTROY: NodeSpec = NodeSpec::new("net:socket:destroy", 0x100C);
pub const SPEC_NET_SOCKET_ADDRESS: NodeSpec = NodeSpec::new("net:socket:address", 0x100D);
pub const SPEC_NET_SOCKET_SET_NO_DELAY: NodeSpec = NodeSpec::new("net:socket:setNoDelay", 0x100E);
pub const SPEC_NET_SOCKET_SET_KEEP_ALIVE: NodeSpec =
    NodeSpec::new("net:socket:setKeepAlive", 0x100F);
pub const SPEC_NET_SOCKET_SET_ENCODING: NodeSpec = NodeSpec::new("net:socket:setEncoding", 0x1010);
pub const SPEC_NET_SOCKET_PAUSE: NodeSpec = NodeSpec::new("net:socket:pause", 0x1011);
pub const SPEC_NET_SOCKET_RESUME: NodeSpec = NodeSpec::new("net:socket:resume", 0x1012);

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
pub const SPEC_FS_LSTAT: NodeSpec = NodeSpec::new("fs:lstat", 0x110D);
pub const SPEC_FS_ACCESS: NodeSpec = NodeSpec::new("fs:access", 0x110E);
pub const SPEC_FS_RMDIR: NodeSpec = NodeSpec::new("fs:rmdir", 0x110F);
pub const SPEC_FS_RM: NodeSpec = NodeSpec::new("fs:rm", 0x1110);
pub const SPEC_FS_RENAME: NodeSpec = NodeSpec::new("fs:rename", 0x1111);
pub const SPEC_FS_APPENDFILE: NodeSpec = NodeSpec::new("fs:appendFile", 0x1112);
pub const SPEC_FS_COPYFILE: NodeSpec = NodeSpec::new("fs:copyFile", 0x1113);
pub const SPEC_FS_MKDTEMP: NodeSpec = NodeSpec::new("fs:mkdtemp", 0x1114);
pub const SPEC_FS_READLINK: NodeSpec = NodeSpec::new("fs:readlink", 0x1115);
pub const SPEC_FS_CHMOD: NodeSpec = NodeSpec::new("fs:chmod", 0x1116);
pub const SPEC_FS_TRUNCATE: NodeSpec = NodeSpec::new("fs:truncate", 0x1117);
pub const SPEC_FS_LSTATSYNC: NodeSpec = NodeSpec::new("fs:lstatSync", 0x1118);
pub const SPEC_FS_ACCESSSYNC: NodeSpec = NodeSpec::new("fs:accessSync", 0x1119);
pub const SPEC_FS_RMDIRSYNC: NodeSpec = NodeSpec::new("fs:rmdirSync", 0x111A);
pub const SPEC_FS_RMSYNC: NodeSpec = NodeSpec::new("fs:rmSync", 0x111B);
pub const SPEC_FS_RENAMESYNC: NodeSpec = NodeSpec::new("fs:renameSync", 0x111C);
pub const SPEC_FS_APPENDFILESYNC: NodeSpec = NodeSpec::new("fs:appendFileSync", 0x111D);
pub const SPEC_FS_COPYFILESYNC: NodeSpec = NodeSpec::new("fs:copyFileSync", 0x111E);
pub const SPEC_FS_MKDTEMPSYNC: NodeSpec = NodeSpec::new("fs:mkdtempSync", 0x111F);
pub const SPEC_FS_READLINKSYNC: NodeSpec = NodeSpec::new("fs:readlinkSync", 0x1120);
pub const SPEC_FS_CHMODSYNC: NodeSpec = NodeSpec::new("fs:chmodSync", 0x1121);
pub const SPEC_FS_TRUNCATESYNC: NodeSpec = NodeSpec::new("fs:truncateSync", 0x1122);
pub const SPEC_FS_MKDIRSYNC: NodeSpec = NodeSpec::new("fs:mkdirSync", 0x1123);
pub const SPEC_FS_UNLINKSYNC: NodeSpec = NodeSpec::new("fs:unlinkSync", 0x1124);
pub const SPEC_FS_STAT_ISFILE: NodeSpec = NodeSpec::new("fs:Stats:isFile", 0x1130);
pub const SPEC_FS_STAT_ISDIR: NodeSpec = NodeSpec::new("fs:Stats:isDirectory", 0x1131);
pub const SPEC_FS_STAT_ISSYMLINK: NodeSpec = NodeSpec::new("fs:Stats:isSymbolicLink", 0x1132);
pub const SPEC_FS_STAT_ISBLOCK: NodeSpec = NodeSpec::new("fs:Stats:isBlockDevice", 0x1133);
pub const SPEC_FS_STAT_ISCHAR: NodeSpec = NodeSpec::new("fs:Stats:isCharacterDevice", 0x1134);
pub const SPEC_FS_STAT_ISFIFO: NodeSpec = NodeSpec::new("fs:Stats:isFIFO", 0x1135);
pub const SPEC_FS_STAT_ISSOCKET: NodeSpec = NodeSpec::new("fs:Stats:isSocket", 0x1136);
pub const SPEC_FS_REALPATH: NodeSpec = NodeSpec::new("fs:realpath", 0x1137);
pub const SPEC_FSP_READFILE: NodeSpec = NodeSpec::new("fs:promises:readFile", 0x1140);
pub const SPEC_FSP_WRITEFILE: NodeSpec = NodeSpec::new("fs:promises:writeFile", 0x1141);
pub const SPEC_FSP_APPENDFILE: NodeSpec = NodeSpec::new("fs:promises:appendFile", 0x1142);
pub const SPEC_FSP_STAT: NodeSpec = NodeSpec::new("fs:promises:stat", 0x1143);
pub const SPEC_FSP_LSTAT: NodeSpec = NodeSpec::new("fs:promises:lstat", 0x1144);
pub const SPEC_FSP_READDIR: NodeSpec = NodeSpec::new("fs:promises:readdir", 0x1145);
pub const SPEC_FSP_MKDIR: NodeSpec = NodeSpec::new("fs:promises:mkdir", 0x1146);
pub const SPEC_FSP_UNLINK: NodeSpec = NodeSpec::new("fs:promises:unlink", 0x1147);
pub const SPEC_FSP_RMDIR: NodeSpec = NodeSpec::new("fs:promises:rmdir", 0x1148);
pub const SPEC_FSP_RM: NodeSpec = NodeSpec::new("fs:promises:rm", 0x1149);
pub const SPEC_FSP_RENAME: NodeSpec = NodeSpec::new("fs:promises:rename", 0x114A);
pub const SPEC_FSP_COPYFILE: NodeSpec = NodeSpec::new("fs:promises:copyFile", 0x114B);
pub const SPEC_FSP_ACCESS: NodeSpec = NodeSpec::new("fs:promises:access", 0x114C);
pub const SPEC_FSP_MKDTEMP: NodeSpec = NodeSpec::new("fs:promises:mkdtemp", 0x114D);
pub const SPEC_FSP_READLINK: NodeSpec = NodeSpec::new("fs:promises:readlink", 0x114E);
pub const SPEC_FSP_CHMOD: NodeSpec = NodeSpec::new("fs:promises:chmod", 0x114F);
pub const SPEC_FSP_TRUNCATE: NodeSpec = NodeSpec::new("fs:promises:truncate", 0x1150);
pub const SPEC_FSP_REALPATH: NodeSpec = NodeSpec::new("fs:promises:realpath", 0x1151);

pub const SPEC_REQUIRE: NodeSpec = NodeSpec::new("require", 0x1200);
pub const SPEC_READLINE: NodeSpec = NodeSpec::new("readline:createInterface", 0x1300);
pub const SPEC_CJS_WRAP: NodeSpec = NodeSpec::new("__quench_cjs_wrap__", 0x1d00);
pub const SPEC_CP_SPAWNSYNC: NodeSpec = NodeSpec::new("child_process:spawnSync", 0x1e00);
pub const SPEC_CP_EXECSYNC: NodeSpec = NodeSpec::new("child_process:execSync", 0x1e01);
pub const SPEC_CP_EXEC: NodeSpec = NodeSpec::new("child_process:exec", 0x1e02);
pub const SPEC_CP_SPAWN: NodeSpec = NodeSpec::new("child_process:spawn", 0x1e03);
pub const SPEC_URL_PATH_TO_FILE_URL: NodeSpec = NodeSpec::new("url:pathToFileURL", 0x0505);
pub const SPEC_URL_GET_HREF: NodeSpec = NodeSpec::new("url:get:href", 0x0506);
pub const SPEC_URL_GET_PROTOCOL: NodeSpec = NodeSpec::new("url:get:protocol", 0x0507);
pub const SPEC_URL_GET_USERNAME: NodeSpec = NodeSpec::new("url:get:username", 0x0508);
pub const SPEC_URL_GET_PASSWORD: NodeSpec = NodeSpec::new("url:get:password", 0x0509);
pub const SPEC_URL_GET_HOST: NodeSpec = NodeSpec::new("url:get:host", 0x050A);
pub const SPEC_URL_GET_HOSTNAME: NodeSpec = NodeSpec::new("url:get:hostname", 0x050B);
pub const SPEC_URL_GET_PORT: NodeSpec = NodeSpec::new("url:get:port", 0x050C);
pub const SPEC_URL_GET_PATHNAME: NodeSpec = NodeSpec::new("url:get:pathname", 0x050D);
pub const SPEC_URL_GET_SEARCH: NodeSpec = NodeSpec::new("url:get:search", 0x050E);
pub const SPEC_URL_GET_HASH: NodeSpec = NodeSpec::new("url:get:hash", 0x050F);
pub const SPEC_URL_GET_ORIGIN: NodeSpec = NodeSpec::new("url:get:origin", 0x0510);
pub const SPEC_URL_GET_SEARCH_PARAMS: NodeSpec = NodeSpec::new("url:get:searchParams", 0x0511);
pub const SPEC_URL_TO_STRING: NodeSpec = NodeSpec::new("url:toString", 0x0512);
pub const SPEC_URL_TO_JSON: NodeSpec = NodeSpec::new("url:toJSON", 0x0513);
pub const SPEC_URL_REVOKE_OBJECT_URL: NodeSpec = NodeSpec::new("url:revokeObjectURL", 0x0514);
pub const SPEC_URL_FILE_URL_TO_PATH: NodeSpec = NodeSpec::new("url:fileURLToPath", 0x0515);
pub const SPEC_URL_TO_HTTP_OPTIONS: NodeSpec = NodeSpec::new("url:urlToHttpOptions", 0x0516);
pub const SPEC_URL_DOMAIN_TO_ASCII: NodeSpec = NodeSpec::new("url:domainToASCII", 0x0517);
pub const SPEC_URL_DOMAIN_TO_UNICODE: NodeSpec = NodeSpec::new("url:domainToUnicode", 0x0518);
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

pub const SPEC_VM_RUN_IN_NEW_CONTEXT: NodeSpec = NodeSpec::new("vm:runInNewContext", 0x1600);

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
pub fn namespace_bindings(
    argv: &[String],
    exec_path: &str,
) -> Vec<(String, quench_runtime::value::Value)> {
    let mut out = Vec::new();
    push_bindings(&mut out, argv, exec_path);
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

fn push_bindings(
    out: &mut Vec<(String, quench_runtime::value::Value)>,
    argv: &[String],
    exec_path: &str,
) {
    out.push((
        "console".to_string(),
        crate::modules::console::build_value(),
    ));
    out.push((
        "process".to_string(),
        crate::modules::process::build(argv, exec_path),
    ));
    out.push(("Buffer".to_string(), crate::modules::buffer::build_object()));
}

fn timers_binding(
    name: &'static str,
    spec: crate::registry::NodeSpec,
) -> (String, quench_runtime::value::Value) {
    (name.to_string(), crate::host::capability(spec))
}
