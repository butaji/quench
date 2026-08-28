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
use quench_runtime::ops::{HostCapabilityKind, HostCapabilityRef, RealmId};

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

/// Declare a family of host facts once. The same `NodeSpec` values feed
/// namespace construction, capability dispatch, and future generated
/// evidence tables; mechanical registrations should not repeat ids inline.
macro_rules! node_api {
    ($(($name:ident, $cap_name:ident, $label:literal, $cap:expr)),* $(,)?) => {
        $(
            pub const $name: NodeSpec = NodeSpec::new($label, $cap);
            pub const $cap_name: CapId = $cap;
        )*
    };
    ($(($name:ident, $label:literal, $cap:expr)),* $(,)?) => {
        $(pub const $name: NodeSpec = NodeSpec::new($label, $cap);)*
    };
}

impl NodeSpec {
    pub const fn new(name: &'static str, cap: CapId) -> Self {
        Self { name, cap }
    }
}

/// All Node host capabilities. Stable ids; do not reorder.
node_api! {
    (SPEC_TEST, "test:test", 0x1b00),
    (SPEC_TEST_SKIP, "test:skip", 0x1b01),
    (SPEC_TEST_MOCK_FN, "test:mock:fn", 0x1b02),
    (SPEC_TEST_MOCK_CALL, "test:mock:call", 0x1b03),
    (SPEC_EVENT_TRUSTED_GET, "event:isTrusted:get", 0x1b04),
    (SPEC_TEST_MOCK_METHOD, "test:mock:method", 0x1b05),
    (SPEC_TEST_MOCK_RESTORE, "test:mock:restore", 0x1b06),
    (SPEC_TEST_MOCK_BIND, "test:mock:bind", 0x1b07),
    (SPEC_TEST_MOCK_BOUND_CALL, "test:mock:bound-call", 0x1b08),
    (SPEC_TEST_MOCK_GETTER, "test:mock:getter", 0x1b09),
    (SPEC_TEST_MOCK_SETTER, "test:mock:setter", 0x1b0A),
    (SPEC_TEST_MOCK_CALL_COUNT, "test:mock:callCount", 0x1b0B),
    (SPEC_TEST_MOCK_IMPLEMENTATION, "test:mock:implementation", 0x1b0C),
    (SPEC_TEST_MOCK_IMPLEMENTATION_ONCE, "test:mock:implementationOnce", 0x1b0D),
    (SPEC_TEST_BEFORE_EACH, "test:beforeEach", 0x1b0E),
    (SPEC_TEST_AFTER_EACH, "test:afterEach", 0x1b0F),
    (SPEC_TEST_NESTED, "test:nested", 0x1b10),
    (SPEC_TEST_MOCK_RESET_CALLS, "test:mock:resetCalls", 0x1b11),
    (SPEC_TEST_MOCK_RESET, "test:mock:reset", 0x1b12),
    (SPEC_TEST_MOCK_PROPERTY, "test:mock:property", 0x1b13),
    (SPEC_TEST_MOCK_ACCESS_COUNT, "test:mock:accessCount", 0x1b14),
    (SPEC_TEST_MOCK_RESET_ACCESSES, "test:mock:resetAccesses", 0x1b15),
    (SPEC_TEST_MOCK_PROPERTY_GET, "test:mock:propertyGet", 0x1b16),
    (SPEC_TEST_MOCK_PROPERTY_SET, "test:mock:propertySet", 0x1b17),
    (SPEC_TEST_MOCK_PROPERTY_ONCE, "test:mock:propertyOnce", 0x1b18),
    (SPEC_TEST_MOCK_TIMERS_ENABLE, "test:mock:timers:enable", 0x1b19),
    (SPEC_TEST_MOCK_TIMERS_TICK, "test:mock:timers:tick", 0x1b1A),
    (SPEC_TEST_MOCK_TIMERS_SETTIME, "test:mock:timers:setTime", 0x1b1B),
    (SPEC_TEST_MOCK_TIMERS_RESET, "test:mock:timers:reset", 0x1b1C),
    (SPEC_TEST_MOCK_MODULE, "test:mock:module", 0x1b1D),
    (SPEC_TEST_CONTEXT_SKIP, "test:context:skip", 0x1b1E),
    (SPEC_TEST_CONTEXT_TODO, "test:context:todo", 0x1b1F),
    (SPEC_TEST_RUN_EMIT, "test:run:emit", 0x1b20),
    (SPEC_TEST_GET_CONTEXT, "test:getTestContext", 0x1b21),
}

node_api! {
    (SPEC_INTERNAL_JS_STREAM, "internal:js_stream", 0x0F12),
    (SPEC_VM_SOURCE_TEXT_MODULE, "vm:SourceTextModule", 0x0F11),
    (SPEC_VM_MODULE_LINK, "vm:SourceTextModule:link", 0x0F13),
    (SPEC_VM_MODULE_EVALUATE, "vm:SourceTextModule:evaluate", 0x0F14),
    (SPEC_TEXT_DECODER_NEW, "TextDecoder:new", 0x0809),
    (SPEC_TEXT_DECODER_DECODE, "TextDecoder:decode", 0x080A),
    (SPEC_TEXT_ENCODER_NEW, "TextEncoder:new", 0x084C),
    (SPEC_TEXT_ENCODER_ENCODE, "TextEncoder:encode", 0x084D),
    (SPEC_TEXT_ENCODER_ENCODE_INTO, "TextEncoder:encodeInto", 0x084E),
    (SPEC_UTIL_FORMAT, "util:format", 0x0300),
    (SPEC_UTIL_INSPECT, "util:inspect", 0x0301),
    (SPEC_UTIL_ABORTED, "util:aborted", 0x0310),
    (SPEC_UTIL_ABORTED_RESOLVE, "util:aborted:resolve", 0x0311),
    (SPEC_UTIL_TYPES, "util:types", 0x0302),
    (SPEC_UTIL_GETCALLSITES, "util:getCallSites", 0x0303),
    (SPEC_UTIL_IS, "util:is", 0x030D),
    (SPEC_UTIL_INHERITS, "util:inherits", 0x0304),
    (SPEC_UTIL_STRIP_VT, "util:stripVTControlCharacters", 0x0305),
    (SPEC_UTIL_FORMAT_WITH_OPTIONS, "util:formatWithOptions", 0x0306),
    (SPEC_UTIL_STYLE_TEXT, "util:styleText", 0x0307),
    (SPEC_UTIL_IS_DEEP_STRICT_EQUAL, "util:isDeepStrictEqual", 0x0308),
    (SPEC_UTIL_TO_USV_STRING, "util:toUSVString", 0x0309),
    (SPEC_UTIL_IS_NATIVE_ERROR, "util.types:isNativeError", 0x030A),
    (SPEC_UTIL_PARSE_ENV, "util:parseEnv", 0x030B),
    (SPEC_UTIL_TYPE_PREDICATE, "util.types:predicate", 0x030C),
    (SPEC_CONSOLE_LOG, "console:log", 0x0200),
    (SPEC_CONSOLE_INFO, "console:info", 0x0201),
    (SPEC_CONSOLE_WARN, "console:warn", 0x0202),
    (SPEC_CONSOLE_ERROR, "console:error", 0x0203),
    (SPEC_CONSOLE_DEBUG, "console:debug", 0x0204),
    (SPEC_CONSOLE_TRACE, "console:trace", 0x0205),
}
node_api! {
    (SPEC_EVENTS_NEW, CAP_EVENTS_NEW, "events:EventEmitter", 0x0100),
    (SPEC_EVENTS_FROM, CAP_EVENTS_FROM, "events:from", 0x0101),
    (SPEC_EVENTS_ON, CAP_EVENTS_ON, "events:on", 0x0102),
    (SPEC_EVENTS_EMIT, CAP_EVENTS_EMIT, "events:emit", 0x0103),
    (SPEC_EVENTS_CAPTURE_GET, CAP_EVENTS_CAPTURE_GET, "events:captureRejections:get", 0x0104),
    (SPEC_EVENTS_CAPTURE_SET, CAP_EVENTS_CAPTURE_SET, "events:captureRejections:set", 0x0119),
    (SPEC_EVENTS_DEFAULT_MAX_GET, CAP_EVENTS_DEFAULT_MAX_GET, "events:defaultMaxListeners:get", 0x0125),
    (SPEC_EVENTS_DEFAULT_MAX_SET, CAP_EVENTS_DEFAULT_MAX_SET, "events:defaultMaxListeners:set", 0x0126),
    (SPEC_EVENTS_RAW_LISTENERS, CAP_EVENTS_RAW_LISTENERS, "events:rawListeners", 0x0127),
    (SPEC_EVENTS_ONCE, CAP_EVENTS_ONCE, "events:once", 0x0105),
    (SPEC_EVENTS_REMOVE_LISTENER, CAP_EVENTS_REMOVE_LISTENER, "events:removeListener", 0x0106),
    (SPEC_EVENTS_REMOVE_ALL, CAP_EVENTS_REMOVE_ALL, "events:removeAllListeners", 0x0107),
    (SPEC_EVENTS_LISTENERS, CAP_EVENTS_LISTENERS, "events:listeners", 0x0108),
    (SPEC_EVENTS_EVENT_NAMES, CAP_EVENTS_EVENT_NAMES, "events:eventNames", 0x0109),
    (SPEC_EVENTS_LISTENER_COUNT, CAP_EVENTS_LISTENER_COUNT, "events:listenerCount", 0x010A),
    (SPEC_EVENTS_PREPEND, CAP_EVENTS_PREPEND, "events:prependListener", 0x010B),
    (SPEC_EVENTS_PREPEND_ONCE, CAP_EVENTS_PREPEND_ONCE, "events:prependOnceListener", 0x010C),
    (SPEC_EVENTS_SET_MAX, CAP_EVENTS_SET_MAX, "events:setMaxListeners", 0x010D),
    (SPEC_EVENTS_GET_MAX, CAP_EVENTS_GET_MAX, "events:getMaxListeners", 0x010E),
    (SPEC_EVENTS_SET_MAX_STATIC, CAP_EVENTS_SET_MAX_STATIC, "events:setMaxListeners:static", 0x010F),
    (SPEC_EVENTS_GET_LISTENERS, CAP_EVENTS_GET_LISTENERS, "events:getEventListeners", 0x0110),
    (SPEC_EVENTS_LISTENER_COUNT_STATIC, CAP_EVENTS_LISTENER_COUNT_STATIC, "events:listenerCount:static", 0x0111),
    (SPEC_EVENTS_GET_MAX_STATIC, CAP_EVENTS_GET_MAX_STATIC, "events:getMaxListeners:static", 0x0112),
    (SPEC_TARGET_ADD, CAP_TARGET_ADD, "eventTarget:addEventListener", 0x0113),
    (SPEC_TARGET_REMOVE, CAP_TARGET_REMOVE, "eventTarget:removeEventListener", 0x0114),
    (SPEC_TARGET_DISPATCH, CAP_TARGET_DISPATCH, "eventTarget:dispatchEvent", 0x0115),
    (SPEC_EVENT_TARGET_NEW, CAP_EVENT_TARGET_NEW, "eventTarget:EventTarget", 0x0116),
    (SPEC_RUN_UNCAUGHT, CAP_RUN_UNCAUGHT, "process:runUncaught", 0x0117),
    (SPEC_EVENT_TARGET_REJECTION, CAP_EVENT_TARGET_REJECTION, "eventTarget:promiseRejection", 0x0128),
    (SPEC_EVENTS_ABORT_LISTENER, CAP_EVENTS_ABORT_LISTENER, "events:addAbortListener:listener", 0x0130),
    (SPEC_EVENTS_ABORT_DISPOSE, CAP_EVENTS_ABORT_DISPOSE, "events:addAbortListener:dispose", 0x0131),
    (SPEC_EVENTS_ADD_ABORT, CAP_EVENTS_ADD_ABORT, "events:addAbortListener", 0x0132),
}
node_api! {
    (SPEC_DIAGNOSTICS_CHANNEL, "diagnostics_channel:channel", 0x1F00),
    (SPEC_DIAGNOSTICS_SUBSCRIBE, "diagnostics_channel:subscribe", 0x1F01),
    (SPEC_DIAGNOSTICS_UNSUBSCRIBE, "diagnostics_channel:unsubscribe", 0x1F02),
    (SPEC_DIAGNOSTICS_HAS_SUBSCRIBERS, "diagnostics_channel:hasSubscribers", 0x1F03),
    (SPEC_DIAGNOSTICS_CHANNEL_CONSTRUCTOR, "diagnostics_channel:Channel", 0x1F04),
    (SPEC_DIAGNOSTICS_CHANNEL_SUBSCRIBE, "diagnostics_channel:Channel:subscribe", 0x1F05),
    (SPEC_DIAGNOSTICS_CHANNEL_UNSUBSCRIBE, "diagnostics_channel:Channel:unsubscribe", 0x1F06),
    (SPEC_DIAGNOSTICS_CHANNEL_PUBLISH, "diagnostics_channel:Channel:publish", 0x1F07),
    (SPEC_DIAGNOSTICS_CHANNEL_BIND_STORE, "diagnostics_channel:Channel:bindStore", 0x1F08),
    (SPEC_DIAGNOSTICS_CHANNEL_UNBIND_STORE, "diagnostics_channel:Channel:unbindStore", 0x1F09),
    (SPEC_DOMAIN_CREATE, "domain:create", 0x1F50),
    (SPEC_DOMAIN_CONSTRUCTOR, "domain:Domain", 0x1F51),
    (SPEC_DOMAIN_ENTER, "domain:enter", 0x1F52),
    (SPEC_DOMAIN_EXIT, "domain:exit", 0x1F53),
    (SPEC_DOMAIN_ADD, "domain:add", 0x1F54),
    (SPEC_DOMAIN_REMOVE, "domain:remove", 0x1F55),
    (SPEC_DOMAIN_RUN, "domain:run", 0x1F56),
    (SPEC_DOMAIN_DISPOSE, "domain:dispose", 0x1F57),
    (SPEC_DOMAIN_ON, "domain:on", 0x1F58),
    (SPEC_DOMAIN_ADD_EMITTER, "domain:addEmitter", 0x1F59),
    (SPEC_CLUSTER_FORK, "cluster:fork", 0x1F40),
    (SPEC_CLUSTER_DISCONNECT, "cluster:disconnect", 0x1F41),
    (SPEC_CLUSTER_WORKER_IS_DEAD, "cluster:Worker:isDead", 0x1F42),
    (SPEC_CLUSTER_WORKER_IS_CONNECTED, "cluster:Worker:isConnected", 0x1F43),
    (SPEC_CLUSTER_WORKER_ON, "cluster:Worker:on", 0x1F44),
    (SPEC_CLUSTER_WORKER_EMIT, "cluster:Worker:emit", 0x1F45),
    (SPEC_CLUSTER_WORKER_DISCONNECT, "cluster:Worker:disconnect", 0x1F46),
    (SPEC_CLUSTER_WORKER_KILL, "cluster:Worker:kill", 0x1F47),
    (SPEC_DIAGNOSTICS_TRACING_CHANNEL, "diagnostics_channel:tracingChannel", 0x1F0A),
    (SPEC_DIAGNOSTICS_TRACING_SUBSCRIBE, "diagnostics_channel:TracingChannel:subscribe", 0x1F0B),
    (SPEC_DIAGNOSTICS_TRACING_UNSUBSCRIBE, "diagnostics_channel:TracingChannel:unsubscribe", 0x1F0C),
    (SPEC_DIAGNOSTICS_TRACING_TRACE_SYNC, "diagnostics_channel:TracingChannel:traceSync", 0x1F0D),
    (SPEC_DIAGNOSTICS_BOUNDED_CHANNEL, "diagnostics_channel:boundedChannel", 0x1F0E),
    (SPEC_DIAGNOSTICS_BOUNDED_SUBSCRIBE, "diagnostics_channel:BoundedChannel:subscribe", 0x1F0F),
    (SPEC_DIAGNOSTICS_BOUNDED_UNSUBSCRIBE, "diagnostics_channel:BoundedChannel:unsubscribe", 0x1F10),
    (SPEC_DIAGNOSTICS_BOUNDED_RUN, "diagnostics_channel:BoundedChannel:run", 0x1F11),
    (SPEC_DIAGNOSTICS_CHANNEL_SCOPE, "diagnostics_channel:Channel:withStoreScope", 0x1F12),
    (SPEC_DIAGNOSTICS_SCOPE_DISPOSE, "diagnostics_channel:StoreScope:dispose", 0x1F13),
    (SPEC_ASYNC_RESOURCE, "async_hooks:AsyncResource", 0x1410),
    (SPEC_ASYNC_EXECUTION_ID, "async_hooks:executionAsyncId", 0x1411),
    (SPEC_ASYNC_TRIGGER_ID, "async_hooks:triggerAsyncId", 0x1412),
    (SPEC_ASYNC_EXECUTION_RESOURCE, "async_hooks:executionAsyncResource", 0x1413),
    (SPEC_ASYNC_CREATE_HOOK, "async_hooks:createHook", 0x1414),
    (SPEC_ASYNC_RESOURCE_RUN, "async_hooks:resource:runInAsyncScope", 0x1415),
    (SPEC_ASYNC_RESOURCE_BEFORE, "async_hooks:resource:emitBefore", 0x1416),
    (SPEC_ASYNC_RESOURCE_AFTER, "async_hooks:resource:emitAfter", 0x1417),
    (SPEC_ASYNC_RESOURCE_DESTROY, "async_hooks:resource:emitDestroy", 0x1418),
    (SPEC_ASYNC_RESOURCE_ID, "async_hooks:resource:asyncId", 0x1419),
    (SPEC_ASYNC_RESOURCE_TRIGGER, "async_hooks:resource:triggerAsyncId", 0x141A),
    (SPEC_ASYNC_HOOK_ENABLE, "async_hooks:hook:enable", 0x141B),
    (SPEC_ASYNC_HOOK_DISABLE, "async_hooks:hook:disable", 0x141C),
    (SPEC_ASYNC_LOCAL_STORAGE, "async_hooks:AsyncLocalStorage", 0x1F30),
    (SPEC_ASYNC_LOCAL_GET, "async_hooks:AsyncLocalStorage:getStore", 0x1F31),
    (SPEC_ASYNC_LOCAL_RUN, "async_hooks:AsyncLocalStorage:run", 0x1F32),
    (SPEC_ASYNC_LOCAL_ENTER, "async_hooks:AsyncLocalStorage:enterWith", 0x1F33),
    (SPEC_ASYNC_LOCAL_DISABLE, "async_hooks:AsyncLocalStorage:disable", 0x1F34),
    (SPEC_ASYNC_WORKER_RESOURCE, "async_hooks:workerResource", 0x1F35),
    (SPEC_INSPECTOR_SESSION, "inspector:Session", 0x1500),
    (SPEC_INSPECTOR_CONNECT, "inspector:Session:connect", 0x1501),
    (SPEC_INSPECTOR_CONNECT_MAIN, "inspector:Session:connectToMainThread", 0x1502),
    (SPEC_INSPECTOR_DISCONNECT, "inspector:Session:disconnect", 0x1503),
    (SPEC_INSPECTOR_POST, "inspector:Session:post", 0x1504),
    (SPEC_INSPECTOR_OPEN, "inspector:open", 0x1505),
    (SPEC_INSPECTOR_CLOSE, "inspector:close", 0x1506),
    (SPEC_INSPECTOR_WAIT, "inspector:waitForDebugger", 0x1507),
    (SPEC_WASI_CONSTRUCTOR, "wasi:WASI", 0x1C00),
    (SPEC_WASI_START, "wasi:WASI:start", 0x1C01),
    (SPEC_WASI_INITIALIZE, "wasi:WASI:initialize", 0x1C02),
    (SPEC_WASI_IMPORT_OBJECT, "wasi:WASI:getImportObject", 0x1C03),
}

node_api! {
    (SPEC_PATH_JOIN, CAP_PATH_JOIN, "path:join", 0x0400),
    (SPEC_PATH_RESOLVE, CAP_PATH_RESOLVE, "path:resolve", 0x0401),
    (SPEC_PATH_NORMALIZE, CAP_PATH_NORMALIZE, "path:normalize", 0x0402),
    (SPEC_PATH_DIRNAME, CAP_PATH_DIRNAME, "path:dirname", 0x0403),
    (SPEC_PATH_BASENAME, CAP_PATH_BASENAME, "path:basename", 0x0404),
    (SPEC_PATH_EXTNAME, CAP_PATH_EXTNAME, "path:extname", 0x0405),
    (SPEC_PATH_ISABSOLUTE, CAP_PATH_ISABSOLUTE, "path:isAbsolute", 0x0406),
    (SPEC_PATH_RELATIVE, CAP_PATH_RELATIVE, "path:relative", 0x0409),
    (SPEC_PATH_PARSE, CAP_PATH_PARSE, "path:parse", 0x040A),
    (SPEC_PATH_FORMAT, CAP_PATH_FORMAT, "path:format", 0x040B),
    (SPEC_PATH_TO_NAMESPACED, CAP_PATH_TO_NAMESPACED, "path:toNamespacedPath", 0x040C),
    (SPEC_PATH_MATCHES_GLOB, CAP_PATH_MATCHES_GLOB, "path:matchesGlob", 0x040D),
    (SPEC_PATH_WIN32_JOIN, CAP_PATH_WIN32_JOIN, "path.win32:join", 0x0410),
    (SPEC_PATH_WIN32_RESOLVE, CAP_PATH_WIN32_RESOLVE, "path.win32:resolve", 0x0411),
    (SPEC_PATH_WIN32_NORMALIZE, CAP_PATH_WIN32_NORMALIZE, "path.win32:normalize", 0x0412),
    (SPEC_PATH_WIN32_DIRNAME, CAP_PATH_WIN32_DIRNAME, "path.win32:dirname", 0x0413),
    (SPEC_PATH_WIN32_BASENAME, CAP_PATH_WIN32_BASENAME, "path.win32:basename", 0x0414),
    (SPEC_PATH_WIN32_EXTNAME, CAP_PATH_WIN32_EXTNAME, "path.win32:extname", 0x0415),
    (SPEC_PATH_WIN32_ISABSOLUTE, CAP_PATH_WIN32_ISABSOLUTE, "path.win32:isAbsolute", 0x0416),
    (SPEC_PATH_WIN32_RELATIVE, CAP_PATH_WIN32_RELATIVE, "path.win32:relative", 0x0417),
    (SPEC_PATH_WIN32_PARSE, CAP_PATH_WIN32_PARSE, "path.win32:parse", 0x0418),
    (SPEC_PATH_WIN32_FORMAT, CAP_PATH_WIN32_FORMAT, "path.win32:format", 0x0419),
    (SPEC_PATH_WIN32_TO_NAMESPACED, CAP_PATH_WIN32_TO_NAMESPACED, "path.win32:toNamespacedPath", 0x041A),
    (SPEC_PATH_WIN32_MATCHES_GLOB, CAP_PATH_WIN32_MATCHES_GLOB, "path.win32:matchesGlob", 0x041B),
}

node_api! {
    (SPEC_URL_PARSE, CAP_URL_PARSE, "url:parse", 0x0500),
    (SPEC_URL_FORMAT, CAP_URL_FORMAT, "url:format", 0x0501),
    (SPEC_URL_RESOLVE, CAP_URL_RESOLVE, "url:resolve", 0x0502),
    (SPEC_URL_NEW, CAP_URL_NEW, "url:URL", 0x0503),
    (SPEC_URL_LEGACY_NEW, CAP_URL_LEGACY_NEW, "url:Url", 40),
    (SPEC_URL_RESOLVE_OBJECT, CAP_URL_RESOLVE_OBJECT, "url:resolveObject", 0x0520),
    (SPEC_URL_SEARCHPARAMS_NEW, CAP_URL_SEARCH, "url:URLSearchParams", 0x0504),
}

node_api! {
    (SPEC_QS_PARSE, CAP_QS_PARSE, "querystring:parse", 0x0600),
    (SPEC_QS_STRINGIFY, CAP_QS_STRINGIFY, "querystring:stringify", 0x0601),
    (SPEC_QS_ESCAPE, CAP_QS_ESCAPE, "querystring:escape", 0x0602),
    (SPEC_QS_UNESCAPE, CAP_QS_UNESCAPE, "querystring:unescape", 0x0603),
    (SPEC_QS_UNESCAPE_BUFFER, CAP_QS_UNESCAPE_BUFFER, "querystring:unescapeBuffer", 0x0604),
}

node_api! {
    (SPEC_TIMERS_SETTIMEOUT, CAP_TIMERS_SETTIMEOUT, "timers:setTimeout", 0x0700),
    (SPEC_TIMERS_CLEARTIMEOUT, CAP_TIMERS_CLEARTIMEOUT, "timers:clearTimeout", 0x0701),
    (SPEC_TIMERS_SETINTERVAL, CAP_TIMERS_SETINTERVAL, "timers:setInterval", 0x0702),
    (SPEC_TIMERS_CLEARINTERVAL, CAP_TIMERS_CLEARINTERVAL, "timers:clearInterval", 0x0703),
    (SPEC_TIMERS_SETIMMEDIATE, CAP_TIMERS_SETIMMEDIATE, "timers:setImmediate", 0x0704),
    (SPEC_TIMERS_CLEARIMMEDIATE, CAP_TIMERS_CLEARIMMEDIATE, "timers:clearImmediate", 0x0705),
    (SPEC_TIMERS_TICK, CAP_TIMERS_TICK, "timers:tick", 0x0706),
    (SPEC_TIMERS_UNREF, CAP_TIMERS_UNREF, "timers:unref", 0x0708),
    (SPEC_TIMERS_REF, CAP_TIMERS_REF, "timers:ref", 0x0709),
    (SPEC_TIMERS_HASREF, CAP_TIMERS_HASREF, "timers:hasRef", 0x070A),
    (SPEC_TIMERS_REFRESH, CAP_TIMERS_REFRESH, "timers:refresh", 0x070B),
}
node_api! {
    (SPEC_RUN_LOOP, CAP_RUN_LOOP, "__quench_run_loop__", 0x070C),
    (SPEC_RUN_EXIT, CAP_RUN_EXIT, "__quench_run_exit__", 0x070D),
    (SPEC_INTERNAL_UTIL_SLEEP, CAP_INTERNAL_UTIL_SLEEP, "internal/util:sleep", 0x070E),
    (SPEC_INTERNAL_UTIL_ASSERT_CRYPTO, CAP_INTERNAL_UTIL_ASSERT_CRYPTO, "internal/util:assertCrypto", 0x0721),
    (SPEC_TIMERS_CLOSE, CAP_TIMERS_CLOSE, "timers:close", 0x070F),
    (SPEC_TIMERS_TO_PRIMITIVE, CAP_TIMERS_TO_PRIMITIVE, "timers:toPrimitive", 0x071D),
    (SPEC_TIMERS_GET_LIBUV_NOW, CAP_TIMERS_GET_LIBUV_NOW, "timers:getLibuvNow", 0x0714),
    (SPEC_UTIL_PROMISIFY, CAP_UTIL_PROMISIFY, "util:promisify", 0x071E),
    (SPEC_UTIL_PROMISIFIED_CALL, CAP_UTIL_PROMISIFIED_CALL, "util:promisifiedCall", 0x071F),
    (SPEC_UTIL_PROMISIFIED_CALLBACK, CAP_UTIL_PROMISIFIED_CALLBACK, "util:promisifiedCallback", 0x0720),
    (SPEC_LINKED_LIST_INIT, CAP_LINKED_LIST_INIT, "internal/linkedlist:init", 0x0718),
    (SPEC_LINKED_LIST_REMOVE, CAP_LINKED_LIST_REMOVE, "internal/linkedlist:remove", 0x0719),
    (SPEC_LINKED_LIST_APPEND, CAP_LINKED_LIST_APPEND, "internal/linkedlist:append", 0x071A),
    (SPEC_LINKED_LIST_IS_EMPTY, CAP_LINKED_LIST_IS_EMPTY, "internal/linkedlist:isEmpty", 0x071B),
    (SPEC_LINKED_LIST_PEEK, CAP_LINKED_LIST_PEEK, "internal/linkedlist:peek", 0x071C),
    (SPEC_TIMERS_SCHEDULE, CAP_TIMERS_SCHEDULE, "timers:scheduleTimer", 0x0715),
    (SPEC_TIMERS_TOGGLE_REF, CAP_TIMERS_TOGGLE_REF, "timers:toggleTimerRef", 0x0716),
    (SPEC_TIMERS_TOGGLE_IMMEDIATE_REF, CAP_TIMERS_TOGGLE_IMMEDIATE_REF, "timers:toggleImmediateRef", 0x0717),
}
pub const SPEC_QUEUE_MICROTASK: NodeSpec = NodeSpec::new("timers:queueMicrotask", 0x0707);
pub const SPEC_UTIL_DEPRECATE: NodeSpec = NodeSpec::new("util:deprecate", 0x0730);
pub const SPEC_UTIL_DEPRECATED_CALL: NodeSpec = NodeSpec::new("util:deprecatedCall", 0x0731);
pub const SPEC_UTIL_SYSTEM_ERROR_NAME: NodeSpec = NodeSpec::new("util:getSystemErrorName", 0x0733);
pub const SPEC_UTIL_DEBUGLOG: NodeSpec = NodeSpec::new("util:debuglog", 0x0735);
pub const SPEC_UTIL_EXCEPTION_WITH_HOST_PORT: NodeSpec =
    NodeSpec::new("util:exceptionWithHostPort", 0x0734);
pub const SPEC_UTIL_CONVERT_SIGNAL_TO_EXIT_CODE: NodeSpec =
    NodeSpec::new("util:convertProcessSignalToExitCode", 0x2203);
pub const SPEC_CP_KILL: NodeSpec = NodeSpec::new("child_process:ChildProcess:kill", 0x2204);
pub const SPEC_CP_STDIN_WRITE: NodeSpec = NodeSpec::new("child_process:stdin:write", 0x1E09);
pub const SPEC_CP_STDIN_END: NodeSpec = NodeSpec::new("child_process:stdin:end", 0x1E0A);
pub const SPEC_CP_ABORT: NodeSpec = NodeSpec::new("child_process:abort", 0x1E0B);
pub const SPEC_CP_ABORT_EMIT: NodeSpec = NodeSpec::new("child_process:abortEmit", 0x1E0C);
pub const SPEC_CP_FORK: NodeSpec = NodeSpec::new("child_process:fork", 0x1E0D);
pub const SPEC_CP_SEND: NodeSpec = NodeSpec::new("child_process:send", 0x1E0E);
pub const SPEC_CP_MESSAGE_EMIT: NodeSpec = NodeSpec::new("child_process:messageEmit", 0x1E0F);
pub const SPEC_CP_DISCONNECT: NodeSpec = NodeSpec::new("child_process:disconnect", 0x1E10);
pub const SPEC_CP_DISCONNECT_EMIT: NodeSpec = NodeSpec::new("child_process:disconnectEmit", 0x1E11);
pub const SPEC_CP_CONSTRUCTOR: NodeSpec = NodeSpec::new("child_process:ChildProcess", 0x2205);
pub const SPEC_CP_INSTANCE_SPAWN: NodeSpec =
    NodeSpec::new("child_process:ChildProcess:spawn", 0x2206);
pub const SPEC_INTERNAL_UTIL_EMIT_WARNING: NodeSpec =
    NodeSpec::new("internal/util:emitExperimentalWarning", 0x2201);
pub const SPEC_OS_GET_PRIORITY: NodeSpec = NodeSpec::new("os:getPriority", 0x0736);
pub const SPEC_OS_SET_PRIORITY: NodeSpec = NodeSpec::new("os:setPriority", 0x0737);
pub const SPEC_INTERNAL_OS_GET_HOME_DIRECTORY: NodeSpec =
    NodeSpec::new("internal/os:getHomeDirectory", 0x0738);
pub const SPEC_INTERNAL_BINDING: NodeSpec = NodeSpec::new("internal:test-binding", 0x0710);
pub const SPEC_INTERNAL_BUFFER_FILL: NodeSpec = NodeSpec::new("internal:buffer-fill", 0x0711);
pub const SPEC_INTERNAL_VIEW_HAS_BUFFER: NodeSpec =
    NodeSpec::new("internal:view-has-buffer", 0x0712);
pub const SPEC_INTERNAL_GET_PROXY_DETAILS: NodeSpec =
    NodeSpec::new("internal:get-proxy-details", 0x2200);
pub const SPEC_INTERNAL_BUFFER_ALIGNED_OFFSET: NodeSpec =
    NodeSpec::new("internal:buffer-array-buffer-aligned-offset", 0x0713);

pub const SPEC_BUFFER_FROM: NodeSpec = NodeSpec::new("buffer:from", 0x0800);
pub const SPEC_BUFFER_ALLOC: NodeSpec = NodeSpec::new("buffer:alloc", 0x0801);
pub const SPEC_BUFFER_BYTELENGTH: NodeSpec = NodeSpec::new("buffer:byteLength", 0x0802);
pub const SPEC_BUFFER_ISBUFFER: NodeSpec = NodeSpec::new("buffer:isBuffer", 0x0803);
pub const SPEC_BUFFER_CONCAT: NodeSpec = NodeSpec::new("buffer:concat", 0x0804);
pub const SPEC_BUFFER_NEW: NodeSpec = NodeSpec::new("buffer:Buffer", 0x0805);
pub const SPEC_BUFFER_ATOB: NodeSpec = NodeSpec::new("buffer:atob", 0x0806);
pub const SPEC_BUFFER_BTOA: NodeSpec = NodeSpec::new("buffer:btoa", 0x0807);
pub const SPEC_BUFFER_ASCII_WRITE: NodeSpec = NodeSpec::new("buffer:asciiWrite", 0x0850);
pub const SPEC_BUFFER_LATIN1_WRITE: NodeSpec = NodeSpec::new("buffer:latin1Write", 0x0851);
pub const SPEC_BUFFER_UTF8_WRITE: NodeSpec = NodeSpec::new("buffer:utf8Write", 0x0852);
pub const SPEC_INTERNAL_BUFFER_UTF8_WRITE: NodeSpec =
    NodeSpec::new("internal/buffer:utf8Write", 0x0856);
pub const SPEC_BUFFER_SUBARRAY: NodeSpec = NodeSpec::new("buffer:subarray", 0x0853);
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
pub const SPEC_BUFFER_INSPECT_MAX_BYTES_GET: NodeSpec =
    NodeSpec::new("buffer:inspectMaxBytesGet", 0x0854);
pub const SPEC_BUFFER_INSPECT_MAX_BYTES_SET: NodeSpec =
    NodeSpec::new("buffer:inspectMaxBytesSet", 0x0855);

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
pub const SPEC_PROCESS_KILL: NodeSpec = NodeSpec::new("process:kill", 0x0A20);
pub const SPEC_PROCESS_CWD: NodeSpec = NodeSpec::new("process:cwd", 0x0A02);
pub const SPEC_PROCESS_CHDIR: NodeSpec = NodeSpec::new("process:chdir", 0x0A03);
pub const SPEC_PROCESS_NEXT_TICK: NodeSpec = NodeSpec::new("process:nextTick", 0x0A04);
pub const SPEC_PROCESS_HRTIME: NodeSpec = NodeSpec::new("process:hrtime", 0x0A05);
pub const SPEC_PROCESS_HRTIME_BIGINT: NodeSpec = NodeSpec::new("process:hrtime.bigint", 0x0A0B);
pub const SPEC_PROCESS_CPU_USAGE: NodeSpec = NodeSpec::new("process:cpuUsage", 0x0A10);
pub const SPEC_PROCESS_UPTIME: NodeSpec = NodeSpec::new("process:uptime", 0x0A11);
pub const SPEC_PROCESS_AVAILABLE_MEMORY: NodeSpec =
    NodeSpec::new("process:availableMemory", 0x0A1B);
pub const SPEC_PROCESS_CONSTRAINED_MEMORY: NodeSpec =
    NodeSpec::new("process:constrainedMemory", 0x0A1C);
pub const SPEC_PROCESS_GETUID: NodeSpec = NodeSpec::new("process:getuid", 0x0A12);
pub const SPEC_PROCESS_GETGID: NodeSpec = NodeSpec::new("process:getgid", 0x0A13);
pub const SPEC_PROCESS_GETEUID: NodeSpec = NodeSpec::new("process:geteuid", 0x0A14);
pub const SPEC_PROCESS_GETEGID: NodeSpec = NodeSpec::new("process:getegid", 0x0A15);
pub const SPEC_PROCESS_SETUID: NodeSpec = NodeSpec::new("process:setuid", 0x0A16);
pub const SPEC_PROCESS_SETGID: NodeSpec = NodeSpec::new("process:setgid", 0x0A17);
pub const SPEC_PROCESS_SETEUID: NodeSpec = NodeSpec::new("process:seteuid", 0x0A18);
pub const SPEC_PROCESS_SETEGID: NodeSpec = NodeSpec::new("process:setegid", 0x0A19);
pub const SPEC_PROCESS_ACTIVE_RESOURCES: NodeSpec =
    NodeSpec::new("process:getActiveResourcesInfo", 0x0A1A);
pub const SPEC_PROCESS_UMASK: NodeSpec = NodeSpec::new("process:umask", 0x0A06);
pub const SPEC_PROCESS_ON: NodeSpec = NodeSpec::new("process:on", 0x0A07);
pub const SPEC_PROCESS_ONCE: NodeSpec = NodeSpec::new("process:once", 0x0A08);
pub const SPEC_PROCESS_REMOVE_LISTENER: NodeSpec = NodeSpec::new("process:removeListener", 0x0A0E);
pub const SPEC_PROCESS_REMOVE_ALL_LISTENERS: NodeSpec =
    NodeSpec::new("process:removeAllListeners", 0x0A0F);
pub const SPEC_PROCESS_EMIT: NodeSpec = NodeSpec::new("process:emit", 0x0A0C);
pub const SPEC_PROCESS_EMIT_WARNING: NodeSpec = NodeSpec::new("process:emitWarning", 0x0A0D);
pub const SPEC_PROCESS_EXIT_CODE_GET: NodeSpec = NodeSpec::new("process:exitCode:get", 0x0A21);
pub const SPEC_PROCESS_EXIT_CODE_SET: NodeSpec = NodeSpec::new("process:exitCode:set", 0x0A22);

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
pub const SPEC_OS_ENDIANNESS: NodeSpec = NodeSpec::new("os:endianness", 0x0B11);
pub const SPEC_OS_VERSION: NodeSpec = NodeSpec::new("os:version", 0x0B12);
pub const SPEC_OS_MACHINE: NodeSpec = NodeSpec::new("os:machine", 0x0B13);
pub const SPEC_OS_USERINFO: NodeSpec = NodeSpec::new("os:userInfo", 0x0B14);
pub const SPEC_OS_AVAILABLE_PARALLELISM: NodeSpec =
    NodeSpec::new("os:availableParallelism", 0x0B0E);

pub const SPEC_STREAM_READABLE: NodeSpec = NodeSpec::new("stream:Readable", 0x0C00);
pub const SPEC_STREAM_WRITABLE: NodeSpec = NodeSpec::new("stream:Writable", 0x0C01);
pub const SPEC_STREAM_DUPLEX: NodeSpec = NodeSpec::new("stream:Duplex", 0x0C02);
pub const SPEC_STREAM_TRANSFORM: NodeSpec = NodeSpec::new("stream:Transform", 0x0C03);
pub const SPEC_STREAM_PIPELINE: NodeSpec = NodeSpec::new("stream:pipeline", 0x0C04);
pub const SPEC_STREAM_FINISHED: NodeSpec = NodeSpec::new("stream:finished", 0x0C05);

pub const SPEC_STRING_DECODER: NodeSpec = NodeSpec::new("string_decoder:StringDecoder", 0x0D00);

pub const SPEC_DNS_LOOKUP: NodeSpec = NodeSpec::new("dns:lookup", 0x0E00);
pub const SPEC_DNS_RESOLVE4: NodeSpec = NodeSpec::new("dns:resolve4", 0x0E01);
pub const SPEC_DNS_LOOKUP_ADDRESSES: NodeSpec = NodeSpec::new("dns:lookupAddresses", 0x0E02);
pub const SPEC_DNS_EXCEPTION: NodeSpec = NodeSpec::new("dns:DNSException", 0x0E03);

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
pub const SPEC_HTTP_REQ_SET_HEADER: NodeSpec = NodeSpec::new("http:req:setHeader", 0x0F0D);
pub const SPEC_HTTP_AGENT: NodeSpec = NodeSpec::new("http:Agent", 0x0F0E);
pub const SPEC_HTTP_REQ_RESUME: NodeSpec = NodeSpec::new("http:req:resume", 0x0F0F);
pub const SPEC_HTTP_RES_SET_ENCODING: NodeSpec = NodeSpec::new("http:res:setEncoding", 0x0F10);

pub const SPEC_NET_CONNECT: NodeSpec = NodeSpec::new("net:connect", 0x1000);
pub const SPEC_NET_SOCKET: NodeSpec = NodeSpec::new("net:Socket", 0x1013);
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
pub const SPEC_NET_SERVER_UNREF: NodeSpec = NodeSpec::new("net:server:unref", 0x1014);
pub const SPEC_NET_SERVER_REF: NodeSpec = NodeSpec::new("net:server:ref", 0x1015);
pub const SPEC_NET_SOCKET_WRITE: NodeSpec = NodeSpec::new("net:socket:write", 0x100A);
pub const SPEC_NET_SOCKET_END: NodeSpec = NodeSpec::new("net:socket:end", 0x100B);
pub const SPEC_NET_SOCKET_DESTROY: NodeSpec = NodeSpec::new("net:socket:destroy", 0x100C);
pub const SPEC_NET_SOCKET_ADDRESS: NodeSpec = NodeSpec::new("net:socket:address", 0x100D);
pub const SPEC_NET_SOCKET_SET_NO_DELAY: NodeSpec = NodeSpec::new("net:socket:setNoDelay", 0x100E);
pub const SPEC_NET_SOCKET_SET_KEEP_ALIVE: NodeSpec =
    NodeSpec::new("net:socket:setKeepAlive", 0x100F);
pub const SPEC_NET_LOOKUP_CALLBACK: NodeSpec = NodeSpec::new("net:lookupCallback", 0x1016);
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
pub const SPEC_FS_SYMLINKSYNC: NodeSpec = NodeSpec::new("fs:symlinkSync", 0x1125);
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
pub const SPEC_FS_WATCH: NodeSpec = NodeSpec::new("fs:watch", 0x1152);
pub const SPEC_FS_WATCH_CLOSE: NodeSpec = NodeSpec::new("fs:watch:close", 0x1157);
pub const SPEC_FS_READSTREAM: NodeSpec = NodeSpec::new("fs:ReadStream", 0x1153);
pub const SPEC_FS_WRITESTREAM: NodeSpec = NodeSpec::new("fs:WriteStream", 0x1154);
pub const SPEC_FS_OPENDIR: NodeSpec = NodeSpec::new("fs:opendir", 0x1155);
pub const SPEC_FS_OPENDIRSYNC: NodeSpec = NodeSpec::new("fs:opendirSync", 0x1156);
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

// zlib sync compression (flate2-backed).
pub const SPEC_ZLIB_GZIP: NodeSpec = NodeSpec::new("zlib:gzipSync", 0x1700);
pub const SPEC_ZLIB_GUNZIP: NodeSpec = NodeSpec::new("zlib:gunzipSync", 0x1701);
pub const SPEC_ZLIB_DEFLATE_RAW: NodeSpec = NodeSpec::new("zlib:deflateRawSync", 0x1702);
pub const SPEC_ZLIB_INFLATE_RAW: NodeSpec = NodeSpec::new("zlib:inflateRawSync", 0x1703);
pub const SPEC_ZLIB_DEFLATE: NodeSpec = NodeSpec::new("zlib:deflateSync", 0x1704);
pub const SPEC_ZLIB_INFLATE: NodeSpec = NodeSpec::new("zlib:inflateSync", 0x1705);
pub const SPEC_CJS_WRAP: NodeSpec = NodeSpec::new("__quench_cjs_wrap__", 0x1d00);
pub const SPEC_CP_SPAWNSYNC: NodeSpec = NodeSpec::new("child_process:spawnSync", 0x1e00);
pub const SPEC_CP_EXECSYNC: NodeSpec = NodeSpec::new("child_process:execSync", 0x1e01);
pub const SPEC_CP_EXEC: NodeSpec = NodeSpec::new("child_process:exec", 0x1e02);
pub const SPEC_CP_EXECFILE: NodeSpec = NodeSpec::new("child_process:execFile", 0x1e03);
pub const SPEC_CP_EXECFILE_ABORT: NodeSpec = NodeSpec::new("child_process:execFileAbort", 0x1e07);
pub const SPEC_CP_EXECFILE_COMPLETE: NodeSpec =
    NodeSpec::new("child_process:execFileComplete", 0x1e08);
pub const SPEC_CP_SPAWN: NodeSpec = NodeSpec::new("child_process:spawn", 0x1e06);
pub const SPEC_CP_SPAWN_ERROR_EMIT: NodeSpec =
    NodeSpec::new("child_process:spawnErrorEmit", 0x1e04);
pub const SPEC_CP_SPAWN_OUTPUT_EMIT: NodeSpec =
    NodeSpec::new("child_process:spawnOutputEmit", 0x1e05);
node_api! {
    (SPEC_URL_PATH_TO_FILE_URL, CAP_URL_PATH_TO_FILE_URL, "url:pathToFileURL", 0x0505),
    (SPEC_URL_GET_HREF, CAP_URL_GET_HREF, "url:get:href", 0x0506),
    (SPEC_URL_GET_PROTOCOL, CAP_URL_GET_PROTOCOL, "url:get:protocol", 0x0507),
    (SPEC_URL_GET_USERNAME, CAP_URL_GET_USERNAME, "url:get:username", 0x0508),
    (SPEC_URL_GET_PASSWORD, CAP_URL_GET_PASSWORD, "url:get:password", 0x0509),
    (SPEC_URL_GET_HOST, CAP_URL_GET_HOST, "url:get:host", 0x050A),
    (SPEC_URL_GET_HOSTNAME, CAP_URL_GET_HOSTNAME, "url:get:hostname", 0x050B),
    (SPEC_URL_GET_PORT, CAP_URL_GET_PORT, "url:get:port", 0x050C),
    (SPEC_URL_GET_PATHNAME, CAP_URL_GET_PATHNAME, "url:get:pathname", 0x050D),
    (SPEC_URL_GET_SEARCH, CAP_URL_GET_SEARCH, "url:get:search", 0x050E),
    (SPEC_URL_GET_HASH, CAP_URL_GET_HASH, "url:get:hash", 0x050F),
    (SPEC_URL_GET_ORIGIN, CAP_URL_GET_ORIGIN, "url:get:origin", 0x0510),
    (SPEC_URL_GET_SEARCH_PARAMS, CAP_URL_GET_SEARCH_PARAMS, "url:get:searchParams", 0x0511),
    (SPEC_URL_TO_STRING, CAP_URL_TO_STRING, "url:toString", 0x0512),
    (SPEC_URL_TO_JSON, CAP_URL_TO_JSON, "url:toJSON", 0x0513),
    (SPEC_URL_REVOKE_OBJECT_URL, CAP_URL_REVOKE_OBJECT_URL, "url:revokeObjectURL", 0x0514),
    (SPEC_URL_FILE_URL_TO_PATH, CAP_URL_FILE_URL_TO_PATH, "url:fileURLToPath", 0x0515),
    (SPEC_URL_TO_HTTP_OPTIONS, CAP_URL_TO_HTTP_OPTIONS, "url:urlToHttpOptions", 0x0516),
    (SPEC_URL_DOMAIN_TO_ASCII, CAP_URL_DOMAIN_TO_ASCII, "url:domainToASCII", 0x0517),
    (SPEC_URL_DOMAIN_TO_UNICODE, CAP_URL_DOMAIN_TO_UNICODE, "url:domainToUnicode", 0x0518),
}
node_api! {
    (SPEC_STRUCTURED_CLONE, "structuredClone", 0x1F36),
    (SPEC_FETCH, "fetch", 0x1F21),
    (SPEC_GC, "gc", 0x2117),
    (SPEC_ABORT_CONTROLLER, "AbortController", 0x1F22),
    (SPEC_ABORT_CONTROLLER_ABORT, "AbortController.abort", 0x1F25),
    (SPEC_ABORT_CONTROLLER_SIGNAL_GET, "AbortController.signal:get", 0x1F27),
    (SPEC_ABORT_SIGNAL, "AbortSignal", 0x1F23),
    (SPEC_ABORT_SIGNAL_ABORTED_GET, "AbortSignal.aborted:get", 0x1F28),
    (SPEC_ABORT_SIGNAL_HAS_INSTANCE, "AbortSignal.hasInstance", 0x1F29),
    (SPEC_ABORT_SIGNAL_THROW_IF_ABORTED, "AbortSignal.throwIfAborted", 0x1F2A),
    (SPEC_ABORT_SIGNAL_ABORT, "AbortSignal.abort", 0x1F24),
    (SPEC_ABORT_SIGNAL_TIMEOUT, "AbortSignal.timeout", 0x1F40),
    (SPEC_ABORT_SIGNAL_ANY, "AbortSignal.any", 0x1F41),
    (SPEC_ABORT_EVENT_STOP_IMMEDIATE, "AbortEvent.stopImmediatePropagation", 0x1F26),
}
node_api! {
    (SPEC_NODE_EVENT_TARGET_NEW, CAP_NODE_EVENT_TARGET_NEW, "eventTarget:NodeEventTarget", 0x0139),
    (SPEC_NODE_EVENT_TARGET_ADD, CAP_NODE_EVENT_TARGET_ADD, "nodeEventTarget:addEventListener", 0x013A),
    (SPEC_NODE_EVENT_TARGET_REMOVE, CAP_NODE_EVENT_TARGET_REMOVE, "nodeEventTarget:removeEventListener", 0x013B),
    (SPEC_NODE_EVENT_TARGET_DISPATCH, CAP_NODE_EVENT_TARGET_DISPATCH, "nodeEventTarget:dispatchEvent", 0x013C),
    (SPEC_NODE_EVENT_TARGET_ON, CAP_NODE_EVENT_TARGET_ON, "nodeEventTarget:on", 0x013D),
    (SPEC_NODE_EVENT_TARGET_ONCE, CAP_NODE_EVENT_TARGET_ONCE, "nodeEventTarget:once", 0x013E),
    (SPEC_NODE_EVENT_TARGET_REMOVE_ALL, CAP_NODE_EVENT_TARGET_REMOVE_ALL, "nodeEventTarget:removeAllListeners", 0x013F),
    (SPEC_NODE_EVENT_TARGET_LISTENER_COUNT, CAP_NODE_EVENT_TARGET_LISTENER_COUNT, "nodeEventTarget:listenerCount", 0x0140),
    (SPEC_NODE_EVENT_TARGET_EVENT_NAMES, CAP_NODE_EVENT_TARGET_EVENT_NAMES, "nodeEventTarget:eventNames", 0x0141),
    (SPEC_NODE_EVENT_TARGET_SET_MAX, CAP_NODE_EVENT_TARGET_SET_MAX, "nodeEventTarget:setMaxListeners", 0x0142),
    (SPEC_NODE_EVENT_TARGET_GET_MAX, CAP_NODE_EVENT_TARGET_GET_MAX, "nodeEventTarget:getMaxListeners", 0x0143),
    (SPEC_NODE_EVENT_TARGET_EMIT, CAP_NODE_EVENT_TARGET_EMIT, "nodeEventTarget:emit", 0x0144),
}
node_api! {
    (SPEC_EVENT, CAP_EVENT, "Event", 0x0118),
    (SPEC_EVENT_PREVENT_DEFAULT, CAP_EVENT_PREVENT_DEFAULT, "Event.preventDefault", 0x011a),
    (SPEC_EVENT_STOP_PROPAGATION, CAP_EVENT_STOP_PROPAGATION, "Event.stopPropagation", 0x011b),
    (SPEC_EVENT_STOP_IMMEDIATE, CAP_EVENT_STOP_IMMEDIATE, "Event.stopImmediatePropagation", 0x011c),
    (SPEC_EVENT_COMPOSED_PATH, CAP_EVENT_COMPOSED_PATH, "Event.composedPath", 0x011d),
    (SPEC_EVENT_GET_CANCEL_BUBBLE, CAP_EVENT_GET_CANCEL_BUBBLE, "Event.cancelBubble.get", 0x011e),
    (SPEC_EVENT_SET_CANCEL_BUBBLE, CAP_EVENT_SET_CANCEL_BUBBLE, "Event.cancelBubble.set", 0x011f),
    (SPEC_DEFINE_EVENT_HANDLER, CAP_DEFINE_EVENT_HANDLER, "internal:eventTarget:defineEventHandler", 0x0120),
    (SPEC_EVENT_HANDLER_GET, CAP_EVENT_HANDLER_GET, "EventHandler.get", 0x0121),
    (SPEC_EVENT_HANDLER_SET, CAP_EVENT_HANDLER_SET, "EventHandler.set", 0x0122),
    (SPEC_EVENT_GET_PROPERTY, CAP_EVENT_GET_PROPERTY, "Event.property:get", 0x0129),
    (SPEC_CUSTOM_EVENT, CAP_CUSTOM_EVENT, "CustomEvent", 0x0123),
    (SPEC_EVENT_SOURCE, CAP_EVENT_SOURCE, "EventSource", 0x0124),
}

pub const SPEC_ASSERT_OK: NodeSpec = NodeSpec::new("assert:ok", 0x1420);
pub const SPEC_ASSERT_STRICT_EQUAL: NodeSpec = NodeSpec::new("assert:strictEqual", 0x1421);
pub const SPEC_ASSERT_NOT_STRICT_EQUAL: NodeSpec = NodeSpec::new("assert:notStrictEqual", 0x1422);
pub const SPEC_ASSERT_EQUAL: NodeSpec = NodeSpec::new("assert:equal", 0x1423);
pub const SPEC_ASSERT_NOT_EQUAL: NodeSpec = NodeSpec::new("assert:notEqual", 0x1424);
pub const SPEC_ASSERT_DEEP_STRICT_EQUAL: NodeSpec = NodeSpec::new("assert:deepStrictEqual", 0x1425);
pub const SPEC_ASSERT_NOT_DEEP_STRICT_EQUAL: NodeSpec =
    NodeSpec::new("assert:notDeepStrictEqual", 0x1426);
pub const SPEC_ASSERT_THROWS: NodeSpec = NodeSpec::new("assert:throws", 0x1427);
pub const SPEC_ASSERT_DOES_NOT_THROW: NodeSpec = NodeSpec::new("assert:doesNotThrow", 0x1428);
pub const SPEC_ASSERT_FAIL: NodeSpec = NodeSpec::new("assert:fail", 0x1429);
pub const SPEC_ASSERT_IF_ERROR: NodeSpec = NodeSpec::new("assert:ifError", 0x142A);
pub const SPEC_ASSERT_MATCH: NodeSpec = NodeSpec::new("assert:match", 0x142B);
pub const SPEC_ASSERT_DOES_NOT_MATCH: NodeSpec = NodeSpec::new("assert:doesNotMatch", 0x142C);
pub const SPEC_ASSERT_CONSTRUCTOR: NodeSpec = NodeSpec::new("assert:Assert", 0x142D);
pub const SPEC_ASSERTION_ERROR_CONSTRUCTOR: NodeSpec =
    NodeSpec::new("assert:AssertionError", 0x142E);
pub const SPEC_ASSERT_PARTIAL_DEEP_STRICT_EQUAL: NodeSpec =
    NodeSpec::new("assert:partialDeepStrictEqual", 0x2202);
pub const SPEC_ASSERT_DEEP_EQUAL: NodeSpec = NodeSpec::new("assert:deepEqual", 0x142F);
pub const SPEC_ASSERT_NOT_DEEP_EQUAL: NodeSpec = NodeSpec::new("assert:notDeepEqual", 0x1430);

pub const SPEC_VM_RUN_IN_NEW_CONTEXT: NodeSpec = NodeSpec::new("vm:runInNewContext", 0x1600);
pub const SPEC_VM_CREATE_CONTEXT: NodeSpec = NodeSpec::new("vm:createContext", 0x1601);
pub const SPEC_VM_RUN_IN_CONTEXT: NodeSpec = NodeSpec::new("vm:runInContext", 0x1602);
pub const SPEC_VM_IS_CONTEXT: NodeSpec = NodeSpec::new("vm:isContext", 0x1603);

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
    // QuickJS exposes the Float16 view storage but not its constructor.  The
    // Node facade still needs the two-byte view in common buffer-source paths;
    // use the engine's canonical Uint16 constructor until native Float16
    // element conversion is available.
    let float16_prototype = quench_runtime::host_api::object(vec![]);
    let _ = quench_runtime::execute::set_property(
        float16_prototype.clone(),
        "\0float16_constructor",
        quench_runtime::value::Value::Boolean(true),
    );
    let float16_receiver = quench_runtime::host_api::object(vec![
        (
            "\0float16_constructor".into(),
            quench_runtime::value::Value::Boolean(true),
        ),
        ("\0prototype".into(), float16_prototype.clone()),
    ]);
    let float16_constructor = quench_runtime::host_api::bound_builtin(
        quench_runtime::ops::Builtin::Uint16Array,
        float16_receiver,
    );
    let float16_constructor = quench_runtime::execute::set_property(
        float16_constructor,
        "prototype",
        float16_prototype.clone(),
    );
    let float16_constructor = quench_runtime::execute::set_property(
        float16_constructor,
        "name",
        quench_runtime::value::Value::String("Float16Array".into()),
    );
    let float16_constructor = quench_runtime::execute::set_property(
        float16_constructor,
        "BYTES_PER_ELEMENT",
        quench_runtime::value::Value::Number(2.0),
    );
    let _ = quench_runtime::execute::set_property(
        float16_prototype,
        "constructor",
        float16_constructor.clone(),
    );
    out.push(("Float16Array".to_string(), float16_constructor));
    out.push(("require".to_string(), crate::host::capability(SPEC_REQUIRE)));
    out.push((
        "__quench_cjs_wrap__".to_string(),
        crate::host::capability(crate::registry::SPEC_CJS_WRAP),
    ));
    out.push((
        "__quench_run_loop__".to_string(),
        crate::host::capability(crate::registry::SPEC_RUN_LOOP),
    ));
    out.push((
        "__quench_process_next_tick".to_string(),
        crate::host::capability(crate::registry::SPEC_PROCESS_NEXT_TICK),
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
    let abort_controller = crate::host::capability(crate::registry::SPEC_ABORT_CONTROLLER);
    let abort_controller_abort =
        crate::host::capability(crate::registry::SPEC_ABORT_CONTROLLER_ABORT);
    let abort_controller =
        quench_runtime::execute::set_property(abort_controller, "abort", abort_controller_abort);
    let controller_prototype = quench_runtime::execute::set_property(
        quench_runtime::host_api::object(Vec::new()),
        "abort",
        crate::host::capability(crate::registry::SPEC_ABORT_CONTROLLER_ABORT),
    );
    let controller_prototype = quench_runtime::execute::define_property(
        controller_prototype,
        "signal",
        quench_runtime::host_api::object(vec![(
            "get".into(),
            crate::host::capability(crate::registry::SPEC_ABORT_CONTROLLER_SIGNAL_GET),
        )]),
    )
    .unwrap_or_else(|_| quench_runtime::host_api::object(Vec::new()));
    let abort_controller =
        quench_runtime::execute::set_property(abort_controller, "prototype", controller_prototype);
    out.push(("AbortController".to_string(), abort_controller));
    let abort_signal = crate::host::capability(crate::registry::SPEC_ABORT_SIGNAL);
    let abort = crate::host::capability(crate::registry::SPEC_ABORT_SIGNAL_ABORT);
    let abort_signal = quench_runtime::execute::set_property(abort_signal, "abort", abort);
    let timeout = crate::host::capability(crate::registry::SPEC_ABORT_SIGNAL_TIMEOUT);
    let abort_signal = quench_runtime::execute::set_property(abort_signal, "timeout", timeout);
    let any = crate::host::capability(crate::registry::SPEC_ABORT_SIGNAL_ANY);
    let abort_signal = quench_runtime::execute::set_property(abort_signal, "any", any);
    let abort_signal = quench_runtime::execute::set_property(
        abort_signal,
        "Symbol.hasInstance",
        crate::host::capability(crate::registry::SPEC_ABORT_SIGNAL_HAS_INSTANCE),
    );
    let signal_prototype = quench_runtime::execute::set_property(
        quench_runtime::host_api::object(Vec::new()),
        "reason",
        quench_runtime::value::Value::Undefined,
    );
    let signal_prototype = quench_runtime::execute::define_property(
        signal_prototype,
        "aborted",
        quench_runtime::host_api::object(vec![(
            "get".into(),
            crate::host::capability(crate::registry::SPEC_ABORT_SIGNAL_ABORTED_GET),
        )]),
    )
    .unwrap_or_else(|_| quench_runtime::host_api::object(Vec::new()));
    let abort_signal =
        quench_runtime::execute::set_property(abort_signal, "prototype", signal_prototype);
    out.push(("AbortSignal".to_string(), abort_signal));
    out.push((
        "gc".to_string(),
        crate::host::capability(crate::registry::SPEC_GC),
    ));
    out.push((
        "console".to_string(),
        crate::modules::console::build_value(),
    ));
    let event_target = crate::host::capability(SPEC_EVENT_TARGET_NEW);
    let event_target_prototype = quench_runtime::execute::define_property(
        crate::modules::event_target::prototype(),
        "constructor",
        crate::host::namespace_object_from_pairs(vec![
            ("value".into(), event_target.clone()),
            (
                "writable".into(),
                quench_runtime::value::Value::Boolean(true),
            ),
            (
                "enumerable".into(),
                quench_runtime::value::Value::Boolean(false),
            ),
            (
                "configurable".into(),
                quench_runtime::value::Value::Boolean(true),
            ),
        ]),
    )
    .unwrap_or_else(|_| crate::modules::event_target::prototype());
    let _ = quench_runtime::execute::set_callable_property(
        &event_target,
        "prototype",
        event_target_prototype,
    );
    out.push(("EventTarget".to_string(), event_target));
    let event = crate::host::capability(crate::registry::SPEC_EVENT);
    let event_prototype = quench_runtime::execute::define_property(
        crate::host::namespace_object_from_pairs(Vec::new()),
        "constructor",
        crate::host::namespace_object_from_pairs(vec![
            ("value".into(), event.clone()),
            (
                "writable".into(),
                quench_runtime::value::Value::Boolean(true),
            ),
            (
                "enumerable".into(),
                quench_runtime::value::Value::Boolean(false),
            ),
            (
                "configurable".into(),
                quench_runtime::value::Value::Boolean(true),
            ),
        ]),
    )
    .unwrap_or_else(|_| crate::host::namespace_object_from_pairs(Vec::new()));
    let event_prototype = install_event_prototype(event_prototype);
    let _ = quench_runtime::execute::set_callable_property(
        &event,
        "prototype",
        event_prototype.clone(),
    );
    let _ = quench_runtime::execute::set_callable_property(
        &event,
        "length",
        quench_runtime::value::Value::Number(1.0),
    );
    for (name, value) in [
        ("NONE", 0.0),
        ("CAPTURING_PHASE", 1.0),
        ("AT_TARGET", 2.0),
        ("BUBBLING_PHASE", 3.0),
    ] {
        let _ = quench_runtime::execute::set_callable_property(
            &event,
            name,
            quench_runtime::value::Value::Number(value),
        );
    }
    out.push(("Event".to_string(), event));
    let custom_event = crate::host::capability(crate::registry::SPEC_CUSTOM_EVENT);
    let custom_event_prototype = install_custom_event_prototype(event_prototype.clone());
    let _ = quench_runtime::execute::set_callable_property(
        &custom_event,
        "prototype",
        custom_event_prototype,
    );
    for (name, value) in [
        ("NONE", 0.0),
        ("CAPTURING_PHASE", 1.0),
        ("AT_TARGET", 2.0),
        ("BUBBLING_PHASE", 3.0),
    ] {
        let _ = quench_runtime::execute::set_callable_property(
            &custom_event,
            name,
            quench_runtime::value::Value::Number(value),
        );
    }
    let _ = quench_runtime::execute::set_callable_property(
        &custom_event,
        "length",
        quench_runtime::value::Value::Number(1.0),
    );
    out.push(("CustomEvent".to_string(), custom_event));
    out.push((
        "__quench_event_source".to_string(),
        crate::host::capability(crate::registry::SPEC_EVENT_SOURCE),
    ));
    out.push(("atob".to_string(), crate::modules::buffer::atob_value()));
    out.push(("btoa".to_string(), crate::modules::buffer::btoa_value()));
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

fn install_event_prototype(
    mut prototype: quench_runtime::value::Value,
) -> quench_runtime::value::Value {
    for (name, value) in [
        (
            "preventDefault",
            crate::host::capability(SPEC_EVENT_PREVENT_DEFAULT),
        ),
        (
            "stopPropagation",
            crate::host::capability(SPEC_EVENT_STOP_PROPAGATION),
        ),
        (
            "stopImmediatePropagation",
            crate::host::capability(SPEC_EVENT_STOP_IMMEDIATE),
        ),
        (
            "composedPath",
            crate::host::capability(SPEC_EVENT_COMPOSED_PATH),
        ),
    ] {
        quench_runtime::execute::set_property_in_place(&prototype, name, value);
    }
    let accessors = [
        "target",
        "currentTarget",
        "srcElement",
        "type",
        "cancelable",
        "defaultPrevented",
        "timeStamp",
        "returnValue",
        "bubbles",
        "composed",
        "eventPhase",
    ];
    for name in accessors {
        let getter = quench_runtime::host_api::bound_capability_with_arguments(
            HostCapabilityRef {
                realm: RealmId::ROOT,
                kind: HostCapabilityKind::Custom(SPEC_EVENT_GET_PROPERTY.cap),
            },
            vec![quench_runtime::value::Value::String(name.into())],
        );
        prototype = quench_runtime::execute::define_property(
            prototype,
            name,
            quench_runtime::host_api::object(vec![
                ("get".into(), getter),
                (
                    "enumerable".into(),
                    quench_runtime::value::Value::Boolean(true),
                ),
                (
                    "configurable".into(),
                    quench_runtime::value::Value::Boolean(true),
                ),
            ]),
        )
        .unwrap_or_else(|_| quench_runtime::host_api::object(Vec::new()));
    }
    quench_runtime::execute::define_property(
        prototype,
        "cancelBubble",
        quench_runtime::host_api::object(vec![
            (
                "get".into(),
                crate::host::capability(SPEC_EVENT_GET_CANCEL_BUBBLE),
            ),
            (
                "set".into(),
                crate::host::capability(SPEC_EVENT_SET_CANCEL_BUBBLE),
            ),
            (
                "enumerable".into(),
                quench_runtime::value::Value::Boolean(true),
            ),
            (
                "configurable".into(),
                quench_runtime::value::Value::Boolean(true),
            ),
        ]),
    )
    .unwrap_or_else(|_| quench_runtime::host_api::object(Vec::new()))
}

fn install_custom_event_prototype(
    event_prototype: quench_runtime::value::Value,
) -> quench_runtime::value::Value {
    let prototype = quench_runtime::host_api::object(Vec::new());
    let prototype = quench_runtime::execute::set_prototype_of(&prototype, &event_prototype)
        .unwrap_or(prototype);
    quench_runtime::execute::define_property(
        prototype,
        "detail",
        quench_runtime::host_api::object(vec![
            (
                "get".into(),
                quench_runtime::host_api::bound_capability_with_arguments(
                    HostCapabilityRef {
                        realm: RealmId::ROOT,
                        kind: HostCapabilityKind::Custom(SPEC_EVENT_GET_PROPERTY.cap),
                    },
                    vec![quench_runtime::value::Value::String("detail".into())],
                ),
            ),
            (
                "enumerable".into(),
                quench_runtime::value::Value::Boolean(true),
            ),
            (
                "configurable".into(),
                quench_runtime::value::Value::Boolean(true),
            ),
        ]),
    )
    .unwrap_or_else(|_| quench_runtime::host_api::object(Vec::new()))
}

fn timers_binding(
    name: &'static str,
    spec: crate::registry::NodeSpec,
) -> (String, quench_runtime::value::Value) {
    (name.to_string(), crate::host::capability(spec))
}
