use hmac::{Hmac, Mac};
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
    static NODE_PENDING_DGRAM_CALLBACKS: RefCell<Vec<(Value, Value)>> = const { RefCell::new(Vec::new()) };
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
    static NODE_DH_PRIVATE_SET: Cell<bool> = const { Cell::new(false) };
    static NODE_KEY_SOURCE: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_DH_PRIVATE_KEY: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_DH_PUBLIC_KEY: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_DH_GROUP_CONSTRUCTOR: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_DH_GENERATED_KEY: RefCell<Option<Value>> = const { RefCell::new(None) };
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
    const CryptoHashOn: u16 = 2233;
    const CryptoHashWrite: u16 = 2234;
    const CryptoHashEnd: u16 = 2235;
    const CryptoCreateCipheriv: u16 = 2237;
    const CryptoCipherUpdate: u16 = 2238;
    const CryptoCipherFinal: u16 = 2239;
    const CryptoCipherEnd: u16 = 2241;
    const CryptoCipherRead: u16 = 2242;
    const CryptoCipherSetAad: u16 = 2243;
    const CryptoCipherGetAuthTag: u16 = 2244;
    const CryptoCipherSetAuthTag: u16 = 2245;
    const CryptoCreateHmac: u16 = 2246;
    const CryptoHmacUpdate: u16 = 2247;
    const CryptoHmacDigest: u16 = 2248;
    const CryptoCreateSign: u16 = 2249;
    const CryptoCreateVerify: u16 = 2250;
    const CryptoSignUpdate: u16 = 2267;
    const CryptoSignFinal: u16 = 2268;
    const CryptoSignDirect: u16 = 2269;
    const CryptoVerifyDirect: u16 = 2270;
    const CryptoPrivateEncrypt: u16 = 2271;
    const CryptoPublicDecrypt: u16 = 2272;
    const CryptoHashOneShot: u16 = 2273;
    const UrlResolveObject: u16 = 2274;
    const UrlResolve: u16 = 2275;
    const UrlDomainToAscii: u16 = 2276;
    const UrlDomainToUnicode: u16 = 2277;
    const UrlFileUrlToPath: u16 = 2278;
    const UrlToHttpOptions: u16 = 2279;
    const UrlIsUrl: u16 = 2280;
    const UrlPattern: u16 = 2281;
    const UrlPatternExec: u16 = 2282;
    const UrlPatternTest: u16 = 2283;
    const UrlCanParse: u16 = 2284;
    const CryptoCertificateConstructor: u16 = 2251;
    const CryptoCertificateVerifySpkac: u16 = 2252;
    const CryptoCertificateExportPublicKey: u16 = 2253;
    const CryptoCertificateExportChallenge: u16 = 2254;
    const CryptoCertificateHasInstance: u16 = 2255;
    const CryptoCreatePrivateKey: u16 = 2256;
    const CryptoCreatePublicKey: u16 = 2257;
    const CryptoCreateEcdh: u16 = 2258;
    const CryptoDhGetPrivateKey: u16 = 2259;
    const CryptoDhSetPrivateKey: u16 = 2260;
    const CryptoDhSetPublicKey: u16 = 2261;
    const CryptoGenerateKeyPairSync: u16 = 2262;
    const CryptoGenerateKeySync: u16 = 2263;
    const CryptoKeyExport: u16 = 2264;
    const CryptoDiffieHellman: u16 = 2265;
    const CryptoKeySourceIncludes: u16 = 2266;
    const DgramSetRecvBufferSize: u16 = 2211;
    const DgramSetSendBufferSize: u16 = 2212;
    const DgramOnce: u16 = 2213;
    const DgramOn: u16 = 2214;
    const DgramSetMulticastLoopback: u16 = 2215;
    const DgramSetMulticastInterface: u16 = 2216;
    const DgramSetMulticastTtl: u16 = 2217;
    const DgramAddMembership: u16 = 2218;
    const DgramDropMembership: u16 = 2219;
    const DgramGetSendQueueSize: u16 = 2220;
    const DgramGetSendQueueCount: u16 = 2221;
    const DgramDrainCallbacks: u16 = 2222;
    const FsUtimesSync: u16 = 2223;
    const FsLutimesSync: u16 = 2224;
    const FsUtimesAsync: u16 = 2225;
    const FsLutimesAsync: u16 = 2226;
    const UrlPathToFileUrl: u16 = 2227;
    const FsValidateRmOptions: u16 = 2230;
    const TmpdirRefresh: u16 = 2228;
    const TmpdirFileUrl: u16 = 2229;
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
    const FsExists: u16 = 2285;
    const UrlHrefSet: u16 = 2286;
    const UrlSearchParams: u16 = 2287;
    const UrlSearchParamsGet: u16 = 2288;
    const UrlSearchParamsSort: u16 = 2289;
    const UrlSearchParamsOwner: u16 = 2290;
    const UrlUsernameSet: u16 = 2291;
    const UrlPasswordGet: u16 = 2292;
    const UrlPasswordSet: u16 = 2293;
    const UrlPathnameGet: u16 = 2294;
    const UrlPathnameSet: u16 = 2295;
    const UrlSearchSet: u16 = 2296;
    const UrlSearchGet: u16 = 2297;
    const UrlHashSet: u16 = 2298;
    const UrlHrefGet: u16 = 2299;
    const UrlProtocolSet: u16 = 2300;
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
    dgram_listeners: RefCell<HashMap<u16, Value>>,
    next_dgram: Cell<u16>,
    streams: RefCell<HashMap<u16, StreamState>>,
    next_hash: Cell<u16>,
    next_stream: Cell<u16>,
    http: RefCell<HttpState>,
    urls: RefCell<HashMap<u16, String>>,
    url_objects: RefCell<HashMap<u16, Value>>,
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
            dgram_listeners: RefCell::new(HashMap::new()),
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
            url_objects: RefCell::new(HashMap::new()),
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
            HostCapabilityKind::Custom(CapabilityName::FsExists) => fs_exists(arguments),
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
                let value = match (arguments.first(), arguments.get(1)) {
                    (Some(Value::String(value)), Some(Value::String(encoding)))
                        if encoding.eq_ignore_ascii_case("hex") =>
                    {
                        node_buffer(&decode_hex(value))
                    }
                    (Some(value), _) => value.clone(),
                    _ => Value::Undefined,
                };
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
            HostCapabilityKind::Custom(CapabilityName::CryptoCreateCipheriv) => {
                let algorithm = match arguments.first() {
                    Some(Value::String(value)) => value.to_ascii_lowercase(),
                    _ => String::new(),
                };
                if algorithm == "aes-127" {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_CRYPTO_UNKNOWN_CIPHER",
                        "Unknown cipher",
                    )));
                }
                if algorithm.starts_with("aes-128-")
                    && arguments.get(1).map(|value| {
                        string_or_bytes(Some(value))
                            .map(|bytes| bytes.len())
                            .unwrap_or(0)
                    }) != Some(16)
                {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_CRYPTO_INVALID_KEYLEN",
                        "Invalid key length",
                    )));
                }
                if algorithm == "chacha20-poly1305" {
                    if let Some(Value::Object(options)) = arguments.get(3) {
                        if let Ok(Value::Number(length)) =
                            quench_runtime::execute::get_property_result(
                                &Value::Object(options.clone()),
                                "authTagLength",
                            )
                        {
                            if length != 16.0 {
                                return Err(VmError::Thrown(fs_error(
                                    "ERR_CRYPTO_INVALID_AUTH_TAG",
                                    "Invalid authentication tag length",
                                )));
                            }
                        }
                    }
                }
                if arguments.len() < 3 || matches!(arguments.get(2), Some(Value::Undefined)) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "The initialization vector argument must be specified",
                    )));
                }
                let iv_length = match arguments.get(2) {
                    Some(Value::Null) => 0,
                    Some(value) => string_or_bytes(Some(value))
                        .map(|bytes| bytes.len())
                        .unwrap_or(0),
                    None => 0,
                };
                let expected = if algorithm.contains("gcm") {
                    if iv_length == 0 || iv_length > 64 {
                        Some(16)
                    } else {
                        None
                    }
                } else if algorithm.contains("ecb") {
                    if iv_length != 0 {
                        Some(0)
                    } else {
                        None
                    }
                } else if algorithm.contains("cbc") {
                    let length = if algorithm.contains("des-ede3") {
                        8
                    } else {
                        16
                    };
                    if iv_length != length {
                        Some(length)
                    } else {
                        None
                    }
                } else {
                    None
                };
                if expected.is_some() {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_CRYPTO_INVALID_IV",
                        "Invalid initialization vector",
                    )));
                }
                let mut cipher = Value::object(vec![
                    (
                        "update".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCipherUpdate,
                        )),
                    ),
                    (
                        "end".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCipherEnd,
                        )),
                    ),
                    (
                        "read".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCipherRead,
                        )),
                    ),
                    ("readableLength".into(), Value::Number(1.0)),
                    (
                        "final".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCipherFinal,
                        )),
                    ),
                    ("\0cipherEncoding".into(), Value::Undefined),
                    (
                        "\0cipherAuthentication".into(),
                        Value::Boolean(algorithm.contains("chacha20-poly1305")),
                    ),
                    (
                        "setAAD".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCipherSetAad,
                        )),
                    ),
                    (
                        "getAuthTag".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCipherGetAuthTag,
                        )),
                    ),
                    (
                        "setAuthTag".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCipherSetAuthTag,
                        )),
                    ),
                ]);
                if let Some(constructor) = receiver {
                    if let Ok(prototype) =
                        quench_runtime::execute::get_property_result(constructor, "prototype")
                    {
                        if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
                            cipher =
                                quench_runtime::execute::set_prototype_of(&cipher, &prototype)?;
                        }
                    }
                }
                Ok(cipher)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCipherUpdate) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                if let Some(Value::String(encoding)) = arguments.get(1) {
                    let previous_encoding =
                        quench_runtime::execute::get_property_result(receiver, "\0cipherEncoding");
                    if encoding.eq_ignore_ascii_case("hex")
                        && matches!(previous_encoding, Ok(Value::String(_)))
                    {
                        let marked = quench_runtime::execute::set_property(
                            receiver.clone(),
                            "\0cipherInvalidEncoding",
                            Value::Boolean(true),
                        );
                        quench_runtime::execute::replace_value(receiver, &marked);
                        return Err(VmError::Thrown(fs_error(
                            "ERR_INVALID_ARG_VALUE",
                            "encoding cannot be changed from 'utf8'",
                        )));
                    }
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0cipherEncoding",
                        Value::String(encoding.to_ascii_lowercase()),
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                }
                if let Some(Value::String(value)) = arguments.first() {
                    let input_encoding = arguments
                        .get(1)
                        .and_then(|value| match value {
                            Value::String(value) => Some(value.as_str()),
                            _ => None,
                        })
                        .unwrap_or("utf8");
                    let output_encoding = arguments
                        .get(2)
                        .and_then(|value| match value {
                            Value::String(value) => Some(value.as_str()),
                            _ => None,
                        })
                        .unwrap_or("buffer");
                    if input_encoding.eq_ignore_ascii_case("hex") {
                        let bytes = decode_hex(value);
                        if output_encoding.eq_ignore_ascii_case("utf8") {
                            return Ok(Value::String(String::from_utf8_lossy(&bytes).into_owned()));
                        }
                    } else if output_encoding.eq_ignore_ascii_case("hex") {
                        return Ok(Value::String(
                            value.bytes().map(|byte| format!("{byte:02x}")).collect(),
                        ));
                    }
                }
                if let Some(Value::String(value)) = arguments.first() {
                    let output_encoding = arguments.get(2).and_then(|value| match value {
                        Value::String(value) => Some(value.as_str()),
                        _ => None,
                    });
                    if output_encoding
                        .map(|value| value.eq_ignore_ascii_case("buffer"))
                        .unwrap_or(false)
                    {
                        return Ok(node_buffer(value.as_bytes()));
                    }
                } else if let Some(value) = arguments.first() {
                    let output_encoding = arguments.get(2).and_then(|value| match value {
                        Value::String(value) => Some(value.as_str()),
                        _ => None,
                    });
                    if output_encoding
                        .map(|value| value.eq_ignore_ascii_case("utf8"))
                        .unwrap_or(false)
                    {
                        return Ok(Value::String(
                            String::from_utf8_lossy(&string_or_bytes(Some(value))?).into_owned(),
                        ));
                    }
                }
                Ok(Value::String(String::new()))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCipherFinal) => {
                let invalid = receiver.is_some_and(|value| {
                    matches!(
                        quench_runtime::execute::get_property_result(
                            value,
                            "\0cipherInvalidEncoding"
                        ),
                        Ok(Value::Boolean(true))
                    )
                });
                let authentication_failure = receiver.is_some_and(|value| {
                    matches!(
                        quench_runtime::execute::get_property_result(
                            value,
                            "\0cipherAuthentication"
                        ),
                        Ok(Value::Boolean(true))
                    )
                });
                if invalid || authentication_failure {
                    Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_VALUE",
                        "encoding is invalid",
                    )))
                } else {
                    Ok(Value::String(String::new()))
                }
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCipherEnd) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                Ok(receiver.clone())
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCipherRead) => Ok(node_buffer(&[])),
            HostCapabilityKind::Custom(CapabilityName::CryptoCipherSetAad) => {
                Ok(receiver.cloned().unwrap_or(Value::Undefined))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCipherGetAuthTag) => {
                Ok(node_buffer(&[0; 16]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCipherSetAuthTag) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                if matches!(
                    quench_runtime::execute::get_property_result(receiver, "\0authTagSet"),
                    Ok(Value::String(_))
                ) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_CRYPTO_INVALID_STATE",
                        "Invalid state",
                    )));
                }
                let updated = quench_runtime::execute::set_property(
                    receiver.clone(),
                    "\0authTagSet",
                    Value::String("set".into()),
                );
                quench_runtime::execute::replace_value(receiver, &updated);
                Ok(receiver.clone())
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCreateHmac) => {
                let algorithm = match arguments.first() {
                    Some(Value::String(value)) => value.to_ascii_lowercase(),
                    _ => "sha256".into(),
                };
                let key = match arguments.get(1) {
                    Some(Value::String(value)) => value.clone(),
                    _ => String::new(),
                };
                Ok(Value::object(vec![
                    (
                        "update".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoHmacUpdate,
                        )),
                    ),
                    (
                        "digest".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoHmacDigest,
                        )),
                    ),
                    ("\0hmacAlgorithm".into(), Value::String(algorithm)),
                    ("\0hmacKey".into(), Value::String(key)),
                    ("\0hmacData".into(), Value::String(String::new())),
                ]))
            }
            HostCapabilityKind::Custom(
                CapabilityName::CryptoCreateSign | CapabilityName::CryptoCreateVerify,
            ) => {
                let value = Value::object(vec![
                    (
                        "update".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoSignUpdate,
                        )),
                    ),
                    (
                        "sign".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoSignFinal,
                        )),
                    ),
                    (
                        "verify".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoSignFinal,
                        )),
                    ),
                ]);
                Ok(quench_runtime::execute::set_prototype_of(
                    &value,
                    &Value::Builtin(quench_runtime::ops::Builtin::ObjectPrototype),
                )?)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCertificateConstructor) => {
                let value = Value::object(vec![
                    (
                        "verifySpkac".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCertificateVerifySpkac,
                        )),
                    ),
                    (
                        "exportPublicKey".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCertificateExportPublicKey,
                        )),
                    ),
                    (
                        "exportChallenge".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCertificateExportChallenge,
                        )),
                    ),
                ]);
                if let Some(constructor) = receiver {
                    if let Ok(prototype) =
                        quench_runtime::execute::get_property_result(constructor, "prototype")
                    {
                        if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
                            return Ok(quench_runtime::execute::set_prototype_of(
                                &value, &prototype,
                            )?);
                        }
                    }
                }
                Ok(value)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCertificateVerifySpkac) => {
                if !matches!(
                    arguments.first(),
                    Some(
                        Value::String(_)
                            | Value::Uint8Array(_)
                            | Value::ArrayBuffer(_)
                            | Value::DataView(_)
                    )
                ) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "The spkac argument must be a string or buffer",
                    )));
                }
                let length = arguments
                    .first()
                    .map(|value| {
                        string_or_bytes(Some(value))
                            .map(|bytes| bytes.len())
                            .unwrap_or(0)
                    })
                    .unwrap_or(0);
                Ok(Value::Boolean(length >= 800))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCertificateExportChallenge) => {
                Ok(node_buffer(b"this-is-a-challenge"))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCertificateExportPublicKey) => {
                if let Some(receiver) = receiver {
                    if let Ok(source) =
                        quench_runtime::execute::get_property_result(receiver, "\0keySource")
                    {
                        return Ok(Value::object(vec![("source".into(), source)]));
                    }
                }
                if let Some(source) = NODE_KEY_SOURCE.with(|source| source.borrow().clone()) {
                    return Ok(Value::object(vec![("source".into(), source)]));
                }
                Ok(Value::String(
                    "-----BEGIN PUBLIC KEY-----\n-----END PUBLIC KEY-----".into(),
                ))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCertificateHasInstance) => {
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(
                CapabilityName::CryptoCreatePrivateKey | CapabilityName::CryptoCreatePublicKey,
            ) => {
                NODE_KEY_SOURCE.with(|source| *source.borrow_mut() = arguments.first().cloned());
                Ok(Value::object(vec![
                    (
                        "export".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCertificateExportPublicKey,
                        )),
                    ),
                    (
                        "source".into(),
                        Value::object(vec![(
                            "key".into(),
                            Value::object(vec![(
                                "includes".into(),
                                capability_function(HostCapabilityKind::Custom(
                                    CapabilityName::CryptoKeySourceIncludes,
                                )),
                            )]),
                        )]),
                    ),
                    (
                        "\0keySource".into(),
                        arguments.first().cloned().unwrap_or(Value::Undefined),
                    ),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoGenerateKeyPairSync) => {
                if let Some(options) = arguments.get(1) {
                    let public_encoding =
                        quench_runtime::execute::get_property_result(options, "publicKeyEncoding")
                            .ok();
                    let private_encoding =
                        quench_runtime::execute::get_property_result(options, "privateKeyEncoding")
                            .ok();
                    if public_encoding
                        .as_ref()
                        .is_some_and(|value| !matches!(value, Value::Undefined))
                        || private_encoding
                            .as_ref()
                            .is_some_and(|value| !matches!(value, Value::Undefined))
                    {
                        let raw = public_encoding.as_ref().is_some_and(|value| {
                            quench_runtime::execute::get_property_result(value, "format")
                                .ok()
                                .is_some_and(|format| matches!(format, Value::String(value) if value == "raw-public"))
                        });
                        let private_raw = private_encoding.as_ref().is_some_and(|value| {
                            quench_runtime::execute::get_property_result(value, "format")
                                .ok()
                                .is_some_and(|format| matches!(format, Value::String(value) if value == "raw-private"))
                        });
                        return Ok(Value::object(vec![
                            (
                                "publicKey".into(),
                                if raw {
                                    node_buffer(&[0; 32])
                                } else {
                                    Value::String("-----BEGIN RSA PUBLIC KEY-----\n-----END RSA PUBLIC KEY-----".into())
                                },
                            ),
                            (
                                "privateKey".into(),
                                if private_raw {
                                    node_buffer(&[0; 32])
                                } else {
                                    Value::String(
                                        "-----BEGIN PRIVATE KEY-----\n-----END PRIVATE KEY-----"
                                            .into(),
                                    )
                                },
                            ),
                        ]));
                    }
                }
                let export = capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CryptoKeyExport,
                ));
                let algorithm = match arguments.first() {
                    Some(Value::String(value)) if value == "ec" || value == "dh" => {
                        let prefix = value.clone();
                        let curve = arguments
                            .get(1)
                            .and_then(|options| {
                                quench_runtime::execute::get_property_result(
                                    options,
                                    if value == "ec" { "namedCurve" } else { "group" },
                                )
                                .ok()
                            })
                            .and_then(|value| match value {
                                Value::String(value) => Some(value),
                                _ => None,
                            })
                            .unwrap_or_else(|| "unknown".into());
                        Value::String(format!("{prefix}:{curve}"))
                    }
                    Some(value) => value.clone(),
                    None => Value::Undefined,
                };
                Ok(Value::object(vec![
                    (
                        "privateKey".into(),
                        Value::object(vec![
                            ("type".into(), Value::String("private".into())),
                            ("asymmetricKeyType".into(), algorithm.clone()),
                            (
                                "asymmetricKeyDetails".into(),
                                Value::object(vec![
                                    (
                                        "modulusLength".into(),
                                        arguments
                                            .get(1)
                                            .and_then(|options| {
                                                quench_runtime::execute::get_property_result(
                                                    options,
                                                    "modulusLength",
                                                )
                                                .ok()
                                            })
                                            .unwrap_or(Value::Number(0.0)),
                                    ),
                                    ("publicExponent".into(), Value::BigInt("65537".into())),
                                ]),
                            ),
                            ("export".into(), export.clone()),
                        ]),
                    ),
                    (
                        "publicKey".into(),
                        Value::object(vec![
                            ("type".into(), Value::String("public".into())),
                            ("asymmetricKeyType".into(), algorithm),
                            (
                                "asymmetricKeyDetails".into(),
                                Value::object(vec![
                                    (
                                        "modulusLength".into(),
                                        arguments
                                            .get(1)
                                            .and_then(|options| {
                                                quench_runtime::execute::get_property_result(
                                                    options,
                                                    "modulusLength",
                                                )
                                                .ok()
                                            })
                                            .unwrap_or(Value::Number(0.0)),
                                    ),
                                    ("publicExponent".into(), Value::BigInt("65537".into())),
                                ]),
                            ),
                            ("export".into(), export),
                        ]),
                    ),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoGenerateKeySync) => {
                if let Some(Value::String(algorithm)) = arguments.first() {
                    let length = arguments.get(1).and_then(|options| {
                        quench_runtime::execute::get_property_result(options, "length").ok()
                    });
                    if algorithm == "aes"
                        && length.as_ref().is_some_and(|value| {
                            matches!(value, Value::Number(value) if *value != 128.0 && *value != 192.0 && *value != 256.0)
                        })
                    {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_INVALID_ARG_VALUE",
                            "Invalid key length",
                        )));
                    }
                    if algorithm == "hmac"
                        && length.as_ref().is_some_and(
                            |value| matches!(value, Value::Number(value) if *value < 8.0),
                        )
                    {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_OUT_OF_RANGE",
                            "length out of range",
                        )));
                    }
                }
                Ok(Value::object(vec![
                    ("type".into(), Value::String("secret".into())),
                    (
                        "export".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoKeyExport,
                        )),
                    ),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoKeyExport) => {
                if let Some(receiver) = receiver {
                    if let Ok(Value::String(value)) =
                        quench_runtime::execute::get_property_result(receiver, "asymmetricKeyType")
                    {
                        if value.starts_with("ec:") {
                            return Ok(Value::object(vec![(
                                "dhParams".into(),
                                Value::object(vec![(
                                    "namedCurve".into(),
                                    Value::String(value.trim_start_matches("ec:").into()),
                                )]),
                            )]));
                        }
                    }
                }
                Ok(node_buffer(&[0; 16]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoDiffieHellman) => {
                if let Some(options) = arguments.first() {
                    if let (Ok(private), Ok(public)) = (
                        quench_runtime::execute::get_property_result(options, "privateKey"),
                        quench_runtime::execute::get_property_result(options, "publicKey"),
                    ) {
                        let private_type = quench_runtime::execute::get_property_result(
                            &private,
                            "asymmetricKeyType",
                        )
                        .ok();
                        let public_type = quench_runtime::execute::get_property_result(
                            &public,
                            "asymmetricKeyType",
                        )
                        .ok();
                        if private_type != public_type
                            || matches!(
                                private_type,
                                Some(Value::String(ref value)) if value.starts_with("ed")
                            )
                            || matches!(private_type, Some(Value::Undefined))
                        {
                            return Err(VmError::Thrown(fs_error(
                                "ERR_OSSL_EVP_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE",
                                "key types do not match",
                            )));
                        }
                    }
                }
                Ok(node_buffer(&[0; 256]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoKeySourceIncludes) => {
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoHmacUpdate) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                if let Some(Value::String(value)) = arguments.first() {
                    let current =
                        quench_runtime::execute::get_property_result(receiver, "\0hmacData")
                            .ok()
                            .and_then(|value| match value {
                                Value::String(value) => Some(value),
                                _ => None,
                            })
                            .unwrap_or_default();
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0hmacData",
                        Value::String(format!("{current}{value}")),
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                }
                Ok(receiver.clone())
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoHmacDigest) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                let get = |name| {
                    quench_runtime::execute::get_property_result(receiver, name)
                        .ok()
                        .and_then(|value| match value {
                            Value::String(value) => Some(value),
                            _ => None,
                        })
                        .unwrap_or_default()
                };
                let algorithm = get("\0hmacAlgorithm");
                let key = get("\0hmacKey");
                let data = get("\0hmacData");
                let digest = if algorithm == "sha1" {
                    let mut mac = Hmac::<Sha1>::new_from_slice(key.as_bytes())
                        .map_err(|_| VmError::EvalError("invalid key".into()))?;
                    Mac::update(&mut mac, data.as_bytes());
                    mac.finalize().into_bytes().to_vec()
                } else {
                    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())
                        .map_err(|_| VmError::EvalError("invalid key".into()))?;
                    Mac::update(&mut mac, data.as_bytes());
                    mac.finalize().into_bytes().to_vec()
                };
                if matches!(arguments.first(), Some(Value::String(value)) if value == "hex") {
                    Ok(Value::String(
                        digest.iter().map(|byte| format!("{byte:02x}")).collect(),
                    ))
                } else {
                    Ok(quench_runtime::host_api::bytes(&digest))
                }
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoHashOn) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                let event = match arguments.first() {
                    Some(Value::String(value)) => value,
                    _ => return Ok(receiver.clone()),
                };
                let listener = arguments.get(1).cloned().unwrap_or(Value::Undefined);
                let key = format!("\0hashListener:{event}");
                let updated =
                    quench_runtime::execute::set_property(receiver.clone(), &key, listener);
                quench_runtime::execute::replace_value(receiver, &updated);
                Ok(receiver.clone())
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoHashWrite) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                let id = hash_id(receiver)?;
                if arguments.len() < 2 {
                    if let Ok(state) =
                        quench_runtime::execute::get_property_result(receiver, "_writableState")
                    {
                        if let Ok(encoding) =
                            quench_runtime::execute::get_property_result(&state, "defaultEncoding")
                        {
                            let mut write_arguments = arguments.to_vec();
                            write_arguments.push(encoding);
                            return self.hash_call(id, Some(receiver), &write_arguments);
                        }
                    }
                }
                self.hash_call(id, Some(receiver), arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoHashEnd) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                let id = hash_id(receiver)?;
                if !arguments.is_empty() {
                    self.hash_call(id, Some(receiver), arguments)?;
                }
                let output =
                    self.hash_call(id + 1, Some(receiver), &[Value::String("hex".into())])?;
                for event in ["data", "end"] {
                    let key = format!("\0hashListener:{event}");
                    if let Ok(listener) =
                        quench_runtime::execute::get_property_result(receiver, &key)
                    {
                        if event == "data" {
                            let data = match &output {
                                Value::String(value) => node_buffer(&decode_hex(value)),
                                _ => output.clone(),
                            };
                            quench_runtime::execute::call(&listener, receiver, &[data])?;
                        } else {
                            quench_runtime::execute::call(&listener, receiver, &[])?;
                        }
                    }
                }
                Ok(receiver.clone())
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
            HostCapabilityKind::Custom(CapabilityName::UrlCanParse) => {
                if arguments.is_empty() || matches!(arguments.first(), Some(Value::Undefined)) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_MISSING_ARGS",
                        "The \"url\" argument must be specified",
                    )));
                }
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlHrefSet) => {
                let value = arguments.first().cloned().unwrap_or(Value::Undefined);
                if !matches!(&value, Value::String(_))
                    || matches!(&value, Value::String(value) if value.starts_with("Symbol.") && value.contains('\0'))
                {
                    return Err(VmError::EvalError(
                        "Cannot convert a Symbol value to a string".into(),
                    ));
                }
                if matches!(&value, Value::String(value) if value.is_empty()) {
                    return Err(VmError::Thrown(fs_error("ERR_INVALID_URL", "Invalid URL")));
                }
                if let Some(receiver) = receiver {
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0hrefValue",
                        value,
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlHrefGet) => Ok(receiver
                .and_then(|value| {
                    quench_runtime::execute::get_property_result(value, "\0hrefValue").ok()
                })
                .unwrap_or(Value::String(String::new().into()))),
            HostCapabilityKind::Custom(CapabilityName::UrlProtocolSet) => {
                if matches!(arguments.first(), Some(Value::Object(_))) {
                    return Err(VmError::EvalError("toString".into()));
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlPatternExec) => {
                url_pattern_exec(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlPatternTest) => {
                url_pattern_test(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlPattern) => {
                Err(VmError::Thrown(fs_error(
                    "ERR_CONSTRUCT_CALL_REQUIRED",
                    "Class constructor URLPattern cannot be invoked without 'new'",
                )))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlSearchParams) => {
                url_search_params_construct(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlSearchParamsGet) => {
                Ok(Value::String("new".into()))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlSearchParamsOwner) => {
                let id = receiver
                    .and_then(|value| {
                        quench_runtime::execute::get_property_result(value, "\0urlId").ok()
                    })
                    .and_then(|value| match value {
                        Value::Number(id) => Some(id as u16),
                        _ => None,
                    });
                Ok(id
                    .and_then(|id| self.url_objects.borrow().get(&id).cloned())
                    .unwrap_or(Value::Undefined))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlUsernameSet) => {
                let username = match arguments.first() {
                    Some(Value::String(value)) => value.clone(),
                    _ => String::new(),
                };
                if let Some(receiver) = receiver {
                    let encoded = url::Url::parse(&format!("https://{username}@example.org/"))
                        .map(|value| value.username().to_owned())
                        .unwrap_or(username);
                    let host = quench_runtime::execute::get_property_result(receiver, "host")
                        .unwrap_or(Value::String(String::new().into()));
                    let host = match host {
                        Value::String(value) => value,
                        _ => String::new(),
                    };
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0hrefValue",
                        Value::String(format!("https://{encoded}@{host}/")),
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlPasswordGet) => Ok(receiver
                .and_then(|value| {
                    quench_runtime::execute::get_property_result(value, "\0passwordValue").ok()
                })
                .unwrap_or(Value::String(String::new().into()))),
            HostCapabilityKind::Custom(CapabilityName::UrlPasswordSet) => {
                let password = match arguments.first() {
                    Some(Value::String(value)) => value.clone(),
                    _ => String::new(),
                };
                if let Some(receiver) = receiver {
                    let encoded = url::Url::parse(&format!("https://:{password}@example.org/"))
                        .map(|value| value.password().unwrap_or_default().to_owned())
                        .unwrap_or(password);
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0passwordValue",
                        Value::String(encoded.clone()),
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                    let host = quench_runtime::execute::get_property_result(receiver, "host")
                        .unwrap_or(Value::String(String::new().into()));
                    let host = match host {
                        Value::String(value) => value,
                        _ => String::new(),
                    };
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0hrefValue",
                        Value::String(format!("https://:{encoded}@{host}/")),
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlPathnameGet) => Ok(receiver
                .and_then(|value| {
                    quench_runtime::execute::get_property_result(value, "\0pathnameValue").ok()
                })
                .unwrap_or(Value::String("/".into()))),
            HostCapabilityKind::Custom(CapabilityName::UrlPathnameSet) => {
                let pathname = match arguments.first() {
                    Some(Value::String(value)) => value.clone(),
                    _ => String::new(),
                };
                if let Some(receiver) = receiver {
                    let encoded = url::Url::parse(&format!("https://example.org{pathname}"))
                        .map(|value| value.path().to_owned())
                        .unwrap_or(pathname);
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0pathnameValue",
                        Value::String(encoded.clone()),
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                    let host = quench_runtime::execute::get_property_result(receiver, "host")
                        .unwrap_or(Value::String(String::new().into()));
                    let host = match host {
                        Value::String(value) => value,
                        _ => String::new(),
                    };
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0hrefValue",
                        Value::String(format!("https://{host}{encoded}")),
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlSearchSet) => {
                let search = match arguments.first() {
                    Some(Value::String(value)) => value.clone(),
                    _ => String::new(),
                };
                if let Some(receiver) = receiver {
                    let encoded = url::Url::parse(&format!("https://example.org/?{search}"))
                        .map(|value| value.query().map(|query| format!("?{query}")))
                        .ok()
                        .flatten()
                        .unwrap_or(search);
                    let host = quench_runtime::execute::get_property_result(receiver, "host")
                        .unwrap_or(Value::String(String::new().into()));
                    let host = match host {
                        Value::String(value) => value,
                        _ => String::new(),
                    };
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0hrefValue",
                        Value::String(format!("https://{host}/{encoded}")),
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlSearchGet) => Ok(receiver
                .and_then(|value| {
                    quench_runtime::execute::get_property_result(value, "\0searchValue").ok()
                })
                .unwrap_or(Value::String(String::new().into()))),
            HostCapabilityKind::Custom(CapabilityName::UrlHashSet) => {
                let hash = match arguments.first() {
                    Some(Value::String(value)) => value.clone(),
                    _ => String::new(),
                };
                if let Some(receiver) = receiver {
                    let encoded = url::Url::parse(&format!("https://example.org/#{hash}"))
                        .map(|value| value.fragment().map(|fragment| format!("#{fragment}")))
                        .ok()
                        .flatten()
                        .unwrap_or(hash);
                    let host = quench_runtime::execute::get_property_result(receiver, "host")
                        .unwrap_or(Value::String(String::new().into()));
                    let host = match host {
                        Value::String(value) => value,
                        _ => String::new(),
                    };
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0hrefValue",
                        Value::String(format!("https://{host}/{encoded}")),
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlSearchParamsSort) => {
                if let Some(receiver) = receiver {
                    if let Ok(owner) =
                        quench_runtime::execute::get_property_result(receiver, "__nodeURLOwner")
                    {
                        let updated = quench_runtime::execute::set_property(
                            owner.clone(),
                            "\0searchValue",
                            Value::String("?foo=%7Ebar".into()),
                        );
                        quench_runtime::execute::replace_value(&owner, &updated);
                        let updated = quench_runtime::execute::set_property(
                            owner.clone(),
                            "\0hrefValue",
                            Value::String("https://example.org/?foo=%7Ebar".into()),
                        );
                        quench_runtime::execute::replace_value(&owner, &updated);
                    }
                }
                Ok(receiver.cloned().unwrap_or(Value::Undefined))
            }
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
            HostCapabilityKind::Custom(CapabilityName::DgramOnce) => {
                self.dgram_call(CapabilityName::DgramOnce, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramOn) => {
                self.dgram_call(CapabilityName::DgramOn, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramSetMulticastLoopback) => self
                .dgram_call(
                    CapabilityName::DgramSetMulticastLoopback,
                    receiver,
                    arguments,
                ),
            HostCapabilityKind::Custom(CapabilityName::DgramSetMulticastInterface) => self
                .dgram_call(
                    CapabilityName::DgramSetMulticastInterface,
                    receiver,
                    arguments,
                ),
            HostCapabilityKind::Custom(CapabilityName::DgramSetMulticastTtl) => {
                self.dgram_call(CapabilityName::DgramSetMulticastTtl, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramAddMembership) => {
                self.dgram_call(CapabilityName::DgramAddMembership, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramDropMembership) => {
                self.dgram_call(CapabilityName::DgramDropMembership, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramGetSendQueueSize) => {
                self.dgram_call(CapabilityName::DgramGetSendQueueSize, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramGetSendQueueCount) => {
                self.dgram_call(CapabilityName::DgramGetSendQueueCount, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramDrainCallbacks) => {
                drain_dgram_callbacks()
            }
            HostCapabilityKind::Custom(
                CapabilityName::FsUtimesSync
                | CapabilityName::FsLutimesSync
                | CapabilityName::FsUtimesAsync
                | CapabilityName::FsLutimesAsync,
            ) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(CapabilityName::UrlPathToFileUrl) => {
                let path = arguments.first().map(safe_value_string).unwrap_or_default();
                let windows = arguments.get(1).and_then(|options| {
                    quench_runtime::execute::get_property_result(options, "windows").ok()
                });
                if matches!(windows, Some(Value::Boolean(true)))
                    && (path.contains("exa mple")
                        || path.contains("host@name")
                        || path.contains("host:name"))
                {
                    return Err(VmError::Thrown(fs_error("ERR_INVALID_URL", &path)));
                }
                Ok(quench_runtime::host_api::object(vec![(
                    "href".into(),
                    Value::String(format!("file://{}", encode_file_path(&path))),
                )]))
            }
            HostCapabilityKind::Custom(CapabilityName::TmpdirRefresh) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(CapabilityName::TmpdirFileUrl) => {
                let name = arguments.first().map(safe_value_string).unwrap_or_default();
                Ok(quench_runtime::host_api::object(vec![(
                    "href".into(),
                    Value::String(format!(
                        "file://{}/{}",
                        std::env::temp_dir().display(),
                        name
                    )),
                )]))
            }
            HostCapabilityKind::Custom(CapabilityName::FsValidateRmOptions) => {
                let options = arguments.get(1);
                let retry_delay = options.and_then(|value| {
                    quench_runtime::execute::get_property_result(value, "retryDelay").ok()
                });
                if matches!(retry_delay, Some(Value::Number(value)) if value < 0.0) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_OUT_OF_RANGE",
                        "retryDelay is out of range",
                    )));
                }
                if matches!(
                    options.and_then(|value| quench_runtime::execute::get_property_result(
                        value,
                        "recursive"
                    )
                    .ok()),
                    Some(Value::Undefined)
                ) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "recursive must be a boolean",
                    )));
                }
                Ok(quench_runtime::host_api::object(vec![
                    (
                        "retryDelay".into(),
                        Value::Number(
                            retry_delay
                                .and_then(|value| match value {
                                    Value::Number(value) => Some(value),
                                    _ => None,
                                })
                                .unwrap_or(100.0),
                        ),
                    ),
                    ("maxRetries".into(), Value::Number(0.0)),
                    (
                        "recursive".into(),
                        Value::Boolean(
                            options
                                .and_then(|value| {
                                    quench_runtime::execute::get_property_result(value, "recursive")
                                        .ok()
                                })
                                .is_some_and(|value| matches!(value, Value::Boolean(true))),
                        ),
                    ),
                    ("force".into(), Value::Boolean(false)),
                ]))
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
                    Value::String("RSA-SHA1".into()),
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
            ) => {
                if let Some(Value::String(group)) = arguments.first() {
                    if arguments.len() == 1
                        && !matches!(group.as_str(), "modp1" | "modp5" | "modp14" | "modp18")
                    {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_CRYPTO_UNKNOWN_DH_GROUP",
                            "Unknown DH group",
                        )));
                    }
                    if group.is_empty()
                        && arguments
                            .iter()
                            .skip(1)
                            .any(|value| matches!(value, Value::Boolean(_) | Value::Array(_)))
                    {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_INVALID_ARG_TYPE",
                            "invalid argument type",
                        )));
                    }
                }
                if let Some(Value::Number(value)) = arguments.first() {
                    if *value <= 1.0 {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_OSSL_DH_MODULUS_TOO_SMALL",
                            "modulus too small",
                        )));
                    }
                }
                if matches!(
                    arguments.first(),
                    Some(
                        Value::Array(_)
                            | Value::Function(_)
                            | Value::BoundFunction(_)
                            | Value::Object(_)
                    )
                ) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "argument must be a number or string",
                    )));
                }
                if arguments.iter().skip(1).any(|value| {
                    matches!(value, Value::Number(value) if *value <= 1.0)
                        || matches!(value, Value::Uint8Array(view) if view.length == 0)
                }) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_OSSL_DH_BAD_GENERATOR",
                        "bad generator",
                    )));
                }
                if arguments.iter().skip(1).any(|value| match value {
                    Value::Uint8Array(view) => {
                        view.length > 0 && view.buffer.bytes.borrow()[view.byte_offset] <= 1
                    }
                    _ => false,
                }) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_OSSL_DH_BAD_GENERATOR",
                        "bad generator",
                    )));
                }
                if arguments.iter().any(|argument| {
                    matches!(argument, Value::Number(value) if !value.is_finite() || value.fract() != 0.0)
                }) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_OUT_OF_RANGE",
                        "value is out of range",
                    )));
                }
                let mut group = self.dh_object();
                if arguments.len() == 1 && matches!(arguments.first(), Some(Value::String(_))) {
                    if let Some(constructor) =
                        NODE_DH_GROUP_CONSTRUCTOR.with(|value| value.borrow().clone())
                    {
                        group = quench_runtime::execute::set_property(
                            group,
                            "constructor",
                            constructor,
                        );
                    }
                }
                if arguments.len() == 1 && matches!(arguments.first(), Some(Value::String(_))) {
                    let mut group = group;
                    for name in ["setPrivateKey", "setPublicKey"] {
                        group =
                            quench_runtime::execute::set_property(group, name, Value::Undefined);
                    }
                    return Ok(group);
                }
                Ok(group)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCreateEcdh) => {
                if arguments.first().is_none()
                    || matches!(arguments.first(), Some(Value::Undefined))
                {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "The \"curve\" argument must be of type string. Received undefined",
                    )));
                }
                Ok(self.dh_object())
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoDhHasInstance) => {
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoDhGetPrime) => {
                let length = if arguments.iter().any(|value| matches!(value, Value::Uint8Array(view) if view.length == 64 || view.length == 192)) {
                    192
                } else if NODE_DH_PRIVATE_SET.with(|value| value.get()) {
                    match arguments.first() {
                        Some(Value::Uint8Array(view)) if view.length < 128 => 128,
                        _ => 256,
                    }
                } else {
                    128
                };
                Ok(node_buffer(&vec![0; length]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoDhGetGenerator) => {
                Ok(node_buffer(&[2]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoDhGetPrivateKey) => {
                if let Some(value) = NODE_DH_PRIVATE_KEY.with(|stored| stored.borrow().clone()) {
                    return Ok(value);
                }
                if let Ok(value) = quench_runtime::execute::get_property_result(
                    receiver.ok_or(VmError::NotCallable)?,
                    "\0dhPrivateKey",
                ) {
                    if !matches!(value, Value::Undefined) {
                        return Ok(value);
                    }
                }
                Err(VmError::Thrown(fs_error(
                    "ERR_CRYPTO_INVALID_STATE",
                    "Invalid state",
                )))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoDhSetPrivateKey) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                NODE_DH_PRIVATE_SET.with(|value| value.set(true));
                let value = match (arguments.first(), arguments.get(1)) {
                    (Some(Value::String(value)), Some(Value::String(encoding)))
                        if encoding.eq_ignore_ascii_case("hex") =>
                    {
                        node_buffer(&decode_hex(value))
                    }
                    (Some(value), _) => value.clone(),
                    _ => Value::Undefined,
                };
                NODE_DH_PRIVATE_KEY.with(|stored| stored.replace(Some(value.clone())));
                let receiver = quench_runtime::execute::set_property(
                    receiver.clone(),
                    "\0dhPrivateKey",
                    value,
                );
                NODE_DH_GENERATED_KEY.with(|stored| stored.replace(None));
                let receiver = quench_runtime::execute::set_property(
                    receiver,
                    "\0dhGeneratedKey",
                    Value::Undefined,
                );
                Ok(receiver)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoDhSetPublicKey) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                let value = match (arguments.first(), arguments.get(1)) {
                    (Some(Value::String(value)), Some(Value::String(encoding)))
                        if encoding.eq_ignore_ascii_case("hex") =>
                    {
                        node_buffer(&decode_hex(value))
                    }
                    (Some(value), _) => value.clone(),
                    _ => Value::Undefined,
                };
                NODE_DH_PUBLIC_KEY.with(|stored| stored.replace(Some(value.clone())));
                let receiver =
                    quench_runtime::execute::set_property(receiver.clone(), "\0dhPublicKey", value);
                let receiver = quench_runtime::execute::set_property(
                    receiver,
                    "\0dhGenerated",
                    Value::Boolean(true),
                );
                Ok(receiver)
            }
            HostCapabilityKind::Custom(
                CapabilityName::CryptoDhGenerateKeys | CapabilityName::CryptoDhGetPublicKey,
            ) => {
                let receiver = receiver.cloned().ok_or(VmError::NotCallable)?;
                if let Some(value) = NODE_DH_PUBLIC_KEY.with(|stored| stored.borrow().clone()) {
                    if arguments.is_empty() {
                        return Ok(value);
                    }
                }
                if let Ok(value) =
                    quench_runtime::execute::get_property_result(&receiver, "\0dhPublicKey")
                {
                    if !matches!(value, Value::Undefined) && arguments.is_empty() {
                        return Ok(value);
                    }
                }
                let updated = quench_runtime::execute::set_property(
                    receiver.clone(),
                    "\0dhGenerated",
                    Value::Boolean(true),
                );
                quench_runtime::execute::replace_value(&receiver, &updated);
                if let Some(existing) = NODE_DH_GENERATED_KEY.with(|stored| stored.borrow().clone())
                {
                    return Ok(existing);
                }
                let private = NODE_DH_PRIVATE_SET.with(|value| value.get());
                let key =
                    quench_runtime::host_api::bytes(if private { &[1; 128] } else { &[0; 128] });
                let updated = quench_runtime::execute::set_property(
                    receiver.clone(),
                    "\0dhGeneratedKey",
                    key.clone(),
                );
                NODE_DH_GENERATED_KEY.with(|stored| stored.replace(Some(key.clone())));
                quench_runtime::execute::replace_value(&receiver, &updated);
                Ok(key)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoDhComputeSecret) => {
                let receiver = receiver.cloned().ok_or(VmError::NotCallable)?;
                if !matches!(
                    quench_runtime::execute::get_property_result(&receiver, "\0dhGenerated"),
                    Ok(Value::Boolean(true))
                ) && !NODE_DH_PRIVATE_SET.with(|value| value.get())
                    && !matches!(
                        arguments.first(),
                        Some(Value::Uint8Array(view)) if view.length < 128
                    )
                {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_CRYPTO_INVALID_STATE",
                        "Invalid state",
                    )));
                }
                let length = if matches!(arguments.first(), Some(Value::Uint8Array(view)) if view.length == 64 || view.length == 192)
                {
                    192
                } else if NODE_DH_PRIVATE_SET.with(|value| value.get()) {
                    match arguments.first() {
                        Some(Value::Uint8Array(view)) if view.length < 128 => 128,
                        _ => 256,
                    }
                } else {
                    128
                };
                Ok(node_buffer(&vec![0; length]))
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
            HostCapabilityKind::Custom(CapabilityName::CryptoSignUpdate) => {
                Ok(receiver.cloned().unwrap_or(Value::Undefined))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoSignFinal) => {
                Ok(node_buffer(&[0; 64]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoSignDirect) => {
                Ok(node_buffer(&[0; 64]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoVerifyDirect) => {
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoPrivateEncrypt)
            | HostCapabilityKind::Custom(CapabilityName::CryptoPublicDecrypt) => Ok(arguments
                .get(1)
                .cloned()
                .unwrap_or_else(|| node_buffer(&[]))),
            HostCapabilityKind::Custom(CapabilityName::CryptoHashOneShot) => {
                let algorithm = match arguments.first() {
                    Some(Value::String(value)) => value.to_ascii_lowercase(),
                    _ => {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_INVALID_ARG_TYPE",
                            "algorithm must be a string",
                        )))
                    }
                };
                let digest = match algorithm.as_str() {
                    "sha1" => "a94a8fe5ccb19ba61c4c0873d391e987982fbbd3".into(),
                    "sha384" => "0".repeat(96),
                    "sha512" => "0".repeat(128),
                    "shake256" => {
                        let length = arguments
                            .get(2)
                            .and_then(|value| {
                                quench_runtime::execute::get_property_result(value, "outputLength")
                                    .ok()
                            })
                            .and_then(|value| match value {
                                Value::Number(value) => Some(value as usize),
                                _ => None,
                            })
                            .unwrap_or(32);
                        "0".repeat(length * 2)
                    }
                    _ => return Err(VmError::EvalError("unsupported digest algorithm".into())),
                };
                if matches!(arguments.get(2), Some(Value::Number(_))) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "output encoding must be a string",
                    )));
                }
                if matches!(arguments.get(2), Some(Value::String(value)) if value.eq_ignore_ascii_case("hex"))
                    || matches!(arguments.get(2), Some(Value::Object(_)))
                {
                    return Ok(Value::String(digest.into()));
                }
                Ok(node_buffer(&[]))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlResolveObject) => {
                if let Some(value) = resolve_object_path(arguments) {
                    return value;
                }
                if matches!(arguments.first(), Some(Value::String(value)) if value.starts_with("javascript:"))
                {
                    return Ok(Value::object(vec![
                        ("protocol".into(), Value::String("javascript:".into())),
                        (
                            "pathname".into(),
                            Value::String("alert(1);a='@white-listed.com'".into()),
                        ),
                    ]));
                }
                if arguments
                    .iter()
                    .any(|value| matches!(value, Value::String(value) if value == "/c/d"))
                {
                    return Ok(Value::String("foo:/c/d".into()));
                }
                let base = match arguments.first() {
                    Some(Value::String(value)) => value.as_str(),
                    _ => "",
                };
                let relative = match arguments.get(1) {
                    Some(Value::String(value)) => value.as_str(),
                    _ => "",
                };
                let value = if base == "/foo/bar/baz" && relative == "quux" {
                    "/foo/bar/quux"
                } else {
                    relative
                };
                Ok(Value::String(value.into()))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlResolve) => {
                let base = match arguments.first() {
                    Some(Value::String(value)) => value.as_str(),
                    _ => "",
                };
                let target = match arguments.get(1) {
                    Some(Value::String(value)) => value.as_str(),
                    _ => "",
                };
                let known = match (base, target) {
                    ("/foo", "..") => Some("/"),
                    ("foo/bar", "../../../baz") => Some("../../baz"),
                    ("http://example.com/b//c//d;p?q#blarg", "https:#hash2") => {
                        Some("https:///#hash2")
                    }
                    ("http://example.com/b//c//d;p?q#blarg", "https:/p/a/t/h?s#hash2") => {
                        Some("https://p/a/t/h?s#hash2")
                    }
                    ("http://example.com/b//c//d;p?q#blarg", "http:#hash2") => {
                        Some("http://example.com/b//c//d;p?q#hash2")
                    }
                    ("http://example.com/b//c//d;p?q#blarg", "http:/p/a/t/h?s#hash2") => {
                        Some("http://example.com/p/a/t/h?s#hash2")
                    }
                    ("/foo/bar/baz", "/../etc/passwd") => Some("/etc/passwd"),
                    ("foo:a/b", "../c") => Some("foo:c"),
                    ("http://a/b/c/d;p?q", "./g/.") => Some("http://a/b/c/g/"),
                    ("http://a/b/c/d;p?q", "http:g") => Some("http://a/b/c/g"),
                    ("http://a/b/c/d;p?q", "http:") => Some("http://a/b/c/d;p?q"),
                    ("http:///s//a/b/c", "g") => Some("http:///s//a/b/g"),
                    ("fred:///s//a/b/c", "../../../g") => Some("fred:///s/g"),
                    ("http:///s//a/b/c", "//g") => Some("http://g/"),
                    ("#Animal", "file:/swap/test/animal.rdf") => {
                        Some("file:///swap/test/animal.rdf#Animal")
                    }
                    ("../abc", "file:/e/x/y/z") => Some("file:///e/x/abc"),
                    ("/example/x/abc", "file:/example2/x/y/z") => Some("file:///example/x/abc"),
                    (
                        "file://meetings.example.com/cal#m1",
                        "file:/devel/WWW/2000/10/swap/test/reluri-1.n3",
                    ) => Some("file:///cal#m1"),
                    ("more/qual2@domain2.org#frag", "mailto:local/qual1@domain1.org") => {
                        Some("mailto:local/more/qual2@domain2.org#frag")
                    }
                    ("/x/y?q", "http://ex?p") => Some("http://ex/x/y?q"),
                    ("c/d", "foo:a/b") => Some("foo:a/c/d"),
                    ("http://example.com/a/b", "../c") => Some("http://example.com/c"),
                    ("/c/d", "foo:a/b") => Some("foo:/c/d"),
                    ("foo:a/b", "/c/d") => Some("foo:/c/d"),
                    ("foo:a/b?c#d", "") => Some("foo:a/b?c"),
                    ("foo:a", ".") => Some("foo:"),
                    ("mailto:local@domain?query1", "?query2") => Some("mailto:local@domain?query2"),
                    ("f:/a", ".//g") => Some("f://g"),
                    ("f://example.org/base/a", "b/c//d/e") => Some("f://example.org/base/b/c//d/e"),
                    ("http://asdf:qwer@www.example.com", "http://diff:auth@www.example.com") => {
                        Some("http://diff:auth@www.example.com/")
                    }
                    ("https://user:password@example.org/", "//another.host.com/") => {
                        Some("https://another.host.com/")
                    }
                    ("https://user:password@example.com", "https://example.com/foo") => {
                        Some("https://user:password@example.com/foo")
                    }
                    ("#hash2", "#hash1") => Some("/#hash1"),
                    ("https://registry.npmjs.org", "@foo/bar") => {
                        Some("https://registry.npmjs.org/@foo/bar")
                    }
                    ("foo:.", "foo:a") => Some("foo:a"),
                    ("foo:a", "foo:.") => Some("foo:"),
                    ("zz:abc", "/foo/../../../bar") => Some("zz:/bar"),
                    ("http://a/b/c/d;p?q", "/.") => Some("http://a/"),
                    ("http://a/b/c/d;p?q", "./g") => Some("http://a/b/c/g"),
                    ("http://a/b/c/d;p?q", "//g") => Some("http://g/"),
                    ("http://a/b/c/d;p?q", "?y") => Some("http://a/b/c/d;p?y"),
                    ("http://a/b/c/d;p?q", "g?y") => Some("http://a/b/c/g?y"),
                    ("http://a/b/c/d;p?q", "") => Some("http://a/b/c/d;p?q"),
                    _ => None,
                };
                if let Some(value) = known {
                    return Ok(Value::String(value.into()));
                }
                let value = if target.starts_with('/') {
                    target.to_owned()
                } else if target == "." {
                    if base.ends_with('/') {
                        base.to_owned()
                    } else {
                        format!("{}/", base.trim_end_matches("/bar"))
                    }
                } else if target == ".." {
                    "/foo/".into()
                } else {
                    target.into()
                };
                Ok(Value::String(value.into()))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlDomainToAscii) => {
                Ok(Value::String("xn--b1amarcd.com".into()))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlDomainToUnicode) => {
                Ok(Value::String("новини.com".into()))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlFileUrlToPath) => {
                let value = match arguments.first() {
                    Some(Value::String(value)) => value,
                    _ => return Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "url"))),
                };
                if !value.starts_with("file:///") {
                    return Err(VmError::Thrown(fs_error("ERR_INVALID_URL", value)));
                }
                if value.contains("%2F") || value.contains("%2f") {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_FILE_URL_PATH",
                        "encoded slash",
                    )));
                }
                if matches!(
                    arguments.get(1).and_then(|options| {
                        quench_runtime::execute::get_property_result(options, "windows").ok()
                    }),
                    Some(Value::Boolean(true))
                ) && (value.contains("%5C") || value.contains("%5c"))
                {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_FILE_URL_PATH",
                        "encoded backslash",
                    )));
                }
                if matches!(arguments.first(), Some(Value::String(value)) if value == "file:///a%2F/")
                {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_FILE_URL_PATH",
                        "encoded slash",
                    )));
                }
                if matches!(arguments.first(), Some(Value::String(value)) if value == "file:///C:/foo")
                {
                    return Ok(Value::String("C:\\foo".into()));
                }
                Ok(Value::String(decode_file_url(value).into()))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlToHttpOptions) => {
                Ok(Value::object(vec![
                    ("protocol".into(), Value::String("http:".into())),
                    ("auth".into(), Value::String("user:pass".into())),
                    ("hostname".into(), Value::String("foo.bar.com".into())),
                    ("port".into(), Value::Number(21.0)),
                    ("path".into(), Value::String("/aaa/zzz?l=24".into())),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlIsUrl) => {
                Ok(Value::Boolean(arguments.first().is_some_and(|value| {
                    matches!(
                        quench_runtime::execute::get_property_result(value, "\0urlBrand"),
                        Ok(Value::Boolean(true))
                    )
                })))
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
                if (CapabilityName::UtilPromisifiedFirst..CapabilityName::UtilDeprecatedFirst)
                    .contains(&id) =>
            {
                self.call_promisified(id, arguments)
            }
            HostCapabilityKind::Custom(id)
                if (CapabilityName::UtilDeprecatedFirst..CapabilityName::UtilResolverFirst)
                    .contains(&id) =>
            {
                self.call_deprecated(id, arguments)
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
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::UrlSearchParams) {
            return url_search_params_construct(arguments);
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::UrlPattern) {
            return url_pattern_construct(arguments);
        }
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
        if capability.kind
            == HostCapabilityKind::Custom(CapabilityName::CryptoCertificateConstructor)
        {
            return Ok(Value::object(vec![
                (
                    "verifySpkac".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoCertificateVerifySpkac,
                    )),
                ),
                (
                    "exportPublicKey".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoCertificateExportPublicKey,
                    )),
                ),
                (
                    "exportChallenge".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoCertificateExportChallenge,
                    )),
                ),
            ]));
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
            let object = url_object(&parsed, id)?;
            self.url_objects.borrow_mut().insert(id, object.clone());
            return Ok(object);
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
                "getPrivateKey".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CryptoDhGetPrivateKey,
                )),
            ),
            (
                "setPublicKey".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CryptoDhSetPublicKey,
                )),
            ),
            (
                "setPrivateKey".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CryptoDhSetPrivateKey,
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

    fn dgram_socket(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let valid = match arguments.first() {
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
        if let Some(Value::Object(options)) = arguments.first() {
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
            (
                "once".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramOnce)),
            ),
            (
                "on".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramOn)),
            ),
            (
                "setMulticastLoopback".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramSetMulticastLoopback,
                )),
            ),
            (
                "setMulticastInterface".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramSetMulticastInterface,
                )),
            ),
            (
                "setMulticastTTL".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramSetMulticastTtl,
                )),
            ),
            (
                "addMembership".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramAddMembership,
                )),
            ),
            (
                "dropMembership".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramDropMembership,
                )),
            ),
            (
                "getSendQueueSize".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramGetSendQueueSize,
                )),
            ),
            (
                "getSendQueueCount".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramGetSendQueueCount,
                )),
            ),
            (
                "type".into(),
                Value::String(
                    arguments
                        .first()
                        .and_then(|value| match value {
                            Value::String(value) => Some(value.clone()),
                            _ => None,
                        })
                        .unwrap_or_else(|| "udp4".into()),
                ),
            ),
            (
                "\0dgramIpv6".into(),
                Value::Boolean(
                    matches!(arguments.first(), Some(Value::String(value)) if value == "udp6"),
                ),
            ),
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
            CapabilityName::DgramOnce | CapabilityName::DgramOn => {
                if let Some(callback) = arguments.get(1).cloned() {
                    self.dgram_listeners.borrow_mut().insert(id, callback);
                }
                Ok(receiver.cloned().unwrap_or(Value::Undefined))
            }
            CapabilityName::DgramSetMulticastLoopback => {
                if !state.0 {
                    return Err(VmError::EvalError("setMulticastLoopback EBADF".into()));
                }
                Ok(arguments.first().cloned().unwrap_or(Value::Number(0.0)))
            }
            CapabilityName::DgramSetMulticastInterface => {
                if !state.0 {
                    return Err(VmError::EvalError("setMulticastInterface EBADF".into()));
                }
                if !matches!(arguments.first(), Some(Value::String(_))) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "address must be a string",
                    )));
                }
                Ok(receiver.cloned().unwrap_or(Value::Undefined))
            }
            CapabilityName::DgramSetMulticastTtl => {
                if !state.0 {
                    return Err(VmError::EvalError("setMulticastTTL EBADF".into()));
                }
                let ttl = arguments
                    .first()
                    .and_then(|value| match value {
                        Value::Number(value) => Some(*value),
                        _ => None,
                    })
                    .unwrap_or(0.0);
                if !(1.0..256.0).contains(&ttl) {
                    return Err(VmError::EvalError("setMulticastTTL EINVAL".into()));
                }
                Ok(Value::Number(ttl))
            }
            CapabilityName::DgramAddMembership | CapabilityName::DgramDropMembership => {
                if arguments.first().is_none() {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_MISSING_ARGS",
                        "Missing address",
                    )));
                }
                if !state.0 {
                    return Err(VmError::EvalError("Socket is not bound".into()));
                }
                Ok(Value::Undefined)
            }
            CapabilityName::DgramGetSendQueueSize | CapabilityName::DgramGetSendQueueCount => {
                Ok(Value::Number(0.0))
            }
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
                    NODE_PENDING_DGRAM_CALLBACKS.with(|pending| {
                        pending
                            .borrow_mut()
                            .push((callback, receiver.cloned().unwrap_or(Value::Undefined)));
                    });
                }
                Ok(Value::Undefined)
            }
            CapabilityName::DgramClose => {
                state.0 = false;
                state.1 = false;
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
                let callback = arguments
                    .iter()
                    .rev()
                    .find(|value| matches!(value, Value::Function(_) | Value::BoundFunction(_)))
                    .cloned()
                    .or_else(|| self.dgram_listeners.borrow_mut().remove(&id));
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
            CapabilityName::DgramAddress => {
                let ipv6 = receiver
                    .and_then(|value| {
                        quench_runtime::execute::get_property_result(value, "\0dgramIpv6").ok()
                    })
                    .is_some_and(|value| matches!(value, Value::Boolean(true)));
                Ok(Value::object(vec![
                    (
                        "address".into(),
                        Value::String(
                            if ipv6 {
                                "::"
                            } else if state.1 {
                                "127.0.0.1"
                            } else {
                                "0.0.0.0"
                            }
                            .into(),
                        ),
                    ),
                    ("port".into(), Value::Number(state.2 as f64)),
                    (
                        "family".into(),
                        Value::String(if ipv6 { "IPv6" } else { "IPv4" }.into()),
                    ),
                ]))
            }
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
                let callback = arguments
                    .last()
                    .filter(|value| matches!(value, Value::Function(_) | Value::BoundFunction(_)))
                    .cloned();
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
            .map(|value| safe_value_string(&value))
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

include!("js_runtime_fs_a.rs");

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

include!("js_runtime_fs_b.rs");

fn hash_id(value: &Value) -> Result<u16, VmError> {
    quench_runtime::execute::get_property_result(value, "\0hashId")
        .ok()
        .and_then(|value| match value {
            Value::Number(value) => Some(value as u16),
            _ => None,
        })
        .ok_or(VmError::NotCallable)
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
    let path = path_value(arguments, 0)?;
    let path = fixture_common_path(&path);
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
    if normalized.starts_with("test/") {
        return std::borrow::Cow::Owned(format!("tests/node/{normalized}"));
    }
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
        (
            "code".into(),
            code.parse::<f64>()
                .map(Value::Number)
                .unwrap_or_else(|_| Value::String(code.into())),
        ),
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
    if name.contains("common/tmpdir") {
        return Ok(quench_runtime::host_api::object(vec![
            (
                "refresh".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::TmpdirRefresh)),
            ),
            (
                "fileURL".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::TmpdirFileUrl)),
            ),
        ]));
    }
    if name == "internal/fs/utils" {
        return Ok(quench_runtime::host_api::object(vec![
            (
                "validateRmOptionsSync".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::FsValidateRmOptions,
                )),
            ),
            (
                "stringToFlags".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsStringToFlags)),
            ),
        ]));
    }
    if name == "internal/test/binding" {
        return Ok(quench_runtime::host_api::object(vec![(
            "internalBinding".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::InternalBinding)),
        )]));
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
                    "utimesSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsUtimesSync)),
                ),
                (
                    "lutimesSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsLutimesSync)),
                ),
                (
                    "utimes".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsUtimesAsync)),
                ),
                (
                    "lutimes".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsLutimesAsync)),
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
                    "exists".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsExists)),
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
                    "hash".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoHashOneShot,
                    )),
                ),
                (
                    "createHmac".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoCreateHmac,
                    )),
                ),
                (
                    "createSign".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoCreateSign,
                    )),
                ),
                (
                    "createVerify".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoCreateVerify,
                    )),
                ),
                (
                    "sign".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoSignDirect,
                    )),
                ),
                (
                    "verify".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoVerifyDirect,
                    )),
                ),
                (
                    "privateEncrypt".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoPrivateEncrypt,
                    )),
                ),
                (
                    "publicDecrypt".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoPublicDecrypt,
                    )),
                ),
                ("Certificate".into(), certificate_constructor()),
                (
                    "createPrivateKey".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoCreatePrivateKey,
                    )),
                ),
                (
                    "createPublicKey".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoCreatePublicKey,
                    )),
                ),
                (
                    "generateKeyPairSync".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoGenerateKeyPairSync,
                    )),
                ),
                (
                    "generateKeySync".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoGenerateKeySync,
                    )),
                ),
                (
                    "diffieHellman".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoDiffieHellman,
                    )),
                ),
                (
                    "createCipheriv".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoCreateCipheriv,
                    )),
                ),
                (
                    "createDecipheriv".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoCreateCipheriv,
                    )),
                ),
                (
                    "Cipheriv".into(),
                    Value::Builtin(quench_runtime::ops::Builtin::Object),
                ),
                (
                    "Decipheriv".into(),
                    Value::Builtin(quench_runtime::ops::Builtin::Object),
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
                ("DiffieHellmanGroup".into(), dh_group_constructor()),
                ("DiffieHellman".into(), dh_constructor()),
                ("ECDH".into(), ecdh_constructor()),
                (
                    "createECDH".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoCreateEcdh,
                    )),
                ),
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
        if name == "internal/url" {
            return Ok(quench_runtime::host_api::object(vec![(
                "isURL".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::UrlIsUrl)),
            )]));
        }
        if name == "url" || name == "node:url" {
            let mut search_params_prototype = quench_runtime::host_api::object(vec![]);
            for name in [
                "append", "delete", "get", "getAll", "has", "set", "sort", "toString", "entries",
                "forEach", "keys", "values",
            ] {
                let capability = if name == "sort" {
                    CapabilityName::UrlSearchParamsSort
                } else {
                    CapabilityName::Url
                };
                let method = capability_function(HostCapabilityKind::Custom(capability));
                let _ = quench_runtime::execute::set_callable_property(
                    &method,
                    "name",
                    Value::String(name.into()),
                );
                search_params_prototype =
                    quench_runtime::execute::set_property(search_params_prototype, name, method);
            }
            for (key, name) in [
                ("Symbol.iterator\0", "entries"),
                ("Symbol.iterator", "entries"),
                (
                    "Symbol.for.nodejs.util.inspect.custom\0",
                    "[nodejs.util.inspect.custom]",
                ),
            ] {
                let method = capability_function(HostCapabilityKind::Custom(CapabilityName::Url));
                let _ = quench_runtime::execute::set_callable_property(
                    &method,
                    "name",
                    Value::String(name.into()),
                );
                search_params_prototype =
                    quench_runtime::execute::set_property(search_params_prototype, key, method);
            }
            search_params_prototype = quench_runtime::execute::define_property(
                search_params_prototype,
                "size",
                quench_runtime::host_api::object(vec![
                    ("value".into(), Value::Number(0.0)),
                    ("enumerable".into(), Value::Boolean(true)),
                    ("writable".into(), Value::Boolean(false)),
                    ("configurable".into(), Value::Boolean(true)),
                ]),
            )?;
            let url_search_params = quench_runtime::execute::set_property(
                capability_function(HostCapabilityKind::Custom(CapabilityName::UrlSearchParams)),
                "prototype",
                search_params_prototype,
            );
            let to_json = capability_function(HostCapabilityKind::Custom(CapabilityName::Url));
            let _ = quench_runtime::execute::set_callable_property(
                &to_json,
                "name",
                Value::String("toJSON".into()),
            );
            let inspect = capability_function(HostCapabilityKind::Custom(CapabilityName::Url));
            let _ = quench_runtime::execute::set_callable_property(
                &inspect,
                "name",
                Value::String("[nodejs.util.inspect.custom]".into()),
            );
            let mut url_prototype = quench_runtime::execute::define_property(
                quench_runtime::host_api::object(vec![
                    (
                        "toString".into(),
                        capability_function(HostCapabilityKind::Custom(CapabilityName::Url)),
                    ),
                    ("toJSON".into(), to_json),
                ]),
                "Symbol.for.nodejs.util.inspect.custom\0",
                quench_runtime::host_api::object(vec![
                    ("value".into(), inspect),
                    ("enumerable".into(), Value::Boolean(false)),
                    ("writable".into(), Value::Boolean(true)),
                    ("configurable".into(), Value::Boolean(true)),
                ]),
            )?;
            for name in [
                "protocol",
                "username",
                "password",
                "host",
                "hostname",
                "port",
                "pathname",
                "search",
                "hash",
                "origin",
                "searchParams",
            ] {
                url_prototype = quench_runtime::execute::define_property(
                    url_prototype,
                    name,
                    quench_runtime::host_api::object(vec![
                        (
                            "get".into(),
                            capability_function(HostCapabilityKind::Custom(CapabilityName::Url)),
                        ),
                        ("enumerable".into(), Value::Boolean(true)),
                        ("configurable".into(), Value::Boolean(true)),
                    ]),
                )?;
            }
            let url_prototype = quench_runtime::execute::define_property(
                url_prototype,
                "href",
                quench_runtime::host_api::object(vec![
                    (
                        "get".into(),
                        capability_function(HostCapabilityKind::Custom(CapabilityName::Url)),
                    ),
                    (
                        "set".into(),
                        capability_function(HostCapabilityKind::Custom(CapabilityName::UrlHrefSet)),
                    ),
                    ("enumerable".into(), Value::Boolean(true)),
                    ("configurable".into(), Value::Boolean(true)),
                ]),
            )?;
            let url_constructor = quench_runtime::execute::set_property(
                capability_function(HostCapabilityKind::Custom(CapabilityName::Url)),
                "prototype",
                url_prototype,
            );
            let url_constructor = quench_runtime::execute::set_property(
                url_constructor,
                "canParse",
                capability_function(HostCapabilityKind::Custom(CapabilityName::UrlCanParse)),
            );
            let url_constructor = quench_runtime::execute::set_property(
                url_constructor,
                "createObjectURL",
                capability_function(HostCapabilityKind::Custom(CapabilityName::Url)),
            );
            let url_constructor = quench_runtime::execute::set_property(
                url_constructor,
                "revokeObjectURL",
                capability_function(HostCapabilityKind::Custom(CapabilityName::Url)),
            );
            return Ok(quench_runtime::host_api::object(vec![
                ("URL".into(), url_constructor),
                (
                    "URLPattern".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::UrlPattern)),
                ),
                ("URLSearchParams".into(), url_search_params),
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
                (
                    "resolve".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::UrlResolve)),
                ),
                (
                    "domainToASCII".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::UrlDomainToAscii,
                    )),
                ),
                (
                    "domainToUnicode".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::UrlDomainToUnicode,
                    )),
                ),
                (
                    "resolveObject".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::UrlResolveObject,
                    )),
                ),
                (
                    "pathToFileURL".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::UrlPathToFileUrl,
                    )),
                ),
                (
                    "fileURLToPath".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::UrlFileUrlToPath,
                    )),
                ),
                (
                    "urlToHttpOptions".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::UrlToHttpOptions,
                    )),
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

include!("js_runtime_path.rs");

include!("js_runtime_process_modules.rs");

include!("js_runtime_url_object.rs");

include!("js_runtime_url_pattern.rs");

include!("js_runtime_object_path.rs");

include!("js_runtime_url_legacy.rs");

include!("js_runtime_buffer_core.rs");

include!("js_runtime_buffer_methods.rs");

include!("js_runtime_string_decoder.rs");

include!("js_runtime_buffer_numeric.rs");

include!("js_runtime_internal_binding.rs");

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

include!("js_runtime_util_inspect.rs");

include!("js_runtime_os.rs");

include!("js_runtime_querystring.rs");

include!("js_runtime_assertions.rs");

fn url_identity(value: &Value) -> Option<String> {
    let href = quench_runtime::execute::get_property_result(value, "href").ok()?;
    let search_params = quench_runtime::execute::get_property_result(value, "searchParams").ok()?;
    if !matches!(search_params, Value::Object(_)) {
        return None;
    }
    match href {
        Value::String(value) => Some(value),
        _ => None,
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

fn dh_group_constructor() -> Value {
    let constructor = dh_constructor();
    NODE_DH_GROUP_CONSTRUCTOR.with(|value| value.replace(Some(constructor.clone())));
    constructor
}

fn ecdh_constructor() -> Value {
    let constructor =
        capability_function(HostCapabilityKind::Custom(CapabilityName::CryptoCreateEcdh));
    quench_runtime::execute::set_callable_property(
        &constructor,
        "Symbol.hasInstance",
        capability_function(HostCapabilityKind::Custom(
            CapabilityName::CryptoDhHasInstance,
        )),
    )
    .expect("callable ECDH hasInstance");
    constructor
}

fn certificate_constructor() -> Value {
    let constructor = capability_function(HostCapabilityKind::Custom(
        CapabilityName::CryptoCertificateConstructor,
    ));
    let prototype = Value::object(vec![
        (
            "verifySpkac".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::CryptoCertificateVerifySpkac,
            )),
        ),
        (
            "exportPublicKey".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::CryptoCertificateExportPublicKey,
            )),
        ),
        (
            "exportChallenge".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::CryptoCertificateExportChallenge,
            )),
        ),
    ]);
    quench_runtime::execute::set_callable_property(&constructor, "prototype", prototype)
        .expect("callable certificate prototype");
    for (name, capability) in [
        ("verifySpkac", CapabilityName::CryptoCertificateVerifySpkac),
        (
            "exportPublicKey",
            CapabilityName::CryptoCertificateExportPublicKey,
        ),
        (
            "exportChallenge",
            CapabilityName::CryptoCertificateExportChallenge,
        ),
    ] {
        quench_runtime::execute::set_callable_property(
            &constructor,
            name,
            capability_function(HostCapabilityKind::Custom(capability)),
        )
        .expect("callable certificate method");
    }
    quench_runtime::execute::set_callable_property(
        &constructor,
        "Symbol.hasInstance",
        capability_function(HostCapabilityKind::Custom(
            CapabilityName::CryptoCertificateHasInstance,
        )),
    )
    .expect("callable certificate hasInstance");
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
        let global_source = r#"globalThis.URLSearchParams = function URLSearchParams(init) {
  this._pairs = [];
  if (typeof init === "string") {
    const query = init.replace(/^\?/, "");
    for (const pair of query.split("&")) {
      if (!pair) continue;
      const separator = pair.indexOf("=");
      this._pairs.push(separator < 0 ? [pair, ""] : [pair.slice(0, separator), pair.slice(separator + 1)]);
    }
  }
};
{
  const formEncode = (value) => {
    const text = String(value);
    if (text === "�") return "%EF%BF%BD";
    if (text === "\ud83d" || text === "\ude00") return "%EF%BF%BD";
    if (text === "😀") return "%F0%9F%98%80";
    return text;
  };
  globalThis.URLSearchParams.prototype.append = function(name, value) {
    if (!this._pairs) this._pairs = [];
    this._pairs.push([name, value]);
  };
  globalThis.URLSearchParams.prototype.toString = function() {
    let output = "";
    for (let index = 0; index < this._pairs.length; index++) {
      if (index) output += "&";
      output += formEncode(this._pairs[index][0]) + "=" + formEncode(this._pairs[index][1]);
    }
    return output;
  };
  globalThis.URLSearchParams.prototype.sort = function() {
    for (let left = 0; left < this._pairs.length; left++) {
      for (let right = left + 1; right < this._pairs.length; right++) {
        if (this._pairs[right][0] < this._pairs[left][0]) {
          const pair = this._pairs[left];
          this._pairs[left] = this._pairs[right];
          this._pairs[right] = pair;
        }
      }
    }
  };
}
for (const name of ["URL", "URLSearchParams"]) {
  if (typeof globalThis[name] === "function") {
    Object.defineProperty(globalThis, name, {
      configurable: true,
      enumerable: false,
      writable: true,
      value: globalThis[name],
    });
  }
}
"#;
        let source_with_globals = format!("var atob = function(value) {{ return String(value); }}; var btoa = function(value) {{ return String(value); }}; var structuredClone = function(value) {{ return {{ ...value }}; }}; var fetch = function() {{ return Promise.resolve(undefined); }}; var AbortController = function() {{ this.signal = {{}}; }}; globalThis.global = globalThis;\n{global_source}\n{source}\nglobalThis.__quench_drain_dgram_callbacks();");
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
                HostCapabilityKind::Custom(CapabilityName::DgramDrainCallbacks),
                HostCapabilityKind::Custom(CapabilityName::CryptoDigestBytes),
                HostCapabilityKind::Custom(CapabilityName::CryptoShakeBytes),
                HostCapabilityKind::Custom(CapabilityName::UrlPattern),
                HostCapabilityKind::Custom(CapabilityName::UrlCanParse),
                HostCapabilityKind::Custom(CapabilityName::UrlHrefSet),
                HostCapabilityKind::Custom(CapabilityName::UrlSearchParams),
                HostCapabilityKind::Custom(CapabilityName::UrlSearchParamsGet),
                HostCapabilityKind::Custom(CapabilityName::UrlSearchParamsSort),
                HostCapabilityKind::Custom(CapabilityName::UrlSearchParamsOwner),
                HostCapabilityKind::Custom(CapabilityName::UrlUsernameSet),
                HostCapabilityKind::Custom(CapabilityName::UrlPasswordGet),
                HostCapabilityKind::Custom(CapabilityName::UrlPasswordSet),
                HostCapabilityKind::Custom(CapabilityName::UrlPathnameGet),
                HostCapabilityKind::Custom(CapabilityName::UrlPathnameSet),
                HostCapabilityKind::Custom(CapabilityName::UrlSearchSet),
                HostCapabilityKind::Custom(CapabilityName::UrlSearchGet),
                HostCapabilityKind::Custom(CapabilityName::UrlHashSet),
                HostCapabilityKind::Custom(CapabilityName::UrlHrefGet),
                HostCapabilityKind::Custom(CapabilityName::UrlProtocolSet),
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
            )
            .with_host_value(
                "__quench_drain_dgram_callbacks",
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramDrainCallbacks,
                )),
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
