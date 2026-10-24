//! Node API adapters and runtime-neutral host facts.
//!
//! Shared-runtime adapters own guest-visible behavior. Neutral modules own
//! data and host state that those adapters consume.

pub(crate) mod assert_shared_vm;
pub(crate) mod napi;
pub(crate) mod worker_shared_vm;
pub(crate) mod async_hook_registry_shared_vm;
pub(crate) mod async_hooks_shared_vm;
pub(crate) mod async_hooks_state;
pub(crate) mod blob_binding_shared_vm;
pub(crate) mod buffer_binding_shared_vm;
pub(crate) mod buffer_codec;
pub(crate) mod buffer_shared_vm;
pub(crate) mod primordials_shared_vm;
pub(crate) mod cares_binding;
#[cfg(quench_node_cares)] pub(crate) mod cares_channel_shared_vm;
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
pub(crate) mod console_shared_vm;
pub(crate) mod contextify_shared_vm;
#[cfg(quench_openssl_provider)]
pub(crate) mod crypto_digest;
#[cfg(quench_openssl_provider)]
pub(crate) mod crypto_digest_error;
pub(crate) mod crypto_engine_shared_vm;
pub(crate) mod crypto_error_shared_vm;
#[cfg(not(quench_openssl_provider))]
pub(crate) mod crypto_sha1;
pub(crate) mod crypto_shared_vm;
pub(crate) mod crypto_strings;
pub(crate) mod diagnostics_channel_shared_vm;
#[cfg(not(quench_node_cares))]
pub(crate) mod dns_shared_vm;
pub(crate) mod domain_shared_vm;
pub(crate) mod domain_source;
pub(crate) mod domain_trampoline;
pub(crate) mod emitter_async_resource_shared_vm;
pub(crate) mod eval_globals_shared_vm;
pub(crate) mod event_constructor_shared_vm;
pub(crate) mod event_emitter_shared_vm;
pub(crate) mod event_limits_shared_vm;
pub(crate) mod events_abort_shared_vm;
pub(crate) mod events_shared_vm;
pub(crate) mod fetch_shared_vm;
pub(crate) mod fs_directory_shared_vm;
pub(crate) mod fs_error_codes;
pub(crate) mod fs_error_details;
pub(crate) mod fs_ops;
pub(crate) mod fs_path;
pub(crate) mod fs_policy_shared_vm;
pub(crate) mod fs_read_file_shared_vm;
pub(crate) mod fs_shared_vm;
pub(crate) mod fs_stat_watch_shared_vm;
pub(crate) mod fs_state;
pub(crate) mod fs_watch_shared_vm;
pub(crate) mod fs_write_shared_vm;
pub(crate) mod http_protocol;
pub(crate) mod http_shared_vm;
pub(crate) mod http_status_shared_vm;
#[cfg(quench_icu)]
pub(crate) mod icu_binding_shared_vm;
pub(crate) mod memory_facts;
pub(crate) mod module_shared_vm;
pub(crate) mod native_task_pool;
pub(crate) mod net_config;
pub(crate) mod net_connect_error;
pub(crate) mod net_endpoint_shared_vm;
pub(crate) mod net_lookup_shared_vm;
pub(crate) mod net_options_shared_vm;
pub(crate) mod net_reference_shared_vm;
pub(crate) mod net_server_admission;
pub(crate) mod net_server_listen;
pub(crate) mod net_shared_vm;
pub(crate) mod net_socket_address;
pub(crate) mod net_socket_options;
pub(crate) mod net_socket_read;
pub(crate) mod node_errors_shared_vm;
pub(crate) mod node_protocol_constants;
pub(crate) mod os_facts;
pub(crate) mod os_shared_vm;
pub(crate) mod path_algorithms;
pub(crate) mod path_shared_vm;
pub(crate) mod perf_hooks;
pub(crate) mod performance_binding_shared_vm;
pub(crate) mod perf_timeline_query;
pub(crate) mod process_binding_shared_vm;
#[cfg(unix)]
pub(crate) mod process_execve_shared_vm;
pub(crate) mod process_memory_shared_vm;
#[cfg(unix)]
pub(crate) mod process_metrics_shared_vm;
pub(crate) mod process_shared_vm;
pub(crate) mod process_warning_shared_vm;
mod process_exception_route;
pub(crate) mod process_fatal_exception_route;
pub(crate) mod process_signals_state;
pub(crate) mod process_state;
pub(crate) mod process_stdin_shared_vm;
pub(crate) mod readline_shared_vm;
pub(crate) mod shared_event_loop;
pub(crate) mod string_decoder_codec;
pub(crate) mod string_decoder_shared_vm;
pub(crate) mod test_shared_vm;
pub(crate) mod text_decoder_codec;
#[cfg(quench_icu)]
pub(crate) mod text_decoder_icu;
pub(crate) mod text_decoder_shared_vm;
pub(crate) mod timers_shared_vm;
pub(crate) mod tls_shared_vm;
pub(crate) mod tty_shared_vm;
pub(crate) mod tty_state;
pub(crate) mod url_binding_shared_vm;
pub(crate) mod url_shared_vm;
pub(crate) mod util_callbackify_shared_vm;
pub(crate) mod util_env;
pub(crate) mod util_parse_args_shared_vm;
pub(crate) mod util_shared_vm;
pub mod util_strip;
pub(crate) mod util_style_shared_vm;
pub(crate) mod util_system_error_shared_vm;
pub(crate) mod ipc_serdes_shared_vm;
pub(crate) mod serdes_shared_vm;
pub(crate) mod v8_shared_vm;
pub(crate) mod vm_shared_vm;
pub(crate) mod web_stream_consumers_shared_vm;
pub(crate) mod zlib_constants;

pub(crate) mod trace_shared_vm;
pub(crate) mod trace_state;




pub(crate) mod encoding_binding_shared_vm;

pub(crate) mod crypto_key_shared_vm;

#[cfg(unix)]
pub(crate) mod os_binding_shared_vm;

pub(crate) mod cli_options;
pub(crate) mod cli_options_shared_vm;

pub(crate) mod zlib_native_codec;
pub(crate) mod zlib_native_shared_vm;

pub(crate) mod native_work;

pub(crate) mod report_configuration_shared_vm;

pub(crate) mod crypto_secret_job_shared_vm;

pub(crate) mod tcp_binding;
pub(crate) mod tcp_owned;

#[cfg(quench_openssl_provider)]
pub(crate) mod crypto_provider_shared_vm;

pub(crate) mod stream_binding;

#[cfg(quench_openssl_provider)]
pub(crate) mod dom_exception_shared_vm;

pub(crate) mod events_weak_shared_vm;
pub(crate) mod util_aborted_source_shared_vm;

#[cfg(quench_openssl_provider)]
pub(crate) mod crypto_digest_cache;

#[cfg(quench_openssl_provider)]
pub(crate) mod crypto_hash_shared_vm;
#[cfg(quench_openssl_provider)]
pub(crate) mod crypto_hash_source_shared_vm;
#[cfg(quench_openssl_provider)]
pub(crate) mod crypto_hash_source;
#[cfg(quench_openssl_provider)]
pub(crate) mod crypto_digest_state;

pub(crate) mod abort_dependencies_shared_vm;

pub(crate) mod abort_timeout_shared_vm;

pub(crate) mod crypto_secret_material;
#[cfg(quench_openssl_provider)]
pub(crate) mod crypto_hkdf;

pub(crate) mod udp_transport;
pub(crate) mod udp_binding;

pub(crate) mod fd_handle_type;
pub(crate) mod native_number;

pub(crate) mod native_async_wrap;

#[cfg(quench_openssl_provider)]
mod crypto_kdf;
#[cfg(quench_openssl_provider)]
mod crypto_pbkdf2;

// `v2-cloud-node`'s Worker, unrouted since `worker_threads` is `worker_shared_vm` (decision 31);
// the Node lane ports its additions onto that module or removes this one.
#[allow(dead_code)]
pub(crate) mod worker_threads_shared_vm;
pub(crate) mod worker_owned_clone;
pub(crate) mod worker_error_packet;

pub(crate) mod unix_rights;
pub(crate) mod ipc_pipe;
pub(crate) mod pipe_binding;


pub(crate) mod crypto_constants;
pub(crate) mod crypto_key_layout;
pub(crate) mod crypto_key_material_shared_vm;
#[cfg(quench_openssl_provider)]
pub(crate) use crypto_provider_shared_vm::native as crypto_provider_facts;
#[cfg(quench_openssl_provider)]
pub(crate) mod crypto_asymmetric;
#[cfg(quench_openssl_provider)]
pub(crate) mod crypto_asymmetric_generate;
#[cfg(quench_openssl_provider)]
pub(crate) mod crypto_asymmetric_operations;
#[cfg(quench_openssl_provider)]
pub(crate) mod crypto_asymmetric_shared_vm;
#[cfg(quench_openssl_provider)]
pub(crate) mod crypto_keypair_request;
#[cfg(quench_openssl_provider)]
pub(crate) mod crypto_keypair_config;
#[cfg(quench_openssl_provider)]
pub(crate) mod crypto_signature_state;
#[cfg(quench_openssl_provider)]
pub(crate) mod crypto_signature_request;
#[cfg(quench_openssl_provider)]
pub(crate) mod crypto_signature_shared_vm;
#[cfg(quench_openssl_provider)]
pub(crate) mod crypto_rsa_shared_vm;
