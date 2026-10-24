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

/// Every VM field that holds the active realm's or the VM's prototype of a builtin family.
/// One list, so writing and reading cannot disagree about which fields or their order.
macro_rules! builtin_prototypes {
    ($apply:ident) => {
        $apply!(
            object_proto, function_proto, array_proto, string_proto, array_buffer_proto,
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

impl Snap for Native {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        self.write_snapshot(writer);
        Ok(())
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Self::read_snapshot(reader)
    }
}

impl Snap for WellKnownSymbols {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        self.0.snap(writer)
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(Self(Snap::unsnap(reader)?))
    }
}

impl Snap for ProgramId {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        writer.u32(self.raw());
        Ok(())
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(Self::from_raw(reader.u32()?))
    }
}

impl Snap for JsString {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        self.write_snapshot(writer);
        Ok(())
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Self::read_snapshot(reader)
    }
}

/// `PropertyKey` tags, in declaration order.
mod property_key_tag {
    pub(super) const STRING: u8 = 0;
    pub(super) const SYMBOL: u8 = 1;
    pub(super) const PRIVATE: u8 = 2;
}

impl Snap for property_key::PropertyKey {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        match *self {
            Self::String(atom) => {
                writer.u8(property_key_tag::STRING);
                writer.u32(atom);
            }
            Self::Symbol(symbol) => {
                writer.u8(property_key_tag::SYMBOL);
                writer.value(symbol);
            }
            Self::Private(atom) => {
                writer.u8(property_key_tag::PRIVATE);
                writer.u32(atom);
            }
        }
        Ok(())
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(match reader.u8()? {
            property_key_tag::STRING => Self::String(reader.u32()?),
            property_key_tag::SYMBOL => Self::Symbol(reader.value()?),
            property_key_tag::PRIVATE => Self::Private(reader.u32()?),
            _ => return Err(SnapshotError("invalid snapshot property key".into())),
        })
    }
}

crate::snapshot::snap_struct!(PropertyAttributes { writable, enumerable, configurable, accessor, getter, setter });
crate::snapshot::snap_struct!(ShapeAttributes { writable, enumerable, configurable, accessor, accessors });
crate::snapshot::snap_struct!(DataAttributes { writable, enumerable, configurable });
crate::snapshot::snap_struct!(GlobalLexicalState { declarations, bindings, immutable_bindings });
crate::snapshot::snap_struct!(ParkedRealm { global, state, prototypes });
crate::snapshot::snap_struct!(ArgumentsObjectTemplate { anchor, length_slot, callee_slot, iterator_slot });
crate::snapshot::snap_struct!(IteratorRealmPrototypes { helper, wrapper, generator, async_generator });
crate::snapshot::snap_struct!(ErrorRealmIntrinsics {
    host_formatter, constructor, stack_getter, stack_setter, call_site_constructor, call_site_prototype,
});
crate::snapshot::snap_struct!(PrivateHomeKey { environment, program, function, name });
crate::snapshot::snap_struct!(RuntimeAtoms {
    lexical_this, new_target, regexp_exec, regexp_flags, last_index, regexp_flag_accessors, then,
    constructor, next, name, empty, value, done, iterator_return, writable, enumerable, configurable,
    get, set,
    bound_target, bound_this, bound_args,
});
crate::snapshot::snap_struct!(RealmIntrinsics {
    arguments_objects, iterator_prototypes, builtin_prototypes, function_constructors,
    promise_constructors, error_intrinsics, regexp_intrinsics, intl_number_format_constructors,
    intl_number_format_prototypes, intl_number_format_fallback_symbols, intl_collator_constructors,
    intl_collator_prototypes, intl_plural_rules_prototypes, intl_datetime_format_constructors,
    intl_datetime_format_prototypes, intl_datetime_format_fallback_symbols,
    intl_display_names_constructors, intl_display_names_prototypes,
    intl_duration_format_constructors, intl_duration_format_prototypes,
    intl_list_format_constructors, intl_list_format_prototypes,
    intl_relative_time_format_prototypes, intl_segmenter_prototypes,
    intl_segment_iterator_prototypes, intl_segments_prototypes, intl_locale_prototypes,
});

crate::snapshot::snapshot_enum!(dictionary_trigger: DictionaryTrigger {
    PropertyCount, DeletionPattern, PrototypeUse, ModuleNamespace,
});

impl Snap for DictionaryTrigger {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        dictionary_trigger::write(writer, *self);
        Ok(())
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        dictionary_trigger::read(reader)
    }
}

/// `ShapeTransition` tags, in declaration order.
mod transition_tag {
    pub(super) const ROOT: u8 = 0;
    pub(super) const FUNCTION_NAME: u8 = 1;
    pub(super) const FUNCTION_NAME_RECONFIGURED: u8 = 2;
    pub(super) const FUNCTION_LENGTH_RECONFIGURED: u8 = 3;
    pub(super) const CONSTRUCTED: u8 = 4;
    pub(super) const ADD: u8 = 5;
    pub(super) const DELETE: u8 = 6;
    pub(super) const VACANT: u8 = 7;
    pub(super) const DESCRIPTOR: u8 = 8;
    pub(super) const DICTIONARY: u8 = 9;
}

impl Snap for ShapeTransition {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        match *self {
            Self::Root => writer.u8(transition_tag::ROOT),
            Self::FunctionName { name } => {
                writer.u8(transition_tag::FUNCTION_NAME);
                writer.u32(name);
            }
            Self::FunctionNameReconfigured => writer.u8(transition_tag::FUNCTION_NAME_RECONFIGURED),
            Self::FunctionLengthReconfigured => writer.u8(transition_tag::FUNCTION_LENGTH_RECONFIGURED),
            Self::Constructed { constructor, new_target_is_base } => {
                writer.u8(transition_tag::CONSTRUCTED);
                writer.value(constructor);
                writer.bool(new_target_is_base);
            }
            Self::Add { key, slot, attributes } => {
                writer.u8(transition_tag::ADD);
                key.snap(writer)?;
                writer.u32(slot);
                attributes.snap(writer)?;
            }
            Self::Delete { key, slot } => {
                writer.u8(transition_tag::DELETE);
                key.snap(writer)?;
                writer.u32(slot);
            }
            Self::Vacant => writer.u8(transition_tag::VACANT),
            Self::Descriptor { slot, attributes } => {
                writer.u8(transition_tag::DESCRIPTOR);
                writer.u32(slot);
                attributes.snap(writer)?;
            }
            Self::Dictionary { trigger } => {
                writer.u8(transition_tag::DICTIONARY);
                trigger.snap(writer)?;
            }
        }
        Ok(())
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(match reader.u8()? {
            transition_tag::ROOT => Self::Root,
            transition_tag::FUNCTION_NAME => Self::FunctionName { name: reader.u32()? },
            transition_tag::FUNCTION_NAME_RECONFIGURED => Self::FunctionNameReconfigured,
            transition_tag::FUNCTION_LENGTH_RECONFIGURED => Self::FunctionLengthReconfigured,
            transition_tag::CONSTRUCTED => Self::Constructed {
                constructor: reader.value()?,
                new_target_is_base: reader.bool()?,
            },
            transition_tag::ADD => Self::Add {
                key: Snap::unsnap(reader)?,
                slot: reader.u32()?,
                attributes: Snap::unsnap(reader)?,
            },
            transition_tag::DELETE => Self::Delete { key: Snap::unsnap(reader)?, slot: reader.u32()? },
            transition_tag::VACANT => Self::Vacant,
            transition_tag::DESCRIPTOR => Self::Descriptor { slot: reader.u32()?, attributes: Snap::unsnap(reader)? },
            transition_tag::DICTIONARY => Self::Dictionary { trigger: Snap::unsnap(reader)? },
            _ => return Err(SnapshotError("invalid snapshot shape transition".into())),
        })
    }
}

impl<H: Host> Vm<H> {
    /// The VM as a snapshot, which `restore_snapshot` makes again in a new VM. Refused while
    /// anything runs or waits: a frame, a job, a module load or an embedder's script.
    pub(crate) fn write_snapshot(
        &self,
        write_host: impl FnOnce(&H, &mut crate::SnapshotWriter<'_>) -> Result<(), String>,
    ) -> SnapshotResult<Vec<u8>> {
        let Self {
            native_microtask_running,
            host,
            specialized,
            heap,
            realm,
            object_proto: _,
            function_proto: _,
            array_proto: _,
            string_proto: _,
            array_buffer_proto: _,
            shared_array_buffer_proto: _,
            array_iterator_proto: _,
            typed_array_proto: _,
            uint8_array_proto: _,
            uint8_clamped_array_proto: _,
            uint16_array_proto: _,
            uint32_array_proto: _,
            int8_array_proto: _,
            int16_array_proto: _,
            int32_array_proto: _,
            bigint64_array_proto: _,
            biguint64_array_proto: _,
            float16_array_proto: _,
            float32_array_proto: _,
            float64_array_proto: _,
            data_view_proto: _,
            map_proto: _,
            set_proto: _,
            shadow_realm_proto: _,
            map_iterator_proto: _,
            set_iterator_proto: _,
            weak_map_proto: _,
            weak_set_proto: _,
            weak_ref_proto: _,
            kept_alive,
            finalization_registry_proto: _,
            iterator_proto: _,
            string_iterator_proto: _,
            regexp_string_iterator_proto: _,
            generator_proto: _,
            iterator_helper_proto: _,
            wrap_for_valid_iterator_proto: _,
            async_iterator_proto: _,
            async_generator_proto: _,
            async_from_sync_iterator_proto: _,
            regexp_proto: _,
            temporal_plain_date_proto: _,
            temporal_plain_time_proto: _,
            temporal_plain_month_day_proto: _,
            temporal_plain_year_month_proto: _,
            natives,
            atom_strings,
            frames,
            native_stack_frames,
            native_root_lists: _,
            gc_root_count: _,
            gc_owned_root_count: _,
            frame_pool: _,
            pending_call_stacks: _,
            lane_stack: _,
            execution_mode: _,
            execution_interrupt: _,
            wasm_reference_epoch: _,
            active_call_roots,
            with_stack,
            startup_intrinsics,
            with_scope_entered,
            test262_agent,
            shared_async_waiters,
            programs,
            program_cache_layouts: _,
            active_registers: _,
            lane_raised,
            lane_returned,
            active_layout: _,
            active_constants: _,
            active_field_entries: _,
            active_program: _,
            profile: _,
            shapes,
            shape_constructors,
            shape_accessors,
            transitions,
            shape_slot_memo: _,
            plain_copy_shapes: _,
            plain_assign_shapes: _,
            embedding_record_shapes: _,
            typed_array_geometry: _,
            symbol_write_proofs: _,
            symbol_load_proofs: _,
            private_site_homes: _,
            array_iteration_proof: _,
            function_name_shapes: _,
            typeof_names: _,
            closure_templates: _,
            enumerable_keys: _,
            capture_walks: _,
            shape_attribute_memo: _,
            name_atom_memo: _,
            atom_text,
            atoms: _,
            array_index_atoms: _,
            atom_sequence,
            regexp_subjects: _,
            regexp_matchers: _,
            atom_classes: _,
            dynamic_strings: _,
            symbol_registry,
            well_known_symbols,
            string_concats: _,
            string_atoms: _,
            recent_atom_strings: _,
            call_targets: _,
            field_caches: _,
            inherited_field_indices: _,
            inherited_field_caches: _,
            accessor_field_caches: _,
            field_add_indices: _,
            field_add_sets: _,
            megamorphic_field_indices: _,
            field_cache_writes: _,
            megamorphic_fields: _,
            length_atom,
            prototype_atom,
            instanceof_constructor_layouts: _,
            closure_shapes: _,
            iterator_result_shape: _,
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
            constructed_shapes: _,
            method_caches: _,
            megamorphic_methods: _,
            method_cache_atoms: _,
            #[cfg(feature = "profile-aggregate")]
                invalidated_methods: _,
            object_shapes: _,
            descriptors,
            function_values,
            wasm_exported_functions,
            private_homes,
            default_collation: _,
            wasm_js,
            direct_eval,
            direct_eval_var_program,
            parameter_eval,
            eval_script_context,
            deferred_dependency_batch,
            construct_target,
            // Each process draws its own sequence (`fresh_random_seed`).
            random_state: _,
            // Each process keeps its own budget.
            dynamic_functions_to_keep: _,
            lane_deferred: _,
            // Resolutions are the process's (its files), not the snapshot's.
            resolved_module_requests: _,
            host_beside_activations: _,
        } = self;
        let Realm {
            globals,
            global_lexical_declarations,
            global_lexical_bindings,
            immutable_global_lexical_bindings,
            global_lexical_states,
            parked,
            intrinsics,
            jobs,
            template_objects,
            promise,
        } = realm;
        let running = [
            ("a running frame", frames.is_empty() && native_stack_frames.is_empty() && active_call_roots.is_empty()),
            ("a with statement", with_stack.is_empty()),
            ("a pending job", jobs.is_empty() && kept_alive.is_empty() && !native_microtask_running),
            ("an Atomics waiter", shared_async_waiters.is_empty() && !test262_agent.installed),
            ("a lane completion", lane_raised.is_none() && lane_returned.is_none()),
            (
                "an eval or construction",
                !direct_eval
                    && direct_eval_var_program.is_none()
                    && !parameter_eval
                    && !eval_script_context
                    && !deferred_dependency_batch
                    && construct_target.is_none(),
            ),
            ("WebAssembly", wasm_exported_functions.is_empty()),
        ];
        if let Some((what, _)) = running.iter().find(|(_, quiescent)| !quiescent) {
            return Err(SnapshotError::unsupported(what));
        }
        let mut writer = Writer::new();
        writer.bool(*specialized);
        // Atoms first: the programs decode against the restored sequence.
        writer.len(atom_text.len());
        for name in atom_text.iter() {
            writer.str(name);
        }
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
        atom_strings.snap(&mut writer)?;
        startup_intrinsics.write_snapshot(&mut writer)?;
        writer.bool(*with_scope_entered);
        programs.write_snapshot(&mut writer)?;
        // The root shape is every table's first; the others follow their parents.
        writer.len(shapes.len());
        for shape in &shapes[1..] {
            writer.u32(shape.parent);
            shape.transition.snap(&mut writer)?;
            writer.u32(shape.storage_len);
            shape.dictionary_trigger.snap(&mut writer)?;
        }
        shape_constructors.snap(&mut writer)?;
        shape_accessors.snap(&mut writer)?;
        writer.u32s(&transitions.children().collect::<Vec<_>>());
        symbol_registry.snap(&mut writer)?;
        well_known_symbols.snap(&mut writer)?;
        for atom in [length_atom, prototype_atom, to_fixed_atom, to_precision_atom] {
            writer.u32(*atom);
        }
        runtime_atoms.snap(&mut writer)?;
        descriptors.snap(&mut writer)?;
        writer.len(function_values.len());
        for (key, handles) in function_values {
            key.snap(&mut writer)?;
            let live = handles.iter().filter_map(|handle| heap.weak_value(*handle)).collect::<Vec<_>>();
            writer.values(&live);
        }
        private_homes.snap(&mut writer)?;
        let wasm_js::WasmJsIntrinsics { module, instance, memory, table, global, js_tag } = wasm_js;
        [*module, *instance, *memory, *table, *global, *js_tag].snap(&mut writer)?;
        writer.value(*instanceof_has_instance_holder);
        write_host(host, &mut crate::SnapshotWriter { writer: &mut writer, heap }).map_err(SnapshotError)?;
        Ok(writer.finish())
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
        let mut reader = Reader::new((*bytes).as_ref(), Some(H::snapshot_bytes_read))?;
        self.specialized = reader.bool()?;
        self.read_atoms(&mut reader)?;
        self.heap = Heap::read_snapshot(&mut reader, self.heap.memory_policy())?;
        self.realm.globals = reader.value()?;
        self.realm.global_lexical_declarations = Snap::unsnap(&mut reader)?;
        self.realm.global_lexical_bindings = Snap::unsnap(&mut reader)?;
        self.realm.immutable_global_lexical_bindings = Snap::unsnap(&mut reader)?;
        self.realm.global_lexical_states = Snap::unsnap(&mut reader)?;
        self.realm.parked = Snap::unsnap(&mut reader)?;
        self.realm.intrinsics = Snap::unsnap(&mut reader)?;
        self.realm.template_objects = Snap::unsnap(&mut reader)?;
        self.realm.promise = promise::PromiseRuntime::read_snapshot(&mut reader)?;
        macro_rules! read_prototypes {
            ($($field:ident),*) => { $( self.$field = reader.value()?; )* };
        }
        builtin_prototypes!(read_prototypes);
        self.natives = Snap::unsnap(&mut reader)?;
        self.atom_strings = Snap::unsnap(&mut reader)?;
        self.startup_intrinsics = startup_intrinsics::StartupIntrinsics::read_snapshot(&mut reader)?;
        self.with_scope_entered = reader.bool()?;
        self.programs = ProgramStore::read_snapshot(&mut reader, &bytes, &self.atom_sequence)?;
        self.read_shapes(&mut reader)?;
        self.symbol_registry = Snap::unsnap(&mut reader)?;
        self.well_known_symbols = Snap::unsnap(&mut reader)?;
        self.length_atom = reader.u32()?;
        self.prototype_atom = reader.u32()?;
        self.to_fixed_atom = reader.u32()?;
        self.to_precision_atom = reader.u32()?;
        self.runtime_atoms = Snap::unsnap(&mut reader)?;
        self.descriptors = Snap::unsnap(&mut reader)?;
        let function_values =
            read_list(&mut reader, |reader| Ok((<(ProgramId, u32)>::unsnap(reader)?, reader.values()?)))?;
        for (key, functions) in function_values {
            let handles = functions
                .into_iter()
                .map(|function| {
                    self.heap
                        .weak_handle(function)
                        .ok_or_else(|| SnapshotError("snapshot function cache names a dead value".into()))
                })
                .collect::<SnapshotResult<Vec<_>>>()?;
            self.function_values.insert(key, handles);
        }
        self.private_homes = Snap::unsnap(&mut reader)?;
        let [module, instance, memory, table, global, js_tag] = <[Value; 6]>::unsnap(&mut reader)?;
        self.wasm_js = wasm_js::WasmJsIntrinsics { module, instance, memory, table, global, js_tag };
        let holder = reader.value()?;
        self.set_instanceof_has_instance_holder(holder);
        read_host(&mut self.host, &mut crate::SnapshotReader { reader: &mut reader, heap: &self.heap })
            .map_err(SnapshotError)?;
        reader.finish()?;
        let main = self.rebased_entry(entry)?;
        self.programs.install_main(Rc::new(main.clone()));
        self.rebuild_program_cache_layouts();
        self.materialize_program_constants(ProgramId::MAIN, &main);
        self.set_active_program(ProgramId::MAIN);
        Ok(main)
    }

    /// The atom sequence `write_snapshot` wrote: the base table, then each appended name, which
    /// the VM indexes again as it appends them.
    fn read_atoms(&mut self, reader: &mut Reader<'_>) -> SnapshotResult<()> {
        let base = reader.len()?;
        let mut text = String::new();
        let mut ends = Vec::with_capacity(base);
        for _ in 0..base {
            text.push_str(reader.str()?);
            ends.push(u32::try_from(text.len()).map_err(|_| SnapshotError("snapshot atom text exceeds u32".into()))?);
        }
        let appended = reader.len()?;
        self.atom_text = AtomTable::new(text, ends);
        self.atom_sequence = Rc::new(crate::bytecode::AtomSequence::new(self.atom_text.clone()));
        // Sized for every atom at once: growing would hash every name again.
        self.atoms = hashbrown::HashTable::with_capacity(base + appended);
        self.array_index_atoms.clear();
        self.name_atom_memo.clear();
        self.private_site_homes.clear();
        self.atom_classes.clear();
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
        let children = reader.u32s()?;
        self.transitions = shape_transitions::ShapeTransitionIndex::with_capacity(children.len());
        for child in children {
            let indexed = self.shapes.get(child as usize).is_some_and(|shape| {
                shape_transitions_origin_key(&self.shapes, shape).is_some()
            });
            if !indexed {
                return Err(SnapshotError("snapshot transition names no cached shape".into()));
            }
            self.transitions.insert_child(&self.shapes, child);
        }
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
        self.field_caches.clear();
        self.megamorphic_field_indices.clear();
        self.field_add_indices.clear();
        self.inherited_field_indices.clear();
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
                    field_base: self.field_caches.len(),
                    method_base: self.method_caches.len(),
                    object_base: self.object_shapes.len(),
                    shared_uncached_field: None,
                }),
            }
        }
    }
}

/// Whether `shape`'s transition is one the transition index caches.
fn shape_transitions_origin_key(shapes: &[Shape], shape: &Shape) -> Option<ShapeTransitionKey> {
    match (shape.transition, shape.parent()) {
        (ShapeTransition::Dictionary { .. }, Some(parent)) => shapes[parent as usize].transition.cache_key(),
        (transition, _) => transition.cache_key(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{Engine, Host, NativeContext, RootId, RootedError, Runtime};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Global code a host runs while installing: what a snapshot keeps.
    const SETUP: &str = r#"
        var counter = 0;
        function bump() { return ++counter; }
        var pattern = /a(b+)c/g;
        var Point = class Point {
            #x;
            constructor(x) { this.#x = x; }
            get x() { return this.#x; }
            static make(x) { return new Point(x); }
        };
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
        print(readLexical(), pattern.exec('xabbc')[1], Point.make(4).x, typeof Point);
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
        fn finish_installation(context: &mut NativeContext<'_, Self>) -> Result<(), RootedError> {
            let callback = context.host_mut().resume_callback.take().expect("installed callback");
            let undefined = context.undefined();
            context.call_rooted(callback, undefined, &[])?;
            context.release_root(callback);
            Ok(())
        }
    }

    impl Host for SnapshotHost {
        fn write_line(&mut self, line: &str) {
            self.output.borrow_mut().push(line.into());
        }

        fn clock_millis(&mut self) -> f64 {
            0.0
        }

        fn initialize(context: &mut NativeContext<'_, Self>) -> Result<(), RootedError> {
            let callback = context.evaluate_script_rooted(SETUP, "setup.js")?;
            let callback = context.retain(callback)?;
            context.host_mut().resume_callback = Some(callback);
            if context.host_mut().building {
                return Ok(());
            }
            Self::finish_installation(context)
        }

        fn resume(context: &mut NativeContext<'_, Self>) -> Result<(), RootedError> {
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
