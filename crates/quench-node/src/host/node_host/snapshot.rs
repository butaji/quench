//! `NodeHost`'s fields in a startup snapshot (`crate::host::startup_snapshot`): each is
//! written, left to the restoring process, or required empty at the snapshot point.

use super::*;
use crate::host::startup_snapshot::{require_quiescent, HostSnap};
use quench_runtime::{SnapshotReader, SnapshotWriter};

impl NodeHost {
    pub(crate) fn write_snapshot(&self, writer: &mut SnapshotWriter<'_>) -> Result<(), String> {
        let Self {
            // The installation and the process's own entry and environment.
            node_library_root: _,
            compile_cache: _,
            shared_state,
            commonjs_entry: _,
            inline_source: _,
            eval_builtin_globals: _,
            eval_source_nonempty: _,
            initial_node_debug: _,
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
        shared_state.borrow().write_snapshot(writer)
    }

    pub(crate) fn read_snapshot(&mut self, reader: &mut SnapshotReader<'_, '_>) -> Result<(), String> {
        self.shared_state.borrow_mut().read_snapshot(reader)
    }
}

impl SharedNodeState {
    fn write_snapshot(&self, writer: &mut SnapshotWriter<'_>) -> Result<(), String> {
        let Self {
            workers,
            vm_scripts,
            async_hooks,
            native_tasks,
            #[cfg(quench_node_cares)]
            cares,
            dns,
            performance_native,
            scheduler,
            fs,
            fs_watch,
            tls_contexts,
            #[cfg(quench_icu)]
            icu_converters,
            fs_stats,
            fs_policy,
            fs_stat_watch,
            cwd: _,
            module_parent_api,
            active_esm_requires,
            builtin_cache,
            buffer_pool,
            commonjs_cache,
            primordials_module,
            realm_module_programs,
            crypto_handle_symbol,
            library_originals,
            event_targets_api,
            trace,
            util_intrinsics,
            node_error_api,
            process_argv: _,
            process_control: _,
            process_signals,
            stdin,
            process_ipc,
            #[cfg(quench_openssl_provider)]
            crypto_hashes,
            crypto_key_material,
            #[cfg(quench_openssl_provider)]
            crypto_signatures,
            crypto_secret_jobs,
            zlib_native,
            messaging,
            #[cfg(not(quench_node_cares))]
            dns_channels,
            serdes,
            unhandled_rejection_mode: _,
            output: _,
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
            fetch,
            diagnostics,
            http,
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
        } = self;
        require_quiescent(native_tasks.snapshot_quiescent(), "the native task pool")?;
        require_quiescent(scheduler.snapshot_quiescent(), "scheduled callbacks")?;
        require_quiescent(fs.snapshot_quiescent(), "open file descriptors")?;
        require_quiescent(process_signals.snapshot_quiescent(), "signal listeners")?;
        require_quiescent(realm_module_programs.is_empty(), "realm module programs")?;
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
        #[cfg(quench_node_cares)]
        cares.write_snapshot(writer)?;
        #[cfg(quench_icu)]
        icu_converters.write_snapshot(writer)?;
        #[cfg(quench_openssl_provider)]
        {
            crypto_hashes.write_snapshot(writer)?;
            crypto_signatures.write_snapshot(writer)?;
        }
        #[cfg(not(quench_node_cares))]
        dns_channels.write_snapshot(writer)?;
        workers.write_snapshot(writer)?;
        vm_scripts.write_snapshot(writer)?;
        async_hooks.write_snapshot(writer)?;
        dns.write_snapshot(writer)?;
        performance_native.write_snapshot(writer)?;
        fs_watch.write_snapshot(writer)?;
        tls_contexts.write_snapshot(writer)?;
        fs_stats.write_snapshot(writer)?;
        fs_policy.write_snapshot(writer)?;
        fs_stat_watch.write_snapshot(writer)?;
        module_parent_api.write(writer)?;
        builtin_cache.write(writer)?;
        buffer_pool.write_snapshot(writer)?;
        commonjs_cache.write(writer)?;
        primordials_module.write(writer)?;
        crypto_handle_symbol.write(writer)?;
        for original in &library_originals.0 {
            original.write(writer)?;
        }
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
        timer_duration_warnings.write(writer)?;
        fetch.write_snapshot(writer)?;
        diagnostics.write(writer)?;
        http.write_snapshot(writer)?;
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
        #[cfg(quench_node_cares)]
        self.cares.read_snapshot(reader)?;
        #[cfg(quench_icu)]
        self.icu_converters.read_snapshot(reader)?;
        #[cfg(quench_openssl_provider)]
        {
            self.crypto_hashes.read_snapshot(reader)?;
            self.crypto_signatures.read_snapshot(reader)?;
        }
        #[cfg(not(quench_node_cares))]
        self.dns_channels.read_snapshot(reader)?;
        self.workers.read_snapshot(reader)?;
        self.vm_scripts.read_snapshot(reader)?;
        self.async_hooks.read_snapshot(reader)?;
        self.dns.read_snapshot(reader)?;
        self.performance_native.read_snapshot(reader)?;
        self.fs_watch.read_snapshot(reader)?;
        self.tls_contexts.read_snapshot(reader)?;
        self.fs_stats.read_snapshot(reader)?;
        self.fs_policy.read_snapshot(reader)?;
        self.fs_stat_watch.read_snapshot(reader)?;
        self.module_parent_api = HostSnap::read(reader)?;
        self.builtin_cache = HostSnap::read(reader)?;
        self.buffer_pool.read_snapshot(reader)?;
        self.commonjs_cache = HostSnap::read(reader)?;
        self.primordials_module = HostSnap::read(reader)?;
        self.crypto_handle_symbol = HostSnap::read(reader)?;
        for original in &mut self.library_originals.0 {
            *original = HostSnap::read(reader)?;
        }
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
        self.timer_duration_warnings = HostSnap::read(reader)?;
        self.fetch.read_snapshot(reader)?;
        self.diagnostics = HostSnap::read(reader)?;
        self.http.read_snapshot(reader)?;
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
