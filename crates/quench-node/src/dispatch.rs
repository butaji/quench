//! Single canonical dispatch table.
//!
//! Each capability id maps to one Rust handler. The host's
//! `Host::call`/`construct` routes through this table. Adding a
//! new Node API is a one-line entry in the appropriate per-domain
//! table; no plumbing changes.

use crate::dispatch_fs::fs_dispatch;
use crate::dispatch_handlers as handlers;

pub use crate::dispatch_handlers::{CallHandler, ConstructHandler};

#[path = "dispatch_caps.rs"]
mod dispatch_caps;
use dispatch_caps::*;

/// Single canonical mapping from capability id to call handler.
pub fn lookup(cap: u16) -> Option<CallHandler> {
    use handlers::*;
    let h = match cap {
        0x1a00 => crate::modules::compat_extra::sea_is_sea,
        CAP_EVENTS_FROM => events_from,
        CAP_EVENTS_EMIT => events_method_emit,
        CAP_CONSOLE_LOG | CAP_CONSOLE_INFO | CAP_CONSOLE_DEBUG => console_log,
        CAP_CONSOLE_WARN | CAP_CONSOLE_ERROR => console_warn,
        CAP_CONSOLE_TRACE => console_trace,
        CAP_UTIL_FORMAT => util_format,
        CAP_UTIL_INSPECT => util_inspect,
        CAP_STRING_DECODER => string_decoder_call,
        CAP_STRING_DECODER_WRITE => string_decoder_write,
        CAP_STRING_DECODER_END => string_decoder_end,
        0x2401 => crate::modules::sqlite::exec,
        0x2402 => crate::modules::sqlite::prepare,
        0x2403 => crate::modules::sqlite::run,
        0x2404 => crate::modules::sqlite::all,
        0x2405 => crate::modules::sqlite::close,
        _ => return events_dispatch(cap),
    };
    Some(h)
}

fn events_dispatch(cap: u16) -> Option<CallHandler> {
    use crate::modules::{event_target, events};
    Some(match cap {
        CAP_EVENTS_ON => events::method_on,
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
        CAP_URL_CAN_PARSE => url_can_parse,
        CAP_URL_PARSE_STATIC => url_parse_static,
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
        CAP_TTY_ISATTY => tty_isatty,
        _ => return crate::dispatch_buffer::buffer_dispatch(cap).or_else(|| process_dispatch(cap)),
    })
}

fn process_dispatch(cap: u16) -> Option<CallHandler> {
    use handlers::*;
    Some(match cap {
        CAP_PROCESS_EXIT => process_exit,
        CAP_PROCESS_KILL => process_kill,
        CAP_PROCESS_EXIT_CODE_GET => process_exit_code_get,
        CAP_PROCESS_EXIT_CODE_SET => process_exit_code_set,
        CAP_PROCESS_CWD => process_cwd,
        CAP_PROCESS_CHDIR => process_chdir,
        CAP_PROCESS_NEXT_TICK => process_next_tick,
        CAP_PROCESS_HRTIME => process_hrtime,
        CAP_PROCESS_UMASK => process_umask,
        CAP_PROCESS_ON => process_on,
        CAP_PROCESS_ONCE => process_once,
        CAP_PROCESS_GETUID => process_getuid,
        CAP_PROCESS_GETGID => process_getgid,
        CAP_PROCESS_UPTIME => process_uptime,
        CAP_PROCESS_MEMORYUSAGE => process_memory_usage,
        CAP_PROCESS_RESOURCE_USAGE => process_resource_usage,
        CAP_PROCESS_CPU_USAGE => process_cpu_usage,
        CAP_PROCESS_BINDING => process_binding,
        CAP_PROCESS_ACTIVE_RESOURCES => process_active_resources,
        CAP_PROCESS_REPORT => process_report,
        CAP_CRYPTO_RANDOM_BYTES => crypto_random_bytes,
        CAP_CRYPTO_RANDOM_FILL_SYNC => crypto_random_fill_sync,
        CAP_CRYPTO_UNSUPPORTED => crypto_unsupported,
        CAP_CRYPTO_CREATE_HASH => crypto_create_hash,
        CAP_CRYPTO_CREATE_HMAC => crypto_create_hmac,
        CAP_CRYPTO_TIMING_SAFE_EQUAL => crypto_timing_safe_equal,
        CAP_CRYPTO_RANDOM_UUID => crypto_random_uuid,
        CAP_CRYPTO_RANDOM_INT => crypto_random_int,
        CAP_CRYPTO_GET_HASHES => crypto_get_hashes,
        CAP_CRYPTO_GET_CIPHERS => crypto_get_ciphers,
        CAP_CRYPTO_HASH_UPDATE => crypto_hash_update,
        CAP_CRYPTO_HASH_DIGEST => crypto_hash_digest,
        CAP_CRYPTO_HMAC_UPDATE => crypto_hmac_update,
        CAP_CRYPTO_HMAC_DIGEST => crypto_hmac_digest,
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
        CAP_DNS_PROMISE_LOOKUP => dns_promise_lookup,
        CAP_DNS_PROMISE_RESOLVE4 => dns_promise_resolve4,
        CAP_HTTP_REQUEST => http_request,
        CAP_HTTP_GET => http_get,
        CAP_HTTP_SERVER => http_create_server,
        CAP_HTTP_RES_SET_HEADER => crate::modules::http::res_set_header,
        CAP_HTTP_RES_WRITE_HEAD => crate::modules::http::res_write_head,
        CAP_HTTP_RES_WRITE => crate::modules::http::res_write,
        CAP_HTTP_RES_END => crate::modules::http::res_end,
        CAP_HTTP_CONN => crate::modules::http::connection_handler,
        CAP_HTTP_DATA => crate::modules::http::data_handler,
        CAP_HTTP_REQ_WRITE => crate::modules::http_client::req_write,
        CAP_HTTP_REQ_END => crate::modules::http_client::req_end,
        CAP_HTTP_RESDATA => crate::modules::http_client::data_handler,
        CAP_HTTP_RESEND => crate::modules::http_client::res_end_handler,
        CAP_HTTP_RESUME => crate::modules::http_client::res_resume,
        CAP_DGRAM_CREATE => dgram_create,
        CAP_DGRAM_BIND => dgram_bind,
        CAP_DGRAM_SEND => dgram_send,
        CAP_DGRAM_CLOSE => dgram_close,
        CAP_DGRAM_ADDRESS => dgram_address,
        CAP_DGRAM_SET_TTL => crate::modules::dgram::set_ttl,
        CAP_DGRAM_SET_BROADCAST => crate::modules::dgram::set_broadcast,
        CAP_DGRAM_SET_MULTICAST_TTL => crate::modules::dgram::set_multicast_ttl,
        CAP_DGRAM_SET_MULTICAST_LOOPBACK => crate::modules::dgram::set_multicast_loopback,
        CAP_DGRAM_ADD_MEMBERSHIP => crate::modules::dgram::add_membership,
        CAP_DGRAM_DROP_MEMBERSHIP => crate::modules::dgram::drop_membership,
        CAP_DGRAM_GET_SEND_QUEUE_SIZE => crate::modules::dgram::get_send_queue_size,
        CAP_DGRAM_GET_SEND_QUEUE_COUNT => crate::modules::dgram::get_send_queue_count,
        CAP_DGRAM_REF => crate::modules::dgram::ref_socket,
        CAP_DGRAM_UNREF => crate::modules::dgram::unref_socket,
        _ => return network_dispatch_secondary(cap),
    })
}

fn network_dispatch_secondary(cap: u16) -> Option<CallHandler> {
    use handlers::*;
    Some(match cap {
        CAP_NET_CONNECT => net_connect,
        CAP_NET_SERVER => net_create_server_call,
        CAP_NET_ISIP => net_is_ip,
        CAP_NET_ISIPV4 => net_is_ipv4,
        CAP_NET_ISIPV6 => net_is_ipv6,
        CAP_NET_GET_ASF_TIMEOUT => net_get_asf_timeout,
        CAP_NET_SET_ASF_TIMEOUT => net_set_asf_timeout,
        CAP_NET_SERVER_LISTEN => crate::modules::net::server_listen,
        CAP_NET_SERVER_CLOSE => crate::modules::net::server_close,
        CAP_NET_SERVER_ADDRESS => crate::modules::net::server_address,
        CAP_NET_SERVER_GET_CONNECTIONS => crate::modules::net::server_get_connections,
        CAP_NET_SERVER_REF => crate::modules::net::server_ref,
        CAP_NET_SERVER_UNREF => crate::modules::net::server_unref,
        CAP_NET_SOCKET_WRITE => crate::modules::net::socket_write,
        CAP_NET_SOCKET_END => crate::modules::net::socket_end,
        CAP_NET_SOCKET_DESTROY => crate::modules::net::socket_destroy,
        CAP_NET_SOCKET_ADDRESS => crate::modules::net::socket_address,
        CAP_NET_SOCKET_SET_NO_DELAY => crate::modules::net::socket_set_no_delay,
        CAP_NET_SOCKET_SET_KEEP_ALIVE => crate::modules::net::socket_set_keep_alive,
        CAP_NET_SOCKET_SET_ENCODING => crate::modules::net::socket_set_encoding,
        CAP_NET_SOCKET_PAUSE => crate::modules::net::socket_pause,
        CAP_NET_SOCKET_SET_TIMEOUT => crate::modules::net::socket_set_timeout,
        CAP_NET_SOCKET_RESUME => crate::modules::net::socket_resume,
        CAP_NET_SOCKET_REF => crate::modules::net::socket_ref,
        CAP_NET_SOCKET_UNREF => crate::modules::net::socket_unref,
        _ => return network_dispatch_host(cap),
    })
}

fn network_dispatch_host(cap: u16) -> Option<CallHandler> {
    use handlers::*;
    Some(match cap {
        CAP_CRYPTO_SUBTLE_DIGEST => crypto_subtle_digest,
        CAP_CRYPTO_SUBTLE_IMPORT_KEY => crypto_subtle_import_key,
        CAP_CRYPTO_SUBTLE_ENCRYPT => crypto_subtle_encrypt,
        CAP_CRYPTO_SUBTLE_DECRYPT => crypto_subtle_decrypt,
        CAP_CRYPTO_SUBTLE_SIGN => crypto_subtle_sign,
        CAP_CRYPTO_SUBTLE_VERIFY => crypto_subtle_verify,
        CAP_CRYPTO_SUBTLE_GENERATE_KEY => crypto_subtle_generate_key,
        CAP_CRYPTO_SUBTLE_EXPORT_KEY => crypto_subtle_export_key,
        CAP_CRYPTO_SUBTLE_DERIVE_BITS => crypto_subtle_derive_bits,
        CAP_CRYPTO_SUBTLE_DERIVE_KEY => crypto_subtle_derive_key,
        CAP_REQUIRE => node_require,
        CAP_REQUIRE_FOR => node_require_for,
        CAP_CJS_WRAP => cjs_wrap,
        CAP_UTIL_GETCALLSITES => util_get_call_sites,
        CAP_BUFFER_ATOB => buffer_atob,
        CAP_BUFFER_BTOA => buffer_btoa,
        CAP_URL_PATH_TO_FILE_URL => url_path_to_file_url,
        CAP_CP_SPAWNSYNC => cp_spawn_sync,
        CAP_CP_EXECSYNC => cp_exec_sync,
        CAP_CP_EXEC | CAP_CP_SPAWN => cp_async,
        CAP_CP_EXECFILE => cp_exec_file,
        CAP_CP_EXECFILESYNC => cp_exec_file_sync,
        CAP_TEST_RUN => test_run,
        CAP_TEST_SKIP => test_skip,
        CAP_STRUCTURED_CLONE => structured_clone,
        CAP_FETCH => fetch,
        CAP_VM_RUN_IN_NEW_CONTEXT => crate::modules::vm::run_in_new_context,
        CAP_READLINE => crate::modules::readline::create_interface,
        CAP_READLINE_QUESTION => crate::modules::readline::question,
        CAP_READLINE_WRITE | CAP_READLINE_CLOSE => crate::modules::readline::noop,
        CAP_READLINE_CALLBACK => crate::modules::readline::question_callback,
        CAP_TTY_READSTREAM => crate::modules::tty::read_stream,
        CAP_TTY_WRITESTREAM => crate::modules::tty::write_stream,
        CAP_READLINE_DRIVER => crate::modules::readline::driver_handler,
        CAP_READLINE_DONE => crate::modules::readline::done_handler,
        _ => return network_dispatch_compression(cap),
    })
}

fn network_dispatch_compression(cap: u16) -> Option<CallHandler> {
    Some(match cap {
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
        CAP_ASSERT_NOT_DEEP_STRICT_EQUAL => assert::not_deep_strict_equal,
        CAP_ASSERT_THROWS => assert_validate::throws,
        CAP_ASSERT_DOES_NOT_THROW => assert_validate::does_not_throw,
        CAP_ASSERT_FAIL => assert::fail,
        CAP_ASSERT_IF_ERROR => assert::if_error,
        CAP_ASSERT_MATCH => assert_validate::matches,
        CAP_ASSERT_DOES_NOT_MATCH => assert_validate::does_not_match,
        CAP_ASSERT_REJECTS => assert::rejects,
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
        CAP_URL_NEW => url_new,
        CAP_TEXT_DECODER_NEW => crate::modules::text_decoder::new_text_decoder,
        CAP_TEXT_ENCODER_NEW => crate::modules::text_encoder::new_text_encoder,
        CAP_URL_SEARCH => url_search_params,
        CAP_READLINE => readline_create_interface,
        CAP_NET_SERVER => net_create_server,
        CAP_HTTP_SERVER => http_construct_server,
        CAP_ABORT_CONTROLLER => abort_controller_new,
        CAP_ABORT_SIGNAL => abort_signal_new,
        0x0805 => buffer_new_construct,
        0x2400 => crate::modules::sqlite::construct,
        _ => return None,
    })
}
