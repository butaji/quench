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

/// The shape table past its root, which every table holds first: each shape's parent, transition,
/// storage length and dictionary trigger, in order (a shape follows its parent).
fn write_shapes(writer: &mut Writer, shapes: &[Shape]) -> SnapshotResult<()> {
    writer.len(shapes.len());
    for shape in &shapes[1..] {
        writer.u32(shape.parent);
        shape.transition.snap(writer)?;
        writer.u32(shape.storage_len);
        shape.dictionary_trigger.snap(writer)?;
    }
    Ok(())
}

impl<H: Host> Vm<H> {
    /// The VM as a snapshot, which `restore_snapshot` makes again in a new VM. Refused while
    /// anything runs or waits: a frame, a job, a module load or an embedder's script.
    pub(crate) fn write_snapshot(
        &self,
        write_host: impl FnOnce(&H, &mut crate::SnapshotWriter<'_>) -> Result<(), String>,
    ) -> SnapshotResult<Vec<u8>> {
        #[cfg(feature = "native-execution")]
        if self.native_program_resolver.is_some() {
            return Err(SnapshotError::unsupported("a runtime native source resolver"));
        }
        let Self {
            typed_views: _,
            native_microtask_running: _,
            unobserved_jobs_entered: _,
            host,
            specialized,
            heap,
            realm,
            object_proto: _, function_proto: _, array_proto: _, string_proto: _, number_proto: _,
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
            js_native_contexts: _,
            native_roots: _,
            lane_stack: _,
            lane_roots: _,
            execution_mode: _,
            native_execution_rejections: _,
            js_tier_pending: _,
            #[cfg(feature = "native-execution")]
            native_program_resolver: _,
            execution_interrupt: _,
            active_call_roots: _,
            with_stack: _,
            startup_intrinsics,
            with_scope_entered, scope_names: _,
            test262_agent: _,
            shared_async_waiters: _,
            programs,
            program_cache_layouts: _,
            free_cache_layouts: _,
            active_registers: _,
            lane_raised: _,
            lane_raise_site: _,
            lane_returned: _,
            lane_native: _,
            lane_native_returned: _,
            lane_suspended: _,
            active_context: _, program_contexts: _, idle_context: _, active_field_entries: _, field_entries_epoch: _,
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
            embedding_record_shapes: _,
            typed_array_geometry: _,
            symbol_write_proofs: _,
            symbol_load_proofs: _,
            private_site_homes: _,
            array_iteration_shape: _,
            collection_iteration_shape: _,
            array_species_proof: _,
            for_in: _,
            json_plain: _,
            species_slot_proof: _,
            iterator_absence_proof: _,
            promise_species_proof: _,
            promise_then_memo: _,
            function_name_shapes: _,
            typeof_names: _,
            closure_templates: _,
            enumerable_keys: _,
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
            call_targets: _, frameless: _,
            field_caches: _,
            inherited_field_indices: _,
            accessor_field_indices: _,
            accessor_fields: _,
            field_add_indices: _,
            field_add_sets: _, json_parse_cache: _,
            keyed_adds: _,
            megamorphic_field_indices: _,
            field_cache_writes: _,
            megamorphic_fields: _,
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
            descriptor_shapes: _,
            descriptor_layout: _,
            regexp_object_shapes: _,
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
        write_shapes(&mut writer, shapes)?;
        shape_constructors.snap(&mut writer)?;
        shape_accessors.snap(&mut writer)?;
        writer.u32s(&transitions.children().collect::<Vec<_>>());
        shape_lookup::owned_dictionaries(shapes).snap(&mut writer)?;
        symbol_registry.snap(&mut writer)?;
        well_known_symbols.snap(&mut writer)?;
        for atom in [length_atom, prototype_atom, to_fixed_atom, to_precision_atom] { writer.u32(*atom); }
        runtime_atoms.snap(&mut writer)?;
        descriptors.snap(&mut writer)?;
        writer.len(function_values.len());
        for (key, handles) in function_values.iter() {
            key.snap(&mut writer)?;
            let live = handles.iter().filter_map(|handle| heap.weak_value(*handle)).collect::<Vec<_>>();
            writer.values(&live);
        }
        private_homes.snap(&mut writer)?;
        let wasm_js::WasmJsIntrinsics { module, instance, memory, table, global, tag, js_tag } = wasm_js;
        [*module, *instance, *memory, *table, *global, *tag, *js_tag].snap(&mut writer)?;
        writer.value(*instanceof_has_instance_holder);
        write_host(host, &mut crate::SnapshotWriter { writer: &mut writer, heap }).map_err(SnapshotError)?;
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
            ("a lane completion", self.lane_raised.is_none() && self.lane_returned.is_none() && self.lane_native_returned.is_none() && self.lane_suspended.is_none()),
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
        read_host: impl FnOnce(&mut H, &mut crate::SnapshotReader<'_, '_>) -> Result<(), String>,
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

    pub(super) fn restore_snapshot_state(
        &mut self,
        bytes: crate::KeptBytes,
        native: Option<&ResidualProgram>,
        read_host: impl FnOnce(&mut H, &mut crate::SnapshotReader<'_, '_>) -> Result<(), String>,
    ) -> SnapshotResult<()> {
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("snapshot_restore_begin");
        let mut reader = Reader::new((*bytes).as_ref(), Some(H::kept_bytes_read))?;
        self.specialized = reader.bool()?;
        if native.is_some_and(|program| program.specialized != self.specialized) {
            return Err(SnapshotError("native script specialization does not match runtime".into()));
        }
        self.read_atoms(&mut reader, native.map(|program| &program.atoms))?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("snapshot_atoms");
        self.heap = Heap::read_snapshot(&mut reader, self.heap.memory_policy())?;
        self.intern_typeof_names();
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("snapshot_heap");
        self.realm.globals = reader.value()?;
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
        self.private_homes = Snap::unsnap(&mut reader)?;
        let [module, instance, memory, table, global, tag, js_tag] = <[Value; 7]>::unsnap(&mut reader)?;
        self.wasm_js = wasm_js::WasmJsIntrinsics { module, instance, memory, table, global, tag, js_tag };
        let holder = reader.value()?;
        self.set_instanceof_has_instance_holder(holder);
        read_host(&mut self.host, &mut crate::SnapshotReader { reader: &mut reader, heap: &self.heap })
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

    pub(super) fn install_restored_main(&mut self, main: Rc<ResidualProgram>) {
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
            self.shapes.push(Shape::child(
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
#[path = "snapshot_tests.rs"]
mod tests;
