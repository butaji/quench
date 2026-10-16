//! The existing Node host adapted to the shared VM; Node policy stays in modules.

use super::NodeHost;
use quench_runtime::{HostFunction, HostFunctionId, NativeContext, RootedError, SystemHost};

pub(crate) mod commonjs;
mod commonjs_exports;
pub(crate) use commonjs::source_kind;

pub(crate) fn bindings() -> &'static [HostFunction<NodeHost>] {
    quench_runtime::host_functions![
        method "nodeLibraryRequire" (1) => commonjs::require_node_library,
        method "nodeTestMock" (0) => crate::modules::test_shared_vm::mock,
        method "nodeTestNamespace" (0) => crate::modules::test_shared_vm::namespace,
        method "contextifyScript" (0) => crate::modules::contextify_shared_vm::script,
        method "contextifyRunInContext" (0) => crate::modules::contextify_shared_vm::run_in_context,
        method "contextifyCompileFunction" (0) => crate::modules::contextify_shared_vm::compile_function,
        method "nodePrintEvalScript" (1) => crate::shared_run::print_eval_script,
        method "callbackify" (1) => crate::modules::util_callbackify_shared_vm::callbackify,
        method "gc" (0) => crate::modules::process_shared_vm::collect_garbage,
        method "utilCallbackified" (2) => crate::modules::util_callbackify_shared_vm::callbackified,
        method "utilCallbackifySettled" (1) => crate::modules::util_callbackify_shared_vm::settled,
        method "utilCallbackifyInvoke" (0) => crate::modules::util_callbackify_shared_vm::invoke,
        method "uvErrName" (0) => crate::modules::util_system_error_shared_vm::errname,
        method "uvGetErrorMap" (0) => crate::modules::util_system_error_shared_vm::error_map,
        method "uvGetErrorMessage" (0) => crate::modules::util_system_error_shared_vm::error_message,
        method "parseEnv" (1) => crate::modules::util_shared_vm::parse_env,
        method "parseArgs" (0) => crate::modules::util_parse_args_shared_vm::parse,
        method "utilParseArgsPositional" (1) => crate::modules::util_parse_args_shared_vm::positional_map,
        method "utilTypeCheck" (0) => crate::modules::util_shared_vm::type_check,
        method "utilProxyDetails" (0) => crate::modules::util_shared_vm::proxy_details,
        method "utilGetCallerLocation" (0) => crate::modules::util_shared_vm::get_caller_location,
        method "utilGetCallSites" (0) => crate::modules::util_shared_vm::get_call_sites,
        method "utilConstructorName" (0) => crate::modules::util_shared_vm::constructor_name,
        method "utilPreviewEntries" (0) => crate::modules::util_shared_vm::preview_entries,
        method "availableMemory" (0) => crate::modules::process_memory_shared_vm::available,
        method "constrainedMemory" (0) => crate::modules::process_memory_shared_vm::constrained,
        method "uptime" (0) => crate::modules::process_shared_vm::uptime,
        #[cfg(target_os = "linux")]
        method "processMetricsHrtime" (0) => crate::modules::process_metrics_shared_vm::hrtime,
        #[cfg(target_os = "linux")]
        method "processMetricsHrtimeBigInt" (0) => crate::modules::process_metrics_shared_vm::hrtime_bigint,
        #[cfg(target_os = "linux")]
        method "processMetricsCpu" (0) => crate::modules::process_metrics_shared_vm::process_cpu,
        #[cfg(target_os = "linux")]
        method "processMetricsThreadCpu" (0) => crate::modules::process_metrics_shared_vm::thread_cpu,
        #[cfg(target_os = "linux")]
        method "processMetricsResources" (0) => crate::modules::process_metrics_shared_vm::resources,
        #[cfg(target_os = "linux")]
        method "processMetricsRss" (0) => crate::modules::process_metrics_shared_vm::rss,
        method "processExitCodeGet" (0) => crate::modules::process_shared_vm::exit_code_get,
        method "processExitCodeSet" (1) => crate::modules::process_shared_vm::exit_code_set,
        method "processExit" (1) => crate::modules::process_shared_vm::exit,
        method "reallyExit" (1) => crate::modules::process_shared_vm::really_exit,
        method "processStreamWrite" (1) => crate::modules::process_shared_vm::stream_write,
        method "processOpenStdin" (0) => crate::modules::process_stdin_shared_vm::open,
        method "processStdin" (0) => crate::modules::process_stdin_shared_vm::get,
        method "processStdinRead" (1) => crate::modules::process_stdin_shared_vm::read,
        method "processStdinPause" (0) => crate::modules::process_stdin_shared_vm::pause,
        method "processStdinRef" (0) => crate::modules::process_stdin_shared_vm::reference,
        method "processStdinPump" (0) => crate::modules::process_stdin_shared_vm::pump,
        method "processStdinDestroy" (2) => crate::modules::process_stdin_shared_vm::destroy,
        method "processCwd" (0) => crate::modules::process_shared_vm::cwd,
        method "processGetgroups" (0) => crate::modules::process_shared_vm::getgroups,
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
        method "bufferConstructor" (0) => crate::modules::buffer_shared_vm::constructor,
        method "webStreamsInitialize" (0) => commonjs::initialize_web_streams,
        method "markAsUncloneable" (1) => crate::modules::clone_shared_vm::mark_as_uncloneable,
        method "createUnsafeArrayBuffer" (0) => crate::modules::buffer_alloc_shared_vm::allocate,
        method "arrayBufferAlignedOffset" (0) => crate::modules::buffer_alloc_shared_vm::aligned_offset,
        method "bufferIsLatin1" (1) => crate::modules::buffer_latin1_shared_vm::public,
        method "bufferBindingIsLatin1" (0) => crate::modules::buffer_latin1_shared_vm::binding,
        method "markAsUntransferable" (1) => crate::modules::clone_shared_vm::mark_as_untransferable,
        method "isMarkedAsUntransferable" (1) => crate::modules::clone_shared_vm::is_marked_as_untransferable,
        method "webApisInitialize" (0) => commonjs::initialize_web_apis,
        method "webFetchInitialize" (0) => commonjs::initialize_fetch_apis,
        method "processSurfaceInitialize" (0) => commonjs::initialize_process_surface,
        method "processRawDebug" (0) => crate::modules::process_shared_vm::raw_debug,
        method "processReport" (0) => crate::modules::report_configuration_shared_vm::report,
        method "reportConfigGet" (0) => crate::modules::report_configuration_shared_vm::get,
        method "reportConfigSet" (0) => crate::modules::report_configuration_shared_vm::set,
        method "processLoadEnvFile" (1) => crate::modules::process_shared_vm::load_env_file,
        method "processSetEnv" (2) => crate::modules::process_shared_vm::set_env,
        method "processDeleteEnv" (1) => crate::modules::process_shared_vm::delete_env,
        method "processBinding" (1) => crate::modules::process_binding_shared_vm::binding,
        method "processSetUncaughtExceptionCaptureCallback" (1) => crate::modules::process_shared_vm::set_uncaught_exception_capture_callback,
        method "processHasUncaughtExceptionCaptureCallback" (0) => crate::modules::process_shared_vm::has_uncaught_exception_capture_callback,
        method "processGetActiveHandles" (0) => crate::modules::process_shared_vm::get_active_handles,
        method "processGetActiveNetworkResources" (0) => crate::modules::process_shared_vm::get_active_network_resources,
        method "nextTick" (1) => crate::modules::process_shared_vm::next_tick,
        method "processStartListeningSignal" (1) => crate::modules::process_shared_vm::start_listening_signal,
        method "processStopListeningSignal" (1) => crate::modules::process_shared_vm::stop_listening_signal,
        method "processIpcListenerReference" (1) => crate::modules::child_process_ipc_shared_vm::listener_reference,
        method "require" (1) => commonjs::require,
        method "resolve" (2) => commonjs::resolve,
        method "childProcessSyncOptions" (2) => crate::modules::child_process_options_shared_vm::sync_options,
        method "childProcessReference" (2) => crate::modules::child_process_shared_vm::reference_child,
        method "childProcessHandleReference" (0) => crate::modules::child_process_shared_vm::reference_handle,
        method "childProcessHandleHasRef" (0) => crate::modules::child_process_shared_vm::handle_has_ref,
        method "childProcessDispose" (1) => crate::modules::child_process_shared_vm::dispose_child,
        method "childProcessValidateFile" (1) => crate::modules::child_process_options_shared_vm::validate_file,
        method "childProcessPrepareOptions" (2) => crate::modules::child_process_options_shared_vm::prepare,
        method "childProcessSpawnSync" (3) => crate::modules::child_process_shared_vm::spawn_sync,
        method "execFile" (4) => crate::modules::child_process_exec_shared_vm::exec_file,
        method "exec" (3) => crate::modules::child_process_exec_shared_vm::exec,
        method "childProcessPromiseExec" (1) => crate::modules::child_process_exec_shared_vm::promise_exec,
        method "childProcessPromiseExecSettled" (3) => crate::modules::child_process_exec_shared_vm::promise_settled,
        method "nodeErrorConstructor" (0) => crate::modules::node_errors_shared_vm::constructor,
        method "errorSourcePositions" (0) => crate::modules::node_errors_shared_vm::source_positions,
        method "errorStackFormat" (3) => crate::modules::node_errors_shared_vm::format_stack,
        method "errorPrepareStackTraceInitialize" (0) => crate::modules::node_errors_shared_vm::initialize_prepare_stack_trace,
        method "processEmitWarningInitialize" (0) => crate::modules::process_shared_vm::initialize_emit_warning,
        method "processWarningListener" (0) => crate::modules::process_shared_vm::warning_listener,
        method "nodeErrorBrand" (0) => crate::modules::node_errors_shared_vm::brand,
        method "asyncHookInit" (7) => crate::modules::async_hook_registry_shared_vm::initialize_hook,
        method "asyncHookEnable" (1) => crate::modules::async_hook_registry_shared_vm::enable,
        method "asyncHookDisable" (1) => crate::modules::async_hook_registry_shared_vm::disable,
        method "asyncHooksEnabled" (0) => crate::modules::async_hook_registry_shared_vm::enabled,
        method "asyncExecutionId" (0) => crate::modules::async_hooks_shared_vm::execution_id,
        method "asyncTriggerId" (0) => crate::modules::async_hooks_shared_vm::trigger_id,
        method "asyncExecutionResource" (0) => crate::modules::async_hooks_shared_vm::execution_resource,
        method "asyncExternalInit" (3) => crate::modules::async_hooks_shared_vm::external_init,
        method "asyncPromisePrune" (0) => crate::modules::async_hooks_shared_vm::prune_promises,
        method "asyncTickInvoke" (0) => crate::modules::shared_event_loop::invoke_tick,
        method "asyncHookDispatch" (2) => crate::modules::async_hook_registry_shared_vm::dispatch,
        method "asyncDestroyDispatch" (0) => crate::modules::async_hooks_shared_vm::destroy_dispatch,
        method "emitterAsyncConstructor" (0) => crate::modules::emitter_async_resource_shared_vm::constructor,
        method "emitterAsyncPrepare" (2) => crate::modules::emitter_async_resource_shared_vm::prepare,
        method "emitterAsyncInit" (3) => crate::modules::emitter_async_resource_shared_vm::initialize,
        method "emitterAsyncEmit" (3) => crate::modules::emitter_async_resource_shared_vm::emit,
        method "emitterAsyncDestroy" (1) => crate::modules::emitter_async_resource_shared_vm::destroy,
        method "emitterAsyncGet" (2) => crate::modules::emitter_async_resource_shared_vm::get_value,
        method "emitterAsyncLink" (2) => crate::modules::emitter_async_resource_shared_vm::link,
        method "emitterAsyncOwner" (1) => crate::modules::emitter_async_resource_shared_vm::owner,
        method "nodeErrorToString" (1) => crate::modules::node_errors_shared_vm::to_string,
        method "utilInspectQuote" (1) => crate::modules::util_shared_vm::inspect_quote,
        method "styleText" (3) => crate::modules::util_style_shared_vm::style_text,
        method "execFileEvent" (2) => crate::modules::child_process_exec_shared_vm::event,
        method "childProcessShellArgsWarning" (1) => crate::modules::child_process_shared_vm::shell_args_warning,
        method "childProcessForkPlan" (3) => crate::modules::child_process_fork_shared_vm::plan,
        method "ipcSend" (4) => crate::modules::child_process_ipc_shared_vm::send,
        method "ipcDisconnect" (0) => crate::modules::child_process_ipc_shared_vm::disconnect,
        method "ipcDescriptor" (0) => crate::modules::child_process_ipc_shared_vm::descriptor,
        method "ipcReference" (0) => crate::modules::child_process_ipc_shared_vm::reference,
        method "ipcReferenceCounted" (0) => crate::modules::child_process_ipc_shared_vm::reference_counted,
        method "eventsOnInitialize" (0) => commonjs::events_on_initialize,
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
        method "childProcessDestroyInput" (3) => crate::modules::child_process_shared_vm::destroy_input,
        method "childProcessPrepareExit" (3) => crate::modules::child_process_shared_vm::prepare_exit,
        method "vmCreateRealm" (0) => crate::modules::vm_shared_vm::create_realm,
        method "vmEvaluateScript" (3) => crate::modules::vm_shared_vm::evaluate_script,
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
        method "urlInitialize" (0) => crate::modules::url_shared_vm::initialize,
        method "urlResolve" (2) => crate::modules::url_shared_vm::resolve,
        method "urlResolveObject" (2) => crate::modules::url_shared_vm::resolve_object,
        method "urlFormat" (2) => crate::modules::url_shared_vm::format,
        method "domainToASCII" (1) => crate::modules::url_shared_vm::domain_to_ascii,
        method "domainToUnicode" (1) => crate::modules::url_shared_vm::domain_to_unicode,
        method "urlToHttpOptions" (1) => crate::modules::url_shared_vm::url_to_http_options,
        method "querystringParse" (4) => crate::modules::querystring_shared_vm::parse,
        method "querystringStringify" (4) => crate::modules::querystring_shared_vm::stringify,
        #[cfg(target_os = "linux")]
        method "osAvailableParallelism" (0) => crate::modules::os_binding_shared_vm::available_parallelism,
        #[cfg(target_os = "linux")]
        method "osBindingCpus" (0) => crate::modules::os_binding_shared_vm::cpus,
        #[cfg(target_os = "linux")]
        method "osBindingHome" (0) => crate::modules::os_binding_shared_vm::home,
        #[cfg(target_os = "linux")]
        method "osBindingHostname" (0) => crate::modules::os_binding_shared_vm::hostname,
        #[cfg(target_os = "linux")]
        method "osBindingInterfaces" (0) => crate::modules::os_binding_shared_vm::interfaces,
        #[cfg(target_os = "linux")]
        method "osBindingLoadavg" (0) => crate::modules::os_binding_shared_vm::loadavg,
        #[cfg(target_os = "linux")]
        method "osBindingGetPriority" (0) => crate::modules::os_binding_shared_vm::get_priority,
        #[cfg(target_os = "linux")]
        method "osBindingInformation" (0) => crate::modules::os_binding_shared_vm::information,
        #[cfg(target_os = "linux")]
        method "osBindingUserInfo" (0) => crate::modules::os_binding_shared_vm::user_info,
        #[cfg(target_os = "linux")]
        method "osBindingSetPriority" (0) => crate::modules::os_binding_shared_vm::set_priority,
        #[cfg(target_os = "linux")]
        method "osBindingTempdir" (0) => crate::modules::os_binding_shared_vm::tempdir,
        #[cfg(target_os = "linux")]
        method "osBindingUptime" (0) => crate::modules::os_binding_shared_vm::uptime,
        method "osString" (0) => crate::modules::os_shared_vm::string_operation,
        method "osFreemem" (0) => crate::modules::os_shared_vm::freemem_operation,
        method "osTotalmem" (0) => crate::modules::os_shared_vm::totalmem_operation,
        method "osUptime" (0) => crate::modules::os_shared_vm::uptime_operation,
        method "osNetworkInterfaces" (0) => crate::modules::os_shared_vm::network_interfaces_operation,
        method "osCpus" (0) => crate::modules::os_shared_vm::cpus_operation,
        method "btoa" (1) => crate::modules::base64_shared_vm::btoa,
        method "atob" (1) => crate::modules::base64_shared_vm::atob,
        method "moduleConstruct" (0) => crate::modules::module_shared_vm::construct,
        method "moduleConstructorGet" (0) => crate::modules::module_shared_vm::constructor_get,
        method "moduleParentGet" (0) => crate::modules::module_shared_vm::parent_get,
        method "moduleParentSet" (1) => crate::modules::module_shared_vm::parent_set,
        method "moduleRequire" (1) => crate::modules::module_shared_vm::require,
        method "moduleCompile" (3) => crate::modules::module_shared_vm::compile,
        method "netLookupResult" (3) => crate::modules::net_lookup_shared_vm::result,
        method "netLookupError" (2) => crate::modules::net_lookup_shared_vm::error,
        method "netGetAutoSelectFamily" (0) => crate::modules::net_lookup_shared_vm::get_default,
        method "netSetAutoSelectFamily" (1) => crate::modules::net_lookup_shared_vm::set_default,
        method "netGetAutoSelectFamilyAttemptTimeout" (0) => crate::modules::net_shared_vm::get_timeout,
        method "netSetAutoSelectFamilyAttemptTimeout" (1) => crate::modules::net_shared_vm::set_timeout,
        method "netIsIP" (1) => crate::modules::net_shared_vm::is_ip,
        method "netIsIPv4" (1) => crate::modules::net_shared_vm::is_ipv4,
        method "netIsIPv6" (1) => crate::modules::net_shared_vm::is_ipv6,
        method "netDeliverConnection" (0) => crate::modules::net_shared_vm::deliver_connection,
        method "netSocketReference" (2) => crate::modules::net_reference_shared_vm::reference_socket,
        method "netSocketUnrefTimeout" (1) => crate::modules::net_reference_shared_vm::unref_timeout,
        method "netSocketInstallHandle" (2) => crate::modules::net_reference_shared_vm::install_socket,
        method "netServerReference" (2) => crate::modules::net_reference_shared_vm::reference_server,
        method "netListenerReference" (0) => crate::modules::net_reference_shared_vm::reference_handle,
        method "netListenerHasRef" (0) => crate::modules::net_reference_shared_vm::handle_has_ref,
        method "netListenError" (2) => crate::modules::net_shared_vm::listen_error,
        method "netDeliverConnect" (0) => crate::modules::net_shared_vm::deliver_connect,
        method "netServerInitializeOptions" (2) => crate::modules::net_socket_options::initialize_server,
        method "netServerAcceptedOptions" (1) => crate::modules::net_socket_options::accepted_options,
        method "netServerOnConnection" (3) => crate::modules::net_server_admission::on_connection,
        method "netSocketCreateHandle" (1) => crate::modules::net_reference_shared_vm::create_socket_handle,
        method "netHandleClose" (0) => crate::modules::net_reference_shared_vm::close,
        method "netHandleReadStart" (0) => crate::modules::net_reference_shared_vm::read_start,
        method "netHandleReadStop" (0) => crate::modules::net_reference_shared_vm::read_stop,
        method "netSocketPeerName" (1) => crate::modules::net_socket_address::peer,
        method "netSocketLocalName" (1) => crate::modules::net_socket_address::local,
        method "netSocketEndpointView" (1) => crate::modules::net_socket_address::view,
        method "netSocketRead" (2) => crate::modules::net_socket_read::read,
        method "netSocketReadAfterConnect" (0) => crate::modules::net_socket_read::read_after_connect,
        method "netSocketInitializeRead" (2) => crate::modules::net_socket_read::initialize,
        method "netParentReadingGet" (0) => crate::modules::net_socket_read::parent_reading_get,
        method "netParentReadingSet" (1) => crate::modules::net_socket_read::parent_reading_set,
        method "netHandleCloseComplete" (0) => crate::modules::net_reference_shared_vm::close_complete,
        method "netHandleGetSockName" (0) => crate::modules::net_reference_shared_vm::get_sock_name,
        method "netHandleGetPeerName" (0) => crate::modules::net_reference_shared_vm::get_peer_name,
        method "netServerGetConnections" (2) => crate::modules::net_server_admission::get_connections,
        method "netServerConnectionsResult" (0) => crate::modules::net_server_admission::connections_result,
        method "netHandleSetNoDelay" (0) => crate::modules::net_reference_shared_vm::set_no_delay,
        method "netHandleSetKeepAlive" (0) => crate::modules::net_reference_shared_vm::set_keep_alive,
        method "netSocketInitializeOptions" (2) => crate::modules::net_socket_options::initialize,
        method "netSocketSetNoDelay" (2) => crate::modules::net_socket_options::no_delay,
        method "netSocketSetKeepAlive" (5) => crate::modules::net_socket_options::keep_alive,
        method "netServerInitEmitter" (1) => crate::modules::net_shared_vm::initialize_server_emitter,
        method "netSocketInitEnd" (3) => crate::modules::net_shared_vm::initialize_end,
        method "netSocketReadableEnd" (1) => crate::modules::net_shared_vm::readable_end,
        method "netSocketWriteAfterFin" (4) => crate::modules::net_shared_vm::write_after_fin,
        method "netSocketOptions" (2) => crate::modules::net_options_shared_vm::socket_options,
        method "netServerOptions" (1) => crate::modules::net_options_shared_vm::server_options,
        method "netNormalizeEndpoint" (4) => crate::modules::net_endpoint_shared_vm::normalize,
        method "netConnect" (4) => crate::modules::net_shared_vm::connect_operation,
        method "emitterInitialize" (2) => crate::modules::event_constructor_shared_vm::initialize,
        method "emitterRemoveAllListeners" (3) => crate::modules::event_emitter_shared_vm::remove_all_listeners,
        method "emitterRemoveListener" (3) => crate::modules::event_emitter_shared_vm::remove_listener,
        method "emitterEmit" (3) => crate::modules::event_emitter_shared_vm::emit,
        method "emitterEmitListeners" (4) => crate::modules::event_emitter_shared_vm::emit_listeners,
        method "emitterCaptureDefaultGet" (0) => crate::modules::event_constructor_shared_vm::default_get,
        method "emitterCaptureDefaultSet" (1) => crate::modules::event_constructor_shared_vm::default_set,
        method "emitterCaptureRejected" (1) => crate::modules::event_constructor_shared_vm::capture_rejected,
        method "emitterCaptureDeliver" (1) => crate::modules::event_constructor_shared_vm::capture_deliver,
        method "emitterAddListener" (4) => crate::modules::event_emitter_shared_vm::add_listener,
        method "emitterOnce" (4) => crate::modules::event_emitter_shared_vm::once,
        method "emitterOnceInvoke" (0) => crate::modules::event_emitter_shared_vm::once_invoke,
        method "eventsDefaultMaxGet" (0) => crate::modules::event_limits_shared_vm::default_get,
        method "consoleLoadUtil" (0) => crate::modules::console_shared_vm::load_util,
        method "eventGlobalsInitialize" (0) => crate::modules::events_shared_vm::initialize_event_globals,
        method "eventAbortStateInitialize" (0) => crate::modules::events_shared_vm::initialize_abort_state,
        method "eventsDefaultMaxSet" (1) => crate::modules::event_limits_shared_vm::default_set,
        method "emitterMaxGet" (1) => crate::modules::event_limits_shared_vm::emitter_get,
        method "emitterMaxSet" (2) => crate::modules::event_limits_shared_vm::emitter_set,
        method "eventsMaxGet" (1) => crate::modules::event_limits_shared_vm::global_get,
        method "eventsMaxSet" (1) => crate::modules::event_limits_shared_vm::global_set,
        method "emitterEventNames" (1) => crate::modules::event_emitter_shared_vm::names,
        method "emitterListeners" (2) => crate::modules::event_emitter_shared_vm::listeners,
        method "emitterRawListeners" (2) => crate::modules::event_emitter_shared_vm::raw_listeners,
        method "emitterListenerCount" (3) => crate::modules::event_emitter_shared_vm::count,
        method "eventsListenerCount" (2) => crate::modules::events_shared_vm::listener_count,
        method "eventsGetEventListeners" (2) => crate::modules::events_shared_vm::get_event_listeners,
        method "abortSignalBrand" (1) => crate::modules::events_shared_vm::abort_signal_brand,
        method "abortAddEventListener" (6) => crate::modules::events_abort_shared_vm::abort_add_event_listener,
        method "abortDispatchListeners" (4) => crate::modules::events_abort_shared_vm::abort_dispatch_listeners,
        method "eventTargetQueueError" (1) => crate::modules::events_abort_shared_vm::queue_listener_error,
        method "eventTargetRethrow" (0) => crate::modules::events_abort_shared_vm::rethrow_listener_error,
        #[cfg(quench_node_brotli)]
        method "BrotliEncoder" (0) => crate::modules::zlib_native_shared_vm::create_brotli_encoder,
        #[cfg(quench_node_brotli)]
        method "BrotliDecoder" (0) => crate::modules::zlib_native_shared_vm::create_brotli_decoder,
        method "ZstdCompress" (0) => crate::modules::zlib_native_shared_vm::create_zstd_compress,
        method "ZstdDecompress" (0) => crate::modules::zlib_native_shared_vm::create_zstd_decompress,
        method "Zlib" (0) => crate::modules::zlib_native_shared_vm::create,
        method "zlibNativeInit" (0) => crate::modules::zlib_native_shared_vm::initialize,
        method "zlibNativeWrite" (0) => crate::modules::zlib_native_shared_vm::write,
        method "zlibNativeWriteSync" (0) => crate::modules::zlib_native_shared_vm::write_sync,
        method "zlibNativeClose" (0) => crate::modules::zlib_native_shared_vm::close,
        method "zlibNativePoll" (0) => crate::modules::zlib_native_shared_vm::poll,
        method "eventsAddAbortListener" (2) => crate::modules::events_abort_shared_vm::add_abort_listener,
        method "abortListenerDispose" (0) => crate::modules::events_abort_shared_vm::abort_listener_dispose,
        method "abortedListenerJob" (0) => crate::modules::events_abort_shared_vm::aborted_listener_job,
        method "abortGetHandler" (1) => crate::modules::events_abort_shared_vm::abort_get_handler,
        method "abortSetHandler" (4) => crate::modules::events_abort_shared_vm::abort_set_handler,
        method "abortInvokeHandler" (2) => crate::modules::events_abort_shared_vm::abort_invoke_handler,
        method "abortRemoveEventListener" (4) => crate::modules::events_abort_shared_vm::abort_remove_event_listener,
        method "netSocketEndStream" (4) => crate::modules::net_shared_vm::end_stream,
        method "netSocketAccountWrite" (4) => crate::modules::net_shared_vm::account_write_operation,
        method "netSocketWrite" (3) => crate::modules::net_shared_vm::write_operation,
        method "netSocketEnd" (1) => crate::modules::net_shared_vm::end_operation,
        method "netSocketCloseCompleted" (2) => crate::modules::net_shared_vm::close_completed,
        method "netSocketDestroy" (3) => crate::modules::net_shared_vm::destroy_operation,
        method "netSocketSetEncoding" (2) => crate::modules::net_shared_vm::set_encoding_operation,
        method "netServerListening" (0) => crate::modules::net_server_listen::listening,
        method "netServerLookupCompleted" (2) => crate::modules::net_server_listen::lookup_completed,
        method "netServerClosePending" (1) => crate::modules::net_server_listen::close_pending,
        method "netServerCloseCallback" (0) => crate::modules::net_server_listen::close_callback,
        method "netServerListen" (0) => crate::modules::net_server_listen::listen,
        method "netServerClose" (1) => crate::modules::net_server_listen::close,
        method "netServerAddress" (0) => crate::modules::net_shared_vm::server_address_operation,
        method "httpCreateServer" (1) => crate::modules::http_shared_vm::create_server,
        method "httpServerListen" (2) => crate::modules::http_shared_vm::server_listen,
        method "httpServerAddress" (0) => crate::modules::http_shared_vm::server_address,
        method "httpServerClose" (0) => crate::modules::http_shared_vm::server_close,
        method "httpAgentCreate" (1) => crate::modules::http_shared_vm::agent_create,
        method "httpAgentDestroy" (1) => crate::modules::http_shared_vm::agent_destroy,
        method "getName" (0) => crate::modules::http_shared_vm::agent_get_name,
        method "_resolveFilename" (4) => commonjs::resolve_from_parent,
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
        method "httpResponseWrite" (7) => crate::modules::http_shared_vm::response_write,
        method "httpResponseFinish" (7) => crate::modules::http_shared_vm::response_finish,
        method "httpResponseWriteHead" (6) => crate::modules::http_shared_vm::response_write_head,
        method "httpResponseInitialize" (2) => crate::modules::http_shared_vm::response_initialize,
        method "httpResponseSend" (4) => crate::modules::http_shared_vm::response_send,
        method "bufferToString" (3) => crate::modules::buffer_string_shared_vm::to_string,
        method "assertStrictEqual" (2) => crate::modules::assert_shared_vm::strict_equal,
        method "assertNotStrictEqual" (2) => crate::modules::assert_shared_vm::not_strict_equal,
        method "vfsDescriptorGet" (1) => commonjs::vfs_descriptor_get,
        method "vfsDescriptorClose" (1) => commonjs::vfs_descriptor_close,
        method "vfsDescriptorOpen" (1) => commonjs::vfs_descriptor_open,
        method "vfsDescriptorRelease" (1) => commonjs::vfs_descriptor_release,
        method "vfsStreamPath" (2) => commonjs::vfs_stream_path,
        method "vfsBindPosition" (3) => commonjs::vfs_bind_position,
        method "vfsPositionGet" (0) => commonjs::vfs_position_get,
        method "vfsPositionSet" (1) => commonjs::vfs_position_set,
        method "utilExternalValue" (0) => crate::modules::util_shared_vm::external_value,
        method "cryptoCaptureKeyBrands" (2) => crate::modules::crypto_key_shared_vm::capture_brands,
        method "cryptoKeySlots" (0) => crate::modules::crypto_key_shared_vm::operation,
        method "bufferCompareRaw" (0) => crate::modules::buffer_compare_shared_vm::raw_compare,
        method "bufferCompare" (2) => crate::modules::buffer_compare_shared_vm::compare,
        method "bufferEquals" (1) => crate::modules::buffer_compare_shared_vm::equals,
        method "bufferStringSlice" (2) => crate::modules::buffer_string_shared_vm::string_slice,
        method "bufferFrom" (3) => crate::modules::buffer_from_shared_vm::from,
        method "bufferAllocUnsafe" (2) => crate::modules::buffer_pool_shared_vm::alloc_unsafe,
        method "bufferAllocUnsafeSlow" (2) => crate::modules::buffer_pool_shared_vm::alloc_unsafe_slow,
        method "bufferConcat" (2) => crate::modules::buffer_pool_shared_vm::concat,
        method "bufferCopyBytesFrom" (3) => crate::modules::buffer_from_shared_vm::copy_bytes,
        method "nodeEvalBuiltinGet" (0) => crate::modules::eval_globals_shared_vm::load,
        method "nodeEvalBuiltinCachedGet" (0) => crate::modules::eval_globals_shared_vm::cached,
        method "nodeEvalBuiltinSet" (1) => crate::modules::eval_globals_shared_vm::replace,
        method "httpValidateHeaderNamePublic" (0) => crate::modules::http_shared_vm::validate_header_name,
        method "httpValidateHeaderNameOriginal" (2) => crate::modules::http_shared_vm::validate_header_name,
        method "httpValidateHeaderValuePublic" (0) => crate::modules::http_shared_vm::validate_header_value,
        method "httpValidateHeaderValueOriginal" (3) => crate::modules::http_shared_vm::validate_header_value,
        method "httpResponseDestroy" (1) => crate::modules::http_shared_vm::response_destroy,
        method "fsReadStreamOpen" (1) => crate::modules::fs_shared_vm::read_stream_open,
        method "fsReadStreamRead" (2) => crate::modules::fs_shared_vm::read_stream_read,
        method "fsReadStreamClose" (1) => crate::modules::fs_shared_vm::read_stream_close,
        method "fsWriteStreamOpen" (2) => crate::modules::fs_shared_vm::write_stream::open,
        method "fsWriteStreamWrite" (3) => crate::modules::fs_shared_vm::write_stream::write,
        method "fsWriteStreamClose" (1) => crate::modules::fs_shared_vm::write_stream::close,
        method "fsWatchIgnore" (1) => crate::modules::fs_watch_shared_vm::ignore,
        method "fsWatchIgnorePattern" (1) => crate::modules::fs_watch_shared_vm::ignore_pattern,
        method "fsWatchIgnoreRegExp" (1) => crate::modules::fs_watch_shared_vm::ignore_regexp,
        method "fsWatch" (3) => crate::modules::fs_watch_shared_vm::watch,
        method "fsStatWatch" (3) => crate::modules::fs_stat_watch_shared_vm::watch,
        method "fsStatUnwatch" (2) => crate::modules::fs_stat_watch_shared_vm::unwatch,
        method "fsStatWatchStop" (1) => crate::modules::fs_stat_watch_shared_vm::stop,
        method "fsStatWatchRef" (2) => crate::modules::fs_stat_watch_shared_vm::reference,
        method "fsStatWatchStopNT" (1) => crate::modules::fs_stat_watch_shared_vm::stop_event,
        method "fsStatWatchDispatch" (0) => crate::modules::fs_stat_watch_shared_vm::dispatch,
        method "fsStatsConstruct" (4) => crate::modules::fs_shared_vm::stat::types::construct,
        method "fsStatsMode" (2) => crate::modules::fs_shared_vm::stat::types::mode,
        method "fsStatsCheckMode" (3) => crate::modules::fs_shared_vm::stat::types::check_mode,
        method "fsStatsDate" (2) => crate::modules::fs_shared_vm::stat::types::date,
        method "fsWatchClose" (1) => crate::modules::fs_watch_shared_vm::close,
        method "fsWatchRef" (2) => crate::modules::fs_watch_shared_vm::reference,
        method "fsWatchHandleClose" (1) => crate::modules::fs_watch_shared_vm::handle_close,
        method "fsWatchHandleRef" (2) => crate::modules::fs_watch_shared_vm::handle_ref,
        method "fsWatchHandleHasRef" (1) => crate::modules::fs_watch_shared_vm::handle_has_ref,
        method "fsWatchHandleInitialized" (1) => crate::modules::fs_watch_shared_vm::handle_initialized,
        method "fsWatchAbort" (0) => crate::modules::fs_watch_shared_vm::abort,
        method "fsWatchDetachAbort" (0) => crate::modules::fs_watch_shared_vm::detach_abort,
        method "fsWatchCloseNT" (1) => crate::modules::fs_watch_shared_vm::close_event,
        method "fsWatchDispatch" (0) => crate::modules::fs_watch_shared_vm::dispatch,
        method "fsRecursiveInitialize" (1) => crate::modules::fs_watch_shared_vm::recursive::initialize,
        method "fsRecursiveStart" (3) => crate::modules::fs_watch_shared_vm::recursive::start,
        method "fsRecursiveClose" (2) => crate::modules::fs_watch_shared_vm::recursive::close,
        method "fsRecursiveRef" (2) => crate::modules::fs_watch_shared_vm::recursive::reference,
        method "fsRecursiveNotify" (6) => crate::modules::fs_watch_shared_vm::recursive::notify,
        method "fsRecursiveIterate" (2) => crate::modules::fs_watch_shared_vm::recursive::iterator::iterate,
        method "fsRecursiveNext" (2) => crate::modules::fs_watch_shared_vm::recursive::iterator::next,
        method "fsRecursiveReturn" (2) => crate::modules::fs_watch_shared_vm::recursive::iterator::finish,
        method "fsRecursiveIteratorExecutor" (2) => crate::modules::fs_watch_shared_vm::recursive::iterator::executor,
        method "fsRecursiveIteratorChange" (2) => crate::modules::fs_watch_shared_vm::recursive::iterator::change,
        method "fsRecursiveIteratorAbort" (0) => crate::modules::fs_watch_shared_vm::recursive::iterator::abort,
        method "fsRecursivePromiseWatch" (2) => crate::modules::fs_watch_shared_vm::recursive::promise_watch,
        method "fsPromiseWatchNext" (1) => crate::modules::fs_watch_shared_vm::promise::next,
        method "fsPromiseWatchReturn" (1) => crate::modules::fs_watch_shared_vm::promise::finish,
        method "fsPromiseWatchExecutor" (2) => crate::modules::fs_watch_shared_vm::promise::executor,
        method "fsPromiseWatchEvent" (3) => crate::modules::fs_watch_shared_vm::promise::event,
        method "fsPromiseWatchCancel" (0) => crate::modules::fs_watch_shared_vm::promise::cancel,
        method "urlIsURL" (1) => crate::modules::url_shared_vm::is_url,
        method "toPathIfFileURL" (1) => crate::modules::url_shared_vm::to_path_if_file_url,
        method "cliOptionsValues" (0) => crate::modules::cli_options_shared_vm::values,
        method "cliParseNodeOptions" (0) => crate::modules::cli_options_shared_vm::parse_environment,
        method "fsFileHandleReadParameters" (0) => crate::modules::fs_shared_vm::file_handle_read::parameters,
        method "fsBindingInternalModuleStat" (0) => crate::modules::fs_directory_shared_vm::internal_module_stat,
        method "fsBindingReaddir" (0) => crate::modules::fs_directory_shared_vm::readdir,
        method "fsBindingReaddirRecursive" (0) => crate::modules::fs_directory_shared_vm::readdir_recursive,
        method "fsPromisesGlobInitialize" (0) => crate::modules::fs_shared_vm::initialize_promises_glob_policy,
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
        method "fsRm" (3) => crate::modules::fs_shared_vm::sync::rm_async,
        method "fsRmComplete" (0) => crate::modules::fs_shared_vm::sync::rm_complete,
        method "fsWriteFileSync" (3) => crate::modules::fs_shared_vm::sync::write_file_sync,
        method "fsOpenSync" (3) => crate::modules::fs_shared_vm::sync::open_sync,
        method "fsCloseSync" (1) => crate::modules::fs_shared_vm::sync::close_sync,
        method "fsFstatSync" (1) => crate::modules::fs_shared_vm::stat::fstat_sync,
        method "fsReadSync" (6) => crate::modules::fs_shared_vm::sync::read_sync,
        method "fsWriteSync" (5) => crate::modules::fs_write_shared_vm::write_sync,
        method "fsWriteDescriptor" (3) => crate::modules::fs_shared_vm::sync::write_descriptor,
        method "fsSyncDescriptor" (2) => crate::modules::fs_shared_vm::sync::sync_descriptor,
        method "fsStatSync" (1) => crate::modules::fs_shared_vm::stat::stat_sync,
        method "fsStatfsSync" (1) => crate::modules::fs_shared_vm::stat::statfs_sync,
        method "fsTruncateSync" (2) => crate::modules::fs_shared_vm::stat::truncate_sync,
        method "fsFtruncateSync" (2) => crate::modules::fs_shared_vm::stat::ftruncate_sync,
        method "fsAccessSync" (2) => crate::modules::fs_shared_vm::stat::access_sync,
        method "fsChmodSync" (2) => crate::modules::fs_shared_vm::stat::chmod_sync,
        method "fsFchmodSync" (2) => crate::modules::fs_shared_vm::stat::fchmod_sync,
        method "fsUtimesSync" (3) => crate::modules::fs_shared_vm::stat::utimes_sync,
        method "fsLstatSync" (1) => crate::modules::fs_shared_vm::stat::lstat_sync,
        method "fsReaddirPolicy" (2) => crate::modules::fs_shared_vm::readdir_policy,
        method "fsReadlinkSync" (1) => crate::modules::fs_shared_vm::stat::read_link_sync,
        method "fsRealpathSync" (1) => crate::modules::fs_shared_vm::stat::realpath_sync,
        method "tlsContextOuterInit" (0) => crate::modules::tls_shared_vm::facade::initialize,
        method "tlsServerCreate" (2) => crate::modules::tls_shared_vm::server::create,
        method "tlsServerConstruct" (4) => crate::modules::tls_shared_vm::server::construct,
        method "tlsServerSetContext" (2) => crate::modules::tls_shared_vm::server::set_context_operation,
        method "tlsServerConnection" (1) => crate::modules::tls_shared_vm::server::connection,
        method "tlsServerSocketError" (1) => crate::modules::tls_shared_vm::server::socket_error,
        method "tlsSocketTransportError" (1) => crate::modules::tls_shared_vm::socket::transport_error,
        method "tlsSocketConnect" (0) => crate::modules::tls_shared_vm::socket::connect,
        method "tlsSocketConstruct" (4) => crate::modules::tls_shared_vm::socket::construct,
        method "tlsSocketStart" (0) => crate::modules::tls_shared_vm::socket::start,
        method "tlsSocketInitRead" (2) => crate::modules::tls_shared_vm::socket::init_read,
        method "netDeliverData" (4) => crate::modules::net_shared_vm::deliver_data,
        method "netDeliverEof" (2) => crate::modules::net_shared_vm::deliver_eof,
        method "tlsSocketError" (2) => crate::modules::tls_shared_vm::socket::tls_error,
        method "tlsSocketReleaseControl" (1) => crate::modules::tls_shared_vm::socket::release_control,
        method "tlsSocketSecure" (0) => crate::modules::tls_shared_vm::socket::secure,
        method "tlsSocketUpgrade" (3) => crate::modules::tls_shared_vm::socket::upgrade,
        method "tlsSocketAuthorize" (1) => crate::modules::tls_shared_vm::socket::authorize,
        method "tlsSocketInfo" (2) => crate::modules::tls_shared_vm::socket::info,
        method "tlsHandleVerifyError" (0) => crate::modules::tls_shared_vm::handle::verify_error,
        method "tlsMatchName" (1) => crate::modules::tls_shared_vm::identity::match_name,
        method "tlsLowercaseName" (1) => crate::modules::tls_shared_vm::identity::lowercase_name,
        method "tlsCollectAltName" (1) => crate::modules::tls_shared_vm::identity::collect_alt_name,
        method "tlsCheckServerIdentity" (2) => crate::modules::tls_shared_vm::identity::check_server_identity,
        method "httpIncomingCreate" (5) => crate::modules::http_shared_vm::incoming::create,
        method "httpIncomingRead" (1) => crate::modules::http_shared_vm::incoming::read,
        method "httpIncomingDestroy" (2) => crate::modules::http_shared_vm::incoming::destroy,
        method "bufferCopyArrayBuffer" (0) => crate::modules::buffer_fill_shared_vm::copy_array_buffer,
        method "encodingUtf8" (0) => crate::modules::encoding_binding_shared_vm::encode,
        method "encodingInto" (0) => crate::modules::encoding_binding_shared_vm::encode_into,
        method "encodingDecodeUtf8" (0) => crate::modules::encoding_binding_shared_vm::decode,
        method "utilPromiseDetails" (0) => crate::modules::util_shared_vm::promise_details,
        method "utilDefineLazyProperties" (0) => crate::modules::util_shared_vm::define_lazy_properties,
        method "utilLazyPropertyInitialize" (0) => crate::modules::util_shared_vm::initialize_lazy_property,
        method "utilMarkPromiseHandled" (0) => crate::modules::util_shared_vm::mark_promise_handled,
        method "utilOwnNonIndexProperties" (0) => crate::modules::util_shared_vm::own_non_index_properties,
        method "cryptoSetEngineBinding" (0) => crate::modules::crypto_engine_shared_vm::set_engine_binding,
        method "tlsBundledRoots" (0) => crate::modules::tls_shared_vm::bundled::bundled,
        method "tlsGetCiphers" (0) => crate::modules::tls_shared_vm::discovery::ciphers,
        method "tlsContextCreate" (1) => crate::modules::tls_shared_vm::facade::create,
        method "tlsContextConfigure" (0) => crate::modules::tls_shared_vm::configuration::method,
        method "tlsContextRegister" (0) => crate::modules::tls_shared_vm::contexts::register,
        method "tlsContextInit" (0) => crate::modules::tls_shared_vm::contexts::initialize,
        method "tlsContextSetOptions" (0) => crate::modules::tls_shared_vm::contexts::set_options,
        method "tlsContextGetMinProto" (0) => crate::modules::tls_shared_vm::contexts::minimum,
        method "tlsContextGetMaxProto" (0) => crate::modules::tls_shared_vm::contexts::maximum,
        method "cryptoHandleSymbol" (0) => crate::modules::crypto_shared_vm::handle_symbol,
        method "cryptoSetEngine" (2) => crate::modules::crypto_engine_shared_vm::set_engine,
        method "cryptoHash" (3) => crate::modules::crypto_shared_vm::hash,
        method "cryptoHmac" (3) => crate::modules::crypto_shared_vm::hmac,
        method "cryptoSign" (6) => crate::modules::crypto_shared_vm::sign,
        method "cryptoVerify" (7) => crate::modules::crypto_shared_vm::verify,
        method "cryptoRandomBytes" (1) => crate::modules::crypto_shared_vm::random_bytes,
        method "get crypto" (0) => crate::modules::crypto_shared_vm::global_crypto,
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
        method "perfHooksEntries" (0) => crate::modules::perf_timeline_query::entries,
        method "perfHooksCompareEntries" (2) => crate::modules::perf_timeline_query::compare,
        method "perfHooksNow" (0) => crate::modules::perf_hooks::now,
        method "dnsLookup" (3) => crate::modules::dns_shared_vm::lookup,
        #[cfg(quench_icu)]
        method "icuGetConverter" (2) => crate::modules::icu_binding_shared_vm::get_converter,
        #[cfg(quench_icu)]
        method "icuDecode" (4) => crate::modules::icu_binding_shared_vm::decode,
        #[cfg(quench_icu)]
        method "icuStringWidth" (3) => crate::modules::icu_binding_shared_vm::string_width,
        #[cfg(quench_icu)]
        method "icuTranscode" (3) => crate::modules::icu_binding_shared_vm::transcode,
        #[cfg(quench_icu)]
        method "icuErrorName" (1) => crate::modules::icu_binding_shared_vm::error_name,
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
        method "bufferLegacyConstruct" (3) => crate::modules::buffer_constructor_shared_vm::construct,
        method "bufferIsBuffer" (1) => crate::modules::buffer_constructor_shared_vm::is_buffer,
        method "bufferSpeciesGet" (0) => crate::modules::buffer_constructor_shared_vm::species,
        method "bufferOf" (0) => crate::modules::buffer_constructor_shared_vm::of,
        method "bufferInspect" (2) => crate::modules::buffer_inspect_shared_vm::inspect,
        method "bufferInspectMaxGet" (0) => crate::modules::buffer_inspect_shared_vm::maximum,
        method "bufferInspectMaxSet" (1) => crate::modules::buffer_inspect_shared_vm::set_maximum,
        method "bufferFill" (4) => crate::modules::buffer_fill_shared_vm::fill,
        method "bufferBindingFill" (0) => crate::modules::buffer_fill_shared_vm::binding_fill,
        method "bufferWrite" (4) => crate::modules::buffer_write_shared_vm::write,
        method "bufferSwap" (0) => crate::modules::buffer_swap_shared_vm::operation,
        method "bufferBigInt" (3) => crate::modules::buffer_bigint_shared_vm::operation,
        method "bufferBackingGet" (0) => crate::modules::buffer_shared_vm::backing_get,
        method "bufferSearch" (5) => crate::modules::buffer_search_shared_vm::search,
        method "bufferEncode" (2) => crate::modules::buffer_shared_vm::encode,
        method "bufferEncodeView" (2) => crate::modules::buffer_shared_vm::encode_view,
        method "bufferDecode" (2) => crate::modules::buffer_shared_vm::decode,
        method "bufferCanonicalEncoding" (1) => crate::modules::buffer_shared_vm::canonical_encoding,
        method "textDecoderCanonicalEncoding" (1) => crate::modules::text_decoder_shared_vm::canonical_encoding,
        method "textDecoderDecode" (4) => crate::modules::text_decoder_shared_vm::decode,
        method "textDecoderInitialize" (0) => crate::modules::text_decoder_shared_vm::initialize,
        method "stringDecoderDecode" (0) => crate::modules::string_decoder_shared_vm::decode,
        method "stringDecoderFlush" (0) => crate::modules::string_decoder_shared_vm::flush,
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
        method "processGetActiveTimerResources" (0) => crate::modules::timers_shared_vm::active_resources,
        method "timerPromisifyGet" (0) => crate::modules::timers_shared_vm::promisify_get,
        method "timerLibuvNow" (0) => crate::modules::timers_shared_vm::libuv_now,
        method "internalSetUnrefTimeout" (2) => crate::modules::timers_shared_vm::set_unref_timeout,
        method "timerRefresh" (0) => crate::modules::timers_shared_vm::refresh,
        method "timerInvoke" (0) => crate::modules::timers_shared_vm::invoke_timeout,
        method "timerRepeatDelay" (0) => crate::modules::timers_shared_vm::repeat_delay,
        method "timerToPrimitive" (1) => crate::modules::timers_shared_vm::to_primitive,
        method "timerPrimitiveCleanup" (2) => crate::modules::timers_shared_vm::primitive_cleanup,
        method "timerDispose" (1) => crate::modules::timers_shared_vm::timer_dispose,
        method "immediateDispose" (1) => crate::modules::timers_shared_vm::immediate_dispose,
        method "timerClose" (1) => crate::modules::timers_shared_vm::timer_close,
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
        method "isBuiltin" (1) => crate::modules::module_shared_vm::is_builtin,
        method "createRequire" (1) => crate::modules::module_shared_vm::create_require,
        method "moduleNodePaths" (1) => crate::modules::module_shared_vm::node_module_paths,
        method "findPackageJSON" (1) => crate::modules::module_shared_vm::find_package_json,
        method "pathToFileURL" (2) => crate::modules::url_shared_vm::path_to_file_url,
        method "fileURLToPath" (1) => crate::modules::url_shared_vm::file_url_to_path,
        method "fileURLToPathBuffer" (2) => crate::modules::url_shared_vm::file_url_to_path_buffer,
        global "structuredClone" (1) => crate::modules::clone_shared_vm::structured_clone,
        method "ttyIsatty" (1) => crate::modules::tty_shared_vm::isatty,
        method "consoleNoop" (0) => crate::modules::console_shared_vm::noop,
        method "consoleInitializeWrite" (3) => crate::modules::console_shared_vm::initialize_write,
        method "consoleWrite" (3) => crate::modules::console_shared_vm::write,
        method "consoleWriteError" (1) => crate::modules::console_shared_vm::write_error,
        method "traceInitialize" (2) => crate::modules::trace_shared_vm::initialize,
        method "traceInspect" (3) => crate::modules::trace_shared_vm::inspect,
        method "traceInternal" (1) => crate::modules::trace_shared_vm::internal_binding,
        method "traceRecord" (5) => crate::modules::trace_shared_vm::record,
        method "traceCategoryBuffer" (1) => crate::modules::trace_shared_vm::category_buffer,
        method "traceCategoryEnabled" (1) => crate::modules::trace_shared_vm::category_enabled,
        method "traceCreate" (1) => crate::modules::trace_shared_vm::create,
        method "traceEnable" (0) => crate::modules::trace_shared_vm::enable,
        method "traceDisable" (0) => crate::modules::trace_shared_vm::disable,
        method "traceCategories" (0) => crate::modules::trace_shared_vm::categories,
        method "traceEnabled" (0) => crate::modules::trace_shared_vm::enabled,
        method "traceGetEnabledCategories" (0) => crate::modules::trace_shared_vm::get_enabled_categories,
        method "consoleTime" (2) => crate::modules::console_timing_shared_vm::time,
        method "consoleTimeEnd" (2) => crate::modules::console_timing_shared_vm::time_end,
        method "consoleTimeLog" (2) => crate::modules::console_timing_shared_vm::time_log,
        method "consoleCount" (2) => crate::modules::console_timing_shared_vm::count,
        method "consoleCountReset" (2) => crate::modules::console_timing_shared_vm::count_reset,
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
    const OBSERVE_PROMISE_LIFECYCLE: bool = true;

    fn proper_tail_calls(&self) -> bool {
        false
    }

    fn initialize_shadow_realm(
        context: &mut quench_runtime::NativeContext<'_, Self>,
        global: quench_runtime::RootId,
    ) -> Result<Option<quench_runtime::RootId>, quench_runtime::RootedError> {
        crate::modules::node_errors_shared_vm::initialize_shadow_policy(context, global).map(Some)
    }

    fn error_stack_formatter(&self) -> Option<quench_runtime::RootId> {
        self.shared_state().borrow().builtin_cache.get(crate::modules::node_errors_shared_vm::DEFAULT_STACK_FORMATTER).copied()
    }

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

    fn compile_wasm(
        &mut self,
        bytes: &[u8],
        mode: quench_runtime::ExecutionMode,
    ) -> Result<quench_runtime::WasmModule, String> {
        quench_wasm::Engine::new()
            .compile(bytes)
            .and_then(|module| module.lower_shared_module_for(mode))
            .map_err(|error| error.to_string())
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
            let exports = builtin_exports
                .get(builtin_name)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
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

        // Node resolves filesystem specifiers as URLs before accessing a file.
        // The URL suffix belongs to module identity, never to the disk path.
        let referrer_url = if referrer.starts_with("file:") {
            Some(url::Url::parse(referrer).map_err(|error| error.to_string())?)
        } else { None };
        let referrer_path = match &referrer_url {
            Some(url) => url.to_file_path().map_err(|_| "invalid file URL referrer".to_owned())?,
            None => std::path::PathBuf::from(referrer),
        };
        let internal = specifier.starts_with("internal/");
        let base = if internal {
            self.node_library_root.join("lib")
        } else {
            referrer_path.parent().unwrap_or(Path::new(".")).to_path_buf()
        };
        let file_url = if specifier.get(..5).is_some_and(|prefix| prefix.eq_ignore_ascii_case("file:")) {
            Some(url::Url::parse(specifier).map_err(|error| error.to_string())?)
        } else if !internal && (specifier.starts_with("./") || specifier.starts_with("../") || Path::new(specifier).is_absolute()) {
            let base_url = match referrer_url {
                Some(url) => url,
                None => {
                    let absolute = if referrer_path.is_absolute() { referrer_path.clone() }
                        else { std::env::current_dir().map_err(|error| error.to_string())?.join(&referrer_path) };
                    url::Url::from_file_path(absolute).map_err(|_| "invalid file module referrer".to_owned())?
                }
            };
            Some(base_url.join(specifier).map_err(|error| error.to_string())?)
        } else { None };
        let path = if let Some(url) = &file_url {
            let encoded = url.path().to_ascii_lowercase();
            if encoded.contains("%2f") || (cfg!(windows) && encoded.contains("%5c")) {
                return Err("File URL path must not include encoded path separators".into());
            }
            Some(url.to_file_path().map_err(|_| "invalid file module URL".to_owned())?)
        } else if !internal {
            commonjs::exact_file_path(&base, specifier)
        } else { None };
        let path = match path {
            Some(path) => path,
            None => {
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
                    Err(
                        oxc_resolver::ResolveError::NotFound(_)
                        | oxc_resolver::ResolveError::Specifier(_),
                    ) => return Ok(None),
                    Err(error) => return Err(error.to_string()),
                };
                resolution.full_path()
            }
        };
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        let bytes =
            std::fs::read(&path).map_err(|error| format!("module {}: {error}", path.display()))?;
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if extension == "node" {
            return Ok(None);
        }
        let name = if let Some(original) = file_url.as_ref().filter(|url| url.query().is_some_and(|value| !value.is_empty()) || url.fragment().is_some_and(|value| !value.is_empty())) {
            let mut canonical = url::Url::from_file_path(&path).map_err(|_| "invalid canonical module path".to_owned())?;
            canonical.set_query(original.query().filter(|value| !value.is_empty()));
            canonical.set_fragment(original.fragment().filter(|value| !value.is_empty()));
            canonical.to_string()
        } else { path.to_string_lossy().into_owned() };
        let source = if extension == "json" {
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            let kind = Self::source_kind(&path)?;
            if kind == quench_runtime::SourceKind::Module {
                String::from_utf8(bytes.clone())
                    .map_err(|error| format!("module {} is not UTF-8: {error}", path.display()))?
            } else {
                let filename = serde_json::to_string(&path.to_string_lossy()).map_err(|error| error.to_string())?;
                let mut source = format!(
                    "import {{ createRequire as __quenchCreateRequire }} from 'node:module';\nconst __quenchRequire = __quenchCreateRequire({filename});\nconst __quenchModule = __quenchRequire({filename});\nexport default __quenchModule;\n"
                );
                source.push_str("export { __quenchModule as 'module.exports' };\n");
                for (index, export) in
                    commonjs_exports::names(&path, &String::from_utf8_lossy(&bytes))
                        .into_iter()
                        .enumerate()
                {
                    let local = format!("__quenchExport{index}");
                    source.push_str("const ");
                    source.push_str(&local);
                    source.push_str(" = __quenchModule[");
                    source.push_str(
                        &serde_json::to_string(&export).map_err(|error| error.to_string())?,
                    );
                    source.push_str("];\n");
                    source.push_str("export { ");
                    source.push_str(&local);
                    source.push_str(" as ");
                    source.push_str(
                        &serde_json::to_string(&export).map_err(|error| error.to_string())?,
                    );
                    source.push_str(" };\n");
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
        if source_name.starts_with("node:") || source_name.starts_with("file:") || source_name.starts_with("<") {
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

    fn stack_source_name(&mut self, source_name: &str, is_module: bool) -> String {
        if is_module {
            self.import_meta_url(source_name)
        } else if self.eval_builtin_globals && source_name == self.inline_source.compilation_name() {
            self.inline_source.label().to_owned()
        } else if let Ok(relative) = std::path::Path::new(source_name).strip_prefix(self.node_library_root.join("lib")) {
            format!("node:{}", relative.with_extension("").to_string_lossy().replace('\\', "/"))
        } else {
            source_name.to_owned()
        }
    }

    fn create_promise_context(
        &mut self,
        parent: Option<quench_runtime::HostExecutionContext>,
    ) -> Option<quench_runtime::HostExecutionContext> {
        Some(crate::modules::async_hooks_shared_vm::create_promise_context(&self.shared_state, parent))
    }

    fn initialize_promise(
        context: &mut NativeContext<'_, Self>,
        promise: quench_runtime::RootId,
        resource: quench_runtime::HostExecutionContext,
    ) -> Result<(), RootedError> {
        crate::modules::async_hooks_shared_vm::initialize_promise(context, promise, resource)
    }

    fn promise_job_boundary(
        context: &mut NativeContext<'_, Self>,
        resource: quench_runtime::HostExecutionContext,
        before: bool,
    ) -> Result<(), RootedError> {
        crate::modules::async_hooks_shared_vm::promise_job_boundary(context, resource, before)
    }

    fn promise_settled(
        context: &mut NativeContext<'_, Self>,
        resource: quench_runtime::HostExecutionContext,
    ) -> Result<(), RootedError> {
        crate::modules::async_hooks_shared_vm::promise_settled(context, resource)
    }

    fn capture_job_context(&mut self, resource: Option<quench_runtime::HostExecutionContext>) -> Option<quench_runtime::HostExecutionContext> {
        crate::modules::async_hooks_shared_vm::capture_job_context(&self.shared_state, resource)
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
        crate::host::resident_pages::release_read_only_pages();
        // Each step stores what it builds in the heap or retains it, so its own handles are
        // released as it ends (`scoped`) instead of piling up for the whole bootstrap.
        context.scoped(crate::modules::text_decoder_shared_vm::install_global)?;
        #[cfg(feature = "profile-memory")]
        context.profile_memory_checkpoint("node_text_decoder");
        context
            .host_mut()
            .shared_state()
            .borrow_mut()
            .reset_cached_vm_roots();
        context.scoped(crate::modules::process_shared_vm::initialize)?;
        #[cfg(feature = "profile-memory")]
        context.profile_memory_checkpoint("node_process");
        crate::host::resident_pages::release_read_only_pages();
        context.scoped(|context| {
            crate::modules::events_shared_vm::initialize_targets(context)?;
            crate::modules::async_hook_registry_shared_vm::initialize(context).map(|_| ())
        })?;
        #[cfg(feature = "profile-memory")]
        context.profile_memory_checkpoint("node_event_target_polyfill");
        context.scoped(install_event_emitter)?;
        #[cfg(feature = "profile-memory")]
        context.profile_memory_checkpoint("node_event_emitter_polyfill");
        crate::host::resident_pages::release_read_only_pages();
        context.scoped(install_gc_global)?;
        context.scoped(crate::modules::timers_shared_vm::install_promisify_globals)?;
        commonjs::initialize(context)?;
        #[cfg(feature = "profile-memory")]
        context.profile_memory_checkpoint("node_commonjs");
        Ok(())
    }
}

/// The shared EventEmitter, and the process streams that take part in its protocol.
fn install_event_emitter(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let event_data = context
        .host_mut()
        .shared_state()
        .borrow()
        .event_targets_api
        .ok_or_else(|| RootedError::host("event listener configuration missing"))?;
    let initialize = context.host_function_with_data(operation("emitterInitialize"), event_data)?;
    let dispatch =
        context.host_function_with_data(operation("emitterEmitListeners"), event_data)?;
    let emitter_factory = context.evaluate_script_rooted(
        crate::polyfills::bootstrap::event_emitter::JS,
        "node:bootstrap/event-emitter.js",
    )?;
    let undefined = context.undefined();
    let event_emitter = context.call_rooted(emitter_factory, undefined, &[initialize, dispatch])?;
    context.release_root(event_emitter);
    crate::modules::event_constructor_shared_vm::install(context)?;
    crate::modules::event_emitter_shared_vm::initialize(context)?;
    crate::modules::event_limits_shared_vm::initialize(context)?;
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
    context.set_prototype_rooted(process, prototype)?;
    context.call_rooted(emitter, process, &[])?;
    let key = context.string_rooted("on");
    let on = context.get_property_rooted(process, key)?;
    for (event, operation_name, name) in [
        ("newListener", "processStartListeningSignal", "startListeningIfSignal"),
        ("removeListener", "processStopListeningSignal", "stopListeningIfSignal"),
    ] {
        let event = context.string_rooted(event);
        let function = context.host_constructor_named(operation(operation_name), name)?;
        context.call_rooted(on, process, &[event, function])?;
    }
    crate::modules::child_process_ipc_shared_vm::install_process(context, process, emitter)
}

/// `globalThis.gc` under `--expose-gc`.
fn install_gc_global(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
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
        let global = context.global_root()?;
        context.define_data_property_rooted(global, key, gc, true, true, false)?;
    }
    Ok(())
}
