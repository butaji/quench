//! `NodeHost`'s fields in a startup snapshot (`crate::host::startup_snapshot`): each is
//! written, left to the restoring process, or required empty at the snapshot point.

use super::*;
use crate::host::startup_snapshot::{require_quiescent, HostSnap};
use quench_runtime::{SnapshotReader, SnapshotWriter};

impl NodeHost {
    pub(crate) fn write_snapshot(&self, writer: &mut SnapshotWriter<'_>) -> Result<(), String> {
        let Self {
            #[cfg(feature = "native-execution")]
            module_assets: _,
            // The installation and the process's own entry and environment.
            compile_cache: _,
            #[cfg(feature = "native-execution")]
            native_program_resolver: _,
            shared_state,
            commonjs_entry: _,
            inline_source: _,
            eval_builtin_globals: _,
            eval_source_nonempty: _,
            eval_print: _,
            eval_crypto: _,
            net_pending_writes,
            net_pending_ends,
            net_pending_destroys,
            building_snapshot: _,
            // False at the snapshot point; a restored host sets it in `Host::resume`.
            bootstrapped: _,
            // Caches of this process's filesystem, which the restoring one fills itself.
            resolved_imports: _,
        } = self;
        require_quiescent(
            net_pending_writes.is_empty() && net_pending_ends.is_empty() && net_pending_destroys.is_empty(),
            "socket writes",
        )?;
        writer.u64(crate::host::shared_vm::BOOTSTRAP_ABI);
        shared_state.borrow().write_snapshot(writer)
    }

    pub(crate) fn read_snapshot(&mut self, reader: &mut SnapshotReader<'_, '_>) -> Result<(), String> {
        if reader.u64()? != crate::host::shared_vm::BOOTSTRAP_ABI {
            return Err("Node bootstrap ABI does not match runtime".into());
        }
        self.shared_state.borrow_mut().read_snapshot(reader)
    }
}

impl SharedNodeState {
    fn write_snapshot(&self, writer: &mut SnapshotWriter<'_>) -> Result<(), String> {
        let Self {
            vm_scripts,
            async_hooks,
            native_tasks,
            dns,
            performance_native,
            scheduler,
            fs,
            fs_watch,
            tls_contexts,
            #[cfg(quench_icu)]
            icu_converters,
            fs_classes,
            fs_stat_watch,
            cwd: _,
            module_parent_api,
            active_esm_requires,
            builtin_cache,
            buffer_pool,
            commonjs_cache,
            event_targets_api,
            trace,
            util_intrinsics,
            node_error_api,
            process_argv: _,
            process_control: _,
            process_signals,
            stdin,
            process_ipc,
            node_test: _,
            cluster: _,
            // `--hmr` disables startup snapshots (`names_only_inline_source`).
            hmr: _,
            repl_resources: _,
            #[cfg(quench_openssl_provider)]
            crypto_hashes,
            #[cfg(quench_openssl_provider)]
            crypto,
            crypto_key_material,
            #[cfg(quench_openssl_provider)]
            crypto_signatures,
            crypto_secret_jobs,
            zlib_native,
            messaging,
            dns_channels,
            serdes,
            unhandled_rejection_mode: _,
            output: _,
            stdout_output: _,
            module_require_trace_active: _,
            net_auto_select_family_attempt_timeout,
            net_auto_select_family,
            process_module,
            process_capture_state,
            path_module,
            timer_handle_api,
            emitter_keys,
            tick_invocation,
            timer_duration_warnings,
            exec_argv: _,
            cli_options: _,
            report_config: _,
            initial_node_options: _,
            initial_pending_deprecation: _,
            diagnostics,
            http,
            http2,
            tcp,
            net_sockets,
            net_closing_sockets,
            net_socket_close_completed,
            udp,
            native_async_wrap,
            native_pipe,
            native_stream,
            native_tcp_listeners,
            net_servers,
            child_process_shell_args_warning_emitted,
            child_processes,
            net_socket_constructor,
            net_handle_owner_symbol,
            net_server_on_connection,
            net_handle_onclose_symbol,
            net_socket_option_keys,
            net_handle_identities,
            net_handle_close_callbacks,
            error_event_name,
            stream_intrinsics,
            stream_high_water_marks,
        } = self;
        require_quiescent(native_tasks.snapshot_quiescent(), "the native task pool")?;
        require_quiescent(scheduler.snapshot_quiescent(), "scheduled callbacks")?;
        require_quiescent(fs.snapshot_quiescent(), "open file descriptors")?;
        require_quiescent(process_signals.snapshot_quiescent(), "signal listeners")?;
        require_quiescent(active_esm_requires.is_empty(), "an ESM require")?;
        require_quiescent(
            net_sockets.is_empty()
                && net_closing_sockets.is_empty()
                && native_tcp_listeners.is_empty()
                && net_servers.is_empty()
                && child_processes.is_empty()
                && net_handle_identities.is_empty()
                && net_handle_close_callbacks.is_empty(),
            "a socket, server or child process",
        )?;
        #[cfg(quench_icu)]
        icu_converters.write_snapshot(writer)?;
        #[cfg(quench_openssl_provider)]
        {
            crypto_hashes.write_snapshot(writer)?;
            crypto.write_snapshot(writer)?;
            crypto_signatures.write_snapshot(writer)?;
        }
        dns_channels.write_snapshot(writer)?;
        vm_scripts.write_snapshot(writer)?;
        async_hooks.write_snapshot(writer)?;
        dns.write_snapshot(writer)?;
        performance_native.write_snapshot(writer)?;
        fs_watch.write_snapshot(writer)?;
        tls_contexts.write_snapshot(writer)?;
        fs_classes.write_snapshot(writer)?;
        fs_stat_watch.write_snapshot(writer)?;
        module_parent_api.write(writer)?;
        builtin_cache.write(writer)?;
        buffer_pool.write_snapshot(writer)?;
        commonjs_cache.write(writer)?;
        event_targets_api.write(writer)?;
        trace.write_snapshot(writer)?;
        util_intrinsics.write(writer)?;
        node_error_api.write(writer)?;
        stdin.write_snapshot(writer)?;
        process_ipc.write_snapshot(writer)?;
        crypto_key_material.write_snapshot(writer)?;
        crypto_secret_jobs.write_snapshot(writer)?;
        zlib_native.write_snapshot(writer)?;
        messaging.write_snapshot(writer)?;
        serdes.write_snapshot(writer)?;
        net_auto_select_family_attempt_timeout.write(writer)?;
        net_auto_select_family.write(writer)?;
        process_module.write(writer)?;
        process_capture_state.write(writer)?;
        path_module.write(writer)?;
        timer_handle_api.write(writer)?;
        emitter_keys.write(writer)?;
        tick_invocation.write(writer)?;
        stream_intrinsics.write(writer)?;
        stream_high_water_marks.write(writer)?;
        timer_duration_warnings.write(writer)?;
        diagnostics.write(writer)?;
        http.write_snapshot(writer)?;
        http2.write_snapshot(writer)?;
        tcp.write_snapshot(writer)?;
        net_socket_close_completed.write(writer)?;
        udp.write_snapshot(writer)?;
        native_async_wrap.write_snapshot(writer)?;
        native_pipe.write_snapshot(writer)?;
        native_stream.write_snapshot(writer)?;
        child_process_shell_args_warning_emitted.write(writer)?;
        for root in [
            net_socket_constructor,
            net_handle_owner_symbol,
            net_server_on_connection,
            net_handle_onclose_symbol,
            net_socket_option_keys,
            error_event_name,
        ] {
            root.write(writer)?;
        }
        Ok(())
    }

    /// Restores what `write_snapshot` wrote over this process's fresh state.
    fn read_snapshot(&mut self, reader: &mut SnapshotReader<'_, '_>) -> Result<(), String> {
        #[cfg(quench_icu)]
        self.icu_converters.read_snapshot(reader)?;
        #[cfg(quench_openssl_provider)]
        {
            self.crypto_hashes.read_snapshot(reader)?;
            self.crypto.read_snapshot(reader)?;
            self.crypto_signatures.read_snapshot(reader)?;
        }
        self.dns_channels.read_snapshot(reader)?;
        self.vm_scripts.read_snapshot(reader)?;
        self.async_hooks.read_snapshot(reader)?;
        self.dns.read_snapshot(reader)?;
        self.performance_native.read_snapshot(reader)?;
        self.fs_watch.read_snapshot(reader)?;
        self.tls_contexts.read_snapshot(reader)?;
        self.fs_classes.read_snapshot(reader)?;
        self.fs_stat_watch.read_snapshot(reader)?;
        self.module_parent_api = HostSnap::read(reader)?;
        self.builtin_cache = HostSnap::read(reader)?;
        self.buffer_pool.read_snapshot(reader)?;
        self.commonjs_cache = HostSnap::read(reader)?;
        self.event_targets_api = HostSnap::read(reader)?;
        self.trace.read_snapshot(reader)?;
        self.util_intrinsics = HostSnap::read(reader)?;
        self.node_error_api = HostSnap::read(reader)?;
        self.stdin.read_snapshot(reader)?;
        self.process_ipc.read_snapshot(reader)?;
        self.crypto_key_material.read_snapshot(reader)?;
        self.crypto_secret_jobs.read_snapshot(reader)?;
        self.zlib_native.read_snapshot(reader)?;
        self.messaging.read_snapshot(reader)?;
        self.serdes.read_snapshot(reader)?;
        self.net_auto_select_family_attempt_timeout = HostSnap::read(reader)?;
        self.net_auto_select_family = HostSnap::read(reader)?;
        self.process_module = HostSnap::read(reader)?;
        self.process_capture_state = HostSnap::read(reader)?;
        self.path_module = HostSnap::read(reader)?;
        self.timer_handle_api = HostSnap::read(reader)?;
        self.emitter_keys = HostSnap::read(reader)?;
        self.tick_invocation = HostSnap::read(reader)?;
        self.stream_intrinsics = HostSnap::read(reader)?;
        self.stream_high_water_marks = HostSnap::read(reader)?;
        self.timer_duration_warnings = HostSnap::read(reader)?;
        self.diagnostics = HostSnap::read(reader)?;
        self.http.read_snapshot(reader)?;
        self.http2.read_snapshot(reader)?;
        self.tcp.read_snapshot(reader)?;
        self.net_socket_close_completed = HostSnap::read(reader)?;
        self.udp.read_snapshot(reader)?;
        self.native_async_wrap.read_snapshot(reader)?;
        self.native_pipe.read_snapshot(reader)?;
        self.native_stream.read_snapshot(reader)?;
        self.child_process_shell_args_warning_emitted = HostSnap::read(reader)?;
        for root in [
            &mut self.net_socket_constructor,
            &mut self.net_handle_owner_symbol,
            &mut self.net_server_on_connection,
            &mut self.net_handle_onclose_symbol,
            &mut self.net_socket_option_keys,
            &mut self.error_event_name,
        ] {
            *root = HostSnap::read(reader)?;
        }
        Ok(())
    }
}

impl HostSnap for CommonJsCache {
    fn write(&self, writer: &mut SnapshotWriter<'_>) -> Result<(), String> {
        let (tag, root) = match *self {
            Self::Bootstrap(root) => (false, root),
            Self::Module(root) => (true, root),
        };
        writer.bool(tag);
        writer.root(root)
    }
    fn read(reader: &mut SnapshotReader<'_, '_>) -> Result<Self, String> {
        Ok(if reader.bool()? { Self::Module(reader.root()?) } else { Self::Bootstrap(reader.root()?) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quench_runtime::{Engine, Runtime};

    #[test]
    fn restored_module_paths_use_the_receiving_process() {
        use crate::host::startup_snapshot::SnapshotKind;
        use std::{fs, process::Command};
        const WORKER: &str = "QUENCH_TEST_MODULE_PATH_SNAPSHOT";
        const SOURCE: &str = r#"
const Module = require('node:module');
const path = require('node:path');
const home = process.env.HOME;
const expected = home ? [path.resolve(home, '.node_modules'), path.resolve(home, '.node_libraries')] : [];
expected.push(path.resolve(process.execPath, '..', '..', 'lib', 'node'));
if (JSON.stringify(Module.globalPaths) !== JSON.stringify(expected)) throw new Error('stale module paths');
const descriptor = Object.getOwnPropertyDescriptor(Module, 'globalPaths');
if (!descriptor.writable || !descriptor.enumerable || !descriptor.configurable) throw new Error('path descriptor');
if (Module !== require('module')) throw new Error('module identity');
const before = Module.globalPaths;
Module._initPaths();
if (before === Module.globalPaths || JSON.stringify(before) !== JSON.stringify(Module.globalPaths)) throw new Error('initPaths copy');
"#;
        if let Some(directory) = std::env::var_os(WORKER) {
            let directory = std::path::PathBuf::from(directory);
            let program = Engine::specialize(SOURCE, "restored-paths.js").unwrap();
            let building = std::env::var_os("QUENCH_TEST_BUILD_SNAPSHOT").is_some();
            for (name, kind) in [("bootstrap", SnapshotKind::Bootstrap), ("preloaded", SnapshotKind::Preloaded)] {
                let file = directory.join(name);
                let mut host = NodeHost::new(vec!["quench".into()]);
                if building { host.building_snapshot = Some(kind); }
                let mut runtime = Runtime::new(host);
                if building {
                    let bytes = runtime.build_snapshot(&program, |host, writer| host.write_snapshot(writer)).unwrap();
                    fs::write(file, bytes).unwrap();
                } else {
                    #[cfg(feature = "native-execution")]
                    assert!(runtime.host_mut().accepts_native_bootstrap());
                    let entry = runtime.restore_snapshot(Rc::new(fs::read(file).unwrap()), &program, |host, reader| host.read_snapshot(reader)).unwrap();
                    runtime.execute_restored(&entry).unwrap();
                }
            }
            return;
        }
        let directory = std::env::temp_dir().join(format!("quench-module-path-snapshot-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        for (building, home) in [(true, Some("/snapshot-builder-home")), (false, Some("/snapshot-receiver-home")), (false, Some("relative-home")), (false, Some("")), (false, None)] {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command.env_clear().env(WORKER, &directory).env("RUST_MIN_STACK", "35651584")
                .args(["--exact", "host::node_host::snapshot::tests::restored_module_paths_use_the_receiving_process", "--nocapture"]);
            if building { command.env("QUENCH_TEST_BUILD_SNAPSHOT", "1"); }
            if let Some(home) = home { command.env("HOME", home); }
            let output = command.output().unwrap();
            assert!(output.status.success(), "{building} {home:?}: {}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn incompatible_bootstrap_host_abi_is_rejected_before_host_state_is_read() {
        let program = Engine::specialize("", "host-abi.js").unwrap();
        let mut host = NodeHost::new(vec!["quench".into()]);
        host.building_snapshot = Some(crate::host::startup_snapshot::SnapshotKind::Bootstrap);
        let mut builder = Runtime::new(host);
        let bytes = builder.build_snapshot(&program, |_, writer| {
            writer.u64(crate::host::shared_vm::BOOTSTRAP_ABI.wrapping_add(1));
            Ok(())
        }).unwrap();
        let mut runtime = Runtime::new(NodeHost::new(vec!["quench".into()]));
        let error = runtime.restore_snapshot(Rc::new(bytes), &program, |host, reader| host.read_snapshot(reader)).unwrap_err();
        assert!(error.contains("Node bootstrap ABI does not match runtime"), "{error}");
        assert!(!runtime.host_mut().bootstrapped);
    }
}
