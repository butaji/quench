//! Single canonical dispatch table.
//!
//! Each capability id maps to one Rust handler. The host's
//! `Host::call`/`construct` routes through this table. Adding a
//! new Node API is a one-line entry in the appropriate per-domain
//! table; no plumbing changes.

#[path = "dispatch_handlers.rs"]
pub mod handlers;

use crate::host::HostState;
use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

pub type CallHandler = fn(&Rc<RefCell<HostState>>, &[Value]) -> Result<Value, VmError>;
pub type ConstructHandler = fn(&Rc<RefCell<HostState>>, &[Value]) -> Result<Value, VmError>;

const CAP_EVENTS_NEW: u16 = 0x0100;
const CAP_EVENTS_FROM: u16 = 0x0101;
const CAP_EVENTS_ON: u16 = 0x0102;
const CAP_EVENTS_EMIT: u16 = 0x0103;
const CAP_CONSOLE_LOG: u16 = 0x0200;
const CAP_CONSOLE_INFO: u16 = 0x0201;
const CAP_CONSOLE_WARN: u16 = 0x0202;
const CAP_CONSOLE_ERROR: u16 = 0x0203;
const CAP_CONSOLE_DEBUG: u16 = 0x0204;
const CAP_CONSOLE_TRACE: u16 = 0x0205;
const CAP_UTIL_FORMAT: u16 = 0x0300;
const CAP_UTIL_INSPECT: u16 = 0x0301;
const CAP_PATH_JOIN: u16 = 0x0400;
const CAP_PATH_RESOLVE: u16 = 0x0401;
const CAP_PATH_NORMALIZE: u16 = 0x0402;
const CAP_PATH_DIRNAME: u16 = 0x0403;
const CAP_PATH_BASENAME: u16 = 0x0404;
const CAP_PATH_EXTNAME: u16 = 0x0405;
const CAP_PATH_ISABSOLUTE: u16 = 0x0406;
const CAP_URL_PARSE: u16 = 0x0500;
const CAP_URL_FORMAT: u16 = 0x0501;
const CAP_URL_RESOLVE: u16 = 0x0502;
const CAP_URL_NEW: u16 = 0x0503;
const CAP_URL_SEARCH: u16 = 0x0504;
const CAP_QS_PARSE: u16 = 0x0600;
const CAP_QS_STRINGIFY: u16 = 0x0601;
const CAP_QS_ESCAPE: u16 = 0x0602;
const CAP_QS_UNESCAPE: u16 = 0x0603;
const CAP_TIMERS_SETTIMEOUT: u16 = 0x0700;
const CAP_TIMERS_CLEARTIMEOUT: u16 = 0x0701;
const CAP_TIMERS_SETINTERVAL: u16 = 0x0702;
const CAP_TIMERS_CLEARINTERVAL: u16 = 0x0703;
const CAP_TIMERS_SETIMMEDIATE: u16 = 0x0704;
const CAP_TIMERS_CLEARIMMEDIATE: u16 = 0x0705;
const CAP_TIMERS_TICK: u16 = 0x0706;
const CAP_BUFFER_FROM: u16 = 0x0800;
const CAP_BUFFER_ALLOC: u16 = 0x0801;
const CAP_BUFFER_BYTELENGTH: u16 = 0x0802;
const CAP_BUFFER_ISBUFFER: u16 = 0x0803;
const CAP_BUFFER_CONCAT: u16 = 0x0804;
const CAP_BUFFER_NEW: u16 = 0x0805;
const CAP_TTY_ISATTY: u16 = 0x0900;
const CAP_PROCESS_EXIT: u16 = 0x0A01;
const CAP_PROCESS_CWD: u16 = 0x0A02;
const CAP_PROCESS_CHDIR: u16 = 0x0A03;
const CAP_PROCESS_NEXT_TICK: u16 = 0x0A04;
const CAP_PROCESS_HRTIME: u16 = 0x0A05;
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
const CAP_STREAM_READABLE: u16 = 0x0C00;
const CAP_STREAM_WRITABLE: u16 = 0x0C01;
const CAP_STREAM_DUPLEX: u16 = 0x0C02;
const CAP_STREAM_TRANSFORM: u16 = 0x0C03;
const CAP_STREAM_PIPELINE: u16 = 0x0C04;
const CAP_STRING_DECODER: u16 = 0x0D00;
const CAP_DNS_LOOKUP: u16 = 0x0E00;
const CAP_DNS_RESOLVE4: u16 = 0x0E01;
const CAP_HTTP_REQUEST: u16 = 0x0F00;
const CAP_HTTP_GET: u16 = 0x0F01;
const CAP_HTTP_SERVER: u16 = 0x0F02;
const CAP_NET_CONNECT: u16 = 0x1000;
const CAP_NET_SERVER: u16 = 0x1001;
const CAP_NET_ISIP: u16 = 0x1002;
const CAP_NET_ISIPV4: u16 = 0x1003;
const CAP_NET_ISIPV6: u16 = 0x1004;
const CAP_FS_READFILE: u16 = 0x1100;
const CAP_FS_WRITEFILE: u16 = 0x1101;
const CAP_FS_STAT: u16 = 0x1102;
const CAP_FS_READDIR: u16 = 0x1103;
const CAP_FS_EXISTS: u16 = 0x1104;
const CAP_FS_MKDIR: u16 = 0x1105;
const CAP_FS_UNLINK: u16 = 0x1106;
const CAP_FS_READFILESYNC: u16 = 0x1107;
const CAP_FS_WRITEFILESYNC: u16 = 0x1108;
const CAP_FS_STATSYNC: u16 = 0x1109;
const CAP_FS_READDIRSYNC: u16 = 0x110A;
const CAP_FS_EXISTSSYNC: u16 = 0x110B;
const CAP_FS_REALSYNC: u16 = 0x110C;
const CAP_REQUIRE: u16 = 0x1200;
const CAP_READLINE: u16 = 0x1300;

/// Single canonical mapping from capability id to call handler.
pub fn lookup(cap: u16) -> Option<CallHandler> {
    use handlers::*;
    let h = match cap {
        CAP_EVENTS_FROM => events_from,
        CAP_EVENTS_ON => events_method_on,
        CAP_EVENTS_EMIT => events_method_emit,
        CAP_CONSOLE_LOG | CAP_CONSOLE_INFO | CAP_CONSOLE_DEBUG => console_log,
        CAP_CONSOLE_WARN | CAP_CONSOLE_ERROR => console_warn,
        CAP_CONSOLE_TRACE => console_trace,
        CAP_UTIL_FORMAT => util_format,
        CAP_UTIL_INSPECT => util_inspect,
        CAP_PATH_JOIN => path_join,
        CAP_PATH_RESOLVE => path_resolve,
        CAP_PATH_NORMALIZE => path_normalize,
        CAP_PATH_DIRNAME => path_dirname,
        CAP_PATH_BASENAME => path_basename,
        CAP_PATH_EXTNAME => path_extname,
        CAP_PATH_ISABSOLUTE => path_is_absolute,
        CAP_URL_PARSE => url_parse,
        CAP_URL_FORMAT => url_format,
        CAP_URL_RESOLVE => url_resolve,
        CAP_QS_PARSE => qs_parse,
        CAP_QS_STRINGIFY => qs_stringify,
        CAP_QS_ESCAPE => qs_escape,
        CAP_QS_UNESCAPE => qs_unescape,
        CAP_TIMERS_SETTIMEOUT => timers_set_timeout,
        CAP_TIMERS_CLEARTIMEOUT => timers_clear_timeout,
        CAP_TIMERS_SETINTERVAL => timers_set_interval,
        CAP_TIMERS_CLEARINTERVAL => timers_clear_interval,
        CAP_TIMERS_SETIMMEDIATE => timers_set_immediate,
        CAP_TIMERS_CLEARIMMEDIATE => timers_clear_immediate,
        CAP_TIMERS_TICK => timers_tick,
        _ => return os_buffer_dispatch(cap),
    };
    Some(h)
}

fn os_buffer_dispatch(cap: u16) -> Option<CallHandler> {
    use handlers::*;
    Some(match cap {
        CAP_BUFFER_FROM => buffer_from,
        CAP_BUFFER_ALLOC => buffer_alloc,
        CAP_BUFFER_BYTELENGTH => buffer_byte_length,
        CAP_BUFFER_ISBUFFER => buffer_is_buffer,
        CAP_BUFFER_CONCAT => buffer_concat,
        CAP_BUFFER_NEW => buffer_new,
        CAP_TTY_ISATTY => tty_isatty,
        _ => return process_dispatch(cap),
    })
}

fn process_dispatch(cap: u16) -> Option<CallHandler> {
    use handlers::*;
    Some(match cap {
        CAP_PROCESS_EXIT => process_exit,
        CAP_PROCESS_CWD => process_cwd,
        CAP_PROCESS_CHDIR => process_chdir,
        CAP_PROCESS_NEXT_TICK => process_next_tick,
        CAP_PROCESS_HRTIME => process_hrtime,
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
        CAP_FS_READFILE => fs_read_file,
        CAP_FS_WRITEFILE => fs_write_file,
        CAP_FS_STAT => fs_stat,
        CAP_FS_READDIR => fs_readdir,
        CAP_FS_EXISTS => fs_exists,
        CAP_FS_MKDIR => fs_mkdir,
        CAP_FS_UNLINK => fs_unlink,
        CAP_FS_READFILESYNC => fs_read_file_sync,
        CAP_FS_WRITEFILESYNC => fs_write_file_sync,
        CAP_FS_STATSYNC => fs_stat_sync,
        CAP_FS_READDIRSYNC => fs_readdir_sync,
        CAP_FS_EXISTSSYNC => fs_exists_sync,
        CAP_FS_REALSYNC => fs_realpath_sync,
        CAP_NET_CONNECT => net_connect,
        CAP_NET_ISIP => net_is_ip,
        CAP_NET_ISIPV4 => net_is_ipv4,
        CAP_NET_ISIPV6 => net_is_ipv6,
        CAP_REQUIRE => node_require,
        _ => return None,
    })
}

/// Single canonical mapping from capability id to construct handler.
pub fn lookup_construct(cap: u16) -> Option<ConstructHandler> {
    use handlers::*;
    Some(match cap {
        CAP_EVENTS_NEW => events_new,
        CAP_STREAM_READABLE => stream_readable,
        CAP_STREAM_WRITABLE => stream_writable,
        CAP_STREAM_DUPLEX => stream_duplex,
        CAP_STREAM_TRANSFORM => stream_transform,
        CAP_STRING_DECODER => string_decoder_new,
        CAP_URL_NEW => url_new,
        CAP_URL_SEARCH => url_search_params,
        CAP_NET_SERVER => net_create_server,
        CAP_HTTP_SERVER => http_create_server,
        CAP_BUFFER_NEW => buffer_new,
        CAP_READLINE => readline_create_interface,
        _ => return None,
    })
}
