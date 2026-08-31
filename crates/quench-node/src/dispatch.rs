//! Single canonical dispatch table.
//!
//! Each capability id maps to one Rust handler. The host's
//! `Host::call`/`construct` routes through this table. Adding a
//! new Node API is a one-line entry in the appropriate per-domain
//! table; no plumbing changes.

use crate::dispatch_fs::fs_dispatch;
use crate::dispatch_handlers as handlers;
use crate::modules::events;
use crate::registry::*;

pub use crate::dispatch_handlers::{CallHandler, ConstructHandler};

const CAP_CONSOLE_LOG: u16 = 0x0200;
const CAP_CONSOLE_INFO: u16 = 0x0201;
const CAP_CONSOLE_WARN: u16 = 0x0202;
const CAP_CONSOLE_ERROR: u16 = 0x0203;
const CAP_CONSOLE_DEBUG: u16 = 0x0204;
const CAP_CONSOLE_TRACE: u16 = 0x0205;
const CAP_UTIL_FORMAT: u16 = 0x0300;
const CAP_UTIL_DEPRECATE: u16 = 0x0730;
const CAP_UTIL_DEPRECATED_CALL: u16 = 0x0731;
const CAP_UTIL_SYSTEM_ERROR_NAME: u16 = 0x0733;
const CAP_UTIL_CONVERT_SIGNAL_TO_EXIT_CODE: u16 =
    crate::registry::SPEC_UTIL_CONVERT_SIGNAL_TO_EXIT_CODE.cap;
const CAP_UTIL_DEBUGLOG: u16 = 0x0735;
const CAP_UTIL_EXCEPTION_WITH_HOST_PORT: u16 = 0x0734;
const CAP_INTERNAL_UTIL_EMIT_WARNING: u16 = crate::registry::SPEC_INTERNAL_UTIL_EMIT_WARNING.cap;
const CAP_INTERNAL_UTIL_NORMALIZE_ENCODING: u16 =
    crate::registry::SPEC_INTERNAL_UTIL_NORMALIZE_ENCODING.cap;
const CAP_INTERNAL_UTIL_GET_CIDR: u16 = crate::registry::SPEC_INTERNAL_UTIL_GET_CIDR.cap;
const CAP_INTERNAL_UTIL_CONSTRUCT_SHARED_ARRAY_BUFFER: u16 =
    crate::registry::SPEC_INTERNAL_UTIL_CONSTRUCT_SHARED_ARRAY_BUFFER.cap;
const CAP_INTERNAL_UTIL_DECORATE_ERROR_STACK: u16 =
    crate::registry::SPEC_INTERNAL_UTIL_DECORATE_ERROR_STACK.cap;
const CAP_INTERNAL_UTIL_ASSIGN_FUNCTION_NAME: u16 =
    crate::registry::SPEC_INTERNAL_UTIL_ASSIGN_FUNCTION_NAME.cap;
const CAP_INTERNAL_UTIL_IS_ERROR: u16 = crate::registry::SPEC_INTERNAL_UTIL_IS_ERROR.cap;
const CAP_OS_GET_PRIORITY: u16 = 0x0736;
const CAP_OS_AVAILABLE_PARALLELISM: u16 = crate::registry::SPEC_OS_AVAILABLE_PARALLELISM.cap;
const CAP_OS_SET_PRIORITY: u16 = 0x0737;
const CAP_INTERNAL_OS_GET_HOME_DIRECTORY: u16 = 0x0738;
const CAP_UTIL_INSPECT: u16 = 0x0301;
const CAP_UTIL_ABORTED: u16 = 0x0310;
const CAP_UTIL_ABORTED_RESOLVE: u16 = 0x0311;
const CAP_UTIL_TO_USV_STRING: u16 = 0x0309;
const CAP_UTIL_IS_NATIVE_ERROR: u16 = 0x030A;
const CAP_UTIL_PARSE_ENV: u16 = 0x030B;
const CAP_UTIL_TYPE_PREDICATE: u16 = 0x030C;
const CAP_INTERNAL_JS_STREAM: u16 = 0x0F12;
const CAP_VM_SOURCE_TEXT_MODULE: u16 = 0x0F11;
const CAP_VM_MODULE_LINK: u16 = 0x0F13;
const CAP_VM_MODULE_EVALUATE: u16 = 0x0F14;
const CAP_UTIL_INHERITS: u16 = 0x0304;
const CAP_UTIL_STRIP_VT: u16 = 0x0305;
const CAP_UTIL_FORMAT_WITH_OPTIONS: u16 = 0x0306;
const CAP_UTIL_STYLE_TEXT: u16 = 0x0307;
const CAP_UTIL_IS_DEEP_STRICT_EQUAL: u16 = 0x0308;
const CAP_TEXT_DECODER_NEW: u16 = 0x0809;
const CAP_TEXT_DECODER_DECODE: u16 = 0x080A;
const CAP_TEXT_ENCODER_NEW: u16 = 0x084C;
const CAP_QUEUE_MICROTASK: u16 = 0x0707;
const CAP_INTERNAL_GET_PROXY_DETAILS: u16 = crate::registry::SPEC_INTERNAL_GET_PROXY_DETAILS.cap;
const CAP_BUFFER_NEW: u16 = 0x0805;
const CAP_DNS_LOOKUP_ADDRESSES: u16 = 0x0E02;
const CAP_TTY_ISATTY: u16 = 0x0900;
const CAP_PROCESS_EXIT: u16 = 0x0A01;
const CAP_PROCESS_KILL: u16 = 0x0A20;
const CAP_PROCESS_CWD: u16 = 0x0A02;
const CAP_PROCESS_CHDIR: u16 = 0x0A03;
const CAP_PROCESS_NEXT_TICK: u16 = 0x0A04;
const CAP_PROCESS_HRTIME: u16 = 0x0A05;
const CAP_PROCESS_HRTIME_BIGINT: u16 = 0x0A0B;
const CAP_PROCESS_CPU_USAGE: u16 = 0x0A10;
const CAP_PROCESS_UPTIME: u16 = 0x0A11;
const CAP_PROCESS_AVAILABLE_MEMORY: u16 = 0x0A1B;
const CAP_PROCESS_CONSTRAINED_MEMORY: u16 = 0x0A1C;
const CAP_PROCESS_MEMORY_USAGE: u16 = 0x0A2C;
const CAP_PROCESS_GETUID: u16 = 0x0A12;
const CAP_PROCESS_GETGID: u16 = 0x0A13;
const CAP_PROCESS_GETEUID: u16 = 0x0A14;
const CAP_PROCESS_GETEGID: u16 = 0x0A15;
const CAP_PROCESS_SETUID: u16 = 0x0A16;
const CAP_PROCESS_SETGID: u16 = 0x0A17;
const CAP_PROCESS_SETEUID: u16 = 0x0A18;
const CAP_PROCESS_SETEGID: u16 = 0x0A19;
const CAP_PROCESS_ACTIVE_RESOURCES: u16 = 0x0A1A;
const CAP_OS_PLATFORM: u16 = 0x0B00;
const CAP_OS_ARCH: u16 = 0x0B01;
const CAP_OS_HOSTNAME: u16 = 0x0B02;
const CAP_OS_TYPE: u16 = 0x0B03;
const CAP_OS_RELEASE: u16 = 0x0B04;
const CAP_OS_CPUS: u16 = 0x0B05;
const CAP_OS_TMPDIR: u16 = 0x0B06;
const CAP_OS_HOMEDIR: u16 = 0x0B07;
const CAP_OS_EOL: u16 = 0x0B08;
const CAP_OS_UPTIME: u16 = 0x0B09;
const CAP_OS_FREEMEM: u16 = 0x0B0A;
const CAP_OS_TOTALMEM: u16 = 0x0B0B;
const CAP_OS_LOADAVG: u16 = 0x0B0C;
const CAP_OS_NETIF: u16 = 0x0B0D;
const CAP_OS_ENDIANNESS: u16 = 0x0B11;
const CAP_OS_VERSION: u16 = 0x0B12;
const CAP_OS_MACHINE: u16 = 0x0B13;
const CAP_OS_USERINFO: u16 = 0x0B14;
const CAP_STREAM_READABLE: u16 = 0x0C00;
const CAP_STREAM_WRITABLE: u16 = 0x0C01;
const CAP_STREAM_DUPLEX: u16 = 0x0C02;
const CAP_STREAM_TRANSFORM: u16 = 0x0C03;
const CAP_STREAM_PIPELINE: u16 = 0x0C04;
const CAP_STRING_DECODER: u16 = 0x0D00;
const CAP_STRING_DECODER_WRITE: u16 = 0x0D01;
const CAP_STRING_DECODER_END: u16 = 0x0D02;
const CAP_STRING_DECODER_CALL: u16 = 0x0D03;
const CAP_STRING_DECODER_TEXT: u16 = 0x0D04;
const CAP_DNS_LOOKUP: u16 = 0x0E00;
const CAP_DNS_RESOLVE4: u16 = 0x0E01;
const CAP_HTTP_REQUEST: u16 = 0x0F00;
const CAP_HTTP_GET: u16 = 0x0F01;
const CAP_HTTP_SERVER: u16 = 0x0F02;
const CAP_HTTP_RES_SET_HEADER: u16 = 0x0F03;
const CAP_HTTP_RES_REMOVE_HEADER: u16 = crate::registry::SPEC_HTTP_RES_REMOVE_HEADER.cap;
const CAP_HTTP_RES_CORK: u16 = crate::registry::SPEC_HTTP_RES_CORK.cap;
const CAP_HTTP_RES_UNCORK: u16 = crate::registry::SPEC_HTTP_RES_UNCORK.cap;
const CAP_HTTP_RES_SET_HEADERS: u16 = crate::registry::SPEC_HTTP_RES_SET_HEADERS.cap;
const CAP_HTTP_RES_WRITE_HEAD: u16 = 0x0F04;
const CAP_HTTP_RES_WRITE: u16 = 0x0F05;
const CAP_HTTP_RES_END: u16 = 0x0F06;
const CAP_HTTP_CONN: u16 = 0x0F07;
const CAP_HTTP_DATA: u16 = 0x0F08;
const CAP_HTTP_REQ_WRITE: u16 = 0x0F09;
const CAP_HTTP_REQ_END: u16 = 0x0F0A;
const CAP_HTTP_REQ_SET_HEADER: u16 = 0x0F0D;
const CAP_HTTP_AGENT: u16 = 0x0F0E;
const CAP_HTTP_REQ_RESUME: u16 = 0x0F0F;
const CAP_HTTP_RES_SET_ENCODING: u16 = 0x0F10;
const CAP_HTTP_RESDATA: u16 = 0x0F0B;
const CAP_HTTP_RESEND: u16 = 0x0F0C;
const CAP_NET_CONNECT: u16 = 0x1000;
const CAP_NET_SOCKET: u16 = 0x1013;
const CAP_NET_SERVER: u16 = 0x1001;
const CAP_NET_LOOKUP_CALLBACK: u16 = 0x1016;
const CAP_NET_ISIP: u16 = 0x1002;
const CAP_NET_ISIPV4: u16 = 0x1003;
const CAP_NET_ISIPV6: u16 = 0x1004;
const CAP_REQUIRE: u16 = 0x1200;
const CAP_READLINE: u16 = 0x1300;
const CAP_READLINE_DRIVER: u16 = 0x1301;
const CAP_READLINE_DONE: u16 = 0x1302;
const CAP_ASYNC_RESOURCE: u16 = crate::registry::SPEC_ASYNC_RESOURCE.cap;
const CAP_ASYNC_EXECUTION_ID: u16 = crate::registry::SPEC_ASYNC_EXECUTION_ID.cap;
const CAP_ASYNC_TRIGGER_ID: u16 = crate::registry::SPEC_ASYNC_TRIGGER_ID.cap;
const CAP_ASYNC_EXECUTION_RESOURCE: u16 = crate::registry::SPEC_ASYNC_EXECUTION_RESOURCE.cap;
const CAP_ASYNC_CREATE_HOOK: u16 = crate::registry::SPEC_ASYNC_CREATE_HOOK.cap;
const CAP_ASYNC_RESOURCE_RUN: u16 = crate::registry::SPEC_ASYNC_RESOURCE_RUN.cap;
const CAP_ASYNC_RESOURCE_BEFORE: u16 = crate::registry::SPEC_ASYNC_RESOURCE_BEFORE.cap;
const CAP_ASYNC_RESOURCE_AFTER: u16 = crate::registry::SPEC_ASYNC_RESOURCE_AFTER.cap;
const CAP_ASYNC_RESOURCE_DESTROY: u16 = crate::registry::SPEC_ASYNC_RESOURCE_DESTROY.cap;
const CAP_ASYNC_RESOURCE_ID: u16 = crate::registry::SPEC_ASYNC_RESOURCE_ID.cap;
const CAP_ASYNC_RESOURCE_TRIGGER: u16 = crate::registry::SPEC_ASYNC_RESOURCE_TRIGGER.cap;
const CAP_ASYNC_HOOK_ENABLE: u16 = crate::registry::SPEC_ASYNC_HOOK_ENABLE.cap;
const CAP_ASYNC_HOOK_DISABLE: u16 = crate::registry::SPEC_ASYNC_HOOK_DISABLE.cap;
const CAP_ASYNC_LOCAL_GET: u16 = crate::registry::SPEC_ASYNC_LOCAL_GET.cap;
const CAP_ASYNC_LOCAL_RUN: u16 = crate::registry::SPEC_ASYNC_LOCAL_RUN.cap;
const CAP_ASYNC_LOCAL_ENTER: u16 = crate::registry::SPEC_ASYNC_LOCAL_ENTER.cap;
const CAP_ASYNC_LOCAL_DISABLE: u16 = crate::registry::SPEC_ASYNC_LOCAL_DISABLE.cap;
const CAP_ASYNC_LOCAL_STORAGE: u16 = crate::registry::SPEC_ASYNC_LOCAL_STORAGE.cap;
const CAP_ASYNC_WORKER_RESOURCE: u16 = crate::registry::SPEC_ASYNC_WORKER_RESOURCE.cap;
const CAP_INSPECTOR_SESSION: u16 = crate::registry::SPEC_INSPECTOR_SESSION.cap;
const CAP_INSPECTOR_CONNECT: u16 = crate::registry::SPEC_INSPECTOR_CONNECT.cap;
const CAP_INSPECTOR_CONNECT_MAIN: u16 = crate::registry::SPEC_INSPECTOR_CONNECT_MAIN.cap;
const CAP_INSPECTOR_DISCONNECT: u16 = crate::registry::SPEC_INSPECTOR_DISCONNECT.cap;
const CAP_INSPECTOR_POST: u16 = crate::registry::SPEC_INSPECTOR_POST.cap;
const CAP_INSPECTOR_OPEN: u16 = crate::registry::SPEC_INSPECTOR_OPEN.cap;
const CAP_INSPECTOR_CLOSE: u16 = crate::registry::SPEC_INSPECTOR_CLOSE.cap;
const CAP_INSPECTOR_WAIT: u16 = crate::registry::SPEC_INSPECTOR_WAIT.cap;
const CAP_WASI_CONSTRUCTOR: u16 = crate::registry::SPEC_WASI_CONSTRUCTOR.cap;
const CAP_WASI_START: u16 = crate::registry::SPEC_WASI_START.cap;
const CAP_WASI_INITIALIZE: u16 = crate::registry::SPEC_WASI_INITIALIZE.cap;
const CAP_WASI_IMPORT_OBJECT: u16 = crate::registry::SPEC_WASI_IMPORT_OBJECT.cap;
const CAP_DIAGNOSTICS_CHANNEL: u16 = crate::registry::SPEC_DIAGNOSTICS_CHANNEL.cap;
const CAP_DIAGNOSTICS_CHANNEL_CONSTRUCTOR: u16 =
    crate::registry::SPEC_DIAGNOSTICS_CHANNEL_CONSTRUCTOR.cap;
const CAP_DIAGNOSTICS_SUBSCRIBE: u16 = crate::registry::SPEC_DIAGNOSTICS_SUBSCRIBE.cap;
const CAP_DIAGNOSTICS_UNSUBSCRIBE: u16 = crate::registry::SPEC_DIAGNOSTICS_UNSUBSCRIBE.cap;
const CAP_DIAGNOSTICS_HAS_SUBSCRIBERS: u16 = crate::registry::SPEC_DIAGNOSTICS_HAS_SUBSCRIBERS.cap;
const CAP_DIAGNOSTICS_CHANNEL_SUBSCRIBE: u16 =
    crate::registry::SPEC_DIAGNOSTICS_CHANNEL_SUBSCRIBE.cap;
const CAP_DIAGNOSTICS_CHANNEL_UNSUBSCRIBE: u16 =
    crate::registry::SPEC_DIAGNOSTICS_CHANNEL_UNSUBSCRIBE.cap;
const CAP_DIAGNOSTICS_CHANNEL_PUBLISH: u16 = crate::registry::SPEC_DIAGNOSTICS_CHANNEL_PUBLISH.cap;
const CAP_DIAGNOSTICS_CHANNEL_BIND_STORE: u16 =
    crate::registry::SPEC_DIAGNOSTICS_CHANNEL_BIND_STORE.cap;
const CAP_DIAGNOSTICS_CHANNEL_UNBIND_STORE: u16 =
    crate::registry::SPEC_DIAGNOSTICS_CHANNEL_UNBIND_STORE.cap;
const CAP_DIAGNOSTICS_TRACING_CHANNEL: u16 = crate::registry::SPEC_DIAGNOSTICS_TRACING_CHANNEL.cap;
const CAP_DIAGNOSTICS_TRACING_SUBSCRIBE: u16 =
    crate::registry::SPEC_DIAGNOSTICS_TRACING_SUBSCRIBE.cap;
const CAP_DIAGNOSTICS_TRACING_UNSUBSCRIBE: u16 =
    crate::registry::SPEC_DIAGNOSTICS_TRACING_UNSUBSCRIBE.cap;
const CAP_DIAGNOSTICS_TRACING_TRACE_SYNC: u16 =
    crate::registry::SPEC_DIAGNOSTICS_TRACING_TRACE_SYNC.cap;
const CAP_DIAGNOSTICS_BOUNDED_CHANNEL: u16 = crate::registry::SPEC_DIAGNOSTICS_BOUNDED_CHANNEL.cap;
const CAP_DIAGNOSTICS_BOUNDED_SUBSCRIBE: u16 =
    crate::registry::SPEC_DIAGNOSTICS_BOUNDED_SUBSCRIBE.cap;
const CAP_DIAGNOSTICS_BOUNDED_UNSUBSCRIBE: u16 =
    crate::registry::SPEC_DIAGNOSTICS_BOUNDED_UNSUBSCRIBE.cap;
const CAP_DIAGNOSTICS_BOUNDED_RUN: u16 = crate::registry::SPEC_DIAGNOSTICS_BOUNDED_RUN.cap;
const CAP_DIAGNOSTICS_CHANNEL_SCOPE: u16 = crate::registry::SPEC_DIAGNOSTICS_CHANNEL_SCOPE.cap;
const CAP_DOMAIN_CREATE: u16 = crate::registry::SPEC_DOMAIN_CREATE.cap;
const CAP_DOMAIN_CONSTRUCTOR: u16 = crate::registry::SPEC_DOMAIN_CONSTRUCTOR.cap;
const CAP_DOMAIN_ENTER: u16 = crate::registry::SPEC_DOMAIN_ENTER.cap;
const CAP_DOMAIN_EXIT: u16 = crate::registry::SPEC_DOMAIN_EXIT.cap;
const CAP_DOMAIN_ADD: u16 = crate::registry::SPEC_DOMAIN_ADD.cap;
const CAP_DOMAIN_REMOVE: u16 = crate::registry::SPEC_DOMAIN_REMOVE.cap;
const CAP_DOMAIN_RUN: u16 = crate::registry::SPEC_DOMAIN_RUN.cap;
const CAP_DOMAIN_DISPOSE: u16 = crate::registry::SPEC_DOMAIN_DISPOSE.cap;
const CAP_DOMAIN_ON: u16 = crate::registry::SPEC_DOMAIN_ON.cap;
const CAP_DOMAIN_ADD_EMITTER: u16 = crate::registry::SPEC_DOMAIN_ADD_EMITTER.cap;
const CAP_CLUSTER_FORK: u16 = crate::registry::SPEC_CLUSTER_FORK.cap;
const CAP_CLUSTER_DISCONNECT: u16 = crate::registry::SPEC_CLUSTER_DISCONNECT.cap;
const CAP_CLUSTER_SETUP_PRIMARY: u16 = crate::registry::SPEC_CLUSTER_SETUP_PRIMARY.cap;
const CAP_CLUSTER_SETUP_MASTER: u16 = crate::registry::SPEC_CLUSTER_SETUP_MASTER.cap;
const CAP_CLUSTER_SETUP_EVENT: u16 = crate::registry::SPEC_CLUSTER_SETUP_EVENT.cap;
const CAP_CLUSTER_CLOSE_WORKER_NET: u16 = crate::registry::SPEC_CLUSTER_CLOSE_WORKER_NET.cap;
const CAP_CLUSTER_WORKER_IS_DEAD: u16 = crate::registry::SPEC_CLUSTER_WORKER_IS_DEAD.cap;
const CAP_CLUSTER_WORKER_IS_CONNECTED: u16 = crate::registry::SPEC_CLUSTER_WORKER_IS_CONNECTED.cap;
const CAP_CLUSTER_WORKER_ON: u16 = crate::registry::SPEC_CLUSTER_WORKER_ON.cap;
const CAP_CLUSTER_WORKER_EMIT: u16 = crate::registry::SPEC_CLUSTER_WORKER_EMIT.cap;
const CAP_CLUSTER_WORKER_DISCONNECT: u16 = crate::registry::SPEC_CLUSTER_WORKER_DISCONNECT.cap;
const CAP_CLUSTER_WORKER_KILL: u16 = crate::registry::SPEC_CLUSTER_WORKER_KILL.cap;
const CAP_DIAGNOSTICS_SCOPE_DISPOSE: u16 = crate::registry::SPEC_DIAGNOSTICS_SCOPE_DISPOSE.cap;
const CAP_ZLIB_GZIP: u16 = 0x1700;
const CAP_ZLIB_GUNZIP: u16 = 0x1701;
const CAP_ZLIB_DEFLATE_RAW: u16 = 0x1702;
const CAP_ZLIB_INFLATE_RAW: u16 = 0x1703;
const CAP_ZLIB_DEFLATE: u16 = 0x1704;
const CAP_ZLIB_INFLATE: u16 = 0x1705;
const CAP_ASSERT_OK: u16 = crate::registry::SPEC_ASSERT_OK.cap;
const CAP_ASSERT_STRICT_EQUAL: u16 = crate::registry::SPEC_ASSERT_STRICT_EQUAL.cap;
const CAP_ASSERT_NOT_STRICT_EQUAL: u16 = crate::registry::SPEC_ASSERT_NOT_STRICT_EQUAL.cap;
const CAP_ASSERT_EQUAL: u16 = crate::registry::SPEC_ASSERT_EQUAL.cap;
const CAP_ASSERT_NOT_EQUAL: u16 = crate::registry::SPEC_ASSERT_NOT_EQUAL.cap;
const CAP_ASSERT_DEEP_STRICT_EQUAL: u16 = crate::registry::SPEC_ASSERT_DEEP_STRICT_EQUAL.cap;
const CAP_ASSERT_NOT_DEEP_STRICT_EQUAL: u16 =
    crate::registry::SPEC_ASSERT_NOT_DEEP_STRICT_EQUAL.cap;
const CAP_ASSERT_THROWS: u16 = crate::registry::SPEC_ASSERT_THROWS.cap;
const CAP_ASSERT_DOES_NOT_THROW: u16 = crate::registry::SPEC_ASSERT_DOES_NOT_THROW.cap;
const CAP_ASSERT_FAIL: u16 = crate::registry::SPEC_ASSERT_FAIL.cap;
const CAP_ASSERT_IF_ERROR: u16 = crate::registry::SPEC_ASSERT_IF_ERROR.cap;
const CAP_ASSERT_MATCH: u16 = crate::registry::SPEC_ASSERT_MATCH.cap;
const CAP_ASSERT_DOES_NOT_MATCH: u16 = crate::registry::SPEC_ASSERT_DOES_NOT_MATCH.cap;
const CAP_ASSERT_CONSTRUCTOR: u16 = crate::registry::SPEC_ASSERT_CONSTRUCTOR.cap;
const CAP_ASSERTION_ERROR_CONSTRUCTOR: u16 = crate::registry::SPEC_ASSERTION_ERROR_CONSTRUCTOR.cap;
const CAP_ASSERT_PARTIAL_DEEP_STRICT_EQUAL: u16 =
    crate::registry::SPEC_ASSERT_PARTIAL_DEEP_STRICT_EQUAL.cap;
const CAP_ASSERT_DEEP_EQUAL: u16 = crate::registry::SPEC_ASSERT_DEEP_EQUAL.cap;
const CAP_ASSERT_NOT_DEEP_EQUAL: u16 = crate::registry::SPEC_ASSERT_NOT_DEEP_EQUAL.cap;
const CAP_CJS_WRAP: u16 = 0x1d00;
const CAP_UTIL_GETCALLSITES: u16 = 0x0303;
const CAP_BUFFER_ATOB: u16 = 0x0806;
const CAP_BUFFER_BTOA: u16 = 0x0807;
const CAP_VM_RUN_IN_NEW_CONTEXT: u16 = 0x1600;
const CAP_VM_CREATE_CONTEXT: u16 = 0x1601;
const CAP_VM_RUN_IN_CONTEXT: u16 = 0x1602;
const CAP_VM_IS_CONTEXT: u16 = 0x1603;
const CAP_URL_PATH_TO_FILE_URL: u16 = 0x0505;
const CAP_URL_PATTERN: u16 = 2281;
const CAP_URL_PATTERN_GET: u16 = 2283;
const CAP_URL_PATTERN_TEST: u16 = 2284;
const CAP_URL_PATTERN_EXEC: u16 = 2285;
const CAP_CP_SPAWNSYNC: u16 = 0x1e00;
const CAP_CP_EXECSYNC: u16 = 0x1e01;
const CAP_CP_EXEC: u16 = 0x1e02;
const CAP_CP_EXECFILE: u16 = 0x1e03;
const CAP_CP_EXECFILE_ABORT: u16 = 0x1e07;
const CAP_CP_EXECFILE_COMPLETE: u16 = 0x1e08;
const CAP_CP_SPAWN: u16 = 0x1e06;
const CAP_CP_SPAWN_ERROR_EMIT: u16 = 0x1e04;
const CAP_CP_SPAWN_OUTPUT_EMIT: u16 = 0x1e05;
const CAP_CP_KILL: u16 = crate::registry::SPEC_CP_KILL.cap;
const CAP_CP_STDIN_WRITE: u16 = 0x1E09;
const CAP_CP_STDIN_END: u16 = 0x1E0A;
const CAP_CP_STDOUT_READ: u16 = crate::registry::SPEC_CP_STDOUT_READ.cap;
const CAP_CP_STREAM_SET_ENCODING: u16 = crate::registry::SPEC_CP_STREAM_SET_ENCODING.cap;
const CAP_CP_EXEC_COMPLETE: u16 = crate::registry::SPEC_CP_EXEC_COMPLETE.cap;
const CAP_CP_EXEC_ERROR: u16 = crate::registry::SPEC_CP_EXEC_ERROR.cap;
const CAP_CP_ABORT: u16 = 0x1E0B;
const CAP_CP_ABORT_EMIT: u16 = 0x1E0C;
const CAP_CP_FORK: u16 = 0x1E0D;
const CAP_CP_SEND: u16 = 0x1E0E;
const CAP_CP_MESSAGE_EMIT: u16 = 0x1E0F;
const CAP_CP_DISCONNECT: u16 = 0x1E10;
const CAP_CP_DISCONNECT_EMIT: u16 = 0x1E11;
const CAP_CP_CONSTRUCTOR: u16 = crate::registry::SPEC_CP_CONSTRUCTOR.cap;
const CAP_CP_INSTANCE_SPAWN: u16 = crate::registry::SPEC_CP_INSTANCE_SPAWN.cap;
const CAP_PROCESS_UMASK: u16 = 0x0A06;
const CAP_PROCESS_ON: u16 = 0x0A07;
const CAP_PROCESS_ONCE: u16 = 0x0A08;
const CAP_PROCESS_EMIT: u16 = 0x0A0C;
const CAP_PROCESS_EMIT_WARNING: u16 = 0x0A0D;
const CAP_PROCESS_EXIT_CODE_GET: u16 = 0x0A21;
const CAP_PROCESS_EXIT_CODE_SET: u16 = 0x0A22;
const CAP_PROCESS_REMOVE_LISTENER: u16 = 0x0A0E;
const CAP_PROCESS_REMOVE_ALL_LISTENERS: u16 = 0x0A0F;
const CAP_STDOUT_WRITE: u16 = 0x0A09;
const CAP_STDERR_WRITE: u16 = 0x0A0A;
const CAP_NET_GET_ASF_TIMEOUT: u16 = 0x1005;
const CAP_NET_SET_ASF_TIMEOUT: u16 = 0x1006;
const CAP_NET_SERVER_LISTEN: u16 = 0x1007;
const CAP_NET_SERVER_CLOSE: u16 = 0x1008;
const CAP_NET_SERVER_ADDRESS: u16 = 0x1009;
const CAP_NET_SERVER_UNREF: u16 = 0x1014;
const CAP_NET_SERVER_REF: u16 = 0x1015;
const CAP_NET_SERVER_GET_CONNECTIONS: u16 = 0x1030;
const CAP_NET_SOCKET_WRITE: u16 = 0x100A;
const CAP_NET_SOCKET_END: u16 = 0x100B;
const CAP_NET_SOCKET_DESTROY: u16 = 0x100C;
const CAP_NET_SOCKET_UNREF: u16 = 2290;
const CAP_NET_SOCKET_REF: u16 = 2291;
const CAP_NET_BLOCK_LIST: u16 = 2292;
const CAP_NET_BLOCK_LIST_ADD_SUBNET: u16 = 2293;
const CAP_NET_BLOCK_LIST_ADD_ADDRESS: u16 = 2294;
const CAP_NET_BLOCK_LIST_CHECK: u16 = 2295;
const CAP_NET_SOCKET_ADDRESS: u16 = 0x100D;
const CAP_NET_SOCKET_SET_NO_DELAY: u16 = 0x100E;
const CAP_NET_SOCKET_SET_TOS: u16 = 0x102D;
const CAP_NET_SOCKET_GET_TOS: u16 = 0x102E;
const CAP_NET_SOCKET_HANDLE_CLOSE: u16 = 0x102F;
const CAP_NET_SOCKET_SET_KEEP_ALIVE: u16 = 0x100F;
const CAP_NET_SOCKET_SET_ENCODING: u16 = 0x1010;
const CAP_NET_SOCKET_PAUSE: u16 = 0x1011;
const CAP_NET_SOCKET_RESUME: u16 = 0x1012;
const CAP_NET_ASYNC_ITERATOR: u16 = crate::registry::SPEC_NET_ASYNC_ITERATOR.cap;
const CAP_NET_ASYNC_ITERATOR_NEXT: u16 = crate::registry::SPEC_NET_ASYNC_ITERATOR_NEXT.cap;
const CAP_NET_ASYNC_ITERATOR_RETURN: u16 = crate::registry::SPEC_NET_ASYNC_ITERATOR_RETURN.cap;
const CAP_NET_SOCKET_RESET_AND_DESTROY: u16 =
    crate::registry::SPEC_NET_SOCKET_RESET_AND_DESTROY.cap;
const CAP_NET_SOCKET_ONREAD: u16 = crate::registry::SPEC_NET_SOCKET_ONREAD.cap;
const CAP_STRUCTURED_CLONE: u16 = crate::registry::SPEC_STRUCTURED_CLONE.cap;
const CAP_FETCH: u16 = 0x1F21;
const CAP_GC: u16 = 0x2117;
const CAP_ABORT_CONTROLLER: u16 = 0x1F22;
const CAP_ABORT_SIGNAL: u16 = 0x1F23;
const CAP_ABORT_SIGNAL_ABORT: u16 = 0x1F24;
const CAP_ABORT_SIGNAL_TIMEOUT: u16 = crate::registry::SPEC_ABORT_SIGNAL_TIMEOUT.cap;
const CAP_ABORT_SIGNAL_TIMEOUT_FIRE: u16 = crate::registry::SPEC_ABORT_SIGNAL_TIMEOUT_FIRE.cap;
const CAP_ABORT_SIGNAL_ANY: u16 = crate::registry::SPEC_ABORT_SIGNAL_ANY.cap;
const CAP_ABORT_CONTROLLER_ABORT: u16 = 0x1F25;
const CAP_ABORT_EVENT_STOP_IMMEDIATE: u16 = 0x1F26;
const CAP_ABORT_CONTROLLER_SIGNAL_GET: u16 = 0x1F27;
const CAP_ABORT_SIGNAL_ABORTED_GET: u16 = 0x1F28;
const CAP_ABORT_SIGNAL_HAS_INSTANCE: u16 = 0x1F29;
const CAP_ABORT_SIGNAL_THROW_IF_ABORTED: u16 = 0x1F2A;
const CAP_TEST_RUN: u16 = 0x1b00;
const CAP_TEST_SKIP: u16 = 0x1b01;
const CAP_TEST_MOCK_FN: u16 = 0x1b02;
const CAP_TEST_MOCK_CALL: u16 = 0x1b03;
const CAP_EVENT_TRUSTED_GET: u16 = 0x1b04;
const CAP_TEST_MOCK_METHOD: u16 = 0x1b05;
const CAP_TEST_MOCK_RESTORE: u16 = 0x1b06;
const CAP_TEST_MOCK_BIND: u16 = 0x1b07;
const CAP_TEST_MOCK_BOUND_CALL: u16 = 0x1b08;
const CAP_TEST_MOCK_GETTER: u16 = 0x1b09;
const CAP_TEST_MOCK_SETTER: u16 = 0x1b0A;
const CAP_TEST_MOCK_CALL_COUNT: u16 = 0x1b0B;
const CAP_TEST_MOCK_IMPLEMENTATION: u16 = 0x1b0C;
const CAP_TEST_MOCK_IMPLEMENTATION_ONCE: u16 = 0x1b0D;
const CAP_TEST_BEFORE_EACH: u16 = 0x1b0E;
const CAP_TEST_AFTER_EACH: u16 = 0x1b0F;
const CAP_TEST_NESTED: u16 = 0x1b10;
const CAP_TEST_MOCK_RESET_CALLS: u16 = 0x1b11;
const CAP_TEST_MOCK_RESET: u16 = 0x1b12;
const CAP_TEST_MOCK_PROPERTY: u16 = 0x1b13;
const CAP_TEST_MOCK_ACCESS_COUNT: u16 = 0x1b14;
const CAP_TEST_MOCK_RESET_ACCESSES: u16 = 0x1b15;
const CAP_TEST_MOCK_PROPERTY_GET: u16 = 0x1b16;
const CAP_TEST_MOCK_PROPERTY_SET: u16 = 0x1b17;
const CAP_TEST_MOCK_PROPERTY_ONCE: u16 = 0x1b18;
const CAP_TEST_MOCK_TIMERS_ENABLE: u16 = 0x1b19;
const CAP_TEST_MOCK_TIMERS_TICK: u16 = 0x1b1A;
const CAP_TEST_MOCK_TIMERS_SETTIME: u16 = 0x1b1B;
const CAP_TEST_MOCK_TIMERS_RESET: u16 = 0x1b1C;
const CAP_TEST_MOCK_MODULE: u16 = 0x1b1D;
const CAP_TEST_CONTEXT_SKIP: u16 = 0x1b1E;
const CAP_TEST_CONTEXT_TODO: u16 = 0x1b1F;
const CAP_TEST_RUN_EMIT: u16 = 0x1b20;
const CAP_TEST_GET_CONTEXT: u16 = 0x1b21;

/// Single canonical mapping from capability id to call handler.
pub fn lookup(cap: u16) -> Option<CallHandler> {
    use handlers::*;
    let h = match cap {
        CAP_REQUIRE => node_require,
        CAP_PROCESS_ENV_SET => handlers::process_env_set,
        CAP_EVENTS_NEW => handlers::events_call,
        CAP_EVENTS_ABORT_LISTENER => handlers::events_abort_listener,
        CAP_EVENTS_ABORT_DISPOSE => handlers::events_abort_dispose,
        CAP_EVENTS_ADD_ABORT => handlers::events_add_abort_listener,
        CAP_ABORT_CONTROLLER_ABORT => handlers::abort_controller_abort,
        CAP_ABORT_EVENT_STOP_IMMEDIATE => handlers::abort_event_stop_immediate,
        CAP_EVENT_PREVENT_DEFAULT => handlers::event_prevent_default,
        CAP_EVENT_STOP_PROPAGATION => handlers::event_stop_propagation,
        CAP_EVENT_STOP_IMMEDIATE => handlers::event_stop_immediate,
        CAP_EVENT_COMPOSED_PATH => handlers::event_composed_path,
        CAP_EVENT_GET_CANCEL_BUBBLE => handlers::event_get_cancel_bubble,
        CAP_EVENT_SET_CANCEL_BUBBLE => handlers::event_set_cancel_bubble,
        CAP_DEFINE_EVENT_HANDLER => handlers::define_event_handler,
        CAP_EVENT_HANDLER_GET => handlers::event_handler_get,
        CAP_EVENT_HANDLER_SET => handlers::event_handler_set,
        CAP_EVENT_SOURCE => handlers::event_source,
        CAP_ABORT_SIGNAL_ABORT => handlers::abort_signal_abort,
        CAP_ABORT_SIGNAL_TIMEOUT => handlers::abort_signal_timeout_call,
        CAP_ABORT_SIGNAL_TIMEOUT_FIRE => handlers::abort_signal_timeout_fire,
        CAP_ABORT_SIGNAL_ANY => handlers::abort_signal_any_call,
        CAP_BUFFER_OF => handlers::buffer_of,
        CAP_BUFFER_RESOLVE_OBJECT_URL => crate::modules::buffer::resolve_object_url,
        CAP_STRING_DECODER => string_decoder_invoke,
        CAP_DNS_LOOKUP_ADDRESSES => crate::modules::dns::lookup_addresses_handler,
        CAP_SEA_IS_SEA => crate::modules::compat_extra::sea_is_sea,
        CAP_EVENTS_FROM => events_from,
        CAP_EVENTS_ON => events_method_on,
        CAP_EVENTS_EMIT => events_method_emit,
        CAP_EVENTS_CAPTURE_GET => handlers::events_capture_get,
        CAP_EVENTS_CAPTURE_SET => handlers::events_capture_set,
        CAP_EVENTS_DEFAULT_MAX_GET => handlers::events_default_max_get,
        CAP_EVENTS_DEFAULT_MAX_SET => handlers::events_default_max_set,
        CAP_EVENTS_RAW_LISTENERS => events::method_raw_listeners,
        CAP_CONSOLE_LOG | CAP_CONSOLE_INFO | CAP_CONSOLE_DEBUG => console_log,
        CAP_CONSOLE_WARN | CAP_CONSOLE_ERROR => console_warn,
        CAP_CONSOLE_TRACE => console_trace,
        CAP_UTIL_FORMAT => util_format,
        CAP_UTIL_DEPRECATE => util_deprecate,
        CAP_UTIL_DEPRECATED_CALL => util_deprecated_call,
        CAP_UTIL_SYSTEM_ERROR_NAME => util_system_error_name,
        CAP_UTIL_CONVERT_SIGNAL_TO_EXIT_CODE => util_convert_signal_to_exit_code,
        CAP_UTIL_DEBUGLOG => util_debuglog,
        CAP_UTIL_EXCEPTION_WITH_HOST_PORT => util_exception_with_host_port,
        CAP_INTERNAL_UTIL_EMIT_WARNING => internal_util_emit_warning,
        CAP_INTERNAL_UTIL_NORMALIZE_ENCODING => util_normalize_encoding,
        CAP_INTERNAL_UTIL_GET_CIDR => util_get_cidr,
        CAP_INTERNAL_UTIL_CONSTRUCT_SHARED_ARRAY_BUFFER => util_construct_shared_array_buffer,
        CAP_INTERNAL_UTIL_DECORATE_ERROR_STACK => internal_util_decorate_error_stack,
        CAP_INTERNAL_UTIL_ASSIGN_FUNCTION_NAME => internal_util_assign_function_name,
        CAP_INTERNAL_UTIL_IS_ERROR => internal_util_is_error,
        CAP_OS_GET_PRIORITY => os_get_priority,
        CAP_OS_SET_PRIORITY => os_set_priority,
        CAP_OS_AVAILABLE_PARALLELISM => crate::modules::os::available_parallelism,
        CAP_INTERNAL_OS_GET_HOME_DIRECTORY => internal_os_get_home_directory,
        CAP_UTIL_INSPECT => util_inspect,
        CAP_UTIL_ABORTED => util_aborted,
        CAP_UTIL_ABORTED_RESOLVE => util_aborted_resolve,
        CAP_UTIL_PARSE_ENV => util_parse_env,
        CAP_UTIL_PROMISIFY => util_promisify,
        CAP_UTIL_PROMISIFIED_CALL => util_promisified_call,
        CAP_UTIL_PROMISIFIED_CALLBACK => util_promisified_callback,
        CAP_STRING_DECODER_WRITE => string_decoder_write,
        CAP_STRING_DECODER_END => string_decoder_end,
        CAP_STRING_DECODER_CALL => string_decoder_call,
        CAP_STRING_DECODER_TEXT => string_decoder_text,
        _ => return events_dispatch(cap),
    };
    Some(h)
}

fn events_dispatch(cap: u16) -> Option<CallHandler> {
    use crate::modules::{event_target, events};
    Some(match cap {
        CAP_EVENTS_ONCE => events::method_once,
        CAP_EVENTS_REMOVE_LISTENER => events::method_remove_listener,
        CAP_EVENTS_REMOVE_ALL => events::method_remove_all_listeners,
        CAP_EVENTS_LISTENERS => events::method_listeners,
        CAP_EVENTS_EVENT_NAMES => events::method_event_names,
        CAP_EVENTS_LISTENER_COUNT => events::method_listener_count,
        CAP_EVENTS_PREPEND => events::method_prepend_listener,
        CAP_EVENTS_PREPEND_ONCE => events::method_prepend_once_listener,
        CAP_EVENTS_SET_MAX => events::method_set_max_listeners,
        CAP_EVENTS_GET_MAX => events::method_get_max_listeners,
        CAP_EVENTS_SET_MAX_STATIC => event_target::set_max_listeners,
        CAP_EVENTS_GET_LISTENERS => event_target::get_event_listeners,
        CAP_EVENTS_LISTENER_COUNT_STATIC => event_target::listener_count,
        CAP_EVENTS_GET_MAX_STATIC => event_target::get_max_listeners,
        CAP_TARGET_ADD => event_target::add_event_listener,
        CAP_TARGET_REMOVE => event_target::remove_event_listener,
        CAP_TARGET_DISPATCH => event_target::dispatch_event,
        CAP_NODE_EVENT_TARGET_ADD => event_target::node_add_event_listener,
        CAP_NODE_EVENT_TARGET_REMOVE => event_target::node_remove_listener,
        CAP_NODE_EVENT_TARGET_DISPATCH => event_target::dispatch_event,
        CAP_NODE_EVENT_TARGET_ON => event_target::node_on,
        CAP_NODE_EVENT_TARGET_ONCE => event_target::node_once,
        CAP_NODE_EVENT_TARGET_REMOVE_ALL => event_target::node_remove_all_listeners,
        CAP_NODE_EVENT_TARGET_LISTENER_COUNT => event_target::node_listener_count,
        CAP_NODE_EVENT_TARGET_EVENT_NAMES => event_target::node_event_names,
        CAP_NODE_EVENT_TARGET_SET_MAX => event_target::node_set_max_listeners,
        CAP_NODE_EVENT_TARGET_GET_MAX => event_target::node_get_max_listeners,
        CAP_NODE_EVENT_TARGET_EMIT => event_target::node_emit,
        CAP_EVENT_TARGET_REJECTION => handlers::event_target_rejection,
        CAP_EVENT_GET_PROPERTY => handlers::event_get_property,
        _ => return path_dispatch(cap),
    })
}

fn path_dispatch(cap: u16) -> Option<CallHandler> {
    use crate::modules::path_posix as posix;
    Some(match cap {
        CAP_PATH_JOIN => posix::join,
        CAP_PATH_RESOLVE => posix::resolve,
        CAP_PATH_NORMALIZE => posix::normalize,
        CAP_PATH_DIRNAME => posix::dirname,
        CAP_PATH_BASENAME => posix::basename,
        CAP_PATH_EXTNAME => posix::extname,
        CAP_PATH_ISABSOLUTE => posix::is_absolute,
        CAP_PATH_RELATIVE => posix::relative,
        CAP_PATH_PARSE => posix::parse,
        CAP_PATH_FORMAT => posix::format,
        CAP_PATH_TO_NAMESPACED => posix::to_namespaced_path,
        CAP_PATH_MATCHES_GLOB => posix::matches_glob,
        _ => return path_win32_dispatch(cap),
    })
}

fn path_win32_dispatch(cap: u16) -> Option<CallHandler> {
    use crate::modules::{path_win32 as win32, path_win32_extra as wextra};
    Some(match cap {
        CAP_PATH_WIN32_JOIN => wextra::join,
        CAP_PATH_WIN32_RESOLVE => win32::resolve,
        CAP_PATH_WIN32_NORMALIZE => crate::modules::path_win32_normalize::normalize,
        CAP_PATH_WIN32_DIRNAME => wextra::dirname,
        CAP_PATH_WIN32_BASENAME => wextra::basename,
        CAP_PATH_WIN32_EXTNAME => wextra::extname,
        CAP_PATH_WIN32_ISABSOLUTE => win32::is_absolute,
        CAP_PATH_WIN32_RELATIVE => wextra::relative,
        CAP_PATH_WIN32_PARSE => wextra::parse,
        CAP_PATH_WIN32_FORMAT => wextra::format,
        CAP_PATH_WIN32_TO_NAMESPACED => win32::to_namespaced_path,
        CAP_PATH_WIN32_MATCHES_GLOB => wextra::matches_glob,
        _ => return url_dispatch(cap),
    })
}

fn url_dispatch(cap: u16) -> Option<CallHandler> {
    use handlers::*;
    Some(match cap {
        CAP_URL_PARSE => url_parse,
        CAP_URL_PATTERN_GET => crate::modules::url::url_pattern_get,
        CAP_URL_PATTERN_TEST => crate::modules::url::url_pattern_test,
        CAP_URL_PATTERN_EXEC => crate::modules::url::url_pattern_exec,
        CAP_URL_FORMAT => url_format,
        CAP_URL_RESOLVE => url_resolve,
        CAP_URL_RESOLVE_OBJECT => url_resolve_object,
        CAP_QS_PARSE => crate::modules::querystring_parse::parse,
        CAP_QS_STRINGIFY => crate::modules::querystring_stringify::stringify,
        CAP_QS_ESCAPE => crate::modules::querystring::escape,
        CAP_QS_UNESCAPE => crate::modules::querystring::unescape,
        CAP_QS_UNESCAPE_BUFFER => crate::modules::querystring::unescape_buffer,
        CAP_URL_GET_HREF => crate::modules::url_whatwg::get_href,
        CAP_URL_GET_PROTOCOL => crate::modules::url_whatwg::get_protocol,
        CAP_URL_GET_USERNAME => crate::modules::url_whatwg::get_username,
        CAP_URL_GET_PASSWORD => crate::modules::url_whatwg::get_password,
        CAP_URL_GET_HOST => crate::modules::url_whatwg::get_host,
        CAP_URL_GET_HOSTNAME => crate::modules::url_whatwg::get_hostname,
        CAP_URL_GET_PORT => crate::modules::url_whatwg::get_port,
        CAP_URL_GET_PATHNAME => crate::modules::url_whatwg::get_pathname,
        CAP_URL_GET_SEARCH => crate::modules::url_whatwg::get_search,
        CAP_URL_GET_HASH => crate::modules::url_whatwg::get_hash,
        CAP_URL_GET_ORIGIN => crate::modules::url_whatwg::get_origin,
        CAP_URL_GET_SEARCH_PARAMS => crate::modules::url_whatwg::get_search_params,
        CAP_URL_TO_STRING => crate::modules::url_whatwg::get_href,
        CAP_URL_TO_JSON => crate::modules::url_whatwg::get_href,
        CAP_URL_CREATE_OBJECT_URL => crate::modules::url_whatwg::create_object_url,
        CAP_URL_REVOKE_OBJECT_URL => crate::modules::url_whatwg::revoke_object_url,
        CAP_URL_FILE_URL_TO_PATH => crate::modules::url_file::file_url_to_path,
        CAP_URL_TO_HTTP_OPTIONS => crate::modules::url_whatwg::url_to_http_options,
        CAP_URL_DOMAIN_TO_ASCII => crate::modules::url_whatwg::domain_to_ascii,
        CAP_URL_DOMAIN_TO_UNICODE => crate::modules::url_whatwg::domain_to_unicode,
        CAP_UTIL_INHERITS => crate::modules::util_inherits::inherits,
        CAP_UTIL_STRIP_VT => util_strip_vt,
        CAP_UTIL_FORMAT_WITH_OPTIONS => util_format_with_options,
        CAP_UTIL_STYLE_TEXT => crate::modules::util_style_text::style_text,
        CAP_UTIL_IS_DEEP_STRICT_EQUAL => util_is_deep_strict_equal,
        CAP_UTIL_TO_USV_STRING => util_to_usv_string,
        CAP_UTIL_IS_NATIVE_ERROR => util_is_native_error,
        CAP_UTIL_TYPE_PREDICATE => util_type_predicate,
        CAP_TEXT_DECODER_DECODE => crate::modules::text_decoder::decode,
        _ => return timers_dispatch(cap),
    })
}

fn timers_dispatch(cap: u16) -> Option<CallHandler> {
    use handlers::*;
    Some(match cap {
        CAP_TIMERS_SETTIMEOUT => timers_set_timeout,
        CAP_TIMERS_CLEARTIMEOUT => timers_clear_timeout,
        CAP_TIMERS_SETINTERVAL => timers_set_interval,
        CAP_TIMERS_CLEARINTERVAL => timers_clear_interval,
        CAP_TIMERS_SETIMMEDIATE => timers_set_immediate,
        CAP_TIMERS_CLEARIMMEDIATE => timers_clear_immediate,
        CAP_TIMERS_TICK => timers_tick,
        CAP_QUEUE_MICROTASK => queue_microtask,
        CAP_TIMERS_UNREF => timers_method_unref,
        CAP_TIMERS_REF => timers_method_ref,
        CAP_TIMERS_HASREF => timers_method_has_ref,
        CAP_TIMERS_REFRESH => timers_method_refresh,
        CAP_RUN_LOOP => timers_run_loop,
        CAP_RUN_EXIT => timers_run_exit,
        CAP_RUN_UNCAUGHT => uncaught_dispatch,
        CAP_INTERNAL_UTIL_SLEEP => internal_util_sleep,
        CAP_INTERNAL_UTIL_ASSERT_CRYPTO => internal_util_assert_crypto,
        CAP_TIMERS_CLOSE => timers_method_close,
        CAP_TIMERS_GET_LIBUV_NOW => timers_get_libuv_now,
        CAP_TIMERS_TO_PRIMITIVE => timers_method_to_primitive,
        CAP_LINKED_LIST_INIT => linked_list_init,
        CAP_LINKED_LIST_REMOVE => linked_list_remove,
        CAP_LINKED_LIST_APPEND => linked_list_append,
        CAP_LINKED_LIST_IS_EMPTY => linked_list_is_empty,
        CAP_LINKED_LIST_PEEK => linked_list_peek,
        CAP_TIMERS_SCHEDULE => timers_schedule,
        CAP_TIMERS_TOGGLE_REF => timers_toggle_ref,
        CAP_TIMERS_TOGGLE_IMMEDIATE_REF => timers_toggle_immediate_ref,
        CAP_INTERNAL_BINDING => internal_binding,
        CAP_INTERNAL_BUFFER_FILL => internal_buffer_fill,
        CAP_INTERNAL_VIEW_HAS_BUFFER => internal_view_has_buffer,
        CAP_INTERNAL_BUFFER_ALIGNED_OFFSET => internal_buffer_aligned_offset,
        CAP_INTERNAL_GET_PROXY_DETAILS => internal_get_proxy_details,
        CAP_DIAGNOSTICS_CHANNEL => crate::modules::diagnostics_channel::channel,
        CAP_DIAGNOSTICS_SUBSCRIBE => crate::modules::diagnostics_channel::subscribe,
        CAP_DIAGNOSTICS_UNSUBSCRIBE => crate::modules::diagnostics_channel::unsubscribe,
        CAP_DIAGNOSTICS_HAS_SUBSCRIBERS => crate::modules::diagnostics_channel::has_subscribers,
        CAP_DIAGNOSTICS_CHANNEL_SUBSCRIBE => crate::modules::diagnostics_channel::channel_subscribe,
        CAP_DIAGNOSTICS_CHANNEL_UNSUBSCRIBE => {
            crate::modules::diagnostics_channel::channel_unsubscribe
        }
        CAP_DIAGNOSTICS_CHANNEL_PUBLISH => crate::modules::diagnostics_channel::publish,
        CAP_DIAGNOSTICS_CHANNEL_BIND_STORE => crate::modules::diagnostics_channel::bind_store,
        CAP_DIAGNOSTICS_CHANNEL_UNBIND_STORE => crate::modules::diagnostics_channel::unbind_store,
        CAP_DIAGNOSTICS_TRACING_CHANNEL => crate::modules::diagnostics_channel::tracing_channel,
        CAP_DIAGNOSTICS_TRACING_SUBSCRIBE => crate::modules::diagnostics_channel::tracing_subscribe,
        CAP_DIAGNOSTICS_TRACING_UNSUBSCRIBE => {
            crate::modules::diagnostics_channel::tracing_unsubscribe
        }
        CAP_DIAGNOSTICS_TRACING_TRACE_SYNC => crate::modules::diagnostics_channel::trace_sync,
        CAP_DIAGNOSTICS_BOUNDED_CHANNEL => crate::modules::diagnostics_channel::bounded_channel,
        CAP_DIAGNOSTICS_BOUNDED_SUBSCRIBE => crate::modules::diagnostics_channel::bounded_subscribe,
        CAP_DIAGNOSTICS_BOUNDED_UNSUBSCRIBE => {
            crate::modules::diagnostics_channel::bounded_unsubscribe
        }
        CAP_DIAGNOSTICS_BOUNDED_RUN => crate::modules::diagnostics_channel::bounded_run,
        CAP_DIAGNOSTICS_BOUNDED_SCOPE => crate::modules::diagnostics_channel::bounded_scope,
        CAP_DOMAIN_CREATE => crate::modules::domain::create,
        CAP_DOMAIN_ENTER => crate::modules::domain::enter,
        CAP_DOMAIN_EXIT => crate::modules::domain::exit,
        CAP_DOMAIN_ADD => crate::modules::domain::add,
        CAP_DOMAIN_REMOVE => crate::modules::domain::remove,
        CAP_DOMAIN_RUN => crate::modules::domain::run,
        CAP_DOMAIN_DISPOSE => crate::modules::domain::dispose,
        CAP_DOMAIN_ON => crate::modules::domain::on,
        CAP_DOMAIN_ADD_EMITTER => crate::modules::domain::add_emitter,
        CAP_DOMAIN_ONCE => crate::modules::domain::once,
        CAP_DOMAIN_BIND => crate::modules::domain::bind,
        CAP_DOMAIN_BIND_CALL => crate::modules::domain::bind_call,
        CAP_DOMAIN_INTERCEPT => crate::modules::domain::intercept,
        CAP_DOMAIN_INTERCEPT_CALL => crate::modules::domain::intercept_call,
        CAP_CLUSTER_FORK => crate::modules::cluster::fork,
        CAP_CLUSTER_DISCONNECT => crate::modules::cluster::disconnect_all,
        CAP_CLUSTER_SETUP_PRIMARY | CAP_CLUSTER_SETUP_MASTER => crate::modules::cluster::setup_primary,
        CAP_CLUSTER_SETUP_EVENT => crate::modules::cluster::setup_event,
        CAP_CLUSTER_CLOSE_WORKER_NET => crate::modules::cluster::close_worker_net_binding,
        CAP_CLUSTER_WORKER_IS_DEAD => crate::modules::cluster::is_dead,
        CAP_CLUSTER_WORKER_IS_CONNECTED => crate::modules::cluster::is_connected,
        CAP_CLUSTER_WORKER_ON => crate::modules::cluster::on,
        CAP_CLUSTER_WORKER_EMIT => crate::modules::cluster::emit,
        CAP_CLUSTER_WORKER_DISCONNECT => crate::modules::cluster::disconnect,
        CAP_CLUSTER_WORKER_KILL => crate::modules::cluster::kill,
        CAP_CLUSTER_WORKER_SEND => crate::modules::cluster::send,
        CAP_CLUSTER_WORKER_PROCESS_SEND => crate::modules::cluster::process_send,
        CAP_DIAGNOSTICS_CHANNEL_SCOPE => crate::modules::diagnostics_channel::with_store_scope,
        CAP_DIAGNOSTICS_CHANNEL_RUN_STORES => crate::modules::diagnostics_channel::run_stores,
        CAP_DIAGNOSTICS_SCOPE_DISPOSE => crate::modules::diagnostics_channel::dispose_store_scope,
        _ => return os_buffer_dispatch(cap),
    })
}

fn os_buffer_dispatch(cap: u16) -> Option<CallHandler> {
    use handlers::*;
    Some(match cap {
        CAP_TTY_ISATTY => tty_isatty,
        _ => return crate::dispatch_buffer::buffer_dispatch(cap).or_else(|| process_dispatch(cap)),
    })
}

fn process_dispatch(cap: u16) -> Option<CallHandler> {
    use handlers::*;
    Some(match cap {
        CAP_PROCESS_EXIT => process_exit,
        CAP_PROCESS_KILL => process_kill,
        CAP_PROCESS_CWD => process_cwd,
        CAP_PROCESS_CHDIR => process_chdir,
        CAP_PROCESS_NEXT_TICK => process_next_tick,
        CAP_PROCESS_HRTIME => process_hrtime,
        CAP_PROCESS_HRTIME_BIGINT => process_hrtime_bigint,
        CAP_PROCESS_CPU_USAGE => process_cpu_usage,
        CAP_PROCESS_INITGROUPS => process_initgroups,
        CAP_PROCESS_SETGROUPS => process_setgroups,
        CAP_PROCESS_BINDING_UV_ERRNAME => process_binding_uv_errname,
        CAP_PROCESS_SET_SOURCE_MAPS_ENABLED => process_set_source_maps_enabled,
        CAP_PROCESS_REF => process_ref,
        CAP_PROCESS_UNREF => process_unref,
        CAP_PROCESS_SET_UNCAUGHT_EXCEPTION_CAPTURE_CALLBACK => {
            process_set_uncaught_exception_capture_callback
        }
        CAP_PROCESS_HAS_UNCAUGHT_EXCEPTION_CAPTURE_CALLBACK => {
            process_has_uncaught_exception_capture_callback
        }
        CAP_PROCESS_UPTIME => process_uptime,
        CAP_PROCESS_AVAILABLE_MEMORY => process_available_memory,
        CAP_PROCESS_CONSTRAINED_MEMORY => process_constrained_memory,
        CAP_PROCESS_MEMORY_USAGE => process_memory_usage,
        CAP_PROCESS_GETUID => process_getuid,
        CAP_PROCESS_GETGID => process_getgid,
        CAP_PROCESS_GETEUID => process_geteuid,
        CAP_PROCESS_GETEGID => process_getegid,
        CAP_PROCESS_SETUID => process_setuid,
        CAP_PROCESS_SETGID => process_setgid,
        CAP_PROCESS_SETEUID => process_seteuid,
        CAP_PROCESS_SETEGID => process_setegid,
        CAP_PROCESS_ACTIVE_RESOURCES => process_active_resources,
        CAP_PROCESS_UMASK => process_umask,
        CAP_PROCESS_ON => process_on,
        CAP_PROCESS_ONCE => process_once,
        CAP_PROCESS_EMIT => process_emit,
        CAP_PROCESS_REMOVE_LISTENER => process_remove_listener,
        CAP_PROCESS_REMOVE_ALL_LISTENERS => process_remove_all_listeners,
        CAP_PROCESS_EMIT_WARNING => process_emit_warning,
        CAP_PROCESS_EXIT_CODE_GET => process_exit_code_get,
        CAP_PROCESS_EXIT_CODE_SET => process_exit_code_set,
        CAP_STDOUT_WRITE | CAP_STDERR_WRITE => {
            |state, _receiver, args| crate::modules::process::stream_write(state, args)
        }
        _ => return os_dispatch(cap),
    })
}

fn os_dispatch(cap: u16) -> Option<CallHandler> {
    use handlers::*;
    Some(match cap {
        CAP_OS_PLATFORM => os_platform,
        CAP_OS_ARCH => os_arch,
        CAP_OS_HOSTNAME => os_hostname,
        CAP_OS_TYPE => os_type,
        CAP_OS_RELEASE => os_release,
        CAP_OS_CPUS => os_cpus,
        CAP_OS_TMPDIR => os_tmpdir,
        CAP_OS_HOMEDIR => os_homedir,
        CAP_OS_EOL => os_eol,
        CAP_OS_UPTIME => os_uptime,
        CAP_OS_FREEMEM => os_freemem,
        CAP_OS_TOTALMEM => os_totalmem,
        CAP_OS_LOADAVG => os_loadavg,
        CAP_OS_NETIF => os_network_interfaces,
        CAP_OS_ENDIANNESS => os_endianness,
        CAP_OS_VERSION => os_version,
        CAP_OS_MACHINE => os_machine,
        CAP_OS_USERINFO => os_user_info,
        _ => return network_dispatch(cap),
    })
}

fn network_dispatch(cap: u16) -> Option<CallHandler> {
    use handlers::*;
    Some(match cap {
        CAP_STREAM_PIPELINE => stream_pipeline,
        CAP_DNS_LOOKUP => dns_lookup,
        CAP_DNS_RESOLVE4 => dns_resolve4,
        CAP_HTTP_REQUEST => http_request,
        CAP_HTTP_GET => http_get,
        CAP_HTTPS_REQUEST => http_request,
        CAP_HTTPS_GET => http_get,
        CAP_TLS_CREATE_SECURE_CONTEXT => crate::modules::tls::create_secure_context,
        CAP_TLS_CREATE_SERVER => crate::modules::tls::create_server,
        CAP_TLS_CONNECT => crate::modules::tls::connect,
        CAP_TLS_CONVERT_ALPN => crate::modules::tls::convert_alpn,
        CAP_TLS_GET_CIPHERS => crate::modules::tls::get_ciphers,
        CAP_HTTP_SERVER => http_create_server,
        CAP_HTTP_RES_SET_HEADER => crate::modules::http::res_set_header,
        CAP_HTTP_RES_REMOVE_HEADER => crate::modules::http::res_remove_header,
        CAP_HTTP_RES_CORK => crate::modules::http::res_cork,
        CAP_HTTP_RES_UNCORK => crate::modules::http::res_uncork,
        CAP_HTTP_RES_SET_HEADERS => crate::modules::http::res_set_headers,
        CAP_HTTP_RES_WRITE_HEAD => crate::modules::http::res_write_head,
        CAP_HTTP_RES_WRITE => crate::modules::http::res_write,
        CAP_HTTP_RES_END => crate::modules::http::res_end,
        CAP_HTTP_RES_WRITE_CONTINUE => crate::modules::http::res_write_continue,
        CAP_HTTP_RES_DESTROY => crate::modules::http::res_destroy,
        CAP_HTTP_RES_FLUSH_HEADERS => crate::modules::http::res_flush_headers,
        CAP_HTTP_CONN => crate::modules::http::connection_handler,
        CAP_HTTP_DATA => crate::modules::http::data_handler,
        CAP_HTTP_REQ_WRITE => crate::modules::http_client::req_write,
        CAP_HTTP_REQ_END => crate::modules::http_client::req_end,
        CAP_HTTP_REQ_SET_HEADER => crate::modules::http_client::req_set_header,
        CAP_HTTP_REQ_SET_TIMEOUT => crate::modules::http_client::req_set_timeout,
        CAP_HTTP_REQ_TIMEOUT_FIRE => crate::modules::http_client::req_timeout_fire,
        CAP_HTTP_AGENT => crate::modules::http_client::agent_call,
        CAP_HTTPS_AGENT => crate::modules::http_client::https_agent_call,
        CAP_HTTP_AGENT_GET_NAME => crate::modules::http_client::agent_get_name,
        CAP_HTTP_AGENT_CONNECT => crate::modules::http_client::agent_connect,
        CAP_HTTP_AGENT_ADD_REQUEST => crate::modules::http_client::agent_add_request,
        CAP_HTTP_AGENT_KEEP_SOCKET_ALIVE => crate::modules::http_client::agent_keep_socket_alive,
        CAP_HTTP_RES_SET_TIMEOUT => crate::modules::http_client::res_set_timeout,
        CAP_HTTP_REQ_PATH_GET => crate::modules::http_client::req_path_get,
        CAP_HTTP_REQ_PATH_SET => crate::modules::http_client::req_path_set,
        CAP_HTTP_RES_PIPE => crate::modules::http_client::res_pipe,
        CAP_HTTP_RES_PIPE_DATA => crate::modules::http_client::res_pipe_data,
        CAP_HTTP_RES_PIPE_END => crate::modules::http_client::res_pipe_end,
        CAP_HTTP_REQ_RESUME => crate::modules::http::request_resume,
        CAP_HTTP_REQ_DESTROY => crate::modules::http::request_destroy,
        CAP_HTTP_REQ_ABORT => crate::modules::http_client::req_abort,
        CAP_HTTP_REQ_CLIENT_DESTROY => crate::modules::http_client::req_destroy,
        CAP_HTTP_INCOMING_DESTROY => crate::modules::http::incoming_destroy,
        CAP_HTTP_REQ_SIGNAL_ABORT => crate::modules::http_client::req_signal_abort,
        CAP_HTTP_RES_SET_ENCODING => crate::modules::http_client::res_set_encoding,
        CAP_HTTP_RES_READ => crate::modules::http_client::res_read,
        CAP_HTTP_RESDATA => crate::modules::http_client::data_handler,
        CAP_HTTP_RESEND => crate::modules::http_client::res_end_handler,
        CAP_HTTP_REQCLOSE => crate::modules::http_client::req_close,
        CAP_HTTP_REQ_ERROR => crate::modules::http_client::req_error,
        CAP_HTTP_OUTGOING_WRITE => handlers::http_outgoing_write,
        CAP_HTTP_OUTGOING_END => handlers::http_outgoing_end,
        CAP_HTTP_OUTGOING_DESTROY => handlers::http_outgoing_destroy,
        CAP_NET_CONNECT => net_connect,
        CAP_NET_PIPE_BIND => crate::modules::net::pipe_bind,
        CAP_NET_BOUND_SOCKET_ADDRESS => crate::modules::net::bound_socket_address,
        CAP_NET_BOUND_SOCKET_FD => crate::modules::net::bound_socket_fd,
        CAP_NET_BOUND_SOCKET_CLOSE => crate::modules::net::bound_socket_close,
        CAP_NET_TCP_BIND => crate::modules::net::tcp_bind,
        CAP_NET_SERVER_LISTEN2 => crate::modules::net::server_listen2,
        CAP_NET_LOOKUP_CALLBACK => handlers::net_lookup_callback,
        CAP_VM_MODULE_LINK => handlers::vm_module_link,
        CAP_VM_MODULE_EVALUATE => handlers::vm_module_evaluate,
        CAP_NET_SOCKET => net_socket_call,
        CAP_NET_SERVER => net_create_server_call,
        CAP_NET_ISIP => net_is_ip,
        CAP_NET_ISIPV4 => net_is_ipv4,
        CAP_NET_ISIPV6 => net_is_ipv6,
        CAP_NET_GET_ASF_TIMEOUT => net_get_asf_timeout,
        CAP_NET_SET_ASF_TIMEOUT => net_set_asf_timeout,
        CAP_NET_GET_ASF => net_get_asf,
        CAP_NET_SET_ASF => net_set_asf,
        CAP_NET_SERVER_LISTEN => crate::modules::net::server_listen,
        CAP_NET_SERVER_CLOSE => crate::modules::net::server_close,
        CAP_NET_SERVER_CLOSE_IDLE => crate::modules::net::server_close_idle,
        CAP_NET_SERVER_ADDRESS => crate::modules::net::server_address,
        CAP_NET_SERVER_UNREF => crate::modules::net::server_unref,
        CAP_NET_SERVER_REF => crate::modules::net::server_ref,
        CAP_NET_SERVER_GET_CONNECTIONS => crate::modules::net::server_get_connections,
        CAP_NET_SOCKET_WRITE => crate::modules::net::socket_write,
        CAP_NET_SOCKET_END => crate::modules::net::socket_end,
        CAP_NET_SOCKET_DESTROY => crate::modules::net::socket_destroy,
        CAP_NET_SOCKET_ABORT => crate::modules::net::socket_abort,
        CAP_NET_SOCKET_UNREF => crate::modules::net::socket_unref,
        CAP_NET_SOCKET_REF => crate::modules::net::socket_ref,
        CAP_NET_BLOCK_LIST_ADD_SUBNET => crate::modules::net::block_list_add_subnet,
        CAP_NET_BLOCK_LIST_ADD_ADDRESS => crate::modules::net::block_list_add_address,
        CAP_NET_BLOCK_LIST_CHECK => crate::modules::net::block_list_check,
        CAP_NET_SOCKET_ADDRESS => crate::modules::net::socket_address,
        CAP_NET_SOCKET_SET_NO_DELAY => crate::modules::net::socket_set_no_delay,
        CAP_NET_SOCKET_SET_TOS => crate::modules::net::socket_set_type_of_service,
        CAP_NET_SOCKET_GET_TOS => crate::modules::net::socket_get_type_of_service,
        CAP_NET_SOCKET_HANDLE_CLOSE => crate::modules::net::socket_handle_close,
        CAP_NET_SOCKET_SET_KEEP_ALIVE => crate::modules::net::socket_set_keep_alive,
        CAP_NET_SOCKET_SET_TIMEOUT => crate::modules::net::socket_set_timeout,
        CAP_NET_SOCKET_TIMEOUT_FIRE => crate::modules::net::socket_timeout_fire,
        CAP_NET_SOCKET_SET_ENCODING => crate::modules::net::socket_set_encoding,
        CAP_NET_SOCKET_PAUSE => crate::modules::net::socket_pause,
        CAP_NET_SOCKET_RESUME => crate::modules::net::socket_resume,
        CAP_NET_ASYNC_ITERATOR => crate::modules::net::async_iterator,
        CAP_NET_ASYNC_ITERATOR_NEXT => crate::modules::net::async_iterator_next,
        CAP_NET_ASYNC_ITERATOR_RETURN => crate::modules::net::async_iterator_return,
        CAP_NET_SOCKET_RESET_AND_DESTROY => crate::modules::net::socket_reset_and_destroy,
        CAP_NET_SOCKET_ONREAD => crate::modules::net::socket_onread,
        CAP_REQUIRE => node_require,
        CAP_CJS_WRAP => cjs_wrap,
        CAP_UTIL_GETCALLSITES => util_get_call_sites,
        CAP_BUFFER_ATOB => buffer_atob,
        CAP_BUFFER_BTOA => buffer_btoa,
        CAP_URL_PATH_TO_FILE_URL => url_path_to_file_url,
        CAP_CP_SPAWNSYNC => cp_spawn_sync,
        CAP_CP_SPAWN => cp_spawn,
        CAP_CP_SPAWN_ERROR_EMIT => cp_spawn_error_emit,
        CAP_CP_SPAWN_OUTPUT_EMIT => cp_spawn_output_emit,
        CAP_CP_KILL => cp_kill,
        CAP_CP_STDIN_WRITE => cp_stdin_write,
        CAP_CP_STDIN_END => cp_stdin_end,
        CAP_CP_STDOUT_READ => cp_stdout_read,
        CAP_CP_STREAM_SET_ENCODING => cp_stream_set_encoding,
        CAP_CP_EXEC_COMPLETE => cp_exec_complete,
        CAP_CP_EXEC_ERROR => cp_exec_error,
        CAP_CP_ABORT => cp_abort,
        CAP_CP_ABORT_EMIT => cp_abort_emit,
        CAP_CP_FORK => cp_fork,
        CAP_CP_SEND => cp_send,
        CAP_CP_SEND_ACK => cp_send_ack,
        CAP_CP_MESSAGE_EMIT => cp_message_emit,
        CAP_CP_DISCONNECT => cp_disconnect,
        CAP_CP_DISCONNECT_EMIT => cp_disconnect_emit,
        CAP_CP_INSTANCE_SPAWN => cp_instance_spawn,
        CAP_CP_EXECSYNC => cp_exec_sync,
        CAP_CP_EXEC => cp_async,
        CAP_CP_EXECFILE => cp_exec_file,
        CAP_CP_EXECFILE_ABORT => cp_exec_file_abort,
        CAP_CP_EXECFILE_COMPLETE => cp_exec_file_complete,
        CAP_TEST_RUN => test_run,
        CAP_TEST_DONE => test_done,
        CAP_TEST_SKIP => test_skip,
        CAP_TEST_MOCK_FN => test_mock_fn,
        CAP_TEST_MOCK_CALL => test_mock_call,
        CAP_TEST_MOCK_METHOD => test_mock_method,
        CAP_TEST_MOCK_RESTORE => test_mock_restore,
        CAP_TEST_MOCK_BIND => test_mock_bind,
        CAP_TEST_MOCK_BOUND_CALL => test_mock_bound_call,
        CAP_TEST_MOCK_GETTER => test_mock_getter,
        CAP_TEST_MOCK_SETTER => test_mock_setter,
        CAP_TEST_MOCK_CALL_COUNT => test_mock_call_count,
        CAP_TEST_MOCK_IMPLEMENTATION => test_mock_implementation,
        CAP_TEST_MOCK_IMPLEMENTATION_ONCE => test_mock_implementation_once,
        CAP_TEST_BEFORE_EACH => test_before_each,
        CAP_TEST_AFTER_EACH => test_after_each,
        CAP_TEST_NESTED => test_nested,
        CAP_TEST_MOCK_RESET_CALLS => test_mock_reset_calls,
        CAP_TEST_MOCK_RESET => test_mock_reset,
        CAP_TEST_MOCK_PROPERTY => test_mock_property,
        CAP_TEST_MOCK_ACCESS_COUNT => test_mock_access_count,
        CAP_TEST_MOCK_RESET_ACCESSES => test_mock_reset_accesses,
        CAP_TEST_MOCK_PROPERTY_GET => test_mock_property_get,
        CAP_TEST_MOCK_PROPERTY_SET => test_mock_property_set,
        CAP_TEST_MOCK_PROPERTY_ONCE => test_mock_property_once,
        CAP_TEST_MOCK_TIMERS_ENABLE => test_mock_timers_enable,
        CAP_TEST_MOCK_TIMERS_TICK => test_mock_timers_tick,
        CAP_TEST_MOCK_TIMERS_SETTIME => test_mock_timers_set_time,
        CAP_TEST_MOCK_TIMERS_RESET => test_mock_timers_reset,
        CAP_TEST_MOCK_MODULE => test_mock_module,
        CAP_TEST_CONTEXT_SKIP => test_context_skip,
        CAP_TEST_CONTEXT_TODO => test_context_todo,
        CAP_TEST_RUN_EMIT => test_run_emit,
        CAP_TEST_GET_CONTEXT => test_get_context,
        CAP_EVENT_TRUSTED_GET => event_trusted_get,
        CAP_ABORT_CONTROLLER_SIGNAL_GET => abort_controller_signal_get,
        CAP_ABORT_SIGNAL_ABORTED_GET => abort_signal_aborted_get,
        CAP_ABORT_SIGNAL_HAS_INSTANCE => abort_signal_has_instance,
        CAP_ABORT_SIGNAL_THROW_IF_ABORTED => abort_signal_throw_if_aborted,
        CAP_STRUCTURED_CLONE => structured_clone,
        CAP_FETCH => fetch,
        CAP_GC => gc,
        CAP_VM_RUN_IN_NEW_CONTEXT => crate::modules::vm::run_in_new_context,
        CAP_VM_CREATE_CONTEXT => crate::modules::vm::create_context,
        CAP_VM_RUN_IN_CONTEXT => crate::modules::vm::run_in_context,
        CAP_VM_IS_CONTEXT => crate::modules::vm::is_context,
        CAP_READLINE => crate::modules::readline::create_interface,
        CAP_READLINE_DRIVER => crate::modules::readline::driver_handler,
        CAP_READLINE_DONE => crate::modules::readline::done_handler,
        CAP_ASYNC_EXECUTION_ID => crate::modules::async_hooks::execution_id,
        CAP_ASYNC_TRIGGER_ID => crate::modules::async_hooks::trigger_id,
        CAP_ASYNC_EXECUTION_RESOURCE => crate::modules::async_hooks::execution_resource,
        CAP_ASYNC_CREATE_HOOK => crate::modules::async_hooks::create_hook,
        CAP_ASYNC_RESOURCE_RUN => crate::modules::async_hooks::resource_run,
        CAP_ASYNC_RESOURCE_BEFORE => crate::modules::async_hooks::resource_before,
        CAP_ASYNC_RESOURCE_AFTER => crate::modules::async_hooks::resource_after,
        CAP_ASYNC_RESOURCE_DESTROY => crate::modules::async_hooks::resource_destroy,
        CAP_ASYNC_RESOURCE_ID => crate::modules::async_hooks::resource_id,
        CAP_ASYNC_RESOURCE_TRIGGER => crate::modules::async_hooks::resource_trigger,
        CAP_ASYNC_RESOURCE_DOMAIN => crate::modules::async_hooks::resource_domain,
        CAP_ASYNC_RESOURCE_BIND => crate::modules::async_hooks::resource_bind,
        CAP_ASYNC_RESOURCE_STATIC_BIND => crate::modules::async_hooks::resource_static_bind,
        CAP_ASYNC_HOOK_ENABLE => crate::modules::async_hooks::hook_enable,
        CAP_ASYNC_HOOK_DISABLE => crate::modules::async_hooks::hook_disable,
        CAP_ASYNC_LOCAL_GET => crate::modules::async_hooks::local_get_store,
        CAP_ASYNC_LOCAL_RUN => crate::modules::async_hooks::local_run,
        CAP_ASYNC_LOCAL_ENTER => crate::modules::async_hooks::local_enter_with,
        CAP_ASYNC_LOCAL_DISABLE => crate::modules::async_hooks::local_disable,
        CAP_ASYNC_LOCAL_EXIT => crate::modules::async_hooks::local_exit,
        CAP_ASYNC_LOCAL_SCOPE => crate::modules::async_hooks::local_scope,
        CAP_ASYNC_LOCAL_SCOPE_DISPOSE => crate::modules::async_hooks::local_scope_dispose,
        CAP_ASYNC_LOCAL_BIND => crate::modules::async_hooks::local_bind,
        CAP_ASYNC_LOCAL_BIND_CALL => crate::modules::async_hooks::local_bind_call,
        CAP_ASYNC_LOCAL_SNAPSHOT => crate::modules::async_hooks::local_snapshot,
        CAP_ASYNC_LOCAL_SNAPSHOT_CALL => crate::modules::async_hooks::local_snapshot_call,
        CAP_ASYNC_WORKER_RESOURCE => crate::modules::async_hooks::worker_resource,
        CAP_INSPECTOR_CONNECT => crate::modules::inspector::connect,
        CAP_INSPECTOR_CONNECT_MAIN => crate::modules::inspector::connect,
        CAP_INSPECTOR_DISCONNECT => crate::modules::inspector::disconnect,
        CAP_INSPECTOR_POST => crate::modules::inspector::post,
        CAP_INSPECTOR_OPEN => crate::modules::inspector::open,
        CAP_INSPECTOR_CLOSE | CAP_INSPECTOR_WAIT => crate::modules::inspector::noop,
        CAP_WASI_START => crate::modules::wasi::start,
        CAP_WASI_INITIALIZE => crate::modules::wasi::initialize,
        CAP_WASI_IMPORT_OBJECT => crate::modules::wasi::import_object,
        CAP_ZLIB_GZIP => crate::modules::zlib::gzip,
        CAP_ZLIB_GUNZIP => crate::modules::zlib::gunzip,
        CAP_ZLIB_DEFLATE_RAW => crate::modules::zlib::deflate_raw,
        CAP_ZLIB_INFLATE_RAW => crate::modules::zlib::inflate_raw,
        CAP_ZLIB_DEFLATE => crate::modules::zlib::deflate,
        CAP_ZLIB_INFLATE => crate::modules::zlib::inflate,
        _ => return fs_dispatch(cap),
    })
}

pub fn assert_dispatch(cap: u16) -> Option<CallHandler> {
    use crate::modules::{assert, assert_validate};
    Some(match cap {
        CAP_ASSERT_OK => assert::ok,
        CAP_ASSERT_STRICT_EQUAL => assert::strict_equal,
        CAP_ASSERT_NOT_STRICT_EQUAL => assert::not_strict_equal,
        CAP_ASSERT_EQUAL => assert::equal,
        CAP_ASSERT_NOT_EQUAL => assert::not_equal,
        CAP_ASSERT_DEEP_STRICT_EQUAL => assert::deep_strict_equal,
        CAP_ASSERT_DEEP_EQUAL => assert::deep_equal,
        CAP_ASSERT_NOT_DEEP_EQUAL => assert::not_deep_equal,
        CAP_ASSERT_PARTIAL_DEEP_STRICT_EQUAL => assert::partial_deep_strict_equal,
        CAP_ASSERT_NOT_DEEP_STRICT_EQUAL => assert::not_deep_strict_equal,
        CAP_ASSERT_THROWS => assert_validate::throws,
        CAP_ASSERT_DOES_NOT_THROW => assert_validate::does_not_throw,
        CAP_ASSERT_FAIL => assert::fail,
        CAP_ASSERT_IF_ERROR => assert::if_error,
        CAP_ASSERT_MATCH => assert_validate::matches,
        CAP_ASSERT_DOES_NOT_MATCH => assert_validate::does_not_match,
        CAP_ASSERT_CONSTRUCTOR => crate::modules::assert::constructor_call,
        _ => return None,
    })
}

/// Single canonical mapping from capability id to construct handler.
pub fn lookup_construct(cap: u16) -> Option<ConstructHandler> {
    use handlers::*;
    Some(match cap {
        CAP_VM_RUN_IN_NEW_CONTEXT => crate::modules::vm::construct_run_in_new_context,
        CAP_INTERNAL_JS_STREAM => internal_js_stream_construct,
        CAP_VM_SOURCE_TEXT_MODULE => vm_source_text_module_construct,
        CAP_EVENTS_NEW => events_new,
        CAP_EVENT_TARGET_NEW => crate::modules::event_target::new_target,
        CAP_NODE_EVENT_TARGET_NEW => crate::modules::event_target::new_node_target,
        CAP_MESSAGE_CHANNEL => crate::dispatch_handlers::message_channel_construct,
        CAP_STREAM_READABLE => stream_readable,
        CAP_STREAM_WRITABLE => stream_writable,
        CAP_STREAM_DUPLEX => stream_duplex,
        CAP_STREAM_TRANSFORM => stream_transform,
        CAP_STRING_DECODER => string_decoder_new,
        CAP_URL_NEW => url_new,
        CAP_URL_LEGACY_NEW => url_legacy_new,
        CAP_URL_PATTERN => crate::modules::url::url_pattern_construct,
        CAP_TEXT_DECODER_NEW => crate::modules::text_decoder::new_text_decoder,
        CAP_TEXT_ENCODER_NEW => crate::modules::text_encoder::new_text_encoder,
        CAP_URL_SEARCH => url_search_params,
        CAP_NET_SERVER => net_create_server,
        CAP_NET_CONNECT => crate::modules::net::socket_construct,
        CAP_NET_SOCKET => crate::modules::net::socket_construct,
        CAP_NET_PIPE => crate::modules::net::pipe_construct,
        CAP_NET_BOUND_SOCKET => crate::modules::net::bound_socket_construct,
        CAP_NET_TCP => crate::modules::net::tcp_construct,
        CAP_NET_BLOCK_LIST => crate::modules::net::block_list_construct,
        CAP_HTTP_SERVER => http_create_server_construct,
        CAP_HTTP_AGENT => crate::modules::http_client::agent_construct,
        CAP_HTTPS_AGENT => crate::modules::http_client::https_agent_construct,
        CAP_TTY_READ_STREAM | CAP_TTY_WRITE_STREAM => crate::modules::tty::stream_construct,
        CAP_HTTP_CLIENT_REQUEST => crate::modules::http_client::request,
        CAP_HTTP_INCOMING => crate::modules::http::incoming_construct,
        CAP_HTTP_OUTGOING => handlers::http_outgoing_construct,
        CAP_FS_CREATE_READSTREAM | CAP_FS_READSTREAM => {
            crate::modules::fs::construct_read_stream
        }
        CAP_BUFFER_NEW => buffer_new_construct,
        CAP_READLINE => readline_create_interface,
        CAP_ASYNC_RESOURCE => crate::modules::async_hooks::new_resource,
        CAP_ASYNC_LOCAL_STORAGE => crate::modules::async_hooks::new_async_local_storage,
        CAP_INSPECTOR_SESSION => crate::modules::inspector::new_session,
        CAP_WASI_CONSTRUCTOR => crate::modules::wasi::new_wasi,
        CAP_DIAGNOSTICS_CHANNEL_CONSTRUCTOR => crate::modules::diagnostics_channel::new_channel,
        CAP_DOMAIN_CONSTRUCTOR => crate::modules::domain::new_domain,
        CAP_ABORT_CONTROLLER => abort_controller_new,
        CAP_ABORT_SIGNAL => abort_signal_new,
        CAP_ABORT_SIGNAL_TIMEOUT => abort_signal_timeout,
        CAP_ABORT_SIGNAL_ANY => abort_signal_any,
        CAP_EVENT => handlers::event_new,
        CAP_CUSTOM_EVENT => handlers::custom_event_new,
        CAP_ASSERT_CONSTRUCTOR => crate::modules::assert::constructor_new,
        CAP_ASSERTION_ERROR_CONSTRUCTOR => crate::modules::assert::assertion_error_constructor,
        CAP_FS_DIR => crate::modules::fs::dir_construct,
        CAP_TEST_MOCK_CALL => test_mock_construct,
        CAP_CP_CONSTRUCTOR => cp_constructor,
        CAP_DNS_EXCEPTION => crate::modules::dns::dns_exception,
        _ => return None,
    })
}
