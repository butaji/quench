//! Node API adapters and runtime-neutral host facts.
//!
//! Shared-runtime adapters own guest-visible behavior. Neutral modules own
//! data and host state that those adapters consume.

pub(crate) mod assert_shared_vm;
pub(crate) mod base64_shared_vm;
pub(crate) mod async_hook_registry_shared_vm;
pub(crate) mod async_hooks_shared_vm;
pub(crate) mod async_hooks_state;
pub(crate) mod buffer_codec;
pub(crate) mod buffer_shared_vm;
pub(crate) mod buffer_search_shared_vm;
pub(crate) mod buffer_bigint_shared_vm;
pub(crate) mod buffer_swap_shared_vm;
pub(crate) mod buffer_constructor_shared_vm;
pub(crate) mod buffer_from_shared_vm;
pub(crate) mod buffer_compare_shared_vm;
pub(crate) mod buffer_string_shared_vm;
pub(crate) mod child_process_env_shared_vm;
pub(crate) mod child_process_exec_shared_vm;
pub(crate) mod child_process_fork_shared_vm;
pub(crate) mod child_process_input_shared_vm;
pub(crate) mod child_process_ipc_shared_vm;
pub(crate) mod child_process_options_shared_vm;
#[cfg(unix)]
pub(crate) mod child_process_output_shared_vm;
pub(crate) mod child_process_shared_vm;
pub(crate) mod child_process_stdio_shared_vm;
#[cfg(unix)]
pub(crate) mod child_process_sync;
pub(crate) mod clone_shared_vm;
pub(crate) mod eval_globals_shared_vm;
pub(crate) mod console_shared_vm;
pub(crate) mod console_source;
pub(crate) mod crypto_engine_shared_vm;
pub(crate) mod crypto_error_shared_vm;
pub(crate) mod crypto_sha1;
pub(crate) mod crypto_shared_vm;
pub(crate) mod crypto_strings;
pub(crate) mod diagnostics_channel_shared_vm;
pub(crate) mod dns_shared_vm;
pub(crate) mod domain_shared_vm;
pub(crate) mod emitter_async_resource_shared_vm;
pub(crate) mod event_constructor_shared_vm;
pub(crate) mod event_emitter_shared_vm;
pub(crate) mod event_limits_shared_vm;
pub(crate) mod events_abort_shared_vm;
pub(crate) mod events_shared_vm;
pub(crate) mod fetch_shared_vm;
pub(crate) mod fs_error_codes;
pub(crate) mod fs_error_details;
pub(crate) mod fs_ops;
pub(crate) mod fs_path;
pub(crate) mod fs_policy_shared_vm;
pub(crate) mod fs_read_file_shared_vm;
pub(crate) mod fs_write_shared_vm;
pub(crate) mod fs_shared_vm;
pub(crate) mod fs_stat_watch_shared_vm;
pub(crate) mod fs_state;
pub(crate) mod fs_watch_shared_vm;
pub(crate) mod http_protocol;
pub(crate) mod http_shared_vm;
pub(crate) mod http_status_shared_vm;
pub(crate) mod module_shared_vm;
pub(crate) mod net_socket_options;
pub(crate) mod net_server_admission;
pub(crate) mod net_socket_read;
pub(crate) mod net_socket_address;
pub(crate) mod net_config;
pub(crate) mod net_connect_error;
pub(crate) mod net_endpoint_shared_vm;
pub(crate) mod net_lookup_shared_vm;
pub(crate) mod net_options_shared_vm;
pub(crate) mod net_reference_shared_vm;
pub(crate) mod net_shared_vm;
pub(crate) mod node_errors_shared_vm;
pub(crate) mod os_facts;
pub(crate) mod memory_facts;
pub(crate) mod process_memory_shared_vm;
pub(crate) mod os_shared_vm;
pub(crate) mod path_algorithms;
pub(crate) mod path_shared_vm;
pub(crate) mod perf_hooks;
pub(crate) mod perf_timeline_query;
pub(crate) mod process_shared_vm;
pub(crate) mod process_signals_state;
pub(crate) mod process_state;
pub(crate) mod process_stdin_shared_vm;
pub(crate) mod querystring_data;
pub(crate) mod querystring_shared_vm;
pub(crate) mod readline_shared_vm;
pub(crate) mod shared_event_loop;
pub(crate) mod string_decoder_codec;
pub(crate) mod string_decoder_shared_vm;
pub(crate) mod test_shared_vm;
pub(crate) mod text_decoder_codec;
#[cfg(quench_icu)]
pub(crate) mod text_decoder_icu;
#[cfg(quench_icu)]
pub(crate) mod icu_binding_shared_vm;
pub(crate) mod text_decoder_shared_vm;
pub(crate) mod timers_shared_vm;
pub(crate) mod tls_shared_vm;
pub(crate) mod tty_shared_vm;
pub(crate) mod tty_state;
pub(crate) mod url_legacy;
pub(crate) mod url_shared_vm;
pub(crate) mod url_whatwg_data;
pub(crate) mod util_callbackify_shared_vm;
pub(crate) mod util_extend_shared_vm;
pub(crate) mod util_env;
pub(crate) mod util_parse_args_shared_vm;
pub(crate) mod util_shared_vm;
pub mod util_strip;
pub(crate) mod util_style_shared_vm;
pub(crate) mod util_system_error_shared_vm;
pub(crate) mod v8_shared_vm;
pub(crate) mod vm_shared_vm;
pub(crate) mod web_stream_consumers_shared_vm;
pub(crate) mod zlib_constants;
pub(crate) mod zlib_options_shared_vm;
pub(crate) mod zlib_shared_vm;
pub(crate) mod zlib_stream_shared_vm;

pub(crate) mod trace_shared_vm;
pub(crate) mod trace_state;

pub(crate) mod console_timing_shared_vm;

pub(crate) mod buffer_write_shared_vm;



pub(crate) mod buffer_fill_shared_vm;

pub(crate) mod buffer_inspect_shared_vm;

pub(crate) mod encoding_binding_shared_vm;

pub(crate) mod crypto_key_shared_vm;

#[cfg(target_os = "linux")]
pub(crate) mod os_binding_shared_vm;
