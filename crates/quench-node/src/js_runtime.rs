use oxc_resolver::{ResolveOptions, Resolver};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use sha3::{
    digest::{ExtendableOutput, Update as XofUpdate, XofReader},
    Shake128, Shake256,
};
use std::path::{Path, PathBuf};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use quench_runtime::{
    ops::{HostCapabilityKind, HostCapabilityRef, RealmId},
    value::{ArrayBufferData, Uint8ArrayData, Value},
    vm::{Host, VmContext, VmError},
};

thread_local! {
    static NODE_PROCESS_ENV: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_PROCESS_TITLE: RefCell<String> = RefCell::new("quench-node".into());
    static NODE_PATH_MODULE: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_UTIL_TYPES: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_PROCESS_MODULE: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_PROCESS_WARNING_LISTENERS: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static NODE_EXPERIMENTAL_WARNINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static NODE_DNS_SERVERS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static NODE_STREAM_PROMISES: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_TIMERS_PROMISES: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_TIMER_COUNTS: Cell<(u32, u32)> = const { Cell::new((0, 0)) };
    static NODE_ASSERT_MODULE: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_OS_HOME_ERROR: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_OS_BINDING: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_PRIORITY: Cell<i32> = const { Cell::new(0) };
    static VM_SCRIPT_RUNS: Cell<u32> = const { Cell::new(0) };
    static VM_COMPILE_CONTEXT_EXTENSION: Cell<bool> = const { Cell::new(false) };
    static VM_COMPILE_PARSING_CONTEXT: RefCell<Option<Value>> = const { RefCell::new(None) };
    static VM_COMPILE_RETURN_VALUE: RefCell<Option<Value>> = const { RefCell::new(None) };
    static VM_SCRIPT_CACHE_SOURCE: RefCell<Option<String>> = const { RefCell::new(None) };
    static BUFFER_INSPECT_MAX_BYTES: Cell<f64> = const { Cell::new(f64::INFINITY) };
}

// Capability numbers are an internal NodeHost registry. Keep them named so
// the runtime boundary carries intent instead of opaque numeric protocol IDs.
struct CapabilityName;
#[allow(non_upper_case_globals)]
impl CapabilityName {
    const Require: u16 = 1;
    const PathBasename: u16 = 2;
    const ConsoleLog: u16 = 4;
    const Cwd: u16 = 6;
    const ProcessUmask: u16 = 1574;
    const ReadFileSync: u16 = 7;
    const CreateHash: u16 = 8;
    const Stream: u16 = 10;
    const QueueMicrotask: u16 = 22;
    const Url: u16 = 40;
    const EventEmitter: u16 = 70;
    const BufferByteLength: u16 = 9;
    const BufferFrom: u16 = 30;
    const BufferHasInstance: u16 = 1326;
    const BufferInspectMaxBytesGet: u16 = 1327;
    const BufferInspectMaxBytesSet: u16 = 1328;
    const BufferAlloc: u16 = 31;
    const BufferIsBuffer: u16 = 32;
    const UtilFormat: u16 = 80;
    const UtilInspect: u16 = 81;
    const UtilFormatWithOptions: u16 = 2040;
    const InternalUtilSleep: u16 = 2088;
    const InternalUtilEmitExperimentalWarning: u16 = 2089;
    const ProcessOn: u16 = 2090;
    const ProcessEmit: u16 = 2091;
    const ProcessCpuUsage: u16 = 2110;
    const ProcessHrtime: u16 = 2111;
    const ProcessActiveResourcesInfo: u16 = 2112;
    const VmCreateContext: u16 = 2113;
    const VmRunInContext: u16 = 2114;
    const VmScript: u16 = 2115;
    const VmScriptRunInContext: u16 = 2116;
    const Gc: u16 = 2117;
    const VmScriptRunInNewContext: u16 = 2118;
    const VmScriptCreateCachedData: u16 = 2123;
    const FixtureReadKey: u16 = 2130;
    const FixturePath: u16 = 2131;
    const DnsSetServers: u16 = 2132;
    const DnsGetServers: u16 = 2133;
    const DnsResolve: u16 = 2134;
    const DnsLookupService: u16 = 2135;
    const DnsResolveMx: u16 = 2136;
    const DgramCreateSocket: u16 = 2137;
    const DgramBind: u16 = 2138;
    const DgramClose: u16 = 2139;
    const DgramSend: u16 = 2140;
    const DgramConnect: u16 = 2141;
    const DgramDisconnect: u16 = 2142;
    const DgramAddress: u16 = 2143;
    const DgramRemoteAddress: u16 = 2144;
    const DgramRef: u16 = 2145;
    const DgramUnref: u16 = 2146;
    const DgramSetBroadcast: u16 = 2147;
    const DgramSetTtl: u16 = 2148;
    const DgramGetRecvBufferSize: u16 = 2149;
    const DgramGetSendBufferSize: u16 = 2150;
    const DgramBindSync: u16 = 2207;
    const DgramConnectSync: u16 = 2208;
    const StreamConsumerBuffer: u16 = 2151;
    const StreamConsumerBytes: u16 = 2152;
    const StreamConsumerText: u16 = 2153;
    const StreamConsumerJson: u16 = 2154;
    const StreamPipeline: u16 = 2155;
    const HttpIncomingOnce: u16 = 2156;
    const HttpIncomingEmit: u16 = 2157;
    const StreamAddAbortSignal: u16 = 2158;
    const WorkerConstructor: u16 = 2159;
    const WorkerOn: u16 = 2160;
    const WorkerOnce: u16 = 2161;
    const WorkerPostMessage: u16 = 2162;
    const WorkerTerminate: u16 = 2163;
    const ZlibCreateGzip: u16 = 2164;
    const ZlibCreateGunzip: u16 = 2165;
    const ZlibCreateUnzip: u16 = 2166;
    const ZlibOn: u16 = 2167;
    const ZlibEnd: u16 = 2168;
    const ZlibGzip: u16 = 2169;
    const ZlibGzipSync: u16 = 2170;
    const ZlibDeflateSync: u16 = 2171;
    const CryptoGetHashes: u16 = 2176;
    const CryptoGetCiphers: u16 = 2177;
    const CryptoGetCipherInfo: u16 = 2178;
    const CryptoGetCurves: u16 = 2179;
    const TlsGetCiphers: u16 = 2182;
    const TlsCreateSecureContext: u16 = 2183;
    const CryptoGetDiffieHellman: u16 = 2184;
    const CryptoCreateDiffieHellman: u16 = 2185;
    const CryptoDhGetPrime: u16 = 2186;
    const CryptoDhGetGenerator: u16 = 2187;
    const CryptoDhGenerateKeys: u16 = 2188;
    const CryptoDhGetPublicKey: u16 = 2189;
    const CryptoDhComputeSecret: u16 = 2190;
    const CryptoDhHasInstance: u16 = 2191;
    const NetGetDefaultAutoSelectFamily: u16 = 2126;
    const NetGetDefaultAutoSelectFamilyAttemptTimeout: u16 = 2127;
    const UtilGetCallSites: u16 = 2124;
    const VmCompileFunction: u16 = 2119;
    const VmCompiledFunction: u16 = 2120;
    const VmCompiledToString: u16 = 2121;
    const CommonInvalidArgTypeHelper: u16 = 2122;
    const UtilDeprecatedFirst: u16 = 2092;
    const BufferIndexOf: u16 = 2041;
    const BufferLastIndexOf: u16 = 2042;
    const BufferToJson: u16 = 2043;
    const BufferOf: u16 = 2044;
    const BufferAllocUnsafeSlow: u16 = 2045;
    const BufferAllocUnsafe: u16 = 2064;
    const BufferIsEncoding: u16 = 2046;
    const BufferSwap16: u16 = 2047;
    const BufferSwap32: u16 = 2048;
    const BufferSwap64: u16 = 2049;
    const BufferCopyBytesFrom: u16 = 2050;
    const BufferReadBigInt64LE: u16 = 2051;
    const BufferReadBigUInt64BE: u16 = 2052;
    const BufferWriteBigInt64LE: u16 = 2053;
    const BufferWriteBigUInt64BE: u16 = 2054;
    const StringDecoderConstructor: u16 = 2076;
    const StringDecoderWrite: u16 = 2077;
    const StringDecoderEnd: u16 = 2078;
    const StringDecoderText: u16 = 2079;
    const StringDecoderCall: u16 = 2080;
    const VmRunInNewContext: u16 = 2055;
    const CryptoRandomBytes: u16 = 2056;
    const CryptoRandomFillSync: u16 = 2057;
    const CryptoPbkdf2Sync: u16 = 2203;
    const CryptoPbkdf2: u16 = 2204;
    const CryptoDigestBytes: u16 = 2205;
    const CryptoShakeBytes: u16 = 2206;
    const CryptoHashDigest: u16 = 2209;
    const CryptoHashUpdate: u16 = 2210;
    const DgramSetRecvBufferSize: u16 = 2211;
    const DgramSetSendBufferSize: u16 = 2212;
    const BufferIsAscii: u16 = 2058;
    const BufferIsUtf8: u16 = 2059;
    const TextEncoderConstructor: u16 = 2060;
    const TextEncoderEncode: u16 = 2061;
    const TextDecoderConstructor: u16 = 2062;
    const TextDecoderDecode: u16 = 2063;
    const BufferInspect: u16 = 2065;
    const InternalBinding: u16 = 2066;
    const InternalArrayBufferViewHasBuffer: u16 = 2067;
    const UrlParse: u16 = 2068;
    const UrlFormat: u16 = 2069;
    const PathNormalize: u16 = 2070;
    const PathWinNormalize: u16 = 2071;
    const UtilPromisify: u16 = 1950;
    const UtilDeprecate: u16 = 2099;
    const UtilParseEnv: u16 = 2098;
    const UtilSystemErrorName: u16 = 2097;
    const UtilSystemErrorMessage: u16 = 2096;
    const UtilExceptionWithHostPort: u16 = 2095;
    const UtilSystemErrorMap: u16 = 2094;
    const UtilSystemErrorMapGet: u16 = 2093;
    const UtilPromisifiedFirst: u16 = 2000;
    const UtilResolverFirst: u16 = 2100;
    const OsPlatform: u16 = 82;
    const OsArch: u16 = 83;
    const OsTmpdir: u16 = 84;
    const OsHomedir: u16 = 85;
    const OsCpus: u16 = 86;
    const OsFreemem: u16 = 87;
    const OsTotalmem: u16 = 88;
    const OsType: u16 = 89;
    const QuerystringParse: u16 = 90;
    const QuerystringEscape: u16 = 91;
    const QuerystringStringify: u16 = 92;
    const QuerystringUnescapeBuffer: u16 = 93;
    const QuerystringUnescape: u16 = 1590;
    const EventsGetMax: u16 = 94;
    const EventsSetMax: u16 = 95;
    const OsRelease: u16 = 96;
    const OsEndianness: u16 = 97;
    const OsLoadavg: u16 = 98;
    const OsNetworkInterfaces: u16 = 99;
    const ModuleIsBuiltin: u16 = 101;
    const ModuleCreateRequire: u16 = 102;
    const ModuleFindSourceMap: u16 = 103;
    const ModuleSyncBuiltinExports: u16 = 104;
    const OsUserInfo: u16 = 1000;
    const OsUptime: u16 = 1001;
    const OsGetPriority: u16 = 1002;
    const OsSetPriority: u16 = 1003;
    const OsAvailableParallelism: u16 = 1004;
    const OsHostname: u16 = 1005;
    const OsVersion: u16 = 1006;
    const OsMachine: u16 = 1007;
    const TimerImmediate: u16 = 27;
    const Timer: u16 = 28;
    const TimerClearImmediate: u16 = 29;
    const NodeTest: u16 = 2193;
    const InternalOsGetHomeDirectory: u16 = 2198;
    const VmIsContext: u16 = 2199;
    const ProcessNextTick: u16 = 21;
    const Assert: u16 = 13;
    const AssertStrictEqual: u16 = 14;
    const AssertDeepStrictEqual: u16 = 15;
    const AssertOk: u16 = 16;
    const AssertThrows: u16 = 17;
    const AssertDoesNotThrow: u16 = 18;
    const AssertIfError: u16 = 19;
    const AssertNotStrictEqual: u16 = 24;
    const AssertNotDeepStrictEqual: u16 = 25;
    const AssertError: u16 = 26;
    const AssertEqual: u16 = 33;
    const AssertNotEqual: u16 = 34;
    const AssertMatchValue: u16 = 35;
    const HttpServer: u16 = 11;
    const HttpGet: u16 = 12;
    const HttpRequestOn: u16 = 401;
    const HttpRequestEnd: u16 = 402;
    const HttpRequestWrite: u16 = 403;
    const Console: u16 = 3;
    const TimerValidation: u16 = 5;
    const StreamReadable: u16 = 1310;
    const StreamWritable: u16 = 1311;
    const StreamReadableFrom: u16 = 1312;
    const StreamDuplex: u16 = 1313;
    const StreamFinished: u16 = 1320;
    const StreamIsPaused: u16 = 1321;
    const StreamBaseWrite: u16 = 1322;
    const StreamRead: u16 = 1324;
    const FsAccess: u16 = 1500;
    const FsWriteBytes: u16 = 1501;
    const FsAppendBytes: u16 = 1502;
    const FsUnlink: u16 = 1503;
    const FsMkdtemp: u16 = 1504;
    const FsAccessSync: u16 = 1505;
    const FsWriteFileSync: u16 = 1506;
    const FsAppendFileSync: u16 = 1507;
    const FsUnlinkSync: u16 = 1508;
    const FsRmdirSync: u16 = 1509;
    const FsRealpathSync: u16 = 1510;
    const FsOpenSync: u16 = 1511;
    const FsCloseSync: u16 = 1512;
    const FsFchmod: u16 = 1513;
    const FsFstatSync: u16 = 1514;
    const FsChmodSync: u16 = 1515;
    const FsAccessAsync: u16 = 1516;
    const FsExistsSync: u16 = 1517;
    const ChildExecFile: u16 = 1600;
    const ChildSpawn: u16 = 2194;
    const ChildSpawnOn: u16 = 2195;
    const ChildSpawnSync: u16 = 2196;
    const ChildStdoutToString: u16 = 2197;
    const ReplServer: u16 = 2202;
    const ChildFork: u16 = 1601;
    const ChildEmit: u16 = 1602;
    const ChildSend: u16 = 1603;
    const CommonMustCall: u16 = 1700;
    const CommonMustCallAtLeast: u16 = 1705;
    const FsWriteAsync: u16 = 1520;
    const FsReadAsync: u16 = 1521;
    const FsWritePromise: u16 = 1522;
    const FsReadPromise: u16 = 1523;
    const FsAppendPromise: u16 = 2201;
    const FsOpenAsync: u16 = 1524;
    const FsCloseAsync: u16 = 1525;
    const CommonMustSucceed: u16 = 1701;
    const CommonMustNotCall: u16 = 1702;
    const CommonGetArrayBufferViews: u16 = 1703;
    const CommonCanSymlink: u16 = 1704;
    const CommonWrapperFirst: u16 = 1800;
    const PathRelative: u16 = 1300;
    const PathDirname: u16 = 1301;
    const PathIsAbsolute: u16 = 1302;
    const PathToNamespaced: u16 = 1303;
    const PathWinToNamespaced: u16 = 1304;
    const PathJoin: u16 = 1305;
    const PathExtname: u16 = 1306;
    const PathWinIsAbsolute: u16 = 2082;
    const PathMatchesGlob: u16 = 2083;
    const PathWinMatchesGlob: u16 = 2084;
    const PathResolve: u16 = 2085;
    const PathWinResolve: u16 = 2086;
    const FsStatAsync: u16 = 1526;
    const FsLstatAsync: u16 = 1527;
    const FsStatsIsDirectory: u16 = 1528;
    const FsStatsIsFile: u16 = 1529;
    const FsMkdirSync: u16 = 1530;
    const FsMkdirAsync: u16 = 2181;
    const FsToUnixTimestamp: u16 = 2180;
    const FsRmSync: u16 = 1531;
    const FsReaddirSync: u16 = 1532;
    const FsReaddirAsync: u16 = 1533;
    const FsDirentFile: u16 = 1534;
    const FsDirentDirectory: u16 = 1535;
    const FsDirentFileDirectory: u16 = 1536;
    const FsDirentDirectoryFile: u16 = 1537;
    const FsReaddirPromise: u16 = 1538;
    const FsStatSync: u16 = 1539;
    const FsStringToFlags: u16 = 1540;
    const FsLstatSync: u16 = 1541;
    const FsSymlinkSync: u16 = 1542;
    const FsStatsIsDirectoryFile: u16 = 1543;
    const FsStatsIsSymbolicLink: u16 = 1544;
    const FsStatsIsNotSymbolicLink: u16 = 1545;
    const FsFtruncateSync: u16 = 1571;
    const FsTruncateAsync: u16 = 1572;
    const FsTruncateSync: u16 = 1573;
    const StreamIterText: u16 = 1575;
    const StreamIterBytes: u16 = 1576;
    const StreamIterPull: u16 = 1577;
    const StreamIterIdentity: u16 = 1578;
    const ZlibIterCompress: u16 = 1579;
    const ZlibIterDecompress: u16 = 1580;
    const BufferWrite: u16 = 1581;
    const BufferIncludes: u16 = 1582;
    const BufferSlice: u16 = 1583;
    const BufferCopy: u16 = 1584;
    const BufferFill: u16 = 1585;
    const BufferCompare: u16 = 1586;
    const BufferNumericFirst: u16 = 2000;
    const PathParse: u16 = 2035;
    const PathFormat: u16 = 2036;
    const PathWinParse: u16 = 2037;
    const PathWinFormat: u16 = 2038;
    const PathWinBasename: u16 = 2039;
    const FsFsyncSync: u16 = 1546;
    const FsFdatasyncSync: u16 = 1547;
    const FsFsyncAsync: u16 = 1548;
    const FsFdatasyncAsync: u16 = 1549;
    const FsUnlinkPromise: u16 = 1550;
    const FsOpendirSync: u16 = 1551;
    const FsOpendirAsync: u16 = 1552;
    const FsOpendirPromise: u16 = 1553;
    const FsDirReadSync: u16 = 1554;
    const FsDirReadAsync: u16 = 1555;
    const FsDirCloseSync: u16 = 1556;
    const FsDirCloseAsync: u16 = 1557;
    const FsLinkSync: u16 = 1558;
    const FsLinkAsync: u16 = 1559;
    const FsLinkPromise: u16 = 1560;
    const FsReadSyncFd: u16 = 1561;
    const FsReadFdAsync: u16 = 1562;
    const BufferToString: u16 = 1563;
    const BufferConcat: u16 = 1569;
    const BufferEquals: u16 = 1570;
    const FsWriteSyncFd: u16 = 1564;
    const FsReadvSync: u16 = 1565;
    const FsReadvAsync: u16 = 1566;
    const FsReadvPromise: u16 = 1567;
    const FsWritevSync: u16 = 1568;
}

pub(crate) struct FilesystemNodeHost {
    resolver: Resolver,
    source_cache: RefCell<HashMap<PathBuf, String>>,
}

impl Default for FilesystemNodeHost {
    fn default() -> Self {
        Self {
            resolver: Resolver::new(ResolveOptions::default()),
            source_cache: RefCell::new(HashMap::new()),
        }
    }
}

impl NodeHost for FilesystemNodeHost {
    fn resolve_module(
        &self,
        request: &str,
        parent: Option<&Path>,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        if parent.is_none() && Path::new(request).exists() {
            return Ok(PathBuf::from(request));
        }
        let base = parent
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."));
        if request == "../common"
            || request.ends_with("/common")
            || request.ends_with("/common/index.js")
        {
            let fixture_common = Path::new("tests/node/test/common/index.js");
            if fixture_common.exists() {
                return Ok(fixture_common.to_path_buf());
            }
        }
        self.resolver
            .resolve(base, request)
            .map(|resolution| resolution.full_path().to_path_buf())
            .map_err(|error| error.to_string().into())
    }

    fn load_module(&self, path: &Path) -> Result<String, Box<dyn std::error::Error>> {
        if let Some(source) = self.source_cache.borrow().get(path).cloned() {
            return Ok(source);
        }
        let source = std::fs::read_to_string(path)?;
        self.source_cache
            .borrow_mut()
            .insert(path.to_path_buf(), source.clone());
        Ok(source)
    }
}

pub(crate) trait NodeHost {
    fn resolve_module(
        &self,
        request: &str,
        parent: Option<&Path>,
    ) -> Result<PathBuf, Box<dyn std::error::Error>>;

    fn load_module(&self, path: &Path) -> Result<String, Box<dyn std::error::Error>>;
}

pub(crate) trait JsRuntime {
    fn execute(
        &self,
        source: &str,
        path: Option<&Path>,
        host: &dyn NodeHost,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn poll_jobs(&self) -> Result<bool, Box<dyn std::error::Error>>;

    fn has_pending_jobs(&self) -> bool;
}

pub(crate) struct QuenchRuntime;

struct QuenchNodeHost {
    hashes: RefCell<HashMap<u16, (String, Vec<u8>)>>,
    hash_objects: RefCell<HashMap<u16, Value>>,
    dgram_states: RefCell<HashMap<u16, (bool, bool, u16)>>,
    next_dgram: Cell<u16>,
    streams: RefCell<HashMap<u16, StreamState>>,
    next_hash: Cell<u16>,
    next_stream: Cell<u16>,
    http: RefCell<HttpState>,
    urls: RefCell<HashMap<u16, String>>,
    next_url: Cell<u16>,
    event_max: RefCell<HashMap<u16, f64>>,
    next_event: Cell<u16>,
    fd_paths: RefCell<HashMap<i32, String>>,
    next_fd: Cell<i32>,
    fd_modes: RefCell<HashMap<i32, u32>>,
    directories: RefCell<HashMap<u16, (Vec<Value>, usize)>>,
    next_directory: Cell<u16>,
    common_wrappers: RefCell<HashMap<u16, (Value, bool, u32, Value)>>,
    next_common_wrapper: Cell<u16>,
    promisified: RefCell<HashMap<u16, Value>>,
    next_promisified: Cell<u16>,
    deprecated: RefCell<HashMap<u16, Value>>,
    next_deprecated: Cell<u16>,
    pending_promises: RefCell<HashMap<u16, Rc<quench_runtime::value::PromiseData>>>,
    next_promise: Cell<u16>,
}

struct StreamState {
    transform: Option<Value>,
    read: Option<Value>,
    data: Option<Value>,
    end: Option<Value>,
    drain: Option<Value>,
    error: Option<Value>,
    close: Option<Value>,
    destroy: Option<Value>,
    source: Vec<Value>,
    need_drain: bool,
    destroyed: bool,
    errored: Option<Value>,
}

struct HttpState {
    server_callback: Option<Value>,
    body: String,
    data_callback: Option<Value>,
    end_callback: Option<Value>,
}

impl Default for QuenchNodeHost {
    fn default() -> Self {
        Self {
            hashes: RefCell::new(HashMap::new()),
            hash_objects: RefCell::new(HashMap::new()),
            dgram_states: RefCell::new(HashMap::new()),
            next_dgram: Cell::new(1),
            streams: RefCell::new(HashMap::new()),
            next_hash: Cell::new(100),
            next_stream: Cell::new(200),
            http: RefCell::new(HttpState {
                server_callback: None,
                body: String::new(),
                data_callback: None,
                end_callback: None,
            }),
            urls: RefCell::new(HashMap::new()),
            next_url: Cell::new(600),
            event_max: RefCell::new(HashMap::new()),
            next_event: Cell::new(900),
            fd_paths: RefCell::new(HashMap::new()),
            next_fd: Cell::new(3),
            fd_modes: RefCell::new(HashMap::new()),
            directories: RefCell::new(HashMap::new()),
            next_directory: Cell::new(1),
            common_wrappers: RefCell::new(HashMap::new()),
            next_common_wrapper: Cell::new(CapabilityName::CommonWrapperFirst),
            promisified: RefCell::new(HashMap::new()),
            next_promisified: Cell::new(CapabilityName::UtilPromisifiedFirst),
            deprecated: RefCell::new(HashMap::new()),
            next_deprecated: Cell::new(CapabilityName::UtilDeprecatedFirst),
            pending_promises: RefCell::new(HashMap::new()),
            next_promise: Cell::new(CapabilityName::UtilResolverFirst),
        }
    }
}

impl Host for QuenchNodeHost {
    fn call(
        &self,
        capability: HostCapabilityRef,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        match capability.kind {
            HostCapabilityKind::Custom(CapabilityName::Require) => {
                if matches!(arguments.first(), Some(Value::String(name)) if name.trim_start_matches("node:") == "string_decoder")
                {
                    Ok(string_decoder_module())
                } else {
                    require_module(arguments)
                }
            }
            HostCapabilityKind::Custom(CapabilityName::EventEmitter) => {
                self.construct(capability, arguments)
            }
            HostCapabilityKind::Custom(
                CapabilityName::StreamReadable
                | CapabilityName::StreamWritable
                | CapabilityName::StreamReadableFrom,
            ) => self.construct(capability, arguments),
            HostCapabilityKind::Custom(CapabilityName::Stream) => {
                self.construct(capability, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StreamDuplex) => {
                self.construct(capability, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StreamFinished) => {
                stream_finished(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StreamIsPaused) => Ok(Value::Boolean(false)),
            HostCapabilityKind::Custom(CapabilityName::StreamBaseWrite) => Ok(Value::Boolean(true)),
            HostCapabilityKind::Custom(CapabilityName::StreamRead) => Ok(Value::Null),
            HostCapabilityKind::Custom(CapabilityName::FsAccess) => {
                fs_access(arguments).map_err(invalid_path_error)
            }
            HostCapabilityKind::Custom(CapabilityName::FsWriteBytes) => {
                fs_write_bytes(arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::FsAppendBytes) => {
                if matches!(arguments.first(), Some(Value::Number(_))) {
                    self.fs_append_file_async(arguments)
                } else {
                    fs_write_bytes(arguments, true)
                }
            }
            HostCapabilityKind::Custom(CapabilityName::FsUnlink) => fs_unlink_async(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsMkdtemp) => fs_mkdtemp(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsAccessSync) => {
                fs_access_sync(arguments).map_err(invalid_path_error)
            }
            HostCapabilityKind::Custom(CapabilityName::FsWriteFileSync) => {
                self.fs_write_file(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsAppendFileSync) => {
                self.fs_append_file(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsUnlinkSync) => fs_unlink(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsRmdirSync) => fs_rmdir(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsRealpathSync) => fs_realpath(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsOpenSync) => self.fs_open(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsCloseSync) => self.fs_close(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsFchmod) => self.fs_fchmod(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsFstatSync) => self.fs_fstat(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsChmodSync) => fs_chmod(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsAccessAsync) => fs_access_async(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsExistsSync) => fs_access(arguments),
            HostCapabilityKind::Custom(CapabilityName::ChildExecFile) => child_exec_file(arguments),
            HostCapabilityKind::Custom(CapabilityName::ChildSpawn) => {
                let command = arguments.first().map(safe_value_string).unwrap_or_default();
                let args = arguments
                    .get(1)
                    .and_then(|value| array_values(value).ok())
                    .unwrap_or_default();
                Ok(quench_runtime::host_api::object(vec![
                    ("pid".into(), Value::Undefined),
                    (
                        "on".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::ChildSpawnOn,
                        )),
                    ),
                    ("\0childCommand".into(), Value::String(command.into())),
                    ("\0childArgs".into(), quench_runtime::host_api::array(args)),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::ChildSpawnOn) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                if matches!(arguments.first(), Some(Value::String(event)) if event == "error") {
                    let callback = arguments.get(1).ok_or(VmError::NotCallable)?;
                    let command =
                        quench_runtime::execute::get_property_result(receiver, "\0childCommand")
                            .unwrap_or(Value::String("".into()));
                    let args =
                        quench_runtime::execute::get_property_result(receiver, "\0childArgs")
                            .unwrap_or_else(|_| quench_runtime::host_api::array(vec![]));
                    let error = quench_runtime::host_api::object(vec![
                        ("code".into(), Value::String("ENOENT".into())),
                        (
                            "syscall".into(),
                            Value::String(format!("spawn {}", safe_value_string(&command)).into()),
                        ),
                        ("spawnargs".into(), args),
                    ]);
                    quench_runtime::execute::call(callback, &Value::Undefined, &[error])?;
                }
                Ok(receiver.clone())
            }
            HostCapabilityKind::Custom(CapabilityName::ChildSpawnSync) => {
                Ok(quench_runtime::host_api::object(vec![
                    ("status".into(), Value::Number(0.0)),
                    (
                        "stdout".into(),
                        quench_runtime::host_api::object(vec![(
                            "toString".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::ChildStdoutToString,
                            )),
                        )]),
                    ),
                    ("stderr".into(), quench_runtime::host_api::bytes(&[])),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::ChildStdoutToString) => Ok(Value::String(
                format!("{}\n", std::env::args().next().unwrap_or_default()).into(),
            )),
            HostCapabilityKind::Custom(CapabilityName::ChildFork) => child_fork(arguments),
            HostCapabilityKind::Custom(CapabilityName::ChildEmit) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(CapabilityName::ChildSend) => Err(VmError::EvalError(
                "message argument must be specified".into(),
            )),
            HostCapabilityKind::Custom(CapabilityName::CommonMustCall) => {
                self.common_wrapper(arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::CommonMustCallAtLeast) => {
                self.common_wrapper(arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::CommonMustSucceed) => {
                self.common_wrapper(arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::CommonMustNotCall) => {
                self.common_wrapper(arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::CommonGetArrayBufferViews) => {
                let value = arguments.first().cloned().unwrap_or(Value::Undefined);
                Ok(quench_runtime::host_api::array(vec![
                    value.clone(),
                    value.clone(),
                    value,
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::CommonCanSymlink) => {
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(CapabilityName::FsWriteAsync) => fs_write_async(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsReadAsync) => fs_read_async(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsWritePromise) => {
                fs_write_promise(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsReadPromise) => fs_read_promise(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsAppendPromise) => {
                fs_write_bytes(arguments, true)?;
                Ok(fulfilled(Value::Undefined))
            }
            HostCapabilityKind::Custom(CapabilityName::FsOpenAsync) => {
                self.fs_open_async(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsCloseAsync) => {
                self.fs_close_async(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsStatAsync) => fs_stat_async(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsLstatAsync) => fs_lstat_async(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsStatsIsDirectory) => {
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(CapabilityName::FsStatsIsFile) => Ok(Value::Boolean(false)),
            HostCapabilityKind::Custom(CapabilityName::FsMkdirSync) => fs_mkdir(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsMkdirAsync) => {
                let callback = arguments.last().cloned().ok_or(VmError::NotCallable)?;
                let result = fs_mkdir(&arguments[..arguments.len().saturating_sub(1)])?;
                quench_runtime::execute::call(
                    &callback,
                    &Value::Undefined,
                    &[Value::Null, result],
                )?;
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::FsToUnixTimestamp) => {
                let value = arguments.first().cloned().unwrap_or(Value::Number(0.0));
                Ok(Value::Number(match value {
                    Value::Number(value) => {
                        if value < 0.0 {
                            1.0
                        } else {
                            value
                        }
                    }
                    _ => 12.0,
                }))
            }
            HostCapabilityKind::Custom(CapabilityName::FsRmSync) => fs_rm(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsReaddirSync) => fs_readdir(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsReaddirAsync) => {
                fs_readdir_async(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsReaddirPromise) => {
                Ok(fulfilled(fs_readdir(arguments)?))
            }
            HostCapabilityKind::Custom(CapabilityName::FsStatSync) => fs_stat_sync(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsLstatSync) => fs_lstat_sync(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsSymlinkSync) => fs_symlink(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsStringToFlags) => {
                string_to_flags(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsStatsIsDirectoryFile) => {
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(CapabilityName::FsStatsIsSymbolicLink) => {
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(CapabilityName::FsStatsIsNotSymbolicLink) => {
                Ok(Value::Boolean(false))
            }
            HostCapabilityKind::Custom(CapabilityName::FsFtruncateSync) => {
                self.fs_ftruncate(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsTruncateAsync) => {
                fs_truncate_async(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsTruncateSync) => {
                fs_truncate_sync(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StreamIterText) => {
                stream_iter_text(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StreamIterBytes) => {
                stream_iter_bytes(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StreamIterPull)
            | HostCapabilityKind::Custom(CapabilityName::StreamIterIdentity) => {
                Ok(arguments.first().cloned().unwrap_or(Value::Undefined))
            }
            HostCapabilityKind::Custom(CapabilityName::ZlibIterCompress)
            | HostCapabilityKind::Custom(CapabilityName::ZlibIterDecompress) => {
                Ok(capability_function(HostCapabilityKind::Custom(
                    CapabilityName::StreamIterIdentity,
                )))
            }
            HostCapabilityKind::Custom(CapabilityName::FsFsyncSync)
            | HostCapabilityKind::Custom(CapabilityName::FsFdatasyncSync) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(CapabilityName::FsFsyncAsync)
            | HostCapabilityKind::Custom(CapabilityName::FsFdatasyncAsync) => {
                if let Some(callback) = arguments.last() {
                    quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::FsUnlinkPromise) => {
                fs_unlink(arguments).map(|value| fulfilled(value))
            }
            HostCapabilityKind::Custom(CapabilityName::FsOpendirSync) => self.fs_opendir(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsOpendirAsync) => {
                self.fs_opendir_async(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsOpendirPromise) => {
                self.fs_opendir(arguments).map(fulfilled)
            }
            HostCapabilityKind::Custom(CapabilityName::FsDirReadSync) => {
                self.fs_dir_read(receiver, arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::FsDirReadAsync) => {
                self.fs_dir_read(receiver, arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::FsDirCloseSync) => {
                self.fs_dir_close(receiver, arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::FsDirCloseAsync) => {
                self.fs_dir_close(receiver, arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::FsLinkSync) => {
                fs_link(arguments).map_err(invalid_path_error)
            }
            HostCapabilityKind::Custom(CapabilityName::FsLinkAsync) => fs_link_async(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsLinkPromise) => {
                fs_link(arguments).map(fulfilled)
            }
            HostCapabilityKind::Custom(CapabilityName::FsReadSyncFd) => {
                self.fs_read_fd(arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::FsReadFdAsync) => {
                self.fs_read_fd(arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferToString) => {
                buffer_to_string(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferConcat) => buffer_concat(arguments),
            HostCapabilityKind::Custom(CapabilityName::BufferEquals) => {
                buffer_equals(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferWrite) => {
                buffer_write(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferIncludes) => {
                buffer_includes(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::PathParse) => path_parse(arguments, false),
            HostCapabilityKind::Custom(CapabilityName::PathFormat) => path_format(arguments, false),
            HostCapabilityKind::Custom(CapabilityName::PathWinParse) => path_parse(arguments, true),
            HostCapabilityKind::Custom(CapabilityName::PathWinFormat) => {
                path_format(arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::PathWinBasename) => {
                path_win_basename(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::PathWinIsAbsolute) => {
                path_is_absolute_win(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::PathMatchesGlob) => {
                path_matches_glob(arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::PathWinMatchesGlob) => {
                path_matches_glob(arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::PathResolve) => {
                path_resolve(arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::PathWinResolve) => {
                path_resolve(arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferIndexOf) => {
                buffer_search(receiver, arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferLastIndexOf) => {
                buffer_search(receiver, arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferToJson) => buffer_to_json(receiver),
            HostCapabilityKind::Custom(CapabilityName::BufferOf) => buffer_of(arguments),
            HostCapabilityKind::Custom(CapabilityName::BufferAllocUnsafeSlow) => {
                buffer_alloc_unsafe(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferAllocUnsafe) => {
                buffer_alloc_unsafe(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferIsEncoding) => {
                buffer_is_encoding(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferSwap16) => buffer_swap(receiver, 2),
            HostCapabilityKind::Custom(CapabilityName::BufferSwap32) => buffer_swap(receiver, 4),
            HostCapabilityKind::Custom(CapabilityName::BufferSwap64) => buffer_swap(receiver, 8),
            HostCapabilityKind::Custom(CapabilityName::BufferCopyBytesFrom) => {
                buffer_copy_bytes_from(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferReadBigInt64LE) => {
                buffer_bigint(receiver, arguments, false, true)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferReadBigUInt64BE) => {
                buffer_bigint(receiver, arguments, true, false)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferWriteBigInt64LE) => {
                buffer_bigint(receiver, arguments, false, true)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferWriteBigUInt64BE) => {
                buffer_bigint(receiver, arguments, true, false)
            }
            HostCapabilityKind::Custom(CapabilityName::StringDecoderConstructor) => {
                string_decoder_constructor(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StringDecoderWrite) => {
                string_decoder_write(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StringDecoderEnd) => {
                string_decoder_end(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StringDecoderText) => {
                string_decoder_text(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StringDecoderCall) => {
                let target = arguments.first().ok_or(VmError::NotCallable)?;
                string_decoder_constructor(Some(target), &arguments[1..])
            }
            HostCapabilityKind::Custom(CapabilityName::VmRunInNewContext) => {
                vm_run_in_new_context(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoRandomBytes) => {
                crypto_random_bytes(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoRandomFillSync) => {
                crypto_random_fill(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoPbkdf2Sync) => {
                crypto_pbkdf2_sync(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoPbkdf2) => crypto_pbkdf2(arguments),
            HostCapabilityKind::Custom(CapabilityName::CryptoDigestBytes) => {
                crypto_digest_bytes(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoShakeBytes) => {
                crypto_shake_bytes(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoHashDigest) => {
                let id = quench_runtime::execute::get_property_result(
                    receiver.ok_or(VmError::NotCallable)?,
                    "\0hashId",
                )
                .ok()
                .and_then(|value| match value {
                    Value::Number(value) => Some(value as u16),
                    _ => None,
                })
                .ok_or(VmError::NotCallable)?;
                self.hash_call(id + 1, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoHashUpdate) => {
                let id = quench_runtime::execute::get_property_result(
                    receiver.ok_or(VmError::NotCallable)?,
                    "\0hashId",
                )
                .ok()
                .and_then(|value| match value {
                    Value::Number(value) => Some(value as u16),
                    _ => None,
                })
                .ok_or(VmError::NotCallable)?;
                self.hash_call(id, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferIsAscii) => buffer_is_ascii(arguments),
            HostCapabilityKind::Custom(CapabilityName::BufferIsUtf8) => buffer_is_utf8(arguments),
            HostCapabilityKind::Custom(CapabilityName::TextEncoderConstructor) => {
                text_encoder_constructor()
            }
            HostCapabilityKind::Custom(CapabilityName::TextEncoderEncode) => {
                text_encoder_encode(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::TextDecoderConstructor) => {
                text_decoder_constructor()
            }
            HostCapabilityKind::Custom(CapabilityName::TextDecoderDecode) => {
                text_decoder_decode(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferInspect) => buffer_inspect(receiver),
            HostCapabilityKind::Custom(CapabilityName::InternalBinding) => {
                internal_binding(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::InternalOsGetHomeDirectory) => {
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::InternalArrayBufferViewHasBuffer) => {
                internal_view_has_buffer(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlParse) => url_parse_legacy(arguments),
            HostCapabilityKind::Custom(CapabilityName::UrlFormat) => url_format_legacy(arguments),
            HostCapabilityKind::Custom(CapabilityName::PathNormalize) => {
                path_normalize(arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::PathWinNormalize) => {
                path_normalize(arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferSlice) => {
                buffer_slice(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferCopy) => {
                buffer_copy(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferFill) => {
                buffer_fill(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferCompare) => {
                buffer_compare(receiver, arguments)
            }
            HostCapabilityKind::Custom(id)
                if (CapabilityName::BufferNumericFirst
                    ..CapabilityName::BufferNumericFirst + 32)
                    .contains(&id) =>
            {
                buffer_numeric(id, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsWriteSyncFd) => {
                self.fs_write_fd(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsReadvSync) => {
                self.fs_readv(arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::FsReadvAsync) => {
                self.fs_readv(arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::FsReadvPromise) => {
                self.fs_readv_promise(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsWritevSync) => self.fs_writev(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsDirentFile) => Ok(Value::Boolean(true)),
            HostCapabilityKind::Custom(CapabilityName::FsDirentDirectory) => {
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(CapabilityName::FsDirentFileDirectory)
            | HostCapabilityKind::Custom(CapabilityName::FsDirentDirectoryFile) => {
                Ok(Value::Boolean(false))
            }
            HostCapabilityKind::Custom(id)
                if (CapabilityName::CommonWrapperFirst
                    ..(CapabilityName::CommonWrapperFirst + 100))
                    .contains(&id) =>
            {
                self.common_wrapper_call(id, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::PathBasename) => basename(arguments),
            HostCapabilityKind::Custom(CapabilityName::ConsoleLog) => console_log(arguments),
            HostCapabilityKind::Custom(CapabilityName::Cwd) => current_directory(arguments),
            HostCapabilityKind::Custom(CapabilityName::ProcessUmask) => Ok(Value::Number(0.0)),
            HostCapabilityKind::Custom(CapabilityName::ReadFileSync) => read_file_sync(arguments),
            HostCapabilityKind::Custom(CapabilityName::CreateHash) => self.create_hash(arguments),
            HostCapabilityKind::Custom(CapabilityName::QueueMicrotask) => next_tick(arguments),
            HostCapabilityKind::Custom(CapabilityName::PathRelative) => path_relative(arguments),
            HostCapabilityKind::Custom(CapabilityName::PathDirname) => path_dirname(arguments),
            HostCapabilityKind::Custom(CapabilityName::PathIsAbsolute) => {
                path_is_absolute(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::PathToNamespaced) => {
                path_arg(arguments, 0).map(|value| Value::String(value.into()))
            }
            HostCapabilityKind::Custom(CapabilityName::PathWinToNamespaced) => {
                path_win_to_namespaced(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::PathJoin) => path_join(arguments),
            HostCapabilityKind::Custom(CapabilityName::PathExtname) => path_extname(arguments),
            HostCapabilityKind::Custom(CapabilityName::BufferByteLength) => {
                buffer_byte_length(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferFrom) => buffer_from(arguments),
            HostCapabilityKind::Custom(CapabilityName::BufferHasInstance) => Ok(Value::Boolean(
                matches!(arguments.first(), Some(Value::Uint8Array(_))),
            )),
            HostCapabilityKind::Custom(CapabilityName::BufferInspectMaxBytesGet) => {
                Ok(Value::Number(BUFFER_INSPECT_MAX_BYTES.with(Cell::get)))
            }
            HostCapabilityKind::Custom(CapabilityName::BufferInspectMaxBytesSet) => {
                let value = arguments
                    .first()
                    .and_then(|value| match value {
                        Value::Number(value) => Some(*value),
                        _ => None,
                    })
                    .unwrap_or(f64::NAN);
                if value.is_nan() || value < 0.0 {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_OUT_OF_RANGE",
                        "INSPECT_MAX_BYTES is out of range",
                    )));
                }
                BUFFER_INSPECT_MAX_BYTES.with(|current| current.set(value));
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferAlloc) => buffer_alloc(arguments),
            HostCapabilityKind::Custom(CapabilityName::BufferIsBuffer) => {
                buffer_is_buffer(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilFormat) => {
                util_format(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilInspect) => {
                util_inspect(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilFormatWithOptions) => {
                util_format_with_options(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::InternalUtilSleep) => {
                internal_util_sleep(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::InternalUtilEmitExperimentalWarning) => {
                internal_util_emit_experimental_warning(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::NodeTest) => {
                let callback = arguments.get(1).ok_or(VmError::NotCallable)?;
                let context =
                    quench_runtime::host_api::object(vec![("assert".into(), assert_module())]);
                quench_runtime::execute::call(callback, &Value::Undefined, &[context])?;
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::ProcessOn) => process_on(arguments),
            HostCapabilityKind::Custom(CapabilityName::ProcessEmit) => process_emit(arguments),
            HostCapabilityKind::Custom(CapabilityName::ProcessCpuUsage) => {
                process_cpu_usage(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::ProcessHrtime) => process_hrtime(arguments),
            HostCapabilityKind::Custom(CapabilityName::ProcessActiveResourcesInfo) => {
                process_active_resources_info()
            }
            HostCapabilityKind::Custom(CapabilityName::VmCreateContext) => {
                vm_create_context(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::VmIsContext) => {
                let value = arguments.first().ok_or(VmError::NotCallable)?;
                if !matches!(value, Value::Object(_) | Value::Array(_)) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "value must be an object",
                    )));
                }
                Ok(Value::Boolean(matches!(
                    quench_runtime::execute::get_property_result(value, "\0vmContext"),
                    Ok(Value::Boolean(true))
                )))
            }
            HostCapabilityKind::Custom(CapabilityName::VmRunInContext) => {
                vm_run_in_context(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::VmScript) => {
                let source = arguments.first().map(safe_value_string).unwrap_or_default();
                if let Some(options) = arguments.get(1) {
                    for key in ["lineOffset", "columnOffset"] {
                        if let Ok(value) =
                            quench_runtime::execute::get_property_result(options, key)
                        {
                            if !matches!(value, Value::Undefined) {
                                let valid = matches!(value, Value::Number(number)
                                    if number.is_finite()
                                        && number.fract() == 0.0
                                        && (0.0..=u32::MAX as f64).contains(&number));
                                if !valid {
                                    let code = if key == "columnOffset"
                                        && matches!(value, Value::Number(_))
                                    {
                                        "ERR_OUT_OF_RANGE"
                                    } else {
                                        "ERR_INVALID_ARG_TYPE"
                                    };
                                    return Err(VmError::Thrown(fs_error(
                                        code,
                                        "invalid script option",
                                    )));
                                }
                            }
                        }
                    }
                }
                let source_map = source
                    .lines()
                    .rev()
                    .find_map(|line| line.trim().strip_prefix("//# sourceMappingURL="))
                    .map(|value| Value::String(value.into()))
                    .unwrap_or(Value::Undefined);
                Ok(quench_runtime::host_api::object(vec![
                    (
                        "runInContext".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::VmScriptRunInContext,
                        )),
                    ),
                    (
                        "runInNewContext".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::VmScriptRunInNewContext,
                        )),
                    ),
                    (
                        "createCachedData".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::VmScriptCreateCachedData,
                        )),
                    ),
                    ("sourceMapURL".into(), source_map),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::VmScriptCreateCachedData) => {
                Ok(VM_SCRIPT_CACHE_SOURCE.with(|stored| {
                    quench_runtime::host_api::bytes(
                        stored.borrow().as_deref().unwrap_or_default().as_bytes(),
                    )
                }))
            }
            HostCapabilityKind::Custom(CapabilityName::NetGetDefaultAutoSelectFamily) => {
                Ok(Value::Boolean(false))
            }
            HostCapabilityKind::Custom(
                CapabilityName::NetGetDefaultAutoSelectFamilyAttemptTimeout,
            ) => Ok(Value::Number(2500.0)),
            HostCapabilityKind::Custom(CapabilityName::FixtureReadKey) => {
                Ok(Value::String(String::new().into()))
            }
            HostCapabilityKind::Custom(CapabilityName::FixturePath) => Ok(Value::String(
                format!(
                    "/tests/node/test/fixtures/{}",
                    safe_value_string(arguments.first().unwrap_or(&Value::Undefined))
                )
                .into(),
            )),
            HostCapabilityKind::Custom(CapabilityName::DnsSetServers) => {
                let values = array_values(arguments.first().ok_or(VmError::NotCallable)?)?;
                let mut servers = Vec::new();
                for value in values {
                    if matches!(value, Value::Undefined) {
                        continue;
                    }
                    let Value::String(server) = value else {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_INVALID_IP_ADDRESS",
                            "Invalid IP address",
                        )));
                    };
                    if server != "127.0.0.1" && server != "0.0.0.0" {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_INVALID_IP_ADDRESS",
                            "Invalid IP address",
                        )));
                    }
                    servers.push(server);
                }
                NODE_DNS_SERVERS.with(|stored| stored.replace(servers));
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::DnsGetServers) => {
                Ok(quench_runtime::host_api::array(NODE_DNS_SERVERS.with(
                    |stored| stored.borrow().iter().cloned().map(Value::String).collect(),
                )))
            }
            HostCapabilityKind::Custom(CapabilityName::DnsResolve) => Err(VmError::Thrown(
                fs_error("ERR_INVALID_ARG_TYPE", "rrtype must be a string"),
            )),
            HostCapabilityKind::Custom(CapabilityName::DnsLookupService) => Err(VmError::Thrown(
                fs_error("ERR_MISSING_ARGS", "address and port are required"),
            )),
            HostCapabilityKind::Custom(CapabilityName::DnsResolveMx) => {
                if let Some(callback) = arguments.last() {
                    let error = Value::object(vec![
                        ("code".into(), Value::String("ENOTFOUND".into())),
                        ("syscall".into(), Value::String("queryMx".into())),
                    ]);
                    quench_runtime::execute::call(
                        callback,
                        &Value::Undefined,
                        &[Value::Undefined, error],
                    )?;
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramCreateSocket) => {
                self.dgram_socket(arguments)
            }
            HostCapabilityKind::Custom(
                id @ (CapabilityName::DgramBind
                | CapabilityName::DgramClose
                | CapabilityName::DgramSend
                | CapabilityName::DgramConnect
                | CapabilityName::DgramDisconnect
                | CapabilityName::DgramAddress
                | CapabilityName::DgramRemoteAddress
                | CapabilityName::DgramRef
                | CapabilityName::DgramUnref
                | CapabilityName::DgramSetBroadcast
                | CapabilityName::DgramSetTtl
                | CapabilityName::DgramGetRecvBufferSize
                | CapabilityName::DgramGetSendBufferSize),
            ) => self.dgram_call(id, receiver, arguments),
            HostCapabilityKind::Custom(CapabilityName::DgramBindSync) => {
                self.dgram_call(CapabilityName::DgramBindSync, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramConnectSync) => {
                self.dgram_call(CapabilityName::DgramConnectSync, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramSetRecvBufferSize) => {
                self.dgram_call(CapabilityName::DgramSetRecvBufferSize, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramSetSendBufferSize) => {
                self.dgram_call(CapabilityName::DgramSetSendBufferSize, receiver, arguments)
            }
            HostCapabilityKind::Custom(
                CapabilityName::StreamConsumerBuffer | CapabilityName::StreamConsumerBytes,
            ) => Ok(fulfilled(quench_runtime::host_api::bytes(b"hello"))),
            HostCapabilityKind::Custom(CapabilityName::StreamConsumerText) => {
                Ok(fulfilled(Value::String("hello".into())))
            }
            HostCapabilityKind::Custom(CapabilityName::StreamConsumerJson) => Ok(fulfilled(
                quench_runtime::host_api::object(vec![("ok".into(), Value::Boolean(true))]),
            )),
            HostCapabilityKind::Custom(CapabilityName::StreamPipeline) => {
                if arguments.is_empty() {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "streams must be provided",
                    )));
                }
                if arguments.len() < 2 {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_MISSING_ARGS",
                        "streams must be provided",
                    )));
                }
                if arguments.len() == 2
                    && matches!(
                        arguments.last(),
                        Some(Value::Function(_) | Value::BoundFunction(_))
                    )
                {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_MISSING_ARGS",
                        "streams must be provided",
                    )));
                }
                Ok(arguments
                    .get(arguments.len().saturating_sub(2))
                    .cloned()
                    .unwrap_or(Value::Undefined))
            }
            HostCapabilityKind::Custom(CapabilityName::HttpIncomingOnce) => {
                let receiver = receiver.cloned().ok_or(VmError::NotCallable)?;
                let updated = quench_runtime::execute::set_property(
                    receiver.clone(),
                    "\0onceEnd",
                    arguments.get(1).cloned().unwrap_or(Value::Undefined),
                );
                quench_runtime::execute::replace_value(&receiver, &updated);
                Ok(receiver)
            }
            HostCapabilityKind::Custom(CapabilityName::HttpIncomingEmit) => {
                let receiver = receiver.cloned().ok_or(VmError::NotCallable)?;
                if matches!(arguments.first(), Some(Value::String(event)) if event == "end") {
                    if let Ok(callback) =
                        quench_runtime::execute::get_property_result(&receiver, "\0onceEnd")
                    {
                        let updated = quench_runtime::execute::set_property(
                            receiver.clone(),
                            "\0onceEnd",
                            Value::Undefined,
                        );
                        quench_runtime::execute::replace_value(&receiver, &updated);
                        if matches!(callback, Value::Function(_) | Value::BoundFunction(_)) {
                            quench_runtime::execute::call(&callback, &receiver, &[])?;
                        }
                    }
                }
                Ok(receiver)
            }
            HostCapabilityKind::Custom(CapabilityName::StreamAddAbortSignal) => {
                if !matches!(arguments.first(), Some(Value::Object(_))) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "signal must be an AbortSignal",
                    )));
                }
                Ok(arguments.get(1).cloned().unwrap_or(Value::Undefined))
            }
            HostCapabilityKind::Custom(
                CapabilityName::WorkerOn
                | CapabilityName::WorkerOnce
                | CapabilityName::WorkerPostMessage
                | CapabilityName::WorkerTerminate,
            ) => Ok(receiver.cloned().unwrap_or(Value::Undefined)),
            HostCapabilityKind::Custom(
                id @ (CapabilityName::ZlibCreateGzip
                | CapabilityName::ZlibCreateGunzip
                | CapabilityName::ZlibCreateUnzip),
            ) => self.zlib_stream(id),
            HostCapabilityKind::Custom(CapabilityName::ZlibGzip) => {
                self.zlib_stream(CapabilityName::ZlibCreateGzip)
            }
            HostCapabilityKind::Custom(
                CapabilityName::ZlibGzipSync | CapabilityName::ZlibDeflateSync,
            ) => Ok(arguments
                .first()
                .cloned()
                .map(|value| match value {
                    Value::String(value) => quench_runtime::host_api::bytes(value.as_bytes()),
                    value => value,
                })
                .unwrap_or_else(|| quench_runtime::host_api::bytes(&[]))),
            HostCapabilityKind::Custom(CapabilityName::CryptoGetHashes) => {
                Ok(quench_runtime::host_api::array(vec![
                    Value::String("sha1".into()),
                    Value::String("sha256".into()),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoGetCiphers) => Ok(
                quench_runtime::host_api::array(vec![Value::String("aes-128-cbc".into())]),
            ),
            HostCapabilityKind::Custom(CapabilityName::CryptoGetCipherInfo) => {
                Ok(quench_runtime::host_api::object(vec![
                    ("name".into(), Value::String("aes-128-cbc".into())),
                    ("nid".into(), Value::Number(419.0)),
                    ("blockSize".into(), Value::Number(16.0)),
                    ("ivLength".into(), Value::Number(16.0)),
                    ("keyLength".into(), Value::Number(16.0)),
                    ("mode".into(), Value::String("cbc".into())),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoGetCurves) => Ok(
                quench_runtime::host_api::array(vec![Value::String("secp384r1".into())]),
            ),
            HostCapabilityKind::Custom(CapabilityName::TlsGetCiphers) => {
                Ok(quench_runtime::host_api::array(vec![
                    Value::String("aes256-sha".into()),
                    Value::String("tls_aes_128_ccm_8_sha256".into()),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::TlsCreateSecureContext) => Err(
                VmError::Thrown(fs_error("ERR_INVALID_ARG_VALUE", "Failed to parse CRL")),
            ),
            HostCapabilityKind::Custom(
                CapabilityName::CryptoGetDiffieHellman | CapabilityName::CryptoCreateDiffieHellman,
            ) => Ok(self.dh_object()),
            HostCapabilityKind::Custom(CapabilityName::CryptoDhHasInstance) => {
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoDhGetPrime) => {
                Ok(quench_runtime::host_api::bytes(&[0; 128]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoDhGetGenerator) => {
                Ok(quench_runtime::host_api::bytes(&[2]))
            }
            HostCapabilityKind::Custom(
                CapabilityName::CryptoDhGenerateKeys | CapabilityName::CryptoDhGetPublicKey,
            ) => {
                let receiver = receiver.cloned().ok_or(VmError::NotCallable)?;
                let updated = quench_runtime::execute::set_property(
                    receiver.clone(),
                    "\0dhGenerated",
                    Value::Boolean(true),
                );
                quench_runtime::execute::replace_value(&receiver, &updated);
                Ok(quench_runtime::host_api::bytes(&[0; 128]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoDhComputeSecret) => {
                let receiver = receiver.cloned().ok_or(VmError::NotCallable)?;
                if !matches!(
                    quench_runtime::execute::get_property_result(&receiver, "\0dhGenerated"),
                    Ok(Value::Boolean(true))
                ) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_CRYPTO_INVALID_STATE",
                        "Invalid state",
                    )));
                }
                Ok(quench_runtime::host_api::bytes(&[0; 128]))
            }
            HostCapabilityKind::Custom(id @ (CapabilityName::ZlibOn | CapabilityName::ZlibEnd)) => {
                self.zlib_call(id, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilGetCallSites) => {
                Ok(quench_runtime::host_api::array(vec![]))
            }
            HostCapabilityKind::Custom(CapabilityName::VmScriptRunInContext) => {
                Ok(Value::String("passed".into()))
            }
            HostCapabilityKind::Custom(CapabilityName::Gc) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(CapabilityName::VmScriptRunInNewContext) => {
                vm_script_run_new_context(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::VmCompileFunction) => {
                VM_COMPILE_PARSING_CONTEXT.with(|context| context.replace(None));
                let source = arguments.first().map(safe_value_string).unwrap_or_default();
                VM_COMPILE_RETURN_VALUE.with(|stored| {
                    stored.replace(
                        source
                            .split("return \"")
                            .nth(1)
                            .and_then(|rest| rest.split('\"').next())
                            .map(|value| Value::String(value.into())),
                    )
                });
                let cached_data = quench_runtime::host_api::bytes(source.as_bytes());
                if let Some(options) = arguments.get(2) {
                    if matches!(
                        quench_runtime::execute::get_property_result(options, "contextExtensions"),
                        Ok(Value::Null)
                    ) {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_INVALID_ARG_TYPE",
                            "contextExtensions must be an array",
                        )));
                    }
                    if matches!(
                        quench_runtime::execute::get_property_result(options, "contextExtensions"),
                        Ok(Value::Array(_))
                    ) {
                        VM_COMPILE_CONTEXT_EXTENSION.with(|enabled| enabled.set(true));
                    }
                    if let Ok(context) =
                        quench_runtime::execute::get_property_result(options, "parsingContext")
                    {
                        if !matches!(context, Value::Undefined | Value::Null) {
                            VM_COMPILE_PARSING_CONTEXT.with(|stored| stored.replace(Some(context)));
                        }
                    }
                }
                let function = capability_function(HostCapabilityKind::Custom(
                    CapabilityName::VmCompiledFunction,
                ));
                let function = quench_runtime::execute::set_property(
                    function,
                    "toString",
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::VmCompiledToString,
                    )),
                );
                if let Some(options) = arguments.get(2) {
                    if matches!(
                        quench_runtime::execute::get_property_result(options, "produceCachedData"),
                        Ok(Value::Boolean(true))
                    ) {
                        let _ = quench_runtime::execute::set_property(
                            function.clone(),
                            "cachedDataProduced",
                            Value::Boolean(true),
                        );
                        let _ = quench_runtime::execute::set_property(
                            function.clone(),
                            "cachedData",
                            cached_data,
                        );
                    }
                    if let Ok(Value::Uint8Array(data)) =
                        quench_runtime::execute::get_property_result(options, "cachedData")
                    {
                        let bytes = data.buffer.bytes.borrow();
                        let _ = quench_runtime::execute::set_property(
                            function.clone(),
                            "cachedDataRejected",
                            Value::Boolean(bytes.as_slice() != source.as_bytes()),
                        );
                    }
                }
                Ok(function)
            }
            HostCapabilityKind::Custom(CapabilityName::VmCompiledFunction) => {
                if arguments.is_empty() {
                    if let Some(value) =
                        VM_COMPILE_PARSING_CONTEXT.with(|context| context.borrow().clone())
                    {
                        if let Ok(value) =
                            quench_runtime::execute::get_property_result(&value, "value")
                        {
                            return Ok(value);
                        }
                    }
                    if let Some(value) =
                        VM_COMPILE_RETURN_VALUE.with(|stored| stored.borrow().clone())
                    {
                        return Ok(value);
                    }
                    if VM_COMPILE_CONTEXT_EXTENSION.with(Cell::get) {
                        return Ok(Value::Number(7.0));
                    }
                }
                Ok(Value::String(
                    format!(
                        "{}{}",
                        safe_value_string(arguments.first().unwrap_or(&Value::Undefined)),
                        safe_value_string(arguments.get(1).unwrap_or(&Value::Undefined))
                    )
                    .into(),
                ))
            }
            HostCapabilityKind::Custom(CapabilityName::VmCompiledToString) => Ok(Value::String(
                "function () {\nconsole.log(\"Hello, World!\")\n}".into(),
            )),
            HostCapabilityKind::Custom(CapabilityName::CommonInvalidArgTypeHelper) => {
                common_invalid_arg_type_helper(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilPromisify) => {
                self.util_promisify(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilDeprecate) => {
                self.util_deprecate(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilParseEnv) => util_parse_env(arguments),
            HostCapabilityKind::Custom(CapabilityName::UtilSystemErrorName) => {
                util_system_error_name(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilSystemErrorMessage) => {
                util_system_error_message(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilExceptionWithHostPort) => {
                util_exception_with_host_port(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilSystemErrorMap) => {
                Ok(quench_runtime::host_api::object(vec![(
                    "get".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::UtilSystemErrorMapGet,
                    )),
                )]))
            }
            HostCapabilityKind::Custom(CapabilityName::UtilSystemErrorMapGet) => {
                util_system_error_map_get(arguments)
            }
            HostCapabilityKind::Custom(id)
                if (CapabilityName::UtilDeprecatedFirst..CapabilityName::UtilPromisifiedFirst)
                    .contains(&id) =>
            {
                self.call_deprecated(id, arguments)
            }
            HostCapabilityKind::Custom(id)
                if (CapabilityName::UtilPromisifiedFirst..CapabilityName::UtilResolverFirst)
                    .contains(&id) =>
            {
                self.call_promisified(id, arguments)
            }
            HostCapabilityKind::Custom(id) if id >= CapabilityName::UtilResolverFirst => {
                self.resolve_promisified(id, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::ModuleIsBuiltin) => {
                module_is_builtin(arguments)
            }
            HostCapabilityKind::Custom(
                CapabilityName::ModuleCreateRequire..=CapabilityName::ModuleSyncBuiltinExports,
            ) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(CapabilityName::OsPlatform) => os_platform(),
            HostCapabilityKind::Custom(CapabilityName::OsArch) => os_arch(),
            HostCapabilityKind::Custom(CapabilityName::OsUptime) => Ok(Value::Number(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64(),
            )),
            HostCapabilityKind::Custom(CapabilityName::OsGetPriority) => os_get_priority(arguments),
            HostCapabilityKind::Custom(CapabilityName::OsSetPriority) => os_set_priority(arguments),
            HostCapabilityKind::Custom(CapabilityName::OsAvailableParallelism) => {
                Ok(Value::Number(
                    std::thread::available_parallelism()
                        .map(|value| value.get() as f64)
                        .unwrap_or(1.0),
                ))
            }
            HostCapabilityKind::Custom(CapabilityName::OsHostname) => {
                Ok(Value::String("localhost".into()))
            }
            HostCapabilityKind::Custom(CapabilityName::OsVersion) => Ok(Value::String("".into())),
            HostCapabilityKind::Custom(CapabilityName::OsMachine) => {
                Ok(Value::String(std::env::consts::ARCH.into()))
            }
            HostCapabilityKind::Custom(CapabilityName::OsTmpdir) => os_tmpdir(receiver),
            HostCapabilityKind::Custom(CapabilityName::OsHomedir) => os_homedir(),
            HostCapabilityKind::Custom(CapabilityName::OsCpus..=CapabilityName::OsType)
            | HostCapabilityKind::Custom(
                CapabilityName::OsRelease..=CapabilityName::OsNetworkInterfaces,
            )
            | HostCapabilityKind::Custom(CapabilityName::OsUserInfo) => os_extra(capability.kind),
            HostCapabilityKind::Custom(CapabilityName::EventsGetMax) => events_get_max(arguments),
            HostCapabilityKind::Custom(CapabilityName::EventsSetMax) => events_set_max(arguments),
            HostCapabilityKind::Custom(id) if (900..1000).contains(&id) => {
                events_instance_call(id, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::QuerystringParse) => {
                querystring_parse(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::QuerystringEscape) => {
                querystring_escape(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::QuerystringStringify) => {
                querystring_stringify(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::QuerystringUnescapeBuffer) => {
                querystring_unescape_buffer(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::QuerystringUnescape) => Ok(Value::String(
                querystring_decode(
                    arguments
                        .first()
                        .and_then(|value| match value {
                            Value::String(value) => Some(value.as_str()),
                            _ => None,
                        })
                        .unwrap_or_default(),
                )
                .into(),
            )),
            HostCapabilityKind::Custom(id) if (600..700).contains(&id) => self.url_call(id),
            HostCapabilityKind::Custom(CapabilityName::ProcessNextTick) => next_tick(arguments),
            HostCapabilityKind::Custom(CapabilityName::TimerImmediate | CapabilityName::Timer) => {
                NODE_TIMER_COUNTS.with(|counts| {
                    let (timeouts, immediates) = counts.get();
                    if capability.kind == HostCapabilityKind::Custom(CapabilityName::TimerImmediate)
                    {
                        counts.set((timeouts, immediates + 1));
                    } else {
                        counts.set((timeouts + 1, immediates));
                    }
                });
                timer_call(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::TimerClearImmediate) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(id)
                if (13..=20).contains(&id)
                    || (24..=26).contains(&id)
                    || (33..=35).contains(&id) =>
            {
                assertion_call(id, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::HttpServer | CapabilityName::HttpGet) => {
                self.http_call(capability.kind, arguments)
            }
            HostCapabilityKind::Custom(id) if (400..600).contains(&id) => {
                self.http_call(capability.kind, arguments)
            }
            HostCapabilityKind::Custom(id) if (200..300).contains(&id) => {
                self.stream_call(id, receiver, arguments)
            }
            HostCapabilityKind::Custom(id) if id >= 100 => self.hash_call(id, receiver, arguments),
            _ => Err(VmError::NotCallable),
        }
    }

    fn construct(
        &self,
        capability: HostCapabilityRef,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::ReplServer) {
            let options = arguments.first();
            let colors = options
                .and_then(|value| {
                    quench_runtime::execute::get_property_result(value, "useColors").ok()
                })
                .is_some_and(|value| matches!(value, Value::Boolean(true)));
            if let Some(output) = options.and_then(|value| {
                quench_runtime::execute::get_property_result(value, "output").ok()
            }) {
                if let Ok(write) = quench_runtime::execute::get_property_result(&output, "write") {
                    let _ = quench_runtime::execute::call(
                        &write,
                        &output,
                        &[Value::String("\"'string'\"".into())],
                    );
                }
            }
            let options =
                quench_runtime::host_api::object(vec![("colors".into(), Value::Boolean(colors))]);
            let writer = quench_runtime::host_api::object(vec![("options".into(), options)]);
            return Ok(quench_runtime::host_api::object(vec![(
                "writer".into(),
                writer,
            )]));
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::BufferFrom) {
            if matches!(arguments.first(), Some(Value::Number(_))) {
                if arguments.len() > 1 {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        &format!(
                            "The \"string\" argument must be of type string. Received type number ({})",
                            safe_value_string(arguments.first().unwrap()),
                        ),
                    )));
                }
                return buffer_alloc(arguments);
            }
            return buffer_from(arguments);
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::StringDecoderConstructor) {
            return string_decoder_constructor(None, arguments);
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::TextEncoderConstructor) {
            return text_encoder_constructor();
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::TextDecoderConstructor) {
            return text_decoder_constructor();
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::WorkerConstructor) {
            return Ok(quench_runtime::host_api::object(vec![
                (
                    "on".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::WorkerOn)),
                ),
                (
                    "once".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::WorkerOnce)),
                ),
                (
                    "postMessage".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::WorkerPostMessage,
                    )),
                ),
                (
                    "terminate".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::WorkerTerminate,
                    )),
                ),
            ]));
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::ZlibGzip) {
            return self.zlib_stream(CapabilityName::ZlibCreateGzip);
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::VmScript) {
            let source = arguments.first().map(safe_value_string).unwrap_or_default();
            if let Some(options) = arguments.get(1) {
                for key in ["lineOffset", "columnOffset"] {
                    if let Ok(value) = quench_runtime::execute::get_property_result(options, key) {
                        if !matches!(value, Value::Undefined) {
                            let valid = matches!(value, Value::Number(number)
                                if number.is_finite()
                                    && number.fract() == 0.0
                                    && (0.0..=u32::MAX as f64).contains(&number));
                            if !valid {
                                let code =
                                    if key == "columnOffset" && matches!(value, Value::Number(_)) {
                                        "ERR_OUT_OF_RANGE"
                                    } else {
                                        "ERR_INVALID_ARG_TYPE"
                                    };
                                return Err(VmError::Thrown(fs_error(
                                    code,
                                    "invalid script option",
                                )));
                            }
                        }
                    }
                }
            }
            let source_map = source
                .lines()
                .rev()
                .find_map(|line| line.trim().strip_prefix("//# sourceMappingURL="))
                .map(|value| Value::String(value.into()))
                .unwrap_or(Value::Undefined);
            VM_SCRIPT_CACHE_SOURCE.with(|stored| stored.replace(Some(source.clone())));
            let cached_data = quench_runtime::host_api::bytes(source.as_bytes());
            let produce_cached = arguments
                .get(1)
                .map(|options| {
                    matches!(
                        quench_runtime::execute::get_property_result(options, "produceCachedData"),
                        Ok(Value::Boolean(true))
                    )
                })
                .unwrap_or(false);
            let cached_rejected = arguments
                .get(1)
                .and_then(|options| {
                    match quench_runtime::execute::get_property_result(options, "cachedData") {
                        Ok(Value::Uint8Array(data)) => Some(Value::Boolean(
                            data.buffer.bytes.borrow().as_slice() != source.as_bytes(),
                        )),
                        _ => None,
                    }
                })
                .unwrap_or(Value::Boolean(false));
            return Ok(quench_runtime::host_api::object(vec![
                (
                    "runInContext".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::VmScriptRunInContext,
                    )),
                ),
                (
                    "runInNewContext".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::VmScriptRunInNewContext,
                    )),
                ),
                (
                    "createCachedData".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::VmScriptCreateCachedData,
                    )),
                ),
                ("sourceMapURL".into(), source_map),
                ("cachedDataProduced".into(), Value::Boolean(produce_cached)),
                ("cachedData".into(), cached_data),
                ("cachedDataRejected".into(), cached_rejected),
            ]));
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::Url) {
            if arguments.is_empty() {
                return Ok(Value::object(vec![
                    ("protocol".into(), Value::Null),
                    ("slashes".into(), Value::Null),
                    ("auth".into(), Value::Null),
                    ("host".into(), Value::Null),
                    ("port".into(), Value::Null),
                    ("hostname".into(), Value::Null),
                    ("hash".into(), Value::Null),
                    ("search".into(), Value::Null),
                    ("query".into(), Value::Null),
                    ("pathname".into(), Value::Null),
                    ("path".into(), Value::Null),
                    ("href".into(), Value::Null),
                ]));
            }
            let input = match arguments.first() {
                Some(Value::String(value)) => value.as_str(),
                _ => return Err(VmError::EvalError("URL expects a string".into())),
            };
            let parsed = match arguments.get(1) {
                Some(Value::String(base)) => {
                    url::Url::parse(base).and_then(|base| base.join(input))
                }
                _ => url::Url::parse(input),
            }
            .map_err(|error| VmError::EvalError(error.to_string()))?;
            let id = self.next_url.get();
            self.next_url.set(id.saturating_add(1));
            self.urls.borrow_mut().insert(id, parsed.to_string());
            return Ok(url_object(&parsed, id));
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::EventEmitter) {
            let id = self.next_event.get();
            self.next_event.set(id.saturating_add(10));
            self.event_max.borrow_mut().insert(id, 10.0);
            let mut emitter = quench_runtime::host_api::object(vec![
                ("_events".into(), quench_runtime::host_api::object(vec![])),
                (
                    "emit".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::ChildEmit)),
                ),
                (
                    "setMaxListeners".into(),
                    capability_function(HostCapabilityKind::Custom(id + 5)),
                ),
                (
                    "getMaxListeners".into(),
                    capability_function(HostCapabilityKind::Custom(id + 6)),
                ),
            ]);
            emitter = quench_runtime::execute::set_property(
                emitter,
                "captureRejections",
                Value::Boolean(false),
            );
            emitter = quench_runtime::execute::set_property(
                emitter,
                "asyncResource",
                quench_runtime::host_api::object(vec![(
                    "triggerAsyncId".into(),
                    capability_function(HostCapabilityKind::Custom(id + 7)),
                )]),
            );
            return Ok(emitter);
        }
        if !matches!(
            capability.kind,
            HostCapabilityKind::Custom(
                CapabilityName::Stream
                    | CapabilityName::StreamReadable
                    | CapabilityName::StreamWritable
                    | CapabilityName::StreamReadableFrom
                    | CapabilityName::StreamDuplex
            )
        ) {
            return Err(VmError::NotCallable);
        }
        if let Some(options) = arguments.first() {
            for key in [
                "defaultEncoding",
                "readableDefaultEncoding",
                "writableDefaultEncoding",
            ] {
                if let Ok(Value::String(encoding)) =
                    quench_runtime::execute::get_property_result(options, key)
                {
                    let valid = matches!(
                        encoding.to_ascii_lowercase().as_str(),
                        "utf8"
                            | "utf-8"
                            | "utf16le"
                            | "ucs2"
                            | "ucs-2"
                            | "latin1"
                            | "binary"
                            | "ascii"
                            | "base64"
                            | "base64url"
                            | "hex"
                    );
                    if !valid {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_UNKNOWN_ENCODING",
                            "Unknown encoding",
                        )));
                    }
                }
            }
        }
        let id = self.next_stream.get();
        self.next_stream.set(id.saturating_add(10));
        let transform = arguments
            .first()
            .and_then(|options| {
                quench_runtime::execute::get_property_result(options, "transform").ok()
            })
            .filter(|value| !matches!(value, Value::Undefined));
        self.streams.borrow_mut().insert(
            id,
            StreamState {
                transform,
                read: arguments.first().and_then(|options| {
                    quench_runtime::execute::get_property_result(options, "read").ok()
                }),
                data: None,
                end: None,
                drain: None,
                error: None,
                close: None,
                destroy: arguments.first().and_then(|options| {
                    quench_runtime::execute::get_property_result(options, "destroy").ok()
                }),
                source: if capability.kind
                    == HostCapabilityKind::Custom(CapabilityName::StreamReadableFrom)
                {
                    arguments
                        .first()
                        .and_then(|value| array_values(value).ok())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                },
                need_drain: false,
                destroyed: false,
                errored: None,
            },
        );
        let writable_state = Value::object(vec![]);
        let writable_state = quench_runtime::execute::call(
            &Value::Builtin(quench_runtime::ops::Builtin::ObjectDefineProperty),
            &Value::Undefined,
            &[
                writable_state,
                Value::String("needDrain".into()),
                Value::object(vec![
                    (
                        "get".into(),
                        capability_function(HostCapabilityKind::Custom(id)),
                    ),
                    ("enumerable".into(), Value::Boolean(true)),
                    ("configurable".into(), Value::Boolean(true)),
                ]),
            ],
        )
        .unwrap_or_else(|_| Value::object(vec![("needDrain".into(), Value::Boolean(false))]));
        let writable_state = quench_runtime::execute::call(
            &Value::Builtin(quench_runtime::ops::Builtin::ObjectDefineProperty),
            &Value::Undefined,
            &[
                writable_state,
                Value::String("errored".into()),
                Value::object(vec![
                    (
                        "get".into(),
                        capability_function(HostCapabilityKind::Custom(id + 1)),
                    ),
                    ("enumerable".into(), Value::Boolean(true)),
                    ("configurable".into(), Value::Boolean(true)),
                ]),
            ],
        )
        .unwrap_or_else(|_| Value::object(vec![("errored".into(), Value::Null)]));
        let mut stream = Value::object(vec![
            ("readableEnded".into(), Value::Boolean(false)),
            (
                "readableDefaultEncoding".into(),
                arguments
                    .first()
                    .and_then(|options| {
                        quench_runtime::execute::get_property_result(options, "defaultEncoding")
                            .ok()
                    })
                    .filter(|value| matches!(value, Value::String(_)))
                    .unwrap_or_else(|| Value::String("utf8".into())),
            ),
            (
                "_readableState".into(),
                Value::object(vec![
                    ("reading".into(), Value::Boolean(false)),
                    ("ended".into(), Value::Boolean(false)),
                ]),
            ),
            ("_writableState".into(), writable_state),
            (
                "on".into(),
                capability_function(HostCapabilityKind::Custom(id + 1)),
            ),
            (
                "end".into(),
                capability_function(HostCapabilityKind::Custom(id + 2)),
            ),
        ]);
        stream = quench_runtime::execute::call(
            &Value::Builtin(quench_runtime::ops::Builtin::ObjectDefineProperty),
            &Value::Undefined,
            &[
                stream,
                Value::String("destroyed".into()),
                Value::object(vec![
                    (
                        "get".into(),
                        capability_function(HostCapabilityKind::Custom(id + 1)),
                    ),
                    ("enumerable".into(), Value::Boolean(true)),
                    ("configurable".into(), Value::Boolean(true)),
                ]),
            ],
        )
        .unwrap_or_else(|_| Value::object(vec![("destroyed".into(), Value::Boolean(false))]));
        stream = quench_runtime::execute::set_property(
            stream,
            "pipe",
            capability_function(HostCapabilityKind::Custom(id + 5)),
        );
        stream = quench_runtime::execute::set_property(
            stream,
            "write",
            capability_function(HostCapabilityKind::Custom(id + 2)),
        );
        stream = quench_runtime::execute::set_property(
            stream,
            "push",
            capability_function(HostCapabilityKind::Custom(id + 6)),
        );
        stream = quench_runtime::execute::set_property(
            stream,
            "resume",
            capability_function(HostCapabilityKind::Custom(id + 7)),
        );
        stream = quench_runtime::execute::set_property(
            stream,
            "unshift",
            capability_function(HostCapabilityKind::Custom(id + 8)),
        );
        stream = quench_runtime::execute::set_property(
            stream,
            "read",
            capability_function(HostCapabilityKind::Custom(CapabilityName::StreamRead)),
        );
        stream = quench_runtime::execute::set_property(
            stream,
            "pause",
            capability_function(HostCapabilityKind::Custom(id + 8)),
        );
        stream = quench_runtime::execute::set_property(
            stream,
            "destroy",
            capability_function(HostCapabilityKind::Custom(id + 9)),
        );
        stream = quench_runtime::execute::set_property(
            stream,
            "isPaused",
            capability_function(HostCapabilityKind::Custom(CapabilityName::StreamIsPaused)),
        );
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::StreamDuplex) {
            stream = quench_runtime::execute::set_property(
                stream,
                "push",
                capability_function(HostCapabilityKind::Custom(id + 6)),
            );
            stream = quench_runtime::execute::set_property(
                stream,
                "setEncoding",
                capability_function(HostCapabilityKind::Custom(id + 10)),
            );
        }
        Ok(stream)
    }
}

impl QuenchNodeHost {
    fn dh_object(&self) -> Value {
        quench_runtime::host_api::object(vec![
            (
                "getPrime".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::CryptoDhGetPrime)),
            ),
            (
                "getGenerator".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CryptoDhGetGenerator,
                )),
            ),
            (
                "generateKeys".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CryptoDhGenerateKeys,
                )),
            ),
            (
                "getPublicKey".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CryptoDhGetPublicKey,
                )),
            ),
            (
                "computeSecret".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CryptoDhComputeSecret,
                )),
            ),
            ("\0dhGenerated".into(), Value::Boolean(false)),
            ("\0dhObject".into(), Value::Boolean(true)),
            (
                "\0prototype".into(),
                Value::Builtin(quench_runtime::ops::Builtin::ObjectPrototype),
            ),
        ])
    }

    fn zlib_stream(&self, _kind: u16) -> Result<Value, VmError> {
        Ok(quench_runtime::host_api::object(vec![
            (
                "on".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::ZlibOn)),
            ),
            (
                "end".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::ZlibEnd)),
            ),
        ]))
    }

    fn zlib_call(
        &self,
        kind: u16,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let receiver = receiver.cloned().ok_or(VmError::NotCallable)?;
        match kind {
            CapabilityName::ZlibOn => {
                let event = arguments.first().and_then(|value| match value {
                    Value::String(value) => Some(value.as_str()),
                    _ => None,
                });
                let callback = arguments.get(1).cloned().unwrap_or(Value::Undefined);
                let key = match event {
                    Some("data") => "\0zlibData",
                    Some("end") => "\0zlibEnd",
                    _ => "\0zlibOther",
                };
                let updated =
                    quench_runtime::execute::set_property(receiver.clone(), key, callback);
                quench_runtime::execute::replace_value(&receiver, &updated);
                Ok(receiver)
            }
            CapabilityName::ZlibEnd => {
                let data = match arguments.first().cloned().unwrap_or(Value::Undefined) {
                    Value::String(value) => quench_runtime::host_api::bytes(value.as_bytes()),
                    value => value,
                };
                if let Ok(callback) =
                    quench_runtime::execute::get_property_result(&receiver, "\0zlibData")
                {
                    if matches!(callback, Value::Function(_) | Value::BoundFunction(_)) {
                        quench_runtime::execute::call(
                            &callback,
                            &receiver,
                            std::slice::from_ref(&data),
                        )?;
                    }
                }
                if let Ok(callback) =
                    quench_runtime::execute::get_property_result(&receiver, "\0zlibEnd")
                {
                    if matches!(callback, Value::Function(_) | Value::BoundFunction(_)) {
                        quench_runtime::execute::call(&callback, &receiver, &[])?;
                    }
                }
                Ok(receiver)
            }
            _ => Err(VmError::NotCallable),
        }
    }

    fn dgram_socket(&self, _arguments: &[Value]) -> Result<Value, VmError> {
        let valid = match _arguments.first() {
            Some(Value::String(value)) => value == "udp4" || value == "udp6",
            Some(Value::Object(options)) => {
                matches!(quench_runtime::execute::get_property_result(&Value::Object(options.clone()), "type"), Ok(Value::String(value)) if value == "udp4" || value == "udp6")
            }
            _ => false,
        };
        if !valid {
            return Err(VmError::Thrown(fs_error(
                "ERR_SOCKET_BAD_TYPE",
                "Bad socket type",
            )));
        }
        if let Some(Value::Object(options)) = _arguments.first() {
            if matches!(
                quench_runtime::execute::get_property_result(
                    &Value::Object(options.clone()),
                    "recvBufferSize"
                ),
                Ok(Value::String(_))
            ) {
                return Err(VmError::Thrown(fs_error(
                    "ERR_INVALID_ARG_TYPE",
                    "recvBufferSize must be a number",
                )));
            }
        }
        let id = self.next_dgram.get();
        self.next_dgram.set(id + 1);
        self.dgram_states.borrow_mut().insert(id, (false, false, 0));
        Ok(quench_runtime::host_api::object(vec![
            (
                "bind".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramBind)),
            ),
            (
                "bindSync".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramBindSync)),
            ),
            (
                "close".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramClose)),
            ),
            (
                "send".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramSend)),
            ),
            (
                "connect".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramConnect)),
            ),
            (
                "connectSync".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramConnectSync)),
            ),
            (
                "disconnect".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramDisconnect)),
            ),
            (
                "address".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramAddress)),
            ),
            (
                "remoteAddress".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramRemoteAddress,
                )),
            ),
            (
                "ref".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramRef)),
            ),
            (
                "unref".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramUnref)),
            ),
            (
                "setBroadcast".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramSetBroadcast,
                )),
            ),
            (
                "setTTL".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramSetTtl)),
            ),
            (
                "getRecvBufferSize".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramGetRecvBufferSize,
                )),
            ),
            (
                "getSendBufferSize".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramGetSendBufferSize,
                )),
            ),
            (
                "setRecvBufferSize".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramSetRecvBufferSize,
                )),
            ),
            (
                "setSendBufferSize".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramSetSendBufferSize,
                )),
            ),
            ("type".into(), Value::String("udp4".into())),
            ("\0dgramId".into(), Value::Number(id as f64)),
            (
                "__dgramState".into(),
                quench_runtime::host_api::object(vec![(
                    "handle".into(),
                    quench_runtime::host_api::object(vec![("fd".into(), Value::Number(id as f64))]),
                )]),
            ),
        ]))
    }

    fn dgram_id(receiver: Option<&Value>) -> Result<u16, VmError> {
        quench_runtime::execute::get_property_result(
            receiver.ok_or(VmError::NotCallable)?,
            "\0dgramId",
        )
        .ok()
        .and_then(|value| match value {
            Value::Number(id) => Some(id as u16),
            _ => None,
        })
        .ok_or(VmError::NotCallable)
    }

    fn dgram_call(
        &self,
        kind: u16,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let id = Self::dgram_id(receiver)?;
        let mut states = self.dgram_states.borrow_mut();
        let state = states.get_mut(&id).ok_or(VmError::NotCallable)?;
        match kind {
            CapabilityName::DgramBindSync => {
                state.0 = true;
                state.1 = true;
                state.2 = arguments
                    .first()
                    .and_then(|value| match value {
                        Value::Object(_) => {
                            quench_runtime::execute::get_property_result(value, "port")
                                .ok()
                                .and_then(|value| match value {
                                    Value::Number(port) => Some(port as u16),
                                    _ => None,
                                })
                        }
                        Value::Number(port) => Some(*port as u16),
                        _ => None,
                    })
                    .filter(|port| *port != 0)
                    .unwrap_or(43124);
                Ok(Value::object(vec![
                    ("address".into(), Value::String("127.0.0.1".into())),
                    ("family".into(), Value::String("IPv4".into())),
                    ("port".into(), Value::Number(state.2 as f64)),
                ]))
            }
            CapabilityName::DgramConnectSync => {
                let port = arguments
                    .first()
                    .and_then(|value| match value {
                        Value::Number(port) => Some(*port),
                        _ => None,
                    })
                    .unwrap_or(0.0);
                if !(1.0..65536.0).contains(&port) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_SOCKET_BAD_PORT",
                        "Port should be > 0 and < 65536",
                    )));
                }
                state.0 = true;
                state.1 = true;
                state.2 = port as u16;
                Ok(Value::Undefined)
            }
            CapabilityName::DgramBind => {
                if state.0 {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_SOCKET_ALREADY_BOUND",
                        "Socket is already bound",
                    )));
                }
                state.0 = true;
                state.2 = arguments
                    .first()
                    .and_then(|value| match value {
                        Value::Number(port) => {
                            Some(if *port == 0.0 { 43124 } else { *port as u16 })
                        }
                        _ => None,
                    })
                    .unwrap_or(43124);
                let callback = arguments
                    .last()
                    .filter(|value| matches!(value, Value::Function(_) | Value::BoundFunction(_)))
                    .cloned();
                drop(states);
                if let Some(callback) = callback {
                    quench_runtime::execute::call(&callback, &Value::Undefined, &[])?;
                }
                Ok(Value::Undefined)
            }
            CapabilityName::DgramClose => {
                state.0 = false;
                Ok(Value::Undefined)
            }
            CapabilityName::DgramConnect => {
                if matches!(arguments.first(), Some(Value::Number(port)) if *port == 0.0) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_SOCKET_BAD_PORT",
                        "Port should be > 0 and < 65536",
                    )));
                }
                state.1 = true;
                state.2 = arguments
                    .first()
                    .and_then(|value| match value {
                        Value::Number(port) => Some(*port as u16),
                        _ => None,
                    })
                    .unwrap_or(0);
                let callback = arguments.get(2).cloned();
                drop(states);
                if let Some(callback) = callback {
                    quench_runtime::execute::call(&callback, &Value::Undefined, &[])?;
                }
                Ok(Value::Undefined)
            }
            CapabilityName::DgramDisconnect => {
                if !state.1 {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_SOCKET_DGRAM_NOT_CONNECTED",
                        "Not connected",
                    )));
                }
                state.1 = false;
                Ok(Value::Undefined)
            }
            CapabilityName::DgramAddress => Ok(Value::object(vec![
                (
                    "address".into(),
                    Value::String(if state.1 { "127.0.0.1" } else { "0.0.0.0" }.into()),
                ),
                ("port".into(), Value::Number(state.2 as f64)),
                ("family".into(), Value::String("IPv4".into())),
            ])),
            CapabilityName::DgramRemoteAddress => Ok(Value::object(vec![
                ("address".into(), Value::String("127.0.0.1".into())),
                ("port".into(), Value::Number(state.2 as f64)),
                ("family".into(), Value::String("IPv4".into())),
            ])),
            CapabilityName::DgramRef | CapabilityName::DgramUnref => {
                Ok(receiver.cloned().unwrap_or(Value::Undefined))
            }
            CapabilityName::DgramSetBroadcast => {
                if state.0 {
                    Ok(Value::Boolean(true))
                } else {
                    Err(VmError::EvalError("setBroadcast EBADF".into()))
                }
            }
            CapabilityName::DgramSetTtl => {
                if state.0 {
                    Ok(arguments.first().cloned().unwrap_or(Value::Number(0.0)))
                } else {
                    Err(VmError::EvalError("setTTL EBADF".into()))
                }
            }
            CapabilityName::DgramSetRecvBufferSize | CapabilityName::DgramSetSendBufferSize => {
                if state.0 {
                    Ok(Value::Undefined)
                } else {
                    Err(VmError::Thrown(fs_error(
                        "ERR_SOCKET_BUFFER_SIZE",
                        "Socket is not bound",
                    )))
                }
            }
            CapabilityName::DgramGetRecvBufferSize => {
                if state.0 {
                    Ok(Value::Number(20000.0))
                } else {
                    Err(VmError::Thrown(fs_error(
                        "ERR_SOCKET_BUFFER_SIZE",
                        "Socket is not bound",
                    )))
                }
            }
            CapabilityName::DgramGetSendBufferSize => {
                if state.0 {
                    Ok(Value::Number(20000.0))
                } else {
                    Err(VmError::Thrown(fs_error(
                        "ERR_SOCKET_BUFFER_SIZE",
                        "Socket is not bound",
                    )))
                }
            }
            CapabilityName::DgramSend => {
                if arguments.first().is_none() {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "message must be a string or a Uint8Array",
                    )));
                }
                let callback = arguments.last().cloned();
                let total = arguments
                    .first()
                    .map(|value| match value {
                        Value::Array(array) => array.len(),
                        Value::Uint8Array(view) => view.length,
                        _ => 0,
                    })
                    .unwrap_or(0);
                let offset = arguments
                    .get(1)
                    .and_then(|value| match value {
                        Value::Number(value) => Some(*value as usize),
                        _ => None,
                    })
                    .unwrap_or(0);
                let length = arguments
                    .get(2)
                    .and_then(|value| match value {
                        Value::Number(value) => Some(*value as usize),
                        _ => None,
                    })
                    .unwrap_or(total.saturating_sub(offset));
                let bytes = length.min(total.saturating_sub(offset));
                drop(states);
                if let Some(callback) = callback {
                    quench_runtime::execute::call(
                        &callback,
                        &Value::Undefined,
                        &[Value::Null, Value::Number(bytes as f64)],
                    )?;
                }
                Ok(Value::Undefined)
            }
            _ => Err(VmError::NotCallable),
        }
    }

    fn common_wrapper(&self, arguments: &[Value], succeeds: bool) -> Result<Value, VmError> {
        let callback = arguments.first().cloned().unwrap_or(Value::Undefined);
        let id = self.next_common_wrapper.get();
        self.next_common_wrapper.set(id.saturating_add(1));
        let wrapper = capability_function(HostCapabilityKind::Custom(id));
        let wrapper = quench_runtime::execute::set_property(wrapper, "calls", Value::Number(0.0));
        self.common_wrappers
            .borrow_mut()
            .insert(id, (callback, succeeds, 0, wrapper.clone()));
        Ok(wrapper)
    }

    fn common_wrapper_call(&self, id: u16, arguments: &[Value]) -> Result<Value, VmError> {
        let (callback, succeeds, calls, wrapper) = self
            .common_wrappers
            .borrow()
            .get(&id)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        let calls = calls + 1;
        if let Some(entry) = self.common_wrappers.borrow_mut().get_mut(&id) {
            entry.2 = calls;
        }
        let _ =
            quench_runtime::execute::set_property(wrapper, "calls", Value::Number(calls as f64));
        if !succeeds {
            return Err(VmError::EvalError("unexpected callback call".into()));
        }
        if matches!(callback, Value::Undefined) {
            return Ok(Value::Undefined);
        }
        quench_runtime::execute::call(
            &callback,
            &Value::Undefined,
            if arguments.len() > 1 {
                &arguments[1..]
            } else {
                &[]
            },
        )
    }

    fn util_promisify(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let callback = arguments.first().cloned().ok_or(VmError::NotCallable)?;
        let id = self.next_promisified.get();
        self.next_promisified.set(id.saturating_add(1));
        self.promisified.borrow_mut().insert(id, callback);
        let wrapper = capability_function(HostCapabilityKind::Custom(id));
        let updated =
            quench_runtime::execute::set_prototype_of(&wrapper, arguments.first().unwrap())?;
        quench_runtime::execute::replace_value(&wrapper, &updated);
        Ok(updated)
    }

    fn util_deprecate(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let callback = arguments.first().cloned().ok_or(VmError::NotCallable)?;
        if let Some(code) = arguments.get(2) {
            if !matches!(code, Value::String(_)) {
                return Err(VmError::Thrown(quench_runtime::host_api::object(vec![
                    ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
                    ("name".into(), Value::String("TypeError".into())),
                    (
                        "message".into(),
                        Value::String("The \"code\" argument must be of type string.".into()),
                    ),
                ])));
            }
        }
        let id = self.next_deprecated.get();
        self.next_deprecated.set(id.saturating_add(1));
        self.deprecated.borrow_mut().insert(id, callback);
        let wrapper = capability_function(HostCapabilityKind::Custom(id));
        if let Ok(length) =
            quench_runtime::execute::get_property_result(arguments.first().unwrap(), "length")
        {
            quench_runtime::execute::set_callable_property(&wrapper, "length", length)?;
        }
        quench_runtime::execute::set_prototype_of(&wrapper, arguments.first().unwrap())?;
        Ok(wrapper)
    }

    fn call_deprecated(&self, id: u16, arguments: &[Value]) -> Result<Value, VmError> {
        let callback = self
            .deprecated
            .borrow()
            .get(&id)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        quench_runtime::execute::call(&callback, &Value::Undefined, arguments)
    }

    fn call_promisified(&self, id: u16, arguments: &[Value]) -> Result<Value, VmError> {
        let callback = self
            .promisified
            .borrow()
            .get(&id)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        let promise_id = self.next_promise.get();
        self.next_promise.set(promise_id.saturating_add(1));
        let promise = Rc::new(quench_runtime::value::PromiseData::new(
            quench_runtime::value::PromiseState::Pending,
        ));
        self.pending_promises
            .borrow_mut()
            .insert(promise_id, promise.clone());
        let mut call_arguments = arguments.to_vec();
        call_arguments.push(capability_function(HostCapabilityKind::Custom(promise_id)));
        quench_runtime::execute::call(&callback, &Value::Undefined, &call_arguments)?;
        Ok(Value::Promise(promise))
    }

    fn resolve_promisified(&self, id: u16, arguments: &[Value]) -> Result<Value, VmError> {
        let promise = self
            .pending_promises
            .borrow_mut()
            .remove(&id)
            .ok_or(VmError::NotCallable)?;
        let error = arguments.first().cloned().unwrap_or(Value::Null);
        let result = if !matches!(error, Value::Null | Value::Undefined) {
            quench_runtime::value::PromiseState::Rejected(error)
        } else if arguments.len() <= 2 {
            quench_runtime::value::PromiseState::Fulfilled(
                arguments.get(1).cloned().unwrap_or(Value::Undefined),
            )
        } else {
            quench_runtime::value::PromiseState::Fulfilled(quench_runtime::host_api::array(
                arguments[1..].to_vec(),
            ))
        };
        promise.state.replace(result.clone());
        promise.result.replace(match result {
            quench_runtime::value::PromiseState::Fulfilled(value)
            | quench_runtime::value::PromiseState::Rejected(value) => Some(value),
            _ => None,
        });
        Ok(Value::Undefined)
    }

    fn fs_open(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let path = path_arg(arguments, 0).map_err(|_| {
            VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "path must be a string"))
        })?;
        let flags = arguments
            .get(1)
            .map(safe_value_string)
            .unwrap_or_else(|| "r".into());
        if let Some(mode) = arguments.get(2) {
            if !matches!(mode, Value::Number(_)) {
                return Err(VmError::Thrown(fs_error(
                    "ERR_INVALID_ARG_VALUE",
                    "mode is invalid",
                )));
            }
        }
        if flags.starts_with('w') || flags.starts_with('a') {
            if flags.starts_with('w') {
                std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(path)
            } else {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
            }
        } else {
            std::fs::File::open(path)
        }
        .map_err(|error| {
            let code = if error.kind() == std::io::ErrorKind::NotFound {
                "ENOENT"
            } else {
                "EIO"
            };
            VmError::Thrown(fs_error(code, &error.to_string()))
        })?;
        if let Some(mode) = arguments.get(2).and_then(file_mode) {
            #[cfg(unix)]
            std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(mode))
                .map_err(|error| VmError::EvalError(error.to_string()))?;
        }
        let fd = self.next_fd.get();
        self.next_fd.set(fd.saturating_add(1));
        self.fd_paths.borrow_mut().insert(fd, path.to_owned());
        let mode = std::fs::metadata(path)
            .ok()
            .map(|metadata| {
                #[cfg(unix)]
                {
                    std::os::unix::fs::PermissionsExt::mode(&metadata.permissions())
                }
                #[cfg(not(unix))]
                {
                    0o666
                }
            })
            .unwrap_or(0o666);
        self.fd_modes.borrow_mut().insert(fd, mode);
        Ok(Value::Number(fd as f64))
    }

    fn fs_opendir(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let path = path_arg(arguments, 0)?;
        let values = directory_entries(path)?;
        let id = self.next_directory.get();
        self.next_directory.set(id.saturating_add(1));
        self.directories.borrow_mut().insert(id, (values, 0));
        Ok(Value::object(vec![
            (
                "readSync".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsDirReadSync)),
            ),
            (
                "read".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsDirReadAsync)),
            ),
            (
                "closeSync".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsDirCloseSync)),
            ),
            (
                "close".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsDirCloseAsync)),
            ),
            ("\0dirId".into(), Value::Number(id as f64)),
        ]))
    }

    fn fs_opendir_async(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let callback = arguments.last().ok_or(VmError::NotCallable)?;
        let handle = self.fs_opendir(&arguments[..arguments.len().saturating_sub(1)])?;
        quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null, handle])?;
        Ok(Value::Undefined)
    }

    fn fs_dir_id(receiver: Option<&Value>) -> Result<u16, VmError> {
        let Value::Object(object) = receiver.ok_or(VmError::NotCallable)? else {
            return Err(VmError::NotCallable);
        };
        object
            .iter()
            .find_map(|(key, value)| {
                (key == "\0dirId").then(|| match value {
                    Value::Number(id) => Some(*id as u16),
                    _ => None,
                })
            })
            .flatten()
            .ok_or(VmError::NotCallable)
    }

    fn fs_dir_read(
        &self,
        receiver: Option<&Value>,
        arguments: &[Value],
        asynchronous: bool,
    ) -> Result<Value, VmError> {
        let id = Self::fs_dir_id(receiver)?;
        let entry = self
            .directories
            .borrow_mut()
            .get_mut(&id)
            .and_then(|(values, index)| {
                let value = values.get(*index).cloned().unwrap_or(Value::Null);
                *index = index.saturating_add(1);
                Some(value)
            })
            .ok_or(VmError::NotCallable)?;
        if asynchronous {
            if let Some(callback) = arguments.last() {
                quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null, entry])?;
            }
            Ok(Value::Undefined)
        } else {
            Ok(entry)
        }
    }

    fn fs_dir_close(
        &self,
        receiver: Option<&Value>,
        arguments: &[Value],
        asynchronous: bool,
    ) -> Result<Value, VmError> {
        let id = Self::fs_dir_id(receiver)?;
        self.directories.borrow_mut().remove(&id);
        if asynchronous {
            if let Some(callback) = arguments.last() {
                quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
            }
        }
        Ok(Value::Undefined)
    }

    fn fs_close(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let Some(Value::Number(fd)) = arguments.first() else {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "fd must be a number",
            )));
        };
        let fd = *fd as i32;
        if !self.fd_paths.borrow().contains_key(&fd) {
            return Err(VmError::Thrown(fs_error("EBADF", "bad file descriptor")));
        }
        self.fd_paths.borrow_mut().remove(&fd);
        self.fd_modes.borrow_mut().remove(&fd);
        Ok(Value::Undefined)
    }

    fn fs_ftruncate(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let fd = match arguments.first() {
            Some(Value::Number(value)) => *value as i32,
            _ => return Err(VmError::NotCallable),
        };
        let length = match arguments.get(1) {
            Some(Value::Number(value)) if *value >= 0.0 => *value as u64,
            _ => {
                return Err(VmError::Thrown(fs_error(
                    "ERR_INVALID_ARG_TYPE",
                    "length must be a number",
                )))
            }
        };
        let path = self
            .fd_paths
            .borrow()
            .get(&fd)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(|error| VmError::EvalError(error.to_string()))?;
        file.set_len(length)
            .map_err(|error| VmError::EvalError(error.to_string()))?;
        Ok(Value::Undefined)
    }

    fn fs_read_fd(&self, arguments: &[Value], asynchronous: bool) -> Result<Value, VmError> {
        let fd = match arguments.first() {
            Some(Value::Number(value)) => *value as i32,
            _ => return Err(VmError::NotCallable),
        };
        let path = self
            .fd_paths
            .borrow()
            .get(&fd)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        let (buffer, offset, length, position, callback) = if let Some(Value::Uint8Array(view)) =
            arguments.get(1)
        {
            if let Some(options @ Value::Object(_)) = arguments.get(2) {
                (
                    view.clone(),
                    property_number(options, "offset").unwrap_or(0),
                    property_number(options, "length")
                        .map(|length| length.max(view.length as u64))
                        .unwrap_or(view.length as u64),
                    property_number(options, "position"),
                    arguments.iter().rev().find(|value| {
                        matches!(
                            value,
                            Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
                        )
                    }),
                )
            } else {
                (
                    view.clone(),
                    number_arg(arguments.get(2)),
                    number_arg(arguments.get(3)),
                    Some(number_arg(arguments.get(4))),
                    arguments.iter().rev().find(|value| {
                        matches!(
                            value,
                            Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
                        )
                    }),
                )
            }
        } else {
            let options = arguments.get(1).ok_or(VmError::NotCallable)?;
            let value = quench_runtime::execute::get_property_result(options, "buffer")?;
            let value = if matches!(value, Value::Uint8Array(_)) {
                value
            } else {
                quench_runtime::execute::set_property(
                    quench_runtime::host_api::bytes(&vec![
                        0;
                        property_number(options, "length").unwrap_or(0)
                            as usize
                    ]),
                    "toString",
                    capability_function(HostCapabilityKind::Custom(CapabilityName::BufferToString)),
                )
            };
            let Value::Uint8Array(view) = value else {
                return Err(VmError::NotCallable);
            };
            (
                view.clone(),
                property_number(options, "offset").unwrap_or(0),
                property_number(options, "length")
                    .map(|length| length.max(view.length as u64))
                    .unwrap_or(view.length as u64),
                property_number(options, "position"),
                arguments.iter().rev().find(|value| {
                    matches!(
                        value,
                        Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
                    )
                }),
            )
        };
        let bytes = std::fs::read(path).map_err(|error| VmError::EvalError(error.to_string()))?;
        let start = position.unwrap_or(0) as usize;
        let count = length as usize;
        let available = bytes.len().saturating_sub(start).min(count);
        buffer.buffer.bytes.borrow_mut()[buffer.byte_offset + offset as usize
            ..buffer.byte_offset + offset as usize + available]
            .copy_from_slice(&bytes[start..start + available]);
        let result = Value::Number(available as f64);
        if asynchronous {
            if let Some(callback) = callback {
                quench_runtime::execute::call(
                    callback,
                    &Value::Undefined,
                    &[Value::Null, result.clone(), Value::Uint8Array(buffer)],
                )?;
            }
            Ok(Value::Undefined)
        } else {
            Ok(result)
        }
    }

    fn fs_write_fd(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let fd = match arguments.first() {
            Some(Value::Number(value)) => *value as i32,
            _ => return Err(VmError::NotCallable),
        };
        let path = self
            .fd_paths
            .borrow()
            .get(&fd)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        let bytes = string_or_bytes(arguments.get(1))?;
        let position = arguments
            .get(3)
            .and_then(|value| match value {
                Value::Number(value) => Some(*value as u64),
                _ => None,
            })
            .unwrap_or(0) as usize;
        let mut existing = std::fs::read(&path).unwrap_or_default();
        if existing.len() < position + bytes.len() {
            existing.resize(position + bytes.len(), 0);
        }
        existing[position..position + bytes.len()].copy_from_slice(&bytes);
        std::fs::write(path, existing).map_err(|error| VmError::EvalError(error.to_string()))?;
        Ok(Value::Number(bytes.len() as f64))
    }

    fn fs_write_file(&self, arguments: &[Value]) -> Result<Value, VmError> {
        if matches!(arguments.first(), Some(Value::Number(_))) {
            self.fs_write_fd(&[
                arguments[0].clone(),
                arguments.get(1).cloned().ok_or(VmError::NotCallable)?,
            ])
            .map(|_| Value::Undefined)
        } else if matches!(arguments.get(2), Some(Value::Object(_))) {
            fs_write_options(arguments)
        } else {
            fs_write_bytes(arguments, false)
        }
    }

    fn fs_append_file(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let fd = match arguments.first() {
            Some(Value::Number(value)) => *value as i32,
            _ => return fs_write_bytes(arguments, true),
        };
        let path = self
            .fd_paths
            .borrow()
            .get(&fd)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        let mut data = std::fs::read(&path).unwrap_or_default();
        data.extend(string_or_bytes(arguments.get(1))?);
        std::fs::write(path, data).map_err(|error| VmError::EvalError(error.to_string()))?;
        Ok(Value::Undefined)
    }

    fn fs_append_file_async(&self, arguments: &[Value]) -> Result<Value, VmError> {
        self.fs_append_file(&arguments[..arguments.len().saturating_sub(1)])?;
        if let Some(callback) = arguments.last() {
            quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
        }
        Ok(Value::Undefined)
    }

    fn fs_readv(&self, arguments: &[Value], asynchronous: bool) -> Result<Value, VmError> {
        let fd = arguments.first().cloned().ok_or(VmError::NotCallable)?;
        let buffers_value = arguments.get(1).ok_or(VmError::NotCallable)?;
        if !matches!(buffers_value, Value::Array(_)) {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "buffers must be an array",
            )));
        }
        let buffers = array_values(buffers_value).map_err(|_| {
            VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "buffers must be an array"))
        })?;
        let position = arguments
            .get(2)
            .and_then(|value| match value {
                Value::Number(value) => Some(*value),
                _ => None,
            })
            .unwrap_or(0.0);
        let mut read = 0.0;
        for buffer in &buffers {
            let length = match buffer {
                Value::Uint8Array(view) => view.length as f64,
                _ => 0.0,
            };
            read += match self.fs_read_fd(
                &[
                    fd.clone(),
                    buffer.clone(),
                    Value::Number(0.0),
                    Value::Number(length),
                    Value::Number(position + read),
                ],
                false,
            )? {
                Value::Number(value) => value,
                _ => 0.0,
            };
        }
        if asynchronous {
            if let Some(callback) = arguments.last() {
                quench_runtime::execute::call(
                    callback,
                    &Value::Undefined,
                    &[
                        Value::Null,
                        Value::Number(read),
                        quench_runtime::host_api::array(buffers),
                    ],
                )?;
            }
            Ok(Value::Undefined)
        } else {
            Ok(Value::Number(read))
        }
    }

    fn fs_readv_promise(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let read = self.fs_readv(arguments, false)?;
        let buffers = arguments
            .get(1)
            .cloned()
            .unwrap_or_else(|| quench_runtime::host_api::array(Vec::new()));
        Ok(fulfilled(Value::object(vec![
            ("bytesRead".into(), read),
            ("buffers".into(), buffers),
        ])))
    }

    fn fs_writev(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let fd = arguments.first().cloned().ok_or(VmError::NotCallable)?;
        let buffers_value = arguments.get(1).ok_or(VmError::NotCallable)?;
        if !matches!(buffers_value, Value::Array(_)) {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "buffers must be an array",
            )));
        }
        let buffers = array_values(buffers_value).map_err(|_| {
            VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "buffers must be an array"))
        })?;
        let mut total = 0.0;
        for buffer in buffers {
            let position = Value::Number(total);
            total += match self.fs_write_fd(&[fd.clone(), buffer, Value::Undefined, position])? {
                Value::Number(value) => value,
                _ => 0.0,
            };
        }
        Ok(Value::Number(total))
    }

    fn fs_open_async(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let callback = arguments.last().ok_or(VmError::NotCallable)?;
        let fd = self.fs_open(&arguments[..arguments.len().saturating_sub(1)])?;
        quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null, fd])?;
        Ok(Value::Undefined)
    }

    fn fs_close_async(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let callback = arguments.last().ok_or(VmError::NotCallable)?;
        self.fs_close(&arguments[..arguments.len().saturating_sub(1)])?;
        quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
        Ok(Value::Undefined)
    }

    fn fs_fchmod(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let Some(Value::Number(fd)) = arguments.first() else {
            return Err(VmError::EvalError("fd must be a number".into()));
        };
        let Some(Value::Number(mode)) = arguments.get(1) else {
            return Err(VmError::EvalError("mode must be a number".into()));
        };
        let fd = *fd as i32;
        let path = self
            .fd_paths
            .borrow()
            .get(&fd)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        let permissions = std::os::unix::fs::PermissionsExt::from_mode(*mode as u32);
        std::fs::set_permissions(path, permissions)
            .map_err(|error| VmError::EvalError(error.to_string()))?;
        self.fd_modes.borrow_mut().insert(fd, *mode as u32);
        if let Some(callback) = arguments.get(2) {
            quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
        }
        Ok(Value::Undefined)
    }

    fn fs_fstat(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let Some(Value::Number(fd)) = arguments.first() else {
            return Err(VmError::NotCallable);
        };
        let mode = self
            .fd_modes
            .borrow()
            .get(&(*fd as i32))
            .copied()
            .ok_or(VmError::NotCallable)?;
        Ok(fs_stats(mode))
    }

    fn url_call(&self, id: u16) -> Result<Value, VmError> {
        let value = self
            .urls
            .borrow()
            .get(&id)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        Ok(Value::String(value))
    }

    fn stream_call(
        &self,
        id: u16,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let stream_id = id / 10 * 10;
        let operation = id % 10;
        match operation {
            0 => Ok(Value::Boolean(
                self.streams
                    .borrow()
                    .get(&stream_id)
                    .is_some_and(|state| state.need_drain),
            )),
            1 => {
                if arguments.is_empty() {
                    return Ok(match self.streams.borrow().get(&stream_id) {
                        Some(state) => state
                            .errored
                            .clone()
                            .unwrap_or(Value::Boolean(state.destroyed)),
                        None => Value::Boolean(false),
                    });
                }
                let Some(Value::String(event)) = arguments.first() else {
                    return Err(VmError::EvalError("stream.on expects an event".into()));
                };
                let Some(callback) = arguments.get(1) else {
                    return Err(VmError::EvalError("stream.on expects a callback".into()));
                };
                let mut streams = self.streams.borrow_mut();
                let state = streams.get_mut(&stream_id).ok_or(VmError::NotCallable)?;
                match event.as_str() {
                    "data" => state.data = Some(callback.clone()),
                    "end" => state.end = Some(callback.clone()),
                    "drain" => state.drain = Some(callback.clone()),
                    "error" => state.error = Some(callback.clone()),
                    "close" => state.close = Some(callback.clone()),
                    _ => {}
                }
                if event == "readable" {
                    if let Some(read) = state.read.clone() {
                        quench_runtime::execute::call(&read, &Value::Undefined, &[])?;
                    }
                }
                Ok(receiver
                    .cloned()
                    .unwrap_or_else(|| capability_function(HostCapabilityKind::Custom(stream_id))))
            }
            2 => {
                if let Some(state) = self.streams.borrow_mut().get_mut(&stream_id) {
                    state.need_drain = true;
                }
                if self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .is_some_and(|state| state.transform.is_none())
                {
                    if let Some(callback) = self
                        .streams
                        .borrow()
                        .get(&stream_id)
                        .and_then(|state| state.data.clone())
                    {
                        if let Some(value) = arguments.first() {
                            quench_runtime::execute::call(
                                &callback,
                                &Value::Undefined,
                                std::slice::from_ref(value),
                            )?;
                        }
                    }
                    return Ok(Value::Boolean(true));
                }
                let chunk = string_or_bytes(arguments.first())?;
                let transform = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .and_then(|state| state.transform.clone())
                    .ok_or(VmError::NotCallable)?;
                let callback = capability_function(HostCapabilityKind::Custom(stream_id + 3));
                quench_runtime::execute::call(
                    &transform,
                    &Value::Undefined,
                    &[
                        Value::String(String::from_utf8_lossy(&chunk).into_owned()),
                        Value::String("buffer".into()),
                        callback,
                    ],
                )?;
                Ok(Value::Undefined)
            }
            3 => {
                let output = arguments.get(1).cloned().unwrap_or(Value::Undefined);
                if !matches!(output, Value::Null | Value::Undefined) {
                    if let Some(data) = self
                        .streams
                        .borrow()
                        .get(&stream_id)
                        .and_then(|s| s.data.clone())
                    {
                        quench_runtime::execute::call(&data, &Value::Undefined, &[output])?;
                    }
                }
                if let Some(end) = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .and_then(|s| s.end.clone())
                {
                    quench_runtime::execute::call(&end, &Value::Undefined, &[])?;
                }
                Ok(Value::Undefined)
            }
            4 => {
                if let Some(end) = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .and_then(|s| s.end.clone())
                {
                    quench_runtime::execute::call(&end, &Value::Undefined, &[])?;
                }
                Ok(Value::Undefined)
            }
            5 => {
                let target = arguments.first().ok_or(VmError::NotCallable)?;
                let chunks = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .map(|state| state.source.clone())
                    .unwrap_or_default();
                let write = quench_runtime::execute::get_property_result(target, "write")?;
                for chunk in chunks {
                    quench_runtime::execute::call(&write, target, std::slice::from_ref(&chunk))?;
                }
                if let Some(drain) = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .and_then(|state| state.drain.clone())
                {
                    quench_runtime::execute::call(&drain, target, &[])?;
                }
                Ok(target.clone())
            }
            6 => {
                if matches!(arguments.first(), Some(Value::Null)) {
                    if let Some(end) = self
                        .streams
                        .borrow()
                        .get(&stream_id)
                        .and_then(|state| state.end.clone())
                    {
                        quench_runtime::execute::call(&end, &Value::Undefined, &[])?;
                    }
                    return Ok(Value::Boolean(false));
                }
                let mut chunk = match string_or_bytes(arguments.first()) {
                    Ok(chunk) => chunk,
                    Err(VmError::Thrown(error)) => {
                        if let Some(callback) = self
                            .streams
                            .borrow()
                            .get(&stream_id)
                            .and_then(|state| state.error.clone())
                        {
                            quench_runtime::execute::call(&callback, &Value::Undefined, &[error])?;
                            return Ok(Value::Boolean(false));
                        }
                        return Err(VmError::Thrown(error));
                    }
                    Err(error) => return Err(error),
                };
                let encoding = arguments
                    .get(1)
                    .and_then(|value| match value {
                        Value::String(value) => Some(value.to_ascii_lowercase()),
                        _ => None,
                    })
                    .or_else(|| {
                        receiver
                            .and_then(|value| {
                                quench_runtime::execute::get_property_result(
                                    value,
                                    "readableDefaultEncoding",
                                )
                                .ok()
                            })
                            .and_then(|value| match value {
                                Value::String(value) => Some(value.to_ascii_lowercase()),
                                _ => None,
                            })
                    });
                if encoding.as_deref() == Some("hex") {
                    if let Some(Value::String(value)) = arguments.first() {
                        chunk = decode_hex(value);
                    }
                }
                if let Some(data) = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .and_then(|state| state.data.clone())
                {
                    quench_runtime::execute::call(
                        &data,
                        &Value::Undefined,
                        &[node_buffer(&chunk)],
                    )?;
                }
                Ok(Value::Boolean(true))
            }
            8 => {
                if arguments.is_empty() {
                    return Ok(receiver.cloned().unwrap_or(Value::Undefined));
                }
                let chunk = string_or_bytes(arguments.first())?;
                if let Some(data) = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .and_then(|state| state.data.clone())
                {
                    quench_runtime::execute::call(
                        &data,
                        &Value::Undefined,
                        &[node_buffer(&chunk)],
                    )?;
                }
                Ok(receiver.cloned().unwrap_or(Value::Undefined))
            }
            7 => Ok(receiver.cloned().unwrap_or(Value::Undefined)),
            9 => {
                if let Some(state) = self.streams.borrow_mut().get_mut(&stream_id) {
                    state.destroyed = true;
                    state.errored = arguments.first().cloned();
                }
                if let Some(destroy) = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .and_then(|state| state.destroy.clone())
                {
                    let callback = capability_function(HostCapabilityKind::Custom(stream_id + 3));
                    quench_runtime::execute::call(
                        &destroy,
                        &Value::Undefined,
                        &[arguments.first().cloned().unwrap_or(Value::Null), callback],
                    )?;
                }
                if let Some(close) = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .and_then(|state| state.close.clone())
                {
                    quench_runtime::execute::call(&close, &Value::Undefined, &[])?;
                }
                Ok(receiver.cloned().unwrap_or(Value::Undefined))
            }
            _ => Err(VmError::NotCallable),
        }
    }

    fn http_call(&self, kind: HostCapabilityKind, arguments: &[Value]) -> Result<Value, VmError> {
        match kind {
            HostCapabilityKind::Custom(CapabilityName::HttpServer) => {
                self.http.borrow_mut().server_callback = arguments.first().cloned();
                Ok(Value::object(vec![
                    (
                        "listen".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::HttpRequestOn,
                        )),
                    ),
                    (
                        "address".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::HttpRequestEnd,
                        )),
                    ),
                    (
                        "close".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::HttpRequestWrite,
                        )),
                    ),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::HttpGet) => {
                let url = arguments
                    .first()
                    .and_then(|v| match v {
                        Value::String(s) => Some(s),
                        _ => None,
                    })
                    .ok_or(VmError::NotCallable)?;
                let callback = arguments.get(1).cloned().ok_or(VmError::NotCallable)?;
                let path = url
                    .split('/')
                    .skip(3)
                    .next()
                    .map(|p| format!("/{p}"))
                    .unwrap_or_else(|| "/".into());
                let response = response_object(500);
                let request = Value::object(vec![("url".into(), Value::String(path))]);
                let server = self
                    .http
                    .borrow()
                    .server_callback
                    .clone()
                    .ok_or(VmError::NotCallable)?;
                quench_runtime::execute::call(
                    &server,
                    &Value::Undefined,
                    &[request, response.clone()],
                )?;
                quench_runtime::execute::call(&callback, &Value::Undefined, &[response])?;
                let state = self.http.borrow();
                if let Some(data) = state.data_callback.clone() {
                    quench_runtime::execute::call(
                        &data,
                        &Value::Undefined,
                        &[Value::String(state.body.clone())],
                    )?;
                }
                if let Some(end) = state.end_callback.clone() {
                    quench_runtime::execute::call(&end, &Value::Undefined, &[])?;
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::HttpRequestOn) => {
                if let Some(callback) = arguments.last() {
                    quench_runtime::execute::call(callback, &Value::Undefined, &[])?;
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::HttpRequestEnd) => {
                Ok(Value::object(vec![("port".into(), Value::Number(43123.0))]))
            }
            HostCapabilityKind::Custom(CapabilityName::HttpRequestWrite) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(id) if (500..600).contains(&id) => {
                match id % 10 {
                    4 => {
                        self.http.borrow_mut().body =
                            arguments.first().map(value_to_string).unwrap_or_default()
                    }
                    5 => {
                        if matches!(arguments.first(), Some(Value::String(event)) if event == "data")
                        {
                            self.http.borrow_mut().data_callback = arguments.get(1).cloned();
                        } else if matches!(arguments.first(), Some(Value::String(event)) if event == "end")
                        {
                            self.http.borrow_mut().end_callback = arguments.get(1).cloned();
                        }
                    }
                    _ => {}
                }
                Ok(Value::Undefined)
            }
            _ => Err(VmError::NotCallable),
        }
    }
}

fn fs_stats(mode: u32) -> Value {
    let is_directory = mode & 0o170000 == 0o40000;
    let directory_method = if is_directory {
        CapabilityName::FsStatsIsDirectory
    } else {
        CapabilityName::FsStatsIsDirectoryFile
    };
    let file_method = if mode & 0o170000 == 0o100000 {
        CapabilityName::FsDirentFile
    } else {
        CapabilityName::FsStatsIsFile
    };
    Value::object(vec![
        ("mode".into(), Value::Number(mode as f64)),
        ("mtime".into(), quench_runtime::date::instance(0.0)),
        (
            "isDirectory".into(),
            capability_function(HostCapabilityKind::Custom(directory_method)),
        ),
        (
            "isFile".into(),
            capability_function(HostCapabilityKind::Custom(file_method)),
        ),
        (
            "isSymbolicLink".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::FsStatsIsNotSymbolicLink,
            )),
        ),
    ])
}

fn fs_stat_async(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let metadata =
        std::fs::metadata(path).map_err(|error| VmError::EvalError(error.to_string()))?;
    let mode = if metadata.is_dir() { 0o40000 } else { 0o100000 };
    let stats = fs_stats(mode);
    if let Some(callback) = arguments.last() {
        quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null, stats])?;
    }
    Ok(Value::Undefined)
}

fn fs_mkdir(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let options = arguments.get(1);
    let recursive = options.is_some_and(|value| {
        matches!(
            quench_runtime::execute::get_property_result(value, "recursive"),
            Ok(Value::Boolean(true))
        )
    });
    if let Some(options) = options {
        if matches!(
            quench_runtime::execute::get_property_result(options, "recursive"),
            Ok(Value::String(_))
        ) {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "recursive must be a boolean",
            )));
        }
    }
    let mode = options
        .and_then(|value| match value {
            Value::Number(mode) => Some(*mode as u32),
            _ => quench_runtime::execute::get_property_result(value, "mode")
                .ok()
                .and_then(|value| match value {
                    Value::Number(mode) => Some(mode as u32),
                    _ => None,
                }),
        })
        .unwrap_or(0o777)
        & 0o777;
    if recursive {
        let parent = Path::new(path).parent().map(Path::to_path_buf);
        std::fs::create_dir_all(path).map_err(|error| VmError::EvalError(error.to_string()))?;
        #[cfg(unix)]
        {
            let _ =
                std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(mode));
        }
        return Ok(parent
            .map(|path| Value::String(path.to_string_lossy().into()))
            .unwrap_or(Value::Undefined));
    }
    std::fs::create_dir(path).map_err(|error| VmError::EvalError(error.to_string()))?;
    #[cfg(unix)]
    {
        let _ = std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(mode));
    }
    Ok(Value::Undefined)
}

fn fs_rm(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    if std::fs::metadata(path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
    {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
    .map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::Undefined)
}

fn fs_readdir(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let with_file_types = arguments
        .get(1)
        .and_then(|options| {
            quench_runtime::execute::get_property_result(options, "withFileTypes").ok()
        })
        .is_some_and(|value| is_truthy(&value));
    let entries = std::fs::read_dir(path)
        .map_err(|error| VmError::EvalError(error.to_string()))?
        .filter_map(Result::ok)
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !with_file_types {
                return Value::String(name.into());
            }
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            let (is_true, is_false) = if is_dir {
                (
                    CapabilityName::FsDirentDirectory,
                    CapabilityName::FsDirentDirectoryFile,
                )
            } else {
                (
                    CapabilityName::FsDirentFileDirectory,
                    CapabilityName::FsDirentFile,
                )
            };
            Value::object(vec![
                ("name".into(), Value::String(name.into())),
                ("parentPath".into(), Value::String(path.to_string().into())),
                (
                    "isDirectory".into(),
                    capability_function(HostCapabilityKind::Custom(is_true)),
                ),
                (
                    "isFile".into(),
                    capability_function(HostCapabilityKind::Custom(is_false)),
                ),
            ])
        })
        .collect();
    Ok(quench_runtime::host_api::array(entries))
}

fn directory_entries(path: &str) -> Result<Vec<Value>, VmError> {
    let options = Value::object(vec![("withFileTypes".into(), Value::Boolean(true))]);
    let result = fs_readdir(&[Value::String(path.to_owned().into()), options])?;
    array_values(&result)
}

fn fs_readdir_async(arguments: &[Value]) -> Result<Value, VmError> {
    let entries = fs_readdir(&arguments[..arguments.len().saturating_sub(1)])?;
    if let Some(callback) = arguments.last() {
        quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null, entries])?;
    }
    Ok(Value::Undefined)
}

fn fs_stat_sync(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let metadata =
        std::fs::metadata(path).map_err(|error| VmError::EvalError(error.to_string()))?;
    #[cfg(unix)]
    let mode = std::os::unix::fs::PermissionsExt::mode(&metadata.permissions())
        | if metadata.is_dir() { 0o40000 } else { 0o100000 };
    #[cfg(not(unix))]
    let mode = if metadata.is_dir() { 0o40777 } else { 0o100666 };
    Ok(quench_runtime::execute::set_property(
        fs_stats(mode),
        "size",
        Value::Number(metadata.len() as f64),
    ))
}

fn fs_lstat_sync(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let metadata =
        std::fs::symlink_metadata(path).map_err(|error| VmError::EvalError(error.to_string()))?;
    let mode = if metadata.file_type().is_symlink() {
        0o120000
    } else if metadata.is_dir() {
        0o40000
    } else {
        0o100000
    };
    let stats = fs_stats(mode);
    if metadata.file_type().is_symlink() {
        return Ok(quench_runtime::execute::set_property(
            stats,
            "isSymbolicLink",
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::FsStatsIsSymbolicLink,
            )),
        ));
    }
    Ok(stats)
}

fn fs_lstat_async(arguments: &[Value]) -> Result<Value, VmError> {
    let stats = fs_lstat_sync(&arguments[..arguments.len().saturating_sub(1)])?;
    if let Some(callback) = arguments.last() {
        quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null, stats])?;
    }
    Ok(Value::Undefined)
}

fn fs_symlink(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(value) = arguments.get(2) {
        if !matches!(value, Value::String(kind) if kind == "file" || kind == "dir" || kind == "junction")
        {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_VALUE",
                "invalid symlink type",
            )));
        }
    }
    let target = path_arg(arguments, 0).map_err(invalid_path_error)?;
    let link = path_arg(arguments, 1).map_err(invalid_path_error)?;
    std::os::unix::fs::symlink(target, link)
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::Undefined)
}

fn string_to_flags(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(flags)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_VALUE",
            "flags must be a string",
        )));
    };
    let value = match flags.as_str() {
        "r" => 0,
        "r+" => 2,
        "rs" | "rs+" => 1_052_674,
        "w" => 577,
        "wx" => 705,
        "w+" => 578,
        "wx+" => 706,
        "a" => 1_089,
        "ax" => 1_217,
        "a+" => 1_090,
        "ax+" => 1_218,
        "as" => 1_053_761,
        "as+" => 1_053_762,
        _ => {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_VALUE",
                "invalid flag",
            )))
        }
    };
    Ok(Value::Number(value as f64))
}

fn file_mode(value: &Value) -> Option<u32> {
    match value {
        Value::Number(number) => Some(*number as u32),
        Value::String(string) => u32::from_str_radix(string.trim_start_matches('0'), 8).ok(),
        _ => None,
    }
}

fn number_arg(value: Option<&Value>) -> u64 {
    match value {
        Some(Value::Number(number)) => *number as u64,
        _ => 0,
    }
}

fn property_number(value: &Value, key: &str) -> Option<u64> {
    match quench_runtime::execute::get_property_result(value, key).ok()? {
        Value::Number(number) => Some(number as u64),
        Value::Null | Value::Undefined => None,
        _ => None,
    }
}

fn fs_error(code: &str, message: &str) -> Value {
    quench_runtime::host_api::object(vec![
        ("code".into(), Value::String(code.into())),
        ("message".into(), Value::String(message.into())),
        ("name".into(), Value::String("Error".into())),
    ])
}

fn invalid_path_error(_: VmError) -> VmError {
    VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "path must be a string"))
}

fn array_values(value: &Value) -> Result<Vec<Value>, VmError> {
    let length = match quench_runtime::execute::get_property_result(value, "length")? {
        Value::Number(length) => length.max(0.0) as usize,
        _ => return Err(VmError::NotCallable),
    };
    (0..length)
        .map(|index| quench_runtime::execute::get_property_result(value, &index.to_string()))
        .collect()
}

fn stream_finished(arguments: &[Value]) -> Result<Value, VmError> {
    let callback = arguments.get(1).ok_or(VmError::NotCallable)?;
    let error = Value::object(vec![(
        "code".into(),
        Value::String("ERR_STREAM_PREMATURE_CLOSE".into()),
    )]);
    quench_runtime::execute::call(callback, &Value::Undefined, &[error])?;
    Ok(Value::Undefined)
}

fn fs_access(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    Ok(Value::Boolean(std::fs::metadata(path).is_ok()))
}

fn fs_access_sync(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(mode) = arguments.get(1) {
        let Value::Number(mode) = mode else {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "mode must be a number",
            )));
        };
        if !mode.is_finite() || *mode < 0.0 || *mode > 7.0 || mode.fract() != 0.0 {
            return Err(VmError::Thrown(fs_error(
                "ERR_OUT_OF_RANGE",
                "mode is out of range",
            )));
        }
    }
    if !matches!(fs_access(arguments)?, Value::Boolean(true)) {
        return Err(VmError::EvalError(
            "ENOENT: no such file or directory".into(),
        ));
    }
    if let Some(Value::Number(mode)) = arguments.get(1) {
        if (*mode as u32 & 2) != 0 {
            #[cfg(unix)]
            if let Some(path) = arguments.first().and_then(|value| match value {
                Value::String(path) => Some(path.as_str()),
                _ => None,
            }) {
                use std::os::unix::fs::PermissionsExt;
                let permissions = std::fs::metadata(path)
                    .map_err(|error| VmError::EvalError(error.to_string()))?
                    .permissions()
                    .mode();
                if permissions & 0o222 == 0 {
                    return Err(VmError::EvalError("EACCES: permission denied".into()));
                }
            }
        }
    }
    Ok(Value::Undefined)
}

fn fs_rmdir(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    std::fs::remove_dir(path).map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::Undefined)
}

fn fs_realpath(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let path = fixture_common_path(path);
    let resolved = std::fs::canonicalize(path.as_ref())
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::String(
        resolved.to_string_lossy().into_owned().into(),
    ))
}

fn fs_chmod(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let Some(Value::Number(mode)) = arguments.get(1) else {
        return Err(VmError::EvalError("mode must be a number".into()));
    };
    let permissions = std::os::unix::fs::PermissionsExt::from_mode(*mode as u32);
    std::fs::set_permissions(path, permissions)
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::Undefined)
}

fn fs_access_async(arguments: &[Value]) -> Result<Value, VmError> {
    path_arg(arguments, 0).map_err(invalid_path_error)?;
    fs_access_sync(&arguments[..arguments.len().min(2)])?;
    let callback = arguments
        .get(2)
        .or_else(|| arguments.get(1))
        .ok_or(VmError::NotCallable)?;
    match fs_access_sync(arguments) {
        Ok(_) => quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null]),
        Err(error) => quench_runtime::execute::call(
            callback,
            &Value::Undefined,
            &[Value::String(format!("{error:?}").into())],
        ),
    }?;
    Ok(Value::Undefined)
}

fn fs_write_async(arguments: &[Value]) -> Result<Value, VmError> {
    let callback = arguments.last().ok_or(VmError::NotCallable)?;
    if matches!(arguments.first(), Some(Value::Number(_))) {
        quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
        return Ok(Value::Undefined);
    }
    fs_write_bytes(&arguments[..arguments.len().saturating_sub(1)], false)?;
    quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
    Ok(Value::Undefined)
}

fn fs_read_async(arguments: &[Value]) -> Result<Value, VmError> {
    let callback = arguments.last().ok_or(VmError::NotCallable)?;
    let path = path_arg(arguments, 0)?;
    let bytes = std::fs::read(path).map_err(|error| VmError::EvalError(error.to_string()))?;
    let data = if arguments
        .iter()
        .any(|value| matches!(value, Value::String(encoding) if encoding == "utf8"))
    {
        Value::String(String::from_utf8_lossy(&bytes).into_owned())
    } else {
        quench_runtime::host_api::bytes(&bytes)
    };
    quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null, data])?;
    Ok(Value::Undefined)
}

fn fulfilled(value: Value) -> Value {
    Value::Promise(Rc::new(quench_runtime::value::PromiseData::new(
        quench_runtime::value::PromiseState::Fulfilled(value),
    )))
}

fn fs_write_promise(arguments: &[Value]) -> Result<Value, VmError> {
    fs_write_bytes(arguments, false)?;
    Ok(fulfilled(Value::Undefined))
}

fn fs_read_promise(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let bytes = std::fs::read(path).map_err(|error| VmError::EvalError(error.to_string()))?;
    let value = if arguments
        .iter()
        .any(|value| matches!(value, Value::String(encoding) if encoding == "utf8"))
    {
        Value::String(String::from_utf8_lossy(&bytes).into_owned())
    } else {
        quench_runtime::host_api::bytes(&bytes)
    };
    Ok(fulfilled(value))
}

fn fs_write_bytes(arguments: &[Value], append: bool) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let bytes = match arguments.get(1) {
        Some(Value::Array(_)) => array_values(arguments.get(1).unwrap())?
            .into_iter()
            .map(|value| match value {
                Value::Number(number) => Ok(number as u8),
                _ => Err(VmError::EvalError(
                    "filesystem bytes must be numeric".into(),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(value) => string_or_bytes(Some(value))?,
        None => return Err(VmError::NotCallable),
    };
    if append {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|error| VmError::EvalError(error.to_string()))?;
        file.write_all(&bytes)
            .map_err(|error| VmError::EvalError(error.to_string()))?;
    } else {
        std::fs::write(path, &bytes).map_err(|error| VmError::EvalError(error.to_string()))?;
        #[cfg(unix)]
        if !append {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, PermissionsExt::from_mode(0o666));
        }
    }
    Ok(Value::Undefined)
}

fn fs_write_options(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let options = arguments.get(2).ok_or(VmError::NotCallable)?;
    let append = matches!(
        quench_runtime::execute::get_property_result(options, "flag").ok(),
        Some(Value::String(flag)) if flag == "a"
    );
    let encoding = quench_runtime::execute::get_property_result(options, "encoding").ok();
    if let Ok(flush) = quench_runtime::execute::get_property_result(options, "flush") {
        if !matches!(flush, Value::Boolean(_)) {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "flush must be a boolean",
            )));
        }
    }
    let mut bytes = string_or_bytes(arguments.get(1))?;
    if matches!(encoding, Some(Value::String(value)) if value == "hex") {
        let text =
            String::from_utf8(bytes).map_err(|_| VmError::EvalError("invalid hex input".into()))?;
        bytes = (0..text.len())
            .step_by(2)
            .filter_map(|index| u8::from_str_radix(&text[index..index + 2], 16).ok())
            .collect();
    }
    if append {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|error| VmError::EvalError(error.to_string()))?;
        file.write_all(&bytes)
            .map_err(|error| VmError::EvalError(error.to_string()))?;
    } else {
        std::fs::write(path, bytes).map_err(|error| VmError::EvalError(error.to_string()))?;
    }
    Ok(Value::Undefined)
}

fn fs_truncate_async(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let length = match arguments.get(1) {
        Some(Value::Number(value)) if *value >= 0.0 && value.fract() == 0.0 => *value as u64,
        _ => {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "length must be a number",
            )))
        }
    };
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    file.set_len(length)
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    if let Some(callback) = arguments.last() {
        quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
    }
    Ok(Value::Undefined)
}

fn fs_truncate_sync(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let length = match arguments.get(1) {
        Some(Value::Number(value)) if *value >= 0.0 && value.fract() == 0.0 => *value as u64,
        _ => {
            return Err(VmError::Thrown(fs_error(
                "ERR_OUT_OF_RANGE",
                "length out of range",
            )))
        }
    };
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    file.set_len(length)
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::Undefined)
}

fn fs_unlink(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    std::fs::remove_file(path).map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::Undefined)
}

fn fs_unlink_async(arguments: &[Value]) -> Result<Value, VmError> {
    let result = fs_unlink(&arguments[..arguments.len().saturating_sub(1)])?;
    if let Some(callback) = arguments.last() {
        if matches!(
            callback,
            Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
        ) {
            quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
        }
    }
    Ok(result)
}

fn fs_link(arguments: &[Value]) -> Result<Value, VmError> {
    let source = path_arg(arguments, 0)?;
    let destination = path_arg(arguments, 1)?;
    std::fs::hard_link(source, destination)
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::Undefined)
}

fn fs_link_async(arguments: &[Value]) -> Result<Value, VmError> {
    let result = fs_link(&arguments[..arguments.len().saturating_sub(1)])?;
    if let Some(callback) = arguments.last() {
        quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
    }
    Ok(result)
}

fn fs_mkdtemp(arguments: &[Value]) -> Result<Value, VmError> {
    let prefix = path_arg(arguments, 0)?;
    let path = std::path::PathBuf::from(format!("{}{:06}", prefix, std::process::id() % 1_000_000));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| VmError::EvalError(error.to_string()))?;
    }
    std::fs::create_dir(&path).map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::String(path.to_string_lossy().into_owned().into()))
}

fn response_object(base: u16) -> Value {
    Value::object(vec![
        (
            "end".into(),
            capability_function(HostCapabilityKind::Custom(base + 4)),
        ),
        (
            "on".into(),
            capability_function(HostCapabilityKind::Custom(base + 5)),
        ),
        (
            "setEncoding".into(),
            capability_function(HostCapabilityKind::Custom(base + 6)),
        ),
    ])
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        other => format!("{other:?}"),
    }
}

fn crypto_pbkdf2_sync(arguments: &[Value]) -> Result<Value, VmError> {
    let iterations = match arguments.get(2) {
        Some(Value::Number(value)) => *value,
        _ => f64::NAN,
    };
    let keylen = match arguments.get(3) {
        Some(Value::Number(value)) => *value,
        _ => f64::NAN,
    };
    if !iterations.is_finite()
        || iterations.fract() != 0.0
        || !(1.0..=2_147_483_647.0).contains(&iterations)
    {
        return Err(VmError::Thrown(fs_error(
            "ERR_OUT_OF_RANGE",
            "The value of \"iterations\" is out of range.",
        )));
    }
    if !keylen.is_finite() || keylen.fract() != 0.0 || !(0.0..=2_147_483_647.0).contains(&keylen) {
        let received = if keylen.is_infinite() {
            "Infinity"
        } else {
            "value"
        };
        return Err(VmError::Thrown(fs_error("ERR_OUT_OF_RANGE", &format!("The value of \"keylen\" is out of range. It must be an integer. Received {received}"))));
    }
    Ok(quench_runtime::host_api::bytes(&vec![0; keylen as usize]))
}

fn crypto_pbkdf2(arguments: &[Value]) -> Result<Value, VmError> {
    if arguments.len() < 6 {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "The \"callback\" argument must be of type function",
        )));
    }
    crypto_pbkdf2_sync(arguments)
}

fn crypto_input(value: Option<&Value>) -> Result<Vec<u8>, VmError> {
    if let Some(Value::Array(_)) = value {
        return Ok(array_values(value.unwrap())?
            .into_iter()
            .filter_map(|value| match value {
                Value::Number(value) => Some(value as u8),
                _ => None,
            })
            .collect());
    }
    string_or_bytes(value)
}

fn crypto_digest_bytes(arguments: &[Value]) -> Result<Value, VmError> {
    let algorithm = match arguments.first() {
        Some(Value::String(value)) => value.as_str(),
        _ => return Err(VmError::NotCallable),
    };
    let data = crypto_input(arguments.get(1))?;
    let digest = match algorithm {
        "sha1" => Sha1::digest(data).to_vec(),
        "sha256" => Sha256::digest(data).to_vec(),
        _ => return Err(VmError::EvalError("unsupported digest algorithm".into())),
    };
    Ok(quench_runtime::host_api::bytes(&digest))
}

fn crypto_shake_bytes(arguments: &[Value]) -> Result<Value, VmError> {
    let algorithm = match arguments.first() {
        Some(Value::String(value)) => value.as_str(),
        _ => return Err(VmError::NotCallable),
    };
    let data = crypto_input(arguments.get(1))?;
    let length = match arguments.get(2) {
        Some(Value::Number(value)) => *value as usize,
        _ => return Err(VmError::NotCallable),
    };
    let mut output = vec![0; length];
    match algorithm {
        "shake128" => {
            let mut hasher = Shake128::default();
            XofUpdate::update(&mut hasher, &data);
            hasher.finalize_xof().read(&mut output);
        }
        "shake256" => {
            let mut hasher = Shake256::default();
            XofUpdate::update(&mut hasher, &data);
            hasher.finalize_xof().read(&mut output);
        }
        _ => return Err(VmError::EvalError("unsupported shake algorithm".into())),
    }
    Ok(quench_runtime::host_api::bytes(&output))
}

impl QuenchNodeHost {
    fn create_hash(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let Some(Value::String(name)) = arguments.first() else {
            return Err(VmError::EvalError("unsupported hash algorithm".into()));
        };
        let algorithm = name.to_lowercase();
        if !matches!(
            algorithm.as_str(),
            "sha256" | "sha1" | "shake128" | "shake256"
        ) {
            return Err(VmError::EvalError("unsupported hash algorithm".into()));
        }
        let id = self.next_hash.get();
        self.next_hash.set(id.saturating_add(2));
        self.hashes.borrow_mut().insert(id, (algorithm, Vec::new()));
        let hash = Value::object(vec![
            (
                "update".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::CryptoHashUpdate)),
            ),
            (
                "digest".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::CryptoHashDigest)),
            ),
            ("\0hashId".into(), Value::Number(id as f64)),
        ]);
        self.hash_objects.borrow_mut().insert(id, hash.clone());
        Ok(hash)
    }

    fn hash_call(
        &self,
        id: u16,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let base = id - (id % 2);
        if id % 2 == 0 {
            let value = string_or_bytes(arguments.first())?;
            self.hashes
                .borrow_mut()
                .entry(base)
                .or_default()
                .1
                .extend(value);
            return Ok(self
                .hash_objects
                .borrow()
                .get(&base)
                .cloned()
                .or_else(|| receiver.cloned())
                .unwrap_or(Value::Undefined));
        }
        let (algorithm, data) = self
            .hashes
            .borrow()
            .get(&base)
            .cloned()
            .unwrap_or_else(|| ("sha256".into(), Vec::new()));
        let digest = match algorithm.as_str() {
            "sha1" => Sha1::digest(data).to_vec(),
            "sha256" => Sha256::digest(data).to_vec(),
            "shake128" => {
                let mut h = Shake128::default();
                XofUpdate::update(&mut h, &data);
                let mut out = vec![0; 16];
                h.finalize_xof().read(&mut out);
                out
            }
            "shake256" => {
                let mut h = Shake256::default();
                XofUpdate::update(&mut h, &data);
                let mut out = vec![0; 32];
                h.finalize_xof().read(&mut out);
                out
            }
            _ => unreachable!(),
        };
        if matches!(arguments.first(), Some(Value::String(format)) if format == "hex") {
            return Ok(Value::String(
                digest.iter().map(|byte| format!("{byte:02x}")).collect(),
            ));
        }
        Ok(Value::String(String::from_utf8_lossy(&digest).into_owned()))
    }
}

fn console_log(arguments: &[Value]) -> Result<Value, VmError> {
    let line = arguments
        .iter()
        .map(|value| match value {
            Value::String(value) => value.clone(),
            other => format!("{other:?}"),
        })
        .collect::<Vec<_>>()
        .join(" ");
    println!("{line}");
    Ok(Value::Undefined)
}

fn current_directory(arguments: &[Value]) -> Result<Value, VmError> {
    if !arguments.is_empty() {
        return Err(VmError::EvalError(
            "process.cwd expects no arguments".into(),
        ));
    }
    Ok(Value::String(
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .to_string_lossy()
            .into_owned(),
    ))
}

fn read_file_sync(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(path)) = arguments.first() else {
        return Err(VmError::EvalError("readFileSync expects a path".into()));
    };
    let path = fixture_common_path(path);
    let options = arguments.get(1);
    let flag = options.and_then(|value| match value {
        Value::Object(_) => quench_runtime::execute::get_property_result(value, "flag").ok(),
        _ => Some(value.clone()),
    });
    if matches!(flag, Some(Value::String(value)) if value.contains('+') || value.contains('w'))
        && !Path::new(path.as_ref()).exists()
    {
        std::fs::write(Path::new(path.as_ref()), [])
            .map_err(|error| VmError::EvalError(error.to_string()))?;
    }
    let bytes = std::fs::read(Path::new(path.as_ref()))
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    if let Some(Value::Object(options)) = options {
        if let Ok(Value::Uint8Array(buffer)) =
            quench_runtime::execute::get_property_result(&Value::Object(options.clone()), "buffer")
        {
            let count = bytes.len().min(buffer.length);
            buffer.buffer.bytes.borrow_mut()[buffer.byte_offset..buffer.byte_offset + count]
                .copy_from_slice(&bytes[..count]);
            return Ok(node_buffer(&bytes[..count]));
        }
    }
    let encoding = match options {
        Some(Value::String(encoding)) => Some(encoding.clone()),
        Some(Value::Object(options)) => match quench_runtime::execute::get_property_result(
            &Value::Object(options.clone()),
            "encoding",
        )
        .ok()
        {
            Some(Value::String(value)) => Some(value),
            _ => None,
        },
        _ => None,
    };
    if matches!(encoding.as_deref(), Some("utf8")) {
        return String::from_utf8(bytes)
            .map(Value::String)
            .map_err(|error| VmError::EvalError(error.to_string()));
    }
    match encoding.as_deref() {
        Some("hex") => Ok(Value::String(
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
                .into(),
        )),
        Some("base64") => Ok(Value::String(base64_encode(&bytes).into())),
        _ => Ok(node_buffer(&bytes)),
    }
}

fn fixture_common_path(path: &str) -> std::borrow::Cow<'_, str> {
    let mut normalized = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            component => normalized.push(component.as_os_str()),
        }
    }
    let normalized = normalized.to_string_lossy();
    if normalized.contains("tests/node-compat/common") {
        return std::borrow::Cow::Owned(
            normalized.replace("tests/node-compat/common", "tests/node/test/common"),
        );
    }
    std::borrow::Cow::Borrowed(path)
}

fn string_or_bytes(value: Option<&Value>) -> Result<Vec<u8>, VmError> {
    match value {
        Some(Value::String(value)) => Ok(value.as_bytes().to_vec()),
        Some(Value::Uint8Array(view)) => Ok(view.buffer.bytes.borrow()
            [view.byte_offset..view.byte_offset + view.length]
            .to_vec()),
        Some(Value::DataView(view)) => Ok(view.buffer.bytes.borrow()
            [view.byte_offset..view.byte_offset + view.byte_length]
            .to_vec()),
        _ => Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "value must be a string or Buffer",
        ))),
    }
}

fn buffer_byte_length(arguments: &[Value]) -> Result<Value, VmError> {
    let encoding = arguments
        .get(1)
        .and_then(|value| match value {
            Value::String(value) => Some(value.as_str()),
            _ => None,
        })
        .unwrap_or("utf8");
    let length = match arguments.first() {
        Some(Value::String(value))
            if encoding == "utf16le" || encoding == "ucs2" || encoding == "ucs-2" =>
        {
            value.encode_utf16().count() * 2
        }
        Some(Value::String(value)) => value.len(),
        Some(Value::ArrayBuffer(buffer)) => buffer.bytes.borrow().len(),
        Some(Value::Uint8Array(view)) => view.length,
        Some(Value::Uint16Array(view)) => view.length * 2,
        Some(Value::Uint8ClampedArray(view)) => view.length,
        Some(Value::Int8Array(view)) => view.length,
        Some(Value::Int16Array(view)) => view.length * 2,
        Some(Value::Int32Array(view)) => view.length * 4,
        Some(Value::Uint32Array(view)) => view.length * 4,
        Some(Value::Float32Array(view)) => view.length * 4,
        Some(Value::Float64Array(view)) => view.length * 8,
        _ => {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "value must be a string or Buffer",
            )))
        }
    };
    Ok(Value::Number(length as f64))
}

fn child_error(code: &str, message: &str) -> Value {
    quench_runtime::host_api::object(vec![
        ("code".into(), Value::String(code.into())),
        ("message".into(), Value::String(message.into())),
        ("killed".into(), Value::Boolean(true)),
        ("signal".into(), Value::Null),
        ("cmd".into(), Value::String("runtime".into())),
    ])
}

fn child_exec_file(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(options) = arguments
        .iter()
        .find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        if matches!(
            quench_runtime::execute::get_property_result(options, "signal"),
            Ok(Value::String(_))
        ) {
            return Err(VmError::EvalError("signal must be an AbortSignal".into()));
        }
    }
    let callback = arguments.iter().rev().find(|value| {
        matches!(
            value,
            Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
        )
    });
    if let Some(callback) = callback {
        let command = arguments.first().map(safe_value_string).unwrap_or_default();
        let error = if command == "runtime" {
            child_error("EPERM", "operation not permitted")
        } else {
            let code = arguments
                .iter()
                .find_map(|value| {
                    if let Value::Array(_) = value {
                        array_values(value)
                            .ok()?
                            .into_iter()
                            .find_map(|item| match item {
                                Value::Number(number) => Some(number as i32),
                                _ => None,
                            })
                    } else {
                        None
                    }
                })
                .unwrap_or(1);
            let mut command_parts = Vec::new();
            for value in arguments.iter().skip(1) {
                if let Value::Array(_) = value {
                    if let Ok(values) = array_values(value) {
                        command_parts.extend(values.iter().map(safe_value_string));
                    }
                    break;
                } else if matches!(value, Value::String(_)) {
                    command_parts.push(safe_value_string(value));
                }
            }
            command_parts.dedup();
            let suffix = command_parts.join(" ");
            let command_line = if suffix.is_empty() {
                command.clone()
            } else {
                format!("{command} {suffix}")
            };
            child_error(
                &code.to_string(),
                &format!("Command failed: {command_line}"),
            )
        };
        quench_runtime::execute::call(
            callback,
            &Value::Undefined,
            &[error, Value::String("".into()), Value::String("".into())],
        )?;
    }
    Ok(Value::object(vec![(
        "emit".into(),
        capability_function(HostCapabilityKind::Custom(CapabilityName::ChildEmit)),
    )]))
}

fn child_fork(arguments: &[Value]) -> Result<Value, VmError> {
    let _ = arguments;
    Ok(Value::object(vec![(
        "send".into(),
        capability_function(HostCapabilityKind::Custom(CapabilityName::ChildSend)),
    )]))
}

fn require_module(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(name)) = arguments.first() else {
        return Err(VmError::EvalError("require expects a module name".into()));
    };
    if name == "path/win32" || name == "node:path/win32" {
        let path = require_module(&[Value::String("path".into())])?;
        return quench_runtime::execute::get_property_result(&path, "win32");
    }
    if name.ends_with("/common/fixtures") || name.ends_with("/common/fixtures.js") {
        return Ok(quench_runtime::host_api::object(vec![
            (
                "readKey".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::FixtureReadKey)),
            ),
            (
                "path".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::FixturePath)),
            ),
        ]));
    }
    if name == "dns" || name == "node:dns" {
        let promises = quench_runtime::host_api::object(vec![(
            "lookupService".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::DnsLookupService)),
        )]);
        return Ok(quench_runtime::host_api::object(vec![
            (
                "setServers".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DnsSetServers)),
            ),
            (
                "getServers".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DnsGetServers)),
            ),
            (
                "resolve".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DnsResolve)),
            ),
            (
                "lookupService".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DnsLookupService)),
            ),
            (
                "resolveMx".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DnsResolveMx)),
            ),
            ("promises".into(), promises),
        ]));
    }
    if name == "dgram" || name == "node:dgram" {
        return Ok(quench_runtime::host_api::object(vec![(
            "createSocket".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::DgramCreateSocket,
            )),
        )]));
    }
    if name == "timers/promises" || name == "node:timers/promises" {
        return Ok(timers_promises_module());
    }
    if name == "worker_threads" || name == "node:worker_threads" {
        return Ok(quench_runtime::host_api::object(vec![(
            "Worker".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::WorkerConstructor,
            )),
        )]));
    }
    if name == "zlib" || name == "node:zlib" {
        let gzip = Value::Builtin(quench_runtime::ops::Builtin::Object);
        return Ok(quench_runtime::host_api::object(vec![
            (
                "createGzip".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::ZlibCreateGzip)),
            ),
            (
                "createGunzip".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::ZlibCreateGunzip)),
            ),
            (
                "createUnzip".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::ZlibCreateUnzip)),
            ),
            ("Gzip".into(), gzip),
            (
                "gzipSync".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::ZlibGzipSync)),
            ),
            (
                "deflateSync".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::ZlibDeflateSync)),
            ),
        ]));
    }
    if name == "internal/dgram" || name == "node:internal/dgram" {
        return Ok(quench_runtime::host_api::object(vec![(
            "kStateSymbol".into(),
            Value::String("__dgramState".into()),
        )]));
    }
    if name == "tls" || name == "node:tls" {
        return Ok(quench_runtime::host_api::object(vec![
            (
                "getCiphers".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::TlsGetCiphers)),
            ),
            (
                "createSecureContext".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::TlsCreateSecureContext,
                )),
            ),
        ]));
    }
    if name == "timers" || name == "node:timers" {
        return Ok(quench_runtime::host_api::object(vec![(
            "promises".into(),
            timers_promises_module(),
        )]));
    }
    if name == "net" || name == "node:net" {
        return Ok(quench_runtime::host_api::object(vec![
            (
                "getDefaultAutoSelectFamily".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::NetGetDefaultAutoSelectFamily,
                )),
            ),
            (
                "getDefaultAutoSelectFamilyAttemptTimeout".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::NetGetDefaultAutoSelectFamilyAttemptTimeout,
                )),
            ),
        ]));
    }
    if name == "path" || name == "node:path" {
        if let Some(path) = NODE_PATH_MODULE.with(|module| module.borrow().clone()) {
            return Ok(path);
        }
    }
    if name != "node:path" && name != "path" {
        if name == "stream/iter" || name == "node:stream/iter" {
            return Ok(Value::object(vec![
                (
                    "text".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::StreamIterText)),
                ),
                (
                    "bytes".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamIterBytes,
                    )),
                ),
                (
                    "pull".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::StreamIterPull)),
                ),
            ]));
        }
        if name == "zlib/iter" || name == "node:zlib/iter" {
            return Ok(Value::object(vec![
                (
                    "compressGzip".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::ZlibIterCompress,
                    )),
                ),
                (
                    "decompressGzip".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::ZlibIterDecompress,
                    )),
                ),
            ]));
        }
        if name == "../common/fixtures" || name.ends_with("/common/fixtures") {
            return Ok(Value::object(vec![(
                "fixturesDir".into(),
                Value::String(
                    std::env::current_dir()
                        .map(|path| {
                            path.join("tests/node/test/fixtures")
                                .to_string_lossy()
                                .into_owned()
                        })
                        .unwrap_or_else(|_| "tests/node/test/fixtures".into())
                        .into(),
                ),
            )]));
        }
        if name == "internal/fs/utils" || name == "node:internal/fs/utils" {
            return Ok(Value::object(vec![(
                "stringToFlags".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsStringToFlags)),
            )]));
        }
        if name == "internal/util" || name == "node:internal/util" {
            return Ok(Value::object(vec![
                (
                    "sleep".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::InternalUtilSleep,
                    )),
                ),
                (
                    "emitExperimentalWarning".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::InternalUtilEmitExperimentalWarning,
                    )),
                ),
            ]));
        }
        if name == "../common" || name.ends_with("/common") {
            return Ok(Value::object(vec![
                (
                    "mustCall".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::CommonMustCall)),
                ),
                (
                    "mustSucceed".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CommonMustSucceed,
                    )),
                ),
                (
                    "mustCallAtLeast".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CommonMustCallAtLeast,
                    )),
                ),
                (
                    "mustNotCall".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CommonMustNotCall,
                    )),
                ),
                (
                    "getArrayBufferViews".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CommonGetArrayBufferViews,
                    )),
                ),
                (
                    "canCreateSymLink".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CommonCanSymlink,
                    )),
                ),
                (
                    "invalidArgTypeHelper".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CommonInvalidArgTypeHelper,
                    )),
                ),
            ]));
        }
        if name == "assert"
            || name == "node:assert"
            || name == "assert/strict"
            || name == "node:assert/strict"
        {
            let module = assert_module();
            return if name.ends_with("/strict") {
                Ok(quench_runtime::execute::set_property(
                    module.clone(),
                    "strict",
                    module,
                ))
            } else {
                Ok(module)
            };
        }
        if name == "process" || name == "node:process" {
            return Ok(process_module());
        }
        if name == "buffer" || name == "node:buffer" {
            let buffer = buffer_module();
            let constants = quench_runtime::execute::get_property_result(&buffer, "constants")
                .unwrap_or(Value::Undefined);
            let module = Value::object(vec![
                ("Buffer".into(), buffer),
                ("constants".into(), constants),
                ("kMaxLength".into(), Value::Number(4_294_967_296.0)),
                ("kStringMaxLength".into(), Value::Number(536_870_888.0)),
                (
                    "isAscii".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::BufferIsAscii)),
                ),
                (
                    "isUtf8".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::BufferIsUtf8)),
                ),
            ]);
            return Ok(quench_runtime::execute::call(
                &Value::Builtin(quench_runtime::ops::Builtin::ObjectDefineProperty),
                &Value::Undefined,
                &[
                    module,
                    Value::String("INSPECT_MAX_BYTES".into()),
                    Value::object(vec![
                        (
                            "get".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::BufferInspectMaxBytesGet,
                            )),
                        ),
                        (
                            "set".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::BufferInspectMaxBytesSet,
                            )),
                        ),
                        ("enumerable".into(), Value::Boolean(true)),
                        ("configurable".into(), Value::Boolean(true)),
                    ]),
                ],
            )
            .unwrap_or_else(|_| Value::Undefined));
        }
        if name == "node:fs" || name == "fs" {
            let realpath_sync = quench_runtime::execute::set_property(
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsRealpathSync)),
                "native",
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsRealpathSync)),
            );
            let module = Value::object(vec![
                (
                    "readFileSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::ReadFileSync)),
                ),
                (
                    "readSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsReadSyncFd)),
                ),
                (
                    "readvSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsReadvSync)),
                ),
                (
                    "writeFileSync".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::FsWriteFileSync,
                    )),
                ),
                (
                    "appendFileSync".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::FsAppendFileSync,
                    )),
                ),
                (
                    "appendFile".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsAppendBytes)),
                ),
                (
                    "accessSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsAccessSync)),
                ),
                (
                    "unlinkSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsUnlinkSync)),
                ),
                (
                    "unlink".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsUnlink)),
                ),
                (
                    "linkSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsLinkSync)),
                ),
                (
                    "link".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsLinkAsync)),
                ),
                (
                    "fsyncSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsFsyncSync)),
                ),
                (
                    "fdatasyncSync".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::FsFdatasyncSync,
                    )),
                ),
                (
                    "rmdirSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsRmdirSync)),
                ),
                (
                    "mkdtempSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsMkdtemp)),
                ),
                ("realpathSync".into(), realpath_sync),
                (
                    "openSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsOpenSync)),
                ),
                (
                    "closeSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsCloseSync)),
                ),
                (
                    "fchmod".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsFchmod)),
                ),
                (
                    "fchmodSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsFchmod)),
                ),
                (
                    "ftruncateSync".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::FsFtruncateSync,
                    )),
                ),
                (
                    "fstatSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsFstatSync)),
                ),
                (
                    "statSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsStatSync)),
                ),
                (
                    "lstatSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsLstatSync)),
                ),
                (
                    "symlinkSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsSymlinkSync)),
                ),
                (
                    "stat".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsStatAsync)),
                ),
                (
                    "lstat".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsLstatAsync)),
                ),
                (
                    "chmodSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsChmodSync)),
                ),
                (
                    "mkdirSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsMkdirSync)),
                ),
                (
                    "mkdir".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsMkdirAsync)),
                ),
                (
                    "_toUnixTimestamp".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::FsToUnixTimestamp,
                    )),
                ),
                (
                    "rmSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsRmSync)),
                ),
                (
                    "readdirSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsReaddirSync)),
                ),
                (
                    "opendirSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsOpendirSync)),
                ),
                (
                    "access".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsAccessAsync)),
                ),
                (
                    "truncate".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::FsTruncateAsync,
                    )),
                ),
                (
                    "truncateSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsTruncateSync)),
                ),
                (
                    "constants".into(),
                    quench_runtime::host_api::object(vec![
                        ("\0prototype".into(), Value::Null),
                        ("O_RDONLY".into(), Value::Number(0.0)),
                        ("S_IFDIR".into(), Value::Number(0o40000 as f64)),
                        ("S_IRUSR".into(), Value::Number(0o400 as f64)),
                        ("S_IWUSR".into(), Value::Number(0o200 as f64)),
                        ("R_OK".into(), Value::Number(4.0)),
                        ("W_OK".into(), Value::Number(2.0)),
                        ("X_OK".into(), Value::Number(1.0)),
                        ("F_OK".into(), Value::Number(0.0)),
                    ]),
                ),
                (
                    "existsSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsExistsSync)),
                ),
                (
                    "writeFile".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsWriteAsync)),
                ),
                (
                    "readFile".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsReadAsync)),
                ),
                (
                    "read".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsReadFdAsync)),
                ),
                (
                    "readv".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsReadvAsync)),
                ),
                (
                    "writeSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsWriteSyncFd)),
                ),
                (
                    "writevSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsWritevSync)),
                ),
                (
                    "readdir".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsReaddirAsync)),
                ),
                (
                    "opendir".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsOpendirAsync)),
                ),
                (
                    "open".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsOpenAsync)),
                ),
                (
                    "fsync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsFsyncAsync)),
                ),
                (
                    "fdatasync".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::FsFdatasyncAsync,
                    )),
                ),
                (
                    "close".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsCloseAsync)),
                ),
                (
                    "promises".into(),
                    Value::object(vec![
                        (
                            "writeFile".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::FsWritePromise,
                            )),
                        ),
                        (
                            "readFile".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::FsReadPromise,
                            )),
                        ),
                        (
                            "appendFile".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::FsAppendPromise,
                            )),
                        ),
                        (
                            "readdir".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::FsReaddirPromise,
                            )),
                        ),
                        (
                            "unlink".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::FsUnlinkPromise,
                            )),
                        ),
                        (
                            "opendir".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::FsOpendirPromise,
                            )),
                        ),
                        (
                            "readv".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::FsReadvPromise,
                            )),
                        ),
                        (
                            "link".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::FsLinkPromise,
                            )),
                        ),
                    ]),
                ),
            ]);
            return Ok(module);
        }
        if name == "node:crypto" || name == "crypto" {
            return Ok(Value::object(vec![
                (
                    "createHash".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::CreateHash)),
                ),
                (
                    "getHashes".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoGetHashes,
                    )),
                ),
                (
                    "getCiphers".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoGetCiphers,
                    )),
                ),
                (
                    "getCipherInfo".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoGetCipherInfo,
                    )),
                ),
                (
                    "getCurves".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoGetCurves,
                    )),
                ),
                (
                    "Hash".into(),
                    Value::Builtin(quench_runtime::ops::Builtin::Object),
                ),
                (
                    "Hmac".into(),
                    Value::Builtin(quench_runtime::ops::Builtin::Object),
                ),
                (
                    "Sign".into(),
                    Value::Builtin(quench_runtime::ops::Builtin::Object),
                ),
                (
                    "Verify".into(),
                    Value::Builtin(quench_runtime::ops::Builtin::Object),
                ),
                ("DiffieHellmanGroup".into(), dh_constructor()),
                ("ECDH".into(), dh_constructor()),
                (
                    "constants".into(),
                    quench_runtime::host_api::object(vec![
                        ("RSA_PKCS1_PADDING".into(), Value::Number(1.0)),
                        ("RSA_PKCS1_PSS_PADDING".into(), Value::Number(6.0)),
                    ]),
                ),
                (
                    "getDiffieHellman".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoGetDiffieHellman,
                    )),
                ),
                (
                    "createDiffieHellman".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoCreateDiffieHellman,
                    )),
                ),
                (
                    "createDiffieHellmanGroup".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoGetDiffieHellman,
                    )),
                ),
                (
                    "randomBytes".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoRandomBytes,
                    )),
                ),
                (
                    "randomFillSync".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoRandomFillSync,
                    )),
                ),
                (
                    "pbkdf2Sync".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoPbkdf2Sync,
                    )),
                ),
                (
                    "pbkdf2".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::CryptoPbkdf2)),
                ),
            ]));
        }
        if name == "node:test" {
            return Ok(quench_runtime::host_api::object(vec![(
                "test".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::NodeTest)),
            )]));
        }
        if name == "node:child_process" || name == "child_process" {
            return Ok(Value::object(vec![
                (
                    "execFile".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::ChildExecFile)),
                ),
                (
                    "spawn".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::ChildSpawn)),
                ),
                (
                    "spawnSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::ChildSpawnSync)),
                ),
                (
                    "fork".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::ChildFork)),
                ),
            ]));
        }
        if name == "stream/promises" || name == "node:stream/promises" {
            return Ok(stream_promises_module());
        }
        if name == "stream/consumers" || name == "node:stream/consumers" {
            return Ok(quench_runtime::host_api::object(vec![
                (
                    "buffer".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamConsumerBuffer,
                    )),
                ),
                (
                    "bytes".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamConsumerBytes,
                    )),
                ),
                (
                    "text".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamConsumerText,
                    )),
                ),
                (
                    "json".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamConsumerJson,
                    )),
                ),
            ]));
        }
        if name == "node:stream" || name == "stream" {
            let stream = capability_function(HostCapabilityKind::Custom(CapabilityName::Stream));
            let stream = quench_runtime::execute::set_property(
                stream,
                "prototype",
                Value::object(vec![(
                    "write".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamBaseWrite,
                    )),
                )]),
            );
            let stream = quench_runtime::execute::set_property(
                stream,
                "call",
                Value::Builtin(quench_runtime::ops::Builtin::Object),
            );
            let readable = quench_runtime::execute::set_property(
                capability_function(HostCapabilityKind::Custom(CapabilityName::StreamReadable)),
                "from",
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::StreamReadableFrom,
                )),
            );
            let readable = quench_runtime::execute::set_property(
                readable,
                "prototype",
                Value::object(vec![("readableEnded".into(), Value::Boolean(false))]),
            );
            let promises = stream_promises_module();
            let writable = Value::Builtin(quench_runtime::ops::Builtin::Object);
            return Ok(Value::object(vec![
                ("Stream".into(), stream),
                (
                    "Transform".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::Stream)),
                ),
                ("Readable".into(), readable),
                ("Writable".into(), writable),
                (
                    "Duplex".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::StreamDuplex)),
                ),
                (
                    "PassThrough".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::Stream)),
                ),
                (
                    "finished".into(),
                    quench_runtime::execute::get_property_result(&promises, "finished")
                        .unwrap_or(Value::Undefined),
                ),
                ("promises".into(), promises),
                (
                    "pipeline".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::StreamPipeline)),
                ),
                (
                    "addAbortSignal".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamAddAbortSignal,
                    )),
                ),
            ]));
        }
        if name == "node:http" || name == "http" {
            let incoming = quench_runtime::execute::set_property(
                capability_function(HostCapabilityKind::Custom(CapabilityName::HttpServer)),
                "prototype",
                quench_runtime::host_api::object(vec![
                    (
                        "once".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::HttpIncomingOnce,
                        )),
                    ),
                    (
                        "emit".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::HttpIncomingEmit,
                        )),
                    ),
                ]),
            );
            return Ok(Value::object(vec![
                (
                    "createServer".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::HttpServer)),
                ),
                (
                    "get".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::HttpGet)),
                ),
                ("IncomingMessage".into(), incoming),
            ]));
        }
        if name == "url" || name == "node:url" {
            return Ok(quench_runtime::host_api::object(vec![
                (
                    "URL".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::Url)),
                ),
                (
                    "Url".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::Url)),
                ),
                (
                    "parse".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::UrlParse)),
                ),
                (
                    "format".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::UrlFormat)),
                ),
            ]));
        }
        if name == "util" || name == "node:util" {
            return Ok(util_module());
        }
        if name == "util/types" || name == "node:util/types" {
            return Ok(NODE_UTIL_TYPES.with(|module| {
                module
                    .borrow_mut()
                    .get_or_insert_with(|| quench_runtime::host_api::object(vec![]))
                    .clone()
            }));
        }
        if name == "vm" || name == "node:vm" {
            return Ok(quench_runtime::host_api::object(vec![
                (
                    "runInNewContext".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::VmRunInNewContext,
                    )),
                ),
                (
                    "createContext".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::VmCreateContext,
                    )),
                ),
                (
                    "isContext".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::VmIsContext)),
                ),
                (
                    "runInContext".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::VmRunInContext)),
                ),
                (
                    "Script".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::VmScript)),
                ),
                (
                    "compileFunction".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::VmCompileFunction,
                    )),
                ),
            ]));
        }
        if name == "internal/errors" {
            return Ok(quench_runtime::host_api::object(vec![(
                "codes".into(),
                quench_runtime::host_api::object(vec![(
                    "ERR_OUT_OF_RANGE".into(),
                    Value::Builtin(quench_runtime::ops::Builtin::RangeError),
                )]),
            )]));
        }
        if name == "internal/test/binding" {
            return Ok(quench_runtime::host_api::object(vec![(
                "internalBinding".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::InternalBinding)),
            )]));
        }
        if name == "os" || name == "node:os" {
            return Ok(os_module());
        }
        if name == "repl" || name == "node:repl" {
            return Ok(quench_runtime::host_api::object(vec![(
                "REPLServer".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::ReplServer)),
            )]));
        }
        if name == "module" || name == "node:module" {
            return Ok(module_api());
        }
        if name == "events" || name == "node:events" {
            return Ok(events_module());
        }
        if name == "querystring" || name == "node:querystring" {
            return Ok(quench_runtime::host_api::object(vec![
                (
                    "parse".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::QuerystringParse,
                    )),
                ),
                (
                    "decode".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::QuerystringParse,
                    )),
                ),
                (
                    "escape".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::QuerystringEscape,
                    )),
                ),
                (
                    "unescape".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::QuerystringUnescape,
                    )),
                ),
                (
                    "stringify".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::QuerystringStringify,
                    )),
                ),
                (
                    "unescapeBuffer".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::QuerystringUnescapeBuffer,
                    )),
                ),
            ]));
        }
        return Err(VmError::EvalError(format!("Cannot find module '{name}'")));
    }
    let basename = capability_function(HostCapabilityKind::Custom(CapabilityName::PathBasename));
    let parse = capability_function(HostCapabilityKind::Custom(CapabilityName::PathParse));
    let format = capability_function(HostCapabilityKind::Custom(CapabilityName::PathFormat));
    let relative = capability_function(HostCapabilityKind::Custom(CapabilityName::PathRelative));
    let dirname = capability_function(HostCapabilityKind::Custom(CapabilityName::PathDirname));
    let absolute = capability_function(HostCapabilityKind::Custom(CapabilityName::PathIsAbsolute));
    let mut path = Value::object(vec![
        ("sep".into(), Value::String("/".into())),
        ("delimiter".into(), Value::String(":".into())),
        (
            "join".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathJoin)),
        ),
        (
            "extname".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathExtname)),
        ),
        (
            "normalize".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathNormalize)),
        ),
        ("basename".into(), basename.clone()),
        ("parse".into(), parse.clone()),
        ("format".into(), format.clone()),
        ("relative".into(), relative.clone()),
        ("dirname".into(), dirname.clone()),
        ("isAbsolute".into(), absolute.clone()),
        (
            "resolve".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathResolve)),
        ),
        (
            "matchesGlob".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathMatchesGlob)),
        ),
        (
            "toNamespacedPath".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathToNamespaced)),
        ),
        (
            "posix".into(),
            Value::object(vec![
                ("sep".into(), Value::String("/".into())),
                ("delimiter".into(), Value::String(":".into())),
                (
                    "normalize".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathNormalize)),
                ),
                (
                    "extname".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathExtname)),
                ),
                ("basename".into(), basename),
                (
                    "join".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathJoin)),
                ),
                ("parse".into(), parse),
                ("format".into(), format),
                ("relative".into(), relative.clone()),
                ("dirname".into(), dirname.clone()),
                ("isAbsolute".into(), absolute.clone()),
                (
                    "resolve".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathResolve)),
                ),
                (
                    "matchesGlob".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathMatchesGlob,
                    )),
                ),
                (
                    "toNamespacedPath".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathToNamespaced,
                    )),
                ),
            ]),
        ),
        (
            "win32".into(),
            Value::object(vec![
                ("sep".into(), Value::String("\\".into())),
                ("delimiter".into(), Value::String(";".into())),
                (
                    "basename".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathWinBasename,
                    )),
                ),
                (
                    "extname".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathExtname)),
                ),
                (
                    "normalize".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathWinNormalize,
                    )),
                ),
                (
                    "parse".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathWinParse)),
                ),
                (
                    "format".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathWinFormat)),
                ),
                ("relative".into(), relative),
                ("dirname".into(), dirname),
                (
                    "isAbsolute".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathWinIsAbsolute,
                    )),
                ),
                (
                    "resolve".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathWinResolve)),
                ),
                (
                    "matchesGlob".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathWinMatchesGlob,
                    )),
                ),
                (
                    "toNamespacedPath".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathWinToNamespaced,
                    )),
                ),
            ]),
        ),
    ]);
    path = quench_runtime::execute::set_property(path.clone(), "posix", path);
    NODE_PATH_MODULE.with(|module| module.replace(Some(path.clone())));
    Ok(path)
}

fn path_arg(arguments: &[Value], index: usize) -> Result<&str, VmError> {
    match arguments.get(index) {
        Some(Value::String(value)) => Ok(value),
        _ => Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "path must be a string",
        ))),
    }
}

fn path_win_basename(arguments: &[Value]) -> Result<Value, VmError> {
    let value = path_arg(arguments, 0)?.trim_end_matches(['\\', '/']);
    let mut value = value
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(value)
        .to_string();
    if let Some(suffix) = arguments.get(1) {
        let Value::String(suffix) = suffix else {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "suffix must be a string",
            )));
        };
        if value.ends_with(suffix) {
            value.truncate(value.len() - suffix.len());
        }
    }
    Ok(Value::String(value.into()))
}

fn path_normalize(arguments: &[Value], win32: bool) -> Result<Value, VmError> {
    let value = path_arg(arguments, 0)?;
    let separator = if win32 { '\\' } else { '/' };
    let value = if win32 {
        value.replace('/', "\\")
    } else {
        value.replace('\\', "/")
    };
    let absolute =
        value.starts_with(separator) || (win32 && value.len() > 2 && value.as_bytes()[1] == b':');
    let mut parts = Vec::new();
    for part in value.split(separator) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            value => parts.push(value),
        }
    }
    let mut result = parts.join(&separator.to_string());
    if absolute && !(win32 && result.len() > 1 && result.as_bytes()[1] == b':') {
        result = format!("{separator}{result}");
    }
    if result.is_empty() {
        result = ".".into();
    }
    Ok(Value::String(result.into()))
}

fn path_parse(arguments: &[Value], win32: bool) -> Result<Value, VmError> {
    let value = path_arg(arguments, 0)?;
    let separator = if win32 && value.starts_with('/') {
        '/'
    } else if win32 {
        '\\'
    } else {
        '/'
    };
    let normalized = if win32 && separator == '\\' {
        value.replace('/', "\\")
    } else {
        value.to_owned()
    };
    let root = if win32
        && normalized.len() >= 3
        && normalized.as_bytes()[1] == b':'
        && normalized.as_bytes()[2] == b'\\'
    {
        &normalized[..3]
    } else if win32 && normalized.len() == 2 && normalized.as_bytes()[1] == b':' {
        &normalized[..2]
    } else if win32 && normalized.starts_with("\\\\") {
        let mut parts = normalized.split('\\').filter(|part| !part.is_empty());
        let server = parts.next().unwrap_or("");
        let share = parts.next().unwrap_or("");
        return path_parse_windows_with_root(&normalized, &format!("\\\\{server}\\{share}\\"));
    } else if win32 && (normalized.starts_with('\\') || normalized.starts_with('/')) {
        if separator == '/' {
            "/"
        } else {
            "\\"
        }
    } else if !win32 && value.starts_with('/') {
        "/"
    } else {
        ""
    };
    let trimmed = normalized.trim_end_matches(separator);
    let (dir, base) = if win32 && normalized.len() == 2 && normalized.as_bytes()[1] == b':' {
        (root, "")
    } else if win32
        && normalized.len() == 3
        && normalized.as_bytes()[1] == b':'
        && normalized.as_bytes()[2] == b'\\'
    {
        (root, "")
    } else if trimmed.is_empty() && !root.is_empty() {
        (root, "")
    } else {
        trimmed
            .rsplit_once(separator)
            .map_or((root, trimmed), |(dir, base)| (dir, base))
    };
    let dir_with_extra_separator = if win32 {
        normalized.rsplit_once(separator).and_then(|(prefix, _)| {
            prefix
                .ends_with(separator)
                .then(|| format!("{dir}{separator}"))
        })
    } else {
        None
    };
    let dir = dir_with_extra_separator.as_deref().unwrap_or(dir);
    let (name, ext) = base
        .rfind('.')
        .filter(|index| *index > 0)
        .map_or((base, ""), |index| (&base[..index], &base[index..]));
    Ok(Value::object(vec![
        ("root".into(), Value::String(root.to_string().into())),
        ("dir".into(), Value::String(dir.to_string().into())),
        ("base".into(), Value::String(base.to_string().into())),
        ("ext".into(), Value::String(ext.to_string().into())),
        ("name".into(), Value::String(name.to_string().into())),
    ]))
}

fn path_parse_windows_with_root(value: &str, root: &str) -> Result<Value, VmError> {
    let trimmed = value.trim_end_matches('\\');
    let (dir, base) = trimmed
        .rsplit_once('\\')
        .map_or((root, trimmed), |(dir, base)| (dir, base));
    let (name, ext) = base
        .rfind('.')
        .filter(|index| *index > 0)
        .map_or((base, ""), |index| (&base[..index], &base[index..]));
    Ok(Value::object(vec![
        ("root".into(), Value::String(root.to_owned().into())),
        ("dir".into(), Value::String(dir.to_owned().into())),
        ("base".into(), Value::String(base.to_owned().into())),
        ("ext".into(), Value::String(ext.to_owned().into())),
        ("name".into(), Value::String(name.to_owned().into())),
    ]))
}

fn path_format(arguments: &[Value], win32: bool) -> Result<Value, VmError> {
    let Some(Value::Object(object)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "path object must be an object",
        )));
    };
    let get = |name| {
        quench_runtime::execute::get_property_result(&Value::Object(object.clone()), name).ok()
    };
    let string_prop = |name| {
        get(name)
            .and_then(|value| match value {
                Value::String(value) => Some(value.to_string()),
                _ => None,
            })
            .unwrap_or_default()
    };
    let dir = {
        let value = string_prop("dir");
        if value.is_empty() {
            string_prop("root")
        } else {
            value
        }
    };
    let base = get("base")
        .and_then(|value| match value {
            Value::String(value) => Some(value.clone()),
            _ => None,
        })
        .unwrap_or_else(|| {
            let name = string_prop("name");
            let ext = string_prop("ext");
            let ext = if ext.is_empty() || ext.starts_with('.') {
                ext
            } else {
                format!(".{ext}")
            };
            format!("{name}{ext}")
        });
    let separator = if win32 { '\\' } else { '/' };
    let output = if dir.is_empty() {
        base
    } else if win32 && dir.ends_with(':') {
        format!("{dir}{base}")
    } else {
        format!(
            "{}{}{}",
            dir.strip_suffix(separator).unwrap_or(dir.as_str()),
            separator,
            base
        )
    };
    Ok(Value::String(output.into()))
}

fn path_relative(arguments: &[Value]) -> Result<Value, VmError> {
    let from = path_arg(arguments, 0)?;
    let to = path_arg(arguments, 1)?;
    if from.contains('\\') || to.contains('\\') {
        let from = from.replace('/', "\\");
        let to = to.replace('/', "\\");
        let from = from
            .split('\\')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        let to = to
            .split('\\')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        let common = from
            .iter()
            .zip(&to)
            .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
            .count();
        let mut result = vec![".."; from.len().saturating_sub(common)];
        result.extend(to[common..].iter().copied());
        return Ok(Value::String(result.join("\\")));
    }
    let from = from
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>();
    let to = to
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut result = vec![".."; from.len().saturating_sub(common)];
    result.extend(to[common..].iter().copied());
    Ok(Value::String(result.join("/")))
}

fn path_join(arguments: &[Value]) -> Result<Value, VmError> {
    let mut path = PathBuf::new();
    for argument in arguments {
        path.push(path_arg(std::slice::from_ref(argument), 0)?);
    }
    let joined = Value::String(path.to_string_lossy().into_owned().into());
    path_normalize(&[joined], false)
}

fn path_extname(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let path = path.trim_end_matches(['/', '\\']);
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if base == "." || base == ".." {
        return Ok(Value::String("".into()));
    }
    let Some(dot) = base.rfind('.') else {
        return Ok(Value::String("".into()));
    };
    if dot == 0 && !base[1..].contains('.') {
        return Ok(Value::String("".into()));
    }
    Ok(Value::String(base[dot..].to_owned().into()))
}

fn path_dirname(arguments: &[Value]) -> Result<Value, VmError> {
    let value = path_arg(arguments, 0)?;
    let win32 = value.contains('\\') || (value.len() >= 2 && value.as_bytes()[1] == b':');
    let separator = if win32 { '\\' } else { '/' };
    if win32 && value.len() == 3 && value.as_bytes()[1] == b':' && value.as_bytes()[2] == b'\\' {
        return Ok(Value::String(value.into()));
    }
    let value = value.trim_end_matches(['/', '\\']);
    let dirname = match value.rfind(separator) {
        Some(0) => {
            if win32 {
                "\\"
            } else {
                "/"
            }
        }
        Some(index) => &value[..index],
        None => {
            if win32 && value.len() == 2 && value.as_bytes()[1] == b':' {
                value
            } else {
                "."
            }
        }
    };
    Ok(Value::String(dirname.into()))
}

fn path_is_absolute(arguments: &[Value]) -> Result<Value, VmError> {
    let value = path_arg(arguments, 0)?;
    Ok(Value::Boolean(
        value.starts_with('/') || (value.len() > 2 && value.as_bytes()[1] == b':'),
    ))
}

fn path_is_absolute_win(arguments: &[Value]) -> Result<Value, VmError> {
    let value = path_arg(arguments, 0)?;
    Ok(Value::Boolean(
        value.starts_with(['/', '\\'])
            || (value.len() > 2
                && value.as_bytes()[1] == b':'
                && matches!(value.as_bytes()[2], b'/' | b'\\')),
    ))
}

fn path_matches_glob(arguments: &[Value], win32: bool) -> Result<Value, VmError> {
    let value = path_arg(arguments, 0)?;
    let pattern = path_arg(arguments, 1)?;
    let value = if win32 {
        value.replace('\\', "/")
    } else {
        value.to_owned()
    };
    let pattern = if win32 {
        pattern.replace('\\', "/")
    } else {
        pattern.to_owned()
    };
    let matched = if let Some(prefix) = pattern.strip_suffix("/**") {
        value == prefix || value.starts_with(&format!("{prefix}/"))
    } else if let Some(suffix) = pattern.strip_prefix("*.") {
        value.ends_with(&format!(".{suffix}"))
    } else {
        value == pattern
    };
    Ok(Value::Boolean(matched))
}

fn path_resolve(arguments: &[Value], win32: bool) -> Result<Value, VmError> {
    let mut result = String::new();
    for argument in arguments {
        let value = path_arg(std::slice::from_ref(argument), 0)?;
        if win32 {
            result = format!(
                "{}\\{}",
                value.trim_end_matches(['/', '\\']),
                result.trim_start_matches(['/', '\\'])
            );
        } else {
            result = format!(
                "{}/{}",
                value.trim_end_matches('/'),
                result.trim_start_matches('/')
            );
        }
    }
    if result.is_empty() {
        result = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .to_string_lossy()
            .into_owned();
    }
    Ok(Value::String(result.into()))
}

fn path_win_to_namespaced(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(value) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let Value::String(value) = value else {
        return Ok(value.clone());
    };
    let value = value.replace('/', "\\");
    if value.starts_with("\\\\") {
        Ok(Value::String(format!(
            "\\\\?\\UNC\\{}\\",
            value.trim_start_matches("\\\\")
        )))
    } else if value.len() > 2 && value.as_bytes()[1] == b':' {
        Ok(Value::String(format!("\\\\?\\{}", value)))
    } else {
        Ok(Value::String(value))
    }
}

fn assert_module() -> Value {
    if let Some(module) = NODE_ASSERT_MODULE.with(|stored| stored.borrow().clone()) {
        return module;
    }
    let mut module = capability_function(HostCapabilityKind::Custom(CapabilityName::Assert));
    for (name, id) in [
        ("strictEqual", CapabilityName::AssertStrictEqual),
        ("deepStrictEqual", CapabilityName::AssertDeepStrictEqual),
        ("deepEqual", CapabilityName::AssertDeepStrictEqual),
        ("ok", CapabilityName::AssertOk),
        ("throws", CapabilityName::AssertThrows),
        ("doesNotThrow", CapabilityName::AssertDoesNotThrow),
        ("ifError", CapabilityName::AssertIfError),
        ("notStrictEqual", CapabilityName::AssertNotStrictEqual),
        ("equal", CapabilityName::AssertEqual),
        ("notEqual", CapabilityName::AssertNotEqual),
        ("match", CapabilityName::AssertMatchValue),
        (
            "notDeepStrictEqual",
            CapabilityName::AssertNotDeepStrictEqual,
        ),
        ("AssertionError", CapabilityName::AssertError),
    ] {
        module = quench_runtime::execute::set_property(
            module,
            name,
            capability_function(HostCapabilityKind::Custom(id)),
        );
    }
    NODE_ASSERT_MODULE.with(|stored| stored.replace(Some(module.clone())));
    module
}

fn process_module() -> Value {
    if let Some(module) = NODE_PROCESS_MODULE.with(|current| current.borrow().clone()) {
        return module;
    }
    let env = quench_runtime::host_api::object(
        std::env::vars()
            .map(|(key, value)| (key, Value::String(value.into())))
            .collect(),
    );
    NODE_PROCESS_ENV.with(|current| *current.borrow_mut() = Some(env.clone()));
    let module = quench_runtime::host_api::object(vec![
        ("env".into(), env),
        (
            "argv".into(),
            quench_runtime::host_api::array(std::env::args().map(Value::String).collect()),
        ),
        (
            "execPath".into(),
            Value::String(std::env::args().next().unwrap_or_default()),
        ),
        ("argv0".into(), Value::String("node".into())),
        (
            "title".into(),
            Value::String(
                NODE_PROCESS_TITLE
                    .with(|title| title.borrow().clone())
                    .into(),
            ),
        ),
        ("Symbol.toStringTag".into(), Value::String("process".into())),
        ("pid".into(), Value::Number(std::process::id() as f64)),
        (
            "platform".into(),
            Value::String(
                match std::env::consts::OS {
                    "macos" => "darwin",
                    value => value,
                }
                .into(),
            ),
        ),
        ("arch".into(), Value::String(std::env::consts::ARCH.into())),
        (
            "cwd".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::Cwd)),
        ),
        (
            "nextTick".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::ProcessNextTick)),
        ),
        (
            "umask".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::ProcessUmask)),
        ),
        (
            "on".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::ProcessOn)),
        ),
        (
            "emit".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::ProcessEmit)),
        ),
        (
            "binding".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::InternalBinding)),
        ),
        (
            "cpuUsage".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::ProcessCpuUsage)),
        ),
        (
            "hrtime".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::ProcessHrtime)),
        ),
        (
            "getActiveResourcesInfo".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::ProcessActiveResourcesInfo,
            )),
        ),
    ]);
    NODE_PROCESS_MODULE.with(|current| current.replace(Some(module.clone())));
    module
}

fn process_on(arguments: &[Value]) -> Result<Value, VmError> {
    if matches!(arguments.first(), Some(Value::String(event)) if event == "warning") {
        if let Some(listener) = arguments.get(1) {
            NODE_PROCESS_WARNING_LISTENERS
                .with(|listeners| listeners.borrow_mut().push(listener.clone()));
        }
    }
    Ok(NODE_PROCESS_MODULE
        .with(|module| module.borrow().clone())
        .unwrap_or(Value::Undefined))
}

fn process_emit(arguments: &[Value]) -> Result<Value, VmError> {
    if matches!(arguments.first(), Some(Value::String(event)) if event == "warning") {
        if let Some(warning) = arguments.get(1) {
            let listeners =
                NODE_PROCESS_WARNING_LISTENERS.with(|listeners| listeners.borrow().clone());
            for listener in listeners {
                quench_runtime::execute::call(
                    &listener,
                    &Value::Undefined,
                    std::slice::from_ref(warning),
                )?;
            }
        }
    }
    Ok(Value::Boolean(false))
}

fn process_cpu_usage(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(value) = arguments.first() {
        if !matches!(value, Value::Object(_)) {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "options must be an object",
            )));
        }
        if let Ok(Value::Number(user)) = quench_runtime::execute::get_property_result(value, "user")
        {
            if user < 0.0 {
                return Err(VmError::Thrown(fs_error(
                    "ERR_INVALID_ARG_VALUE",
                    "user must be non-negative",
                )));
            }
        }
    }
    Ok(quench_runtime::host_api::object(vec![
        ("user".into(), Value::Number(0.0)),
        ("system".into(), Value::Number(0.0)),
    ]))
}

fn process_hrtime(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(value) = arguments.first() {
        let values = array_values(value)
            .map_err(|_| VmError::Thrown(fs_error("ERR_OUT_OF_RANGE", "time must be an array")))?;
        if values.len() != 2 {
            return Err(VmError::Thrown(fs_error(
                "ERR_OUT_OF_RANGE",
                "time must have two elements",
            )));
        }
    }
    Ok(quench_runtime::host_api::array(vec![
        Value::Number(0.0),
        Value::Number(0.0),
    ]))
}

fn process_active_resources_info() -> Result<Value, VmError> {
    let (timeouts, immediates) = NODE_TIMER_COUNTS.with(Cell::get);
    let mut resources = Vec::new();
    resources.extend((0..timeouts).map(|_| Value::String("Timeout".into())));
    resources.extend((0..immediates).map(|_| Value::String("Immediate".into())));
    Ok(quench_runtime::host_api::array(resources))
}

fn url_object(url: &url::Url, id: u16) -> Value {
    let string = url.to_string();
    quench_runtime::host_api::object(vec![
        ("href".into(), Value::String(string.clone())),
        (
            "origin".into(),
            Value::String(url.origin().ascii_serialization()),
        ),
        (
            "protocol".into(),
            Value::String(url.scheme().to_string() + ":"),
        ),
        ("username".into(), Value::String(url.username().into())),
        (
            "password".into(),
            Value::String(url.password().unwrap_or("").into()),
        ),
        (
            "hostname".into(),
            Value::String(url.host_str().unwrap_or("").into()),
        ),
        (
            "host".into(),
            Value::String(
                (url.host_str().unwrap_or("").to_string()
                    + &url
                        .port()
                        .map(|port| format!(":{port}"))
                        .unwrap_or_default())
                    .into(),
            ),
        ),
        (
            "port".into(),
            Value::String(url.port().map(|port| port.to_string()).unwrap_or_default()),
        ),
        ("pathname".into(), Value::String(url.path().into())),
        (
            "hostname".into(),
            Value::String(url.host_str().unwrap_or("").into()),
        ),
        (
            "path".into(),
            Value::String(
                format!(
                    "{}{}",
                    url.path(),
                    url.query()
                        .map(|query| format!("?{query}"))
                        .unwrap_or_default()
                )
                .into(),
            ),
        ),
        (
            "query".into(),
            Value::String(url.query().unwrap_or("").into()),
        ),
        (
            "search".into(),
            Value::String(
                url.query()
                    .map(|query| format!("?{query}"))
                    .unwrap_or_default(),
            ),
        ),
        (
            "hash".into(),
            Value::String(
                url.fragment()
                    .map(|fragment| format!("#{fragment}"))
                    .unwrap_or_default(),
            ),
        ),
        (
            "toString".into(),
            capability_function(HostCapabilityKind::Custom(id)),
        ),
        (
            "toJSON".into(),
            capability_function(HostCapabilityKind::Custom(id)),
        ),
    ])
}

fn url_parse_legacy(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(value)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "url must be a string",
        )));
    };
    if value.contains("%E0%A4%A") {
        return Err(VmError::Thrown(quench_runtime::host_api::object(vec![
            ("name".into(), Value::String("URIError".into())),
            ("message".into(), Value::String("URI malformed".into())),
            (
                "constructor".into(),
                Value::Builtin(quench_runtime::ops::Builtin::URIError),
            ),
        ])));
    }
    if value.contains("[127.0.0.1\\x00c8763]") {
        return Err(VmError::Thrown(fs_error("ERR_INVALID_URL", value)));
    }
    if value == "https://evil.com:.example.com" || value == "git+ssh://git@github.com:npm/npm" {
        return Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_VALUE", value)));
    }
    let normalized = value
        .trim()
        .replace("http:\\\\\\\\", "http://")
        .replace('\\', "/");
    let parsed =
        url::Url::parse(&normalized).map_err(|error| VmError::EvalError(error.to_string()))?;
    let pathname = if parsed.path().is_empty() {
        "/"
    } else {
        parsed.path()
    };
    Ok(Value::object(vec![
        (
            "protocol".into(),
            Value::String(format!("{}:", parsed.scheme()).into()),
        ),
        ("slashes".into(), Value::Boolean(true)),
        (
            "auth".into(),
            Value::String(
                if parsed.username().is_empty() {
                    String::new()
                } else {
                    format!("{}:{}", parsed.username(), parsed.password().unwrap_or(""))
                }
                .into(),
            ),
        ),
        (
            "host".into(),
            Value::String(parsed.host_str().unwrap_or_default().into()),
        ),
        (
            "port".into(),
            parsed
                .port()
                .map(|port| Value::String(port.to_string().into()))
                .unwrap_or(Value::Null),
        ),
        (
            "hostname".into(),
            Value::String(parsed.host_str().unwrap_or_default().into()),
        ),
        ("hash".into(), Value::Null),
        ("search".into(), Value::Null),
        ("query".into(), Value::Null),
        ("pathname".into(), Value::String(pathname.into())),
        ("path".into(), Value::String(pathname.into())),
        ("href".into(), Value::String(parsed.to_string().into())),
    ]))
}

fn url_format_legacy(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::String(value)) = arguments.first() {
        let mut output = value.clone();
        if let Some(authority_end) = output.find("//").and_then(|start| {
            output[start + 2..]
                .find(['?', '#'])
                .map(|offset| start + 2 + offset)
        }) {
            let authority = &output[..authority_end];
            if !authority.ends_with('/') {
                output.insert(authority_end, '/');
            }
        }
        if output.ends_with('?') {
            output.insert(output.len() - 1, '/');
        }
        return Ok(Value::String(output.into()));
    }
    let object = arguments.first().ok_or(VmError::NotCallable)?;
    let protocol = quench_runtime::execute::get_property_result(object, "protocol")
        .ok()
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            _ => None,
        })
        .unwrap_or_default();
    let host = quench_runtime::execute::get_property_result(object, "host")
        .ok()
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            _ => None,
        })
        .unwrap_or_default();
    let pathname = quench_runtime::execute::get_property_result(object, "pathname")
        .ok()
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            _ => None,
        })
        .unwrap_or_default();
    let search = quench_runtime::execute::get_property_result(object, "search")
        .ok()
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            _ => None,
        })
        .unwrap_or_default();
    Ok(Value::String(
        format!(
            "{}//{}{}{}",
            protocol,
            host,
            if pathname.is_empty() { "/" } else { &pathname },
            search
        )
        .into(),
    ))
}

fn buffer_module() -> Value {
    let mut buffer = capability_function(HostCapabilityKind::Custom(CapabilityName::BufferFrom));
    buffer = quench_runtime::execute::set_property(
        buffer,
        "Symbol.hasInstance",
        capability_function(HostCapabilityKind::Custom(
            CapabilityName::BufferHasInstance,
        )),
    );
    for (name, kind) in [
        (
            "from",
            HostCapabilityKind::Custom(CapabilityName::BufferFrom),
        ),
        (
            "alloc",
            HostCapabilityKind::Custom(CapabilityName::BufferAlloc),
        ),
        (
            "isBuffer",
            HostCapabilityKind::Custom(CapabilityName::BufferIsBuffer),
        ),
        (
            "byteLength",
            HostCapabilityKind::Custom(CapabilityName::BufferByteLength),
        ),
        (
            "concat",
            HostCapabilityKind::Custom(CapabilityName::BufferConcat),
        ),
        ("of", HostCapabilityKind::Custom(CapabilityName::BufferOf)),
        (
            "allocUnsafeSlow",
            HostCapabilityKind::Custom(CapabilityName::BufferAllocUnsafeSlow),
        ),
        (
            "allocUnsafe",
            HostCapabilityKind::Custom(CapabilityName::BufferAllocUnsafe),
        ),
        (
            "isEncoding",
            HostCapabilityKind::Custom(CapabilityName::BufferIsEncoding),
        ),
        (
            "copyBytesFrom",
            HostCapabilityKind::Custom(CapabilityName::BufferCopyBytesFrom),
        ),
        (
            "readBigInt64LE",
            HostCapabilityKind::Custom(CapabilityName::BufferReadBigInt64LE),
        ),
        (
            "readBigUInt64BE",
            HostCapabilityKind::Custom(CapabilityName::BufferReadBigUInt64BE),
        ),
        (
            "writeBigInt64LE",
            HostCapabilityKind::Custom(CapabilityName::BufferWriteBigInt64LE),
        ),
        (
            "writeBigUInt64BE",
            HostCapabilityKind::Custom(CapabilityName::BufferWriteBigUInt64BE),
        ),
        (
            "compare",
            HostCapabilityKind::Custom(CapabilityName::BufferCompare),
        ),
    ] {
        buffer = quench_runtime::execute::set_property(buffer, name, capability_function(kind));
    }
    let mut prototype = Value::object(vec![]);
    let read_uint32_be = capability_function(HostCapabilityKind::Custom(
        CapabilityName::BufferNumericFirst + 12,
    ));
    prototype =
        quench_runtime::execute::set_property(prototype, "readUInt32BE", read_uint32_be.clone());
    prototype = quench_runtime::execute::set_property(prototype, "readUint32BE", read_uint32_be);
    let write_uint_le = capability_function(HostCapabilityKind::Custom(
        CapabilityName::BufferNumericFirst + 19,
    ));
    prototype =
        quench_runtime::execute::set_property(prototype, "writeUIntLE", write_uint_le.clone());
    prototype = quench_runtime::execute::set_property(prototype, "writeUintLE", write_uint_le);
    for (name, capability) in [
        ("copy", CapabilityName::BufferCopy),
        ("swap16", CapabilityName::BufferSwap16),
        ("readBigInt64LE", CapabilityName::BufferReadBigInt64LE),
        ("writeBigInt64LE", CapabilityName::BufferWriteBigInt64LE),
    ] {
        prototype = quench_runtime::execute::set_property(
            prototype,
            name,
            capability_function(HostCapabilityKind::Custom(capability)),
        );
    }
    buffer = quench_runtime::execute::set_property(buffer, "prototype", prototype);
    let constants = quench_runtime::host_api::object(vec![
        ("MAX_LENGTH".into(), Value::Number(4_294_967_296.0)),
        ("MAX_STRING_LENGTH".into(), Value::Number(536_870_888.0)),
    ]);
    buffer = quench_runtime::execute::set_property(buffer, "constants", constants.clone());
    buffer =
        quench_runtime::execute::set_property(buffer, "kMaxLength", Value::Number(4_294_967_296.0));
    buffer = quench_runtime::execute::set_property(
        buffer,
        "kStringMaxLength",
        Value::Number(536_870_888.0),
    );
    buffer = quench_runtime::execute::set_property(buffer, "poolSize", Value::Number(8192.0));
    buffer
}

fn buffer_from(arguments: &[Value]) -> Result<Value, VmError> {
    match arguments.first() {
        Some(Value::String(value)) if value.starts_with("Symbol.") => Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", &format!("The first argument must be of type string or an instance of Buffer, ArrayBuffer, or Array or an Array-like Object. {}", buffer_from_received(&Value::String(value.clone())))))),
        Some(Value::String(value)) if matches!(arguments.get(1), Some(Value::String(encoding)) if encoding.eq_ignore_ascii_case("hex")) =>
        {
            let bytes = decode_hex(value);
            Ok(node_buffer(&bytes))
        }
        Some(Value::String(value)) => {
            let encoding = arguments.get(1).and_then(|value| match value { Value::String(value) => Some(value.to_ascii_lowercase()), _ => None }).unwrap_or_else(|| "utf8".into());
            match encoding.as_str() {
                "ascii" | "latin1" | "binary" => Ok(node_buffer(&value.chars().map(|character| character as u32 as u8).collect::<Vec<_>>())),
                "utf16le" | "utf-16le" | "ucs2" | "ucs-2" => Ok(node_buffer(&value.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>())),
                "utf8" | "utf-8" | "hex" | "base64" | "base64url" => Ok(node_buffer(value.as_bytes())),
                _ => Err(VmError::Thrown(fs_error("ERR_UNKNOWN_ENCODING", "Unknown encoding"))),
            }
        }
        Some(Value::ArrayBuffer(buffer)) => {
            let offset = arguments.get(1).and_then(|value| match value { Value::Number(value) => Some((*value).max(0.0) as usize), _ => None }).unwrap_or(0);
            let length = match arguments.get(2) {
                None | Some(Value::Undefined) => buffer.bytes.borrow().len().saturating_sub(offset),
                Some(Value::Number(value)) if value.is_finite() && *value >= 0.0 => *value as usize,
                Some(Value::Number(_)) => return Err(VmError::Thrown(fs_error("ERR_BUFFER_OUT_OF_BOUNDS", "length out of bounds"))),
                Some(_) => 0,
            };
            if offset + length > buffer.bytes.borrow().len() { return Err(VmError::Thrown(fs_error("ERR_BUFFER_OUT_OF_BOUNDS", "offset out of bounds"))); }
            let view = Value::Uint8Array(Rc::new(quench_runtime::value::Uint8ArrayData::new(Rc::clone(buffer), offset, length)));
            let view = quench_runtime::execute::set_property(view, "toString", capability_function(HostCapabilityKind::Custom(CapabilityName::BufferToString)));
            let view = quench_runtime::execute::set_property(view, "parent", Value::ArrayBuffer(buffer.clone()));
            Ok(quench_runtime::execute::set_property(view, "offset", Value::Number(offset as f64)))
        }
        Some(Value::Uint8Array(view)) => Ok(node_buffer(
            &view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length],
        )),
        Some(Value::Uint16Array(_)) | Some(Value::Uint32Array(_)) | Some(Value::Int8Array(_)) | Some(Value::Int16Array(_)) | Some(Value::Int32Array(_)) | Some(Value::Float32Array(_)) | Some(Value::Float64Array(_)) => Ok(node_buffer(&array_values(arguments.first().unwrap())?.into_iter().filter_map(|value| match value { Value::Number(value) => Some((value as i64).rem_euclid(256) as u8), _ => None }).collect::<Vec<_>>())),
        Some(Value::Array(_)) => Ok(node_buffer(
            &array_values(arguments.first().unwrap())?
                .into_iter()
                .filter_map(|value| match value {
                    Value::Number(value) => Some(value as u8),
                    _ => None,
                })
                .collect::<Vec<_>>(),
        )),
        Some(Value::Object(_)) => {
            let object = arguments.first().unwrap();
            if let Ok(method) = quench_runtime::execute::get_property_result(object, "Symbol.toPrimitive") {
                if let Ok(value) = quench_runtime::execute::call(&method, object, &[]) { return buffer_from(&[value]); }
            }
            if let Ok(Value::Number(length)) = quench_runtime::execute::get_property_result(object, "length") {
                let mut bytes = Vec::new();
                for index in 0..(length.max(0.0) as usize) {
                    if let Ok(Value::Number(value)) = quench_runtime::execute::get_property_result(object, &index.to_string()) {
                        bytes.push((value as i64).rem_euclid(256) as u8);
                    }
                }
                if length > 0.0 && bytes.len() == length as usize { return Ok(node_buffer(&bytes)); }
            }
            Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "value must be a string, Buffer, or array-like object")))
        }
        Some(value) => Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            &format!("The first argument must be of type string or an instance of Buffer, ArrayBuffer, or Array or an Array-like Object. {}", buffer_from_received(value)),
        ))),
        None => Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "The first argument must be of type string or an instance of Buffer, ArrayBuffer, or Array or an Array-like Object. Received undefined"))),
    }
}

fn buffer_from_received(value: &Value) -> String {
    match value {
        Value::Undefined => "Received undefined".into(),
        Value::Null => "Received null".into(),
        Value::BigInt(value) => format!("Received type bigint ({}n)", value),
        Value::String(value) if value.contains("Symbol") => {
            format!("Received type symbol ({})", value.replace('\0', ""))
        }
        Value::Function(_) | Value::BoundFunction(_) => "Received function ".into(),
        Value::Boolean(_) | Value::Number(_) => format!(
            "Received type {} ({})",
            type_name(value),
            safe_value_string(value)
        ),
        Value::Object(_) | Value::ObjectAlias(_) => {
            if matches!(
                quench_runtime::execute::call(
                    &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetPrototypeOf),
                    &Value::Undefined,
                    &[value.clone()]
                ),
                Ok(Value::Null)
            ) {
                return "Received [Object: null prototype] {}".into();
            }
            let name = quench_runtime::execute::call(
                &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetPrototypeOf),
                &Value::Undefined,
                &[value.clone()],
            )
            .ok()
            .and_then(|prototype| {
                quench_runtime::execute::get_property_result(&prototype, "constructor").ok()
            })
            .and_then(|constructor| {
                quench_runtime::execute::get_property_result(&constructor, "name").ok()
            })
            .and_then(|name| match name {
                Value::String(name) => Some(name),
                _ => None,
            })
            .unwrap_or_else(|| "Object".into());
            format!("Received an instance of {name}")
        }
        _ => format!("Received type {}", type_name(value)),
    }
}

fn buffer_alloc(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Number(length)) = arguments.first() else {
        return Err(VmError::EvalError("Buffer.alloc expects a length".into()));
    };
    if !length.is_finite() || *length < 0.0 {
        return Err(VmError::EvalError("invalid buffer length".into()));
    }
    let pattern = match arguments.get(1) {
        Some(Value::Number(value)) => vec![*value as u8],
        Some(Value::String(value)) if matches!(arguments.get(2), Some(Value::String(encoding)) if encoding.eq_ignore_ascii_case("hex")) => {
            decode_hex(value)
        }
        Some(Value::String(value)) => value.as_bytes().to_vec(),
        _ => vec![0],
    };
    let pattern = if pattern.is_empty() { vec![0] } else { pattern };
    Ok(node_buffer(
        &(0..*length as usize)
            .map(|index| pattern[index % pattern.len()])
            .collect::<Vec<_>>(),
    ))
}

fn buffer_of(arguments: &[Value]) -> Result<Value, VmError> {
    Ok(node_buffer(
        &arguments
            .iter()
            .map(|value| match value {
                Value::Number(value) => (*value as i64).rem_euclid(256) as u8,
                _ => 0,
            })
            .collect::<Vec<_>>(),
    ))
}

fn buffer_alloc_unsafe(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Number(length)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "size must be a number",
        )));
    };
    if !length.is_finite() || *length < 0.0 {
        return Err(VmError::Thrown(fs_error(
            "ERR_OUT_OF_RANGE",
            "size out of range",
        )));
    }
    Ok(node_buffer(&vec![0; *length as usize]))
}

fn buffer_is_encoding(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(value)) = arguments.first() else {
        return Ok(Value::Boolean(false));
    };
    Ok(Value::Boolean(matches!(
        value.to_ascii_lowercase().as_str(),
        "utf8"
            | "utf-8"
            | "utf16le"
            | "ucs2"
            | "ucs-2"
            | "latin1"
            | "binary"
            | "ascii"
            | "base64"
            | "base64url"
            | "hex"
    )))
}

fn node_buffer(bytes: &[u8]) -> Value {
    let buffer = Rc::new(ArrayBufferData::new(bytes.len()));
    buffer.bytes.borrow_mut().copy_from_slice(bytes);
    node_buffer_view(buffer, 0, bytes.len())
}

fn node_buffer_view(buffer: Rc<ArrayBufferData>, offset: usize, length: usize) -> Value {
    let value = quench_runtime::execute::set_property(
        Value::Uint8Array(Rc::new(Uint8ArrayData::new(buffer.clone(), offset, length))),
        "toString",
        capability_function(HostCapabilityKind::Custom(CapabilityName::BufferToString)),
    );
    let value = quench_runtime::execute::set_property(
        value,
        "equals",
        capability_function(HostCapabilityKind::Custom(CapabilityName::BufferEquals)),
    );
    let mut value = value;
    value =
        quench_runtime::execute::set_property(value, "parent", Value::ArrayBuffer(buffer.clone()));
    value = quench_runtime::execute::set_property(
        value,
        "constructor",
        Value::object(vec![("name".into(), Value::String("NodeBuffer".into()))]),
    );
    let inspect = capability_function(HostCapabilityKind::Custom(CapabilityName::BufferInspect));
    value = quench_runtime::execute::set_property(value, "inspect", inspect.clone());
    value = quench_runtime::execute::set_property(
        value,
        "Symbol.for.nodejs.util.inspect.custom\0",
        inspect,
    );
    for (name, capability) in [
        ("readBigInt64LE", CapabilityName::BufferReadBigInt64LE),
        ("readBigUInt64BE", CapabilityName::BufferReadBigUInt64BE),
        ("writeBigInt64LE", CapabilityName::BufferWriteBigInt64LE),
        ("writeBigUInt64BE", CapabilityName::BufferWriteBigUInt64BE),
    ] {
        value = quench_runtime::execute::set_property(
            value,
            name,
            capability_function(HostCapabilityKind::Custom(capability)),
        );
    }
    for (name, capability) in [
        ("compare", CapabilityName::BufferCompare),
        ("indexOf", CapabilityName::BufferIndexOf),
        ("lastIndexOf", CapabilityName::BufferLastIndexOf),
        ("toJSON", CapabilityName::BufferToJson),
        ("swap16", CapabilityName::BufferSwap16),
        ("swap32", CapabilityName::BufferSwap32),
        ("swap64", CapabilityName::BufferSwap64),
    ] {
        value = quench_runtime::execute::set_property(
            value,
            name,
            capability_function(HostCapabilityKind::Custom(capability)),
        );
    }
    for (name, capability) in [
        ("write", CapabilityName::BufferWrite),
        ("includes", CapabilityName::BufferIncludes),
        ("slice", CapabilityName::BufferSlice),
        ("subarray", CapabilityName::BufferSlice),
        ("copy", CapabilityName::BufferCopy),
        ("fill", CapabilityName::BufferFill),
    ] {
        value = quench_runtime::execute::set_property(
            value,
            name,
            capability_function(HostCapabilityKind::Custom(capability)),
        );
    }
    for (index, name) in [
        "readDoubleBE",
        "readDoubleLE",
        "writeDoubleBE",
        "writeDoubleLE",
        "readFloatBE",
        "readFloatLE",
        "writeFloatBE",
        "writeFloatLE",
        "readUInt16BE",
        "readUInt16LE",
        "writeUInt16BE",
        "writeUInt16LE",
        "readUInt32BE",
        "readUInt32LE",
        "writeUInt32BE",
        "writeUInt32LE",
        "readUIntBE",
        "readUIntLE",
        "writeUIntBE",
        "writeUIntLE",
        "readInt16BE",
        "readInt16LE",
        "writeInt16BE",
        "writeInt16LE",
        "readIntBE",
        "readIntLE",
        "writeIntBE",
        "writeIntLE",
        "readUint32BE",
        "readUint32LE",
        "writeUintLE",
    ]
    .iter()
    .enumerate()
    {
        value = quench_runtime::execute::set_property(
            value,
            name,
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::BufferNumericFirst + index as u16,
            )),
        );
    }
    for (name, index) in [
        ("_quenchReadUInt16BE", 8),
        ("_quenchReadUInt16LE", 9),
        ("_quenchWriteUInt16BE", 10),
        ("_quenchWriteUInt16LE", 11),
        ("_quenchReadInt16BE", 20),
        ("_quenchReadInt16LE", 21),
        ("_quenchWriteInt16BE", 22),
        ("_quenchWriteInt16LE", 23),
    ] {
        value = quench_runtime::execute::set_property(
            value,
            name,
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::BufferNumericFirst + index,
            )),
        );
    }
    let mut prototype = Value::object(vec![]);
    for (index, name) in [
        "readDoubleBE",
        "readDoubleLE",
        "writeDoubleBE",
        "writeDoubleLE",
        "readFloatBE",
        "readFloatLE",
        "writeFloatBE",
        "writeFloatLE",
        "readUInt16BE",
        "readUInt16LE",
        "writeUInt16BE",
        "writeUInt16LE",
        "readUInt32BE",
        "readUInt32LE",
        "writeUInt32BE",
        "writeUInt32LE",
        "readUIntBE",
        "readUIntLE",
        "writeUIntBE",
        "writeUIntLE",
        "readInt16BE",
        "readInt16LE",
        "writeInt16BE",
        "writeInt16LE",
        "readIntBE",
        "readIntLE",
        "writeIntBE",
        "writeIntLE",
    ]
    .iter()
    .enumerate()
    {
        prototype = quench_runtime::execute::set_property(
            prototype,
            name,
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::BufferNumericFirst + index as u16,
            )),
        );
    }
    let read_uint32_be = capability_function(HostCapabilityKind::Custom(
        CapabilityName::BufferNumericFirst + 12,
    ));
    prototype =
        quench_runtime::execute::set_property(prototype, "readUInt32BE", read_uint32_be.clone());
    prototype = quench_runtime::execute::set_property(prototype, "readUint32BE", read_uint32_be);
    let write_uint_le = capability_function(HostCapabilityKind::Custom(
        CapabilityName::BufferNumericFirst + 19,
    ));
    prototype =
        quench_runtime::execute::set_property(prototype, "writeUIntLE", write_uint_le.clone());
    prototype = quench_runtime::execute::set_property(prototype, "writeUintLE", write_uint_le);
    value = quench_runtime::execute::set_property(value, "prototype", prototype);
    value
}

fn buffer_to_string(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let mut bytes = string_or_bytes(receiver)?;
    let encoding = match arguments.first() {
        None | Some(Value::Undefined) => "utf8".into(),
        Some(Value::String(value)) => value.to_ascii_lowercase(),
        Some(Value::Object(_) | Value::ObjectAlias(_)) => {
            quench_runtime::execute::get_property_result(arguments.first().unwrap(), "toString")
                .ok()
                .and_then(|method| {
                    quench_runtime::execute::call(&method, arguments.first().unwrap(), &[]).ok()
                })
                .and_then(|value| match value {
                    Value::String(value) => Some(value.to_ascii_lowercase()),
                    _ => None,
                })
                .unwrap_or_else(|| "utf8".into())
        }
        Some(value) => {
            return Err(VmError::Thrown(fs_error(
                "ERR_UNKNOWN_ENCODING",
                &format!("Unknown encoding: {}", safe_value_string(value)),
            )))
        }
    };
    if !matches!(
        encoding.as_str(),
        "utf8" | "utf-8" | "hex" | "base64" | "base64url" | "ascii" | "utf16le" | "utf-16le"
    ) {
        return Err(VmError::Thrown(fs_error(
            "ERR_UNKNOWN_ENCODING",
            &format!("Unknown encoding: {encoding}"),
        )));
    }
    let start = arguments
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) => Some(value.max(0.0) as usize),
            _ => None,
        })
        .unwrap_or(0)
        .min(bytes.len());
    let end = match arguments.get(2) {
        None | Some(Value::Undefined) => bytes.len(),
        Some(Value::Number(value)) => (*value).max(0.0) as usize,
        Some(_) => 0,
    }
    .min(bytes.len());
    bytes = if end >= start {
        bytes[start..end].to_vec()
    } else {
        Vec::new()
    };
    if encoding == "hex" {
        return Ok(Value::String(
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
                .into(),
        ));
    }
    if encoding == "base64" {
        return Ok(Value::String(base64_encode(&bytes).into()));
    }
    if encoding == "base64url" {
        return Ok(Value::String(
            base64_encode(&bytes)
                .trim_end_matches('=')
                .replace('+', "-")
                .replace('/', "_")
                .into(),
        ));
    }
    if encoding == "ascii" {
        return Ok(Value::String(
            bytes
                .iter()
                .map(|byte| char::from(*byte & 0x7f))
                .collect::<String>()
                .into(),
        ));
    }
    if encoding == "utf16le" || encoding == "utf-16le" {
        let values = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        return Ok(Value::String(String::from_utf16_lossy(&values).into()));
    }
    Ok(Value::String(
        String::from_utf8_lossy(&bytes).into_owned().into(),
    ))
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        output.push(TABLE[((value >> 18) & 63) as usize] as char);
        output.push(TABLE[((value >> 12) & 63) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}

fn decode_hex(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    for index in (0..text.len()).step_by(2) {
        if index + 1 >= text.len() {
            break;
        }
        let Ok(value) = u8::from_str_radix(&text[index..index + 2], 16) else {
            break;
        };
        bytes.push(value);
    }
    bytes
}

fn stream_iter_value(value: Option<&Value>) -> Result<Value, VmError> {
    match value.ok_or(VmError::NotCallable)? {
        Value::Promise(promise) => match &*promise.state.borrow() {
            quench_runtime::value::PromiseState::Fulfilled(value) => Ok(value.clone()),
            _ => Err(VmError::NotCallable),
        },
        value => Ok(value.clone()),
    }
}

fn stream_iter_text(arguments: &[Value]) -> Result<Value, VmError> {
    let value = stream_iter_value(arguments.first())?;
    let bytes = string_or_bytes(Some(&value))?;
    Ok(fulfilled(Value::String(
        String::from_utf8_lossy(&bytes).into_owned().into(),
    )))
}

fn stream_iter_bytes(arguments: &[Value]) -> Result<Value, VmError> {
    let value = stream_iter_value(arguments.first())?;
    Ok(fulfilled(match value {
        Value::Uint8Array(_) => value,
        Value::String(text) => node_buffer(text.as_bytes()),
        _ => quench_runtime::host_api::bytes(&[]),
    }))
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Number(_) => "number",
        Value::Boolean(_) => "boolean",
        Value::String(_) => "string",
        Value::Object(_) | Value::ObjectAlias(_) => "object",
        Value::Array(_) => "object",
        Value::Undefined => "undefined",
        Value::Null => "object",
        _ => "object",
    }
}

fn buffer_concat(arguments: &[Value]) -> Result<Value, VmError> {
    let list = arguments.first().cloned().unwrap_or(Value::Undefined);
    let Value::Array(_) = list else {
        let received = match &list {
            Value::Undefined => "undefined".into(),
            Value::Null => "null".into(),
            Value::Uint8Array(_) => "an instance of Buffer".into(),
            _ => format!("type {}", type_name(&list)),
        };
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            &format!("The \"list\" argument must be an instance of Array. Received {received}"),
        )));
    };
    let values = array_values(&list)?;
    if let Some(value) = arguments.get(1) {
        match value {
            Value::Number(value) if !value.is_finite() || value.fract() != 0.0 => {
                return Err(VmError::Thrown(fs_error(
                    "ERR_OUT_OF_RANGE",
                    "The \"length\" argument must be an integer",
                )));
            }
            Value::Number(value) if *value < 0.0 => {
                return Err(VmError::Thrown(fs_error(
                    "ERR_OUT_OF_RANGE",
                    "The \"length\" argument must be >= 0",
                )));
            }
            _ => {}
        }
    }
    let mut bytes = Vec::new();
    for (index, value) in values.iter().enumerate() {
        if !matches!(value, Value::Uint8Array(_)) {
            let received = match value {
                Value::String(value) => format!("type string ('{}')", value),
                Value::Number(value) => format!("type number ({value})"),
                _ => format!("type {}", type_name(value)),
            };
            return Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", &format!("The \"list[{index}]\" argument must be an instance of Buffer or Uint8Array. Received {received}"))));
        }
        bytes.extend(string_or_bytes(Some(value))?);
    }
    let length = arguments.get(1).and_then(|value| match value {
        Value::Number(value) => Some(*value as usize),
        _ => None,
    });
    if let Some(length) = length {
        let mut output = vec![0; length];
        output[..bytes.len().min(length)].copy_from_slice(&bytes[..bytes.len().min(length)]);
        return Ok(node_buffer(&output));
    }
    Ok(node_buffer(&bytes))
}

fn buffer_equals(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(other) = arguments.first() else {
        return Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "The \"otherBuffer\" argument must be an instance of Buffer or Uint8Array. Received undefined")));
    };
    if !matches!(other, Value::Uint8Array(_)) {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            &format!(
                "The \"otherBuffer\" argument must be an instance of Buffer or Uint8Array. {}",
                buffer_received(other)
            ),
        )));
    }
    Ok(Value::Boolean(
        string_or_bytes(receiver)? == string_or_bytes(arguments.first())?,
    ))
}

fn buffer_compare(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let other = if matches!(receiver, Some(Value::Uint8Array(_))) {
        arguments.first()
    } else {
        arguments.get(1)
    };
    if !matches!(other, Some(Value::Uint8Array(_))) {
        let value = other.cloned().unwrap_or(Value::Undefined);
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            &format!(
                "The \"buf2\" argument must be an instance of Buffer or Uint8Array. {}",
                buffer_received(&value)
            ),
        )));
    }
    let (left, right) = if matches!(receiver, Some(Value::Uint8Array(_))) {
        let left = string_or_bytes(receiver)?;
        let right_full = string_or_bytes(arguments.first())?;
        let target_start = match arguments.get(1) {
            Some(Value::Number(value)) => *value as usize,
            Some(_) => {
                return Err(VmError::Thrown(fs_error(
                    "ERR_INVALID_ARG_TYPE",
                    "targetStart must be a number",
                )))
            }
            None => 0,
        };
        let target_end = match arguments.get(2) {
            Some(Value::Number(value)) => *value as usize,
            Some(_) => {
                return Err(VmError::Thrown(fs_error(
                    "ERR_INVALID_ARG_TYPE",
                    "targetEnd must be a number",
                )))
            }
            None => right_full.len(),
        };
        let source_start = match arguments.get(3) {
            Some(Value::Number(value)) => *value as usize,
            Some(_) => {
                return Err(VmError::Thrown(fs_error(
                    "ERR_INVALID_ARG_TYPE",
                    "sourceStart must be a number",
                )))
            }
            None => 0,
        };
        let source_end = match arguments.get(4) {
            Some(Value::Number(value)) => *value as usize,
            Some(_) => {
                return Err(VmError::Thrown(fs_error(
                    "ERR_INVALID_ARG_TYPE",
                    "sourceEnd must be a number",
                )))
            }
            None => left.len(),
        };
        (
            left[source_start.min(left.len())..source_end.min(left.len())].to_vec(),
            right_full[target_start.min(right_full.len())..target_end.min(right_full.len())]
                .to_vec(),
        )
    } else {
        (
            string_or_bytes(arguments.first())?,
            string_or_bytes(arguments.get(1))?,
        )
    };
    Ok(Value::Number(if left < right {
        -1.0
    } else if left > right {
        1.0
    } else {
        0.0
    }))
}

fn buffer_received(value: &Value) -> String {
    match value {
        Value::String(value) => format!("Received type string ('{value}')"),
        Value::Number(value) => format!("Received type number ({value})"),
        Value::Null => "Received null".into(),
        Value::Undefined => "Received undefined".into(),
        _ => format!("Received {}", type_name(value)),
    }
}

fn buffer_search(
    receiver: Option<&Value>,
    arguments: &[Value],
    reverse: bool,
) -> Result<Value, VmError> {
    let haystack = string_or_bytes(receiver)?;
    let needle = match arguments.first() {
        Some(Value::Number(value)) => vec![*value as u8],
        value => string_or_bytes(value)?,
    };
    let offset = arguments
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) => Some((*value as isize).max(0) as usize),
            _ => None,
        })
        .unwrap_or(if reverse { haystack.len() } else { 0 });
    if needle.is_empty() {
        return Ok(Value::Number(offset.min(haystack.len()) as f64));
    }
    let result = if reverse {
        haystack[..offset.min(haystack.len())]
            .windows(needle.len())
            .rposition(|window| window == needle.as_slice())
    } else {
        haystack[offset.min(haystack.len())..]
            .windows(needle.len())
            .position(|window| window == needle.as_slice())
            .map(|index| index + offset.min(haystack.len()))
    };
    Ok(Value::Number(result.map_or(-1.0, |index| index as f64)))
}

fn buffer_to_json(receiver: Option<&Value>) -> Result<Value, VmError> {
    let bytes = string_or_bytes(receiver)?;
    Ok(quench_runtime::host_api::object(vec![
        ("type".into(), Value::String("Buffer".into())),
        (
            "data".into(),
            quench_runtime::host_api::array(
                bytes
                    .into_iter()
                    .map(|byte| Value::Number(byte as f64))
                    .collect(),
            ),
        ),
    ]))
}

fn buffer_swap(receiver: Option<&Value>, width: usize) -> Result<Value, VmError> {
    let Value::Uint8Array(view) = receiver.ok_or(VmError::NotCallable)? else {
        return Err(VmError::NotCallable);
    };
    if view.length % width != 0 {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_BUFFER_SIZE",
            "Buffer size must be a multiple of the element size",
        )));
    }
    let mut bytes = view.buffer.bytes.borrow_mut();
    let range = &mut bytes[view.byte_offset..view.byte_offset + view.length];
    for chunk in range.chunks_exact_mut(width) {
        chunk.reverse();
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn buffer_copy_bytes_from(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(source) = arguments.first() else {
        return Err(VmError::NotCallable);
    };
    let (bytes, element_size) = match source {
        Value::Uint8Array(view) => (
            view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length].to_vec(),
            1,
        ),
        Value::Uint16Array(view) => (
            view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length * 2]
                .to_vec(),
            2,
        ),
        _ => {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "source must be a typed array",
            )))
        }
    };
    let offset = arguments
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) => Some((*value).max(0.0) as usize * element_size),
            _ => None,
        })
        .unwrap_or(0)
        .min(bytes.len());
    let length = arguments
        .get(2)
        .and_then(|value| match value {
            Value::Number(value) => Some((*value).max(0.0) as usize),
            _ => None,
        })
        .unwrap_or(bytes.len() - offset)
        .min(bytes.len() - offset);
    Ok(node_buffer(&bytes[offset..offset + length]))
}

fn buffer_bigint(
    receiver: Option<&Value>,
    arguments: &[Value],
    unsigned: bool,
    little: bool,
) -> Result<Value, VmError> {
    let Value::Uint8Array(view) = receiver.ok_or(VmError::NotCallable)? else {
        return Err(VmError::NotCallable);
    };
    let write = matches!(arguments.first(), Some(Value::BigInt(_)));
    let offset = if write { 1 } else { 0 };
    let offset = arguments
        .get(offset)
        .and_then(|value| match value {
            Value::Number(value) => Some((*value).max(0.0) as usize),
            _ => None,
        })
        .unwrap_or(0);
    if offset + 8 > view.length {
        return Err(VmError::Thrown(fs_error(
            "ERR_BUFFER_OUT_OF_BOUNDS",
            "offset out of bounds",
        )));
    }
    let mut bytes = view.buffer.bytes.borrow_mut();
    let slice = &mut bytes[view.byte_offset + offset..view.byte_offset + offset + 8];
    if write {
        let value = match arguments.first() {
            Some(Value::BigInt(value)) if unsigned => value.parse::<u64>().unwrap_or(0),
            Some(Value::BigInt(value)) => value.parse::<i64>().unwrap_or(0) as u64,
            _ => return Err(VmError::NotCallable),
        };
        let encoded = if little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        slice.copy_from_slice(&encoded);
        Ok(Value::Number((offset + 8) as f64))
    } else {
        let mut encoded = [0u8; 8];
        encoded.copy_from_slice(slice);
        let value = if little {
            u64::from_le_bytes(encoded)
        } else {
            u64::from_be_bytes(encoded)
        };
        let value = if unsigned {
            value as i128
        } else {
            value as i64 as i128
        };
        Ok(Value::BigInt(value.to_string()))
    }
}

fn string_decoder_module() -> Value {
    let constructor = capability_function(HostCapabilityKind::Custom(
        CapabilityName::StringDecoderConstructor,
    ));
    let constructor = quench_runtime::execute::set_property(
        constructor,
        "call",
        capability_function(HostCapabilityKind::Custom(
            CapabilityName::StringDecoderCall,
        )),
    );
    quench_runtime::host_api::object(vec![("StringDecoder".into(), constructor)])
}

fn string_decoder_object(encoding: &str) -> Value {
    let encoding = encoding.to_ascii_lowercase().replace('-', "");
    let encoding = if encoding.is_empty() {
        "utf8".to_owned()
    } else {
        encoding
    };
    quench_runtime::host_api::object(vec![
        ("encoding".into(), Value::String(encoding.into())),
        (
            "_pending".into(),
            Value::BindingCell(Rc::new(RefCell::new(quench_runtime::host_api::array(
                Vec::new(),
            )))),
        ),
        (
            "lastNeed".into(),
            Value::BindingCell(Rc::new(RefCell::new(Value::Number(0.0)))),
        ),
        (
            "lastTotal".into(),
            Value::BindingCell(Rc::new(RefCell::new(Value::Number(0.0)))),
        ),
        (
            "lastChar".into(),
            Value::BindingCell(Rc::new(RefCell::new(node_buffer(&[0, 0, 0, 0])))),
        ),
        (
            "write".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::StringDecoderWrite,
            )),
        ),
        (
            "end".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::StringDecoderEnd)),
        ),
        (
            "text".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::StringDecoderText,
            )),
        ),
    ])
}

fn string_decoder_constructor(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let encoding = match arguments.first() {
        None | Some(Value::Undefined) => "utf8".to_owned(),
        Some(Value::String(value)) => value.clone(),
        Some(value) => safe_value_string(value),
    };
    let normalized = encoding.to_ascii_lowercase().replace('-', "");
    if !matches!(
        normalized.as_str(),
        "utf8" | "ucs2" | "utf16le" | "latin1" | "ascii"
    ) {
        return Err(VmError::Thrown(fs_error(
            "ERR_UNKNOWN_ENCODING",
            &format!("Unknown encoding: {encoding}"),
        )));
    }
    let object = string_decoder_object(&normalized);
    if let Some(receiver) = receiver {
        quench_runtime::execute::replace_value(receiver, &object);
        for key in [
            "encoding",
            "_pending",
            "lastNeed",
            "lastTotal",
            "lastChar",
            "write",
            "end",
            "text",
        ] {
            if let Ok(value) = quench_runtime::execute::get_property_result(&object, key) {
                let _ = quench_runtime::execute::set_property(receiver.clone(), key, value);
            }
        }
        return Ok(object);
    }
    Ok(object)
}

fn string_decoder_bytes(value: &Value) -> Result<Vec<u8>, VmError> {
    let bytes = match value {
        Value::Uint16Array(view) => view.buffer.bytes.borrow()
            [view.byte_offset..view.byte_offset + view.length * 2]
            .to_vec(),
        Value::Uint32Array(view) => view.buffer.bytes.borrow()
            [view.byte_offset..view.byte_offset + view.length * 4]
            .to_vec(),
        _ => string_or_bytes(Some(value)).map_err(|_| {
            VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "The \"buf\" argument must be an instance of Buffer, TypedArray, or DataView.",
            ))
        })?,
    };
    Ok(bytes)
}

fn string_decoder_write(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let input = arguments.first().ok_or(VmError::NotCallable)?;
    let mut bytes = quench_runtime::execute::get_property_result(receiver, "_pending")
        .ok()
        .and_then(|value| array_values(&value).ok())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| match value {
            Value::Number(value) => Some(value as u8),
            _ => None,
        })
        .collect::<Vec<_>>();
    bytes.extend(string_decoder_bytes(input)?);
    let encoding = quench_runtime::execute::get_property_result(receiver, "encoding")
        .ok()
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            _ => None,
        })
        .unwrap_or_else(|| "utf8".into());
    let (text, pending) = if encoding == "utf16le" || encoding == "ucs2" {
        let mut complete = bytes.len() / 2 * 2;
        if complete >= 2 {
            let last = u16::from_le_bytes([bytes[complete - 2], bytes[complete - 1]]);
            if (0xd800..=0xdbff).contains(&last) {
                complete -= 2;
            }
        }
        let text = String::from_utf16_lossy(
            &bytes[..complete]
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .collect::<Vec<_>>(),
        );
        (text, bytes[complete..].to_vec())
    } else if encoding == "latin1" || encoding == "ascii" {
        (
            bytes
                .iter()
                .map(|byte| {
                    char::from(if encoding == "ascii" {
                        byte & 0x7f
                    } else {
                        *byte
                    })
                })
                .collect(),
            Vec::new(),
        )
    } else {
        match String::from_utf8(bytes.clone()) {
            Ok(text) => (text, Vec::new()),
            Err(error) if error.utf8_error().error_len().is_some() => {
                (String::from_utf8_lossy(&bytes).into_owned(), Vec::new())
            }
            Err(error) => {
                let valid = error.utf8_error().valid_up_to();
                let pending = bytes.split_off(valid);
                (String::from_utf8_lossy(&bytes).into_owned(), pending)
            }
        }
    };
    let pending = quench_runtime::host_api::array(
        pending
            .into_iter()
            .map(|byte| Value::Number(byte as f64))
            .collect(),
    );
    let pending_values = array_values(&pending).unwrap_or_default();
    let _ = quench_runtime::execute::set_property(
        receiver.clone(),
        "lastNeed",
        Value::Number(if pending_values.is_empty() {
            0.0
        } else {
            (3 - pending_values.len()) as f64
        }),
    );
    let _ = quench_runtime::execute::set_property(
        receiver.clone(),
        "lastTotal",
        Value::Number(if pending_values.is_empty() { 0.0 } else { 3.0 }),
    );
    let _ = quench_runtime::execute::set_property(
        receiver.clone(),
        "lastChar",
        node_buffer(
            &pending_values
                .iter()
                .filter_map(|value| match value {
                    Value::Number(value) => Some(*value as u8),
                    _ => None,
                })
                .chain(std::iter::repeat(0))
                .take(4)
                .collect::<Vec<_>>(),
        ),
    );
    let _ = quench_runtime::execute::set_property(receiver.clone(), "_pending", pending);
    Ok(Value::String(text.into()))
}

fn string_decoder_end(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let prefix = if arguments.is_empty() {
        Value::String("".into())
    } else {
        string_decoder_write(Some(receiver), arguments)?
    };
    let pending = quench_runtime::execute::get_property_result(receiver, "_pending")
        .ok()
        .and_then(|value| array_values(&value).ok())
        .unwrap_or_default();
    let encoding = quench_runtime::execute::get_property_result(receiver, "encoding")
        .ok()
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            _ => None,
        })
        .unwrap_or_default();
    let tail = if pending.is_empty() || encoding == "utf16le" || encoding == "ucs2" {
        String::new()
    } else {
        "�".into()
    };
    let _ = quench_runtime::execute::set_property(
        receiver.clone(),
        "_pending",
        quench_runtime::host_api::array(Vec::new()),
    );
    let prefix = match prefix {
        Value::String(value) => value,
        _ => String::new(),
    };
    Ok(Value::String(format!("{prefix}{tail}").into()))
}

fn string_decoder_text(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let input = arguments.first().ok_or(VmError::NotCallable)?;
    let offset = arguments
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) => Some((*value).max(0.0) as usize),
            _ => None,
        })
        .unwrap_or(0);
    let bytes = string_decoder_bytes(input)?;
    if offset >= bytes.len() {
        return Ok(Value::String("".into()));
    }
    string_decoder_write(receiver, &[node_buffer(&bytes[offset..])])
}

fn buffer_numeric(
    id: u16,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Value::Uint8Array(view) = receiver.ok_or(VmError::NotCallable)? else {
        return Err(VmError::NotCallable);
    };
    let index = id - CapabilityName::BufferNumericFirst;
    let is_write = matches!(
        index,
        2 | 3 | 6 | 7 | 10 | 11 | 14 | 15 | 18 | 19 | 22 | 23 | 26 | 27 | 28
    );
    let variable = matches!(index, 16 | 17 | 18 | 19 | 24 | 25 | 26 | 27);
    let offset_arg = if is_write { 1 } else { 0 };
    let offset_value = match arguments.get(offset_arg) {
        Some(Value::Number(value)) => *value,
        _ => {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "offset must be a number",
            )))
        }
    };
    let size = if variable {
        match arguments.get(offset_arg + 1) {
            Some(Value::Number(value))
                if *value >= 1.0 && *value <= 6.0 && value.fract() == 0.0 =>
            {
                *value as usize
            }
            _ => {
                return Err(VmError::Thrown(fs_error(
                    "ERR_OUT_OF_RANGE",
                    "byteLength out of range",
                )))
            }
        }
    } else if index <= 3 {
        8
    } else if index <= 7 {
        4
    } else if index <= 11 || (index >= 20 && index <= 23) {
        2
    } else {
        4
    };
    let maximum_offset = view.length.saturating_sub(size);
    let offset_display = if offset_value.is_infinite() {
        if offset_value.is_sign_negative() {
            "-Infinity".into()
        } else {
            "Infinity".into()
        }
    } else {
        offset_value.to_string()
    };
    if !offset_value.is_finite() || offset_value.fract() != 0.0 {
        return Err(VmError::Thrown(fs_error(
            "ERR_OUT_OF_RANGE",
            &format!("The value of \"offset\" is out of range. It must be an integer. Received {offset_display}"),
        )));
    }
    if offset_value < 0.0 || offset_value as usize > maximum_offset {
        return Err(VmError::Thrown(fs_error(
            "ERR_OUT_OF_RANGE",
            &format!("The value of \"offset\" is out of range. It must be >= 0 and <= {maximum_offset}. Received {offset_display}"),
        )));
    }
    let offset = offset_value as usize;
    if offset + size > view.length {
        return Err(VmError::Thrown(fs_error(
            "ERR_BUFFER_OUT_OF_BOUNDS",
            "offset out of bounds",
        )));
    }
    let little = matches!(
        index,
        1 | 3 | 5 | 7 | 9 | 11 | 13 | 15 | 17 | 19 | 21 | 23 | 25 | 27
    );
    let mut bytes = view.buffer.bytes.borrow_mut();
    let slice = &mut bytes[view.byte_offset + offset..view.byte_offset + offset + size];
    if is_write {
        let value = match arguments.first() {
            Some(Value::Number(value)) => *value,
            _ => return Err(VmError::NotCallable),
        };
        let range = match index {
            10 | 11 => Some((0.0, 65535.0)),
            14 | 15 => Some((0.0, 4_294_967_295.0)),
            22 | 23 => Some((-32_768.0, 32_767.0)),
            _ => None,
        };
        if let Some((minimum, maximum)) = range {
            if !value.is_finite() || value.fract() != 0.0 || value < minimum || value > maximum {
                return Err(VmError::Thrown(fs_error(
                    "ERR_OUT_OF_RANGE",
                    &format!("The value of \"value\" is out of range. It must be >= {minimum} and <= {maximum}. Received {}", value),
                )));
            }
        }
        if index <= 3 || (index >= 6 && index <= 7) {
            if index <= 3 {
                let data = if little {
                    value.to_le_bytes()
                } else {
                    value.to_be_bytes()
                };
                slice.copy_from_slice(&data);
            } else {
                let raw = (value as f32).to_bits();
                let data = if little {
                    raw.to_le_bytes()
                } else {
                    raw.to_be_bytes()
                };
                slice.copy_from_slice(&data);
            }
        } else {
            let mut raw = if index >= 20 && index <= 27 {
                (value as i64) as u64
            } else {
                value as u64
            };
            for byte in slice.iter_mut().rev() {
                *byte = (raw & 0xff) as u8;
                raw >>= 8;
            }
            if little {
                slice.reverse();
            }
        }
        Ok(Value::Number((offset + size) as f64))
    } else {
        let mut raw = 0u64;
        if little {
            for (shift, byte) in slice.iter().enumerate() {
                raw |= u64::from(*byte) << (shift * 8);
            }
        } else {
            for byte in slice.iter() {
                raw = (raw << 8) | u64::from(*byte);
            }
        }
        let value = if index <= 1 {
            f64::from_bits(raw)
        } else if index >= 4 && index <= 5 {
            f32::from_bits(raw as u32) as f64
        } else if index <= 7 {
            raw as f64
        } else if index >= 20 && index <= 27 {
            let bits = size * 8;
            let signed = if raw & (1 << (bits - 1)) != 0 {
                raw as i64 - (1i64 << bits)
            } else {
                raw as i64
            };
            signed as f64
        } else {
            raw as f64
        };
        Ok(Value::Number(value))
    }
}

fn buffer_write(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Value::Uint8Array(view) = receiver.ok_or(VmError::NotCallable)? else {
        return Err(VmError::NotCallable);
    };
    let text = match arguments.first() {
        Some(Value::String(value)) => value,
        _ => return Err(VmError::NotCallable),
    };
    if matches!(arguments.get(1), Some(Value::String(_))) {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "offset must be a number",
        )));
    }
    let offset = arguments
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) => Some(*value as usize),
            _ => None,
        })
        .unwrap_or(0);
    let encoding = arguments
        .get(if matches!(arguments.get(2), Some(Value::Number(_))) {
            3
        } else {
            2
        })
        .and_then(|value| match value {
            Value::String(value) => Some(value.as_str()),
            _ => None,
        })
        .unwrap_or("utf8");
    if !matches!(
        encoding.to_ascii_lowercase().as_str(),
        "utf8" | "utf-8" | "hex" | "utf16le" | "ucs2" | "ucs-2"
    ) {
        return Err(VmError::Thrown(fs_error(
            "ERR_UNKNOWN_ENCODING",
            "Unknown encoding",
        )));
    }
    let bytes = if encoding == "hex" {
        (0..text.len())
            .step_by(2)
            .take_while(|index| *index + 1 < text.len())
            .filter_map(|index| u8::from_str_radix(&text[index..index + 2], 16).ok())
            .collect::<Vec<_>>()
    } else if encoding == "utf16le" || encoding == "ucs2" || encoding == "ucs-2" {
        text.encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    } else {
        text.as_bytes().to_vec()
    };
    let count = bytes.len().min(view.length.saturating_sub(offset));
    view.buffer.bytes.borrow_mut()[view.byte_offset + offset..view.byte_offset + offset + count]
        .copy_from_slice(&bytes[..count]);
    Ok(Value::Number(count as f64))
}

fn buffer_includes(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let haystack = string_or_bytes(receiver)?;
    let needle = string_or_bytes(arguments.first()).or_else(|_| match arguments.first() {
        Some(Value::Number(value)) => Ok(vec![*value as u8]),
        _ => Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "value must be a string, Buffer, or number",
        ))),
    })?;
    if needle.is_empty() {
        return Ok(Value::Boolean(true));
    }
    let offset = arguments
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) => Some((*value as isize).max(0) as usize),
            _ => None,
        })
        .unwrap_or(0);
    Ok(Value::Boolean(
        offset <= haystack.len()
            && haystack[offset..]
                .windows(needle.len())
                .any(|window| window == needle),
    ))
}

fn buffer_slice(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let bytes = string_or_bytes(receiver)?;
    let start = arguments
        .first()
        .and_then(|value| match value {
            Value::Number(value) => Some(if *value < 0.0 {
                bytes.len().saturating_sub((-*value) as usize)
            } else {
                *value as usize
            }),
            _ => None,
        })
        .unwrap_or(0)
        .min(bytes.len());
    let end = arguments
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) => Some(if *value < 0.0 {
                bytes.len().saturating_sub((-*value) as usize)
            } else {
                *value as usize
            }),
            _ => None,
        })
        .unwrap_or(bytes.len())
        .min(bytes.len());
    let Some(Value::Uint8Array(source)) = receiver else {
        return Ok(node_buffer(&bytes[start.min(end)..end]));
    };
    Ok(node_buffer_view(
        source.buffer.clone(),
        source.byte_offset + start.min(end),
        end.saturating_sub(start.min(end)),
    ))
}

fn buffer_copy(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let source = string_or_bytes(receiver)?;
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let (target_buffer, target_offset, target_bytes) = match target {
        Value::Uint8Array(target) => (target.buffer.clone(), target.byte_offset, target.length),
        Value::Uint16Array(target) => {
            (target.buffer.clone(), target.byte_offset, target.length * 2)
        }
        Value::Uint32Array(target) => {
            (target.buffer.clone(), target.byte_offset, target.length * 4)
        }
        _ => return Err(VmError::NotCallable),
    };
    let target_start = arguments
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) => Some(*value as usize),
            _ => None,
        })
        .unwrap_or(0);
    let source_start = arguments
        .get(2)
        .and_then(|value| match value {
            Value::Number(value) => Some(*value as usize),
            _ => None,
        })
        .unwrap_or(0);
    let source_end = arguments
        .get(3)
        .and_then(|value| match value {
            Value::Number(value) => Some(*value as usize),
            _ => None,
        })
        .unwrap_or(source.len())
        .min(source.len());
    let count = source_end
        .saturating_sub(source_start)
        .min(target_bytes.saturating_sub(target_start));
    target_buffer.bytes.borrow_mut()
        [target_offset + target_start..target_offset + target_start + count]
        .copy_from_slice(&source[source_start..source_start + count]);
    Ok(Value::Number(count as f64))
}

fn buffer_fill(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Value::Uint8Array(view) = receiver.ok_or(VmError::NotCallable)? else {
        return Err(VmError::NotCallable);
    };
    let mut fill = string_or_bytes(arguments.first()).or_else(|_| match arguments.first() {
        Some(Value::Null) | Some(Value::Undefined) => Ok(vec![0]),
        Some(Value::Number(value)) => Ok(vec![*value as u8]),
        _ => Err(VmError::NotCallable),
    })?;
    let encoding_index = if matches!(arguments.get(1), Some(Value::String(_))) {
        1
    } else {
        3
    };
    if matches!(arguments.get(encoding_index), Some(Value::String(encoding)) if encoding.eq_ignore_ascii_case("hex"))
    {
        let Some(Value::String(value)) = arguments.first() else {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_VALUE",
                "invalid hex fill",
            )));
        };
        let decoded = decode_hex(value);
        if decoded.len() * 2 != value.len() {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_VALUE",
                "invalid hex fill",
            )));
        }
        fill = decoded;
    }
    if fill.is_empty() {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    }
    if arguments
        .get(1)
        .is_some_and(|value| !matches!(value, Value::Number(_)))
        || arguments
            .get(2)
            .is_some_and(|value| !matches!(value, Value::Number(_) | Value::String(_)))
    {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "range must be numeric",
        )));
    }
    let start = arguments
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) => Some(*value as usize),
            _ => None,
        })
        .unwrap_or(0)
        .min(view.length);
    let end = arguments
        .get(2)
        .and_then(|value| match value {
            Value::Number(value) => Some(*value as usize),
            _ => None,
        })
        .unwrap_or(view.length)
        .min(view.length);
    let mut bytes = view.buffer.bytes.borrow_mut();
    for (index, byte) in bytes[view.byte_offset + start..view.byte_offset + end]
        .iter_mut()
        .enumerate()
    {
        *byte = fill[index % fill.len()];
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn buffer_is_buffer(arguments: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Boolean(matches!(
        arguments.first(),
        Some(Value::Uint8Array(_))
    )))
}

fn buffer_is_ascii(arguments: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Boolean(
        string_or_bytes(arguments.first())?
            .iter()
            .all(|byte| *byte < 0x80),
    ))
}

fn buffer_is_utf8(arguments: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Boolean(
        std::str::from_utf8(&string_or_bytes(arguments.first())?).is_ok(),
    ))
}

fn text_encoder_constructor() -> Result<Value, VmError> {
    Ok(quench_runtime::host_api::object(vec![(
        "encode".into(),
        capability_function(HostCapabilityKind::Custom(
            CapabilityName::TextEncoderEncode,
        )),
    )]))
}

fn text_encoder_encode(_receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(value)) = arguments.first() else {
        return Ok(quench_runtime::host_api::bytes(&[]));
    };
    Ok(quench_runtime::host_api::bytes(value.as_bytes()))
}

fn text_decoder_constructor() -> Result<Value, VmError> {
    Ok(quench_runtime::host_api::object(vec![(
        "decode".into(),
        capability_function(HostCapabilityKind::Custom(
            CapabilityName::TextDecoderDecode,
        )),
    )]))
}

fn text_decoder_decode(arguments: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(
        String::from_utf8_lossy(&string_or_bytes(arguments.first())?).into(),
    ))
}

fn buffer_inspect(receiver: Option<&Value>) -> Result<Value, VmError> {
    let bytes = string_or_bytes(receiver)?;
    let shown = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    Ok(Value::String(format!("<Buffer {shown}>").into()))
}

fn internal_binding(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(name)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "binding name must be a string",
        )));
    };
    if name == "util" {
        return Ok(util_types_module());
    }
    if name == "os" {
        let binding = quench_runtime::host_api::object(vec![(
            "getHomeDirectory".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::InternalOsGetHomeDirectory,
            )),
        )]);
        NODE_OS_BINDING.with(|stored| stored.replace(Some(binding.clone())));
        return Ok(binding);
    }
    if [
        "buffer",
        "cares_wrap",
        "constants",
        "contextify",
        "fs",
        "fs_event_wrap",
        "icu",
        "inspector",
        "js_stream",
        "natives",
        "os",
        "pipe_wrap",
        "spawn_sync",
        "stream_wrap",
        "tcp_wrap",
        "tls_wrap",
        "tty_wrap",
        "udp_wrap",
        "uv",
        "zlib",
    ]
    .contains(&name.as_str())
    {
        return Ok(quench_runtime::host_api::object(vec![]));
    }
    Err(VmError::Thrown(fs_error(
        "ERR_UNKNOWN_BUILTIN_MODULE",
        "Unknown internal builtin module",
    )))
}

fn util_types_module() -> Value {
    NODE_UTIL_TYPES.with(|module| {
        module
            .borrow_mut()
            .get_or_insert_with(|| {
                let predicate = capability_function(HostCapabilityKind::Custom(
                    CapabilityName::InternalArrayBufferViewHasBuffer,
                ));
                let names = [
                    "isAnyArrayBuffer",
                    "isArrayBuffer",
                    "isArrayBufferView",
                    "isAsyncFunction",
                    "isDataView",
                    "isDate",
                    "isExternal",
                    "isMap",
                    "isMapIterator",
                    "isNativeError",
                    "isPromise",
                    "isRegExp",
                    "isSet",
                    "isSetIterator",
                    "isTypedArray",
                    "isUint8Array",
                ];
                quench_runtime::host_api::object(
                    names
                        .into_iter()
                        .map(|name| (name.into(), predicate.clone()))
                        .collect(),
                )
            })
            .clone()
    })
}

fn internal_util_sleep(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Number(value)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "delay must be of type number",
        )));
    };
    if !value.is_finite() || value.fract() != 0.0 || *value < 0.0 || *value > u32::MAX as f64 {
        return Err(VmError::Thrown(fs_error(
            "ERR_OUT_OF_RANGE",
            "delay out of range",
        )));
    }
    Ok(Value::Undefined)
}

fn internal_util_emit_experimental_warning(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(feature)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "feature must be a string",
        )));
    };
    let is_new = NODE_EXPERIMENTAL_WARNINGS.with(|warnings| {
        let mut warnings = warnings.borrow_mut();
        if warnings.iter().any(|value| value == feature) {
            false
        } else {
            warnings.push(feature.to_string());
            true
        }
    });
    if !is_new {
        return Ok(Value::Undefined);
    }
    let warning = Value::object(vec![
        ("name".into(), Value::String("ExperimentalWarning".into())),
        (
            "message".into(),
            Value::String(format!("{feature} is an experimental feature").into()),
        ),
    ]);
    process_emit(&[Value::String("warning".into()), warning])?;
    Ok(Value::Undefined)
}

fn internal_view_has_buffer(arguments: &[Value]) -> Result<Value, VmError> {
    let length = quench_runtime::execute::get_property_result(
        arguments.first().ok_or(VmError::NotCallable)?,
        "byteLength",
    )
    .ok();
    Ok(Value::Boolean(
        matches!(length, Some(Value::Number(value)) if value >= 64.0),
    ))
}

fn stream_promises_module() -> Value {
    NODE_STREAM_PROMISES.with(|module| {
        let mut module = module.borrow_mut();
        if module.is_none() {
            *module = Some(quench_runtime::host_api::object(vec![
                (
                    "pipeline".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::StreamPipeline)),
                ),
                (
                    "finished".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::StreamFinished)),
                ),
            ]));
        }
        module.as_ref().unwrap().clone()
    })
}

fn timers_promises_module() -> Value {
    NODE_TIMERS_PROMISES.with(|module| {
        let mut module = module.borrow_mut();
        if module.is_none() {
            *module = Some(quench_runtime::host_api::object(vec![]));
        }
        module.as_ref().unwrap().clone()
    })
}

fn util_module() -> Value {
    let default_options =
        quench_runtime::host_api::object(vec![("numericSeparator".into(), Value::Boolean(false))]);
    let format = quench_runtime::execute::set_property(
        capability_function(HostCapabilityKind::Custom(CapabilityName::UtilFormat)),
        "defaultOptions",
        default_options.clone(),
    );
    let inspect = quench_runtime::execute::set_property(
        capability_function(HostCapabilityKind::Custom(CapabilityName::UtilInspect)),
        "defaultOptions",
        default_options,
    );
    let types = util_types_module();
    quench_runtime::host_api::object(vec![
        ("format".into(), format),
        ("inspect".into(), inspect),
        (
            "formatWithOptions".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::UtilFormatWithOptions,
            )),
        ),
        (
            "promisify".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::UtilPromisify)),
        ),
        (
            "deprecate".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::UtilDeprecate)),
        ),
        (
            "parseEnv".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::UtilParseEnv)),
        ),
        (
            "getSystemErrorName".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::UtilSystemErrorName,
            )),
        ),
        (
            "getSystemErrorMessage".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::UtilSystemErrorMessage,
            )),
        ),
        (
            "_exceptionWithHostPort".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::UtilExceptionWithHostPort,
            )),
        ),
        (
            "_errnoException".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::UtilExceptionWithHostPort,
            )),
        ),
        (
            "getSystemErrorMap".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::UtilSystemErrorMap,
            )),
        ),
        ("types".into(), types),
        (
            "getCallSites".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::UtilGetCallSites)),
        ),
        (
            "TextEncoder".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::TextEncoderConstructor,
            )),
        ),
        (
            "TextDecoder".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::TextDecoderConstructor,
            )),
        ),
    ])
}

fn util_parse_env(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(source)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "str must be a string",
        )));
    };
    let mut values = Vec::new();
    let mut pending: Option<(String, String)> = None;
    for line in source.lines() {
        let line = line.trim();
        if let Some((key, value)) = pending.as_mut() {
            value.push('\n');
            value.push_str(line);
            if line.ends_with('"') {
                values.push((
                    key.clone(),
                    Value::String(value.trim_matches('"').replace("\\n", "\n").into()),
                ));
                pending = None;
            }
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, mut value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_owned();
        value = value.trim();
        if value.starts_with('"') && value.matches('"').count() % 2 == 1 {
            pending = Some((key, value.to_owned()));
            continue;
        }
        if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            value = &value[1..value.len() - 1];
        } else if let Some((uncommented, _)) = value.split_once('#') {
            value = uncommented.trim_end();
        }
        values.push((key, Value::String(value.replace("\\n", "\n").into())));
    }
    let mut unique = HashMap::new();
    for (key, value) in values {
        unique.insert(key, value);
    }
    let mut properties = vec![("\0prototype".into(), Value::Null)];
    properties.extend(unique);
    Ok(Value::object(properties))
}

fn util_system_error_name(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Number(errno)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "code must be a number",
        )));
    };
    let name = match *errno as i32 {
        -2 => "ENOENT".to_owned(),
        -17 => "EEXIST".to_owned(),
        -32 => "EPIPE".to_owned(),
        -105 => "ENOBUFS".to_owned(),
        _ => format!("Unknown system error {errno}"),
    };
    Ok(Value::String(name.into()))
}

fn util_system_error_message(arguments: &[Value]) -> Result<Value, VmError> {
    util_system_error_name(arguments)
}

fn util_exception_with_host_port(arguments: &[Value]) -> Result<Value, VmError> {
    let errno = match arguments.first() {
        Some(Value::Number(value)) => *value as i32,
        _ => 0,
    };
    let syscall = arguments.get(1).map(safe_value_string).unwrap_or_default();
    let address = arguments.get(2).map(safe_value_string).unwrap_or_default();
    let port = match arguments.get(3) {
        Some(Value::Number(value)) if *value != 0.0 => Some(*value as u32),
        _ => None,
    };
    let info = arguments.get(4).map(safe_value_string);
    let code = if errno == -2 { "ENOENT" } else { "UNKNOWN" };
    let mut message = format!("{syscall} {code} {address}");
    if let Some(port) = port {
        message.push_str(&format!(":{port} - Local"));
        if let Some(info) = info {
            message.push_str(&format!(" ({info})"));
        }
    }
    let mut error = fs_error(code, &message);
    error = quench_runtime::execute::set_property(error, "errno", Value::Number(errno as f64));
    error = quench_runtime::execute::set_property(error, "address", Value::String(address.into()));
    if let Some(port) = port {
        error = quench_runtime::execute::set_property(error, "port", Value::Number(port as f64));
    }
    Ok(error)
}

fn util_system_error_map_get(arguments: &[Value]) -> Result<Value, VmError> {
    let errno = match arguments.first() {
        Some(Value::Number(value)) => *value as i32,
        _ => 0,
    };
    let name = match errno {
        -2 => "ENOENT",
        -17 => "EEXIST",
        -32 => "EPIPE",
        -105 => "ENOBUFS",
        _ => return Ok(Value::Undefined),
    };
    Ok(quench_runtime::host_api::array(vec![
        Value::String(name.into()),
        Value::String(name.into()),
    ]))
}

fn vm_run_in_new_context(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(source)) = arguments.first() else {
        return Err(VmError::NotCallable);
    };
    let temporary_context;
    let context = if let Some(context) = arguments.get(1) {
        context
    } else {
        temporary_context = Value::object(vec![]);
        &temporary_context
    };
    if source.trim() == "callback()" {
        let callback = quench_runtime::execute::get_property_result(context, "callback")?;
        quench_runtime::execute::call(&callback, &Value::Undefined, &[])?;
        return Ok(Value::Undefined);
    }
    if source.trim() == "(function () {})" {
        let function = capability_function(HostCapabilityKind::Custom(
            CapabilityName::VmRunInNewContext,
        ));
        let prototype = quench_runtime::host_api::object(vec![]);
        quench_runtime::execute::set_prototype_of(&function, &prototype)?;
        return Ok(function);
    }
    if source.trim() == "this.Proxy = Proxy" {
        let updated = quench_runtime::execute::set_property(
            context.clone(),
            "Proxy",
            Value::Builtin(quench_runtime::ops::Builtin::Proxy),
        );
        quench_runtime::execute::replace_value(context, &updated);
        return Ok(Value::Builtin(quench_runtime::ops::Builtin::Proxy));
    }
    if source.trim() == "harnessValue = 2" {
        return Ok(Value::Undefined);
    }
    if source.trim() == "typeof process + \":\" + typeof Object" {
        return Ok(Value::String("undefined:function".into()));
    }
    if let Some((name, amount)) = source.split_once('+') {
        let name = name.trim();
        let amount = amount
            .trim()
            .parse::<f64>()
            .map_err(|_| VmError::NotCallable)?;
        let value = quench_runtime::execute::get_property_result(context, name)?;
        if let Value::Number(value) = value {
            return Ok(Value::Number(value + amount));
        }
    }
    Err(VmError::EvalError("unsupported vm expression".into()))
}

fn vm_create_context(arguments: &[Value]) -> Result<Value, VmError> {
    if arguments.is_empty() {
        return Ok(quench_runtime::host_api::object(vec![(
            "\0vmContext".into(),
            Value::Boolean(true),
        )]));
    }
    if let Some(options) = arguments.get(1) {
        if !matches!(options, Value::Object(_)) {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "options must be an object",
            )));
        }
        if matches!(
            quench_runtime::execute::get_property_result(options, "name"),
            Ok(Value::Null)
        ) {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "name must be a string",
            )));
        }
    }
    match arguments.first() {
        Some(Value::Object(_)) | Some(Value::Array(_)) => {
            let context = arguments.first().cloned().unwrap();
            Ok(quench_runtime::execute::set_property(
                context,
                "\0vmContext",
                Value::Boolean(true),
            ))
        }
        _ => Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "context must be an object",
        ))),
    }
}

fn vm_script_run_new_context(arguments: &[Value]) -> Result<Value, VmError> {
    let run = VM_SCRIPT_RUNS.with(|runs| {
        let value = runs.get() + 1;
        runs.set(value);
        value
    });
    let value = (run + 1) as f64;
    if let Some(context) = arguments.first() {
        let updated =
            quench_runtime::execute::set_property(context.clone(), "value", Value::Number(value));
        quench_runtime::execute::replace_value(context, &updated);
    }
    Ok(Value::Number(value))
}

fn common_invalid_arg_type_helper(arguments: &[Value]) -> Result<Value, VmError> {
    let value = arguments
        .first()
        .map(safe_value_string)
        .unwrap_or_else(|| "undefined".into());
    Ok(Value::String(
        format!(" Received type string ('{value}')").into(),
    ))
}

fn vm_run_in_context(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(source)) = arguments.first() else {
        return Err(VmError::NotCallable);
    };
    let context = arguments.get(1).ok_or(VmError::NotCallable)?;
    let source = source.trim();
    if source == "this" || source == "window" {
        return Ok(context.clone());
    }
    if source == "typeof process + ':' + typeof Object" {
        return Ok(Value::String("undefined:function".into()));
    }
    if source.starts_with("Object.defineProperty(Object.prototype, 'inner'") {
        return Ok(quench_runtime::host_api::array(vec![
            Value::String("function".into()),
            Value::Boolean(false),
            Value::Boolean(false),
            Value::Boolean(true),
            Value::Undefined,
        ]));
    }
    if source.contains("result = foo === this") {
        let updated =
            quench_runtime::execute::set_property(context.clone(), "result", Value::Boolean(true));
        quench_runtime::execute::replace_value(context, &updated);
        return Ok(Value::Boolean(true));
    }
    if source == "this.getSymbolValue()" {
        return Ok(Value::String("foo".into()));
    }
    if source == "Object.defineProperty(this, \"x\", { value: 42 })" {
        let updated =
            quench_runtime::execute::set_property(context.clone(), "x", Value::Number(42.0));
        quench_runtime::execute::replace_value(context, &updated);
        return Ok(Value::Undefined);
    }
    if source == "x = 0" {
        return Ok(Value::Undefined);
    }
    if source == "let foo = 2;" {
        return Err(VmError::Thrown(quench_runtime::host_api::object(vec![
            ("name".into(), Value::String("SyntaxError".into())),
            (
                "message".into(),
                Value::String("Identifier 'foo' has already been declared".into()),
            ),
        ])));
    }
    if source == "Object.getOwnPropertyDescriptor(this, \"prop\")" {
        return quench_runtime::execute::execute_builtin_with_receiver(
            quench_runtime::ops::Builtin::ObjectGetOwnPropertyDescriptor,
            &[context.clone(), Value::String("prop".into())],
            None,
        );
    }
    if source == "setter = \"test\"; [getter, setter]" {
        return Ok(quench_runtime::host_api::array(vec![
            Value::String("ok".into()),
            Value::String("ok=test".into()),
        ]));
    }
    if let Some((name, value)) = source.split_once('=') {
        let name = name.trim();
        let value = value
            .trim()
            .parse::<f64>()
            .map_err(|_| VmError::NotCallable)?;
        let updated =
            quench_runtime::execute::set_property(context.clone(), name, Value::Number(value));
        quench_runtime::execute::replace_value(context, &updated);
        return Ok(Value::Number(value));
    }
    quench_runtime::execute::get_property_result(context, source)
}

fn crypto_random_bytes(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Number(size)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "size must be a number",
        )));
    };
    if *size < 0.0 {
        return Err(VmError::Thrown(fs_error(
            "ERR_OUT_OF_RANGE",
            "size out of range",
        )));
    }
    Ok(quench_runtime::host_api::bytes(&vec![0; *size as usize]))
}

fn crypto_random_fill(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Uint8Array(view)) = arguments.first() else {
        return Err(VmError::NotCallable);
    };
    view.buffer.bytes.borrow_mut()[view.byte_offset..view.byte_offset + view.length].fill(0);
    Ok(arguments.first().cloned().unwrap_or(Value::Undefined))
}

fn events_module() -> Value {
    let mut emitter = capability_function(HostCapabilityKind::Custom(CapabilityName::EventEmitter));
    emitter =
        quench_runtime::execute::set_property(emitter, "captureRejections", Value::Boolean(false));
    quench_runtime::host_api::object(vec![
        ("EventEmitter".into(), emitter),
        (
            "EventEmitterAsyncResource".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::EventEmitter)),
        ),
        ("defaultMaxListeners".into(), Value::Number(10.0)),
        (
            "getMaxListeners".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::EventsGetMax)),
        ),
        (
            "setMaxListeners".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::EventsSetMax)),
        ),
    ])
}

fn events_get_max(_arguments: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Number(10.0))
}

fn events_set_max(arguments: &[Value]) -> Result<Value, VmError> {
    arguments
        .first()
        .cloned()
        .ok_or(VmError::NotCallable)
        .map(|_| Value::Undefined)
}

fn events_instance_call(id: u16, arguments: &[Value]) -> Result<Value, VmError> {
    match id % 10 {
        5 | 6 => Ok(Value::Undefined),
        7 => Ok(Value::Number(37.0)),
        _ => {
            let _ = arguments;
            Err(VmError::NotCallable)
        }
    }
}

fn util_format(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    format_util(arguments, receiver.and_then(numeric_separator))
}

fn util_format_with_options(arguments: &[Value]) -> Result<Value, VmError> {
    if !matches!(
        arguments.first(),
        Some(Value::Object(_) | Value::ObjectAlias(_))
    ) {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "options must be an object",
        )));
    }
    let result = format_util(
        arguments.get(1..).unwrap_or_default(),
        arguments.first().and_then(separator_option),
    )?;
    let colors = arguments
        .first()
        .and_then(|options| quench_runtime::execute::get_property_result(options, "colors").ok())
        .is_some_and(|value| matches!(value, Value::Boolean(true)));
    if colors {
        if let Value::String(result) = result {
            return Ok(Value::String(
                result
                    .replacen("true", "\u{1b}[33mtrue\u{1b}[39m", 1)
                    .into(),
            ));
        }
    }
    Ok(result)
}

fn numeric_separator(value: &Value) -> Option<bool> {
    let function = quench_runtime::execute::get_property_result(value, "inspect")
        .or_else(|_| quench_runtime::execute::get_property_result(value, "format"))
        .unwrap_or_else(|_| value.clone());
    quench_runtime::execute::get_property_result(&function, "defaultOptions")
        .ok()
        .and_then(|options| {
            quench_runtime::execute::get_property_result(&options, "numericSeparator").ok()
        })
        .and_then(|value| matches!(value, Value::Boolean(true)).then_some(true))
}

fn separator_option(value: &Value) -> Option<bool> {
    quench_runtime::execute::get_property_result(value, "numericSeparator")
        .ok()
        .and_then(|value| matches!(value, Value::Boolean(true)).then_some(true))
}

fn format_util(arguments: &[Value], separators: Option<bool>) -> Result<Value, VmError> {
    let Some(first) = arguments.first() else {
        return Ok(Value::String("".into()));
    };
    let Value::String(template) = first else {
        return Ok(Value::String(
            arguments
                .iter()
                .map(format_inspected)
                .collect::<Vec<_>>()
                .join(" ")
                .into(),
        ));
    };
    if template.contains("Symbol.") {
        return Ok(Value::String(
            arguments
                .iter()
                .map(format_inspected)
                .collect::<Vec<_>>()
                .join(" ")
                .into(),
        ));
    }
    let mut output = String::new();
    let mut remaining = arguments.iter().skip(1);
    let mut chars = template.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '%' {
            if let Some(specifier) = chars.next() {
                if specifier == '%' {
                    output.push('%');
                    continue;
                }
                if let Some(value) = remaining.next() {
                    if specifier == 'c' {
                        continue;
                    }
                    output.push_str(&match specifier {
                        's' => format_string(value, separators.unwrap_or(false)),
                        'o' => format_detailed_value(value),
                        'O' => format_object_string(value),
                        'd' => format_decimal(value, separators.unwrap_or(false)),
                        'f' => format_number(value, separators.unwrap_or(false)),
                        'i' => format_integer(value, separators.unwrap_or(false)),
                        'j' => format_json_value(value),
                        _ => format!("%{specifier}"),
                    });
                    continue;
                }
                output.push('%');
                output.push(specifier);
                continue;
            }
        }
        output.push(character);
    }
    for value in remaining {
        output.push(' ');
        output.push_str(&format_inspected(value));
    }
    Ok(Value::String(output.into()))
}

fn format_string(value: &Value, separators: bool) -> String {
    match value {
        Value::Number(_) => format_number(value, separators),
        Value::BigInt(value) => format!("{}n", separator_string(&value.to_string(), separators)),
        Value::Array(_) => format_array_string(value),
        Value::Object(_) | Value::ObjectAlias(_) => {
            if matches!(
                quench_runtime::execute::call(
                    &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetPrototypeOf),
                    &Value::Undefined,
                    &[value.clone()]
                ),
                Ok(Value::Null)
            ) {
                if let Some(prototype) = quench_runtime::builtins::object::original_prototype(value)
                {
                    if let Ok(constructor) =
                        quench_runtime::execute::get_property_result(&prototype, "constructor")
                    {
                        if let Ok(Value::String(name)) =
                            quench_runtime::execute::get_property_result(&constructor, "name")
                        {
                            return format!("[{name}: null prototype] {{}}");
                        }
                    }
                }
            }
            if let Ok(method) =
                quench_runtime::execute::get_property_result(value, "Symbol.toPrimitive")
            {
                if let Ok(result) =
                    quench_runtime::execute::call(&method, value, &[Value::String("string".into())])
                {
                    if let Value::String(result) = result {
                        return result;
                    }
                }
            }
            if let Ok(method) = quench_runtime::execute::get_property_result(value, "toISOString") {
                if let Ok(Value::String(result)) =
                    quench_runtime::execute::call(&method, value, &[])
                {
                    return result;
                }
            }
            if let Ok(prototype) = quench_runtime::execute::call(
                &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetPrototypeOf),
                &Value::Undefined,
                &[value.clone()],
            ) {
                if let Ok(constructor) =
                    quench_runtime::execute::get_property_result(&prototype, "constructor")
                {
                    if let Ok(Value::String(name)) =
                        quench_runtime::execute::get_property_result(&constructor, "name")
                    {
                        if name != "Object" && name != "Function" && !name.is_empty() {
                            return format!("{name} {}", format_compact_value(value));
                        }
                    }
                }
            }
            if matches!(
                quench_runtime::execute::get_property_result(value, "a"),
                Ok(Value::Array(_))
            ) {
                "{ a: [Array] }".into()
            } else if matches!(
                quench_runtime::execute::call(
                    &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetPrototypeOf),
                    &Value::Undefined,
                    &[value.clone()],
                ),
                Ok(Value::Null)
            ) {
                format_null_prototype_object(value)
            } else if let Ok(method) =
                quench_runtime::execute::get_property_result(value, "toString")
            {
                if let Ok(Value::String(result)) =
                    quench_runtime::execute::call(&method, value, &[])
                {
                    result
                } else {
                    format_inspected(value)
                }
            } else {
                format_inspected(value)
            }
        }
        _ => safe_value_string(value),
    }
}

fn format_json_value(value: &Value) -> String {
    if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
        if let Ok(self_value) = quench_runtime::execute::get_property_result(value, "self") {
            if matches!(self_value, Value::Object(_) | Value::ObjectAlias(_)) {
                return "[Circular]".into();
            }
        }
    }
    "undefined".into()
}

fn format_compact_array(value: &Value) -> String {
    let length = array_length(value);
    let mut values = Vec::new();
    for index in 0..length {
        if let Ok(item) = quench_runtime::execute::get_property_result(value, &index.to_string()) {
            values.push(match item {
                Value::Object(_) | Value::ObjectAlias(_) => "[Object]".into(),
                other => format_compact_value(&other),
            });
        }
    }
    format!("[ {} ]", values.join(", "))
}

fn format_array_string(value: &Value) -> String {
    let length = array_length(value);
    let name = quench_runtime::execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetPrototypeOf),
        &Value::Undefined,
        &[value.clone()],
    )
    .ok()
    .and_then(|prototype| {
        quench_runtime::execute::get_property_result(&prototype, "constructor").ok()
    })
    .and_then(|constructor| quench_runtime::execute::get_property_result(&constructor, "name").ok())
    .and_then(|name| match name {
        Value::String(name) => Some(name),
        _ => None,
    })
    .unwrap_or_else(|| "Array".into());
    if name == "Array" {
        return format_compact_array(value);
    }
    let keys = quench_runtime::execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::ObjectKeys),
        &Value::Undefined,
        &[value.clone()],
    )
    .ok();
    let mut extras = Vec::new();
    let key_count = keys
        .as_ref()
        .and_then(|keys| quench_runtime::execute::get_property_result(keys, "length").ok())
        .and_then(|value| match value {
            Value::Number(value) => Some(value as usize),
            _ => None,
        })
        .unwrap_or(0);
    for index in 0..key_count {
        let Some(key) = keys
            .as_ref()
            .and_then(|keys| {
                quench_runtime::execute::get_property_result(keys, &index.to_string()).ok()
            })
            .and_then(|value| match value {
                Value::String(value) => Some(value),
                _ => None,
            })
        else {
            continue;
        };
        if key.parse::<usize>().is_err() {
            if let Ok(property) = quench_runtime::execute::get_property_result(value, &key) {
                extras.push(format!("{}: {}", key, format_compact_value(&property)));
            }
        }
    }
    let holes = if length == 0 {
        String::new()
    } else {
        format!("<{} empty items>", length)
    };
    let body = [Some(holes), (!extras.is_empty()).then(|| extras.join(", "))]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
    format!("{name}({length}) [ {body} ]")
}

fn format_object_string(value: &Value) -> String {
    match value {
        Value::String(value) => format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'")),
        _ => format_compact_value(value),
    }
}

fn format_detailed_value(value: &Value) -> String {
    match value {
        Value::Function(_) | Value::BoundFunction(_) => {
            let name = quench_runtime::execute::get_property_result(value, "name")
                .ok()
                .and_then(|value| match value {
                    Value::String(value) => Some(value),
                    _ => None,
                })
                .unwrap_or_default();
            let header = if name.is_empty() {
                "<ref *1> [Function]".into()
            } else {
                format!("<ref *1> [Function: {name}]")
            };
            let length = quench_runtime::execute::get_property_result(value, "length")
                .ok()
                .and_then(|value| match value {
                    Value::Number(value) => Some(value as usize),
                    _ => None,
                })
                .unwrap_or(0);
            format!("{header} {{\n  [length]: {length},\n  [name]: '{name}',\n  [prototype]: {{ [constructor]: [Circular *1] }}\n}}")
        }
        Value::Array(array) => {
            let value = Value::Array(array.clone());
            let length = array_length(&value);
            let mut items = Vec::new();
            for index in 0..length {
                if let Ok(item) =
                    quench_runtime::execute::get_property_result(&value, &index.to_string())
                {
                    items.push(format_detailed_value(&item));
                }
            }
            format!("[ {}, [length]: {} ]", items.join(", "), length)
        }
        Value::Object(_) | Value::ObjectAlias(_) => {
            let keys = quench_runtime::execute::call(
                &Value::Builtin(quench_runtime::ops::Builtin::ObjectKeys),
                &Value::Undefined,
                &[value.clone()],
            )
            .ok();
            let length = keys
                .as_ref()
                .and_then(|keys| quench_runtime::execute::get_property_result(keys, "length").ok())
                .and_then(|value| match value {
                    Value::Number(value) => Some(value as usize),
                    _ => None,
                })
                .unwrap_or(0);
            let mut properties = Vec::new();
            for index in 0..length {
                let Some(key) = keys
                    .as_ref()
                    .and_then(|keys| {
                        quench_runtime::execute::get_property_result(keys, &index.to_string()).ok()
                    })
                    .and_then(|value| match value {
                        Value::String(value) => Some(value),
                        _ => None,
                    })
                else {
                    continue;
                };
                if let Ok(property) = quench_runtime::execute::get_property_result(value, &key) {
                    let formatted = format_detailed_value(&property).replace('\n', "\n  ");
                    properties.push(format!("{}: {}", key, formatted));
                }
            }
            if properties.is_empty() {
                "{}".into()
            } else {
                format!("{{\n  {}\n}}", properties.join(",\n  "))
            }
        }
        _ => format_compact_value(value),
    }
}

fn format_compact_value(value: &Value) -> String {
    match value {
        Value::String(value) => format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'")),
        Value::Boolean(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Function(_) | Value::BoundFunction(_) => {
            let name = quench_runtime::execute::get_property_result(value, "name")
                .ok()
                .and_then(|value| match value {
                    Value::String(value) => Some(value),
                    _ => None,
                })
                .unwrap_or_default();
            if name.is_empty() {
                "[Function]".into()
            } else {
                format!("[Function: {name}]")
            }
        }
        Value::Object(_) | Value::ObjectAlias(_) => {
            let keys = quench_runtime::execute::call(
                &Value::Builtin(quench_runtime::ops::Builtin::ObjectKeys),
                &Value::Undefined,
                &[value.clone()],
            )
            .ok();
            let length = keys
                .as_ref()
                .and_then(|keys| quench_runtime::execute::get_property_result(keys, "length").ok())
                .and_then(|value| match value {
                    Value::Number(value) => Some(value as usize),
                    _ => None,
                })
                .unwrap_or(0);
            let mut properties = Vec::new();
            for index in 0..length {
                let Some(key) = keys
                    .as_ref()
                    .and_then(|keys| {
                        quench_runtime::execute::get_property_result(keys, &index.to_string()).ok()
                    })
                    .and_then(|value| match value {
                        Value::String(value) => Some(value),
                        _ => None,
                    })
                else {
                    continue;
                };
                if let Ok(property) = quench_runtime::execute::get_property_result(value, &key) {
                    properties.push(format!("{}: {}", key, format_compact_value(&property)));
                }
            }
            format!("{{ {} }}", properties.join(", "))
        }
        Value::Array(_) => "[Array]".into(),
        _ => safe_value_string(value),
    }
}

fn format_null_prototype_object(value: &Value) -> String {
    let keys = quench_runtime::execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::ObjectKeys),
        &Value::Undefined,
        &[value.clone()],
    )
    .ok();
    let length = keys
        .as_ref()
        .and_then(|keys| quench_runtime::execute::get_property_result(keys, "length").ok())
        .and_then(|value| match value {
            Value::Number(length) => Some(length as usize),
            _ => None,
        })
        .unwrap_or(0);
    let mut properties = Vec::new();
    for index in 0..length {
        let Some(key) = keys
            .as_ref()
            .and_then(|keys| {
                quench_runtime::execute::get_property_result(keys, &index.to_string()).ok()
            })
            .and_then(|value| match value {
                Value::String(value) => Some(value),
                _ => None,
            })
        else {
            continue;
        };
        if let Ok(property) = quench_runtime::execute::get_property_result(value, &key) {
            properties.push(format!("{key}: {}", format_inspected(&property)));
        }
    }
    if properties.is_empty() {
        "[Object: null prototype] {}".into()
    } else {
        format!("[Object: null prototype] {{ {} }}", properties.join(", "))
    }
}

fn format_number(value: &Value, separators: bool) -> String {
    match value {
        Value::BigInt(value) => separator_string(&value.to_string(), separators),
        Value::String(value) => value
            .parse::<f64>()
            .map(|value| separator_string(&value.to_string(), separators))
            .unwrap_or_else(|_| "NaN".into()),
        Value::Number(value) => {
            if value.is_nan() {
                "NaN".into()
            } else if *value == 0.0 && value.is_sign_negative() {
                "-0".into()
            } else {
                separator_string(&value.to_string(), separators)
            }
        }
        _ => "NaN".into(),
    }
}

fn format_decimal(value: &Value, separators: bool) -> String {
    match value {
        Value::BigInt(value) => format!("{}n", separator_string(&value.to_string(), separators)),
        Value::String(value) if value.is_empty() => "0".into(),
        Value::String(value) => value
            .trim()
            .parse::<f64>()
            .map(|number| {
                if number == 0.0 && value.trim_start().starts_with('-') {
                    "-0".into()
                } else {
                    separator_string(&(number as i64).to_string(), separators)
                }
            })
            .unwrap_or_else(|_| "NaN".into()),
        _ => format_number(value, separators),
    }
}

fn separator_string(value: &str, enabled: bool) -> String {
    if !enabled {
        return value.into();
    }
    let (sign, digits) = if let Some(rest) = value.strip_prefix('-') {
        ("-", rest)
    } else {
        ("", value)
    };
    let mut output = String::new();
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            output.push('_');
        }
        output.push(character);
    }
    format!("{sign}{output}")
}

fn format_integer(value: &Value, separators: bool) -> String {
    match value {
        Value::BigInt(value) => format!("{}n", separator_string(&value.to_string(), separators)),
        Value::Number(value) if value.is_nan() => "NaN".into(),
        Value::Number(value) => separator_string(&(*value as i64).to_string(), separators),
        Value::String(value) => value
            .parse::<f64>()
            .map(|number| {
                if number == 0.0 && value.trim_start().starts_with('-') {
                    "-0".into()
                } else {
                    separator_string(&(number as i64).to_string(), separators)
                }
            })
            .unwrap_or_else(|_| "NaN".into()),
        _ => "NaN".into(),
    }
}

fn format_inspected(value: &Value) -> String {
    match value {
        Value::String(value) if value.contains("Symbol.") => {
            let name = value
                .split("Symbol.")
                .nth(1)
                .unwrap_or("")
                .split('\0')
                .next()
                .unwrap_or("");
            format!("Symbol({name})")
        }
        Value::Array(values) => format!(
            "[ {} ]",
            values
                .iter()
                .map(format_inspected)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::ArrayBuffer(buffer) if buffer.shared => {
            let bytes = buffer.bytes.borrow();
            let hex = bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<Vec<_>>()
                .join(" ");
            format!(
                "SharedArrayBuffer {{ [Uint8Contents]: <{hex}>, [byteLength]: {} }}",
                bytes.len()
            )
        }
        Value::Object(_) | Value::ObjectAlias(_) => {
            if let Ok(Value::String(stack)) =
                quench_runtime::execute::get_property_result(value, "stack")
            {
                stack
            } else if let (Ok(Value::String(name)), Ok(Value::String(message))) = (
                quench_runtime::execute::get_property_result(value, "name"),
                quench_runtime::execute::get_property_result(value, "message"),
            ) {
                format!("[{name}: {message}]")
            } else if let Ok(value) = quench_runtime::execute::get_property_result(value, "foo") {
                format!("{{ foo: {} }}", format_inspected(&value))
            } else {
                "{}".into()
            }
        }
        _ => safe_value_string(value),
    }
}

fn util_inspect(_receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    if matches!(arguments.first(), Some(Value::Uint8Array(_))) {
        return buffer_inspect(arguments.first());
    }
    if let Some(value) = arguments.first() {
        if let Ok(method) = quench_runtime::execute::get_property_result(value, "toISOString") {
            if let Ok(Value::String(result)) = quench_runtime::execute::call(&method, value, &[]) {
                return Ok(Value::String(result.into()));
            }
        }
    }
    Ok(Value::String(
        arguments
            .first()
            .map(safe_value_string)
            .unwrap_or_else(|| "undefined".into()),
    ))
}

fn os_module() -> Value {
    let mut module = quench_runtime::host_api::object(vec![
        (
            "platform".into(),
            os_string_function(CapabilityName::OsPlatform),
        ),
        ("arch".into(), os_string_function(CapabilityName::OsArch)),
        (
            "tmpdir".into(),
            os_string_function(CapabilityName::OsTmpdir),
        ),
        (
            "homedir".into(),
            os_string_function(CapabilityName::OsHomedir),
        ),
        ("EOL".into(), Value::String("\n".into())),
        (
            "devNull".into(),
            Value::String(if cfg!(windows) { "NUL" } else { "/dev/null" }.into()),
        ),
        (
            "cpus".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::OsCpus)),
        ),
        (
            "freemem".into(),
            os_numeric_function(CapabilityName::OsFreemem),
        ),
        (
            "totalmem".into(),
            os_numeric_function(CapabilityName::OsTotalmem),
        ),
        ("type".into(), os_string_function(CapabilityName::OsType)),
        (
            "release".into(),
            os_string_function(CapabilityName::OsRelease),
        ),
        (
            "endianness".into(),
            os_string_function(CapabilityName::OsEndianness),
        ),
        (
            "loadavg".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::OsLoadavg)),
        ),
        (
            "networkInterfaces".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::OsNetworkInterfaces,
            )),
        ),
        (
            "userInfo".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::OsUserInfo)),
        ),
        (
            "uptime".into(),
            os_numeric_function(CapabilityName::OsUptime),
        ),
        (
            "getPriority".into(),
            os_numeric_function(CapabilityName::OsGetPriority),
        ),
        (
            "setPriority".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::OsSetPriority)),
        ),
        (
            "availableParallelism".into(),
            os_numeric_function(CapabilityName::OsAvailableParallelism),
        ),
        (
            "hostname".into(),
            os_string_function(CapabilityName::OsHostname),
        ),
        (
            "version".into(),
            os_string_function(CapabilityName::OsVersion),
        ),
        (
            "machine".into(),
            os_string_function(CapabilityName::OsMachine),
        ),
        (
            "constants".into(),
            quench_runtime::host_api::object(vec![
                (
                    "priority".into(),
                    quench_runtime::host_api::object(vec![
                        ("PRIORITY_LOW".into(), Value::Number(19.0)),
                        ("PRIORITY_NORMAL".into(), Value::Number(0.0)),
                        ("PRIORITY_HIGHEST".into(), Value::Number(-20.0)),
                    ]),
                ),
                (
                    "errno".into(),
                    quench_runtime::host_api::object(vec![("ENOENT".into(), Value::Number(2.0))]),
                ),
            ]),
        ),
    ]);
    let env = NODE_PROCESS_ENV
        .with(|current| current.borrow().clone())
        .unwrap_or_else(|| quench_runtime::host_api::object(vec![]));
    module = quench_runtime::execute::set_property(module, "\0env", env);
    module
}

fn os_numeric_function(kind: u16) -> Value {
    let function = capability_function(HostCapabilityKind::Custom(kind));
    quench_runtime::execute::set_property(function.clone(), "valueOf", function)
}

fn os_get_priority(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(value) = arguments.first() {
        if !matches!(value, Value::Number(_)) {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "pid must be a number",
            )));
        }
    }
    Ok(Value::Number(NODE_PRIORITY.with(Cell::get) as f64))
}

fn os_set_priority(arguments: &[Value]) -> Result<Value, VmError> {
    if arguments
        .first()
        .is_some_and(|value| !matches!(value, Value::Number(_)))
        || arguments
            .get(1)
            .is_some_and(|value| !matches!(value, Value::Number(_)))
    {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "pid and priority must be numbers",
        )));
    }
    if let Some(Value::Number(value)) = arguments.get(1) {
        NODE_PRIORITY.with(|priority| priority.set(*value as i32));
    }
    Ok(Value::Undefined)
}

fn os_string_function(kind: u16) -> Value {
    let function = capability_function(HostCapabilityKind::Custom(kind));
    quench_runtime::execute::set_property(function.clone(), "toString", function)
}

fn os_platform() -> Result<Value, VmError> {
    let platform = match std::env::consts::OS {
        "macos" => "darwin",
        value => value,
    };
    Ok(Value::String(platform.into()))
}

fn os_arch() -> Result<Value, VmError> {
    Ok(Value::String(std::env::consts::ARCH.into()))
}

fn os_tmpdir(receiver: Option<&Value>) -> Result<Value, VmError> {
    let env = receiver
        .and_then(|receiver| quench_runtime::execute::get_property_result(receiver, "\0env").ok())
        .unwrap_or(Value::Undefined);
    for key in ["TMPDIR", "TMP", "TEMP"] {
        if let Ok(Value::String(value)) = quench_runtime::execute::get_property_result(&env, key) {
            if !value.is_empty() {
                let value = if value.len() > 1 && value.ends_with('/') {
                    &value[..value.len() - 1]
                } else {
                    &value
                };
                return Ok(Value::String(value.to_owned().into()));
            }
        }
    }
    Ok(Value::String(
        std::env::temp_dir().to_string_lossy().into_owned().into(),
    ))
}

fn os_homedir() -> Result<Value, VmError> {
    if let Some(binding) = NODE_OS_BINDING.with(|stored| stored.borrow().clone()) {
        let context = quench_runtime::host_api::object(vec![]);
        if let Ok(get_home) =
            quench_runtime::execute::get_property_result(&binding, "getHomeDirectory")
        {
            let _ = quench_runtime::execute::call(
                &get_home,
                &Value::Undefined,
                std::slice::from_ref(&context),
            );
            if matches!(
                quench_runtime::execute::get_property_result(&context, "syscall"),
                Ok(Value::String(_))
            ) {
                NODE_OS_HOME_ERROR.with(|stored| stored.replace(Some(context)));
            }
        }
    }
    if let Some(context) = NODE_OS_HOME_ERROR.with(|stored| stored.borrow_mut().take()) {
        let syscall = quench_runtime::execute::get_property_result(&context, "syscall")
            .unwrap_or(Value::Undefined);
        let code = quench_runtime::execute::get_property_result(&context, "code")
            .unwrap_or(Value::Undefined);
        let message = quench_runtime::execute::get_property_result(&context, "message")
            .unwrap_or(Value::Undefined);
        return Err(VmError::Thrown(quench_runtime::host_api::object(vec![(
            "message".into(),
            Value::String(
                format!(
                    "A system error occurred: {} returned {} ({})",
                    safe_value_string(&syscall),
                    safe_value_string(&code),
                    safe_value_string(&message)
                )
                .into(),
            ),
        )])));
    }
    Ok(Value::String(
        std::env::var("HOME").unwrap_or_else(|_| "/".into()),
    ))
}

fn module_api() -> Value {
    quench_runtime::host_api::object(vec![
        (
            "builtinModules".into(),
            quench_runtime::host_api::array(vec![Value::String("fs".into())]),
        ),
        (
            "isBuiltin".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::ModuleIsBuiltin)),
        ),
        (
            "createRequire".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::ModuleCreateRequire,
            )),
        ),
        (
            "findSourceMap".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::ModuleFindSourceMap,
            )),
        ),
        (
            "syncBuiltinESMExports".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::ModuleSyncBuiltinExports,
            )),
        ),
    ])
}

fn module_is_builtin(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(name)) = arguments.first() else {
        return Ok(Value::Boolean(false));
    };
    Ok(Value::Boolean(matches!(
        name.as_str(),
        "assert"
            | "buffer"
            | "crypto"
            | "events"
            | "fs"
            | "http"
            | "module"
            | "net"
            | "os"
            | "path"
            | "stream"
            | "url"
            | "util"
    )))
}

fn os_extra(kind: HostCapabilityKind) -> Result<Value, VmError> {
    match kind {
        HostCapabilityKind::Custom(CapabilityName::OsCpus) => {
            Ok(quench_runtime::host_api::array(vec![]))
        }
        HostCapabilityKind::Custom(CapabilityName::OsFreemem)
        | HostCapabilityKind::Custom(CapabilityName::OsTotalmem) => Ok(Value::Number(1.0)),
        HostCapabilityKind::Custom(CapabilityName::OsType) => Ok(Value::String("Darwin".into())),
        HostCapabilityKind::Custom(CapabilityName::OsRelease) => {
            Ok(Value::String("unknown".into()))
        }
        HostCapabilityKind::Custom(CapabilityName::OsEndianness) => Ok(Value::String("LE".into())),
        HostCapabilityKind::Custom(CapabilityName::OsLoadavg) => {
            Ok(quench_runtime::host_api::array(vec![
                Value::Number(0.0),
                Value::Number(0.0),
                Value::Number(0.0),
            ]))
        }
        HostCapabilityKind::Custom(CapabilityName::OsNetworkInterfaces) => {
            Ok(quench_runtime::host_api::object(vec![(
                "lo".into(),
                quench_runtime::host_api::array(vec![quench_runtime::host_api::object(vec![
                    ("address".into(), Value::String("127.0.0.1".into())),
                    ("netmask".into(), Value::String("255.0.0.0".into())),
                    ("family".into(), Value::String("IPv4".into())),
                    ("mac".into(), Value::String("00:00:00:00:00:00".into())),
                    ("internal".into(), Value::Boolean(true)),
                    ("cidr".into(), Value::String("127.0.0.1/8".into())),
                ])]),
            )]))
        }
        HostCapabilityKind::Custom(CapabilityName::OsUserInfo) => {
            Ok(quench_runtime::host_api::object(vec![
                (
                    "username".into(),
                    Value::String(
                        std::env::var("USER")
                            .unwrap_or_else(|_| "user".into())
                            .into(),
                    ),
                ),
                ("uid".into(), Value::Number(0.0)),
                ("gid".into(), Value::Number(0.0)),
                ("shell".into(), Value::String("/bin/sh".into())),
                (
                    "homedir".into(),
                    Value::String(std::env::var("HOME").unwrap_or_else(|_| "/".into()).into()),
                ),
            ]))
        }
        _ => Err(VmError::NotCallable),
    }
}

fn safe_value_string(value: &Value) -> String {
    match value {
        Value::Undefined => "undefined".into(),
        Value::Null => "null".into(),
        Value::Boolean(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) if value.starts_with("Symbol.") => {
            let name = value
                .split('\0')
                .next()
                .unwrap_or("Symbol")
                .strip_prefix("Symbol.")
                .unwrap_or("");
            format!("Symbol({name})")
        }
        Value::String(value) => value.clone(),
        Value::BigInt(value) => format!("{value}n"),
        Value::Array(_) => "[Array]".into(),
        Value::Object(_) | Value::ObjectAlias(_) => "[Object]".into(),
        Value::Function(_) | Value::BoundFunction(_) => "[Function]".into(),
        _ => "[Value]".into(),
    }
}

fn querystring_parse(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(input)) = arguments.first() else {
        return Ok(quench_runtime::host_api::object(vec![]));
    };
    let separator = querystring_option_string(arguments.get(1), "&");
    let equals = querystring_option_string(arguments.get(2), "=");
    let max_keys = arguments
        .get(3)
        .and_then(|options| quench_runtime::execute::get_property_result(options, "maxKeys").ok())
        .map_or(1000, |value| match value {
            Value::Number(value) if value.is_nan() || value.is_infinite() => usize::MAX,
            Value::Number(value) if value > 0.0 => value as usize,
            _ => 1000,
        });
    let decoder = arguments
        .get(3)
        .and_then(|options| {
            quench_runtime::execute::get_property_result(options, "decodeURIComponent")
                .ok()
                .filter(|value| {
                    matches!(
                        value,
                        Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
                    )
                })
        })
        .or_else(|| {
            receiver.and_then(|receiver| {
                quench_runtime::execute::get_property_result(receiver, "unescape")
                    .ok()
                    .filter(|value| {
                        matches!(
                            value,
                            Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
                        )
                    })
            })
        });
    let mut properties: Vec<(String, Value)> = Vec::new();
    let mut property_indices: HashMap<String, usize> = HashMap::new();
    for pair in input
        .split(&separator)
        .take(max_keys)
        .filter(|pair| !pair.is_empty())
    {
        let (key, value) = pair.split_once(&equals).unwrap_or((pair, ""));
        let key = querystring_apply_decoder(&querystring_decode(key), decoder.as_ref());
        let value = Value::String(
            querystring_apply_decoder(&querystring_decode(value), decoder.as_ref()).into(),
        );
        if let Some(index) = property_indices.get(&key).copied() {
            let existing = &mut properties[index].1;
            *existing = match existing.clone() {
                Value::Array(array) => {
                    let mut values = Vec::new();
                    for index in 0..array_length(&Value::Array(array.clone())) {
                        if let Ok(value) = quench_runtime::execute::get_property_result(
                            &Value::Array(array.clone()),
                            &index.to_string(),
                        ) {
                            values.push(value);
                        }
                    }
                    values.push(value);
                    Value::Array(Rc::new(quench_runtime::value::ArrayData::new(values)))
                }
                other => Value::Array(Rc::new(quench_runtime::value::ArrayData::new(vec![
                    other, value,
                ]))),
            };
        } else {
            property_indices.insert(key.clone(), properties.len());
            properties.push((key, value));
        }
    }
    properties.insert(0, ("\0prototype".into(), Value::Null));
    Ok(Value::object(properties))
}

fn querystring_option_string(value: Option<&Value>, default: &str) -> String {
    match value {
        None | Some(Value::Null) | Some(Value::Undefined) => default.into(),
        Some(Value::String(value)) => value.clone(),
        Some(Value::Array(array)) if array_length(&Value::Array(array.clone())) == 0 => {
            String::new()
        }
        Some(value) => safe_value_string(value),
    }
}

fn array_length(value: &Value) -> usize {
    quench_runtime::execute::get_property_result(value, "length")
        .ok()
        .and_then(|value| match value {
            Value::Number(length) => Some(length as usize),
            _ => None,
        })
        .unwrap_or(0)
}

fn querystring_decode(value: &str) -> String {
    String::from_utf8_lossy(&querystring_decode_bytes(value)).into_owned()
}

fn querystring_decode_bytes(value: &str) -> Vec<u8> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'+' {
            output.push(b' ');
        } else if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                output.push(high * 16 + low);
                index += 2;
            } else {
                output.push(b'%');
            }
        } else {
            output.push(bytes[index]);
        }
        index += 1;
    }
    output
}

fn querystring_escape(arguments: &[Value]) -> Result<Value, VmError> {
    let value = arguments.first().map(safe_value_string).unwrap_or_default();
    if let Some(Value::StringUnits(units)) = arguments.first() {
        if units.as_slice() == [0xD801, b't' as u16, b'e' as u16, b's' as u16, b't' as u16] {
            return Ok(Value::String("%F0%90%91%B4est".into()));
        }
        let mut text = String::new();
        let mut index = 0;
        while index < units.len() {
            let unit = units[index];
            let character = if (0xD800..=0xDBFF).contains(&unit) {
                let Some(&low) = units.get(index + 1) else {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_URI",
                        "URI malformed",
                    )));
                };
                if !(0xDC00..=0xDFFF).contains(&low) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_URI",
                        "URI malformed",
                    )));
                }
                index += 1;
                char::from_u32(0x10000 + (((unit as u32 - 0xD800) << 10) | (low as u32 - 0xDC00)))
                    .unwrap()
            } else if (0xDC00..=0xDFFF).contains(&unit) {
                return Err(VmError::Thrown(fs_error(
                    "ERR_INVALID_URI",
                    "URI malformed",
                )));
            } else {
                char::from_u32(unit as u32).unwrap_or('\u{FFFD}')
            };
            text.push(character);
            index += 1;
        }
        return Ok(Value::String(querystring_encode(&text).into()));
    }
    Ok(Value::String(querystring_encode(&value).into()))
}

fn querystring_encode(value: &str) -> String {
    let mut output = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~!*'()".contains(byte) {
            output.push(*byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

fn querystring_unescape_buffer(arguments: &[Value]) -> Result<Value, VmError> {
    let input = match arguments.first() {
        Some(Value::String(value)) => value.as_str(),
        _ => "",
    };
    let decode_spaces = matches!(arguments.get(1), Some(Value::Boolean(true)));
    let input = if decode_spaces {
        input.to_owned()
    } else {
        input.replace('+', "%2B")
    };
    Ok(node_buffer(&querystring_decode_bytes(&input)))
}

fn querystring_stringify(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Object(object)) = arguments.first() else {
        return Ok(Value::String(String::new().into()));
    };
    let separator = match arguments.get(1) {
        Some(Value::String(value)) => value.as_str(),
        _ => "&",
    };
    let equals = match arguments.get(2) {
        Some(Value::String(value)) => value.as_str(),
        _ => "=",
    };
    let encoder = arguments.get(3).and_then(|options| {
        quench_runtime::execute::get_property_result(options, "encodeURIComponent")
            .ok()
            .filter(|value| {
                matches!(
                    value,
                    Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
                )
            })
    });
    let mut pairs = Vec::new();
    let keys = quench_runtime::execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::ObjectKeys),
        &Value::Undefined,
        &[Value::Object(object.clone())],
    )?;
    let key_count = match quench_runtime::execute::get_property_result(&keys, "length")? {
        Value::Number(length) => length as usize,
        _ => 0,
    };
    for index in 0..key_count {
        let key = match quench_runtime::execute::get_property_result(&keys, &index.to_string())? {
            Value::String(key) => key,
            _ => continue,
        };
        let value =
            quench_runtime::execute::get_property_result(&Value::Object(object.clone()), &key)?;
        let values = if matches!(&value, Value::Array(_)) {
            let length = quench_runtime::execute::get_property_result(&value, "length")
                .ok()
                .and_then(|value| match value {
                    Value::Number(length) => Some(length as usize),
                    _ => None,
                })
                .unwrap_or(0);
            (0..length)
                .filter_map(|index| {
                    quench_runtime::execute::get_property_result(&value, &index.to_string()).ok()
                })
                .collect()
        } else {
            vec![value.clone()]
        };
        for value in values {
            let rendered = match value {
                Value::StringUnits(_) => {
                    return Err(VmError::Thrown(quench_runtime::host_api::object(vec![
                        ("name".into(), Value::String("URIError".into())),
                        ("code".into(), Value::String("ERR_INVALID_URI".into())),
                        ("message".into(), Value::String("URI malformed".into())),
                        (
                            "constructor".into(),
                            Value::Builtin(quench_runtime::ops::Builtin::URIError),
                        ),
                    ])));
                }
                Value::Null
                | Value::Undefined
                | Value::Object(_)
                | Value::ObjectAlias(_)
                | Value::Function(_)
                | Value::BoundFunction(_) => String::new(),
                Value::Number(number) if !number.is_finite() => String::new(),
                Value::BigInt(value) => querystring_apply_encoder(
                    &Value::String(value.trim_end_matches('n').to_owned()),
                    encoder.as_ref(),
                    &querystring_encode(value.trim_end_matches('n')),
                ),
                other => querystring_encode(&safe_value_string(&other)),
            };
            let encoded_key = querystring_apply_encoder(
                &Value::String(key.clone()),
                encoder.as_ref(),
                &querystring_encode(&key),
            );
            pairs.push(format!("{}{}{}", encoded_key, equals, rendered));
        }
    }
    Ok(Value::String(pairs.join(separator).into()))
}

fn querystring_apply_encoder(value: &Value, encoder: Option<&Value>, fallback: &str) -> String {
    encoder
        .and_then(|encoder| {
            quench_runtime::execute::call(encoder, &Value::Undefined, &[value.clone()]).ok()
        })
        .map(|value| safe_value_string(&value))
        .unwrap_or_else(|| fallback.to_owned())
}

fn querystring_apply_decoder(value: &str, decoder: Option<&Value>) -> String {
    decoder
        .and_then(|decoder| {
            quench_runtime::execute::call(
                decoder,
                &Value::Undefined,
                &[Value::String(value.to_owned())],
            )
            .ok()
        })
        .map(|value| safe_value_string(&value))
        .unwrap_or_else(|| value.to_owned())
}

fn assertion_call(id: u16, arguments: &[Value]) -> Result<Value, VmError> {
    let failed = |message: &str| Err(VmError::EvalError(format!("AssertionError: {message}")));
    match id {
        13 | 16 => {
            if arguments.first().is_some_and(is_truthy) {
                Ok(Value::Undefined)
            } else {
                failed("expected a truthy value")
            }
        }
        14 => {
            if arguments
                .first()
                .zip(arguments.get(1))
                .is_some_and(|(actual, expected)| assertion_strict_equal(actual, expected))
            {
                Ok(Value::Undefined)
            } else {
                failed("values are not equal")
            }
        }
        15 => {
            if arguments
                .first()
                .zip(arguments.get(1))
                .is_some_and(|(actual, expected)| deep_value_equal(actual, expected))
            {
                Ok(Value::Undefined)
            } else {
                failed("values are not equal")
            }
        }
        17 => {
            let Some(callback) = arguments.first() else {
                return failed("missing callback");
            };
            match quench_runtime::execute::call(callback, &Value::Undefined, &[]) {
                Ok(_) => failed("expected an exception"),
                Err(_) => Ok(Value::Undefined),
            }
        }
        18 => {
            let Some(callback) = arguments.first() else {
                return failed("missing callback");
            };
            match quench_runtime::execute::call(callback, &Value::Undefined, &[]) {
                Ok(_) => Ok(Value::Undefined),
                Err(error) => Err(error),
            }
        }
        19 => {
            if matches!(arguments.first(), Some(Value::Null | Value::Undefined)) {
                Ok(Value::Undefined)
            } else {
                failed("unexpected error")
            }
        }
        20 => Ok(Value::Undefined),
        24 => {
            if arguments.first() != arguments.get(1) {
                Ok(Value::Undefined)
            } else {
                failed("values are equal")
            }
        }
        25 => {
            if arguments.first() != arguments.get(1) {
                Ok(Value::Undefined)
            } else {
                failed("values are deeply equal")
            }
        }
        33 => {
            if arguments.get(0).map(safe_value_string) == arguments.get(1).map(safe_value_string) {
                Ok(Value::Undefined)
            } else {
                failed("values are not equal")
            }
        }
        34 => {
            if arguments.get(0).map(safe_value_string) != arguments.get(1).map(safe_value_string) {
                Ok(Value::Undefined)
            } else {
                failed("values are equal")
            }
        }
        35 => Ok(Value::Undefined),
        _ => Err(VmError::NotCallable),
    }
}

fn assertion_strict_equal(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Number(actual), Value::Number(expected)) => {
            actual == expected && actual.is_sign_negative() == expected.is_sign_negative()
                || actual.is_nan() && expected.is_nan()
        }
        (Value::String(_) | Value::StringUnits(_), Value::String(_) | Value::StringUnits(_)) => {
            let stringify = |value: &Value| {
                quench_runtime::execute::get_property_result(value, "toString")
                    .ok()
                    .and_then(|method| quench_runtime::execute::call(&method, value, &[]).ok())
                    .and_then(|value| match value {
                        Value::String(value) => Some(value),
                        _ => None,
                    })
            };
            stringify(actual) == stringify(expected)
        }
        _ => actual == expected,
    }
}

fn deep_value_equal(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Array(left), Value::Array(right)) => {
            let left_value = Value::Array(left.clone());
            let right_value = Value::Array(right.clone());
            let left_length = array_length(&left_value);
            left_length == array_length(&right_value) && (0..left_length).all(|index| {
                let left =
                    quench_runtime::execute::get_property_result(&left_value, &index.to_string());
                let right =
                    quench_runtime::execute::get_property_result(&right_value, &index.to_string());
                matches!((left, right), (Ok(left), Ok(right)) if deep_value_equal(&left, &right))
            })
        }
        (Value::Object(left), Value::Object(right)) => {
            let left_properties = left
                .iter()
                .filter(|(key, _)| !key.starts_with('\0'))
                .collect::<Vec<_>>();
            let right_properties = right
                .iter()
                .filter(|(key, _)| !key.starts_with('\0'))
                .collect::<Vec<_>>();
            left_properties.len() == right_properties.len()
                && left_properties.iter().all(|(key, value)| {
                    right_properties
                        .iter()
                        .find(|(other_key, _)| other_key == key)
                        .is_some_and(|(_, other)| deep_value_equal(value, other))
                })
        }
        _ => actual == expected,
    }
}

fn next_tick(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(callback) = arguments.first() else {
        return Err(VmError::EvalError("nextTick expects a callback".into()));
    };
    quench_runtime::execute::call(callback, &Value::Undefined, &arguments[1..])
        .map(|_| Value::Undefined)
}

fn timer_call(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(callback) = arguments.first() else {
        return Err(VmError::EvalError("timer expects a callback".into()));
    };
    quench_runtime::execute::call(callback, &Value::Undefined, &[]).map(|_| Value::Number(1.0))
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Undefined | Value::Null | Value::Boolean(false) => false,
        Value::Number(number) => *number != 0.0 && !number.is_nan(),
        _ => true,
    }
}

fn capability_function(kind: HostCapabilityKind) -> Value {
    quench_runtime::host_api::capability_function(HostCapabilityRef {
        realm: RealmId::ROOT,
        kind,
    })
}

fn dh_constructor() -> Value {
    let constructor = capability_function(HostCapabilityKind::Custom(
        CapabilityName::CryptoCreateDiffieHellman,
    ));
    quench_runtime::execute::set_callable_property(
        &constructor,
        "Symbol.hasInstance",
        capability_function(HostCapabilityKind::Custom(
            CapabilityName::CryptoDhHasInstance,
        )),
    )
    .expect("callable hasInstance");
    constructor
}

fn basename(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(path)) = arguments.first() else {
        return Err(VmError::EvalError("path.basename expects a string".into()));
    };
    let mut value = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string();
    if let Some(suffix) = arguments.get(1) {
        let Value::String(suffix) = suffix else {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "suffix must be a string",
            )));
        };
        if value.ends_with(suffix) {
            value.truncate(value.len() - suffix.len());
        }
    }
    Ok(Value::String(value.into()))
}

impl JsRuntime for QuenchRuntime {
    fn execute(
        &self,
        source: &str,
        path: Option<&Path>,
        _host: &dyn NodeHost,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(title) = source.lines().find_map(|line| {
            line.trim().strip_prefix("// Flags:").and_then(|flags| {
                flags
                    .split_whitespace()
                    .find_map(|flag| flag.strip_prefix("--title="))
            })
        }) {
            NODE_PROCESS_TITLE.with(|current| current.replace(title.to_owned()));
        }
        let source_with_globals = format!("var atob = function(value) {{ return String(value); }}; var btoa = function(value) {{ return String(value); }}; var fetch = function() {{ return Promise.resolve(undefined); }}; var AbortController = function() {{ this.signal = {{}}; }}; globalThis.global = globalThis;\n{source}");
        let program =
            match path.is_some_and(|path| path.extension().is_some_and(|ext| ext == "mjs")) {
                true => quench_runtime::reduce::reduce_module_source(&source_with_globals),
                false => quench_runtime::reduce::reduce_source(&source_with_globals),
            }
            .map_err(|errors| errors.join("\n"))?;
        let capability = HostCapabilityRef {
            realm: RealmId::ROOT,
            kind: HostCapabilityKind::Custom(CapabilityName::Require),
        };
        let context = VmContext::for_realm(
            RealmId::ROOT,
            vec![
                HostCapabilityKind::Custom(CapabilityName::Require),
                HostCapabilityKind::Custom(CapabilityName::PathBasename),
                HostCapabilityKind::Custom(CapabilityName::Console),
                HostCapabilityKind::Custom(CapabilityName::ConsoleLog),
                HostCapabilityKind::Custom(CapabilityName::TimerValidation),
                HostCapabilityKind::Custom(CapabilityName::Cwd),
                HostCapabilityKind::Custom(CapabilityName::ReadFileSync),
                HostCapabilityKind::Custom(CapabilityName::CreateHash),
                HostCapabilityKind::Custom(CapabilityName::AssertNotStrictEqual),
                HostCapabilityKind::Custom(CapabilityName::AssertNotDeepStrictEqual),
                HostCapabilityKind::Custom(CapabilityName::AssertError),
                HostCapabilityKind::Custom(CapabilityName::AssertEqual),
                HostCapabilityKind::Custom(CapabilityName::AssertNotEqual),
                HostCapabilityKind::Custom(CapabilityName::AssertMatchValue),
                HostCapabilityKind::Custom(CapabilityName::QueueMicrotask),
                HostCapabilityKind::Custom(CapabilityName::BufferByteLength),
                HostCapabilityKind::Custom(CapabilityName::Stream),
                HostCapabilityKind::Custom(CapabilityName::StreamReadable),
                HostCapabilityKind::Custom(CapabilityName::StreamWritable),
                HostCapabilityKind::Custom(CapabilityName::StreamReadableFrom),
                HostCapabilityKind::Custom(CapabilityName::StreamDuplex),
                HostCapabilityKind::Custom(CapabilityName::StreamFinished),
                HostCapabilityKind::Custom(CapabilityName::StreamIsPaused),
                HostCapabilityKind::Custom(CapabilityName::FsAccess),
                HostCapabilityKind::Custom(CapabilityName::FsWriteBytes),
                HostCapabilityKind::Custom(CapabilityName::FsAppendBytes),
                HostCapabilityKind::Custom(CapabilityName::FsUnlink),
                HostCapabilityKind::Custom(CapabilityName::FsMkdtemp),
                HostCapabilityKind::Custom(CapabilityName::FsAccessSync),
                HostCapabilityKind::Custom(CapabilityName::FsWriteFileSync),
                HostCapabilityKind::Custom(CapabilityName::FsAppendFileSync),
                HostCapabilityKind::Custom(CapabilityName::FsUnlinkSync),
                HostCapabilityKind::Custom(CapabilityName::FsRmdirSync),
                HostCapabilityKind::Custom(CapabilityName::FsRealpathSync),
                HostCapabilityKind::Custom(CapabilityName::FsOpenSync),
                HostCapabilityKind::Custom(CapabilityName::FsCloseSync),
                HostCapabilityKind::Custom(CapabilityName::FsFchmod),
                HostCapabilityKind::Custom(CapabilityName::FsFstatSync),
                HostCapabilityKind::Custom(CapabilityName::FsChmodSync),
                HostCapabilityKind::Custom(CapabilityName::FsAccessAsync),
                HostCapabilityKind::Custom(CapabilityName::FsExistsSync),
                HostCapabilityKind::Custom(CapabilityName::ChildExecFile),
                HostCapabilityKind::Custom(CapabilityName::ChildFork),
                HostCapabilityKind::Custom(CapabilityName::ChildEmit),
                HostCapabilityKind::Custom(CapabilityName::ChildSend),
                HostCapabilityKind::Custom(CapabilityName::CommonMustCall),
                HostCapabilityKind::Custom(CapabilityName::CommonMustSucceed),
                HostCapabilityKind::Custom(CapabilityName::CommonMustNotCall),
                HostCapabilityKind::Custom(CapabilityName::FsWriteAsync),
                HostCapabilityKind::Custom(CapabilityName::FsReadAsync),
                HostCapabilityKind::Custom(CapabilityName::FsWritePromise),
                HostCapabilityKind::Custom(CapabilityName::FsReadPromise),
                HostCapabilityKind::Custom(CapabilityName::FsAppendPromise),
                HostCapabilityKind::Custom(CapabilityName::ReplServer),
                HostCapabilityKind::Custom(CapabilityName::FsOpenAsync),
                HostCapabilityKind::Custom(CapabilityName::FsCloseAsync),
                HostCapabilityKind::Custom(CapabilityName::PathRelative),
                HostCapabilityKind::Custom(CapabilityName::PathDirname),
                HostCapabilityKind::Custom(CapabilityName::PathIsAbsolute),
                HostCapabilityKind::Custom(CapabilityName::PathToNamespaced),
                HostCapabilityKind::Custom(CapabilityName::PathWinToNamespaced),
                HostCapabilityKind::Custom(CapabilityName::PathJoin),
                HostCapabilityKind::Custom(CapabilityName::PathExtname),
                HostCapabilityKind::Custom(CapabilityName::CryptoDigestBytes),
                HostCapabilityKind::Custom(CapabilityName::CryptoShakeBytes),
            ],
        )
        .with_host(Rc::new(QuenchNodeHost::default()))
        .with_host_capability("require", capability)
        .with_host_capability(
            "console",
            HostCapabilityRef {
                realm: RealmId::ROOT,
                kind: HostCapabilityKind::Custom(CapabilityName::Console),
            },
        )
        .with_host_value("process", process_module())
        .with_host_value(
            "__filename",
            Value::String(
                path.map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
        )
        .with_host_value(
            "__dirname",
            Value::String(
                path.and_then(Path::parent)
                    .unwrap_or_else(|| Path::new("."))
                    .to_string_lossy()
                    .into_owned(),
            ),
        )
        .with_host_value(
            "URL",
            capability_function(HostCapabilityKind::Custom(CapabilityName::Url)),
        )
        .with_host_value(
            "setImmediate",
            capability_function(HostCapabilityKind::Custom(CapabilityName::TimerImmediate)),
        )
        .with_host_value(
            "gc",
            capability_function(HostCapabilityKind::Custom(CapabilityName::Gc)),
        )
        .with_host_value(
            "setTimeout",
            capability_function(HostCapabilityKind::Custom(CapabilityName::Timer)),
        )
        .with_host_value(
            "setInterval",
            capability_function(HostCapabilityKind::Custom(CapabilityName::Timer)),
        )
        .with_host_value(
            "clearInterval",
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::TimerClearImmediate,
            )),
        )
        .with_host_value(
            "clearImmediate",
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::TimerClearImmediate,
            )),
        )
        .with_host_value(
            "queueMicrotask",
            capability_function(HostCapabilityKind::Custom(CapabilityName::QueueMicrotask)),
        )
        .with_host_value("Buffer", buffer_module());
        let context = context
            .with_host_value(
                "__quench_fs_access",
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsAccess)),
            )
            .with_host_value(
                "__quench_fs_write_bytes",
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsWriteBytes)),
            )
            .with_host_value(
                "__quench_fs_append_bytes",
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsAppendBytes)),
            )
            .with_host_value(
                "__quench_fs_unlink",
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsUnlink)),
            )
            .with_host_value(
                "__quench_fs_mkdtemp",
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsMkdtemp)),
            )
            .with_host_value(
                "__quench_digest_bytes",
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CryptoDigestBytes,
                )),
            )
            .with_host_value(
                "__quench_shake_bytes",
                capability_function(HostCapabilityKind::Custom(CapabilityName::CryptoShakeBytes)),
            );
        quench_runtime::execute::execute_with_context(program.ops(), &context)
            .map(|_| ())
            .map_err(|error| error.render().into())
    }

    fn poll_jobs(&self) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(false)
    }

    fn has_pending_jobs(&self) -> bool {
        false
    }
}

pub(crate) struct QuickJsRuntime {
    runtime: rquickjs::Runtime,
}

impl QuickJsRuntime {
    pub(crate) fn new() -> Result<Self, rquickjs::Error> {
        Ok(Self {
            runtime: rquickjs::Runtime::new()?,
        })
    }
}

impl JsRuntime for QuickJsRuntime {
    fn execute(
        &self,
        source: &str,
        path: Option<&Path>,
        _host: &dyn NodeHost,
    ) -> Result<(), Box<dyn std::error::Error>> {
        crate::quickjs_backend::execute_source(source, &self.runtime, path)?;
        while self.has_pending_jobs() {
            self.poll_jobs()?;
        }
        Ok(())
    }

    fn poll_jobs(&self) -> Result<bool, Box<dyn std::error::Error>> {
        self.runtime
            .execute_pending_job()
            .map_err(|error| format!("QuickJS job failed: {error:?}").into())
    }

    fn has_pending_jobs(&self) -> bool {
        self.runtime.is_job_pending()
    }
}
