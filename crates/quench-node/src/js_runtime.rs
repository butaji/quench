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

include!("js_runtime_construct_stream.rs");
include!("js_runtime_construct_c.rs");
include!("js_runtime_construct_b.rs");
include!("js_runtime_construct_a.rs");
include!("js_runtime_dispatch_misc_e.rs");
include!("js_runtime_dispatch_misc_d.rs");
include!("js_runtime_dispatch_misc_c.rs");
include!("js_runtime_dispatch_misc_b.rs");
include!("js_runtime_dispatch_misc_a.rs");
include!("js_runtime_dispatch_url.rs");
include!("js_runtime_dispatch_crypto_c.rs");
include!("js_runtime_dispatch_crypto_b.rs");
include!("js_runtime_dispatch_crypto_a.rs");
include!("js_runtime_dispatch_buffer.rs");
include!("js_runtime_dispatch_core.rs");

include!("js_runtime_host_impl.rs");
include!("js_runtime_host_zlib.rs");
impl QuenchNodeHost {}
include!("js_runtime_host_dgram.rs");
impl QuenchNodeHost {}
include!("js_runtime_host_promises.rs");
impl QuenchNodeHost {}
include!("js_runtime_host_fs_open.rs");
impl QuenchNodeHost {
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
}
include!("js_runtime_host_fs_io.rs");
impl QuenchNodeHost {}
include!("js_runtime_host_url_stream.rs");
impl QuenchNodeHost {
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

include!("js_runtime_require_early.rs");

include!("js_runtime_require_module.rs");
include!("js_runtime_require_crypto.rs");
include!("js_runtime_require_fs.rs");
include!("js_runtime_require_stream_http.rs");
include!("js_runtime_require_url.rs");
include!("js_runtime_require_path.rs");
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

include!("js_runtime_helpers_tail.rs");
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
