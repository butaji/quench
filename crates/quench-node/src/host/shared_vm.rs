//! The existing Node host adapted to the shared VM; Node policy stays in modules.

use super::NodeHost;
use quench_runtime::{HostFunction, HostFunctionId, NativeContext, RootedError, SystemHost};
use regex::Regex;
use std::sync::OnceLock;

pub(crate) mod commonjs;
pub(crate) use commonjs::source_kind;

pub(crate) fn bindings() -> &'static [HostFunction<NodeHost>] {
    quench_runtime::host_functions![
        method "callbackify" (1) => crate::modules::util_callbackify_shared_vm::callbackify,
        method "gc" (0) => crate::modules::process_shared_vm::collect_garbage,
        method "utilCallbackified" (2) => crate::modules::util_callbackify_shared_vm::callbackified,
        method "utilCallbackifySettled" (1) => crate::modules::util_callbackify_shared_vm::settled,
        method "utilCallbackifyInvoke" (0) => crate::modules::util_callbackify_shared_vm::invoke,
        method "stripVTControlCharacters" (1) => crate::modules::util_shared_vm::strip_vt,
        method "getSystemErrorName" (1) => crate::modules::util_system_error_shared_vm::get_system_error_name,
        method "parseEnv" (1) => crate::modules::util_shared_vm::parse_env,
        method "utilTypeCheck" (1) => crate::modules::util_shared_vm::type_check,
        method "uptime" (0) => crate::modules::process_shared_vm::uptime,
        method "processExitCodeGet" (0) => crate::modules::process_shared_vm::exit_code_get,
        method "processExitCodeSet" (1) => crate::modules::process_shared_vm::exit_code_set,
        method "processExit" (1) => crate::modules::process_shared_vm::exit,
        method "processStreamWrite" (1) => crate::modules::process_shared_vm::stream_write,
        method "processOpenStdin" (0) => crate::modules::process_stdin_shared_vm::open,
        method "processStdin" (0) => crate::modules::process_stdin_shared_vm::get,
        method "processStdinRead" (1) => crate::modules::process_stdin_shared_vm::read,
        method "processStdinPause" (0) => crate::modules::process_stdin_shared_vm::pause,
        method "processStdinRef" (0) => crate::modules::process_stdin_shared_vm::reference,
        method "processStdinPump" (0) => crate::modules::process_stdin_shared_vm::pump,
        method "processStdinDestroy" (2) => crate::modules::process_stdin_shared_vm::destroy,
        method "processCwd" (0) => crate::modules::process_shared_vm::cwd,
        method "processGetuid" (0) => crate::modules::process_shared_vm::getuid,
        method "processGeteuid" (0) => crate::modules::process_shared_vm::geteuid,
        method "processGetgid" (0) => crate::modules::process_shared_vm::getgid,
        method "processGetegid" (0) => crate::modules::process_shared_vm::getegid,
        method "processSetuid" (1) => crate::modules::process_shared_vm::setuid,
        method "processSeteuid" (1) => crate::modules::process_shared_vm::seteuid,
        method "processSetgid" (1) => crate::modules::process_shared_vm::setgid,
        method "processSetegid" (1) => crate::modules::process_shared_vm::setegid,
        method "processSetgroups" (1) => crate::modules::process_shared_vm::setgroups,
        method "processInitgroups" (2) => crate::modules::process_shared_vm::initgroups,
        method "processHrtimeNow" (0) => crate::modules::process_shared_vm::hrtime_now,
        method "processChdir" (1) => crate::modules::process_shared_vm::chdir,
        method "processUmask" (0) => crate::modules::process_shared_vm::umask,
        method "processKillNative" (2) => crate::modules::process_shared_vm::kill_native,
        method "processKill" (2) => crate::modules::process_shared_vm::kill,
        method "processRawDebug" (0) => crate::modules::process_shared_vm::raw_debug,
        method "processLoadEnvFile" (1) => crate::modules::process_shared_vm::load_env_file,
        method "processSetEnv" (2) => crate::modules::process_shared_vm::set_env,
        method "processDeleteEnv" (1) => crate::modules::process_shared_vm::delete_env,
        method "processSetUncaughtExceptionCaptureCallback" (1) => crate::modules::process_shared_vm::set_uncaught_exception_capture_callback,
        method "processHasUncaughtExceptionCaptureCallback" (0) => crate::modules::process_shared_vm::has_uncaught_exception_capture_callback,
        method "processGetActiveHandles" (0) => crate::modules::process_shared_vm::get_active_handles,
        method "processGetActiveNetworkResources" (0) => crate::modules::process_shared_vm::get_active_network_resources,
        method "nextTick" (1) => crate::modules::process_shared_vm::next_tick,
        method "on" (2) => crate::modules::process_shared_vm::on,
        method "processOnce" (2) => crate::modules::process_shared_vm::once,
        method "processRemoveListener" (2) => crate::modules::process_shared_vm::remove_listener,
        method "processRemoveAllListeners" (2) => crate::modules::process_shared_vm::remove_all_listeners,
        method "processEmit" (2) => crate::modules::process_shared_vm::emit,
        method "processEmitWarning" (2) => crate::modules::process_shared_vm::emit_warning,
        method "processDispatchWarning" (1) => crate::modules::process_shared_vm::dispatch_warning,
        method "require" (1) => commonjs::require,
        method "resolve" (2) => commonjs::resolve,
        method "assertionErrorInitialize" (2) => crate::modules::assert_shared_vm::initialize_assertion_error,
        method "assert" (2) => crate::modules::assert_shared_vm::ok,
        method "ok" (2) => crate::modules::assert_shared_vm::ok,
        method "strict" (1) => crate::modules::assert_shared_vm::ok,
        method "strictEqual" (3) => crate::modules::assert_shared_vm::strict_equal,
        method "notStrictEqual" (3) => crate::modules::assert_shared_vm::not_strict_equal,
        method "equal" (3) => crate::modules::assert_shared_vm::equal,
        method "notEqual" (3) => crate::modules::assert_shared_vm::not_equal,
        method "deepEqual" (3) => crate::modules::assert_shared_vm::legacy_deep_equal,
        method "notDeepEqual" (3) => crate::modules::assert_shared_vm::legacy_not_deep_equal,
        method "isDeepStrictEqual" (2) => crate::modules::assert_shared_vm::is_deep_strict_equal,
        method "partialDeepStrictEqual" (3) => crate::modules::assert_shared_vm::partial_deep_strict_equal,
        method "deepStrictEqual" (3) => crate::modules::assert_shared_vm::deep_strict_equal,
        method "notDeepStrictEqual" (3) => crate::modules::assert_shared_vm::not_deep_strict_equal,
        method "match" (3) => crate::modules::assert_shared_vm::match_string,
        method "doesNotMatch" (3) => crate::modules::assert_shared_vm::does_not_match,
        method "childProcessSpawnSync" (3) => crate::modules::child_process_shared_vm::spawn_sync,
        method "execFile" (4) => crate::modules::child_process_exec_shared_vm::exec_file,
        method "exec" (3) => crate::modules::child_process_exec_shared_vm::exec,
        method "childProcessPromiseExec" (1) => crate::modules::child_process_exec_shared_vm::promise_exec,
        method "childProcessPromiseExecSettled" (3) => crate::modules::child_process_exec_shared_vm::promise_settled,
        method "utilInspectQuote" (1) => crate::modules::util_shared_vm::inspect_quote,
        method "styleText" (3) => crate::modules::util_style_shared_vm::style_text,
        method "utilColorAliasGet" (0) => crate::modules::util_style_shared_vm::alias_get,
        method "utilColorAliasSet" (1) => crate::modules::util_style_shared_vm::alias_set,
        method "readableEncoding" (0) => crate::modules::stream_shared_vm::readable_encoding,
        method "execFileEvent" (2) => crate::modules::child_process_exec_shared_vm::event,
        method "childProcessShellArgsWarning" (1) => crate::modules::child_process_shared_vm::shell_args_warning,
        method "childProcessForkPlan" (3) => crate::modules::child_process_fork_shared_vm::plan,
        method "ipcSend" (4) => crate::modules::child_process_ipc_shared_vm::send,
        method "ipcDisconnect" (0) => crate::modules::child_process_ipc_shared_vm::disconnect,
        method "ipcDescriptor" (0) => crate::modules::child_process_ipc_shared_vm::descriptor,
        method "ipcReference" (0) => crate::modules::child_process_ipc_shared_vm::reference,
        method "ipcComplete" (2) => crate::modules::child_process_ipc_shared_vm::complete,
        method "ipcDecode" (2) => crate::modules::child_process_ipc_shared_vm::decode,
        method "ipcEmit" (3) => crate::modules::child_process_ipc_shared_vm::emit,
        method "childProcessSpawn" (3) => crate::modules::child_process_shared_vm::spawn_child,
        method "childProcessFlushStdio" (1) => crate::modules::child_process_shared_vm::queue_stdio_flush,
        method "childProcessFlushStdioRun" (1) => crate::modules::child_process_shared_vm::flush_stdio,
        method "childProcessRead" (2) => crate::modules::child_process_shared_vm::request_read,
        method "childProcessRegister" (5) => crate::modules::child_process_shared_vm::register_child,
        method "childProcessStreamClosed" (0) => crate::modules::child_process_shared_vm::stream_closed,
        method "childProcessWrite" (3) => crate::modules::child_process_shared_vm::write_child,
        method "childProcessInputComplete" (2) => crate::modules::child_process_input_shared_vm::complete,
        method "childProcessEnd" (2) => crate::modules::child_process_shared_vm::end_child,
        method "fail" (1) => crate::modules::assert_shared_vm::fail,
        method "throws" (3) => crate::modules::assert_shared_vm::throws,
        method "doesNotThrow" (2) => crate::modules::assert_shared_vm::does_not_throw,
        method "ifError" (1) => crate::modules::assert_shared_vm::if_error,
        method "pathJoin" (0) => crate::modules::path_shared_vm::join_operation,
        method "pathResolve" (0) => crate::modules::path_shared_vm::resolve_operation,
        method "pathRelative" (2) => crate::modules::path_shared_vm::relative_operation,
        method "pathBasename" (2) => crate::modules::path_shared_vm::basename_operation,
        method "pathDirname" (1) => crate::modules::path_shared_vm::dirname_operation,
        method "pathExtname" (1) => crate::modules::path_shared_vm::extname_operation,
        method "pathNormalize" (1) => crate::modules::path_shared_vm::normalize_operation,
        method "pathIsAbsolute" (1) => crate::modules::path_shared_vm::is_absolute_operation,
        method "pathToNamespacedPath" (1) => crate::modules::path_shared_vm::to_namespaced_path_operation,
        method "pathParse" (1) => crate::modules::path_shared_vm::parse_operation,
        method "pathFormat" (1) => crate::modules::path_shared_vm::format_operation,
        method "pathMatchesGlob" (2) => crate::modules::path_shared_vm::matches_glob_operation,
        method "urlParse" (3) => crate::modules::url_shared_vm::parse,
        method "urlResolve" (2) => crate::modules::url_shared_vm::resolve,
        method "urlResolveObject" (2) => crate::modules::url_shared_vm::resolve_object,
        method "urlFormat" (2) => crate::modules::url_shared_vm::format,
        method "domainToASCII" (1) => crate::modules::url_shared_vm::domain_to_ascii,
        method "domainToUnicode" (1) => crate::modules::url_shared_vm::domain_to_unicode,
        method "urlToHttpOptions" (1) => crate::modules::url_shared_vm::url_to_http_options,
        method "querystringParse" (4) => crate::modules::querystring_shared_vm::parse,
        method "querystringStringify" (4) => crate::modules::querystring_shared_vm::stringify,
        method "osString" (0) => crate::modules::os_shared_vm::string_operation,
        method "osTotalmem" (0) => crate::modules::os_shared_vm::totalmem_operation,
        method "osUptime" (0) => crate::modules::os_shared_vm::uptime_operation,
        method "osNetworkInterfaces" (0) => crate::modules::os_shared_vm::network_interfaces_operation,
        method "netGetAutoSelectFamilyAttemptTimeout" (0) => crate::modules::net_shared_vm::get_timeout,
        method "netSetAutoSelectFamilyAttemptTimeout" (1) => crate::modules::net_shared_vm::set_timeout,
        method "netIsIP" (1) => crate::modules::net_shared_vm::is_ip,
        method "netIsIPv4" (1) => crate::modules::net_shared_vm::is_ipv4,
        method "netIsIPv6" (1) => crate::modules::net_shared_vm::is_ipv6,
        method "netConnect" (3) => crate::modules::net_shared_vm::connect_operation,
        method "streamPipePlan" (3) => crate::modules::stream_pipe_shared_vm::plan,
        method "streamPipeEnd" (0) => crate::modules::stream_pipe_shared_vm::end,
        method "streamPipeClose" (0) => crate::modules::stream_pipe_shared_vm::close,
        method "streamPipeShouldEnd" (2) => crate::modules::stream_shared_vm::pipe_should_end,
        method "streamApplyTransformOptions" (2) => crate::modules::stream_shared_vm::apply_transform_options,
        method "streamGetDefaultHighWaterMark" (1) => crate::modules::stream_shared_vm::get_default_high_water_mark,
        method "streamSetDefaultHighWaterMark" (2) => crate::modules::stream_shared_vm::set_default_high_water_mark,
        method "streamReplayWrite" (4) => crate::modules::stream_shared_vm::replay_write,
        method "netSocketAccountWrite" (4) => crate::modules::net_shared_vm::account_write_operation,
        method "netSocketWrite" (3) => crate::modules::net_shared_vm::write_operation,
        method "netSocketEnd" (1) => crate::modules::net_shared_vm::end_operation,
        method "netSocketDestroy" (2) => crate::modules::net_shared_vm::destroy_operation,
        method "netSocketSetEncoding" (2) => crate::modules::net_shared_vm::set_encoding_operation,
        method "netServerListen" (3) => crate::modules::net_shared_vm::server_listen_operation,
        method "netServerClose" (1) => crate::modules::net_shared_vm::server_close_operation,
        method "netServerAddress" (1) => crate::modules::net_shared_vm::server_address_operation,
        method "httpCreateServer" (1) => crate::modules::http_shared_vm::create_server,
        method "httpServerListen" (2) => crate::modules::http_shared_vm::server_listen,
        method "httpServerAddress" (0) => crate::modules::http_shared_vm::server_address,
        method "httpServerClose" (0) => crate::modules::http_shared_vm::server_close,
        method "httpAgentCreate" (1) => crate::modules::http_shared_vm::agent_create,
        method "httpAgentDestroy" (1) => crate::modules::http_shared_vm::agent_destroy,
        method "httpRequestCreate" (2) => crate::modules::http_shared_vm::request_create,
        method "httpRequestNormalizeUrl" (1) => crate::modules::http_shared_vm::request_normalize_url,
        method "httpRequestWrite" (3) => crate::modules::http_shared_vm::request_write,
        method "httpRequestRemoveHeader" (2) => crate::modules::http_shared_vm::request_remove_header,
        method "httpRequestEnd" (2) => crate::modules::http_shared_vm::request_end,
        method "httpRequestDestroy" (1) => crate::modules::http_shared_vm::request_destroy,
        method "httpClientResponseDestroy" (1) => crate::modules::http_shared_vm::client_response_destroy,
        method "httpResponseSetHeader" (4) => crate::modules::http_shared_vm::response_set_header,
        method "httpResponseGetHeader" (2) => crate::modules::http_shared_vm::response_get_header,
        method "httpResponseRemoveHeader" (2) => crate::modules::http_shared_vm::response_remove_header,
        method "httpResponseWrite" (5) => crate::modules::http_shared_vm::response_write,
        method "httpResponseFinish" (7) => crate::modules::http_shared_vm::response_finish,
        method "httpResponseWriteHead" (5) => crate::modules::http_shared_vm::response_write_head,
        method "httpResponseDestroy" (1) => crate::modules::http_shared_vm::response_destroy,
        method "fsReadStreamOpen" (1) => crate::modules::fs_shared_vm::read_stream_open,
        method "fsReadStreamRead" (2) => crate::modules::fs_shared_vm::read_stream_read,
        method "fsReadStreamClose" (1) => crate::modules::fs_shared_vm::read_stream_close,
        method "fsWriteStreamOpen" (2) => crate::modules::fs_shared_vm::write_stream::open,
        method "fsWriteStreamWrite" (3) => crate::modules::fs_shared_vm::write_stream::write,
        method "fsWriteStreamClose" (1) => crate::modules::fs_shared_vm::write_stream::close,
        method "fsReadFileSync" (2) => crate::modules::fs_shared_vm::read_file_sync,
        method "fsValidateAbortSignal" (1) => crate::modules::fs_shared_vm::validate_read_signal,
        method "fsReadFileComplete" (5) => crate::modules::fs_shared_vm::complete_read_file,
        method "fsMkdirSync" (2) => crate::modules::fs_shared_vm::sync::mkdir_sync,
        method "fsRmdirSync" (1) => crate::modules::fs_shared_vm::sync::rmdir_sync,
        method "fsMkdtempSync" (1) => crate::modules::fs_shared_vm::sync::mkdtemp_sync,
        method "fsCopyFileSync" (3) => crate::modules::fs_shared_vm::sync::copy_file_sync,
        method "fsSymlinkSync" (3) => crate::modules::fs_shared_vm::sync::symlink_sync,
        method "fsLinkSync" (2) => crate::modules::fs_shared_vm::sync::link_sync,
        method "fsRenameSync" (2) => crate::modules::fs_shared_vm::sync::rename_sync,
        method "fsUnlinkSync" (1) => crate::modules::fs_shared_vm::sync::unlink_sync,
        method "fsChownSync" (3) => crate::modules::fs_shared_vm::stat::chown_sync,
        method "fsLchownSync" (3) => crate::modules::fs_shared_vm::stat::lchown_sync,
        method "fsFchownSync" (3) => crate::modules::fs_shared_vm::stat::fchown_sync,
        method "fsRmSync" (2) => crate::modules::fs_shared_vm::sync::rm_sync,
        method "fsWriteFileSync" (3) => crate::modules::fs_shared_vm::sync::write_file_sync,
        method "fsOpenSync" (3) => crate::modules::fs_shared_vm::sync::open_sync,
        method "fsCloseSync" (1) => crate::modules::fs_shared_vm::sync::close_sync,
        method "fsFstatSync" (1) => crate::modules::fs_shared_vm::stat::fstat_sync,
        method "fsReadDescriptor" (3) => crate::modules::fs_shared_vm::sync::read_descriptor,
        method "fsWriteDescriptor" (3) => crate::modules::fs_shared_vm::sync::write_descriptor,
        method "fsSyncDescriptor" (2) => crate::modules::fs_shared_vm::sync::sync_descriptor,
        method "fsStatMetadata" (1) => crate::modules::fs_shared_vm::stat::metadata,
        method "fsStatSync" (1) => crate::modules::fs_shared_vm::stat::stat_sync,
        method "fsStatfsSync" (1) => crate::modules::fs_shared_vm::stat::statfs_sync,
        method "fsTruncateSync" (2) => crate::modules::fs_shared_vm::stat::truncate_sync,
        method "fsFtruncateSync" (2) => crate::modules::fs_shared_vm::stat::ftruncate_sync,
        method "fsAccessSync" (2) => crate::modules::fs_shared_vm::stat::access_sync,
        method "fsChmodSync" (2) => crate::modules::fs_shared_vm::stat::chmod_sync,
        method "fsFchmodSync" (2) => crate::modules::fs_shared_vm::stat::fchmod_sync,
        method "fsUtimesSync" (3) => crate::modules::fs_shared_vm::stat::utimes_sync,
        method "fsLstatSync" (1) => crate::modules::fs_shared_vm::stat::lstat_sync,
        method "fsReaddirSync" (1) => crate::modules::fs_shared_vm::stat::read_dir_sync,
        method "fsReadlinkSync" (1) => crate::modules::fs_shared_vm::stat::read_link_sync,
        method "fsRealpathSync" (1) => crate::modules::fs_shared_vm::stat::realpath_sync,
        method "cryptoHash" (3) => crate::modules::crypto_shared_vm::hash,
        method "cryptoHmac" (3) => crate::modules::crypto_shared_vm::hmac,
        method "cryptoSign" (6) => crate::modules::crypto_shared_vm::sign,
        method "cryptoVerify" (7) => crate::modules::crypto_shared_vm::verify,
        method "cryptoRandomBytes" (1) => crate::modules::crypto_shared_vm::random_bytes,
        method "cryptoPbkdf2" (5) => crate::modules::crypto_shared_vm::pbkdf2,
        method "cryptoScrypt" (7) => crate::modules::crypto_shared_vm::scrypt,
        method "cryptoCipherProcess" (10) => crate::modules::crypto_shared_vm::cipher_process,
        method "cryptoGenerateRsaKeyPair" (4) => crate::modules::crypto_shared_vm::generate_rsa_key_pair,
        method "cryptoRsaCrypt" (8) => crate::modules::crypto_shared_vm::rsa_crypt,
        method "cryptoEcdh" (5) => crate::modules::crypto_shared_vm::ecdh,
        method "asyncResourceInit" (2) => crate::modules::async_hooks_shared_vm::initialize_resource,
        method "asyncResourceRunInAsyncScope" (2) => crate::modules::async_hooks_shared_vm::run_in_async_scope,
        method "asyncResourceEmitDestroy" (0) => crate::modules::async_hooks_shared_vm::emit_destroy,
        method "asyncLocalStorageInit" (1) => crate::modules::async_hooks_shared_vm::initialize_storage,
        method "asyncLocalStorageEnterWith" (1) => crate::modules::async_hooks_shared_vm::enter_with,
        method "asyncLocalStorageGetStore" (0) => crate::modules::async_hooks_shared_vm::get_store,
        method "asyncLocalStorageDisable" (0) => crate::modules::async_hooks_shared_vm::disable_storage,
        method "diagnosticsSubscribe" (2) => crate::modules::diagnostics_channel_shared_vm::subscribe,
        method "diagnosticsUnsubscribe" (2) => crate::modules::diagnostics_channel_shared_vm::unsubscribe,
        method "diagnosticsHasSubscribers" (1) => crate::modules::diagnostics_channel_shared_vm::has_subscribers,
        method "diagnosticsPublish" (2) => crate::modules::diagnostics_channel_shared_vm::publish,
        method "diagnosticsTracingChannel" (1) => crate::modules::diagnostics_channel_shared_vm::tracing_channel,
        method "diagnosticsTracingSubscribe" (1) => crate::modules::diagnostics_channel_shared_vm::tracing_subscribe,
        method "diagnosticsTraceCallback" (1) => crate::modules::diagnostics_channel_shared_vm::trace_callback,
        method "diagnosticsTraceComplete" (4) => crate::modules::diagnostics_channel_shared_vm::trace_complete,
        method "diagnosticsIsPromise" (1) => crate::modules::diagnostics_channel_shared_vm::is_promise,
        method "perfHooksNow" (0) => crate::modules::perf_hooks::now,
        method "dnsLookup" (3) => crate::modules::dns_shared_vm::lookup,
        method "zlibStreamOpen" (4) => crate::modules::zlib_stream_shared_vm::open,
        method "zlibPrepare" (2) => crate::modules::zlib_options_shared_vm::prepare,
        method "zlibStreamSync" (1) => crate::modules::zlib_stream_shared_vm::sync,
        method "zlibStreamClosed" (0) => crate::modules::zlib_stream_shared_vm::closed,
        method "zlibStreamRead" (1) => crate::modules::zlib_stream_shared_vm::read,
        method "zlibStreamFinish" (2) => crate::modules::zlib_stream_shared_vm::finish,
        method "zlibStreamDestroy" (2) => crate::modules::zlib_stream_shared_vm::destroy,
        method "zlibStreamClose" (1) => crate::modules::zlib_stream_shared_vm::close,
        method "zlibCallbackQueue" (5) => crate::modules::zlib_shared_vm::queue_callback,
        method "zlibCallbackRun" (0) => crate::modules::zlib_shared_vm::run_callback,
        method "zlibCrc32" (2) => crate::modules::zlib_shared_vm::crc32,
        method "bufferEncode" (2) => crate::modules::buffer_shared_vm::encode,
        method "bufferDecode" (2) => crate::modules::buffer_shared_vm::decode,
        method "bufferCanonicalEncoding" (1) => crate::modules::buffer_shared_vm::canonical_encoding,
        method "textDecoderCanonicalEncoding" (1) => crate::modules::text_decoder_shared_vm::canonical_encoding,
        method "textDecoderDecode" (3) => crate::modules::text_decoder_shared_vm::decode,
        method "stringDecoderChunk" (4) => crate::modules::string_decoder_shared_vm::decode_chunk,
        global "fetch" (2) => crate::modules::fetch_shared_vm::fetch,
        method "fetchExecutor" (2) => crate::modules::fetch_shared_vm::executor,
        method "fetchComplete" (1) => crate::modules::fetch_shared_vm::complete,
        method "fetchResponseText" (0) => crate::modules::fetch_shared_vm::response_text,
        method "fetchResponseJson" (0) => crate::modules::fetch_shared_vm::response_json,
        method "fetchHeaderGet" (1) => crate::modules::fetch_shared_vm::header_get,
        method "fetchHeaderHas" (1) => crate::modules::fetch_shared_vm::header_has,
        global "queueMicrotask" (1) => crate::modules::timers_shared_vm::queue_microtask,
        global "setImmediate" (1) => crate::modules::timers_shared_vm::set_immediate,
        global "clearImmediate" (1) => crate::modules::timers_shared_vm::clear_immediate,
        global "setTimeout" (1) => crate::modules::timers_shared_vm::set_timeout,
        global "clearTimeout" (1) => crate::modules::timers_shared_vm::clear_timer,
        global "setInterval" (1) => crate::modules::timers_shared_vm::set_interval,
        global "clearInterval" (1) => crate::modules::timers_shared_vm::clear_timer,
        method "timerRef" (0) => crate::modules::timers_shared_vm::timer_ref,
        method "timerUnref" (0) => crate::modules::timers_shared_vm::timer_unref,
        method "immediateRef" (0) => crate::modules::timers_shared_vm::immediate_ref,
        method "immediateUnref" (0) => crate::modules::timers_shared_vm::immediate_unref,
        method "timerHasRef" (0) => crate::modules::timers_shared_vm::timer_has_ref,
        method "immediateHasRef" (0) => crate::modules::timers_shared_vm::immediate_has_ref,
        method "timersPromiseSetTimeout" (2) => crate::modules::timers_shared_vm::promise_set_timeout,
        method "timersPromiseSetImmediate" (1) => crate::modules::timers_shared_vm::promise_set_immediate,
        method "timersPromiseSchedulerWait" (1) => crate::modules::timers_shared_vm::scheduler_wait,
        method "timersPromiseSchedulerYield" (0) => crate::modules::timers_shared_vm::scheduler_yield,
        method "timersPromiseSchedulerConstructor" (0) => crate::modules::timers_shared_vm::scheduler_constructor,
        method "timersPromiseSchedule" (2) => crate::modules::timers_shared_vm::schedule_promise_timeout,
        method "timersPromiseComplete" (0) => crate::modules::timers_shared_vm::complete_promise_operation,
        method "timersPromiseAbort" (0) => crate::modules::timers_shared_vm::abort_promise_operation,
        method "nodeTest" (2) => crate::modules::test_shared_vm::run,
        method "isBuiltin" (1) => crate::modules::module_shared_vm::is_builtin,
        method "createRequire" (1) => crate::modules::module_shared_vm::create_require,
        method "findPackageJSON" (1) => crate::modules::module_shared_vm::find_package_json,
        method "pathToFileURL" (2) => crate::modules::url_shared_vm::path_to_file_url,
        method "fileURLToPath" (1) => crate::modules::url_shared_vm::file_url_to_path,
        method "fileURLToPathBuffer" (2) => crate::modules::url_shared_vm::file_url_to_path_buffer,
        global "structuredClone" (1) => crate::modules::clone_shared_vm::structured_clone,
        method "ttyIsatty" (1) => crate::modules::tty_shared_vm::isatty,
        method "consoleTrace" (0) => crate::modules::console_shared_vm::trace,
    ]
}

pub(crate) fn operation(name: &str) -> HostFunctionId {
    let index = bindings()
        .iter()
        .position(|binding| binding.name == name)
        .expect("registered Node operation");
    HostFunctionId(u32::try_from(index).expect("Node binding index fits u32"))
}

impl quench_runtime::Host for NodeHost {
    fn write_line(&mut self, text: &str) {
        let output = self.shared_state().borrow().output.clone();
        match output {
            Some(output) => output(&format!("{text}\n")),
            None => quench_runtime::Host::write_line(&mut SystemHost, text),
        }
    }

    fn clock_millis(&mut self) -> f64 {
        quench_runtime::Host::clock_millis(&mut SystemHost)
    }

    fn resolve_dynamic_import(
        &mut self,
        referrer: &str,
        specifier: &str,
    ) -> Result<Option<quench_runtime::ModuleSource>, String> {
        use std::path::Path;

        let builtin_exports: std::collections::BTreeMap<String, Vec<String>> =
            serde_json::from_str(include_str!("builtin_esm_exports.json"))
                .expect("generated Node builtin export names are valid JSON");
        let builtin_name = specifier.strip_prefix("node:").unwrap_or(specifier);
        if crate::host::shared_vm::commonjs::is_builtin_specifier(specifier) {
            let exports = builtin_exports.get(builtin_name).map(Vec::as_slice).unwrap_or(&[]);
            let require_specifier = specifier;
            let mut source = format!(
                "const __quenchModule = globalThis[\"\\0quench:require\"]({});\nexport default __quenchModule;\n",
                serde_json::to_string(require_specifier).map_err(|error| error.to_string())?
            );
            for name in exports {
                source.push_str("export const ");
                source.push_str(name);
                source.push_str(" = __quenchModule[");
                source.push_str(&serde_json::to_string(name).map_err(|error| error.to_string())?);
                source.push_str("];\n");
            }
            return Ok(Some(quench_runtime::ModuleSource {
                name: format!("node:quench-builtin/{builtin_name}"),
                bytes: source.as_bytes().to_vec(),
                source,
            }));
        }

        let referrer = Path::new(referrer);
        let internal = specifier.starts_with("internal/");
        let base = if internal {
            self.node_library_root.join("lib")
        } else {
            referrer.parent().unwrap_or(Path::new(".")).to_path_buf()
        };
        let resolver = oxc_resolver::Resolver::new(oxc_resolver::ResolveOptions {
            extensions: vec![".mjs".into(), ".js".into(), ".json".into(), ".cjs".into()],
            main_files: vec!["index".into()],
            condition_names: vec!["node".into(), "import".into(), "default".into()],
            ..Default::default()
        });
        let resolution_specifier = if internal {
            format!("./{specifier}")
        } else {
            specifier.to_owned()
        };
        let resolution = match resolver.resolve(&base, &resolution_specifier) {
            Ok(resolution) => resolution,
            Err(oxc_resolver::ResolveError::NotFound(_) | oxc_resolver::ResolveError::Specifier(_)) => {
                return Ok(None)
            }
            Err(error) => return Err(error.to_string()),
        };
        let path = resolution.full_path();
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("module {}: {error}", path.display()))?;
        let extension = path.extension().and_then(|value| value.to_str()).unwrap_or("");
        if extension == "node" {
            return Ok(None);
        }
        let name = path.to_string_lossy().into_owned();
        let source = if extension == "json" {
            format!("export default {};", String::from_utf8_lossy(&bytes))
        } else {
            let kind = Self::source_kind(&path)?;
            if kind == quench_runtime::SourceKind::Module {
                String::from_utf8(bytes.clone())
                    .map_err(|error| format!("module {} is not UTF-8: {error}", path.display()))?
            } else {
                let filename = serde_json::to_string(&name).map_err(|error| error.to_string())?;
                let mut source = format!(
                    "import {{ createRequire as __quenchCreateRequire }} from 'node:module';\nconst __quenchRequire = __quenchCreateRequire({filename});\nconst __quenchModule = __quenchRequire({filename});\nexport default __quenchModule;\n"
                );
                for export in commonjs_named_exports(&String::from_utf8_lossy(&bytes)) {
                    source.push_str("export const ");
                    source.push_str(&export);
                    source.push_str(" = __quenchModule[");
                    source.push_str(
                        &serde_json::to_string(&export).map_err(|error| error.to_string())?,
                    );
                    source.push_str("];\n");
                }
                source
            }
        };
        // Keep original bytes for import.meta/source APIs while compiling the
        // generated CJS/JSON interop wrapper when one is required.
        Ok(Some(quench_runtime::ModuleSource {
            name,
            bytes,
            source,
        }))
    }

    fn import_meta_url(&mut self, source_name: &str) -> String {
        if source_name.starts_with("node:") || source_name.starts_with("<") {
            return source_name.to_owned();
        }
        let path = std::path::Path::new(source_name);
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(path))
                .unwrap_or_else(|_| path.to_path_buf())
        };
        url::Url::from_file_path(absolute)
            .map(|url| url.to_string())
            .unwrap_or_else(|_| source_name.to_owned())
    }

    fn capture_job_context(&mut self) -> Option<quench_runtime::HostExecutionContext> {
        crate::modules::async_hooks_shared_vm::capture_job_context(&self.shared_state)
            .map(quench_runtime::HostExecutionContext)
    }

    fn enter_job_context(
        &mut self,
        context: quench_runtime::HostExecutionContext,
    ) -> Option<quench_runtime::HostExecutionContext> {
        let previous =
            crate::modules::async_hooks_shared_vm::enter_job_context(&self.shared_state, context.0);
        Some(quench_runtime::HostExecutionContext(previous))
    }

    fn restore_job_context(&mut self, previous: Option<quench_runtime::HostExecutionContext>) {
        if let Some(previous) = previous {
            crate::modules::async_hooks_shared_vm::restore_job_context(
                &self.shared_state,
                previous.0,
            );
        }
    }

    fn release_job_context(
        &mut self,
        context: quench_runtime::HostExecutionContext,
    ) -> Vec<quench_runtime::RootId> {
        crate::modules::async_hooks_shared_vm::release_job_context(&self.shared_state, context.0)
    }

    fn functions(&self) -> &[HostFunction<Self>] {
        bindings()
    }

    fn initialize(context: &mut NativeContext<'_, Self>) -> Result<(), RootedError> {
        crate::modules::process_shared_vm::initialize(context)?;
        #[cfg(feature = "profile-memory")]
        context.profile_memory_checkpoint("node_process");
        let abort = context.evaluate_script_rooted(
            crate::polyfills::shared_vm::ABORT,
            "node:bootstrap/shared-vm/abort.js",
        )?;
        context.release_root(abort);
        #[cfg(feature = "profile-memory")]
        context.profile_memory_checkpoint("node_abort_polyfill");
        let event_target = context.evaluate_script_rooted(
            crate::polyfills::shared_vm::EVENT_TARGET,
            "node:bootstrap/shared-vm/event-target.js",
        )?;
        context.release_root(event_target);
        #[cfg(feature = "profile-memory")]
        context.profile_memory_checkpoint("node_event_target_polyfill");
        let event_emitter = context.evaluate_script_rooted(
            crate::polyfills::bootstrap::event_emitter::JS,
            "node:bootstrap/event-emitter.js",
        )?;
        context.release_root(event_emitter);
        // Native stdio writes still participate in the shared EventEmitter
        // protocol; consumers such as util.styleText validate that protocol.
        let global = context.global_root()?;
        let key = context.string_rooted("__nodeEventEmitter");
        let emitter = context.get_property_rooted(global, key)?;
        let key = context.string_rooted("prototype");
        let prototype = context.get_property_rooted(emitter, key)?;
        let key = context.string_rooted("process");
        let process = context.get_property_rooted(global, key)?;
        for name in ["stdout", "stderr"] {
            let key = context.string_rooted(name);
            let stream = context.get_property_rooted(process, key)?;
            context.set_prototype_rooted(stream, prototype)?;
        }
        crate::modules::child_process_ipc_shared_vm::install_process(context, process, emitter)?;
        #[cfg(feature = "profile-memory")]
        context.profile_memory_checkpoint("node_event_emitter_polyfill");
        crate::modules::text_decoder_shared_vm::install_global(context)?;
        #[cfg(feature = "profile-memory")]
        context.profile_memory_checkpoint("node_text_decoder");
        let expose_gc = context
            .host_mut()
            .shared_state()
            .borrow()
            .exec_argv
            .iter()
            .any(|flag| flag == "--expose-gc" || flag == "--expose_gc");
        if expose_gc {
            let collect = context.host_function(operation("gc"))?;
            let factory = context.evaluate_script_rooted(
                "collect => function gc(...args) { return collect(...args); }",
                "node:bootstrap/gc.js",
            )?;
            let undefined = context.undefined();
            let gc = context.call_rooted(factory, undefined, &[collect])?;
            let key = context.string_rooted("gc");
            context.define_data_property_rooted(global, key, gc, true, true, false)?;
        }
        commonjs::initialize(context)?;
        #[cfg(feature = "profile-memory")]
        context.profile_memory_checkpoint("node_commonjs");
        Ok(())
    }
}

fn commonjs_named_exports(source: &str) -> Vec<String> {
    static EXPORT_OBJECT: OnceLock<Regex> = OnceLock::new();
    static EXPORT_ASSIGNMENT: OnceLock<Regex> = OnceLock::new();
    static IDENTIFIER: OnceLock<Regex> = OnceLock::new();
    let object_pattern = EXPORT_OBJECT.get_or_init(|| {
        Regex::new(r"(?s)module\s*\.\s*exports\s*=\s*\{([^{}]*)\}")
            .expect("valid CommonJS export object pattern")
    });
    let assignment_pattern = EXPORT_ASSIGNMENT.get_or_init(|| {
        Regex::new(r"(?m)(?:module\s*\.\s*exports|exports)\s*\.\s*([A-Za-z_$][\w$]*)\s*=")
            .expect("valid CommonJS named export assignment pattern")
    });
    let identifier = IDENTIFIER.get_or_init(|| {
        Regex::new(r"^[A-Za-z_$][\w$]*$").expect("valid JavaScript identifier pattern")
    });
    let reserved = [
        "await", "break", "case", "catch", "class", "const", "continue", "debugger",
        "default", "delete", "do", "else", "enum", "export", "extends", "false", "finally",
        "for", "function", "if", "import", "in", "instanceof", "new", "null", "return",
        "super", "switch", "this", "throw", "true", "try", "typeof", "var", "void",
        "while", "with", "yield",
    ];
    let mut names = std::collections::BTreeSet::new();
    for captures in object_pattern.captures_iter(source) {
        let Some(properties) = captures.get(1) else {
            continue;
        };
        for property in properties.as_str().split(',') {
            let property = property.trim();
            let name = property
                .split_once(':')
                .map_or(property, |(name, _)| name)
                .trim();
            if identifier.is_match(name) && !reserved.contains(&name) {
                names.insert(name.to_owned());
            }
        }
    }
    for captures in assignment_pattern.captures_iter(source) {
        if let Some(name) = captures.get(1).map(|name| name.as_str()) {
            if !reserved.contains(&name) {
                names.insert(name.to_owned());
            }
        }
    }
    names.into_iter().collect()
}
