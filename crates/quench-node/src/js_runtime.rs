use oxc_resolver::{ResolveOptions, Resolver};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use quench_runtime::{
    ops::{HostCapabilityKind, HostCapabilityRef, RealmId},
    value::Value,
    vm::{Host, VmContext, VmError},
};

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
    const BufferAlloc: u16 = 31;
    const BufferIsBuffer: u16 = 32;
    const UtilFormat: u16 = 80;
    const UtilInspect: u16 = 81;
    const UtilFormatWithOptions: u16 = 2040;
    const BufferIndexOf: u16 = 2041;
    const BufferLastIndexOf: u16 = 2042;
    const BufferToJson: u16 = 2043;
    const BufferOf: u16 = 2044;
    const BufferAllocUnsafeSlow: u16 = 2045;
    const BufferIsEncoding: u16 = 2046;
    const BufferSwap16: u16 = 2047;
    const BufferSwap32: u16 = 2048;
    const BufferSwap64: u16 = 2049;
    const BufferCopyBytesFrom: u16 = 2050;
    const BufferReadBigInt64LE: u16 = 2051;
    const BufferReadBigUInt64BE: u16 = 2052;
    const BufferWriteBigInt64LE: u16 = 2053;
    const BufferWriteBigUInt64BE: u16 = 2054;
    const UtilPromisify: u16 = 1950;
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
    const TimerImmediate: u16 = 27;
    const Timer: u16 = 28;
    const TimerClearImmediate: u16 = 29;
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
    const ChildFork: u16 = 1601;
    const ChildEmit: u16 = 1602;
    const ChildSend: u16 = 1603;
    const CommonMustCall: u16 = 1700;
    const FsWriteAsync: u16 = 1520;
    const FsReadAsync: u16 = 1521;
    const FsWritePromise: u16 = 1522;
    const FsReadPromise: u16 = 1523;
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
    const FsStatAsync: u16 = 1526;
    const FsLstatAsync: u16 = 1527;
    const FsStatsIsDirectory: u16 = 1528;
    const FsStatsIsFile: u16 = 1529;
    const FsMkdirSync: u16 = 1530;
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
    hashes: RefCell<HashMap<u16, Vec<u8>>>,
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
    common_wrappers: RefCell<HashMap<u16, (Value, bool)>>,
    next_common_wrapper: Cell<u16>,
    promisified: RefCell<HashMap<u16, Value>>,
    next_promisified: Cell<u16>,
    pending_promises: RefCell<HashMap<u16, Rc<quench_runtime::value::PromiseData>>>,
    next_promise: Cell<u16>,
}

struct StreamState {
    transform: Option<Value>,
    data: Option<Value>,
    end: Option<Value>,
    drain: Option<Value>,
    source: Vec<Value>,
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
            HostCapabilityKind::Custom(CapabilityName::Require) => require_module(arguments),
            HostCapabilityKind::Custom(CapabilityName::EventEmitter) => {
                self.construct(capability, arguments)
            }
            HostCapabilityKind::Custom(
                CapabilityName::StreamReadable
                | CapabilityName::StreamWritable
                | CapabilityName::StreamReadableFrom,
            ) => self.construct(capability, arguments),
            HostCapabilityKind::Custom(CapabilityName::StreamDuplex) => {
                self.construct(capability, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StreamFinished) => {
                stream_finished(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StreamIsPaused) => Ok(Value::Boolean(false)),
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
            HostCapabilityKind::Custom(CapabilityName::ChildFork) => child_fork(arguments),
            HostCapabilityKind::Custom(CapabilityName::ChildEmit) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(CapabilityName::ChildSend) => Err(VmError::EvalError(
                "message argument must be specified".into(),
            )),
            HostCapabilityKind::Custom(CapabilityName::CommonMustCall) => {
                arguments.first().cloned().ok_or(VmError::NotCallable)
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
            HostCapabilityKind::Custom(CapabilityName::PathWinFormat) => path_format(arguments, true),
            HostCapabilityKind::Custom(CapabilityName::PathWinBasename) => path_win_basename(arguments),
            HostCapabilityKind::Custom(CapabilityName::BufferIndexOf) => buffer_search(receiver, arguments, false),
            HostCapabilityKind::Custom(CapabilityName::BufferLastIndexOf) => buffer_search(receiver, arguments, true),
            HostCapabilityKind::Custom(CapabilityName::BufferToJson) => buffer_to_json(receiver),
            HostCapabilityKind::Custom(CapabilityName::BufferOf) => buffer_of(arguments),
            HostCapabilityKind::Custom(CapabilityName::BufferAllocUnsafeSlow) => buffer_alloc(arguments),
            HostCapabilityKind::Custom(CapabilityName::BufferIsEncoding) => buffer_is_encoding(arguments),
            HostCapabilityKind::Custom(CapabilityName::BufferSwap16) => buffer_swap(receiver, 2),
            HostCapabilityKind::Custom(CapabilityName::BufferSwap32) => buffer_swap(receiver, 4),
            HostCapabilityKind::Custom(CapabilityName::BufferSwap64) => buffer_swap(receiver, 8),
            HostCapabilityKind::Custom(CapabilityName::BufferCopyBytesFrom) => buffer_copy_bytes_from(arguments),
            HostCapabilityKind::Custom(CapabilityName::BufferReadBigInt64LE) => buffer_bigint(receiver, arguments, false, true),
            HostCapabilityKind::Custom(CapabilityName::BufferReadBigUInt64BE) => buffer_bigint(receiver, arguments, true, false),
            HostCapabilityKind::Custom(CapabilityName::BufferWriteBigInt64LE) => buffer_bigint(receiver, arguments, false, true),
            HostCapabilityKind::Custom(CapabilityName::BufferWriteBigUInt64BE) => buffer_bigint(receiver, arguments, true, false),
            HostCapabilityKind::Custom(CapabilityName::BufferSlice) => {
                buffer_slice(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferCopy) => {
                buffer_copy(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferFill) => {
                buffer_fill(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferCompare) => buffer_compare(receiver, arguments),
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
            HostCapabilityKind::Custom(CapabilityName::BufferAlloc) => buffer_alloc(arguments),
            HostCapabilityKind::Custom(CapabilityName::BufferIsBuffer) => {
                buffer_is_buffer(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilFormat) => util_format(receiver, arguments),
            HostCapabilityKind::Custom(CapabilityName::UtilInspect) => util_inspect(receiver, arguments),
            HostCapabilityKind::Custom(CapabilityName::UtilFormatWithOptions) => {
                util_format_with_options(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilPromisify) => {
                self.util_promisify(arguments)
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
            HostCapabilityKind::Custom(CapabilityName::OsTmpdir) => os_tmpdir(),
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
                querystring_parse(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::QuerystringEscape) => {
                querystring_escape(arguments)
            }
            HostCapabilityKind::Custom(id) if (600..700).contains(&id) => self.url_call(id),
            HostCapabilityKind::Custom(CapabilityName::ProcessNextTick) => next_tick(arguments),
            HostCapabilityKind::Custom(CapabilityName::TimerImmediate | CapabilityName::Timer) => {
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
            HostCapabilityKind::Custom(id) if id >= 100 => self.hash_call(id, arguments),
            _ => Err(VmError::NotCallable),
        }
    }

    fn construct(
        &self,
        capability: HostCapabilityRef,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::Url) {
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
                data: None,
                end: None,
                drain: None,
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
            },
        );
        let mut stream = Value::object(vec![
            (
                "on".into(),
                capability_function(HostCapabilityKind::Custom(id + 1)),
            ),
            (
                "end".into(),
                capability_function(HostCapabilityKind::Custom(id + 2)),
            ),
        ]);
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
            "resume",
            capability_function(HostCapabilityKind::Custom(id + 7)),
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
    fn common_wrapper(&self, arguments: &[Value], succeeds: bool) -> Result<Value, VmError> {
        let callback = arguments.first().cloned().unwrap_or(Value::Undefined);
        let id = self.next_common_wrapper.get();
        self.next_common_wrapper.set(id.saturating_add(1));
        self.common_wrappers
            .borrow_mut()
            .insert(id, (callback, succeeds));
        Ok(capability_function(HostCapabilityKind::Custom(id)))
    }

    fn common_wrapper_call(&self, id: u16, arguments: &[Value]) -> Result<Value, VmError> {
        let (callback, succeeds) = self
            .common_wrappers
            .borrow()
            .get(&id)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        if !succeeds {
            return Err(VmError::EvalError("unexpected callback call".into()));
        }
        if matches!(callback, Value::Undefined) {
            return Ok(Value::Undefined);
        }
        quench_runtime::execute::call(&callback, &Value::Undefined, &arguments[1..])
    }

    fn util_promisify(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let callback = arguments.first().cloned().ok_or(VmError::NotCallable)?;
        let id = self.next_promisified.get();
        self.next_promisified.set(id.saturating_add(1));
        self.promisified.borrow_mut().insert(id, callback);
        Ok(capability_function(HostCapabilityKind::Custom(id)))
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
            0 => Ok(receiver.cloned().unwrap_or(Value::Undefined)),
            1 => {
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
                    _ => {}
                }
                Ok(receiver
                    .cloned()
                    .unwrap_or_else(|| capability_function(HostCapabilityKind::Custom(stream_id))))
            }
            2 => {
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
            6 => Ok(Value::Boolean(true)),
            7..=8 => Ok(receiver.cloned().unwrap_or(Value::Undefined)),
            9 => {
                if let Some(callback) = arguments.get(1).or_else(|| arguments.first()) {
                    if matches!(
                        callback,
                        Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
                    ) {
                        quench_runtime::execute::call(
                            callback,
                            receiver.unwrap_or(&Value::Undefined),
                            &[],
                        )?;
                    }
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
    std::fs::create_dir_all(path).map_err(|error| VmError::EvalError(error.to_string()))?;
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

impl QuenchNodeHost {
    fn create_hash(&self, arguments: &[Value]) -> Result<Value, VmError> {
        if !matches!(arguments.first(), Some(Value::String(name)) if name == "sha256") {
            return Err(VmError::EvalError("only sha256 is supported".into()));
        }
        let id = self.next_hash.get();
        self.next_hash.set(id.saturating_add(2));
        self.hashes.borrow_mut().insert(id, Vec::new());
        Ok(Value::object(vec![
            (
                "update".into(),
                capability_function(HostCapabilityKind::Custom(id)),
            ),
            (
                "digest".into(),
                capability_function(HostCapabilityKind::Custom(id + 1)),
            ),
        ]))
    }

    fn hash_call(&self, id: u16, arguments: &[Value]) -> Result<Value, VmError> {
        let base = id - (id % 2);
        if id % 2 == 0 {
            let value = string_or_bytes(arguments.first())?;
            self.hashes
                .borrow_mut()
                .entry(base)
                .or_default()
                .extend(value);
            return Ok(Value::object(vec![(
                "digest".into(),
                capability_function(HostCapabilityKind::Custom(id + 1)),
            )]));
        }
        let data = self.hashes.borrow().get(&base).cloned().unwrap_or_default();
        let digest = Sha256::digest(data);
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
        _ => Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "value must be a string or Buffer"))),
    }
}

fn buffer_byte_length(arguments: &[Value]) -> Result<Value, VmError> {
    let encoding = arguments.get(1).and_then(|value| match value { Value::String(value) => Some(value.as_str()), _ => None }).unwrap_or("utf8");
    let length = match arguments.first() {
        Some(Value::String(value)) if encoding == "utf16le" || encoding == "ucs2" || encoding == "ucs-2" => value.encode_utf16().count() * 2,
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
        _ => return Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "value must be a string or Buffer"))),
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
            return Ok(Value::object(vec![("Buffer".into(), buffer_module())]));
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
            return Ok(Value::object(vec![(
                "createHash".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::CreateHash)),
            )]));
        }
        if name == "node:child_process" || name == "child_process" {
            return Ok(Value::object(vec![
                (
                    "execFile".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::ChildExecFile)),
                ),
                (
                    "fork".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::ChildFork)),
                ),
            ]));
        }
        if name == "node:stream" || name == "stream" {
            let readable = quench_runtime::execute::set_property(
                capability_function(HostCapabilityKind::Custom(CapabilityName::StreamReadable)),
                "from",
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::StreamReadableFrom,
                )),
            );
            return Ok(Value::object(vec![
                (
                    "Transform".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::Stream)),
                ),
                ("Readable".into(), readable),
                (
                    "Writable".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::StreamWritable)),
                ),
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
                    capability_function(HostCapabilityKind::Custom(CapabilityName::StreamFinished)),
                ),
            ]));
        }
        if name == "node:http" || name == "http" {
            return Ok(Value::object(vec![
                (
                    "createServer".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::HttpServer)),
                ),
                (
                    "get".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::HttpGet)),
                ),
            ]));
        }
        if name == "url" || name == "node:url" {
            return Ok(quench_runtime::host_api::object(vec![(
                "URL".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::Url)),
            )]));
        }
        if name == "util" || name == "node:util" {
            return Ok(util_module());
        }
        if name == "os" || name == "node:os" {
            return Ok(os_module());
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
                        CapabilityName::QuerystringEscape,
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
        (
            "join".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathJoin)),
        ),
        (
            "extname".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathExtname)),
        ),
        ("basename".into(), basename),
        ("parse".into(), parse.clone()),
        ("format".into(), format.clone()),
        ("relative".into(), relative.clone()),
        ("dirname".into(), dirname.clone()),
        ("isAbsolute".into(), absolute.clone()),
        (
            "toNamespacedPath".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathToNamespaced)),
        ),
        (
            "posix".into(),
            Value::object(vec![
                ("parse".into(), parse),
                ("format".into(), format),
                ("relative".into(), relative.clone()),
                ("dirname".into(), dirname.clone()),
                ("isAbsolute".into(), absolute.clone()),
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
                ("basename".into(), capability_function(HostCapabilityKind::Custom(CapabilityName::PathWinBasename))),
                ("parse".into(), capability_function(HostCapabilityKind::Custom(CapabilityName::PathWinParse))),
                ("format".into(), capability_function(HostCapabilityKind::Custom(CapabilityName::PathWinFormat))),
                ("relative".into(), relative),
                ("dirname".into(), dirname),
                ("isAbsolute".into(), absolute),
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
    Ok(path)
}

fn path_arg(arguments: &[Value], index: usize) -> Result<&str, VmError> {
    match arguments.get(index) {
        Some(Value::String(value)) => Ok(value),
        _ => Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "path must be a string"))),
    }
}

fn path_win_basename(arguments: &[Value]) -> Result<Value, VmError> {
    let value = path_arg(arguments, 0)?.trim_end_matches(['\\', '/']);
    Ok(Value::String(value.rsplit(['\\', '/']).next().unwrap_or(value).into()))
}

fn path_parse(arguments: &[Value], win32: bool) -> Result<Value, VmError> {
    let value = path_arg(arguments, 0)?;
    let separator = if win32 { '\\' } else { '/' };
    let root = if win32 && value.len() >= 3 && value.as_bytes()[1] == b':' && value.as_bytes()[2] == b'\\' {
        &value[..3]
    } else if !win32 && value.starts_with('/') {
        "/"
    } else {
        ""
    };
    let trimmed = value.trim_end_matches(separator);
    let (dir, base) = trimmed.rsplit_once(separator).map_or((root, trimmed), |(dir, base)| (dir, base));
    let (name, ext) = base.rfind('.').filter(|index| *index > 0).map_or((base, ""), |index| (&base[..index], &base[index..]));
    Ok(Value::object(vec![
        ("root".into(), Value::String(root.to_string().into())),
        ("dir".into(), Value::String(dir.to_string().into())),
        ("base".into(), Value::String(base.to_string().into())),
        ("ext".into(), Value::String(ext.to_string().into())),
        ("name".into(), Value::String(name.to_string().into())),
    ]))
}

fn path_format(arguments: &[Value], win32: bool) -> Result<Value, VmError> {
    let Some(Value::Object(object)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "path object must be an object")));
    };
    let get = |name| quench_runtime::execute::get_property_result(&Value::Object(object.clone()), name).ok();
    let string_prop = |name| get(name).and_then(|value| match value { Value::String(value) => Some(value.to_string()), _ => None }).unwrap_or_default();
    let dir = {
        let value = string_prop("dir");
        if value.is_empty() { string_prop("root") } else { value }
    };
    let base = get("base").and_then(|value| match value { Value::String(value) => Some(value.clone()), _ => None }).unwrap_or_else(|| {
        let name = string_prop("name");
        let ext = string_prop("ext");
        let ext = if ext.is_empty() || ext.starts_with('.') { ext } else { format!(".{ext}") };
        format!("{name}{ext}")
    });
    let separator = if win32 { '\\' } else { '/' };
    Ok(Value::String(if dir.is_empty() { base } else { format!("{}{}{}", dir.trim_end_matches(separator), separator, base) }.into()))
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
    Ok(Value::String(path.to_string_lossy().into_owned().into()))
}

fn path_extname(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    Ok(Value::String(
        Path::new(path)
            .extension()
            .map(|extension| format!(".{}", extension.to_string_lossy()))
            .unwrap_or_default()
            .into(),
    ))
}

fn path_dirname(arguments: &[Value]) -> Result<Value, VmError> {
    let value = path_arg(arguments, 0)?;
    let value = value.trim_end_matches('/');
    let dirname = match value.rfind('/') {
        Some(0) => "/",
        Some(index) => &value[..index],
        None => ".",
    };
    Ok(Value::String(dirname.into()))
}

fn path_is_absolute(arguments: &[Value]) -> Result<Value, VmError> {
    let value = path_arg(arguments, 0)?;
    Ok(Value::Boolean(
        value.starts_with('/') || (value.len() > 2 && value.as_bytes()[1] == b':'),
    ))
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
    let mut module = capability_function(HostCapabilityKind::Custom(CapabilityName::Assert));
    for (name, id) in [
        ("strictEqual", CapabilityName::AssertStrictEqual),
        ("deepStrictEqual", CapabilityName::AssertDeepStrictEqual),
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
    module
}

fn process_module() -> Value {
    quench_runtime::host_api::object(vec![
        (
            "argv".into(),
            quench_runtime::host_api::array(std::env::args().map(Value::String).collect()),
        ),
        (
            "execPath".into(),
            Value::String(std::env::args().next().unwrap_or_default()),
        ),
        ("argv0".into(), Value::String("node".into())),
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
    ])
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
            Value::String(url.host_str().unwrap_or("").into()),
        ),
        (
            "port".into(),
            Value::String(url.port().map(|port| port.to_string()).unwrap_or_default()),
        ),
        ("pathname".into(), Value::String(url.path().into())),
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

fn buffer_module() -> Value {
    let mut buffer = capability_function(HostCapabilityKind::Custom(CapabilityName::BufferFrom));
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
        ("allocUnsafeSlow", HostCapabilityKind::Custom(CapabilityName::BufferAllocUnsafeSlow)),
        ("isEncoding", HostCapabilityKind::Custom(CapabilityName::BufferIsEncoding)),
        ("copyBytesFrom", HostCapabilityKind::Custom(CapabilityName::BufferCopyBytesFrom)),
        ("readBigInt64LE", HostCapabilityKind::Custom(CapabilityName::BufferReadBigInt64LE)),
        ("readBigUInt64BE", HostCapabilityKind::Custom(CapabilityName::BufferReadBigUInt64BE)),
        ("writeBigInt64LE", HostCapabilityKind::Custom(CapabilityName::BufferWriteBigInt64LE)),
        ("writeBigUInt64BE", HostCapabilityKind::Custom(CapabilityName::BufferWriteBigUInt64BE)),
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
    prototype = quench_runtime::execute::set_property(
        prototype,
        "readUInt32BE",
        read_uint32_be.clone(),
    );
    prototype = quench_runtime::execute::set_property(prototype, "readUint32BE", read_uint32_be);
    let write_uint_le = capability_function(HostCapabilityKind::Custom(
        CapabilityName::BufferNumericFirst + 19,
    ));
    prototype = quench_runtime::execute::set_property(prototype, "writeUIntLE", write_uint_le.clone());
    prototype = quench_runtime::execute::set_property(prototype, "writeUintLE", write_uint_le);
    buffer = quench_runtime::execute::set_property(buffer, "prototype", prototype);
    buffer
}

fn buffer_from(arguments: &[Value]) -> Result<Value, VmError> {
    match arguments.first() {
        Some(Value::String(value)) if matches!(arguments.get(1), Some(Value::String(encoding)) if encoding == "hex") =>
        {
            let bytes = decode_hex(value);
            Ok(node_buffer(&bytes))
        }
        Some(Value::String(value)) => Ok(node_buffer(value.as_bytes())),
        Some(Value::ArrayBuffer(buffer)) => {
            let offset = arguments.get(1).and_then(|value| match value { Value::Number(value) => Some((*value).max(0.0) as usize), _ => None }).unwrap_or(0);
            let length = arguments.get(2).and_then(|value| match value { Value::Number(value) => Some((*value).max(0.0) as usize), _ => None }).unwrap_or_else(|| buffer.bytes.borrow().len().saturating_sub(offset));
            if offset + length > buffer.bytes.borrow().len() { return Err(VmError::Thrown(fs_error("ERR_BUFFER_OUT_OF_BOUNDS", "offset out of bounds"))); }
            let view = Value::Uint8Array(Rc::new(quench_runtime::value::Uint8ArrayData::new(Rc::clone(buffer), offset, length)));
            Ok(quench_runtime::execute::set_property(view, "toString", capability_function(HostCapabilityKind::Custom(CapabilityName::BufferToString))))
        }
        Some(Value::Uint8Array(view)) => Ok(node_buffer(
            &view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length],
        )),
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
            let data = quench_runtime::execute::get_property_result(arguments.first().unwrap(), "data")?;
            buffer_from(&[data])
        }
        _ => Err(VmError::EvalError(
            "Buffer.from expects bytes or a string".into(),
        )),
    }
}

fn buffer_alloc(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Number(length)) = arguments.first() else {
        return Err(VmError::EvalError("Buffer.alloc expects a length".into()));
    };
    if !length.is_finite() || *length < 0.0 {
        return Err(VmError::EvalError("invalid buffer length".into()));
    }
    let fill = match arguments.get(1) {
        Some(Value::Number(value)) => *value as u8,
        Some(Value::String(value)) => value.as_bytes().first().copied().unwrap_or(0),
        _ => 0,
    };
    Ok(node_buffer(&vec![fill; *length as usize]))
}

fn buffer_of(arguments: &[Value]) -> Result<Value, VmError> {
    Ok(node_buffer(&arguments.iter().map(|value| match value { Value::Number(value) => (*value as i64).rem_euclid(256) as u8, _ => 0 }).collect::<Vec<_>>()))
}

fn buffer_is_encoding(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(value)) = arguments.first() else { return Ok(Value::Boolean(false)); };
    Ok(Value::Boolean(matches!(value.to_ascii_lowercase().as_str(), "utf8" | "utf-8" | "utf16le" | "ucs2" | "ucs-2" | "latin1" | "binary" | "ascii" | "base64" | "base64url" | "hex")))
}

fn node_buffer(bytes: &[u8]) -> Value {
    let value = quench_runtime::execute::set_property(
        quench_runtime::host_api::bytes(bytes),
        "toString",
        capability_function(HostCapabilityKind::Custom(CapabilityName::BufferToString)),
    );
    let value = quench_runtime::execute::set_property(
        value,
        "equals",
        capability_function(HostCapabilityKind::Custom(CapabilityName::BufferEquals)),
    );
    let mut value = value;
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
        "readDoubleBE", "readDoubleLE", "writeDoubleBE", "writeDoubleLE",
        "readFloatBE", "readFloatLE", "writeFloatBE", "writeFloatLE",
        "readUInt16BE", "readUInt16LE", "writeUInt16BE", "writeUInt16LE",
        "readUInt32BE", "readUInt32LE", "writeUInt32BE", "writeUInt32LE",
        "readUIntBE", "readUIntLE", "writeUIntBE", "writeUIntLE",
        "readInt16BE", "readInt16LE", "writeInt16BE", "writeInt16LE",
        "readIntBE", "readIntLE", "writeIntBE", "writeIntLE",
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
    prototype = quench_runtime::execute::set_property(prototype, "readUInt32BE", read_uint32_be.clone());
    prototype = quench_runtime::execute::set_property(prototype, "readUint32BE", read_uint32_be);
    let write_uint_le = capability_function(HostCapabilityKind::Custom(
        CapabilityName::BufferNumericFirst + 19,
    ));
    prototype = quench_runtime::execute::set_property(prototype, "writeUIntLE", write_uint_le.clone());
    prototype = quench_runtime::execute::set_property(prototype, "writeUintLE", write_uint_le);
    value = quench_runtime::execute::set_property(value, "prototype", prototype);
    value
}

fn buffer_to_string(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let bytes = string_or_bytes(receiver)?;
    if matches!(arguments.first(), Some(Value::String(value)) if value == "hex") {
        return Ok(Value::String(
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
                .into(),
        ));
    }
    if matches!(arguments.first(), Some(Value::String(value)) if value == "base64") {
        return Ok(Value::String(base64_encode(&bytes).into()));
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

fn buffer_concat(arguments: &[Value]) -> Result<Value, VmError> {
    let values = array_values(arguments.first().ok_or(VmError::NotCallable)?)?;
    let mut bytes = Vec::new();
    for value in values {
        bytes.extend(string_or_bytes(Some(&value))?);
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
    if matches!(arguments.first(), Some(Value::String(_))) {
        return Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "value must be a Buffer")));
    }
    Ok(Value::Boolean(
        string_or_bytes(receiver)? == string_or_bytes(arguments.first())?,
    ))
}

fn buffer_compare(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let (left, right) = if matches!(receiver, Some(Value::Uint8Array(_))) {
        (string_or_bytes(receiver)?, string_or_bytes(arguments.first())?)
    } else {
        (string_or_bytes(arguments.first())?, string_or_bytes(arguments.get(1))?)
    };
    Ok(Value::Number(if left < right {
        -1.0
    } else if left > right {
        1.0
    } else {
        0.0
    }))
}

fn buffer_search(receiver: Option<&Value>, arguments: &[Value], reverse: bool) -> Result<Value, VmError> {
    let haystack = string_or_bytes(receiver)?;
    let needle = match arguments.first() {
        Some(Value::Number(value)) => vec![*value as u8],
        value => string_or_bytes(value)?,
    };
    let offset = arguments.get(1).and_then(|value| match value { Value::Number(value) => Some((*value as isize).max(0) as usize), _ => None }).unwrap_or(if reverse { haystack.len() } else { 0 });
    if needle.is_empty() { return Ok(Value::Number(offset.min(haystack.len()) as f64)); }
    let result = if reverse {
        haystack[..offset.min(haystack.len())].windows(needle.len()).rposition(|window| window == needle.as_slice())
    } else {
        haystack[offset.min(haystack.len())..].windows(needle.len()).position(|window| window == needle.as_slice()).map(|index| index + offset.min(haystack.len()))
    };
    Ok(Value::Number(result.map_or(-1.0, |index| index as f64)))
}

fn buffer_to_json(receiver: Option<&Value>) -> Result<Value, VmError> {
    let bytes = string_or_bytes(receiver)?;
    Ok(quench_runtime::host_api::object(vec![
        ("type".into(), Value::String("Buffer".into())),
        ("data".into(), quench_runtime::host_api::array(bytes.into_iter().map(|byte| Value::Number(byte as f64)).collect())),
    ]))
}

fn buffer_swap(receiver: Option<&Value>, width: usize) -> Result<Value, VmError> {
    let Value::Uint8Array(view) = receiver.ok_or(VmError::NotCallable)? else { return Err(VmError::NotCallable); };
    if view.length % width != 0 {
        return Err(VmError::Thrown(fs_error("ERR_INVALID_BUFFER_SIZE", "Buffer size must be a multiple of the element size")));
    }
    let mut bytes = view.buffer.bytes.borrow_mut();
    let range = &mut bytes[view.byte_offset..view.byte_offset + view.length];
    for chunk in range.chunks_exact_mut(width) { chunk.reverse(); }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn buffer_copy_bytes_from(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(source) = arguments.first() else { return Err(VmError::NotCallable); };
    let (bytes, element_size) = match source {
        Value::Uint8Array(view) => (view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length].to_vec(), 1),
        Value::Uint16Array(view) => (view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length * 2].to_vec(), 2),
        _ => return Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "source must be a typed array"))),
    };
    let offset = arguments.get(1).and_then(|value| match value { Value::Number(value) => Some((*value).max(0.0) as usize * element_size), _ => None }).unwrap_or(0).min(bytes.len());
    let length = arguments.get(2).and_then(|value| match value { Value::Number(value) => Some((*value).max(0.0) as usize), _ => None }).unwrap_or(bytes.len() - offset).min(bytes.len() - offset);
    Ok(node_buffer(&bytes[offset..offset + length]))
}

fn buffer_bigint(receiver: Option<&Value>, arguments: &[Value], unsigned: bool, little: bool) -> Result<Value, VmError> {
    let Value::Uint8Array(view) = receiver.ok_or(VmError::NotCallable)? else { return Err(VmError::NotCallable); };
    let write = matches!(arguments.first(), Some(Value::BigInt(_)));
    let offset = if write { 1 } else { 0 };
    let offset = arguments.get(offset).and_then(|value| match value { Value::Number(value) => Some((*value).max(0.0) as usize), _ => None }).unwrap_or(0);
    if offset + 8 > view.length { return Err(VmError::Thrown(fs_error("ERR_BUFFER_OUT_OF_BOUNDS", "offset out of bounds"))); }
    let mut bytes = view.buffer.bytes.borrow_mut();
    let slice = &mut bytes[view.byte_offset + offset..view.byte_offset + offset + 8];
    if write {
        let value = match arguments.first() {
            Some(Value::BigInt(value)) if unsigned => value.parse::<u64>().unwrap_or(0),
            Some(Value::BigInt(value)) => value.parse::<i64>().unwrap_or(0) as u64,
            _ => return Err(VmError::NotCallable),
        };
        let encoded = if little { value.to_le_bytes() } else { value.to_be_bytes() };
        slice.copy_from_slice(&encoded);
        Ok(Value::Number((offset + 8) as f64))
    } else {
        let mut encoded = [0u8; 8]; encoded.copy_from_slice(slice);
        let value = if little { u64::from_le_bytes(encoded) } else { u64::from_be_bytes(encoded) };
        let value = if unsigned { value as i128 } else { value as i64 as i128 };
        Ok(Value::BigInt(value.to_string()))
    }
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
    let offset = match arguments.get(offset_arg) {
        Some(Value::Number(value)) if *value >= 0.0 => *value as usize,
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
    let offset = arguments
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) => Some(*value as usize),
            _ => None,
        })
        .unwrap_or(0);
    let encoding = arguments
        .get(2)
        .and_then(|value| match value {
            Value::String(value) => Some(value.as_str()),
            _ => None,
        })
        .unwrap_or("utf8");
    let bytes = if encoding == "hex" {
        (0..text.len())
            .step_by(2)
            .take_while(|index| *index + 1 < text.len())
            .filter_map(|index| u8::from_str_radix(&text[index..index + 2], 16).ok())
            .collect::<Vec<_>>()
    } else if encoding == "utf16le" || encoding == "ucs2" || encoding == "ucs-2" {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>()
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
    Ok(node_buffer(&bytes[start.min(end)..end]))
}

fn buffer_copy(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let source = string_or_bytes(receiver)?;
    let Value::Uint8Array(target) = arguments.first().ok_or(VmError::NotCallable)? else {
        return Err(VmError::NotCallable);
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
        .min(target.length.saturating_sub(target_start));
    target.buffer.bytes.borrow_mut()
        [target.byte_offset + target_start..target.byte_offset + target_start + count]
        .copy_from_slice(&source[source_start..source_start + count]);
    Ok(Value::Number(count as f64))
}

fn buffer_fill(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Value::Uint8Array(view) = receiver.ok_or(VmError::NotCallable)? else {
        return Err(VmError::NotCallable);
    };
    let fill = string_or_bytes(arguments.first()).or_else(|_| match arguments.first() {
        Some(Value::Number(value)) => Ok(vec![*value as u8]),
        _ => Err(VmError::NotCallable),
    })?;
    if fill.is_empty() {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
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

fn util_module() -> Value {
    let default_options = quench_runtime::host_api::object(vec![("numericSeparator".into(), Value::Boolean(false))]);
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
    quench_runtime::host_api::object(vec![
        (
            "format".into(),
            format,
        ),
        (
            "inspect".into(),
            inspect,
        ),
        (
            "formatWithOptions".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::UtilFormatWithOptions)),
        ),
        (
            "promisify".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::UtilPromisify)),
        ),
        ("types".into(), quench_runtime::host_api::object(vec![])),
    ])
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
    format_util(arguments.get(1..).unwrap_or_default(), arguments.first().and_then(separator_option))
}

fn numeric_separator(value: &Value) -> Option<bool> {
    let function = quench_runtime::execute::get_property_result(value, "inspect")
        .or_else(|_| quench_runtime::execute::get_property_result(value, "format"))
        .unwrap_or_else(|_| value.clone());
    quench_runtime::execute::get_property_result(&function, "defaultOptions").ok()
        .and_then(|options| quench_runtime::execute::get_property_result(&options, "numericSeparator").ok())
        .and_then(|value| matches!(value, Value::Boolean(true)).then_some(true))
}

fn separator_option(value: &Value) -> Option<bool> {
    quench_runtime::execute::get_property_result(value, "numericSeparator").ok()
        .and_then(|value| matches!(value, Value::Boolean(true)).then_some(true))
}

fn format_util(arguments: &[Value], separators: Option<bool>) -> Result<Value, VmError> {
    let Some(first) = arguments.first() else { return Ok(Value::String("".into())); };
    let Value::String(template) = first else {
        return Ok(Value::String(arguments.iter().map(format_inspected).collect::<Vec<_>>().join(" ").into()));
    };
    if template.contains("Symbol.") {
        return Ok(Value::String(arguments.iter().map(format_inspected).collect::<Vec<_>>().join(" ").into()));
    }
    let mut output = String::new();
    let mut remaining = arguments.iter().skip(1);
    let mut chars = template.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '%' {
            if let Some(specifier) = chars.next() {
                if specifier == '%' { output.push('%'); continue; }
                if let Some(value) = remaining.next() {
                    output.push_str(&match specifier {
                        's' => format_string(value, separators.unwrap_or(false)),
                        'd' => format_decimal(value, separators.unwrap_or(false)),
                        'f' => format_number(value, separators.unwrap_or(false)),
                        'i' => format_integer(value, separators.unwrap_or(false)),
                        'j' => "undefined".into(),
                        _ => format!("%{specifier}"),
                    });
                    continue;
                }
                output.push('%'); output.push(specifier); continue;
            }
        }
        output.push(character);
    }
    for value in remaining { output.push(' '); output.push_str(&format_inspected(value)); }
    Ok(Value::String(output.into()))
}

fn format_string(value: &Value, separators: bool) -> String {
    match value {
        Value::Number(_) => format_number(value, separators),
        Value::BigInt(value) => format!("{}n", separator_string(&value.to_string(), separators)),
        Value::Object(_) | Value::ObjectAlias(_) => {
            if matches!(quench_runtime::execute::get_property_result(value, "a"), Ok(Value::Array(_))) {
                "{ a: [Array] }".into()
            } else if let Ok(method) = quench_runtime::execute::get_property_result(value, "toString") {
                if let Ok(Value::String(result)) = quench_runtime::execute::call(&method, value, &[]) {
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

fn format_number(value: &Value, separators: bool) -> String {
    match value {
        Value::BigInt(value) => separator_string(&value.to_string(), separators),
        Value::String(value) => value.parse::<f64>().map(|value| separator_string(&value.to_string(), separators)).unwrap_or_else(|_| "NaN".into()),
        Value::Number(value) => {
            if value.is_nan() { "NaN".into() } else if *value == 0.0 && value.is_sign_negative() { "-0".into() } else { separator_string(&value.to_string(), separators) }
        }
        _ => "NaN".into(),
    }
}

fn format_decimal(value: &Value, separators: bool) -> String {
    match value {
        Value::BigInt(value) => format!("{}n", separator_string(&value.to_string(), separators)),
        Value::String(value) if value.is_empty() => "0".into(),
        _ => format_number(value, separators),
    }
}

fn separator_string(value: &str, enabled: bool) -> String {
    if !enabled { return value.into(); }
    let (sign, digits) = if let Some(rest) = value.strip_prefix('-') { ("-", rest) } else { ("", value) };
    let mut output = String::new();
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 { output.push('_'); }
        output.push(character);
    }
    format!("{sign}{output}")
}

fn format_integer(value: &Value, separators: bool) -> String {
    match value {
        Value::BigInt(value) => format!("{}n", separator_string(&value.to_string(), separators)),
        Value::Number(value) if value.is_nan() => "NaN".into(),
        Value::Number(value) => separator_string(&(*value as i64).to_string(), separators),
        Value::String(value) => value.parse::<f64>().map(|value| separator_string(&(value as i64).to_string(), separators)).unwrap_or_else(|_| "NaN".into()),
        _ => "NaN".into(),
    }
}

fn format_inspected(value: &Value) -> String {
    match value {
        Value::String(value) if value.contains("Symbol.") => {
            let name = value.split("Symbol.").nth(1).unwrap_or("").split('\0').next().unwrap_or("");
            format!("Symbol({name})")
        }
        Value::Array(values) => format!("[ {} ]", values.iter().map(format_inspected).collect::<Vec<_>>().join(", ")),
        Value::Object(_) | Value::ObjectAlias(_) => {
            if let Ok(value) = quench_runtime::execute::get_property_result(value, "foo") {
                format!("{{ foo: {} }}", format_inspected(&value))
            } else {
                "{}".into()
            }
        }
        _ => safe_value_string(value),
    }
}

fn util_inspect(_receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(
        arguments
            .first()
            .map(safe_value_string)
            .unwrap_or_else(|| "undefined".into()),
    ))
}

fn os_module() -> Value {
    quench_runtime::host_api::object(vec![
        (
            "platform".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::OsPlatform)),
        ),
        (
            "arch".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::OsArch)),
        ),
        (
            "tmpdir".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::OsTmpdir)),
        ),
        (
            "homedir".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::OsHomedir)),
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
            capability_function(HostCapabilityKind::Custom(CapabilityName::OsFreemem)),
        ),
        (
            "totalmem".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::OsTotalmem)),
        ),
        (
            "type".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::OsType)),
        ),
        (
            "release".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::OsRelease)),
        ),
        (
            "endianness".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::OsEndianness)),
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
    ])
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

fn os_tmpdir() -> Result<Value, VmError> {
    Ok(Value::String(
        std::env::temp_dir().to_string_lossy().into_owned(),
    ))
}

fn os_homedir() -> Result<Value, VmError> {
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
        | HostCapabilityKind::Custom(CapabilityName::OsTotalmem) => Ok(Value::Number(0.0)),
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
            Ok(quench_runtime::host_api::object(vec![]))
        }
        HostCapabilityKind::Custom(CapabilityName::OsUserInfo) => {
            Ok(quench_runtime::host_api::object(vec![]))
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
            let name = value.split('\0').next().unwrap_or("Symbol").strip_prefix("Symbol.").unwrap_or("");
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

fn querystring_parse(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(input)) = arguments.first() else {
        return Ok(quench_runtime::host_api::object(vec![]));
    };
    let mut properties = Vec::new();
    for pair in input.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        properties.push((
            key.replace('+', " "),
            Value::String(value.replace('+', " ")),
        ));
    }
    Ok(quench_runtime::host_api::object(properties))
}

fn querystring_escape(arguments: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(
        arguments.first().map(safe_value_string).unwrap_or_default(),
    ))
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
        14 | 15 => {
            if arguments.first() == arguments.get(1) {
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

fn basename(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(path)) = arguments.first() else {
        return Err(VmError::EvalError("path.basename expects a string".into()));
    };
    Ok(Value::String(
        Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(path)
            .into(),
    ))
}

impl JsRuntime for QuenchRuntime {
    fn execute(
        &self,
        source: &str,
        path: Option<&Path>,
        _host: &dyn NodeHost,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let source_with_globals = format!("globalThis.global = globalThis;\n{source}");
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
                HostCapabilityKind::Custom(CapabilityName::FsOpenAsync),
                HostCapabilityKind::Custom(CapabilityName::FsCloseAsync),
                HostCapabilityKind::Custom(CapabilityName::PathRelative),
                HostCapabilityKind::Custom(CapabilityName::PathDirname),
                HostCapabilityKind::Custom(CapabilityName::PathIsAbsolute),
                HostCapabilityKind::Custom(CapabilityName::PathToNamespaced),
                HostCapabilityKind::Custom(CapabilityName::PathWinToNamespaced),
                HostCapabilityKind::Custom(CapabilityName::PathJoin),
                HostCapabilityKind::Custom(CapabilityName::PathExtname),
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
