//! The existing Node host adapted to the shared VM; Node policy stays in modules.

use super::NodeHost;
use quench_runtime::{HostFunction, HostFunctionId, NativeContext, RootedError, SystemHost};

pub(crate) mod commonjs;
mod commonjs_exports;
mod esm_errors;
pub(crate) mod esm_wasm;
pub(crate) mod policy_projection;
pub(crate) use commonjs::source_kind;

pub(crate) use commonjs::with_import_resolver;

pub(crate) fn bindings() -> &'static [HostFunction<NodeHost>] {
    quench_runtime::host_functions![
        method "workerConstruct" (1) => crate::modules::worker_threads_shared_vm::construct,
        method "workerRawUmask" (0) => crate::modules::worker_threads_shared_vm::raw_umask,
        method "workerSerializeFailure" (0) => crate::modules::worker_threads_shared_vm::serialize_failure,
        method "workerEvaluate" (0) => crate::modules::worker_threads_shared_vm::evaluate,
        method "workerThreadId" (0) => crate::modules::worker_threads_shared_vm::thread_id,
        method "workerPost" (0) => crate::modules::worker_threads_shared_vm::post,
        method "workerRef" (0) => crate::modules::worker_threads_shared_vm::ref_worker,
        method "workerUnref" (0) => crate::modules::worker_threads_shared_vm::unref_worker,
        method "workerTerminate" (1) => crate::modules::worker_threads_shared_vm::terminate,
        method "workerTerminateExecutor" (0) => crate::modules::worker_threads_shared_vm::terminate_executor,
        method "workerPortPost" (0) => crate::modules::worker_threads_shared_vm::port_post,
        method "workerPortOn" (2) => crate::modules::worker_threads_shared_vm::port_on,
        method "workerPortRef" (0) => crate::modules::worker_threads_shared_vm::port_ref,
        method "workerPortUnref" (0) => crate::modules::worker_threads_shared_vm::port_unref,
        method "workerPortClose" (0) => crate::modules::worker_threads_shared_vm::port_close,
        method "workerPortHasRef" (0) => crate::modules::worker_threads_shared_vm::port_has_ref,
        method "workerPoll" (0) => crate::modules::worker_threads_shared_vm::poll_native,

        method "nodeLibraryRequire" (1) => commonjs::require_node_library,
        method "primordialsCopy" (7) => crate::modules::primordials_shared_vm::copy,
        method "nodeTestMock" (0) => crate::modules::test_shared_vm::mock,
        method "nodeTestNamespace" (0) => crate::modules::test_shared_vm::namespace,
        method "contextifyScript" (0) => crate::modules::contextify_shared_vm::script,
        method "contextifyRunInContext" (0) => crate::modules::contextify_shared_vm::run_in_context,
        method "contextifyCompileFunction" (0) => crate::modules::contextify_shared_vm::compile_function,
        method "nodePrintEvalScript" (1) => crate::shared_run::print_eval_script,
        method "builtinExportValues" (2) => commonjs::builtin_export_values,
        method "esmWasmInstantiate" (6) => esm_wasm::instantiate,
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
        method "utilIsInsideNodeModules" (0) => crate::modules::util_shared_vm::is_inside_node_modules,
        method "utilTypeCheck" (0) => crate::modules::util_shared_vm::type_check,
        method "utilProxyDetails" (0) => crate::modules::util_shared_vm::proxy_details,
        method "utilGetCallerLocation" (0) => crate::modules::util_shared_vm::get_caller_location,
        method "utilGetCallSites" (0) => crate::modules::util_shared_vm::get_call_sites,
        method "utilGuessHandleType" (1) => crate::modules::util_shared_vm::guess_handle_type,
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
        method "processMetricsMemoryUsage" (1) => crate::modules::process_metrics_shared_vm::memory_usage,
        #[cfg(unix)]
        method "processExecve" (3) => crate::modules::process_execve_shared_vm::execve,
        method "processExitCodeGet" (0) => crate::modules::process_shared_vm::exit_code_get,
        method "processExitCodeSet" (1) => crate::modules::process_shared_vm::exit_code_set,
        method "processDefaultFatalException" (2) => crate::modules::process_fatal_exception_route::default_handler,
        method "processDispatchFatalException" (2) => crate::modules::process_fatal_exception_route::dispatch,
        method "processExit" (1) => crate::modules::process_shared_vm::exit,
        method "processEmitExitEvent" (1) => crate::modules::process_shared_vm::emit_exit_event,
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
        method "processUmask" (1) => crate::modules::process_shared_vm::umask,
        method "processKillNative" (2) => crate::modules::process_shared_vm::kill_native,
        method "processKill" (2) => crate::modules::process_shared_vm::kill,
        method "bufferConstructor" (0) => crate::modules::buffer_shared_vm::constructor,
        method "bufferPrototypeInitialize" (0) => crate::modules::buffer_shared_vm::initialize_prototype,
        method "encodingToAscii" (0) => crate::modules::encoding_binding_shared_vm::to_ascii,
        method "encodingToUnicode" (0) => crate::modules::encoding_binding_shared_vm::to_unicode,
        method "urlBindingDomainToAscii" (0) => crate::modules::url_binding_shared_vm::domain_to_ascii,
        method "urlBindingDomainToUnicode" (0) => crate::modules::url_binding_shared_vm::domain_to_unicode,
        method "urlBindingFormat" (0) => crate::modules::url_binding_shared_vm::format,
        method "urlBindingGetOrigin" (0) => crate::modules::url_binding_shared_vm::get_origin,
        method "urlBindingParse" (0) => crate::modules::url_binding_shared_vm::parse,
        method "urlBindingPathToFileUrl" (0) => crate::modules::url_binding_shared_vm::path_to_file_url,
        method "urlBindingUpdate" (0) => crate::modules::url_binding_shared_vm::update,
        method "urlBindingCanParse" (0) => crate::modules::url_binding_shared_vm::can_parse,
        method "blobBindingCreate" (0) => crate::modules::blob_binding_shared_vm::create,
        method "blobBindingStoreDataObject" (0) => crate::modules::blob_binding_shared_vm::store_data_object,
        method "blobBindingGetDataObject" (0) => crate::modules::blob_binding_shared_vm::get_data_object,
        method "blobBindingRevokeObjectUrl" (0) => crate::modules::blob_binding_shared_vm::revoke_object_url,
        method "blobBindingConcat" (0) => crate::modules::blob_binding_shared_vm::concat,
        method "blobBindingCreateFromFilePath" (0) => crate::modules::blob_binding_shared_vm::create_from_file_path,
        method "blobHandleGetReader" (0) => crate::modules::blob_binding_shared_vm::get_reader,
        method "blobHandleSlice" (0) => crate::modules::blob_binding_shared_vm::slice,
        method "blobReaderPull" (0) => crate::modules::blob_binding_shared_vm::pull,
        method "blobReaderSetWakeup" (0) => crate::modules::blob_binding_shared_vm::set_wakeup,
        method "bufferBindingAtob" (0) => crate::modules::buffer_binding_shared_vm::atob,
        method "bufferBindingBtoa" (0) => crate::modules::buffer_binding_shared_vm::btoa,
        method "bufferBindingByteLengthUtf8" (0) => crate::modules::buffer_binding_shared_vm::byte_length_utf8,
        method "bufferBindingCopy" (0) => crate::modules::buffer_binding_shared_vm::copy,
        method "bufferBindingCompare" (0) => crate::modules::buffer_binding_shared_vm::compare,
        method "bufferBindingCompareOffset" (0) => crate::modules::buffer_binding_shared_vm::compare_offset,
        method "bufferBindingFill" (0) => crate::modules::buffer_binding_shared_vm::fill,
        method "bufferBindingIndexOfBuffer" (0) => crate::modules::buffer_binding_shared_vm::index_of_buffer,
        method "bufferBindingIndexOfNumber" (0) => crate::modules::buffer_binding_shared_vm::index_of_number,
        method "bufferBindingIndexOfString" (0) => crate::modules::buffer_binding_shared_vm::index_of_string,
        method "bufferBindingCopyArrayBuffer" (0) => crate::modules::buffer_binding_shared_vm::copy_array_buffer,
        method "bufferBindingCreateUnsafeArrayBuffer" (0) => crate::modules::buffer_binding_shared_vm::create_unsafe_array_buffer,
        method "bufferBindingArrayBufferAlignedOffset" (0) => crate::modules::buffer_binding_shared_vm::array_buffer_aligned_offset,
        method "bufferBindingSwap16" (0) => crate::modules::buffer_binding_shared_vm::swap16,
        method "bufferBindingSwap32" (0) => crate::modules::buffer_binding_shared_vm::swap32,
        method "bufferBindingSwap64" (0) => crate::modules::buffer_binding_shared_vm::swap64,
        method "bufferBindingIsUtf8" (0) => crate::modules::buffer_binding_shared_vm::is_utf8,
        method "bufferBindingIsAscii" (0) => crate::modules::buffer_binding_shared_vm::is_ascii,
        method "bufferBindingIsLatin1" (0) => crate::modules::buffer_binding_shared_vm::is_latin1,
        method "bufferBindingMask" (0) => crate::modules::buffer_binding_shared_vm::mask,
        method "bufferBindingStringLengthUtf8" (0) => crate::modules::buffer_binding_shared_vm::string_length_utf8,
        method "bufferBindingAsciiSlice" (0) => crate::modules::buffer_binding_shared_vm::ascii_slice,
        method "bufferBindingBase64Slice" (0) => crate::modules::buffer_binding_shared_vm::base64_slice,
        method "bufferBindingBase64urlSlice" (0) => crate::modules::buffer_binding_shared_vm::base64url_slice,
        method "bufferBindingLatin1Slice" (0) => crate::modules::buffer_binding_shared_vm::latin1_slice,
        method "bufferBindingHexSlice" (0) => crate::modules::buffer_binding_shared_vm::hex_slice,
        method "bufferBindingUcs2Slice" (0) => crate::modules::buffer_binding_shared_vm::ucs2_slice,
        method "bufferBindingUtf8Slice" (0) => crate::modules::buffer_binding_shared_vm::utf8_slice,
        method "bufferBindingBase64Write" (0) => crate::modules::buffer_binding_shared_vm::base64_write,
        method "bufferBindingBase64urlWrite" (0) => crate::modules::buffer_binding_shared_vm::base64url_write,
        method "bufferBindingHexWrite" (0) => crate::modules::buffer_binding_shared_vm::hex_write,
        method "bufferBindingUcs2Write" (0) => crate::modules::buffer_binding_shared_vm::ucs2_write,
        method "bufferBindingAsciiWrite" (0) => crate::modules::buffer_binding_shared_vm::ascii_write,
        method "bufferBindingLatin1Write" (0) => crate::modules::buffer_binding_shared_vm::latin1_write,
        method "bufferBindingUtf8Write" (0) => crate::modules::buffer_binding_shared_vm::utf8_write,
        method "bufferBindingSetDetachKey" (0) => crate::modules::buffer_binding_shared_vm::set_detach_key,
        method "webStreamsInitialize" (0) => commonjs::initialize_web_streams,
        method "markAsUncloneable" (1) => crate::modules::clone_shared_vm::mark_as_uncloneable,
        method "markAsUntransferable" (1) => crate::modules::clone_shared_vm::mark_as_untransferable,
        method "isMarkedAsUntransferable" (1) => crate::modules::clone_shared_vm::is_marked_as_untransferable,
        method "webApisInitialize" (0) => commonjs::initialize_web_apis,
        method "webFetchInitialize" (0) => commonjs::initialize_fetch_apis,
        method "processSurfaceInitialize" (0) => commonjs::initialize_process_surface,
        method "processReleaseInitialize" (0) => commonjs::initialize_process_release,
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
        method "resolvePaths" (1) => commonjs::resolve_paths,
        method "ipcSerdesSerialize" (2) => crate::modules::ipc_serdes_shared_vm::serialize_native,
        method "ipcSerdesDeserialize" (1) => crate::modules::ipc_serdes_shared_vm::deserialize_native,
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
        method "consoleGlobalInitialize" (0) => commonjs::initialize_console_global,
        method "consoleLog" (0) => crate::modules::console_shared_vm::log,
        method "consoleLoad" (0) => crate::modules::console_shared_vm::load,
        method "processWarningListenerInitialize" (0) => crate::modules::process_shared_vm::initialize_warning_listener,
        method "processWarningLibrary" (1) => crate::modules::process_warning_shared_vm::library,
        method "processWarningOption" (1) => crate::modules::process_warning_shared_vm::option,
        method "processWarningBasename" (2) => crate::modules::process_warning_shared_vm::path_basename,
        method "consoleWarningWriter" (0) => crate::modules::console_shared_vm::warning_writer,
        method "consoleWarningWrite" (1) => crate::modules::console_shared_vm::warning_write,
        method "processMethodsInitialize" (0) => crate::modules::process_metrics_shared_vm::initialize_methods,
        method "processEmitWarningInitialize" (0) => crate::modules::process_shared_vm::initialize_emit_warning,
        method "processEnvInitialize" (0) => crate::modules::process_shared_vm::initialize_env,
        method "nodeErrorBrand" (0) => crate::modules::node_errors_shared_vm::brand,
        method "asyncHookInit" (7) => crate::modules::async_hook_registry_shared_vm::initialize_hook,
        method "asyncHookEnable" (1) => crate::modules::async_hook_registry_shared_vm::enable,
        method "asyncHookDisable" (1) => crate::modules::async_hook_registry_shared_vm::disable,
        method "asyncHooksEnabled" (0) => crate::modules::async_hook_registry_shared_vm::enabled,
        method "asyncExecutionId" (0) => crate::modules::async_hooks_shared_vm::execution_id,
        method "asyncTriggerId" (0) => crate::modules::async_hooks_shared_vm::trigger_id,
        method "asyncExecutionResource" (0) => crate::modules::async_hooks_shared_vm::execution_resource,
        method "asyncDefaultTriggerScope" (2) => crate::modules::async_hooks_shared_vm::default_trigger_scope,
        method "asyncGetDefaultTrigger" (0) => crate::modules::async_hooks_shared_vm::get_default_trigger,
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
        method "childProcessForkChildProjection" (0) => crate::modules::child_process_shared_vm::fork_child_projection,
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
        method "childProcessConstruct" (3) => crate::modules::child_process_shared_vm::construct_child,
        method "childProcessPrototypeSpawn" (1) => crate::modules::child_process_shared_vm::prototype_spawn,
        method "childProcessSpawnFailed" (3) => crate::modules::child_process_shared_vm::spawn_failed,
        method "childProcessSpawnFailedRun" (0) => crate::modules::child_process_shared_vm::spawn_failed_run,
        method "childProcessSpawnFailedFlush" (0) => crate::modules::child_process_shared_vm::spawn_failed_flush,
        method "childProcessSpawnFailedClosed" (0) => crate::modules::child_process_shared_vm::spawn_failed_closed,
        method "childProcessSpawnSucceeded" (1) => crate::modules::child_process_shared_vm::spawn_succeeded,
        method "childProcessSpawnSucceededRun" (0) => crate::modules::child_process_shared_vm::spawn_succeeded_run,
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
        method "domainTrampolineCall" (0) => crate::modules::domain_trampoline::call,
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
        method "netBoundSocketProjection" (0) => crate::modules::net_shared_vm::bound_socket_projection,
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
        method "nativeStreamConnectWrap" (0) => crate::modules::stream_binding::connect_wrap,
        method "nativeStreamShutdownWrap" (0) => crate::modules::stream_binding::shutdown_wrap,
        method "nativeStreamConnect" (0) => crate::modules::stream_binding::connect4,
        method "nativeStreamConnect6" (0) => crate::modules::stream_binding::connect6,
        method "nativeStreamShutdown" (0) => crate::modules::stream_binding::shutdown,
        method "nativeOptionalCallbackSlot" (1) => crate::modules::shared_event_loop::optional_callback,
        method "udpBroadcast" (1) => crate::modules::udp_binding::broadcast,
        method "udpTtl" (1) => crate::modules::udp_binding::ttl,
        method "udpMulticastTtl" (1) => crate::modules::udp_binding::multicast_ttl,
        method "udpMulticastLoop" (1) => crate::modules::udp_binding::multicast_loop,
        method "udpBufferSize" (3) => crate::modules::udp_binding::buffer_size,
        method "udpDisconnect" (0) => crate::modules::udp_binding::disconnect,
        method "udpOpen" (1) => crate::modules::udp_binding::open,
        method "udpMulticastInterface" (1) => crate::modules::udp_binding::multicast_interface,
        method "udpJoinMembership" (2) => crate::modules::udp_binding::join_membership,
        method "udpLeaveMembership" (2) => crate::modules::udp_binding::leave_membership,
        method "udpJoinSourceMembership" (3) => crate::modules::udp_binding::join_source_membership,
        method "udpLeaveSourceMembership" (3) => crate::modules::udp_binding::leave_source_membership,
        method "udpConstruct" (0) => crate::modules::udp_binding::construct,
        method "udpSendWrap" (0) => crate::modules::udp_binding::send_wrap,
        method "udpBind" (0) => crate::modules::udp_binding::bind4,
        method "udpBind6" (0) => crate::modules::udp_binding::bind6,
        method "udpConnect" (0) => crate::modules::udp_binding::connect4,
        method "udpConnect6" (0) => crate::modules::udp_binding::connect6,
        method "udpRecvStart" (0) => crate::modules::udp_binding::recv_start,
        method "udpRecvStop" (0) => crate::modules::udp_binding::recv_stop,
        method "udpSend" (0) => crate::modules::udp_binding::send4,
        method "udpSend6" (0) => crate::modules::udp_binding::send6,
        method "udpSockName" (0) => crate::modules::udp_binding::sock_name,
        method "udpPeerName" (0) => crate::modules::udp_binding::peer_name,
        method "udpQueueSize" (0) => crate::modules::udp_binding::queue_size,
        method "udpQueueCount" (0) => crate::modules::udp_binding::queue_count,
        method "udpFd" (0) => crate::modules::udp_binding::fd,
        method "udpDeliverMessage" (0) => crate::modules::udp_binding::deliver_message,
        method "udpDeliverSend" (0) => crate::modules::udp_binding::deliver_send,
        method "nativeStreamWriteWrap" (0) => crate::modules::stream_binding::write_wrap,
        method "nativeStreamPrepareCompletion" (2) => crate::modules::stream_binding::prepare_completion,
        method "nativeStreamWriteBuffer" (0) => crate::modules::stream_binding::write_buffer,
        method "nativeStreamReadStart" (0) => crate::modules::stream_binding::read_start,
        method "nativeStreamReadStop" (0) => crate::modules::stream_binding::read_stop,
        method "nativeStreamCopyRead" (1) => crate::modules::stream_binding::copy_read,
        method "nativeStreamDeliverRead" (3) => crate::modules::stream_binding::deliver_read,
        method "nativeTcpFd" (0) => crate::modules::tcp_binding::fd,
        method "nativeAsyncWrapConstruct" (0) => crate::modules::native_async_wrap::abstract_constructor,
        method "nativeAsyncWrapId" (0) => crate::modules::native_async_wrap::get_id,
        method "nativeAsyncWrapProvider" (0) => crate::modules::native_async_wrap::get_provider,
        method "nativePipeConstruct" (1) => crate::modules::pipe_binding::construct,
        method "nativePipeOpen" (1) => crate::modules::pipe_binding::open,
        method "nativePipeReadStart" (0) => crate::modules::pipe_binding::read_start,
        method "nativePipeReadStop" (0) => crate::modules::pipe_binding::read_stop,
        method "nativePipeWriteUtf8" (3) => crate::modules::pipe_binding::write_utf8,
        method "nativePipeWriteBuffer" (3) => crate::modules::pipe_binding::write_buffer,
        method "nativePipeClose" (0) => crate::modules::pipe_binding::close,
        method "nativePipeRef" (0) => crate::modules::pipe_binding::ref_handle,
        method "nativePipeUnref" (0) => crate::modules::pipe_binding::unref_handle,
        method "nativePipeHasRef" (0) => crate::modules::pipe_binding::has_ref,
        method "nativePipeFd" (0) => crate::modules::pipe_binding::fd,
        method "nativePipeQueue" (0) => crate::modules::pipe_binding::queue,
        method "nativePipeWriteComplete" (2) => crate::modules::pipe_binding::write_complete,
        method "nativePipePrepareRead" (1) => crate::modules::pipe_binding::prepare_read,
        method "nativePipeReadComplete" (1) => crate::modules::pipe_binding::read_complete,
        method "nativeTcpConstruct" (1) => crate::modules::tcp_binding::construct,
        method "nativeTcpBind" (0) => crate::modules::tcp_binding::bind4,
        method "nativeTcpBind6" (0) => crate::modules::tcp_binding::bind6,
        method "nativeTcpListen" (0) => crate::modules::tcp_binding::listen,
        method "nativeTcpName" (0) => crate::modules::tcp_binding::name,
        method "nativeTcpQueue" (0) => crate::modules::tcp_binding::queue,
        method "nativeTcpClose" (0) => crate::modules::tcp_binding::close,
        method "nativeTcpRef" (0) => crate::modules::tcp_binding::reference_on,
        method "nativeTcpUnref" (0) => crate::modules::tcp_binding::reference_off,
        method "nativeTcpHasRef" (0) => crate::modules::tcp_binding::has_ref,
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
        method "emitterValidateCaptureRejections" (1) => crate::modules::event_constructor_shared_vm::validate_capture_rejections,
        method "emitterRemoveAllListeners" (3) => crate::modules::event_emitter_shared_vm::remove_all_listeners,
        method "emitterRemoveListener" (3) => crate::modules::event_emitter_shared_vm::remove_listener,
        method "emitterEmit" (3) => crate::modules::event_emitter_shared_vm::emit,
        method "emitterErrorInspector" (0) => crate::modules::events_shared_vm::error_inspector,
        method "emitterEmitListeners" (4) => crate::modules::event_emitter_shared_vm::emit_listeners,
        method "emitterCaptureResult" (4) => crate::modules::event_emitter_shared_vm::capture_listener_result,
        method "emitterCaptureDefaultGet" (0) => crate::modules::event_constructor_shared_vm::default_get,
        method "emitterCaptureDefaultSet" (1) => crate::modules::event_constructor_shared_vm::default_set,
        method "emitterCaptureRejected" (1) => crate::modules::event_constructor_shared_vm::capture_rejected,
        method "emitterCaptureDeliver" (1) => crate::modules::event_constructor_shared_vm::capture_deliver,
        method "emitterAddListener" (4) => crate::modules::event_emitter_shared_vm::add_listener,
        method "emitterOnce" (4) => crate::modules::event_emitter_shared_vm::once,
        method "emitterOnceInvoke" (0) => crate::modules::event_emitter_shared_vm::once_invoke,
        method "eventsDefaultMaxGet" (0) => crate::modules::event_limits_shared_vm::default_get,
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
        method "abortTimeout" (2) => crate::modules::abort_timeout_shared_vm::timeout,
        method "abortAnyConvert" (1) => crate::modules::abort_dependencies_shared_vm::convert,
        method "abortDependenciesLink" (2) => crate::modules::abort_dependencies_shared_vm::link,
        method "abortDependenciesRefresh" (2) => crate::modules::abort_dependencies_shared_vm::refresh,
        method "abortDependenciesSettle" (2) => crate::modules::abort_dependencies_shared_vm::settle,
        method "abortSourceCollected" (1) => crate::modules::abort_dependencies_shared_vm::source_collected,
        method "abortDependentCollected" (1) => crate::modules::abort_dependencies_shared_vm::dependent_collected,
        method "abortSignalBrand" (1) => crate::modules::events_shared_vm::abort_signal_brand,
        method "abortAddEventListener" (6) => crate::modules::events_abort_shared_vm::abort_add_event_listener,
        method "abortDispatchListeners" (4) => crate::modules::events_abort_shared_vm::abort_dispatch_listeners,
        method "eventTargetQueueError" (1) => crate::modules::events_abort_shared_vm::queue_listener_error,
        method "eventTargetRethrow" (0) => crate::modules::events_abort_shared_vm::rethrow_listener_error,
        method "BrotliEncoder" (0) => crate::modules::zlib_native_shared_vm::create_brotli_encoder,
        method "BrotliDecoder" (0) => crate::modules::zlib_native_shared_vm::create_brotli_decoder,
        method "ZstdCompress" (0) => crate::modules::zlib_native_shared_vm::create_zstd_compress,
        method "ZstdDecompress" (0) => crate::modules::zlib_native_shared_vm::create_zstd_decompress,
        method "Zlib" (0) => crate::modules::zlib_native_shared_vm::create,
        method "zlibNativeParams" (0) => crate::modules::zlib_native_shared_vm::params,
        method "zlibNativeReset" (0) => crate::modules::zlib_native_shared_vm::reset,
        method "zlibNativeCrc32" (2) => crate::modules::zlib_native_shared_vm::crc32,
        method "zlibNativeInit" (0) => crate::modules::zlib_native_shared_vm::initialize,
        method "zlibNativeWrite" (0) => crate::modules::zlib_native_shared_vm::write,
        method "zlibNativeWriteSync" (0) => crate::modules::zlib_native_shared_vm::write_sync,
        method "zlibNativeClose" (0) => crate::modules::zlib_native_shared_vm::close,
        method "zlibNativePoll" (0) => crate::modules::zlib_native_shared_vm::poll,
        method "processDlopen" (0) => crate::modules::napi::dlopen,
        method "napiCallback" (0) => crate::modules::napi::invoke,
        method "napiDrain" (0) => crate::modules::napi::drain,
        method "napiPromiseExecutor" (2) => crate::modules::napi::promise_executor,
        method "messagingGlobalsInitialize" (0) => crate::modules::worker_shared_vm::initialize_globals,
        method "messagingChannel" (0) => crate::modules::worker_shared_vm::channel,
        method "messagingPost" (0) => crate::modules::worker_shared_vm::post,
        method "messagingStart" (0) => crate::modules::worker_shared_vm::start,
        method "messagingClose" (0) => crate::modules::worker_shared_vm::close,
        method "messagingRef" (0) => crate::modules::worker_shared_vm::reference,
        method "messagingHasRef" (0) => crate::modules::worker_shared_vm::has_ref,
        method "messagingReceive" (0) => crate::modules::worker_shared_vm::receive,
        method "messagingDrain" (0) => crate::modules::worker_shared_vm::drain,
        method "workerStart" (0) => crate::modules::worker_shared_vm::worker_start,
        method "messagingWorkerRef" (0) => crate::modules::worker_shared_vm::worker_ref,
        method "messagingWorkerTerminate" (0) => crate::modules::worker_shared_vm::worker_terminate,
        method "workerExited" (0) => crate::modules::worker_shared_vm::worker_exited,
        method "workerCaptureError" (0) => crate::modules::worker_shared_vm::capture_error,
        method "eventsAddAbortListener" (2) => crate::modules::events_abort_shared_vm::add_abort_listener,
        method "abortListenerDispose" (0) => crate::modules::events_abort_shared_vm::abort_listener_dispose,
        method "eventWeakListenerCollected" (1) => crate::modules::events_weak_shared_vm::collected,
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
        method "httpUpgradeSocket" (1) => crate::modules::http_shared_vm::upgrade_socket,
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
        method "httpResponseGetHeaders" (1) => crate::modules::http_shared_vm::response_get_headers,
        method "httpResponseRemoveHeader" (2) => crate::modules::http_shared_vm::response_remove_header,
        method "httpResponseWrite" (7) => crate::modules::http_shared_vm::response_write,
        method "httpResponseFinish" (7) => crate::modules::http_shared_vm::response_finish,
        method "httpResponseWriteHead" (6) => crate::modules::http_shared_vm::response_write_head,
        method "httpResponseInitialize" (2) => crate::modules::http_shared_vm::response_initialize,
        method "httpResponseSend" (4) => crate::modules::http_shared_vm::response_send,
        method "assertStrictEqual" (2) => crate::modules::assert_shared_vm::strict_equal,
        method "assertNotStrictEqual" (2) => crate::modules::assert_shared_vm::not_strict_equal,
        method "assertDeepEqual" (2) => crate::modules::assert_shared_vm::deep_equal,
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
        #[cfg(quench_openssl_provider)]
        method "cryptoHkdfJobNew" (6) => crate::modules::crypto_secret_job_shared_vm::create_hkdf,
        #[cfg(quench_openssl_provider)]
        method "cryptoPbkdf2JobNew" (6) => crate::modules::crypto_secret_job_shared_vm::create_pbkdf2,
        #[cfg(quench_openssl_provider)]
        method "cryptoHashJobNew" (3) => crate::modules::crypto_secret_job_shared_vm::create_hash,
        #[cfg(quench_openssl_provider)]
        method "cryptoNativeHashNew" (0) => crate::modules::crypto_hash_shared_vm::create_hash,
        #[cfg(quench_openssl_provider)]
        method "cryptoNativeHmacNew" (0) => crate::modules::crypto_hash_shared_vm::create_hmac,
        #[cfg(quench_openssl_provider)]
        method "cryptoNativeHmacInit" (0) => crate::modules::crypto_hash_shared_vm::init_hmac,
        #[cfg(quench_openssl_provider)]
        method "cryptoNativeHashUpdate" (0) => crate::modules::crypto_hash_shared_vm::update,
        #[cfg(quench_openssl_provider)]
        method "cryptoNativeHashDigest" (0) => crate::modules::crypto_hash_shared_vm::digest,
        #[cfg(quench_openssl_provider)]
        method "cryptoNativeOneShotDigest" (0) => crate::modules::crypto_hash_shared_vm::one_shot,
        method "cryptoSecretMaterial" (1) => crate::modules::crypto_key_shared_vm::secret_material,
        #[cfg(quench_openssl_provider)]
        method "cryptoNativeSignatureNew" (0) => crate::modules::crypto_signature_shared_vm::new,
        #[cfg(quench_openssl_provider)]
        method "cryptoNativeSignatureInit" (1) => crate::modules::crypto_signature_shared_vm::init,
        #[cfg(quench_openssl_provider)]
        method "cryptoNativeSignatureUpdate" (2) => crate::modules::crypto_signature_shared_vm::update,
        #[cfg(quench_openssl_provider)]
        method "cryptoNativeSignFinal" (8) => crate::modules::crypto_signature_shared_vm::sign,
        #[cfg(quench_openssl_provider)]
        method "cryptoNativeVerifyFinal" (9) => crate::modules::crypto_signature_shared_vm::verify,
        #[cfg(quench_openssl_provider)]
        method "cryptoRsaNative" (10) => crate::modules::crypto_rsa_shared_vm::operation,
        #[cfg(quench_openssl_provider)]
        method "cryptoRandomJobNew" (4) => crate::modules::crypto_secret_job_shared_vm::create_random,
        #[cfg(quench_openssl_provider)]
        method "cryptoSignJobNew" (14) => crate::modules::crypto_secret_job_shared_vm::create_sign,
        #[cfg(quench_openssl_provider)]
        method "cryptoRsaKeyPairJobNew" (0) => crate::modules::crypto_secret_job_shared_vm::create_rsa_pair,
        #[cfg(quench_openssl_provider)]
        method "cryptoEcKeyPairJobNew" (0) => crate::modules::crypto_secret_job_shared_vm::create_ec_pair,
        #[cfg(quench_openssl_provider)]
        method "cryptoDsaKeyPairJobNew" (0) => crate::modules::crypto_secret_job_shared_vm::create_dsa_pair,
        #[cfg(quench_openssl_provider)]
        method "cryptoDhKeyPairJobNew" (0) => crate::modules::crypto_secret_job_shared_vm::create_dh_pair,
        #[cfg(quench_openssl_provider)]
        method "cryptoNamedKeyPairJobNew" (0) => crate::modules::crypto_secret_job_shared_vm::create_named_pair,
        method "cryptoSecretJobNew" (5) => crate::modules::crypto_secret_job_shared_vm::create,
        method "cryptoSecretJobRun" (0) => crate::modules::crypto_secret_job_shared_vm::run,
        method "cryptoSecretJobExecutor" (2) => crate::modules::crypto_secret_job_shared_vm::executor,
        method "cryptoSecretJobPoll" (0) => crate::modules::crypto_secret_job_shared_vm::poll,
        method "cryptoSecretJobComplete" (0) => crate::modules::crypto_secret_job_shared_vm::complete,
        #[cfg(quench_openssl_provider)]
        method "cryptoProviderFacts" (1) => crate::modules::crypto_provider_shared_vm::operation,
        #[cfg(quench_openssl_provider)]
        method "cryptoWebNamespace" (0) => crate::modules::crypto_provider_shared_vm::webcrypto,
        #[cfg(quench_openssl_provider)]
        method "webDomExceptionInitialize" (0) => crate::modules::dom_exception_shared_vm::initialize,
        #[cfg(quench_openssl_provider)]
        method "cryptoWebClasses" (0) => crate::modules::crypto_provider_shared_vm::install_classes,
        method "cryptoKeySlots" (0) => crate::modules::crypto_key_shared_vm::operation,
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
        method "toPathIfFileURL" (1) => crate::modules::url_shared_vm::to_path_if_file_url,
        method "cliOptionsValues" (0) => crate::modules::cli_options_shared_vm::values,
        method "cliParseNodeOptions" (0) => crate::modules::cli_options_shared_vm::parse_environment,
        method "fsFileHandleCloseOwned" (0) => crate::modules::fs_shared_vm::file_handle_write::close,
        method "fsFileHandleCloseWait" (0) => crate::modules::fs_shared_vm::file_handle_write::close_wait,
        method "fsFileHandleWriteComplete" (0) => crate::modules::fs_shared_vm::file_handle_write::complete,
        method "fsFileHandleWriteSchedule" (0) => crate::modules::fs_shared_vm::file_handle_write::schedule,
        method "fsFileHandleWriteEffect" (0) => crate::modules::fs_shared_vm::file_handle_write::effect,
        method "fsPromiseWriteOwned" (0) => crate::modules::fs_shared_vm::file_handle_write::public,
        method "fsFileHandleWriteOwned" (0) => crate::modules::fs_shared_vm::file_handle_write::method,
        method "fsFileHandleRegister" (0) => crate::modules::fs_shared_vm::file_handle_write::register,
        method "fsFileHandleClosed" (0) => crate::modules::fs_shared_vm::file_handle_write::closed,
        method "fsPromiseWriteVoid" (0) => crate::modules::fs_shared_vm::file_handle_write::void,
        method "fsFileHandleReadParameters" (0) => crate::modules::fs_shared_vm::file_handle_read::parameters,
        method "fsBindingInternalModuleStat" (0) => crate::modules::fs_directory_shared_vm::internal_module_stat,
        method "fsBindingReaddir" (0) => crate::modules::fs_directory_shared_vm::readdir,
        method "fsBindingReaddirRecursive" (0) => crate::modules::fs_directory_shared_vm::readdir_recursive,
        method "fsPromisesGlobInitialize" (0) => crate::modules::fs_shared_vm::initialize_promises_glob_policy,
        method "fsReadStreamInitialize" (0) => crate::modules::fs_shared_vm::initialize_read_stream,
        method "fsModuleInitialize" (0) => crate::modules::fs_shared_vm::initialize_module,
        method "perfHooksObserversInitialize" (0) => crate::modules::perf_hooks::initialize_observers,
        method "fsLibraryPolicyInitialize" (0) => crate::modules::fs_shared_vm::initialize_library_policy,
        method "fsWriteStreamInitialize" (0) => crate::modules::fs_shared_vm::write_stream::initialize,
        method "fsVfsPair" (2) => crate::modules::fs_policy_shared_vm::vfs_pair,
        method "fsVfsExistsSync" (1) => crate::modules::fs_policy_shared_vm::vfs_exists_sync,
        method "fsWriteFilePublic" (3) => crate::modules::fs_shared_vm::write_public::write,
        method "fsAppendFilePublic" (3) => crate::modules::fs_shared_vm::write_public::append,
        method "fsVfsReadlinkPromise" (3) => crate::modules::fs_shared_vm::promise_public::readlink_continuation,
        method "fsPromisesReadlinkInitialize" (0) => crate::modules::fs_shared_vm::promise_public::initialize,
        method "fsVfsExistsComplete" (1) => crate::modules::fs_shared_vm::callback_public::exists_complete,
        method "fsVfsCallback" (0) => crate::modules::fs_shared_vm::callback_public::dispatch,
        method "fsVfsMetadata" (0) => crate::modules::fs_shared_vm::metadata_public::dispatch,
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
        method "httpIncomingBody" (1) => crate::modules::http_shared_vm::incoming::body,
        method "httpIncomingRead" (1) => crate::modules::http_shared_vm::incoming::read,
        method "httpIncomingDestroy" (2) => crate::modules::http_shared_vm::incoming::destroy,
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
        method "asyncResourceEnterScope" (1) => crate::modules::async_hooks_shared_vm::enter_async_scope,
        method "asyncResourceExitScope" (0) => crate::modules::async_hooks_shared_vm::exit_async_scope,
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
        method "performanceNativeNow" (0) => crate::modules::performance_binding_shared_vm::now,
        method "performanceSetupObservers" (0) => crate::modules::performance_binding_shared_vm::setup,
        method "performanceGcTracking" (0) => crate::modules::performance_binding_shared_vm::gc_tracking,
        method "performanceNotify" (0) => crate::modules::performance_binding_shared_vm::notify,
        method "performanceBootstrapMark" (0) => crate::modules::performance_binding_shared_vm::bootstrap_mark,
        method "perfHooksNow" (0) => crate::modules::perf_hooks::now,
        method "serdesSerializer" (0) => crate::modules::serdes_shared_vm::serializer,
        method "serdesDeserializer" (0) => crate::modules::serdes_shared_vm::deserializer,
        method "serdesRelease" (0) => crate::modules::serdes_shared_vm::release,
        method "serdesWriteHeader" (0) => crate::modules::serdes_shared_vm::write_header,
        method "serdesWriteValue" (0) => crate::modules::serdes_shared_vm::write_value,
        method "serdesReleaseBuffer" (0) => crate::modules::serdes_shared_vm::release_buffer,
        method "serdesTransferArrayBuffer" (0) => crate::modules::serdes_shared_vm::transfer_array_buffer,
        method "serdesWriteUint32" (0) => crate::modules::serdes_shared_vm::write_uint32,
        method "serdesWriteUint64" (0) => crate::modules::serdes_shared_vm::write_uint64,
        method "serdesWriteDouble" (0) => crate::modules::serdes_shared_vm::write_double,
        method "serdesWriteRawBytes" (0) => crate::modules::serdes_shared_vm::write_raw_bytes,
        method "serdesTreatViewsAsHostObjects" (0) => crate::modules::serdes_shared_vm::treat_views_as_host_objects,
        method "serdesReadHeader" (0) => crate::modules::serdes_shared_vm::read_header,
        method "serdesReadValue" (0) => crate::modules::serdes_shared_vm::read_value,
        method "serdesReceiveArrayBuffer" (0) => crate::modules::serdes_shared_vm::receive_array_buffer,
        method "serdesWireFormatVersion" (0) => crate::modules::serdes_shared_vm::wire_format_version,
        method "serdesReadUint32" (0) => crate::modules::serdes_shared_vm::read_uint32,
        method "serdesReadUint64" (0) => crate::modules::serdes_shared_vm::read_uint64,
        method "serdesReadDouble" (0) => crate::modules::serdes_shared_vm::read_double,
        method "serdesReadRawBytes" (0) => crate::modules::serdes_shared_vm::read_raw_bytes,
        method "v8HeapSpaces" (0) => crate::modules::v8_shared_vm::heap_spaces,
        method "v8HeapStatistics" (0) => crate::modules::v8_shared_vm::heap_statistics,
        method "v8HeapSpaceStatistics" (0) => crate::modules::v8_shared_vm::heap_space_statistics,
        method "v8CachedDataVersionTag" (0) => crate::modules::v8_shared_vm::cached_data_version_tag,
        method "v8IsStringOneByteRepresentation" (0) => crate::modules::v8_shared_vm::is_string_one_byte_representation,
        method "v8HashSeed" (0) => crate::modules::v8_shared_vm::hash_seed,
        #[cfg(not(quench_node_cares))]
        method "caresCanonicalizeIp" (0) => crate::modules::dns_shared_vm::canonicalize_ip,
        #[cfg(not(quench_node_cares))]
        method "caresIpv6Buffer" (0) => crate::modules::dns_shared_vm::ipv6_buffer,
        #[cfg(not(quench_node_cares))]
        method "caresStrerror" (0) => crate::modules::dns_shared_vm::strerror,
        #[cfg(not(quench_node_cares))]
        method "caresChannel" (0) => crate::modules::dns_shared_vm::channel,
        #[cfg(not(quench_node_cares))]
        method "caresChannelRelease" (0) => crate::modules::dns_shared_vm::channel_release,
        #[cfg(not(quench_node_cares))]
        method "caresQuery" (0) => crate::modules::dns_shared_vm::query,
        #[cfg(not(quench_node_cares))]
        method "caresGetServers" (0) => crate::modules::dns_shared_vm::get_servers,
        #[cfg(not(quench_node_cares))]
        method "caresSetServers" (0) => crate::modules::dns_shared_vm::set_servers,
        #[cfg(not(quench_node_cares))]
        method "caresSetLocalAddress" (0) => crate::modules::dns_shared_vm::set_local_address,
        #[cfg(not(quench_node_cares))]
        method "caresCancel" (0) => crate::modules::dns_shared_vm::cancel,
        #[cfg(not(quench_node_cares))]
        method "caresDrain" (0) => crate::modules::dns_shared_vm::drain,
        method "dnsAddressArray" (0) => crate::modules::cares_binding::address_array,
        #[cfg(quench_node_cares)]
        method "dnsChannelConstruct" (3) => crate::modules::cares_channel_shared_vm::construct,
        #[cfg(quench_node_cares)]
        method "dnsChannelQuery" (2) => crate::modules::cares_channel_shared_vm::query,
        #[cfg(quench_node_cares)]
        method "dnsChannelGetServers" (0) => crate::modules::cares_channel_shared_vm::get_servers,
        #[cfg(quench_node_cares)]
        method "dnsChannelSetServers" (1) => crate::modules::cares_channel_shared_vm::set_servers,
        #[cfg(quench_node_cares)]
        method "dnsChannelSetLocalAddress" (2) => crate::modules::cares_channel_shared_vm::set_local,
        #[cfg(quench_node_cares)]
        method "dnsChannelCancel" (0) => crate::modules::cares_channel_shared_vm::cancel,
        #[cfg(quench_node_cares)]
        method "dnsChannelStrerror" (1) => crate::modules::cares_channel_shared_vm::strerror,
        #[cfg(quench_node_cares)]
        method "dnsCanonicalizeIP" (1) => crate::modules::cares_channel_shared_vm::canonicalize,
        #[cfg(quench_node_cares)]
        method "dnsIpv6Buffer" (1) => crate::modules::cares_channel_shared_vm::ipv6_buffer,
        #[cfg(quench_node_cares)]
        method "dnsRrCallbackFactory" (1) => crate::modules::cares_channel_shared_vm::callback_factory,
        #[cfg(quench_node_cares)]
        method "dnsRrInvoke" (0) => crate::modules::cares_channel_shared_vm::invoke,
        #[cfg(quench_node_cares)]
        method "dnsRrArray" (0) => crate::modules::cares_channel_shared_vm::array,
        #[cfg(quench_node_cares)]
        method "dnsRrObject" (0) => crate::modules::cares_channel_shared_vm::object,
        #[cfg(quench_node_cares)]
        method "dnsRrBinary" (0) => crate::modules::cares_channel_shared_vm::binary,
        method "dnsRequestConstruct" (0) => crate::modules::cares_binding::construct,
        method "dnsGetNameInfo" (3) => crate::modules::cares_binding::getnameinfo,
        method "dnsGetAddrInfo" (5) => crate::modules::cares_binding::getaddrinfo,
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
        method "fileURLToPath" (1) => crate::modules::url_shared_vm::file_url_to_path,
        method "fileURLToPathBuffer" (2) => crate::modules::url_shared_vm::file_url_to_path_buffer,
        global "structuredClone" (1) => crate::modules::clone_shared_vm::structured_clone,
        method "ttyIsatty" (1) => crate::modules::tty_shared_vm::isatty,
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
    ]
}

/// The host function registered as `name`. Hosts resolve operations per request (an
/// incoming message's stream methods, an accepted socket's handle), so the names are
/// indexed once, by a binary search, rather than scanned among every binding. Each name
/// registers exactly one function (`operation_names_are_unique`).
pub(crate) fn operation(name: &str) -> HostFunctionId {
    /// The bindings' positions, ordered by name: the names are the bindings' own static
    /// strings, so the index holds two bytes a binding. A hash map keyed by the names, sized
    /// for every binding, was 50 KiB of every process's heap at start-up (massif, V8-v7
    /// Richards, 2026-10-22); a binary search costs about eleven comparisons of short names.
    static INDEX: std::sync::OnceLock<Box<[u16]>> = std::sync::OnceLock::new();
    let bindings = bindings();
    let index = INDEX.get_or_init(|| {
        let mut index: Box<[u16]> = (0..bindings.len())
            .map(|position| u16::try_from(position).expect("Node binding index fits u16"))
            .collect();
        // Stable, so a repeated name resolves to its first registration.
        index.sort_by_key(|&position| bindings[usize::from(position)].name);
        index
    });
    let first = index.partition_point(|&position| bindings[usize::from(position)].name < name);
    let position = index
        .get(first)
        .copied()
        .filter(|&position| bindings[usize::from(position)].name == name)
        .expect("registered Node operation");
    HostFunctionId(u32::from(position))
}

/// The host operations that run beside a lane's frameless activations
/// (`host_function_runs_beside_activations`).
const BESIDE_ACTIVATIONS: &[&str] = &[
    "emitterListenerCount",
    "emitterEmitListeners",
    "bufferBindingByteLengthUtf8",
    "nextTick",
    "httpResponseGetHeader",
    "httpResponseGetHeaders",
    "httpResponseSetHeader",
    "httpResponseWrite",
    "httpResponseWriteHead",
    "httpResponseFinish",
    "httpResponseInitialize",
    "httpIncomingDestroy",
    "asyncResourceInit",
    "asyncResourceEnterScope",
    "asyncResourceExitScope",
    "stringDecoderDecode",
    "markAsUncloneable",
];

impl quench_runtime::Host for NodeHost {
    fn observes_promise_lifecycle(&self) -> bool {
        self.shared_state.borrow().async_hooks.tracks_promises()
    }

    /// A job boundary prunes tracked Promise resources and runs before/after hooks; with
    /// no Promise tracked and no hook tracking them, it does neither
    /// (`async_hooks_shared_vm::promise_job_boundary`).
    fn observes_promise_job_boundaries(&self) -> bool {
        self.shared_state.borrow().async_hooks.observes_job_boundaries()
    }

    /// The code a run touched since the last collection leaves the resident set after each
    /// collection, and before a full one; what still runs comes back from the page cache. The
    /// collector's code and the program's are then resident one at a time, and the program's
    /// only as much of it as ran since the last collection (V8-v7 RegExp max RSS, static
    /// x86_64 unbolted, 2026-10-18: 7244-7500 -> 7136-7328 KiB at every collection's end;
    /// Splay 39800 -> 38988 KiB with the release before full collections).
    /// The allocator's free pages, which the collection's sweep just freed, leave with them
    /// (V8-v7 max RSS, static x86_64 unbolted, 2026-10-19: RegExp 6772-7044 -> 6664-6772 KiB,
    /// EarleyBoyer 9048-9088 -> 8536-8576, the rest unchanged). At most once per the policy's
    /// `MemoryPolicy::resident_release_interval`; every policy counts the collection for the
    /// release before output (`resident_pages::release_at_collection`).
    fn at_collection_boundary(&mut self, policy: quench_runtime::MemoryPolicy) {
        crate::host::resident_pages::release_at_collection(policy);
    }

    /// The pages the program touched leave the resident set before the code generator's join
    /// it, as compiling a module's bytecode does (`compile_wasm`): loading, linking and
    /// instantiating a WebAssembly module, or the interpreter and the builtins a JavaScript
    /// program ran. A JavaScript compile is the peak of a short numeric program (V8-v7
    /// NavierStokes `--jit` under MemoryFirst, plain x86_64 build, 2026-10-22: max RSS
    /// 10.84-10.94 -> 8.80-8.89 MB, Score within noise).
    fn before_native_compilation(&mut self) {
        crate::host::resident_pages::release_at_native_compilation();
    }

    /// The code generator's pages, and the allocator's pages its scratch memory freed, leave
    /// the resident set before the program runs on (V8-v7 Crypto `--jit` under MemoryFirst,
    /// plain x86_64 build, 2026-10-22: max RSS 6.82-6.84 -> 5.91 MB with the release before).
    fn after_native_compilation(&mut self) {
        crate::host::resident_pages::release_at_native_compilation();
        crate::host::resident_pages::release_free_heap_pages();
    }

    /// Snapshots and compile-cache entries are tracked read-only mappings
    /// (`startup_snapshot::load_or_schedule`, `CacheEntry::load`).
    fn kept_bytes_read(bytes: &[u8]) {
        crate::host::resident_pages::release_tracked_bytes(bytes);
    }

    fn kept_compiled_program(&mut self, name: &str, parts: &[&str]) -> Option<quench_runtime::KeptBytes> {
        self.compile_cache.entry(name, parts)?.load()
    }
    fn keep_compiled_program(&mut self, name: &str, parts: &[&str], bytes: &[u8]) {
        if let Some(entry) = self.compile_cache.entry(name, parts) {
            entry.store(bytes);
        }
        crate::host::resident_pages::after_program_kept();
    }
    /// A decoded program after bootstrap (the entry and its modules, a builtin or library
    /// module loaded on first use) releases what loading it touched, as its compilation would
    /// have at its phase boundary: with the compile cache warm, a run otherwise reaches no
    /// release between bootstrap and its first collection (CoreMark as a Wasm ES module,
    /// static x86_64 unbolted, 2026-10-21: max RSS 4404 -> 3412 KiB; through
    /// `WebAssembly.instantiate`, 4920 -> 4016). Bootstrap's own programs decode back to back,
    /// where a release after each only refaulted the same loader code (an empty script's run
    /// 11 -> 18 ms). Decodes after bootstrap release at most once per interval, as code
    /// generation does (`resident_pages::at_compile_phase`): an npm package decodes hundreds
    /// of modules back to back, and a release after each paged out every mapping again
    /// (lodash scenario, x86_64 VM, 2026-10-22: 45,700 madvise calls, 87 -> 372 ms).
    fn cached_program_decoded(&mut self) {
        if self.bootstrapped {
            crate::host::resident_pages::at_compile_phase(quench_runtime::CompilePhase::CodeGeneration);
        }
    }

    fn proper_tail_calls(&self) -> bool {
        false
    }

    fn nonextensible_applies_to_private(&self) -> bool {
        false
    }

    /// Node agents (the main thread and workers) may block in `Atomics.wait`.
    fn can_block(&self) -> bool {
        true
    }

    fn initialize_shadow_realm(
        context: &mut quench_runtime::NativeContext<'_, Self>,
        global: quench_runtime::RootId,
    ) -> Result<Option<quench_runtime::RootId>, quench_runtime::RootedError> {
        crate::modules::node_errors_shared_vm::initialize_shadow_policy(context, global).map(Some)
    }

    fn error_stack_formatter(&self) -> Option<quench_runtime::RootId> {
        self.shared_state()
            .borrow()
            .builtin_cache
            .get(crate::modules::node_errors_shared_vm::DEFAULT_STACK_FORMATTER)
            .copied()
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
        // Decoding with validation, and lowering, each run once per module and touch code
        // the other does not, nor what ran before them: each phase's pages leave the
        // resident set before the next phase, and the module runs.
        crate::host::resident_pages::release_read_only_pages();
        let module = quench_wasm::Engine::new().compile(bytes);
        crate::host::resident_pages::release_read_only_pages();
        let module = module
            .and_then(|module| module.lower_shared_module_for(mode))
            .map_err(|error| error.to_string());
        crate::host::resident_pages::release_read_only_pages();
        module
    }

    fn resolve_dynamic_import(
        &mut self,
        referrer: &str,
        specifier: &str,
    ) -> Result<Option<quench_runtime::ModuleSource>, quench_runtime::ModuleLoadError> {
        // Parsed once: every import resolves through here, and the table is 21 KB of JSON.
        static BUILTIN_EXPORTS: std::sync::OnceLock<std::collections::BTreeMap<String, Vec<String>>> =
            std::sync::OnceLock::new();
        let builtin_exports = BUILTIN_EXPORTS.get_or_init(|| {
            serde_json::from_str(include_str!("builtin_esm_exports.json"))
                .expect("generated Node builtin export names are valid JSON")
        });
        let builtin_name = specifier.strip_prefix("node:").unwrap_or(specifier);
        let builtin = crate::host::shared_vm::commonjs::is_builtin_specifier(specifier);
        if !builtin && specifier.starts_with("node:") {
            return Err(esm_errors::unknown_builtin(specifier));
        }
        if let Some(error) = esm_errors::unsupported_scheme(specifier) {
            return Err(error);
        }
        if builtin {
            let exports = builtin_exports
                .get(builtin_name)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let require_specifier = specifier;
            let mut source = format!(
                "const __quenchModule = globalThis[\"\\0quench:require\"]({});\nexport default __quenchModule;\n",
                serde_json::to_string(require_specifier).map_err(|error| error.to_string())?
            );
            // lib/internal/bootstrap/realm.js `syncExports`: each name is read with `getOwn`,
            // in the order the module lists its own keys (`builtin_export_values`). A member
            // still a pending lazy data property is bound to its pending value instead, and
            // the listener gives the binding the member's value when its group materializes.
            let assignments: String = exports
                .iter()
                .enumerate()
                .map(|(index, name)| format!("case {index}: {name} = value; break; "))
                .collect();
            source.push_str(&format!(
                "const __quenchExports = globalThis[{}](__quenchModule, {}, (index, value) => {{ switch (index) {{ {assignments}}} }});\n",
                serde_json::to_string(crate::host::shared_vm::commonjs::BUILTIN_EXPORT_VALUES)
                    .map_err(|error| error.to_string())?,
                serde_json::to_string(exports).map_err(|error| error.to_string())?
            ));
            for (index, name) in exports.iter().enumerate() {
                source.push_str(&format!("export let {name} = __quenchExports[{index}];\n"));
            }
            return Ok(Some(quench_runtime::ModuleSource::text(
                format!("node:quench-builtin/{builtin_name}"),
                source,
            )));
        }

        // Each (referrer, specifier) resolves to one location, as Node's loader caches a
        // resolution (`ModuleLoader#resolveCache`); module linking asks for each request in
        // several of its phases.
        let location = match self
            .resolved_imports
            .get(referrer)
            .and_then(|imports| imports.get(specifier))
        {
            Some(location) => std::rc::Rc::clone(location),
            None => {
                let Some(location) = self.import_location(referrer, specifier)? else {
                    return Ok(None);
                };
                let location = std::rc::Rc::new(location);
                self.resolved_imports
                    .entry(referrer.into())
                    .or_default()
                    .insert(specifier.into(), std::rc::Rc::clone(&location));
                location
            }
        };
        let (path, name) = (location.path.clone(), location.name.clone());
        let bytes = std::fs::read(&path).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => match referrer_location(referrer) {
                Ok((_, referrer_path)) => esm_errors::module_not_found(&path, &referrer_path),
                Err(error) => error,
            },
            _ => format!("module {}: {error}", path.display()).into(),
        })?;
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if extension == "wasm" {
            // Node's `wasm` format: the module record is a facade over the instance.
            let source = esm_wasm::facade(&name, &path, &bytes)?;
            return Ok(Some(quench_runtime::ModuleSource::text(name, source)));
        }
        let source = if extension == "json" {
            commonjs::source_text(&bytes).into_owned()
        } else {
            let kind = commonjs::source_kind_of_real(&path)?;
            if kind == quench_runtime::SourceKind::Module {
                String::from_utf8(bytes)
                    .map_err(|error| format!("module {} is not UTF-8: {error}", path.display()))?
            } else {
                let filename = serde_json::to_string(&path.to_string_lossy())
                    .map_err(|error| error.to_string())?;
                let mut source = format!(
                    "import {{ createRequire as __quenchCreateRequire }} from 'node:module';\nconst __quenchRequire = __quenchCreateRequire({filename});\nconst __quenchModule = __quenchRequire({filename});\nexport default __quenchModule;\n"
                );
                source.push_str("export { __quenchModule as 'module.exports' };\n");
                for (index, export) in
                    commonjs_exports::names(&path, &commonjs::source_text(&bytes))
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
        // Node exposes no module's bytes: it rejects `type: "bytes"` and has source-phase
        // objects only for WebAssembly. So a CommonJS or JSON interop wrapper keeps only the
        // text it compiles, never the file it wraps.
        Ok(Some(quench_runtime::ModuleSource::text(name, source)))
    }

    fn validate_module_type(
        &mut self,
        module: &quench_runtime::ModuleSource,
        module_type: &str,
    ) -> Result<(), quench_runtime::ModuleLoadError> {
        let url = self.import_meta_url(&module.name);
        esm_errors::validate_type(&url, module_type)
    }

    fn import_meta_url(&mut self, source_name: &str) -> String {
        if source_name.starts_with("node:")
            || source_name.starts_with("file:")
            || source_name.starts_with("<")
        {
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
        } else if self.eval_builtin_globals && source_name == self.inline_source.compilation_name()
        {
            self.inline_source.label().to_owned()
        } else if let Ok(relative) =
            std::path::Path::new(source_name).strip_prefix(self.node_library_root.join("lib"))
        {
            format!(
                "node:{}",
                relative
                    .with_extension("")
                    .to_string_lossy()
                    .replace('\\', "/")
            )
        } else {
            source_name.to_owned()
        }
    }

    fn create_promise_context(
        &mut self,
        parent: Option<quench_runtime::HostExecutionContext>,
    ) -> Option<quench_runtime::HostExecutionContext> {
        crate::modules::async_hooks_shared_vm::create_promise_context(&self.shared_state, parent)
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

    fn capture_job_context(
        &mut self,
        resource: Option<quench_runtime::HostExecutionContext>,
    ) -> Option<quench_runtime::HostExecutionContext> {
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

    fn release_promise_contexts(&mut self, contexts: &[quench_runtime::HostExecutionContext]) {
        crate::modules::async_hooks_shared_vm::release_promise_contexts(&self.shared_state, contexts);
    }

    fn functions(&self) -> &[HostFunction<Self>] {
        bindings()
    }

    /// The host operations a lane calls beside its frameless activations: each reads no
    /// frame, only argument values and host state, and enters guest code, if at all, through
    /// the runtime's calls (`Host::host_function_runs_beside_activations`).
    fn host_function_runs_beside_activations(&self, index: usize) -> bool {
        bindings().get(index).is_some_and(|binding| BESIDE_ACTIVATIONS.contains(&binding.name))
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
        context.host_mut().bootstrapped = true;
        Ok(())
    }

    fn resume(context: &mut NativeContext<'_, Self>) -> Result<(), RootedError> {
        crate::host::resident_pages::release_read_only_pages();
        context.scoped(crate::modules::process_shared_vm::refresh_identity)?;
        context.scoped(crate::modules::performance_binding_shared_vm::refresh_time_origins)?;
        commonjs::finish_initialize(context)?;
        context.host_mut().bootstrapped = true;
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
    let shape_key = crate::modules::event_constructor_shared_vm::get(context, event_data, "shapeKey")?;
    let capture_key =
        crate::modules::event_constructor_shared_vm::get(context, event_data, "captureKey")?;
    let validate = context.host_function(operation("emitterValidateCaptureRejections"))?;
    let dispatch =
        context.host_function_with_data(operation("emitterEmitListeners"), event_data)?;
    let emitter_factory = context.evaluate_script_rooted(
        crate::polyfills::bootstrap::event_emitter::JS,
        "node:bootstrap/event-emitter.js",
    )?;
    let undefined = context.undefined();
    let inspector = context.host_function(operation("emitterErrorInspector"))?;
    let event_emitter = context.call_rooted(
        emitter_factory,
        undefined,
        &[shape_key, capture_key, validate, dispatch, inspector],
    )?;
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
        (
            "newListener",
            "processStartListeningSignal",
            "startListeningIfSignal",
        ),
        (
            "removeListener",
            "processStopListeningSignal",
            "stopListeningIfSignal",
        ),
    ] {
        let event = context.string_rooted(event);
        let function = context.host_constructor_named(operation(operation_name), name)?;
        context.call_rooted(on, process, &[event, function])?;
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    #[test]
    fn operation_names_are_unique() {
        let mut names = std::collections::HashSet::new();
        for binding in super::bindings() {
            assert!(names.insert(binding.name), "Node operation {} is registered twice", binding.name);
        }
    }
}

/// Where an import loads from (`NodeHost::import_location`).
pub(crate) struct ImportLocation {
    path: std::path::PathBuf,
    name: String,
}

/// An import referrer's file URL, when it is one, and its path: Node resolves filesystem
/// specifiers as URLs before accessing a file.
fn referrer_location(
    referrer: &str,
) -> Result<(Option<url::Url>, std::path::PathBuf), quench_runtime::ModuleLoadError> {
    let Some(url) = referrer
        .starts_with("file:")
        .then(|| url::Url::parse(referrer))
        .transpose()
        .map_err(|error| error.to_string())?
    else {
        return Ok((None, std::path::PathBuf::from(referrer)));
    };
    let path = url
        .to_file_path()
        .map_err(|_| "invalid file URL referrer".to_owned())?;
    Ok((Some(url), path))
}

impl NodeHost {
    /// Where an import of `specifier` from `referrer` loads from: the file and the module's
    /// name, which keeps a file URL's query and fragment.
    fn import_location(
        &mut self,
        referrer: &str,
        specifier: &str,
    ) -> Result<Option<ImportLocation>, quench_runtime::ModuleLoadError> {
        use std::path::Path;

        // Node resolves filesystem specifiers as URLs before accessing a file.
        // The URL suffix belongs to module identity, never to the disk path.
        let (referrer_url, referrer_path) = referrer_location(referrer)?;
        let internal = specifier.starts_with("internal/");
        let base = if internal {
            self.node_library_root.join("lib")
        } else {
            referrer_path
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf()
        };
        let file_url = if specifier
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file:"))
        {
            Some(url::Url::parse(specifier).map_err(|error| error.to_string())?)
        } else if !internal
            && (specifier.starts_with("./")
                || specifier.starts_with("../")
                || Path::new(specifier).is_absolute())
        {
            let base_url = match referrer_url {
                Some(url) => url,
                None => {
                    let absolute = if referrer_path.is_absolute() {
                        referrer_path.clone()
                    } else {
                        std::env::current_dir()
                            .map_err(|error| error.to_string())?
                            .join(&referrer_path)
                    };
                    url::Url::from_file_path(absolute)
                        .map_err(|_| "invalid file module referrer".to_owned())?
                }
            };
            Some(
                base_url
                    .join(specifier)
                    .map_err(|error| error.to_string())?,
            )
        } else {
            None
        };
        let path = if let Some(url) = &file_url {
            let encoded = url.path().to_ascii_lowercase();
            if encoded.contains("%2f") || (cfg!(windows) && encoded.contains("%5c")) {
                return Err("File URL path must not include encoded path separators".into());
            }
            Some(
                url.to_file_path()
                    .map_err(|_| "invalid file module URL".to_owned())?,
            )
        } else if !internal {
            commonjs::exact_file_path(&base, specifier)
        } else {
            None
        };
        let path = match path {
            Some(path) => path,
            None => {
                let resolution_specifier = if internal {
                    format!("./{specifier}")
                } else {
                    specifier.to_owned()
                };
                let resolved = with_import_resolver(|resolver| {
                    match commonjs::resolve_kept(resolver, |resolver| resolver.resolve(&base, &resolution_specifier)) {
                        Ok(resolution) => Ok(Some(resolution.full_path())),
                        Err(oxc_resolver::ResolveError::Specifier(_)) => Ok(None),
                        Err(error) => Err(esm_errors::unresolved(resolver, error, specifier, &base, &referrer_path)),
                    }
                })?;
                let Some(path) = resolved else {
                    return Ok(None);
                };
                path
            }
        };
        let path = self.real_path(path);
        if !internal {
            if let Some(error) = esm_errors::unloadable(&path) {
                return Err(error);
            }
        }
        let name = if let Some(original) = file_url.as_ref().filter(|url| {
            url.query().is_some_and(|value| !value.is_empty())
                || url.fragment().is_some_and(|value| !value.is_empty())
        }) {
            let mut canonical = url::Url::from_file_path(&path)
                .map_err(|_| "invalid canonical module path".to_owned())?;
            canonical.set_query(original.query().filter(|value| !value.is_empty()));
            canonical.set_fragment(original.fragment().filter(|value| !value.is_empty()));
            canonical.to_string()
        } else {
            path.to_string_lossy().into_owned()
        };
        Ok(Some(ImportLocation { path, name }))
    }
}
