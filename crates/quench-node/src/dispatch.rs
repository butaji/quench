//! Single canonical dispatch table.
//!
//! Each capability id maps to one Rust handler. The host's
//! `Host::call`/`construct` routes through this table. Adding a
//! new Node API is a one-line entry in the appropriate per-domain
//! table; no plumbing changes.

use crate::dispatch_handlers as handlers;

pub use crate::dispatch_handlers::{CallHandler, ConstructHandler};

const CAP_EVENTS_NEW: u16 = 0x0100;
const CAP_EVENTS_FROM: u16 = 0x0101;
const CAP_EVENTS_ON: u16 = 0x0102;
const CAP_EVENTS_EMIT: u16 = 0x0103;
const CAP_EVENTS_ONCE: u16 = 0x0105;
const CAP_EVENTS_REMOVE_LISTENER: u16 = 0x0106;
const CAP_EVENTS_REMOVE_ALL: u16 = 0x0107;
const CAP_EVENTS_LISTENERS: u16 = 0x0108;
const CAP_EVENTS_EVENT_NAMES: u16 = 0x0109;
const CAP_EVENTS_LISTENER_COUNT: u16 = 0x010A;
const CAP_EVENTS_PREPEND: u16 = 0x010B;
const CAP_EVENTS_PREPEND_ONCE: u16 = 0x010C;
const CAP_EVENTS_SET_MAX: u16 = 0x010D;
const CAP_EVENTS_GET_MAX: u16 = 0x010E;
const CAP_EVENTS_SET_MAX_STATIC: u16 = 0x010F;
const CAP_EVENTS_GET_LISTENERS: u16 = 0x0110;
const CAP_EVENTS_LISTENER_COUNT_STATIC: u16 = 0x0111;
const CAP_EVENTS_GET_MAX_STATIC: u16 = 0x0112;
const CAP_TARGET_ADD: u16 = 0x0113;
const CAP_TARGET_REMOVE: u16 = 0x0114;
const CAP_TARGET_DISPATCH: u16 = 0x0115;
const CAP_EVENT_TARGET_NEW: u16 = 0x0116;
const CAP_RUN_UNCAUGHT: u16 = 0x0117;
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
const CAP_PATH_RELATIVE: u16 = 0x0409;
const CAP_PATH_PARSE: u16 = 0x040A;
const CAP_PATH_FORMAT: u16 = 0x040B;
const CAP_PATH_TO_NAMESPACED: u16 = 0x040C;
const CAP_PATH_MATCHES_GLOB: u16 = 0x040D;
const CAP_PATH_WIN32_JOIN: u16 = 0x0410;
const CAP_PATH_WIN32_RESOLVE: u16 = 0x0411;
const CAP_PATH_WIN32_NORMALIZE: u16 = 0x0412;
const CAP_PATH_WIN32_DIRNAME: u16 = 0x0413;
const CAP_PATH_WIN32_BASENAME: u16 = 0x0414;
const CAP_PATH_WIN32_EXTNAME: u16 = 0x0415;
const CAP_PATH_WIN32_ISABSOLUTE: u16 = 0x0416;
const CAP_PATH_WIN32_RELATIVE: u16 = 0x0417;
const CAP_PATH_WIN32_PARSE: u16 = 0x0418;
const CAP_PATH_WIN32_FORMAT: u16 = 0x0419;
const CAP_PATH_WIN32_TO_NAMESPACED: u16 = 0x041A;
const CAP_PATH_WIN32_MATCHES_GLOB: u16 = 0x041B;
const CAP_URL_PARSE: u16 = 0x0500;
const CAP_URL_FORMAT: u16 = 0x0501;
const CAP_URL_RESOLVE: u16 = 0x0502;
const CAP_URL_NEW: u16 = 0x0503;
const CAP_URL_SEARCH: u16 = 0x0504;
const CAP_QS_PARSE: u16 = 0x0600;
const CAP_QS_STRINGIFY: u16 = 0x0601;
const CAP_QS_ESCAPE: u16 = 0x0602;
const CAP_QS_UNESCAPE: u16 = 0x0603;
const CAP_QS_UNESCAPE_BUFFER: u16 = 0x0604;
const CAP_TIMERS_SETTIMEOUT: u16 = 0x0700;
const CAP_TIMERS_CLEARTIMEOUT: u16 = 0x0701;
const CAP_TIMERS_SETINTERVAL: u16 = 0x0702;
const CAP_TIMERS_CLEARINTERVAL: u16 = 0x0703;
const CAP_TIMERS_SETIMMEDIATE: u16 = 0x0704;
const CAP_TIMERS_CLEARIMMEDIATE: u16 = 0x0705;
const CAP_TIMERS_TICK: u16 = 0x0706;
const CAP_TIMERS_UNREF: u16 = 0x0708;
const CAP_TIMERS_REF: u16 = 0x0709;
const CAP_TIMERS_HASREF: u16 = 0x070A;
const CAP_TIMERS_REFRESH: u16 = 0x070B;
const CAP_RUN_LOOP: u16 = 0x070C;
const CAP_RUN_EXIT: u16 = 0x070D;
const CAP_INTERNAL_UTIL_SLEEP: u16 = 0x070E;
const CAP_TIMERS_CLOSE: u16 = 0x070F;
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
const CAP_ASSERT_OK: u16 = 0x1400;
const CAP_ASSERT_STRICT_EQUAL: u16 = 0x1401;
const CAP_ASSERT_NOT_STRICT_EQUAL: u16 = 0x1402;
const CAP_ASSERT_EQUAL: u16 = 0x1403;
const CAP_ASSERT_NOT_EQUAL: u16 = 0x1404;
const CAP_ASSERT_DEEP_STRICT_EQUAL: u16 = 0x1405;
const CAP_ASSERT_NOT_DEEP_STRICT_EQUAL: u16 = 0x1406;
const CAP_ASSERT_THROWS: u16 = 0x1407;
const CAP_ASSERT_DOES_NOT_THROW: u16 = 0x1408;
const CAP_ASSERT_FAIL: u16 = 0x1409;
const CAP_ASSERT_IF_ERROR: u16 = 0x140A;
const CAP_ASSERT_MATCH: u16 = 0x140B;
const CAP_ASSERT_DOES_NOT_MATCH: u16 = 0x140C;
const CAP_CJS_WRAP: u16 = 0x1d00;
const CAP_UTIL_GETCALLSITES: u16 = 0x0303;
const CAP_BUFFER_ATOB: u16 = 0x0806;
const CAP_BUFFER_BTOA: u16 = 0x0807;
const CAP_BUFFER_TOSTRING: u16 = 0x0808;
const CAP_VM_RUN_IN_NEW_CONTEXT: u16 = 0x1600;
const CAP_URL_PATH_TO_FILE_URL: u16 = 0x0505;
const CAP_CP_SPAWNSYNC: u16 = 0x1e00;
const CAP_CP_EXECSYNC: u16 = 0x1e01;
const CAP_CP_EXEC: u16 = 0x1e02;
const CAP_CP_SPAWN: u16 = 0x1e03;
const CAP_PROCESS_UMASK: u16 = 0x0A06;
const CAP_PROCESS_ON: u16 = 0x0A07;
const CAP_PROCESS_ONCE: u16 = 0x0A08;
const CAP_STDOUT_WRITE: u16 = 0x0A09;
const CAP_STDERR_WRITE: u16 = 0x0A0A;
const CAP_NET_GET_ASF_TIMEOUT: u16 = 0x1005;
const CAP_NET_SET_ASF_TIMEOUT: u16 = 0x1006;
const CAP_STRUCTURED_CLONE: u16 = 0x1f00;
const CAP_FETCH: u16 = 0x1f01;
const CAP_ABORT_CONTROLLER: u16 = 0x1f02;
const CAP_ABORT_SIGNAL: u16 = 0x1f03;
const CAP_TEST_RUN: u16 = 0x1b00;

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
        CAP_URL_FORMAT => url_format,
        CAP_URL_RESOLVE => url_resolve,
        CAP_QS_PARSE => crate::modules::querystring_parse::parse,
        CAP_QS_STRINGIFY => crate::modules::querystring_stringify::stringify,
        CAP_QS_ESCAPE => crate::modules::querystring::escape,
        CAP_QS_UNESCAPE => crate::modules::querystring::unescape,
        CAP_QS_UNESCAPE_BUFFER => crate::modules::querystring::unescape_buffer,
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
        CAP_TIMERS_UNREF => timers_method_unref,
        CAP_TIMERS_REF => timers_method_ref,
        CAP_TIMERS_HASREF => timers_method_has_ref,
        CAP_TIMERS_REFRESH => timers_method_refresh,
        CAP_RUN_LOOP => timers_run_loop,
        CAP_RUN_EXIT => timers_run_exit,
        CAP_RUN_UNCAUGHT => uncaught_dispatch,
        CAP_INTERNAL_UTIL_SLEEP => internal_util_sleep,
        CAP_TIMERS_CLOSE => timers_method_close,
        _ => return os_buffer_dispatch(cap),
    })
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
        CAP_BUFFER_TOSTRING => crate::modules::buffer::to_string,
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
        CAP_PROCESS_UMASK => process_umask,
        CAP_PROCESS_ON => process_on,
        CAP_PROCESS_ONCE => process_once,
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
        CAP_HTTP_SERVER => http_create_server,
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
        CAP_NET_GET_ASF_TIMEOUT => net_get_asf_timeout,
        CAP_NET_SET_ASF_TIMEOUT => net_set_asf_timeout,
        CAP_REQUIRE => node_require,
        CAP_CJS_WRAP => cjs_wrap,
        CAP_UTIL_GETCALLSITES => util_get_call_sites,
        CAP_BUFFER_ATOB => buffer_atob,
        CAP_BUFFER_BTOA => buffer_btoa,
        CAP_URL_PATH_TO_FILE_URL => url_path_to_file_url,
        CAP_CP_SPAWNSYNC => cp_spawn_sync,
        CAP_CP_EXECSYNC => cp_exec_sync,
        CAP_CP_EXEC | CAP_CP_SPAWN => cp_async,
        CAP_TEST_RUN => test_run,
        CAP_STRUCTURED_CLONE => structured_clone,
        CAP_FETCH => fetch,
        CAP_VM_RUN_IN_NEW_CONTEXT => crate::modules::vm::run_in_new_context,
        _ => return assert_dispatch(cap),
    })
}

fn assert_dispatch(cap: u16) -> Option<CallHandler> {
    use crate::modules::{assert, assert_validate};
    Some(match cap {
        CAP_ASSERT_OK => assert::ok,
        CAP_ASSERT_STRICT_EQUAL => assert::strict_equal,
        CAP_ASSERT_NOT_STRICT_EQUAL => assert::not_strict_equal,
        CAP_ASSERT_EQUAL => assert::equal,
        CAP_ASSERT_NOT_EQUAL => assert::not_equal,
        CAP_ASSERT_DEEP_STRICT_EQUAL => assert::deep_strict_equal,
        CAP_ASSERT_NOT_DEEP_STRICT_EQUAL => assert::not_deep_strict_equal,
        CAP_ASSERT_THROWS => assert_validate::throws,
        CAP_ASSERT_DOES_NOT_THROW => assert_validate::does_not_throw,
        CAP_ASSERT_FAIL => assert::fail,
        CAP_ASSERT_IF_ERROR => assert::if_error,
        CAP_ASSERT_MATCH => assert_validate::matches,
        CAP_ASSERT_DOES_NOT_MATCH => assert_validate::does_not_match,
        _ => return None,
    })
}

/// Single canonical mapping from capability id to construct handler.
pub fn lookup_construct(cap: u16) -> Option<ConstructHandler> {
    use handlers::*;
    Some(match cap {
        CAP_EVENTS_NEW => events_new,
        CAP_EVENT_TARGET_NEW => crate::modules::event_target::new_target,
        CAP_STREAM_READABLE => stream_readable,
        CAP_STREAM_WRITABLE => stream_writable,
        CAP_STREAM_DUPLEX => stream_duplex,
        CAP_STREAM_TRANSFORM => stream_transform,
        CAP_STRING_DECODER => string_decoder_new,
        CAP_URL_NEW => url_new,
        CAP_URL_SEARCH => url_search_params,
        CAP_NET_SERVER => net_create_server,
        CAP_BUFFER_NEW => buffer_new_construct,
        CAP_READLINE => readline_create_interface,
        CAP_ABORT_CONTROLLER => abort_controller_new,
        CAP_ABORT_SIGNAL => abort_signal_new,
        _ => return None,
    })
}
