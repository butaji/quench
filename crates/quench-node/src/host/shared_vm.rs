//! The existing Node host adapted to the shared VM; Node policy stays in modules.

use super::NodeHost;
use quench_runtime::{HostFunction, HostFunctionId, NativeContext, RootedError, SystemHost};

pub(crate) mod commonjs;
pub(crate) mod commonjs_exports;
mod esm_errors;
pub(crate) mod esm_wasm;
pub(crate) use commonjs::source_kind;

pub(crate) use commonjs::with_import_resolver;

pub(crate) fn bindings() -> &'static [HostFunction<NodeHost>] {
    quench_runtime::host_functions![
        method "workerConstruct" (1) => crate::modules::worker_threads_shared_vm::construct,
        method "streamConstant" (0) => crate::modules::stream_shared_vm::constant,
        method "streamNop" (0) => crate::modules::stream_shared_vm::nop,
        method "streamPrependListener" (3) => crate::modules::stream_shared_vm::legacy::prepend_listener_export,
        method "streamGetHighWaterMark" (4) => crate::modules::stream_shared_vm::state::get_high_water_mark,
        method "streamGetDefaultHighWaterMark" (1) => crate::modules::stream_shared_vm::state::get_default_high_water_mark,
        method "streamSetDefaultHighWaterMark" (2) => crate::modules::stream_shared_vm::state::set_default_high_water_mark,
        method "streamConstructImpl" (2) => crate::modules::stream_shared_vm::destroy::construct_export,
        method "streamDestroyer" (2) => crate::modules::stream_shared_vm::destroy::destroyer_export,
        method "streamDestroy" (2) => crate::modules::stream_shared_vm::destroy::destroy,
        method "streamUndestroy" (0) => crate::modules::stream_shared_vm::destroy::undestroy,
        method "streamErrorOrDestroy" (3) => crate::modules::stream_shared_vm::destroy::error_or_destroy_export,
        method "streamPipelineImpl" (3) => crate::modules::stream_shared_vm::pipeline::pipeline_impl,
        method "streamPipeline" (0) => crate::modules::stream_shared_vm::pipeline::pipeline,
        method "streamAddAbortSignal" (2) => crate::modules::stream_shared_vm::utils::add_abort_signal_export,
        method "streamAddAbortSignalNoValidate" (2) => crate::modules::stream_shared_vm::utils::add_abort_signal_unchecked_export,
        method "streamFinished" (3) => crate::modules::stream_shared_vm::eos::finished,
        method "streamPromisesFinished" (2) => crate::modules::stream_shared_vm::eos::promises_finished,
        method "streamPromisesPipeline" (0) => crate::modules::stream_shared_vm::pipeline::promises_pipeline,
        method "streamIsArrayBufferView" (1) => crate::modules::stream_shared_vm::readable::is_view_export,
        method "streamIsUint8Array" (1) => crate::modules::stream_shared_vm::readable::is_uint8_array_export,
        method "streamUint8ArrayToBuffer" (1) => crate::modules::stream_shared_vm::readable::view_to_buffer_export,
        method "streamTickEmitReadable" (1) => crate::modules::stream_shared_vm::readable::emit_readable_tick,
        method "streamTickMaybeReadMore" (2) => crate::modules::stream_shared_vm::readable::maybe_read_more_tick,
        method "streamTickReadZero" (1) => crate::modules::stream_shared_vm::readable::read_zero_tick,
        method "streamTickUpdateReadableListening" (1) => crate::modules::stream_shared_vm::readable::update_readable_listening_tick,
        method "streamTickResume" (2) => crate::modules::stream_shared_vm::readable::resume_tick,
        method "streamTickEndReadable" (2) => crate::modules::stream_shared_vm::readable::end_readable_tick,
        method "streamTickEndWritable" (1) => crate::modules::stream_shared_vm::readable::end_writable_tick,
        method "streamTickOnWriteError" (4) => crate::modules::stream_shared_vm::writable::on_write_error_tick,
        method "streamTickAfterWrite" (4) => crate::modules::stream_shared_vm::writable::after_write_tick,
        method "streamTickAfterWriteTick" (1) => crate::modules::stream_shared_vm::writable::after_write_tick_info,
        method "streamTickFinishIfNeeded" (2) => crate::modules::stream_shared_vm::writable::finish_if_needed_tick,
        method "streamTickFinish" (2) => crate::modules::stream_shared_vm::writable::finish_tick,
        method "streamTickErrorBuffer" (1) => crate::modules::stream_shared_vm::writable::error_buffer_tick,
        method "streamTickEmitErrorClose" (2) => crate::modules::stream_shared_vm::destroy::emit_error_close_tick,
        method "streamTickEmitClose" (1) => crate::modules::stream_shared_vm::destroy::emit_close_tick,
        method "streamTickEmitError" (2) => crate::modules::stream_shared_vm::destroy::emit_error_tick,
        method "streamTickConstruct" (1) => crate::modules::stream_shared_vm::destroy::construct_tick,
        method "streamTickEmitCloseLegacy" (1) => crate::modules::stream_shared_vm::destroy::emit_close_legacy_tick,
        method "streamTickEmitErrorCloseLegacy" (2) => crate::modules::stream_shared_vm::destroy::emit_error_close_legacy_tick,
        method "streamStateFieldGet" (0) => crate::modules::stream_shared_vm::state::field_get,
        method "streamStateFieldSet" (1) => crate::modules::stream_shared_vm::state::field_set,
        method "streamReadableState" (3) => crate::modules::stream_shared_vm::state::readable_state,
        method "streamWritableState" (3) => crate::modules::stream_shared_vm::state::writable_state,
        method "streamReadableOnConstructed" (1) => crate::modules::stream_shared_vm::readable::on_constructed,
        method "streamWritableOnConstructed" (1) => crate::modules::stream_shared_vm::writable::on_constructed,
        method "streamWritableGetBuffer" (0) => crate::modules::stream_shared_vm::state::get_buffer,
        method "streamOnWrite" (1) => crate::modules::stream_shared_vm::writable::on_write,
        method "streamDestroyAfterConstruct" (1) => crate::modules::stream_shared_vm::destroy::destroy_after_construct,
        method "streamOnDestroy" (1) => crate::modules::stream_shared_vm::destroy::on_destroy,
        method "streamOnConstruct" (1) => crate::modules::stream_shared_vm::destroy::on_construct,
        method "streamConstructDone" (1) => crate::modules::stream_shared_vm::destroy::construct_done,
        method "streamOnConstructed" (0) => crate::modules::stream_shared_vm::destroy::on_constructed,
        method "streamCaptureRejection" (1) => crate::modules::stream_shared_vm::destroy::capture_rejection,
        method "streamDefaultDestroy" (2) => crate::modules::stream_shared_vm::readable::default_destroy,
        method "streamReadable" (1) => crate::modules::stream_shared_vm::readable::constructor,
        method "streamReadableFrom" (2) => crate::modules::stream_shared_vm::from::from,
        method "streamReadablePush" (2) => crate::modules::stream_shared_vm::readable::push,
        method "streamReadableUnshift" (2) => crate::modules::stream_shared_vm::readable::unshift,
        method "streamReadableIsPaused" (0) => crate::modules::stream_shared_vm::readable::is_paused,
        method "streamReadableSetEncoding" (1) => crate::modules::stream_shared_vm::readable::set_encoding,
        method "streamReadableRead" (1) => crate::modules::stream_shared_vm::readable::read,
        method "streamReadableDefaultRead" (1) => crate::modules::stream_shared_vm::readable::default_read,
        method "streamReadablePipe" (2) => crate::modules::stream_shared_vm::readable::pipe,
        method "streamReadableUnpipe" (1) => crate::modules::stream_shared_vm::readable::unpipe,
        method "streamReadableOn" (2) => crate::modules::stream_shared_vm::readable::on_listener,
        method "streamReadableRemoveListener" (2) => crate::modules::stream_shared_vm::readable::remove_listener_method,
        method "streamReadableRemoveAllListeners" (1) => crate::modules::stream_shared_vm::readable::remove_all_listeners,
        method "streamReadableResume" (0) => crate::modules::stream_shared_vm::readable::resume,
        method "streamReadablePause" (0) => crate::modules::stream_shared_vm::readable::pause,
        method "streamReadableWrap" (1) => crate::modules::stream_shared_vm::readable::wrap,
        method "streamReadableIterator" (1) => crate::modules::stream_shared_vm::iterator::iterator,
        method "streamReadableAsyncDispose" (0) => crate::modules::stream_shared_vm::readable::async_dispose,
        method "streamReadableAsyncIterator" (0) => crate::modules::stream_shared_vm::iterator::async_iterator,
        method "streamPipeOnUnpipe" (2) => crate::modules::stream_shared_vm::readable::pipe_on_unpipe,
        method "streamPipeOnEnd" (0) => crate::modules::stream_shared_vm::readable::pipe_on_end,
        method "streamPipeCleanup" (0) => crate::modules::stream_shared_vm::readable::pipe_cleanup,
        method "streamPipeOnData" (1) => crate::modules::stream_shared_vm::readable::pipe_on_data,
        method "streamPipeOnError" (1) => crate::modules::stream_shared_vm::readable::pipe_on_error,
        method "streamPipeOnClose" (0) => crate::modules::stream_shared_vm::readable::pipe_on_close,
        method "streamPipeOnFinish" (0) => crate::modules::stream_shared_vm::readable::pipe_on_finish,
        method "streamPipeUnpipe" (0) => crate::modules::stream_shared_vm::readable::pipe_unpipe,
        method "streamPipeOnDrain" (0) => crate::modules::stream_shared_vm::readable::pipe_on_drain,
        method "streamWrapOnData" (1) => crate::modules::stream_shared_vm::readable::wrap_on_data,
        method "streamWrapOnEnd" (0) => crate::modules::stream_shared_vm::readable::wrap_on_end,
        method "streamWrapOnError" (1) => crate::modules::stream_shared_vm::readable::wrap_on_error,
        method "streamWrapOnClose" (0) => crate::modules::stream_shared_vm::readable::wrap_on_close,
        method "streamWrapRead" (0) => crate::modules::stream_shared_vm::readable::wrap_read,
        method "streamAccessorGet" (0) => crate::modules::stream_shared_vm::utils::accessor_get,
        method "streamAccessorSet" (1) => crate::modules::stream_shared_vm::utils::accessor_set,
        method "streamPredicate" (1) => crate::modules::stream_shared_vm::utils::predicate,
        method "streamOnSignalAbort" (0) => crate::modules::stream_shared_vm::utils::on_signal_abort,
        method "streamWritable" (1) => crate::modules::stream_shared_vm::writable::constructor,
        method "streamWritableHasInstance" (1) => crate::modules::stream_shared_vm::writable::has_instance,
        method "streamWritablePipe" (0) => crate::modules::stream_shared_vm::writable::pipe,
        method "streamWritableWrite" (3) => crate::modules::stream_shared_vm::writable::write,
        method "streamWritableCork" (0) => crate::modules::stream_shared_vm::writable::cork,
        method "streamWritableUncork" (0) => crate::modules::stream_shared_vm::writable::uncork,
        method "streamWritableSetDefaultEncoding" (1) => crate::modules::stream_shared_vm::writable::set_default_encoding,
        method "streamWritableDefaultWrite" (3) => crate::modules::stream_shared_vm::writable::default_write,
        method "streamWritableEnd" (3) => crate::modules::stream_shared_vm::writable::end,
        method "streamWritableDestroy" (2) => crate::modules::stream_shared_vm::writable::destroy,
        method "streamWritableAsyncDispose" (0) => crate::modules::stream_shared_vm::writable::async_dispose,
        method "streamWritevCallback" (1) => crate::modules::stream_shared_vm::writable::writev_callback,
        method "streamOnFinal" (1) => crate::modules::stream_shared_vm::writable::on_final,
        method "streamDuplex" (1) => crate::modules::stream_shared_vm::duplex::constructor,
        method "streamTransform" (1) => crate::modules::stream_shared_vm::duplex::transform,
        method "streamTransformPrefinish" (0) => crate::modules::stream_shared_vm::duplex::transform_prefinish,
        method "streamTransformFinal" (1) => crate::modules::stream_shared_vm::duplex::transform_final,
        method "streamTransformFlushed" (2) => crate::modules::stream_shared_vm::duplex::transform_flushed,
        method "streamTransformDefault" (3) => crate::modules::stream_shared_vm::duplex::default_transform,
        method "streamTransformWrite" (3) => crate::modules::stream_shared_vm::duplex::transform_write,
        method "streamTransformDone" (2) => crate::modules::stream_shared_vm::duplex::transform_done,
        method "streamTransformRead" (0) => crate::modules::stream_shared_vm::duplex::transform_read,
        method "streamPassThrough" (1) => crate::modules::stream_shared_vm::duplex::pass_through,
        method "streamPassThroughTransform" (3) => crate::modules::stream_shared_vm::duplex::pass_through_transform,
        method "streamDuplexSide" (1) => crate::modules::stream_shared_vm::duplex::duplex_side,
        method "streamDuplexSideRead" (0) => crate::modules::stream_shared_vm::duplex::side_read,
        method "streamDuplexSideWrite" (3) => crate::modules::stream_shared_vm::duplex::side_write,
        method "streamDuplexSideFinal" (1) => crate::modules::stream_shared_vm::duplex::side_final,
        method "streamDuplexPair" (1) => crate::modules::stream_shared_vm::duplex::duplex_pair,
        method "streamReadableFromList" (2) => crate::modules::stream_shared_vm::readable::from_list_export,
        method "streamReadableWrapStatic" (2) => crate::modules::stream_shared_vm::readable::wrap_static,
        method "streamWrapDestroy" (2) => crate::modules::stream_shared_vm::readable::wrap_destroy,
        method "streamLegacy" (1) => crate::modules::stream_shared_vm::legacy::constructor,
        method "streamLegacyPipe" (2) => crate::modules::stream_shared_vm::legacy::pipe,
        method "streamLegacyEventNames" (0) => crate::modules::stream_shared_vm::legacy::event_names,
        method "streamLegacyOnData" (1) => crate::modules::stream_shared_vm::legacy::on_data,
        method "streamLegacyOnDrain" (0) => crate::modules::stream_shared_vm::legacy::on_drain,
        method "streamLegacyOnEnd" (0) => crate::modules::stream_shared_vm::legacy::on_end,
        method "streamLegacyOnClose" (0) => crate::modules::stream_shared_vm::legacy::on_close,
        method "streamLegacyOnError" (1) => crate::modules::stream_shared_vm::legacy::on_error,
        method "streamLegacyCleanup" (0) => crate::modules::stream_shared_vm::legacy::cleanup,
        method "streamEosOnLegacyFinish" (0) => crate::modules::stream_shared_vm::eos::on_legacy_finish,
        method "streamEosOnFinish" (0) => crate::modules::stream_shared_vm::eos::on_finish,
        method "streamEosOnEnd" (0) => crate::modules::stream_shared_vm::eos::on_end,
        method "streamEosOnError" (1) => crate::modules::stream_shared_vm::eos::on_error,
        method "streamEosOnClose" (0) => crate::modules::stream_shared_vm::eos::on_close,
        method "streamEosOnClosed" (0) => crate::modules::stream_shared_vm::eos::on_closed,
        method "streamEosOnRequest" (0) => crate::modules::stream_shared_vm::eos::on_request,
        method "streamEosCleanup" (0) => crate::modules::stream_shared_vm::eos::cleanup,
        method "streamEosAbort" (0) => crate::modules::stream_shared_vm::eos::abort,
        method "streamEosWebAbort" (0) => crate::modules::stream_shared_vm::eos::web_abort,
        method "streamEosWebSettled" (0) => crate::modules::stream_shared_vm::eos::web_settled,
        method "streamEosWebDeliver" (0) => crate::modules::stream_shared_vm::eos::web_deliver,
        method "streamPromisesFinishedSettle" (1) => crate::modules::stream_shared_vm::eos::promises_finished_settle,
        method "streamDisposeSettle" (1) => crate::modules::stream_shared_vm::eos::dispose_settle,
        method "streamIteratorSelf" (0) => crate::modules::stream_shared_vm::iterator::iterator_self,
        method "streamIteratorNext" (1) => crate::modules::stream_shared_vm::iterator::next,
        method "streamIteratorReturn" (1) => crate::modules::stream_shared_vm::iterator::return_,
        method "streamIteratorThrow" (1) => crate::modules::stream_shared_vm::iterator::throw,
        method "streamIteratorOnReadable" (0) => crate::modules::stream_shared_vm::iterator::on_readable,
        method "streamIteratorOnFinished" (1) => crate::modules::stream_shared_vm::iterator::on_finished,
        method "streamIteratorResume" (0) => crate::modules::stream_shared_vm::iterator::resume,
        method "streamIteratorContinue" (0) => crate::modules::stream_shared_vm::iterator::continue_step,
        method "streamIteratorYielded" (1) => crate::modules::stream_shared_vm::iterator::yielded,
        method "streamIteratorReturned" (1) => crate::modules::stream_shared_vm::iterator::returned,
        method "streamIteratorYieldFailed" (1) => crate::modules::stream_shared_vm::iterator::yield_failed,
        method "streamFromWholeRead" (0) => crate::modules::stream_shared_vm::from::whole_read,
        method "streamFromRead" (0) => crate::modules::stream_shared_vm::from::read,
        method "streamFromDestroy" (2) => crate::modules::stream_shared_vm::from::destroy,
        method "streamFromFirstAsyncValue" (1) => crate::modules::stream_shared_vm::from::first_async_value,
        method "streamFromRejected" (1) => crate::modules::stream_shared_vm::from::rejected,
        method "streamFromAsyncValue" (1) => crate::modules::stream_shared_vm::from::async_value,
        method "streamFromAsyncResult" (1) => crate::modules::stream_shared_vm::from::async_result,
        method "streamFromClosed" (0) => crate::modules::stream_shared_vm::from::closed,
        method "streamFromCloseFailed" (1) => crate::modules::stream_shared_vm::from::close_failed,
        method "streamFromThrown" (1) => crate::modules::stream_shared_vm::from::thrown,
        method "streamFromThrownValue" (0) => crate::modules::stream_shared_vm::from::thrown_value,
        method "streamFromCloseRejected" (1) => crate::modules::stream_shared_vm::from::close_rejected,
        method "streamFromReturned" (1) => crate::modules::stream_shared_vm::from::returned,
        method "streamFromSettle" (0) => crate::modules::stream_shared_vm::from::settle,
        method "streamPipelineFinish" (1) => crate::modules::stream_shared_vm::pipeline::finish,
        method "streamPipelineFinishOnlyHandleError" (1) => crate::modules::stream_shared_vm::pipeline::finish_only_handle_error,
        method "streamPipelineAbort" (0) => crate::modules::stream_shared_vm::pipeline::abort,
        method "streamPipelineOnError" (1) => crate::modules::stream_shared_vm::pipeline::on_error,
        method "streamPipelineRemoveErrorListener" (0) => crate::modules::stream_shared_vm::pipeline::remove_error_listener,
        method "streamPipelineStageClosed" (0) => crate::modules::stream_shared_vm::pipeline::stage_closed,
        method "streamPipelineStageSettled" (1) => crate::modules::stream_shared_vm::pipeline::stage_settled,
        method "streamPipelineStageDestroy" (1) => crate::modules::stream_shared_vm::pipeline::stage_destroy,
        method "streamPipelineValue" (1) => crate::modules::stream_shared_vm::pipeline::value,
        method "streamPipelineValueFailed" (1) => crate::modules::stream_shared_vm::pipeline::value_failed,
        method "streamPipelineDestinationClosed" (0) => crate::modules::stream_shared_vm::pipeline::destination_closed,
        method "streamPipelineEndDestination" (0) => crate::modules::stream_shared_vm::pipeline::end_destination,
        method "streamPipelineSourceDone" (1) => crate::modules::stream_shared_vm::pipeline::source_done,
        method "streamPumpResume" (1) => crate::modules::stream_shared_vm::pipeline::pump_resume,
        method "streamPumpNext" (0) => crate::modules::stream_shared_vm::pipeline::pump_next,
        method "streamPumpResult" (1) => crate::modules::stream_shared_vm::pipeline::pump_result,
        method "streamPumpSyncValue" (1) => crate::modules::stream_shared_vm::pipeline::pump_sync_value,
        method "streamPumpBodyFailed" (1) => crate::modules::stream_shared_vm::pipeline::pump_body_failed,
        method "streamPumpClosed" (0) => crate::modules::stream_shared_vm::pipeline::pump_closed,
        method "streamPumpFailed" (1) => crate::modules::stream_shared_vm::pipeline::pump_failed,
        method "streamPumpEnded" (0) => crate::modules::stream_shared_vm::pipeline::pump_ended,
        method "streamPromisesPipelineSettle" (2) => crate::modules::stream_shared_vm::pipeline::promises_pipeline_settle,
        method "workerSetEnvironmentData" (2) => crate::modules::worker_threads_shared_vm::set_environment_data,
        method "workerGetEnvironmentData" (1) => crate::modules::worker_threads_shared_vm::get_environment_data,
        method "workerRawUmask" (0) => crate::modules::worker_threads_shared_vm::raw_umask,
        method "workerSerializeFailure" (0) => crate::modules::worker_threads_shared_vm::serialize_failure,
        method "workerEvaluate" (0) => crate::modules::worker_threads_shared_vm::evaluate,
        method "workerThreadId" (0) => crate::modules::worker_threads_shared_vm::thread_id,
        method "workerThreadName" (0) => crate::modules::worker_threads_shared_vm::thread_name,
        method "workerPost" (0) => crate::modules::worker_threads_shared_vm::post,
        method "workerMovePortToContext" (2) => crate::modules::worker_threads_shared_vm::move_port_to_context,
        method "workerRef" (0) => crate::modules::worker_threads_shared_vm::ref_worker,
        method "workerUnref" (0) => crate::modules::worker_threads_shared_vm::unref_worker,
        method "workerHandleHasRef" (0) => crate::modules::worker_threads_shared_vm::handle_has_ref,
        method "workerPortHandleHasRef" (0) => crate::modules::worker_threads_shared_vm::message_port_handle_has_ref,
        method "workerTerminate" (1) => crate::modules::worker_threads_shared_vm::terminate,
        method "workerCpuUsage" (1) => crate::modules::worker_threads_shared_vm::cpu_usage,
        method "workerTerminateExecutor" (0) => crate::modules::worker_threads_shared_vm::terminate_executor,
        method "workerPortPost" (0) => crate::modules::worker_threads_shared_vm::port_post,
        method "workerInternalPost" (1) => crate::modules::worker_threads_shared_vm::internal_post,
        method "workerBroadcastPost" (2) => crate::modules::worker_threads_shared_vm::broadcast_post,
        method "workerBroadcastRef" (1) => crate::modules::worker_threads_shared_vm::broadcast_ref,
        method "workerPortOn" (2) => crate::modules::worker_threads_shared_vm::port_on,
        method "workerPortRef" (0) => crate::modules::worker_threads_shared_vm::port_ref,
        method "workerPortUnref" (0) => crate::modules::worker_threads_shared_vm::port_unref,
        method "workerPortClose" (0) => crate::modules::worker_threads_shared_vm::port_close,
        method "workerPortHasRef" (0) => crate::modules::worker_threads_shared_vm::port_has_ref,
        method "workerPoll" (0) => crate::modules::worker_threads_shared_vm::poll_native,
        method "v8QueryObjects" (1) => crate::modules::v8_shared_vm::query_objects,

        method "nodeLibraryRequire" (1) => commonjs::require_node_library,
        method "primordialsCopy" (7) => crate::modules::primordials_shared_vm::copy,
        method "nodeTestMock" (0) => crate::modules::test_shared_vm::mock,
        method "nodeTestNamespace" (0) => crate::modules::test_shared_vm::namespace,
        method "nodeTestRun" (0) => crate::modules::test_shared_vm::run,
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
        method "utilTypeCheck" (1) => crate::modules::util_module_shared_vm::types_check,
        method "utilKeyTypeCheck" (1) => crate::modules::util_module_shared_vm::key_type_check,
        method "utilInspect" (2) => crate::modules::util_inspect_shared_vm::inspect,
        method "utilFormat" (0) => crate::modules::util_inspect_shared_vm::format,
        method "utilFormatWithOptions" (1) => crate::modules::util_inspect_shared_vm::format_with_options_operation,
        method "utilInspectStylize" (2) => crate::modules::util_inspect_shared_vm::stylize_operation,
        method "utilInspectDefaultsGet" (0) => crate::modules::util_module_shared_vm::inspect_defaults_get,
        method "utilInspectDefaultsSet" (1) => crate::modules::util_module_shared_vm::inspect_defaults_set,
        method "utilColorAliasGet" (0) => crate::modules::util_module_shared_vm::color_alias_get,
        method "utilColorAliasSet" (1) => crate::modules::util_module_shared_vm::color_alias_set,
        method "utilLegacyIs" (1) => crate::modules::util_module_shared_vm::legacy_is,
        method "utilInherits" (2) => crate::modules::util_module_shared_vm::inherits,
        method "utilExtend" (2) => crate::modules::util_module_shared_vm::extend,
        method "utilDeprecate" (4) => crate::modules::util_module_shared_vm::deprecate,
        method "utilDeprecated" (0) => crate::modules::util_module_shared_vm::deprecated,
        method "utilGetCallSite" (0) => crate::modules::util_module_shared_vm::get_call_site,
        method "utilGetCallSitesPublic" (0) => crate::modules::util_module_shared_vm::get_call_sites,
        method "utilGetSystemErrorName" (1) => crate::modules::util_module_shared_vm::get_system_error_name,
        method "utilGetSystemErrorMessage" (1) => crate::modules::util_module_shared_vm::get_system_error_message,
        method "utilGetSystemErrorMap" (0) => crate::modules::util_module_shared_vm::get_system_error_map,
        method "utilErrnoException" (0) => crate::modules::util_module_shared_vm::errno_exception,
        method "utilExceptionWithHostPort" (0) => crate::modules::util_module_shared_vm::exception_with_host_port,
        method "utilStripVTControlCharacters" (1) => crate::modules::util_module_shared_vm::strip_vt_control_characters,
        method "utilToUSVString" (1) => crate::modules::util_module_shared_vm::to_usv_string,
        method "utilSetTraceSigInt" (1) => crate::modules::util_module_shared_vm::set_trace_sig_int,
        method "utilGlobalExport" (0) => crate::modules::util_module_shared_vm::global_export,
        method "utilPromisify" (1) => crate::modules::util_module_shared_vm::promisify,
        method "utilPromisified" (0) => crate::modules::util_module_shared_vm::promisified,
        method "utilPromisifySettle" (2) => crate::modules::util_module_shared_vm::promisify_settle,
        method "utilLog" (0) => crate::modules::util_module_shared_vm::log,
        method "utilDebuglog" (2) => crate::modules::util_module_shared_vm::debuglog,
        method "utilDebugLogger" (0) => crate::modules::util_module_shared_vm::debug_logger,
        method "utilDebugEnabled" (0) => crate::modules::util_module_shared_vm::debug_enabled_get,
        method "utilDebugWrite" (0) => crate::modules::util_module_shared_vm::debug_write,
        method "utilNoop" (0) => crate::modules::util_module_shared_vm::noop,
        method "utilTransferableAbortSignal" (1) => crate::modules::util_module_shared_vm::transferable_abort_signal,
        method "utilTransferableAbortController" (0) => crate::modules::util_module_shared_vm::transferable_abort_controller,
        method "utilAborted" (2) => crate::modules::util_module_shared_vm::aborted,
        method "utilAbortedResolve" (0) => crate::modules::util_module_shared_vm::aborted_resolve,
        method "mimeTypeConstruct" (1) => crate::modules::util_mime_shared_vm::construct_type,
        method "mimeTypeGet" (0) => crate::modules::util_mime_shared_vm::get_field,
        method "mimeTypeSet" (1) => crate::modules::util_mime_shared_vm::set_field,
        method "mimeTypeToString" (0) => crate::modules::util_mime_shared_vm::type_to_string,
        method "mimeParamsConstruct" (0) => crate::modules::util_mime_shared_vm::construct_params,
        method "mimeParamsDelete" (1) => crate::modules::util_mime_shared_vm::params_delete,
        method "mimeParamsGet" (1) => crate::modules::util_mime_shared_vm::params_get,
        method "mimeParamsHas" (1) => crate::modules::util_mime_shared_vm::params_has,
        method "mimeParamsSet" (2) => crate::modules::util_mime_shared_vm::params_set,
        method "mimeParamsEntries" (0) => crate::modules::util_mime_shared_vm::params_entries,
        method "mimeParamsKeys" (0) => crate::modules::util_mime_shared_vm::params_keys,
        method "mimeParamsValues" (0) => crate::modules::util_mime_shared_vm::params_values,
        method "mimeParamsToString" (0) => crate::modules::util_mime_shared_vm::params_to_string,
        method "utilDiff" (2) => crate::modules::util_comparisons_shared_vm::diff,
        method "utilIsDeepStrictEqual" (2) => crate::modules::util_comparisons_shared_vm::is_deep_strict_equal,
        method "utilProxyDetails" (0) => crate::modules::util_shared_vm::proxy_details,
        method "utilGetCallerLocation" (0) => crate::modules::util_shared_vm::get_caller_location,
        method "utilGetCallSites" (0) => crate::modules::util_shared_vm::get_call_sites,
        method "utilGuessHandleType" (1) => crate::modules::util_shared_vm::guess_handle_type,
        method "utilConstructorName" (0) => crate::modules::util_shared_vm::constructor_name,
        method "utilPreviewEntries" (0) => crate::modules::util_shared_vm::preview_entries,
        method "availableMemory" (0) => crate::modules::process_memory_shared_vm::available,
        method "constrainedMemory" (0) => crate::modules::process_memory_shared_vm::constrained,
        method "uptime" (0) => crate::modules::process_shared_vm::uptime,
        #[cfg(unix)]
        method "processMetricsHrtime" (0) => crate::modules::process_metrics_shared_vm::hrtime,
        #[cfg(unix)]
        method "processMetricsHrtimeBigInt" (0) => crate::modules::process_metrics_shared_vm::hrtime_bigint,
        #[cfg(unix)]
        method "processMetricsCpu" (0) => crate::modules::process_metrics_shared_vm::process_cpu,
        #[cfg(unix)]
        method "processMetricsThreadCpu" (0) => crate::modules::process_metrics_shared_vm::thread_cpu,
        #[cfg(unix)]
        method "processMetricsResources" (0) => crate::modules::process_metrics_shared_vm::resources,
        #[cfg(unix)]
        method "processMetricsRss" (0) => crate::modules::process_metrics_shared_vm::rss,
        method "processMetricsMemoryUsage" (1) => crate::modules::process_metrics_shared_vm::memory_usage,
        #[cfg(unix)]
        method "processExecve" (3) => crate::modules::process_execve_shared_vm::execve,
        method "processMetricsPatchProcessObject" (1) => crate::modules::process_metrics_shared_vm::patch_process_object,
        method "processExitCodeGet" (0) => crate::modules::process_shared_vm::exit_code_get,
        method "processExitCodeSet" (1) => crate::modules::process_shared_vm::exit_code_set,
        method "processDefaultFatalException" (2) => crate::modules::process_fatal_exception_route::default_handler,
        method "processDispatchFatalException" (2) => crate::modules::process_fatal_exception_route::dispatch,
        method "processExit" (1) => crate::modules::process_shared_vm::exit,
        method "processEmitExitEvent" (1) => crate::modules::process_shared_vm::emit_exit_event,
        method "reallyExit" (1) => crate::modules::process_shared_vm::really_exit,
        method "asyncHookFatalError" (1) => crate::modules::async_hook_registry_shared_vm::fatal_error,
        method "processConstructor" (0) => crate::modules::process_surface::constructor,
        method "processProtoAccess" (0) => crate::modules::process_surface::proto_access,
        method "processLoadEnvFileNone" (0) => crate::modules::process_surface::load_env_file_none,
        method "processKillPortable" (2) => crate::modules::process_surface::kill_portable,
        method "emitterProtoMaxGet" (0) => crate::modules::event_limits_shared_vm::proto_max_get,
        method "emitterProtoMaxSet" (1) => crate::modules::event_limits_shared_vm::proto_max_set,
        method "emitterConstructor" (1) => crate::modules::event_emitter_shared_vm::constructor,
        method "emitterInit" (1) => crate::modules::event_emitter_shared_vm::init,
        method "emitterProtoEventNames" (0) => crate::modules::event_emitter_shared_vm::proto_event_names,
        method "emitterProtoListeners" (1) => crate::modules::event_emitter_shared_vm::proto_listeners,
        method "emitterProtoRawListeners" (1) => crate::modules::event_emitter_shared_vm::proto_raw_listeners,
        method "emitterProtoListenerCount" (2) => crate::modules::event_emitter_shared_vm::proto_listener_count,
        method "emitterProtoAddListener" (2) => crate::modules::event_emitter_shared_vm::proto_add_listener,
        method "emitterProtoPrependListener" (2) => crate::modules::event_emitter_shared_vm::proto_prepend_listener,
        method "emitterProtoOnce" (2) => crate::modules::event_emitter_shared_vm::proto_once,
        method "emitterProtoPrependOnceListener" (2) => crate::modules::event_emitter_shared_vm::proto_prepend_once_listener,
        method "emitterProtoRemoveListener" (2) => crate::modules::event_emitter_shared_vm::proto_remove_listener,
        method "emitterProtoRemoveAllListeners" (1) => crate::modules::event_emitter_shared_vm::proto_remove_all_listeners,
        method "emitterProtoEmit" (1) => crate::modules::event_emitter_shared_vm::proto_emit,
        method "bufferGlobalGet" (0) => crate::modules::buffer_shared_vm::global_get,
        method "bufferGlobalSet" (1) => crate::modules::buffer_shared_vm::global_set,
        method "bufferClassConstructor" (3) => crate::modules::buffer_module::constructor,
        method "bufferFrom" (3) => crate::modules::buffer_module::static_from,
        method "bufferOf" (0) => crate::modules::buffer_module::of,
        method "bufferAlloc" (3) => crate::modules::buffer_module::alloc,
        method "bufferAllocUnsafe" (1) => crate::modules::buffer_module::alloc_unsafe,
        method "bufferIsBuffer" (1) => crate::modules::buffer_module::is_buffer,
        method "bufferIsEncoding" (1) => crate::modules::buffer_module::is_encoding,
        method "bufferByteLength" (2) => crate::modules::buffer_module::byte_length,
        method "bufferStaticCompare" (2) => crate::modules::buffer_module::static_compare,
        method "bufferConcat" (2) => crate::modules::buffer_module::concat,
        method "bufferCopyBytesFrom" (3) => crate::modules::buffer_module::copy_bytes_from,
        method "bufferToString" (3) => crate::modules::buffer_module::to_string,
        method "bufferEquals" (1) => crate::modules::buffer_module::equals,
        method "bufferCompare" (5) => crate::modules::buffer_module::compare,
        method "bufferCopy" (4) => crate::modules::buffer_module::copy,
        method "bufferFill" (4) => crate::modules::buffer_module::fill,
        method "bufferWrite" (4) => crate::modules::buffer_module::write,
        method "bufferToJson" (0) => crate::modules::buffer_module::to_json,
        method "bufferSlice" (2) => crate::modules::buffer_module::slice,
        method "bufferInspect" (2) => crate::modules::buffer_module::inspect,
        method "bufferIndexOf" (3) => crate::modules::buffer_module::index_of,
        method "bufferSwap" (0) => crate::modules::buffer_module::swap,
        method "bufferEncodingSlice" (2) => crate::modules::buffer_module::encoding_slice,
        method "bufferEncodingWrite" (3) => crate::modules::buffer_module::encoding_write,
        method "bufferNumeric" (2) => crate::modules::buffer_module::numeric,
        method "bufferAliasGet" (0) => crate::modules::buffer_module::alias_get,
        method "bufferSlowBuffer" (1) => crate::modules::buffer_module::slow_buffer,
        method "moduleParentDeprecated" (0) => crate::modules::module_shared_vm::parent_deprecated,
        method "moduleEnableCompileCache" (1) => crate::modules::module_shared_vm::enable_compile_cache,
        method "moduleFlushCompileCache" (0) => crate::modules::module_shared_vm::flush_compile_cache,
        method "moduleGetCompileCacheDir" (0) => crate::modules::module_shared_vm::get_compile_cache_dir,
        method "moduleInitPaths" (0) => crate::modules::module_shared_vm::init_paths,
        method "moduleResolveLookupPaths" (2) => crate::modules::module_shared_vm::resolve_lookup_paths,
        method "moduleLoad" (2) => crate::modules::module_shared_vm::load,
        method "navigatorGet" (0) => crate::host::shared_vm::commonjs::navigator_get,
        method "lazyGlobalGet" (0) => crate::host::shared_vm::commonjs::lazy_global_get,
        method "lazyGlobalSet" (1) => crate::host::shared_vm::commonjs::lazy_global_set,
        method "taskQueueMicrotask" (1) => crate::host::shared_vm::commonjs::queue_microtask,
        method "taskQueueMicrotaskJob" (0) => crate::host::shared_vm::commonjs::queue_microtask_job,
        method "taskQueueMicrotaskRun" (0) => crate::host::shared_vm::commonjs::queue_microtask_run,
        method "strictAssign" (3) => crate::modules::events_shared_vm::strict_assign,
        method "strictDelete" (2) => crate::modules::events_shared_vm::strict_delete,
        method "disposableFactory" (2) => crate::modules::events_shared_vm::disposable_factory,
        method "disposableDispose" (0) => crate::modules::events_shared_vm::disposable_dispose,
        method "processStreamWrite" (1) => crate::modules::process_shared_vm::stream_write,
        method "processStdioEnd" (3) => crate::modules::process_shared_vm::stdio_end,
        method "processStdioEndCallback" (0) => crate::modules::process_shared_vm::stdio_end_callback,
        method "processStdioReference" (0) => crate::modules::process_shared_vm::stdio_reference,
        method "processGlobalGet" (0) => crate::modules::process_shared_vm::global_get,
        method "processGlobalSet" (1) => crate::modules::process_shared_vm::global_set,
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
        method "processHrtime" (1) => crate::modules::process_shared_vm::hrtime,
        method "processHrtimeBigint" (0) => crate::modules::process_shared_vm::hrtime_bigint,
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
        method "messagingStructuredClone" (2) => crate::modules::clone_shared_vm::structured_clone,
        method "messagingSetDeserializer" (1) => crate::modules::messaging_binding_shared_vm::set_deserializer,
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
        method "reportBindingGetReport" (0) => crate::modules::report_configuration_shared_vm::binding_get_report,
        method "processLoadEnvFile" (1) => crate::modules::process_shared_vm::load_env_file,
        method "processSetEnv" (2) => crate::modules::process_shared_vm::set_env,
        method "processEnvSet" (4) => crate::modules::process_shared_vm::env_set,
        method "processEnvDefineProperty" (3) => crate::modules::process_shared_vm::env_define_property,
        method "processEnvDeleteProperty" (2) => crate::modules::process_shared_vm::env_delete_property,
        method "processDeleteEnv" (1) => crate::modules::process_shared_vm::delete_env,
        method "processGetSharedEnv" (1) => crate::modules::process_shared_vm::get_shared_env,
        method "processBinding" (1) => crate::modules::process_binding_shared_vm::binding,
        method "processSetUncaughtExceptionCaptureCallback" (1) => crate::modules::process_shared_vm::set_uncaught_exception_capture_callback,
        method "processHasUncaughtExceptionCaptureCallback" (0) => crate::modules::process_shared_vm::has_uncaught_exception_capture_callback,
        method "processAddUncaughtExceptionCaptureCallback" (1) => crate::modules::process_shared_vm::add_uncaught_exception_capture_callback,
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
        method "errorSetSourceMapsEnabled" (1) => crate::modules::node_errors_shared_vm::set_source_maps_enabled,
        method "errorTriggerUncaughtException" (2) => crate::modules::node_errors_shared_vm::trigger_uncaught_exception,
        method "errorStackFormat" (3) => crate::modules::node_errors_shared_vm::format_stack,
        method "errorPrepareStackTraceInitialize" (0) => crate::modules::node_errors_shared_vm::initialize_prepare_stack_trace,
        method "consoleGlobalInitialize" (0) => commonjs::initialize_console_global,
        method "consoleWrite" (0) => crate::modules::console_shared_vm::write,
        method "consoleLoad" (0) => crate::modules::console_shared_vm::load,
        method "consoleMethod" (0) => crate::modules::console_class_shared_vm::invoke,
        method "consoleConstructor" (1) => crate::modules::console_class_shared_vm::constructor,
        method "consoleNoop" (0) => crate::modules::console_class_shared_vm::noop,
        method "consoleContext" (1) => crate::modules::console_class_shared_vm::context,
        method "consoleCreateTask" (0) => crate::modules::console_class_shared_vm::create_task,
        method "consoleTaskRun" (1) => crate::modules::console_class_shared_vm::task_run,
        method "consoleInspect" (2) => crate::modules::console_format_shared_vm::inspect_operation,
        method "processWarningListenerInitialize" (0) => crate::modules::process_shared_vm::initialize_warning_listener,
        method "processEmitWarning" (4) => crate::modules::process_warning_shared_vm::emit,
        method "processEmitWarningEvent" (0) => crate::modules::process_warning_shared_vm::emit_event,
        method "processOnWarning" (1) => crate::modules::process_warning_shared_vm::on,
        method "consoleWarningWriter" (0) => crate::modules::console_shared_vm::warning_writer,
        method "processMethodsInitialize" (0) => crate::modules::process_metrics_shared_vm::initialize_methods,
        method "processEmitWarningInitialize" (0) => crate::modules::process_shared_vm::initialize_emit_warning,
        method "processEnvInitialize" (0) => crate::modules::process_shared_vm::initialize_env,
        method "asyncHookInit" (7) => crate::modules::async_hook_registry_shared_vm::initialize_hook,
        method "asyncHookEnable" (1) => crate::modules::async_hook_registry_shared_vm::enable,
        method "asyncHookDisable" (1) => crate::modules::async_hook_registry_shared_vm::disable,
        method "asyncHooksEnabled" (0) => crate::modules::async_hook_registry_shared_vm::enabled,
        method "asyncExecutionId" (0) => crate::modules::async_hooks_shared_vm::execution_id,
        method "asyncTriggerId" (0) => crate::modules::async_hooks_shared_vm::trigger_id,
        method "asyncExecutionResource" (0) => crate::modules::async_hooks_shared_vm::execution_resource,
        method "asyncQueueDestroyAsyncId" (1) => crate::modules::async_hooks_shared_vm::queue_destroy_async_id,
        method "asyncSetPromiseHooks" (1) => crate::modules::promise_hooks_shared_vm::set_registry,
        method "asyncDefaultTriggerScope" (2) => crate::modules::async_hooks_shared_vm::default_trigger_scope,
        method "asyncGetDefaultTrigger" (0) => crate::modules::async_hooks_shared_vm::get_default_trigger,
        method "asyncExternalInit" (3) => crate::modules::async_hooks_shared_vm::external_init,
        method "asyncPromisePrune" (0) => crate::modules::async_hooks_shared_vm::prune_promises,
        method "asyncTickInvoke" (0) => crate::modules::shared_event_loop::invoke_tick,
        method "asyncHookDispatch" (2) => crate::modules::async_hook_registry_shared_vm::dispatch,
        method "asyncDestroyDispatch" (0) => crate::modules::async_hooks_shared_vm::destroy_dispatch,
        method "emitterAsyncConstructor" (0) => crate::modules::emitter_async_resource_shared_vm::constructor,
        method "emitterAsyncResourceConstructor" (0) => crate::modules::emitter_async_resource_shared_vm::initialize,
        method "emitterAsyncReferencingConstructor" (3) => crate::modules::emitter_async_resource_shared_vm::initialize_referencing,
        method "emitterAsyncEmit" (1) => crate::modules::emitter_async_resource_shared_vm::emit,
        method "emitterAsyncDestroy" (0) => crate::modules::emitter_async_resource_shared_vm::destroy,
        method "emitterAsyncId" (0) => crate::modules::emitter_async_resource_shared_vm::async_id,
        method "emitterAsyncTriggerId" (0) => crate::modules::emitter_async_resource_shared_vm::trigger_async_id,
        method "emitterAsyncResource" (0) => crate::modules::emitter_async_resource_shared_vm::async_resource,
        method "emitterAsyncEventEmitter" (0) => crate::modules::emitter_async_resource_shared_vm::event_emitter,
        method "nodeErrorToString" (0) => crate::modules::node_errors_shared_vm::to_string,
        method "nodeErrorBrand" (0) => crate::modules::node_errors_shared_vm::brand,
        method "utilInspectQuote" (1) => crate::modules::util_shared_vm::inspect_quote,
        method "styleText" (2) => crate::modules::util_style_shared_vm::style_text,
        method "execFileEvent" (2) => crate::modules::child_process_exec_shared_vm::event,
        method "childProcessShellArgsWarning" (1) => crate::modules::child_process_shared_vm::shell_args_warning,
        method "childProcessForkChildProjection" (0) => crate::modules::child_process_shared_vm::fork_child_projection,
        method "childProcessForkPlan" (3) => crate::modules::child_process_fork_shared_vm::plan,
        method "ipcSend" (4) => crate::modules::child_process_ipc_shared_vm::send,
        method "ipcDisconnect" (0) => crate::modules::child_process_ipc_shared_vm::disconnect,
        method "ipcDescriptor" (0) => crate::modules::child_process_ipc_shared_vm::descriptor,
        method "ipcReference" (0) => crate::modules::child_process_ipc_shared_vm::reference,
        method "ipcReferenceCounted" (0) => crate::modules::child_process_ipc_shared_vm::reference_counted,
        method "ipcComplete" (2) => crate::modules::child_process_ipc_shared_vm::complete,
        method "ipcCloseHandle" (1) => crate::modules::child_process_ipc_shared_vm::close_transferred_handle,
        method "ipcDecode" (4) => crate::modules::child_process_ipc_shared_vm::decode,
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
        method "childProcessReadExtra" (2) => crate::modules::child_process_shared_vm::request_extra_read,
        method "childProcessRegister" (5) => crate::modules::child_process_shared_vm::register_child,
        method "childProcessStreamClosed" (0) => crate::modules::child_process_shared_vm::stream_closed,
        method "childProcessWrite" (3) => crate::modules::child_process_shared_vm::write_child,
        method "childProcessWriteExtra" (4) => crate::modules::child_process_shared_vm::write_extra_child,
        method "childProcessInputComplete" (2) => crate::modules::child_process_input_shared_vm::complete,
        method "childProcessEnd" (2) => crate::modules::child_process_shared_vm::end_child,
        method "childProcessEndExtra" (3) => crate::modules::child_process_shared_vm::end_extra_child,
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
        #[cfg(unix)]
        method "osAvailableParallelism" (0) => crate::modules::os_binding_shared_vm::available_parallelism,
        #[cfg(unix)]
        method "osBindingCpus" (0) => crate::modules::os_binding_shared_vm::cpus,
        #[cfg(unix)]
        method "osBindingHome" (0) => crate::modules::os_binding_shared_vm::home,
        #[cfg(unix)]
        method "osBindingHostname" (0) => crate::modules::os_binding_shared_vm::hostname,
        #[cfg(unix)]
        method "osBindingInterfaces" (0) => crate::modules::os_binding_shared_vm::interfaces,
        #[cfg(unix)]
        method "osBindingLoadavg" (0) => crate::modules::os_binding_shared_vm::loadavg,
        #[cfg(unix)]
        method "osBindingGetPriority" (0) => crate::modules::os_binding_shared_vm::get_priority,
        #[cfg(unix)]
        method "osBindingInformation" (0) => crate::modules::os_binding_shared_vm::information,
        #[cfg(unix)]
        method "osBindingUserInfo" (0) => crate::modules::os_binding_shared_vm::user_info,
        #[cfg(unix)]
        method "osBindingSetPriority" (0) => crate::modules::os_binding_shared_vm::set_priority,
        #[cfg(unix)]
        method "osBindingTempdir" (0) => crate::modules::os_binding_shared_vm::tempdir,
        #[cfg(unix)]
        method "osBindingUptime" (0) => crate::modules::os_binding_shared_vm::uptime,
        method "osString" (0) => crate::modules::os_shared_vm::string_operation,
        method "osFreemem" (0) => crate::modules::os_shared_vm::freemem_operation,
        method "osTotalmem" (0) => crate::modules::os_shared_vm::totalmem_operation,
        method "osUptime" (0) => crate::modules::os_shared_vm::uptime_operation,
        method "osNetworkInterfaces" (0) => crate::modules::os_shared_vm::network_interfaces_operation,
        method "osFact" (0) => crate::modules::os_shared_vm::fact,
        method "osGetPriority" (1) => crate::modules::os_shared_vm::get_priority,
        method "osSetPriority" (2) => crate::modules::os_shared_vm::set_priority,
        method "osUserInfo" (1) => crate::modules::os_shared_vm::user_info,
        method "systemErrorField" (0) => crate::modules::util_system_error_shared_vm::system_error_field,
        method "systemErrorConstructor" (0) => crate::modules::util_system_error_shared_vm::system_error_constructor,
        method "moduleConstruct" (0) => crate::modules::module_shared_vm::construct,
        method "moduleConstructorGet" (0) => crate::modules::module_shared_vm::constructor_get,
        method "moduleParentGet" (0) => crate::modules::module_shared_vm::parent_get,
        method "moduleParentSet" (1) => crate::modules::module_shared_vm::parent_set,
        method "moduleRequire" (1) => crate::modules::module_shared_vm::require,
        method "moduleCompile" (3) => crate::modules::module_shared_vm::compile,
        method "moduleRunMain" (1) => commonjs::module_run_main,
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
        method "netHandleHasPendingRead" (0) => crate::modules::net_reference_shared_vm::has_pending_read,
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
        method "nativeAsyncWrapReset" (1) => crate::modules::native_async_wrap::async_reset,
        method "nativePipeConstruct" (1) => crate::modules::pipe_binding::construct,
        method "nativePipeOpen" (1) => crate::modules::pipe_binding::open,
        method "nativePipeBind" (1) => crate::modules::pipe_binding::bind,
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
        method "netHandleSetTypeOfService" (0) => crate::modules::net_reference_shared_vm::set_type_of_service,
        method "netHandleGetTypeOfService" (0) => crate::modules::net_reference_shared_vm::get_type_of_service,
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
        method "eventsListenerCount" (2) => crate::modules::events_shared_vm::listener_count,
        method "eventsGetEventListeners" (2) => crate::modules::events_shared_vm::get_event_listeners,
        method "eventsAddAbortListener" (2) => crate::modules::events_shared_vm::add_abort_listener,
        method "abortListenerDispose" (0) => crate::modules::events_shared_vm::abort_listener_dispose,
        method "abortedListenerJob" (0) => crate::modules::events_shared_vm::aborted_listener_job,
        method "eventsOnce" (2) => crate::modules::events_once_shared_vm::once,
        method "eventsOnceResolve" (0) => crate::modules::events_once_shared_vm::once_resolve,
        method "eventsOnceError" (1) => crate::modules::events_once_shared_vm::once_error,
        method "eventsOnceAbort" (0) => crate::modules::events_once_shared_vm::once_abort,
        method "eventsOn" (2) => crate::modules::events_once_shared_vm::on,
        method "eventsOnNext" (0) => crate::modules::events_once_shared_vm::on_next,
        method "eventsOnReturn" (0) => crate::modules::events_once_shared_vm::on_return,
        method "eventsOnThrow" (1) => crate::modules::events_once_shared_vm::on_throw,
        method "eventsOnEvent" (0) => crate::modules::events_once_shared_vm::on_event,
        method "eventsOnError" (1) => crate::modules::events_once_shared_vm::on_error,
        method "eventsOnAbort" (0) => crate::modules::events_once_shared_vm::on_abort,
        method "eventsOnClose" (0) => crate::modules::events_once_shared_vm::on_close,
        method "eventConstructor" (1) => crate::modules::event_target_shared_vm::event_constructor,
        method "eventGet" (0) => crate::modules::event_target_shared_vm::event_get,
        method "eventIsTrusted" (0) => crate::modules::event_target_shared_vm::event_is_trusted,
        method "eventSetReturnValue" (1) => crate::modules::event_target_shared_vm::event_set_return_value,
        method "eventSetCancelBubble" (1) => crate::modules::event_target_shared_vm::event_set_cancel_bubble,
        method "eventPreventDefault" (0) => crate::modules::event_target_shared_vm::event_prevent_default,
        method "eventStopPropagation" (0) => crate::modules::event_target_shared_vm::event_stop_propagation,
        method "eventStopImmediatePropagation" (0) => crate::modules::event_target_shared_vm::event_stop_immediate_propagation,
        method "eventComposedPath" (0) => crate::modules::event_target_shared_vm::event_composed_path,
        method "eventInitEvent" (1) => crate::modules::event_target_shared_vm::event_init_event,
        method "eventInspect" (2) => crate::modules::event_target_shared_vm::event_inspect,
        method "customEventConstructor" (1) => crate::modules::event_target_shared_vm::custom_event_constructor,
        method "customEventDetail" (0) => crate::modules::event_target_shared_vm::custom_event_detail,
        method "eventTargetConstructor" (0) => crate::modules::event_target_shared_vm::event_target_constructor,
        method "eventTargetAddEventListener" (2) => crate::modules::event_target_shared_vm::add_event_listener,
        method "eventTargetRemoveEventListener" (2) => crate::modules::event_target_shared_vm::remove_event_listener,
        method "eventTargetDispatchEvent" (1) => crate::modules::event_target_shared_vm::dispatch_event,
        method "eventTargetSignalRemove" (0) => crate::modules::event_target_shared_vm::signal_remove,
        method "eventTargetInspect" (2) => crate::modules::event_target_shared_vm::event_target_inspect,
        method "eventTargetQueueError" (1) => crate::modules::event_target_shared_vm::queue_listener_error,
        method "eventTargetRethrow" (0) => crate::modules::event_target_shared_vm::rethrow_listener_error,
        method "abortSignalConstructor" (0) => crate::modules::abort_controller_shared_vm::signal_constructor,
        method "abortSignalAborted" (0) => crate::modules::abort_controller_shared_vm::signal_aborted,
        method "abortSignalReason" (0) => crate::modules::abort_controller_shared_vm::signal_reason,
        method "abortSignalThrowIfAborted" (0) => crate::modules::abort_controller_shared_vm::signal_throw_if_aborted,
        method "abortSignalGetOnabort" (0) => crate::modules::abort_controller_shared_vm::signal_get_onabort,
        method "abortSignalSetOnabort" (1) => crate::modules::abort_controller_shared_vm::signal_set_onabort,
        method "eventHandlerInvoke" (0) => crate::modules::abort_controller_shared_vm::event_handler_invoke,
        method "abortSignalStaticAbort" (0) => crate::modules::abort_controller_shared_vm::static_abort,
        method "abortSignalStaticTimeout" (1) => crate::modules::abort_controller_shared_vm::static_timeout,
        method "abortSignalTimeoutFire" (0) => crate::modules::abort_controller_shared_vm::timeout_fire,
        method "abortSignalStaticAny" (1) => crate::modules::abort_controller_shared_vm::static_any,
        method "abortSignalInspect" (2) => crate::modules::abort_controller_shared_vm::signal_inspect,
        method "abortSignalAbort" (2) => crate::modules::abort_controller_shared_vm::abort_operation,
        method "abortControllerConstructor" (0) => crate::modules::abort_controller_shared_vm::controller_constructor,
        method "abortControllerSignal" (0) => crate::modules::abort_controller_shared_vm::controller_get_signal,
        method "abortControllerAbort" (0) => crate::modules::abort_controller_shared_vm::controller_abort,
        method "abortControllerInspect" (2) => crate::modules::abort_controller_shared_vm::controller_inspect,
        method "domExceptionConstructor" (0) => crate::modules::dom_exception_shared_vm::constructor,
        method "domExceptionGet" (0) => crate::modules::dom_exception_shared_vm::getter,
        method "eventsDefaultMaxSet" (1) => crate::modules::event_limits_shared_vm::default_set,
        method "emitterMaxGet" (1) => crate::modules::event_limits_shared_vm::emitter_get,
        method "emitterMaxSet" (2) => crate::modules::event_limits_shared_vm::emitter_set,
        method "eventsMaxGet" (1) => crate::modules::event_limits_shared_vm::global_get,
        method "eventsMaxSet" (1) => crate::modules::event_limits_shared_vm::global_set,
        method "emitterEventNames" (1) => crate::modules::event_emitter_shared_vm::names,
        method "emitterListeners" (2) => crate::modules::event_emitter_shared_vm::listeners,
        method "emitterRawListeners" (2) => crate::modules::event_emitter_shared_vm::raw_listeners,
        method "emitterListenerCount" (3) => crate::modules::event_emitter_shared_vm::count,
        method "abortSignalBrand" (1) => crate::modules::events_shared_vm::abort_signal_brand,
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
        method "httpRequestCreate" (4) => crate::modules::http_shared_vm::request_create,
        method "httpRequestNormalizeUrl" (1) => crate::modules::http_shared_vm::request_normalize_url,
        method "httpRequestWrite" (3) => crate::modules::http_shared_vm::request_write,
        method "httpRequestSetHeader" (2) => crate::modules::http_shared_vm::request_set_header,
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
        method "httpResponseWriteHead" (7) => crate::modules::http_shared_vm::response_write_head,
        method "httpResponseInitialize" (2) => crate::modules::http_shared_vm::response_initialize,
        method "httpResponseSend" (4) => crate::modules::http_shared_vm::response_send,
        method "assertCall" (0) => crate::modules::assert_shared_vm::call,
        method "assertAssertionError" (1) => crate::modules::assert_shared_vm::error_constructor,
        method "assertErrorToString" (0) => crate::modules::assert_shared_vm::error_to_string,
        method "assertSettled" (1) => crate::modules::assert_shared_vm::settled,
        method "assertRejected" (1) => crate::modules::assert_shared_vm::rejected,
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
        method "cryptoArgon2JobNew" (10) => crate::modules::crypto_secret_job_shared_vm::create_argon2,
        #[cfg(quench_openssl_provider)]
        method "cryptoHashJobNew" (3) => crate::modules::crypto_secret_job_shared_vm::create_hash,
        #[cfg(quench_openssl_provider)]
        method "cryptoAesJobNew" (8) => crate::modules::crypto_secret_job_shared_vm::create_aes,
        #[cfg(quench_openssl_provider)]
        method "cryptoChaChaJobNew" (6) => crate::modules::crypto_secret_job_shared_vm::create_chacha,
        #[cfg(quench_openssl_provider)]
        method "cryptoRsaCipherJobNew" (7) => crate::modules::crypto_secret_job_shared_vm::create_rsa_cipher,
        #[cfg(quench_openssl_provider)]
        method "cryptoDhBitsJobNew" (11) => crate::modules::crypto_secret_job_shared_vm::create_dh_bits,
        #[cfg(quench_openssl_provider)]
        method "cryptoKmacJobNew" (8) => crate::modules::crypto_secret_job_shared_vm::create_kmac,
        #[cfg(quench_openssl_provider)]
        method "cryptoKemEncapsulateJobNew" (6) => crate::modules::crypto_secret_job_shared_vm::create_kem_encapsulate,
        #[cfg(quench_openssl_provider)]
        method "cryptoKemDecapsulateJobNew" (7) => crate::modules::crypto_secret_job_shared_vm::create_kem_decapsulate,
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
        method "cryptoHmacJobNew" (6) => crate::modules::crypto_secret_job_shared_vm::create_hmac,
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
        method "cryptoDiffieHellman" (2) => crate::modules::crypto_asymmetric_shared_vm::diffie_hellman,
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
        method "cliOptionsInfo" (0) => crate::modules::cli_options_shared_vm::info,
        method "cliOptionsFlags" (0) => crate::modules::cli_options_shared_vm::flags,
        method "cliOptionsValues" (0) => crate::modules::cli_options_shared_vm::values,
        method "cliEmbedderOptions" (0) => crate::modules::cli_options_shared_vm::embedder_options,
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
        method "fsBindingWriteFileUtf8" (4) => crate::modules::fs_shared_vm::write_public::write_utf8_binding,
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
        method "fsLutimesSync" (3) => crate::modules::fs_shared_vm::stat::lutimes_sync,
        method "fsFutimesSync" (3) => crate::modules::fs_shared_vm::stat::futimes_sync,
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
        method "tlsSocketInfo" (3) => crate::modules::tls_shared_vm::socket::info,
        method "tlsHandleVerifyError" (0) => crate::modules::tls_shared_vm::handle::verify_error,
        method "tlsMatchName" (1) => crate::modules::tls_shared_vm::identity::match_name,
        method "tlsLowercaseName" (1) => crate::modules::tls_shared_vm::identity::lowercase_name,
        method "tlsCollectAltName" (1) => crate::modules::tls_shared_vm::identity::collect_alt_name,
        method "tlsCheckServerIdentity" (2) => crate::modules::tls_shared_vm::identity::check_server_identity,
        method "httpIncomingBody" (1) => crate::modules::http_shared_vm::incoming::body,
        method "httpServerSocketCreate" (1) => crate::modules::http_shared_vm::incoming::accepted_socket,
        method "httpServerRequestAbort" (2) => crate::modules::http_shared_vm::incoming::abort_request,
        method "httpServerSocketDestroy" (2) => crate::modules::http_shared_vm::incoming::destroy_socket,
        method "httpServerSocketDestroyById" (0) => crate::modules::http_shared_vm::incoming::destroy_socket_by_id,
        method "httpServerSocketEnd" (0) => crate::modules::http_shared_vm::incoming::end_socket_by_id,
        method "httpServerSocketResume" (0) => crate::modules::http_shared_vm::incoming::resume_socket,
        method "httpServerSocketWriteById" (1) => crate::modules::http_shared_vm::incoming::write_socket_by_id,
        method "objectTemplateCapture" (3) => crate::modules::http_shared_vm::incoming::capture_template,
        method "objectTemplateCopy" (1) => crate::modules::http_shared_vm::incoming::copy_template,
        method "byteStringEnd" (1) => crate::modules::webidl_shared_vm::byte_string_end,
        method "headerFields" (2) => crate::modules::webidl_shared_vm::header_fields,
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
        method "tlsGetCACertificates" (1) => crate::modules::tls_shared_vm::discovery::ca_certificates,
        method "tlsContextCreate" (1) => crate::modules::tls_shared_vm::facade::create,
        method "tlsContextConfigure" (0) => crate::modules::tls_shared_vm::configuration::method,
        method "tlsContextRegister" (0) => crate::modules::tls_shared_vm::contexts::register,
        method "tlsContextInit" (0) => crate::modules::tls_shared_vm::contexts::initialize,
        method "tlsContextSetOptions" (0) => crate::modules::tls_shared_vm::contexts::set_options,
        method "tlsContextGetMinProto" (0) => crate::modules::tls_shared_vm::contexts::minimum,
        method "tlsContextGetMaxProto" (0) => crate::modules::tls_shared_vm::contexts::maximum,
        method "cryptoSetEngine" (2) => crate::modules::crypto_engine_shared_vm::set_engine,
        #[cfg(quench_openssl_provider)]
        method "get crypto" (0) => crate::modules::crypto_node::webcrypto_getter,
        #[cfg(quench_openssl_provider)]
        method "cryptoCheckPrime" (1) => crate::modules::crypto_node::random::check_prime,
        #[cfg(quench_openssl_provider)]
        method "cryptoCheckPrimeSync" (1) => crate::modules::crypto_node::random::check_prime_sync,
        #[cfg(quench_openssl_provider)]
        method "cryptoCreateCipheriv" (4) => crate::modules::crypto_node::cipher::create_cipheriv,
        #[cfg(quench_openssl_provider)]
        method "cryptoCreateDecipheriv" (4) => crate::modules::crypto_node::cipher::create_decipheriv,
        #[cfg(quench_openssl_provider)]
        method "cryptoCreateDiffieHellman" (4) => crate::modules::crypto_node::dh::create_diffie_hellman,
        #[cfg(quench_openssl_provider)]
        method "cryptoCreateDiffieHellmanGroup" (1) => crate::modules::crypto_node::dh::create_diffie_hellman_group,
        #[cfg(quench_openssl_provider)]
        method "cryptoCreateEcdh" (1) => crate::modules::crypto_node::dh::create_ecdh,
        #[cfg(quench_openssl_provider)]
        method "cryptoCreateHash" (2) => crate::modules::crypto_node::hash::create_hash,
        #[cfg(quench_openssl_provider)]
        method "cryptoCreateHmac" (3) => crate::modules::crypto_node::hash::create_hmac,
        #[cfg(quench_openssl_provider)]
        method "cryptoCreatePrivateKey" (1) => crate::modules::crypto_node::keys::create_private_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoCreatePublicKey" (1) => crate::modules::crypto_node::keys::create_public_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoCreateSecretKey" (2) => crate::modules::crypto_node::keys::create_secret_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoCreateSign" (2) => crate::modules::crypto_node::sign::create_sign,
        #[cfg(quench_openssl_provider)]
        method "cryptoCreateVerify" (2) => crate::modules::crypto_node::sign::create_verify,
        #[cfg(quench_openssl_provider)]
        method "cryptoDiffieHellmanStateless" (1) => crate::modules::crypto_node::dh::stateless,
        #[cfg(quench_openssl_provider)]
        method "cryptoGeneratePrime" (3) => crate::modules::crypto_node::random::generate_prime,
        #[cfg(quench_openssl_provider)]
        method "cryptoGeneratePrimeSync" (1) => crate::modules::crypto_node::random::generate_prime_sync,
        #[cfg(quench_openssl_provider)]
        method "cryptoGetCiphers" (0) => crate::modules::crypto_node::cipher::get_ciphers,
        #[cfg(quench_openssl_provider)]
        method "cryptoGetCipherInfo" (2) => crate::modules::crypto_node::cipher::get_cipher_info,
        #[cfg(quench_openssl_provider)]
        method "cryptoGetCurves" (0) => crate::modules::crypto_node::dh::get_curves,
        #[cfg(quench_openssl_provider)]
        method "cryptoGetHashes" (0) => crate::modules::crypto_node::hash::get_hashes,
        #[cfg(quench_openssl_provider)]
        method "cryptoHkdfAsync" (6) => crate::modules::crypto_node::kdf::hkdf,
        #[cfg(quench_openssl_provider)]
        method "cryptoHkdfSync" (5) => crate::modules::crypto_node::kdf::hkdf_sync,
        #[cfg(quench_openssl_provider)]
        method "cryptoPbkdf2Async" (6) => crate::modules::crypto_node::kdf::pbkdf2,
        #[cfg(quench_openssl_provider)]
        method "cryptoPbkdf2Sync" (5) => crate::modules::crypto_node::kdf::pbkdf2_sync,
        #[cfg(quench_openssl_provider)]
        method "cryptoGenerateKeyPair" (3) => crate::modules::crypto_node::keys::generate_key_pair,
        #[cfg(quench_openssl_provider)]
        method "cryptoGenerateKeyPairSync" (2) => crate::modules::crypto_node::keys::generate_key_pair_sync,
        #[cfg(quench_openssl_provider)]
        method "cryptoGenerateKey" (3) => crate::modules::crypto_node::keys::generate_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoGenerateKeySync" (2) => crate::modules::crypto_node::keys::generate_key_sync,
        #[cfg(quench_openssl_provider)]
        method "cryptoPrivateDecrypt" (2) => crate::modules::crypto_node::sign::private_decrypt,
        #[cfg(quench_openssl_provider)]
        method "cryptoPrivateEncrypt" (2) => crate::modules::crypto_node::sign::private_encrypt,
        #[cfg(quench_openssl_provider)]
        method "cryptoPublicDecrypt" (2) => crate::modules::crypto_node::sign::public_decrypt,
        #[cfg(quench_openssl_provider)]
        method "cryptoPublicEncrypt" (2) => crate::modules::crypto_node::sign::public_encrypt,
        #[cfg(quench_openssl_provider)]
        method "cryptoRandomBytesApi" (2) => crate::modules::crypto_node::random::random_bytes,
        #[cfg(quench_openssl_provider)]
        method "cryptoRandomFill" (4) => crate::modules::crypto_node::random::random_fill,
        #[cfg(quench_openssl_provider)]
        method "cryptoRandomFillSync" (1) => crate::modules::crypto_node::random::random_fill_sync,
        #[cfg(quench_openssl_provider)]
        method "cryptoRandomInt" (3) => crate::modules::crypto_node::random::random_int,
        #[cfg(quench_openssl_provider)]
        method "cryptoRandomUuid" (1) => crate::modules::crypto_node::random::random_uuid,
        #[cfg(quench_openssl_provider)]
        method "cryptoScryptAsync" (4) => crate::modules::crypto_node::kdf::scrypt,
        #[cfg(quench_openssl_provider)]
        method "cryptoScryptSync" (3) => crate::modules::crypto_node::kdf::scrypt_sync,
        #[cfg(quench_openssl_provider)]
        method "cryptoSignOneShot" (4) => crate::modules::crypto_node::sign::sign_one_shot,
        #[cfg(quench_openssl_provider)]
        method "cryptoSetEngineApi" (2) => crate::modules::crypto_node::set_engine,
        #[cfg(quench_openssl_provider)]
        method "cryptoTimingSafeEqual" (0) => crate::modules::crypto_node::random::timing_safe_equal,
        #[cfg(quench_openssl_provider)]
        method "cryptoGetFips" (0) => crate::modules::crypto_node::get_fips,
        #[cfg(quench_openssl_provider)]
        method "cryptoSetFips" (1) => crate::modules::crypto_node::set_fips,
        #[cfg(quench_openssl_provider)]
        method "cryptoVerifyOneShot" (5) => crate::modules::crypto_node::sign::verify_one_shot,
        #[cfg(quench_openssl_provider)]
        method "cryptoHashOneShot" (2) => crate::modules::crypto_node::hash::one_shot,
        #[cfg(quench_openssl_provider)]
        method "cryptoSecureHeapUsed" (0) => crate::modules::crypto_node::secure_heap_used,
        #[cfg(quench_openssl_provider)]
        method "cryptoWebcryptoGetter" (0) => crate::modules::crypto_node::webcrypto_getter,
        #[cfg(quench_openssl_provider)]
        method "cryptoSubtleGetter" (0) => crate::modules::crypto_node::subtle_getter,
        #[cfg(quench_openssl_provider)]
        method "cryptoGetRandomValuesGetter" (0) => crate::modules::crypto_node::get_random_values_getter,
        #[cfg(quench_openssl_provider)]
        method "cryptoGetRandomValuesUnbound" (1) => crate::modules::crypto_node::random::get_random_values_unbound,
        #[cfg(quench_openssl_provider)]
        method "cryptoWebClassGlobals" (0) => crate::modules::crypto_node::web_class_globals,
        #[cfg(quench_openssl_provider)]
        method "cryptoLazyTransform" (1) => crate::modules::crypto_node::hash::lazy_transform_constructor,
        #[cfg(quench_openssl_provider)]
        method "cryptoLazyTransformGet" (0) => crate::modules::crypto_node::hash::lazy_transform_get,
        #[cfg(quench_openssl_provider)]
        method "cryptoLazyTransformSet" (1) => crate::modules::crypto_node::hash::lazy_transform_set,
        #[cfg(quench_openssl_provider)]
        method "cryptoHashConstructor" (2) => crate::modules::crypto_node::hash::hash_constructor,
        #[cfg(quench_openssl_provider)]
        method "cryptoHmacConstructor" (3) => crate::modules::crypto_node::hash::hmac_constructor,
        #[cfg(quench_openssl_provider)]
        method "cryptoHashCopy" (1) => crate::modules::crypto_node::hash::copy,
        #[cfg(quench_openssl_provider)]
        method "cryptoHashTransform" (3) => crate::modules::crypto_node::hash::transform,
        #[cfg(quench_openssl_provider)]
        method "cryptoHashFlush" (1) => crate::modules::crypto_node::hash::flush,
        #[cfg(quench_openssl_provider)]
        method "cryptoHashUpdate" (2) => crate::modules::crypto_node::hash::update,
        #[cfg(quench_openssl_provider)]
        method "cryptoHashDigest" (1) => crate::modules::crypto_node::hash::digest,
        #[cfg(quench_openssl_provider)]
        method "cryptoCipheriv" (4) => crate::modules::crypto_node::cipher::cipheriv,
        #[cfg(quench_openssl_provider)]
        method "cryptoDecipheriv" (4) => crate::modules::crypto_node::cipher::decipheriv,
        #[cfg(quench_openssl_provider)]
        method "cryptoCipherTransform" (3) => crate::modules::crypto_node::cipher::transform,
        #[cfg(quench_openssl_provider)]
        method "cryptoCipherFlush" (1) => crate::modules::crypto_node::cipher::flush,
        #[cfg(quench_openssl_provider)]
        method "cryptoCipherUpdate" (3) => crate::modules::crypto_node::cipher::update,
        #[cfg(quench_openssl_provider)]
        method "cryptoCipherFinal" (1) => crate::modules::crypto_node::cipher::final_,
        #[cfg(quench_openssl_provider)]
        method "cryptoCipherSetAutoPadding" (1) => crate::modules::crypto_node::cipher::set_auto_padding,
        #[cfg(quench_openssl_provider)]
        method "cryptoCipherGetAuthTag" (0) => crate::modules::crypto_node::cipher::get_auth_tag,
        #[cfg(quench_openssl_provider)]
        method "cryptoCipherSetAuthTag" (2) => crate::modules::crypto_node::cipher::set_auth_tag,
        #[cfg(quench_openssl_provider)]
        method "cryptoCipherSetAad" (2) => crate::modules::crypto_node::cipher::set_aad,
        #[cfg(quench_openssl_provider)]
        method "cryptoSignConstructor" (2) => crate::modules::crypto_node::sign::sign_constructor,
        #[cfg(quench_openssl_provider)]
        method "cryptoVerifyConstructor" (2) => crate::modules::crypto_node::sign::verify_constructor,
        #[cfg(quench_openssl_provider)]
        method "cryptoSignWrite" (3) => crate::modules::crypto_node::sign::write,
        #[cfg(quench_openssl_provider)]
        method "cryptoSignUpdate" (2) => crate::modules::crypto_node::sign::update,
        #[cfg(quench_openssl_provider)]
        method "cryptoSignSign" (2) => crate::modules::crypto_node::sign::sign,
        #[cfg(quench_openssl_provider)]
        method "cryptoVerifyVerify" (3) => crate::modules::crypto_node::sign::verify,
        #[cfg(quench_openssl_provider)]
        method "cryptoKeyObjectConstructor" (2) => crate::modules::crypto_node::keys::constructor,
        #[cfg(quench_openssl_provider)]
        method "cryptoKeyObjectType" (0) => crate::modules::crypto_node::keys::get_type,
        #[cfg(quench_openssl_provider)]
        method "cryptoKeyObjectEquals" (1) => crate::modules::crypto_node::keys::equals,
        #[cfg(quench_openssl_provider)]
        method "cryptoKeyObjectFrom" (1) => crate::modules::crypto_node::keys::from,
        #[cfg(quench_openssl_provider)]
        method "cryptoKeyObjectSymmetricKeySize" (0) => crate::modules::crypto_node::keys::symmetric_key_size,
        #[cfg(quench_openssl_provider)]
        method "cryptoKeyObjectExportSecret" (1) => crate::modules::crypto_node::keys::export_secret,
        #[cfg(quench_openssl_provider)]
        method "cryptoKeyObjectExportAsymmetric" (1) => crate::modules::crypto_node::keys::export_asymmetric,
        #[cfg(quench_openssl_provider)]
        method "cryptoKeyObjectToCryptoKey" (3) => crate::modules::crypto_node::keys::to_crypto_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoKeyObjectAsymmetricKeyType" (0) => crate::modules::crypto_node::keys::asymmetric_key_type,
        #[cfg(quench_openssl_provider)]
        method "cryptoKeyObjectAsymmetricKeyDetails" (0) => crate::modules::crypto_node::keys::asymmetric_key_details,
        #[cfg(quench_openssl_provider)]
        method "cryptoEcdhConstructor" (1) => crate::modules::crypto_node::dh::ecdh_constructor,
        #[cfg(quench_openssl_provider)]
        method "cryptoEcdhComputeSecret" (3) => crate::modules::crypto_node::dh::ecdh_compute_secret,
        #[cfg(quench_openssl_provider)]
        method "cryptoEcdhSetPrivateKey" (2) => crate::modules::crypto_node::dh::ecdh_set_private_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoEcdhSetPublicKey" (2) => crate::modules::crypto_node::dh::ecdh_set_public_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoEcdhGetPrivateKey" (1) => crate::modules::crypto_node::dh::ecdh_get_private_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoEcdhGenerateKeys" (2) => crate::modules::crypto_node::dh::ecdh_generate_keys,
        #[cfg(quench_openssl_provider)]
        method "cryptoEcdhGetPublicKey" (2) => crate::modules::crypto_node::dh::ecdh_get_public_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoEcdhConvertKey" (5) => crate::modules::crypto_node::dh::ecdh_convert_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoDiffieHellmanConstructor" (4) => crate::modules::crypto_node::dh::diffie_hellman_constructor,
        #[cfg(quench_openssl_provider)]
        method "cryptoDiffieHellmanGroupConstructor" (1) => crate::modules::crypto_node::dh::diffie_hellman_group_constructor,
        #[cfg(quench_openssl_provider)]
        method "cryptoDhGenerateKeys" (1) => crate::modules::crypto_node::dh::dh_generate_keys,
        #[cfg(quench_openssl_provider)]
        method "cryptoDhComputeSecret" (3) => crate::modules::crypto_node::dh::dh_compute_secret,
        #[cfg(quench_openssl_provider)]
        method "cryptoDhGetPrime" (1) => crate::modules::crypto_node::dh::dh_get_prime,
        #[cfg(quench_openssl_provider)]
        method "cryptoDhGetGenerator" (1) => crate::modules::crypto_node::dh::dh_get_generator,
        #[cfg(quench_openssl_provider)]
        method "cryptoDhGetPublicKey" (1) => crate::modules::crypto_node::dh::dh_get_public_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoDhGetPrivateKey" (1) => crate::modules::crypto_node::dh::dh_get_private_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoDhSetPublicKey" (2) => crate::modules::crypto_node::dh::dh_set_public_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoDhSetPrivateKey" (2) => crate::modules::crypto_node::dh::dh_set_private_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoX509Constructor" (1) => crate::modules::crypto_node::x509::constructor,
        #[cfg(quench_openssl_provider)]
        method "cryptoX509Field" (0) => crate::modules::crypto_node::x509::field,
        #[cfg(quench_openssl_provider)]
        method "cryptoX509ToString" (0) => crate::modules::crypto_node::x509::to_string,
        #[cfg(quench_openssl_provider)]
        method "cryptoX509CheckHostApi" (2) => crate::modules::crypto_node::x509::check_host,
        #[cfg(quench_openssl_provider)]
        method "cryptoX509CheckEmail" (2) => crate::modules::crypto_node::x509::check_email,
        #[cfg(quench_openssl_provider)]
        method "cryptoX509CheckIp" (2) => crate::modules::crypto_node::x509::check_ip,
        #[cfg(quench_openssl_provider)]
        method "cryptoX509CheckIssued" (1) => crate::modules::crypto_node::x509::check_issued,
        #[cfg(quench_openssl_provider)]
        method "cryptoX509CheckPrivateKey" (1) => crate::modules::crypto_node::x509::check_private_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoX509Verify" (1) => crate::modules::crypto_node::x509::verify,
        #[cfg(quench_openssl_provider)]
        method "cryptoX509ToLegacyObject" (0) => crate::modules::crypto_node::x509::to_legacy_object,
        #[cfg(quench_openssl_provider)]
        method "cryptoCertificate" (0) => crate::modules::crypto_node::x509::certificate_constructor,
        #[cfg(quench_openssl_provider)]
        method "cryptoSpkacVerify" (2) => crate::modules::crypto_node::x509::verify_spkac,
        #[cfg(quench_openssl_provider)]
        method "cryptoSpkacExportPublicKey" (2) => crate::modules::crypto_node::x509::spkac_public_key_pem,
        #[cfg(quench_openssl_provider)]
        method "cryptoSpkacExportChallenge" (2) => crate::modules::crypto_node::x509::spkac_challenge,
        #[cfg(quench_openssl_provider)]
        method "cryptoWebIllegalConstructor" (0) => crate::modules::crypto_node::web::illegal_constructor,
        #[cfg(quench_openssl_provider)]
        method "cryptoWebSubtle" (0) => crate::modules::crypto_node::web::subtle,
        #[cfg(quench_openssl_provider)]
        method "cryptoWebGetRandomValues" (1) => crate::modules::crypto_node::web::get_random_values,
        #[cfg(quench_openssl_provider)]
        method "cryptoWebRandomUuid" (0) => crate::modules::crypto_node::web::random_uuid,
        #[cfg(quench_openssl_provider)]
        method "cryptoKeyType" (0) => crate::modules::crypto_node::web::key_type,
        #[cfg(quench_openssl_provider)]
        method "cryptoKeyExtractable" (0) => crate::modules::crypto_node::web::key_extractable,
        #[cfg(quench_openssl_provider)]
        method "cryptoKeyAlgorithm" (0) => crate::modules::crypto_node::web::key_algorithm,
        #[cfg(quench_openssl_provider)]
        method "cryptoKeyUsages" (0) => crate::modules::crypto_node::web::key_usages,
        #[cfg(quench_openssl_provider)]
        method "cryptoKeyInspect" (2) => crate::modules::crypto_node::web::key_inspect,
        #[cfg(quench_openssl_provider)]
        method "cryptoSubtleEncrypt" (3) => crate::modules::crypto_node::web::subtle_encrypt,
        #[cfg(quench_openssl_provider)]
        method "cryptoSubtleDecrypt" (3) => crate::modules::crypto_node::web::subtle_decrypt,
        #[cfg(quench_openssl_provider)]
        method "cryptoSubtleSign" (3) => crate::modules::crypto_node::web::subtle_sign,
        #[cfg(quench_openssl_provider)]
        method "cryptoSubtleVerify" (4) => crate::modules::crypto_node::web::subtle_verify,
        #[cfg(quench_openssl_provider)]
        method "cryptoSubtleDigest" (2) => crate::modules::crypto_node::web::subtle_digest,
        #[cfg(quench_openssl_provider)]
        method "cryptoSubtleGenerateKey" (3) => crate::modules::crypto_node::web::subtle_generate_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoSubtleDeriveKey" (5) => crate::modules::crypto_node::web::subtle_derive_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoSubtleDeriveBits" (2) => crate::modules::crypto_node::web::subtle_derive_bits,
        #[cfg(quench_openssl_provider)]
        method "cryptoSubtleImportKey" (5) => crate::modules::crypto_node::web::subtle_import_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoSubtleExportKey" (2) => crate::modules::crypto_node::web::subtle_export_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoSubtleWrapKey" (4) => crate::modules::crypto_node::web::subtle_wrap_key,
        #[cfg(quench_openssl_provider)]
        method "cryptoSubtleUnwrapKey" (7) => crate::modules::crypto_node::web::subtle_unwrap_key,
        method "asyncResourceInit" (2) => crate::modules::async_hooks_shared_vm::initialize_resource,
        method "asyncResourceEnterScope" (1) => crate::modules::async_hooks_shared_vm::enter_async_scope,
        method "asyncResourceExitScope" (0) => crate::modules::async_hooks_shared_vm::exit_async_scope,
        method "asyncResourceEmitDestroy" (0) => crate::modules::async_hooks_shared_vm::emit_destroy,
        method "asyncLocalStorageInit" (1) => crate::modules::async_hooks_shared_vm::initialize_storage,
        method "asyncLocalStorageEnterWith" (1) => crate::modules::async_hooks_shared_vm::enter_with,
        method "asyncLocalStorageGetStore" (0) => crate::modules::async_hooks_shared_vm::get_store,
        method "asyncLocalStorageDisable" (0) => crate::modules::async_hooks_shared_vm::disable_storage,
        method "asyncLocalStorageRun" (2) => crate::modules::async_hooks_shared_vm::run_storage,
        method "asyncLocalStorageConstructor" (0) => crate::modules::async_hooks_module_shared_vm::storage_constructor,
        method "asyncLocalStorageExit" (1) => crate::modules::async_hooks_module_shared_vm::storage_exit,
        method "asyncLocalStorageBind" (1) => crate::modules::async_hooks_module_shared_vm::storage_bind,
        method "asyncLocalStorageSnapshot" (0) => crate::modules::async_hooks_module_shared_vm::storage_snapshot,
        method "asyncLocalStorageSnapshotRun" (1) => crate::modules::async_hooks_module_shared_vm::snapshot_run,
        method "asyncResourceConstructor" (1) => crate::modules::async_hooks_module_shared_vm::resource_constructor,
        method "asyncResourceCollected" (1) => crate::modules::async_hooks_module_shared_vm::resource_collected,
        method "asyncResourceRunInAsyncScope" (1) => crate::modules::async_hooks_module_shared_vm::run_in_async_scope,
        method "asyncResourceDestroy" (0) => crate::modules::async_hooks_module_shared_vm::resource_destroy,
        method "asyncResourceAsyncId" (0) => crate::modules::async_hooks_module_shared_vm::resource_async_id,
        method "asyncResourceTriggerAsyncId" (0) => crate::modules::async_hooks_module_shared_vm::resource_trigger_async_id,
        method "asyncResourceBind" (1) => crate::modules::async_hooks_module_shared_vm::resource_bind,
        method "asyncResourceBound" (0) => crate::modules::async_hooks_module_shared_vm::resource_bound,
        method "asyncResourceStaticBind" (1) => crate::modules::async_hooks_module_shared_vm::resource_static_bind,
        method "asyncHookConstructor" (1) => crate::modules::async_hooks_module_shared_vm::hook_constructor,
        method "asyncCreateHook" (1) => crate::modules::async_hooks_module_shared_vm::create_hook,
        method "asyncHookEnableMethod" (0) => crate::modules::async_hooks_module_shared_vm::hook_enable,
        method "asyncHookDisableMethod" (0) => crate::modules::async_hooks_module_shared_vm::hook_disable,
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
        method "performanceUvMetricsInfo" (0) => crate::modules::performance_binding_shared_vm::uv_metrics_info,
        method "performanceLoopIdleTime" (0) => crate::modules::performance_binding_shared_vm::loop_idle_time,
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
        method "caresCanonicalizeIp" (0) => crate::modules::dns_shared_vm::canonicalize_ip,
        method "caresIpv6Buffer" (0) => crate::modules::dns_shared_vm::ipv6_buffer,
        method "caresStrerror" (0) => crate::modules::dns_shared_vm::strerror,
        method "caresChannel" (0) => crate::modules::dns_shared_vm::channel,
        method "caresChannelRelease" (0) => crate::modules::dns_shared_vm::channel_release,
        method "caresQuery" (0) => crate::modules::dns_shared_vm::query,
        method "caresGetServers" (0) => crate::modules::dns_shared_vm::get_servers,
        method "caresSetServers" (0) => crate::modules::dns_shared_vm::set_servers,
        method "caresSetLocalAddress" (0) => crate::modules::dns_shared_vm::set_local_address,
        method "caresCancel" (0) => crate::modules::dns_shared_vm::cancel,
        method "caresDrain" (0) => crate::modules::dns_shared_vm::drain,
        method "dnsAddressArray" (0) => crate::modules::cares_binding::address_array,
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
        method "urlConstruct" (1) => crate::modules::url_whatwg_shared_vm::construct,
        method "urlToString" (0) => crate::modules::url_whatwg_shared_vm::to_string,
        method "urlGet" (0) => crate::modules::url_whatwg_shared_vm::get_component,
        method "urlSet" (1) => crate::modules::url_whatwg_shared_vm::set_component,
        method "urlInspect" (2) => crate::modules::url_whatwg_shared_vm::inspect,
        method "urlParseStatic" (1) => crate::modules::url_whatwg_shared_vm::parse_static,
        method "urlCanParse" (1) => crate::modules::url_whatwg_shared_vm::can_parse,
        method "urlParamsConstruct" (0) => crate::modules::url_whatwg_shared_vm::construct_params,
        method "urlParamsSize" (0) => crate::modules::url_whatwg_shared_vm::size,
        method "urlParamsAppend" (2) => crate::modules::url_whatwg_shared_vm::append,
        method "urlParamsDelete" (1) => crate::modules::url_whatwg_shared_vm::delete,
        method "urlParamsGet" (1) => crate::modules::url_whatwg_shared_vm::get_value,
        method "urlParamsGetAll" (1) => crate::modules::url_whatwg_shared_vm::get_all,
        method "urlParamsHas" (1) => crate::modules::url_whatwg_shared_vm::has,
        method "urlParamsSet" (2) => crate::modules::url_whatwg_shared_vm::set,
        method "urlParamsSort" (0) => crate::modules::url_whatwg_shared_vm::sort,
        method "urlParamsEntries" (0) => crate::modules::url_whatwg_shared_vm::entries,
        method "urlParamsForEach" (1) => crate::modules::url_whatwg_shared_vm::for_each,
        method "urlParamsKeys" (0) => crate::modules::url_whatwg_shared_vm::keys,
        method "urlParamsValues" (0) => crate::modules::url_whatwg_shared_vm::values,
        method "urlParamsToString" (0) => crate::modules::url_whatwg_shared_vm::params_to_string,
        method "urlParamsInspect" (2) => crate::modules::url_whatwg_shared_vm::params_inspect,
        method "urlParamsIteratorNext" (0) => crate::modules::url_whatwg_shared_vm::iterator_next,
        method "urlParamsIteratorInspect" (2) => crate::modules::url_whatwg_shared_vm::iterator_inspect,
        method "urlLegacyConstruct" (0) => crate::modules::url_legacy_shared_vm::construct,
        method "urlLegacyParseMethod" (3) => crate::modules::url_legacy_shared_vm::parse_method,
        method "urlLegacyFormatMethod" (0) => crate::modules::url_legacy_shared_vm::format_method,
        method "urlLegacyResolveMethod" (1) => crate::modules::url_legacy_shared_vm::resolve_method,
        method "urlLegacyResolveObjectMethod" (1) => crate::modules::url_legacy_shared_vm::resolve_object_method,
        method "urlLegacyParseHostMethod" (0) => crate::modules::url_legacy_shared_vm::parse_host_method,
        method "urlLegacyParse" (3) => crate::modules::url_legacy_shared_vm::parse_function,
        method "urlLegacyResolve" (2) => crate::modules::url_legacy_shared_vm::resolve_function,
        method "urlLegacyResolveObject" (2) => crate::modules::url_legacy_shared_vm::resolve_object_function,
        method "urlLegacyFormat" (2) => crate::modules::url_legacy_shared_vm::format_function,
        method "urlDomainToASCII" (1) => crate::modules::url_shared_vm::domain_to_ascii,
        method "urlDomainToUnicode" (1) => crate::modules::url_shared_vm::domain_to_unicode,
        method "urlPathToFileURL" (2) => crate::modules::url_shared_vm::path_to_file_url,
        method "urlToHttpOptions" (1) => crate::modules::url_shared_vm::url_to_http_options,
        method "urlIsURL" (1) => crate::modules::url_shared_vm::is_url_operation,
        method "punycodeDecode" (1) => crate::modules::punycode_shared_vm::decode,
        method "punycodeEncode" (1) => crate::modules::punycode_shared_vm::encode,
        method "punycodeToASCII" (1) => crate::modules::punycode_shared_vm::to_ascii,
        method "punycodeToUnicode" (1) => crate::modules::punycode_shared_vm::to_unicode,
        method "punycodeUcs2Decode" (1) => crate::modules::punycode_shared_vm::ucs2_decode_operation,
        method "punycodeUcs2Encode" (1) => crate::modules::punycode_shared_vm::ucs2_encode_operation,
        method "querystringUnescapeBuffer" (2) => crate::modules::querystring_shared_vm::unescape_buffer,
        method "querystringUnescape" (2) => crate::modules::querystring_shared_vm::unescape,
        method "querystringEscape" (1) => crate::modules::querystring_shared_vm::escape,
        method "querystringStringify" (4) => crate::modules::querystring_shared_vm::stringify,
        method "querystringParse" (4) => crate::modules::querystring_shared_vm::parse,
        method "stringDecoderConstruct" (1) => crate::modules::string_decoder_shared_vm::construct,
        method "stringDecoderWrite" (1) => crate::modules::string_decoder_shared_vm::write,
        method "stringDecoderEnd" (1) => crate::modules::string_decoder_shared_vm::end,
        method "stringDecoderText" (2) => crate::modules::string_decoder_shared_vm::text,
        method "stringDecoderField" (0) => crate::modules::string_decoder_shared_vm::field,
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
        method "timerToPrimitive" (0) => crate::modules::timers_shared_vm::to_primitive,
        method "timerPrimitiveCleanup" (2) => crate::modules::timers_shared_vm::primitive_cleanup,
        method "timerDispose" (0) => crate::modules::timers_shared_vm::timer_dispose,
        method "immediateDispose" (0) => crate::modules::timers_shared_vm::immediate_dispose,
        method "timerClose" (0) => crate::modules::timers_shared_vm::timer_close,
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
        method "timersIntervalCreate" (2) => crate::modules::timers_interval_shared_vm::set_interval,
        method "timersIntervalNext" (0) => crate::modules::timers_interval_shared_vm::next,
        method "timersIntervalReturn" (1) => crate::modules::timers_interval_shared_vm::finish_with_return,
        method "timersIntervalThrow" (1) => crate::modules::timers_interval_shared_vm::finish_with_throw,
        method "timersIntervalIterator" (0) => crate::modules::timers_interval_shared_vm::iterator,
        method "timersIntervalTick" (0) => crate::modules::timers_interval_shared_vm::tick,
        method "timersIntervalAbort" (0) => crate::modules::timers_interval_shared_vm::abort,
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
        method "cjsLexerParse" (1) => crate::modules::trace_shared_vm::cjs_lexer_parse,
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

/// The file a directory's `package.json` `main` names, which Node suggests importing
/// instead of the directory (`ERR_UNSUPPORTED_DIR_IMPORT`).
fn esm_directory_import_suggestion(directory: &std::path::Path) -> Option<String> {
    let manifest = std::fs::read_to_string(directory.join("package.json")).ok()?;
    let package = serde_json::from_str::<serde_json::Value>(&manifest).ok()?;
    let main = package.get("main")?.as_str()?;
    let target = directory.join(main);
    let candidates = if target.extension().is_some() {
        vec![target]
    } else {
        ["js", "mjs", "cjs", "json"]
            .into_iter()
            .map(|extension| target.with_extension(extension))
            .collect()
    };
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| url::Url::from_file_path(candidate).ok())
        .map(|url| url.to_string())
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
    "bufferBindingAsciiSlice",
    "bufferBindingBase64Slice",
    "bufferBindingBase64urlSlice",
    "bufferBindingHexSlice",
    "bufferBindingLatin1Slice",
    "bufferBindingUcs2Slice",
    "bufferBindingUtf8Slice",
    "bufferBindingUtf8Write",
    "utilTypeCheck",
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
    "objectTemplateCopy",
    "byteStringEnd",
    "headerFields",
    // Socket, timer, decoder and listener bookkeeping a request or message makes several
    // times (ws, undici, esbuild and Fastify package censuses): none reads the frame stack
    // itself, and the guest code or errors any of them reaches take frames first
    // (`Vm::materialize_deferred_lane`).
    "netSocketAccountWrite",
    "netSocketRead",
    "netSocketWrite",
    "netSocketReference",
    "setImmediate",
    "clearTimeout",
    "textDecoderDecode",
    "encodingUtf8",
    "emitterOnce",
    "emitterAddListener",
    "emitterRemoveListener",
    "primordialsCopy",
    "childProcessReference",
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
    /// EarleyBoyer 9048-9088 -> 8536-8576, the rest unchanged). Under every policy the
    /// small-object allocator's empty pages leave too, past what its recent intervals took
    /// (`quench_alloc::release_empty_pages`): a server's collections empty pages of every
    /// phase's short-lived objects, which otherwise stayed resident beside the heap's next
    /// peak (board load, glibc production, same host, 3 runs each, 2026-10-27, final peak
    /// RSS: fastify 41.7-45.3 -> 39.4-40.8 MB, hono 43.0-49.4 -> 38.4-41.1 MB, throughput
    /// within noise). A policy that returns pages at collections (memory-first) releases at
    /// every boundary; the default paces it (`resident_pages::release_at_collection`). Every
    /// policy counts the collection for the release before output
    /// (`resident_pages::note_collection`).
    fn at_collection_boundary(&mut self, policy: quench_runtime::MemoryPolicy) {
        crate::host::resident_pages::release_at_collection(policy.returns_pages_at_collections());
        quench_alloc::release_empty_pages();
        crate::host::resident_pages::note_collection();
    }

    /// The pages the program touched leave the resident set before the code generator's join
    /// it, as compiling a module's bytecode does (`compile_wasm`): loading, linking and
    /// instantiating a WebAssembly module, or the interpreter and the builtins a JavaScript
    /// program ran. A JavaScript compile is the peak of a short numeric program (V8-v7
    /// NavierStokes `--jit` under MemoryFirst, plain x86_64 build, 2026-10-22: max RSS
    /// 10.84-10.94 -> 8.80-8.89 MB, Score within noise).
    fn before_native_compilation(&mut self) {
        crate::host::resident_pages::at_native_compilation(crate::host::resident_pages::NativeCompilation::Before);
    }

    /// The code generator's pages, and the allocator's pages its scratch memory freed, leave
    /// the resident set before the program runs on (V8-v7 Crypto `--jit` under MemoryFirst,
    /// plain x86_64 build, 2026-10-22: max RSS 6.82-6.84 -> 5.91 MB with the release before).
    fn after_native_compilation(&mut self) {
        crate::host::resident_pages::at_native_compilation(crate::host::resident_pages::NativeCompilation::After);
    }

    /// Snapshots and compile-cache entries are tracked read-only mappings
    /// (`startup_snapshot::load_or_schedule`, `CacheEntry::load`).
    fn kept_bytes_read(bytes: &[u8]) {
        crate::host::resident_pages::release_tracked_bytes(bytes);
    }

    #[cfg(feature = "native-execution")]
    fn kept_native_program(&mut self, name: &str, parts: &[&str]) -> Result<Option<quench_runtime::NativeJsProgram<Self>>, quench_runtime::NativeSourceError> {
        match self.native_program_resolver.as_mut() {
            Some(resolve) => resolve(name, parts),
            None => Ok(None),
        }
    }

    fn kept_compiled_program(&mut self, name: &str, parts: &[&str]) -> Option<quench_runtime::KeptBytes> {
        crate::host::resident_pages::release_at_module_load();
        self.compile_cache.entry(name, parts)?.load()
    }
    fn keeps_compiled_programs(&self) -> bool {
        self.compile_cache.directory().is_some()
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

    fn allows_natives_syntax(&self) -> bool {
        self.shared_state.borrow().exec_argv.iter().any(|flag| matches!(flag.as_str(), "--allow-natives-syntax" | "--allow_natives_syntax"))
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

    fn validate_wasm(&mut self, bytes: &[u8], _mode: quench_runtime::ExecutionMode) -> bool {
        quench_wasm::validate_binary(bytes)
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
        &mut self, referrer: &str, specifier: &str,
    ) -> Result<Option<quench_runtime::ModuleSource>, quench_runtime::ModuleLoadError> {
        #[cfg(feature = "native-execution")]
        if let Some(module) = self.embedded_import(referrer, specifier)? { return Ok(Some(module)); }
        let module = self.resolve_filesystem_import(referrer, specifier)?;
        #[cfg(feature = "aot")]
        if let Some(module) = &module { self.record_import(referrer, specifier, module)?; }
        Ok(module)
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

    fn import_meta_filename(&mut self, source_name: &str) -> Option<String> {
        let path = if source_name.starts_with("file:") {
            url::Url::parse(source_name).ok()?.to_file_path().ok()?
        } else if source_name.starts_with("node:") || source_name.starts_with('<') {
            return None;
        } else {
            std::path::PathBuf::from(source_name)
        };
        let absolute = if path.is_absolute() {
            path
        } else {
            std::env::current_dir().ok()?.join(path)
        };
        let canonical = std::fs::canonicalize(&absolute).unwrap_or(absolute);
        Some(canonical.to_string_lossy().into_owned())
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
        commonjs::record_bootstrap_originals(context)?;
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
    let _ = (shape_key, capture_key);
    crate::modules::event_emitter_shared_vm::install_class(context, event_data)?;
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
    let shared = context.host_mut().shared_state();
    let shared = shared.borrow();
    let expose_gc = shared
        .exec_argv
        .iter()
        .any(|flag| flag == "--expose-gc" || flag == "--expose_gc")
        || shared.workers.inherited_expose_gc;
    drop(shared);
    if expose_gc {
        // `gc()` collects at once; Node's `{ execution: 'async' }` form is not implemented.
        let gc = context.host_function(operation("gc"))?;
        let key = context.string_rooted("gc");
        let global = context.global_root()?;
        context.define_data_property_rooted(global, key, gc, true, true, false)?;
    }
    Ok(())
}

/// One export-name authority for Node's ESM facade generation and AOT SDK staging.
pub(crate) fn builtin_export_names(name: &str) -> &'static [String] {
    static EXPORTS: std::sync::OnceLock<std::collections::BTreeMap<String, Vec<String>>> = std::sync::OnceLock::new();
    EXPORTS.get_or_init(|| serde_json::from_str(include_str!("builtin_esm_exports.json"))
        .expect("generated Node builtin export names are valid JSON"))
        .get(name).map(Vec::as_slice).unwrap_or(&[])
}

impl NodeHost {
    /// Node's source/interop policy for a resolved import; frozen images use its
    /// compiler-captured result through the same host contract.
    fn resolve_filesystem_import(
        &mut self, referrer: &str, specifier: &str,
    ) -> Result<Option<quench_runtime::ModuleSource>, quench_runtime::ModuleLoadError> {
        if quench_runtime::NATIVE_SUPPORT_IS_EXECUTION_ONLY {
            return Err("native guest code required: module source resolution is unavailable".into());
        }
        let builtin_name = specifier.strip_prefix("node:").unwrap_or(specifier);
        let builtin = crate::host::shared_vm::commonjs::is_builtin_specifier(specifier);
        if !builtin && specifier.starts_with("node:") {
            return Err(esm_errors::unknown_builtin(specifier));
        }
        if let Some(error) = esm_errors::unsupported_scheme(specifier) {
            return Err(error);
        }
        if builtin {
            let exports = builtin_export_names(builtin_name);
            let require_specifier = format!("node:{builtin_name}");
            let mut source = format!(
                "const __quenchModule = globalThis[\"\\0quench:require\"]({});\nexport default __quenchModule;\n",
                serde_json::to_string(&require_specifier).map_err(|error| error.to_string())?
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
            #[cfg(feature = "aot")]
            self.record_module_bytes(&path, quench_runtime::SourceKind::Module, &bytes).map_err(|error| error.to_string())?;
            // Node's `wasm` format: the module record is a facade over the instance.
            let source = esm_wasm::facade(&name, &path, &bytes)?;
            return Ok(Some(quench_runtime::ModuleSource::text(name, source)));
        }
        let kind = if extension == "json" { quench_runtime::SourceKind::Script }
            else { commonjs::source_kind_of_real_bytes(&path, &bytes)? };
        #[cfg(feature = "aot")]
        self.record_module_bytes(&path, kind, &bytes).map_err(|error| error.to_string())?;
        let source = if extension == "json" {
            commonjs::source_text(&bytes).into_owned()
        } else if kind == quench_runtime::SourceKind::Module {
            String::from_utf8(bytes)
                .map_err(|error| format!("module {} is not UTF-8: {error}", path.display()))?
        } else {
            #[cfg(feature = "aot")]
            self.record_module_resolution(&path, &path.to_string_lossy(), &path)?;
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
        };
        // Node exposes no module's bytes: it rejects `type: "bytes"` and has source-phase
        // objects only for WebAssembly. So a CommonJS or JSON interop wrapper keeps only the
        // text it compiles, never the file it wraps.
        Ok(Some(quench_runtime::ModuleSource::text(name, source).with_typescript(
            kind == quench_runtime::SourceKind::Module && matches!(extension, "ts" | "mts"),
        )))
    }
}

/// Where an import loads from (`NodeHost::import_location`).
pub(crate) struct ImportLocation {
    path: std::path::PathBuf,
    name: String,
}

/// An import referrer's file URL, when it is one, and its path: Node resolves filesystem
/// specifiers as URLs before accessing a file.
pub(crate) fn referrer_location(
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
        // Eval and stdin scripts have synthetic names (`[eval1]`, `<stdin>`); Node resolves
        // their imports from the guest `process.cwd()`.
        let referrer_path = if referrer_url.is_none() && (referrer.starts_with('<') || referrer.starts_with('[')) {
            let cwd = self.shared_state.borrow().cwd.clone();
            cwd.query_path().map_err(|error| error.to_string())?.join(referrer)
        } else {
            referrer_path
        };
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
        if path.is_dir() {
            let mut message = format!(
                "Directory import '{}' is not supported resolving ES modules imported from {referrer}",
                path.display()
            );
            if let Some(suggestion) = esm_directory_import_suggestion(&path) {
                message.push_str(&format!("\nDid you mean to import \"{suggestion}\"?"));
            }
            return Err(quench_runtime::ModuleLoadError::new(
                quench_runtime::ModuleLoadErrorKind::Error,
                "ERR_UNSUPPORTED_DIR_IMPORT",
                message,
            ));
        }
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
