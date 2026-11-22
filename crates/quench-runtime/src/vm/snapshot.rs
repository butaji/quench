//! The VM's part of a startup snapshot (`crate::snapshot`). A snapshot is taken between jobs,
//! with no frame running, and holds every identity space at the indices it had: heap slots and
//! records, atoms, shapes and program ids. A value's bits therefore mean the same thing in the
//! restored VM, which restores each table in place rather than translating values.
//!
//! The main program is the one part left out: it is the entry of the process that took the
//! snapshot. A restoring process installs its own entry there, its atoms renumbered into the
//! restored atom sequence. Caches and memos derived from shapes, programs or values start
//! empty, as after the collection that invalidates them.

use super::*;
use crate::snapshot::{read_list, Reader, Snap, SnapshotError, SnapshotResult, Writer};

mod snap_impls;

/// Every VM field that holds the active realm's or the VM's prototype of a builtin family.
/// One list, so writing and reading cannot disagree about which fields or their order.
macro_rules! builtin_prototypes {
    ($apply:ident) => {
        $apply!(
            object_proto, function_proto, array_proto, string_proto, number_proto, array_buffer_proto,
            shared_array_buffer_proto, array_iterator_proto, typed_array_proto, uint8_array_proto,
            uint8_clamped_array_proto, uint16_array_proto, uint32_array_proto, int8_array_proto,
            int16_array_proto, int32_array_proto, bigint64_array_proto, biguint64_array_proto,
            float16_array_proto, float32_array_proto, float64_array_proto, data_view_proto,
            map_proto, set_proto, shadow_realm_proto, map_iterator_proto, set_iterator_proto,
            weak_map_proto, weak_set_proto, weak_ref_proto, finalization_registry_proto,
            iterator_proto, string_iterator_proto, regexp_string_iterator_proto, generator_proto,
            iterator_helper_proto, wrap_for_valid_iterator_proto, async_iterator_proto,
            async_generator_proto, async_from_sync_iterator_proto, regexp_proto,
            temporal_plain_date_proto, temporal_plain_time_proto, temporal_plain_month_day_proto,
            temporal_plain_year_month_proto
        )
    };
}

impl Vm {
    /// The VM as a snapshot, which `restore_snapshot` makes again in a new VM. Refused while
    /// anything runs or waits: a frame, a job, a module load or an embedder's script.
    pub(crate) fn write_snapshot(
        &self,
        write_host: impl FnOnce(&dyn Host, &mut crate::SnapshotWriter<'_>) -> Result<(), String>,
    ) -> SnapshotResult<Vec<u8>> {
        #[cfg(feature = "native-execution")]
        if self.native_program_resolver.is_some() {
            return Err(SnapshotError::unsupported("a runtime native source resolver"));
        }
        let Self {
            typed_views: _,
            native_microtask_running: _,
            queue_resumed: _,
            declined_fast: _,
            declined_resume: _,
            shared_entry_pair: _,
            share_entry_pair: _,
            main_realm,
            unobserved_jobs_entered: _,
            job_queue_programs: _,
            queue_chains: _,
            chainable_job: _,
            host,
            host_statics: _,
            runs_lane: _,
            specialized,
            heap,
            realm,
            object_proto: _, function_proto: _, array_proto: _, string_proto: _, number_proto: _,
            promise_proto: _, promise_constructor: _,
            array_buffer_proto: _, shared_array_buffer_proto: _, array_iterator_proto: _,
            typed_array_proto: _, uint8_array_proto: _, uint8_clamped_array_proto: _,
            uint16_array_proto: _, uint32_array_proto: _, int8_array_proto: _,
            int16_array_proto: _, int32_array_proto: _, bigint64_array_proto: _,
            biguint64_array_proto: _, float16_array_proto: _, float32_array_proto: _,
            float64_array_proto: _, data_view_proto: _, map_proto: _, set_proto: _,
            shadow_realm_proto: _, map_iterator_proto: _, set_iterator_proto: _,
            weak_map_proto: _, weak_set_proto: _, weak_ref_proto: _, kept_alive: _,
            finalization_registry_proto: _, iterator_proto: _, string_iterator_proto: _,
            regexp_string_iterator_proto: _, generator_proto: _, iterator_helper_proto: _,
            wrap_for_valid_iterator_proto: _, async_iterator_proto: _, async_generator_proto: _,
            async_from_sync_iterator_proto: _, regexp_proto: _, temporal_plain_date_proto: _,
            temporal_plain_time_proto: _, temporal_plain_month_day_proto: _,
            temporal_plain_year_month_proto: _,
            natives,
            frames: _,
            native_stack_frames: _,
            native_root_lists: _,
            gc_owned_root_count: _,
            frame_pool: _,
            lowered_tier0: _,
            pending_call_stacks: _,
            realm_native_proofs: _,
            js_native_contexts: _,
            native_roots: _,
            lane_stack: _,
            lane_roots: _,
            spare_lane_stacks: _,
            execution_mode: _,
            native_execution_rejections: _,
            js_tier_pending: _,
            #[cfg(feature = "native-execution")]
            native_program_resolver: _,
            execution_interrupt: _,
            active_call_roots: _,
            active_roots: _,
            prepared_roots: _,
            step_native_proofs: _,
            with_stack: _,
            startup_intrinsics,
            with_scope_entered, scope_names: _,
            test262_agent: _,
            shared_async_waiters: _,
            programs,
            program_cache_layouts: _,
            free_cache_layouts: _,
            active_registers: _,
            pending_row: _,
            row_stop: _,
            lane_completion: _,
            lane_raise_site: _,
            lane_native: _,
            active_context: _, program_contexts: _, idle_context: _, active_field_entries: _, cache_epochs: _,
            active_program: _,
            profile: _,
            shapes,
            shape_constructors,
            shape_accessors,
            transitions,
            shape_slot_memo: _,
            shape_attribute_memo: _,
            plain_copy_shapes: _,
            plain_assign_shapes: _,
            adopted_shapes: _,
            embedding_record_shapes: _,
            typed_array_geometry: _,
            symbol_write_proofs: _,
            symbol_load_proofs: _,
            private_site_homes: _,
            private_classes,
            array_iteration_shape: _,
            collection_iteration_shape: _,
            array_species_proof: _,
            concat_spreadable_proof: _,
            for_in: _,
            json_plain: _,
            species_slot_proof: _,
            iterator_absence_proof: _,
            then_absence_proof: _,
            promise_species_proof: _,
            promise_then_memo: _,
            function_name_shapes: _,
            typeof_names: _,
            closure_templates: _,
            enumerable_keys: _,
            enumerable_key_lists: _,
            enumerable_key_value_lists: _,
            capture_walks: _,
            name_atom_memo: _,
            atom_text,
            atoms: _,
            array_index_atoms: _,
            atom_sequence,
            regexp_subjects: _,
            concat_memo: _,
            regexp_matchers: _,
            atom_classes: _,
            intrinsic_natives: _,
            native_families: _, first_chunks: _, streamed_stubs: _,
            dynamic_strings: _,
            symbol_registry,
            well_known_symbols,
            bound_parts_layout: _,
            call_targets: _, construct_targets: _, frameless: _,
            field_caches: _,
            inherited_field_indices: _,
            accessor_field_indices: _,
            accessor_fields: _,
            field_add_indices: _,
            field_add_sets: _, field_add_chains: _, json_parse_cache: _,
            keyed_adds: _,
            megamorphic_field_indices: _,
            field_cache_writes: _,
            megamorphic_fields: _,
            field_stubs: _,
            holder_hints: _,
            rc_pinned_shapes: _,
            rc_pin_epoch: _,
            length_atom,
            prototype_atom,
            instanceof_constructor_layouts: _,
            in_presences: _,
            instanceof_inheriting_holders: _,
            instanceof_proofs: _,
            closure_shapes: _,
            resolver_function_shape: _,
            bound_function_shape: _,
            iterator_result_header: _,
            reused_iterator_result: _,
            spare_await_state: _,
            descriptor_shapes: _,
            descriptor_layout: _,
            regexp_object_header: _,
            regexp_last_index_layouts: _,
            regexp_builtin_layouts: _,
            regexp_symbol_layouts: _,
            instanceof_has_instance_holder,
            // Derived from the holder, which the restore sets through its setter.
            instanceof_has_instance_link: _,
            to_fixed_atom,
            to_precision_atom,
            runtime_atoms,
            // A proof of the running heap's methods; a restored heap proves its own.
            constructed_shapes: _,
            // Names constructors and shapes of the running heap; a restored heap derives its own.
            preshaped_receivers: _,
            // Names constructors and layouts of the running heap; a restored heap learns its own.
            learned_receivers: _,
            // Templates name roots of the running heap; a restored heap starts without any,
            // and its hosts capture them again.
            object_templates: _,
            method_caches: _,
            megamorphic_methods: _,
            method_cache_atoms: _,
            method_cache_sites: _,
            #[cfg(feature = "profile-aggregate")]
                invalidated_methods: _,
            object_shapes: _,
            descriptors,
            function_values,
            module_functions,
            wasm_exported_functions: _,
            private_homes,
            default_collation: _,
            wasm_js,
            direct_eval: _,
            direct_eval_var_program: _,
            parameter_eval: _,
            eval_script_context: _,
            deferred_dependency_batch: _,
            construct_target: _,
            // Each process draws its own sequence (`fresh_random_seed`).
            random_state: _,
            // Each process keeps its own budget.
            dynamic_functions_to_keep: _,
            lane_deferred: _, running_window: _,
            // A snapshot is taken between runs, with no lane activation's eval running.
            eval_callers: _,
            // Resolutions are the process's (its files), not the snapshot's.
            resolved_module_requests: _,
            resolved_module_names: _,
        } = self;
        let Realm {
            globals,
            global_lexical_declarations,
            global_lexical_bindings,
            immutable_global_lexical_bindings,
            global_lexical_states,
            parked,
            intrinsics,
            jobs: _,
            template_objects,
            promise,
        } = realm;
        if let Some(what) = self.running_work() {
            return Err(SnapshotError::unsupported(what));
        }
        let mut writer = Writer::new();
        writer.bool(*specialized);
        writer.value(*main_realm);
        // Atoms first: the programs decode against the restored sequence.
        writer.len(atom_text.len());
        atom_text.iter().for_each(|name| writer.str(name));
        writer.len(atom_sequence.appended_len());
        for offset in 0..atom_sequence.appended_len() {
            atom_sequence.appended(offset).write_snapshot(&mut writer);
        }
        heap.write_snapshot(&mut writer)?;
        writer.value(*globals);
        global_lexical_declarations.snap(&mut writer)?;
        global_lexical_bindings.snap(&mut writer)?;
        immutable_global_lexical_bindings.snap(&mut writer)?;
        global_lexical_states.snap(&mut writer)?;
        parked.snap(&mut writer)?;
        intrinsics.snap(&mut writer)?;
        template_objects.snap(&mut writer)?;
        promise.write_snapshot(&mut writer)?;
        macro_rules! write_prototypes {
            ($($field:ident),*) => { $( writer.value(self.$field); )* };
        }
        builtin_prototypes!(write_prototypes);
        natives.snap(&mut writer)?;
        self.heap.kept_atom_strings().collect::<Vec<_>>().snap(&mut writer)?;
        startup_intrinsics.write_snapshot(&mut writer)?;
        writer.bool(*with_scope_entered);
        programs.write_snapshot(&mut writer)?;
        // The root shape is every table's first; the others follow their parents.
        writer.len(shapes.len());
        for shape in &shapes[1..] {
            writer.u32(shape.parent);
            shape.transition.snap(&mut writer)?;
            writer.u32(shape.storage_len);
            shape.dictionary_trigger().snap(&mut writer)?;
        }
        shape_constructors.snap(&mut writer)?;
        shape_accessors.snap(&mut writer)?;
        writer.u32s(&transitions.children().collect::<Vec<_>>());
        shape_lookup::owned_dictionaries(shapes).snap(&mut writer)?;
        symbol_registry.snap(&mut writer)?;
        well_known_symbols.snap(&mut writer)?;
        for atom in [length_atom, prototype_atom, to_fixed_atom, to_precision_atom] {
            writer.u32(*atom);
        }
        runtime_atoms.snap(&mut writer)?;
        descriptors.snap(&mut writer)?;
        writer.len(function_values.len());
        for (key, handles) in function_values.iter() {
            key.snap(&mut writer)?;
            let live = handles.iter().filter_map(|handle| heap.weak_value(*handle)).collect::<Vec<_>>();
            writer.values(&live);
        }
        let live_module_functions = module_functions
            .iter()
            .filter_map(|(program, id, handle)| Some((program, id, heap.weak_value(handle)?)))
            .collect::<Vec<_>>();
        writer.len(live_module_functions.len());
        for (program, id, function) in live_module_functions {
            program.snap(&mut writer)?;
            writer.u32(id);
            writer.value(function);
        }
        private_homes.snap(&mut writer)?;
        // The heap keeps the closures and homes of the classes evaluated so far, so a restored
        // VM must know them: a second evaluation of one shares its private names.
        private_classes.snap(&mut writer)?;
        let wasm_js::WasmJsIntrinsics { module, instance, memory, table, global, tag, js_tag } = wasm_js;
        [*module, *instance, *memory, *table, *global, *tag, *js_tag].snap(&mut writer)?;
        writer.value(*instanceof_has_instance_holder);
        write_host(&**host, &mut crate::SnapshotWriter { writer: &mut writer, heap }).map_err(SnapshotError)?;
        Ok(writer.finish())
    }

    /// What a snapshot cannot be taken across that is running or waiting now, if anything.
    fn running_work(&self) -> Option<&'static str> {
        self.running_execution().or_else(|| self.running_embedding())
    }

    /// The running frames, jobs and lane completions a snapshot cannot be taken across.
    fn running_execution(&self) -> Option<&'static str> {
        let running = [
            ("a running frame", self.frames.is_empty() && self.native_stack_frames.is_empty() && self.active_call_roots.is_empty()),
            ("a with statement", self.with_stack.is_empty()),
            ("a pending job", self.realm.jobs.is_empty() && self.kept_alive.is_empty() && !self.native_microtask_running),
            ("a lane completion", self.lane_completion.is_none()),
            ("a lane native call", self.lane_native.is_none()),
        ];
        running.into_iter().find(|(_, quiescent)| !quiescent).map(|(what, _)| what)
    }

    /// The waiters, evals and WebAssembly exports a snapshot cannot be taken across.
    fn running_embedding(&self) -> Option<&'static str> {
        let running = [
            ("an Atomics waiter", self.shared_async_waiters.is_empty() && !self.test262_agent.installed),
            (
                "an eval or construction",
                !self.direct_eval
                    && self.direct_eval_var_program.is_none()
                    && !self.parameter_eval
                    && !self.eval_script_context
                    && !self.deferred_dependency_batch
                    && self.construct_target.is_none(),
            ),
            ("WebAssembly", self.wasm_exported_functions.is_empty()),
        ];
        running.into_iter().find(|(_, quiescent)| !quiescent).map(|(what, _)| what)
    }

    /// Makes this VM, newly made, the one `write_snapshot` wrote into `bytes`, with `entry` as
    /// its main program; returns `entry` with its atoms numbered as the restored VM's, which
    /// is the program the restored VM runs. The host's own state is restored apart.
    pub(crate) fn restore_snapshot(
        &mut self,
        bytes: crate::KeptBytes,
        entry: &ResidualProgram,
        read_host: impl FnOnce(&mut dyn Host, &mut crate::SnapshotReader<'_, '_>) -> Result<(), String>,
    ) -> SnapshotResult<ResidualProgram> {
        self.restore_snapshot_state(bytes, None, read_host)?;
        let main = self.rebased_entry(entry)?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("snapshot_entry");
        self.install_restored_main(Rc::new(main.clone()));
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("snapshot_ready");
        Ok(main)
    }

    pub fn restore_snapshot_state(
        &mut self,
        bytes: crate::KeptBytes,
        native: Option<&ResidualProgram>,
        read_host: impl FnOnce(&mut dyn Host, &mut crate::SnapshotReader<'_, '_>) -> Result<(), String>,
    ) -> SnapshotResult<()> {
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("snapshot_restore_begin");
        let mut reader = Reader::new((*bytes).as_ref(), Some(self.host_statics.kept_bytes_read))?;
        self.specialized = reader.bool()?;
        self.main_realm = reader.value()?;
        if native.is_some_and(|program| program.specialized != self.specialized) {
            return Err(SnapshotError("native script specialization does not match runtime".into()));
        }
        self.read_atoms(&mut reader, native.map(|program| &program.atoms))?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("snapshot_atoms");
        self.heap = Heap::read_snapshot(&mut reader, self.heap.memory_policy())?;
        self.forget_learned_receivers();
        self.intern_typeof_names();
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("snapshot_heap");
        self.realm.globals = reader.value()?;
        self.realm_native_proofs.forget();
        self.realm.global_lexical_declarations = Snap::unsnap(&mut reader)?;
        self.realm.global_lexical_bindings = Snap::unsnap(&mut reader)?;
        self.realm.immutable_global_lexical_bindings = Snap::unsnap(&mut reader)?;
        self.realm.global_lexical_states = Snap::unsnap(&mut reader)?;
        self.realm.parked = Snap::unsnap(&mut reader)?;
        self.realm.intrinsics = Snap::unsnap(&mut reader)?;
        self.realm.template_objects = Snap::unsnap(&mut reader)?;
        // The restored realms' declarations may shadow names a derived view's hints read.
        self.note_global_lexical_names();
        self.realm.promise = promise::PromiseRuntime::read_snapshot(&mut reader)?;
        macro_rules! read_prototypes {
            ($($field:ident),*) => { $( self.$field = reader.value()?; )* };
        }
        builtin_prototypes!(read_prototypes);
        // The promise fields follow the restored active realm from its intrinsics.
        let intrinsics = &self.realm.intrinsics;
        self.promise_proto =
            intrinsics.builtin_prototypes.get(&(self.realm.globals, Native::Promise)).copied().unwrap_or(Value::NULL);
        self.promise_constructor = intrinsics.promise_constructors.get(&self.realm.globals).copied().unwrap_or(Value::NULL);
        self.natives = Snap::unsnap(&mut reader)?;
        Vec::<(Atom, Value)>::unsnap(&mut reader)?.into_iter().for_each(|(atom, string)| self.heap.register_string_atom(string, atom, true));
        self.startup_intrinsics = startup_intrinsics::StartupIntrinsics::read_snapshot(&mut reader)?;
        self.with_scope_entered = reader.bool()?;
        self.refresh_call_refusal();
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("snapshot_realm");
        self.programs = ProgramStore::read_snapshot(&mut reader, &bytes, &self.atom_sequence)?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("snapshot_programs");
        self.read_shapes(&mut reader)?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("snapshot_shapes");
        self.symbol_registry = Snap::unsnap(&mut reader)?;
        self.well_known_symbols = Snap::unsnap(&mut reader)?;
        self.length_atom = reader.u32()?;
        self.prototype_atom = reader.u32()?;
        self.to_fixed_atom = reader.u32()?;
        self.to_precision_atom = reader.u32()?;
        self.runtime_atoms = Snap::unsnap(&mut reader)?;
        self.descriptors = Snap::unsnap(&mut reader)?;
        self.read_function_values(&mut reader)?;
        self.read_module_functions(&mut reader)?;
        self.private_homes = Snap::unsnap(&mut reader)?;
        self.private_classes = Snap::unsnap(&mut reader)?;
        let [module, instance, memory, table, global, tag, js_tag] = <[Value; 7]>::unsnap(&mut reader)?;
        self.wasm_js = wasm_js::WasmJsIntrinsics { module, instance, memory, table, global, tag, js_tag };
        let holder = reader.value()?;
        self.set_instanceof_has_instance_holder(holder);
        read_host(&mut *self.host, &mut crate::SnapshotReader { reader: &mut reader, heap: &self.heap })
            .map_err(SnapshotError)?;
        reader.finish()?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("snapshot_host");
        Ok(())
    }

    /// Reads the function cache: each key's functions, held weakly as the running VM holds them.
    fn read_function_values(&mut self, reader: &mut Reader<'_>) -> SnapshotResult<()> {
        let function_values =
            read_list(reader, |reader| Ok((<(ProgramId, u32)>::unsnap(reader)?, reader.values()?)))?;
        for (key, functions) in function_values {
            let handles = functions
                .into_iter()
                .map(|function| {
                    self.heap
                        .weak_handle(function)
                        .ok_or_else(|| SnapshotError("snapshot function cache names a dead value".into()))
                })
                .collect::<SnapshotResult<Vec<_>>>()?;
            self.function_values.record(key, handles);
        }
        Ok(())
    }

    /// Reads the module functions: each module instance's hoisted declarations, held weakly as the
    /// running VM holds them.
    fn read_module_functions(&mut self, reader: &mut Reader<'_>) -> SnapshotResult<()> {
        let functions = read_list(reader, |reader| {
            Ok((<ProgramId>::unsnap(reader)?, reader.u32()?, reader.value()?))
        })?;
        for (program, id, function) in functions {
            let handle = self
                .heap
                .weak_handle(function)
                .ok_or_else(|| SnapshotError("snapshot module function names a dead value".into()))?;
            self.module_functions.set(program, id, handle);
        }
        Ok(())
    }

    pub fn install_restored_main(&mut self, main: Rc<ResidualProgram>) {
        self.programs.install_main(main.clone());
        self.rebuild_program_cache_layouts();
        self.keep_slot_representations();
        self.materialize_program_constants(ProgramId::MAIN, &main);
        self.set_active_program(ProgramId::MAIN);
    }

    /// The atom sequence `write_snapshot` wrote: the base table, then each appended name, which
    /// the VM indexes again as it appends them.
    fn read_atoms(&mut self, reader: &mut Reader<'_>, native: Option<&AtomTable>) -> SnapshotResult<()> {
        let base = reader.len()?;
        self.atom_text = if let Some(atoms) = native {
            if atoms.len() != base {
                return Err(SnapshotError("native script atom IDs do not match runtime namespace".into()));
            }
            for name in atoms.iter() {
                if reader.str()? != name {
                    return Err(SnapshotError("native script atom IDs do not match runtime namespace".into()));
                }
            }
            atoms.clone()
        } else {
            let mut text = String::new();
            let mut ends = Vec::with_capacity(base);
            for _ in 0..base {
                text.push_str(reader.str()?);
                ends.push(u32::try_from(text.len()).map_err(|_| SnapshotError("snapshot atom text exceeds u32".into()))?);
            }
            AtomTable::new(text, ends)
        };
        let appended = reader.len()?;
        self.atom_sequence = Rc::new(crate::bytecode::AtomSequence::new(self.atom_text.clone()));
        // Sized for every atom at once: growing would hash every name again.
        self.atoms = hashbrown::HashTable::with_capacity(base + appended);
        self.array_index_atoms.clear();
        self.name_atom_memo.clear();
        self.private_site_homes.clear();
        self.atom_classes.clear();
        self.intrinsic_natives.clear();
        self.atom_classes.reserve(base + appended);
        self.atom_classes.resize_with(base, Default::default);
        let atom_text = self.atom_text.clone();
        for (id, name) in atom_text.iter().enumerate() {
            self.index_atom(Self::atom_hash_str(name), id as Atom, name);
        }
        for _ in 0..appended {
            let name = JsString::read_snapshot(reader)?;
            let hash = Self::atom_hash_units(name.units());
            self.insert_dynamic_atom(name, hash);
        }
        Ok(())
    }

    /// The shape table `write_snapshot` wrote: each shape after the root made again from its
    /// parent, so the facts a shape derives from its chain are derived anew.
    fn read_shapes(&mut self, reader: &mut Reader<'_>) -> SnapshotResult<()> {
        let count = reader.len()?;
        self.shapes = Shape::table();
        self.shapes.reserve(count.saturating_sub(self.shapes.len()));
        for _ in 1..count {
            let parent = reader.u32()?;
            let transition = Snap::unsnap(reader)?;
            let storage_len = reader.u32()?;
            let dictionary_trigger = Snap::unsnap(reader)?;
            let parent_facts = self
                .shapes
                .get(parent as usize)
                .ok_or_else(|| SnapshotError("snapshot shape follows no parent".into()))?
                .facts;
            push_child(&mut self.shapes, Shape::child(
                Some(parent),
                transition,
                storage_len as usize,
                dictionary_trigger,
                parent_facts,
            ));
        }
        self.shape_constructors = Snap::unsnap(reader)?;
        self.shape_accessors = Snap::unsnap(reader)?;
        let children = reader.u32_iter()?;
        self.transitions = shape_transitions::ShapeTransitionIndex::with_capacity(children.len());
        for child in children {
            if !self.transitions.try_insert_child(&self.shapes, child) {
                return Err(SnapshotError("snapshot transition names no cached shape".into()));
            }
        }
        shape_lookup::restore_owned_dictionaries(&mut self.shapes, Snap::unsnap(reader)?)?;
        Ok(())
    }

    /// `entry`, compiled apart, with its atoms numbered as this VM's.
    /// An entry decoded from its cached form (`Runtime::compile_entry`) reads that form again,
    /// with this VM's atoms; any other is encoded compactly for the same. Encoding the entry
    /// was about 1.5% of an npm scenario's instructions (chalk, 2026-10-22).
    fn rebased_entry(&mut self, entry: &ResidualProgram) -> SnapshotResult<ResidualProgram> {
        // A table in the program's own ids marks the form `decode_trusted` reads.
        let cached = entry
            .encoded_records
            .as_ref()
            .filter(|records| records.atoms.is_none())
            .and_then(|records| {
                ResidualProgram::decode_trusted_interned(records.bytes.clone(), &mut |name| self.intern_atom(name)).ok()
            });
        let mut main = match cached {
            Some(main) => main,
            None => {
                let (bytes, _) = entry.encode_compact();
                let text = entry
                    .source
                    .as_ref()
                    .map_or_else(|| crate::bytecode::SourceText::Static(""), |source| source.text.clone());
                let bytes: crate::KeptBytes = Rc::new(bytes);
                ResidualProgram::decode_compact(bytes, &mut |name| self.intern_atom(name), text)
                    .map_err(|error| SnapshotError(format!("snapshot entry: {error}")))?
            }
        };
        main.atoms = AtomTable::shared(self.atom_sequence.clone(), self.atom_sequence.len());
        if entry.validated.holds() {
            main.validated.establish();
        }
        Ok(main)
    }

    /// Every program's cache layout, in id order, with empty caches.
    fn rebuild_program_cache_layouts(&mut self) {
        self.program_cache_layouts.clear();
        self.free_cache_layouts.clear();
        self.field_caches.clear();
        self.megamorphic_field_indices.clear();
        self.field_add_indices.clear();
        self.inherited_field_indices.clear();
        self.accessor_field_indices.clear();
        self.accessor_fields.clear();
        self.method_caches.clear();
        self.object_shapes.clear();
        self.field_cache_writes = Some(Vec::new());
        for index in 0..self.programs.len() {
            let id = ProgramId::from_raw(index as u32);
            // A restored program's layout comes from its recorded site counts: decoding it
            // waits for its first use, which primes its loops.
            match self.programs.cache_site_counts(id) {
                Some(sites) => self.append_cache_layout(id, sites),
                None => self.program_cache_layouts.push(ProgramCacheLayout {
                    sites: Default::default(),
                    field_base: self.field_caches.len(),
                    method_base: self.method_caches.len(),
                    object_base: self.object_shapes.len(),
                    shared_uncached_field: None,
                }),
            }
        }
        self.programs.bump_context_epoch();
    }
}

#[cfg(test)]
mod tests {
    use crate::{Engine, Host, NativeContext, RootId, RootedError, Runtime};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn native_loop_calls_return_their_integer_state_with_a_captured_constant() {
        let body = r#"
            const feedback = 0xD008;
            function state(n, seed) {
                n |= 0; let x = seed | 0;
                for (let i = 0; i < n; i = (i + 1) | 0) {
                    if ((x & 1) === 0) x = x >> 1;
                    else x = (x >> 1) ^ feedback;
                }
                return x | 0;
            }
            for (let warm = 0; warm < 64; warm++) state(1, warm);
            const args = ['-1', '0', '123456789', '1', '17'];
            print(state(Number(args[0]), Number(args[1])));
            print(state(Number(args[1]), Number(args[2])));
            print(state(Number(args[3]), Number(args[2])));
            print(state(Number(args[4]), Number(args[2])));
        "#;
        let method = format!("const sink = {{ log: print }}; {}", body.replace("print(state(", "sink.log(state("));
        for source in [body.to_owned(), format!("(function () {{ {body} }})();"), format!("(function () {{ {method} }})();")] {
            let program = Engine::specialize(&source, "native-loop-result.js").unwrap();
            let mut expected = Runtime::new(crate::test_host::TestHost::default());
            expected.set_execution_mode(crate::ExecutionMode::Interpreted);
            expected.execute(&program).unwrap();
            let expected = expected.host_mut().lines().to_vec();
            let native = crate::NativeJsProgram::compile_eager(program.clone()).unwrap();
            let mut builder = Runtime::new(crate::test_host::TestHost::default());
            let bytes = builder.build_snapshot(&program, |_, _| Ok(())).unwrap();
            let mut runtime = Runtime::new(crate::test_host::TestHost::default());
            runtime.set_execution_mode(crate::ExecutionMode::NativeOnly);
            runtime.execute_native_from_snapshot(Rc::new(bytes), &native, |_, _| Ok(())).unwrap();
            assert_eq!(runtime.host_mut().lines().to_vec(), expected, "{source}");
        }
    }

    #[test]
    fn native_bootstrap_preserves_static_namespace_and_retains_code_for_jobs() {
        let program = Engine::specialize(r#"
            function fold(n, x) { for (let i = 0; i < n; i++) x = (x ^ (x << 3)) | 0; return x; }
            print(fold(17, 7));
            Promise.resolve(9).then(value => print(value));
        "#, "native-bootstrap.js").unwrap();
        let mut builder = Runtime::new(crate::test_host::TestHost::default());
        let bytes = builder.build_snapshot(&program, |_, _| Ok(())).unwrap();
        let mut expected = Runtime::new(crate::test_host::TestHost::default());
        expected.execute(&program).unwrap();
        let expected = expected.host_mut().lines().to_vec();
        for mode in [crate::ExecutionMode::Compiled, crate::ExecutionMode::NativeOnly] {
            let native = crate::NativeJsProgram::compile_eager(program.clone()).unwrap();
            let mut runtime = Runtime::new(crate::test_host::TestHost::default());
            runtime.set_execution_mode(mode);
            runtime.execute_native_from_snapshot(Rc::new(bytes.clone()), &native, |_, _| Ok(())).unwrap();
            let program = native.residual().clone();
            drop(native);
            runtime.run_host_jobs(&program).unwrap();
            assert_eq!(&*runtime.host_mut().lines(), &expected);
            assert_eq!(runtime.native_execution_rejections(), 0);
        }
    }

    #[test]
    fn native_bootstrap_rejects_namespace_or_specialization_mismatch_before_entry() {
        let program = Engine::specialize("function original() { print('effect'); } original();", "native-bootstrap.js").unwrap();
        let mut builder = Runtime::new(crate::test_host::TestHost::default());
        let bytes = builder.build_snapshot(&program, |_, _| Ok(())).unwrap();
        for (foreign, message) in [
            (Engine::specialize("function foreign() { print('effect'); } foreign();", "foreign.js").unwrap(), "atom IDs"),
            (Engine::specialize("function foreign(argument) { print(extra); } foreign();", "foreign.js").unwrap(), "atom IDs"),
            (Engine::specialize_unspecialized("function original() { print('effect'); } original();", "native-bootstrap.js").unwrap(), "specialization"),
        ] {
            let native = crate::NativeJsProgram::compile_eager(foreign).unwrap();
            let mut runtime = Runtime::new(crate::test_host::TestHost::default());
            runtime.set_execution_mode(crate::ExecutionMode::NativeOnly);
            let error = runtime.execute_native_from_snapshot(Rc::new(bytes.clone()), &native, |_, _| Ok(())).unwrap_err();
            assert!(error.to_string().contains(message), "{error}");
            assert!(runtime.host_mut().lines().is_empty());
        }
    }

    /// Global code a host runs while installing: what a snapshot keeps.
    const SETUP: &str = r#"
        var counter = 0;
        function bump() { return ++counter; }
        var pattern = /a(b+)c/g;
        var Point = class Point {
            #x;
            constructor(x) { this.#x = x; }
            get x() { return this.#x; }
            #twice() { return this.#x * 2; }
            twice() { return this.#twice(); }
            static #made = 0;
            static make(x) { Point.#made++; return new Point(x); }
            static has(object) { return #x in object; }
            static get count() { return Point.#made; }
        };
        var kept = new Point(9);
        let lexical = 'lexical';
        function readLexical() { return lexical; }
        var map = new Map([[1, 'one'], [2, 'two']]);
        var set = new Set(['member']);
        var registered = Symbol.for('registered');
        var local = Symbol('local');
        var record = { [local]: 7, nested: { deep: [1, 2, 3] } };
        Object.defineProperty(record, 'hidden', { value: 3, enumerable: false });
        var bound = bump.bind(null);
        var bytes = new Uint8Array([1, 2, 3]);
        var epoch = new Date(0);
        var weak = new WeakMap([[record, 'weak']]);
        var settled = Promise.resolve('settled');
        var template = (strings => strings)`a${1}b`;
        (function () { globalThis.resumed = counter + 100; })
    "#;

    /// The entry a restored runtime runs against the setup's realm.
    const ENTRY: &str = r#"
        print(bump(), bump(), bound(), resumed);
        print(readLexical(), pattern.exec('xabbc')[1], Point.make(4).x, typeof Point, Point.has(kept), Point.has({}), kept.x, kept.twice(), Point.count);
        print(map.get(2), set.has('member'), Symbol.for('registered') === registered);
        print(record[local], JSON.stringify(record), Object.getOwnPropertyNames(record).join());
        print(bytes.join(), epoch.toISOString(), weak.get(record), template.raw.join('|'));
        print([3, 1, 2].sort().join(), 'abc'.replace(/b/, 'B'), new Error('failed').message);
        settled.then(value => print(value));
    "#;

    #[derive(Default)]
    struct SnapshotHost {
        output: Rc<RefCell<Vec<String>>>,
        /// Whether `initialize` stops where a snapshot is taken.
        building: bool,
        resume_callback: Option<RootId>,
    }

    impl SnapshotHost {
        fn finish_installation(context: &mut NativeContext<'_>) -> Result<(), RootedError> {
            let callback = context.host_as::<Self>().resume_callback.take().expect("installed callback");
            let undefined = context.undefined();
            context.call_rooted(callback, undefined, &[])?;
            context.release_root(callback);
            Ok(())
        }
    }

    impl Host for SnapshotHost {
        // Its tests exercise no lane behaviour (`Host::runs_lane`).
        fn runs_lane(&self) -> bool { false }
        fn write_line(&mut self, line: &str) {
            self.output.borrow_mut().push(line.into());
        }

        fn clock_millis(&mut self) -> f64 {
            0.0
        }

        fn initialize(context: &mut NativeContext<'_>) -> Result<(), RootedError> {
            let callback = context.evaluate_script_rooted(SETUP, "setup.js")?;
            let callback = context.retain(callback)?;
            context.host_as::<Self>().resume_callback = Some(callback);
            if context.host_as::<Self>().building {
                return Ok(());
            }
            Self::finish_installation(context)
        }

        fn resume(context: &mut NativeContext<'_>) -> Result<(), RootedError> {
            Self::finish_installation(context)
        }
    }

    fn output_of(runtime: &mut Runtime<SnapshotHost>, program: &crate::ResidualProgram) -> Vec<String> {
        runtime.execute_restored(program).unwrap();
        runtime.run_host_jobs(program).unwrap();
        runtime.host_mut().output.borrow().clone()
    }

    #[test]
    fn restored_realm_runs_an_entry_as_the_installing_one_does() {
        let entry = Engine::specialize(ENTRY, "entry.js").unwrap();
        let mut direct = Runtime::new(SnapshotHost::default());
        direct.execute_deferred_jobs(&entry).unwrap();
        direct.run_host_jobs(&entry).unwrap();
        let expected = direct.host_mut().output.borrow().clone();
        assert_eq!(expected.len(), 7, "{expected:?}");

        // The builder's own entry differs from the restored one: only its atoms carry over.
        let builder_entry = Engine::specialize("print('builder');", "builder.js").unwrap();
        let mut builder = Runtime::new(SnapshotHost { building: true, ..SnapshotHost::default() });
        let snapshot = builder
            .build_snapshot(&builder_entry, |host, writer| writer.option_root(host.resume_callback))
            .unwrap();
        assert!(builder.host_mut().output.borrow().is_empty());

        for _ in 0..2 {
            let mut restored = Runtime::new(SnapshotHost::default());
            let program = restored
                .restore_snapshot(Rc::new(snapshot.clone()), &entry, |host, reader| {
                    host.resume_callback = reader.option_root()?;
                    Ok(())
                })
                .unwrap();
            assert_eq!(output_of(&mut restored, &program), expected);
        }
    }
}
