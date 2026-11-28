use super::*;

impl Vm {
    pub(crate) fn new<T: crate::Host>(host: T) -> Self {
        Self::new_with_execution_interrupt(host, crate::ExecutionInterrupt::default())
    }

    /// A VM that runs only code its image archived: it installs no JIT routes and no source
    /// routes, so a static executable links neither compiler.
    pub(crate) fn new_static<T: crate::Host>(host: T) -> Self {
        Self::with_statics(host, crate::ExecutionInterrupt::default(), crate::host::HostStatics::of::<T>(), &ExecutionTable::ALL)
    }

    /// `new_static` for an image that proved some intrinsic families unreachable: the realm
    /// installs only `builtins`' families, and a family whose installer is `None` keeps its
    /// code out of the image. `execution` is the image's engine entries (`ExecutionTable`).
    pub(crate) fn new_static_with_builtins<T: crate::Host>(host: T, builtins: &'static super::BuiltinTable, execution: &'static ExecutionTable) -> Self {
        let mut vm = Self::with_statics(host, crate::ExecutionInterrupt::default(), crate::host::HostStatics::of::<T>(), execution);
        vm.builtins = builtins;
        if let Some(natives) = builtins.natives {
            vm.native_table = super::native_table::NativeTable::closed(natives);
        }
        vm
    }

    /// A VM only ever restored from a runtime saved at rest (`HostStatics::at_rest`).
    pub(crate) fn new_at_rest<T: crate::Host>(host: T) -> Self {
        Self::with_statics(host, crate::ExecutionInterrupt::default(), crate::host::HostStatics::at_rest::<T>(), &ExecutionTable::ALL)
    }

    pub(crate) fn new_restored<T: crate::Host>(host: T) -> Self {
        Self::with_statics(host, crate::ExecutionInterrupt::default(), crate::host::HostStatics::restored::<T>(), &ExecutionTable::ALL)
    }

    pub(crate) fn new_with_execution_interrupt<T: crate::Host>(
        host: T,
        execution_interrupt: crate::ExecutionInterrupt,
    ) -> Self {
        super::layer_hooks::enable_jit();
        super::layer_hooks::install_intl();
        // A build without the WebAssembly frontend (`wasm` off) links none of its JS API.
        #[cfg(feature = "wasm")]
        super::wasm_js::enable_wasm_js();
        // A static image's support (no `source`) links no parser: its routes never install.
        #[cfg(feature = "source")]
        crate::compile::source_routes::enable();
        Self::with_statics(host, execution_interrupt, crate::host::HostStatics::of::<T>(), &ExecutionTable::ALL)
    }

    fn with_statics<T: crate::Host>(
        host: T,
        execution_interrupt: crate::ExecutionInterrupt,
        host_statics: crate::host::HostStatics,
        execution: &'static ExecutionTable,
    ) -> Self {
        let runs_lane = host.runs_lane();
        let mut vm = Self {
            host: Box::new(host),
            host_statics,
            runs_lane,
            builtins: &BuiltinTable::ALL,
            execution,
            typed_views: Default::default(),
            native_microtask_running: false,
            queue_resumed: Value::UNDEFINED,
            unobserved_jobs_entered: false,
            declined_fast: None,
            declined_resume: None,
            job_queue_programs: Vec::new(),
            queue_chains: false,
            chainable_job: false,
            specialized: true,
            heap: Heap::new(),
            realm: Realm {
                globals: Value::NULL,
                global_lexical_declarations: super::AtomSet::default(),
                global_lexical_bindings: Default::default(),
                immutable_global_lexical_bindings: FxHashSet::default(),
                global_lexical_states: FxHashMap::default(),
                parked: None,
                intrinsics: RealmIntrinsics::default(),
                jobs: std::collections::VecDeque::new(),
                template_objects: FxHashMap::default(),
                promise: Default::default(),
            },
            object_proto: Value::NULL,
            function_proto: Value::NULL,
            array_proto: Value::NULL,
            string_proto: Value::NULL,
            promise_proto: Value::NULL,
            promise_constructor: Value::NULL,
            number_proto: Value::NULL,
            array_buffer_proto: Value::NULL,
            shared_array_buffer_proto: Value::NULL,
            array_iterator_proto: Value::NULL,
            typed_array_proto: Value::NULL,
            typed_array_protos: [Value::NULL; crate::heap::TypedArrayKind::COUNT],
            data_view_proto: Value::NULL,
            map_proto: Value::NULL,
            set_proto: Value::NULL,
            shadow_realm_proto: Value::NULL,
            map_iterator_proto: Value::NULL,
            set_iterator_proto: Value::NULL,
            weak_map_proto: Value::NULL,
            weak_set_proto: Value::NULL,
            weak_ref_proto: Value::NULL,
            kept_alive: rustc_hash::FxHashMap::default(),
            finalization_registry_proto: Value::NULL, iterator_proto: Value::NULL,
            string_iterator_proto: Value::NULL, regexp_string_iterator_proto: Value::NULL,
            generator_proto: Value::NULL, iterator_helper_proto: Value::NULL,
            wrap_for_valid_iterator_proto: Value::NULL, async_iterator_proto: Value::NULL,
            async_generator_proto: Value::NULL, async_from_sync_iterator_proto: Value::NULL,
            regexp_proto: Value::NULL, temporal_plain_date_proto: Value::NULL,
            temporal_plain_time_proto: Value::NULL, temporal_plain_month_day_proto: Value::NULL,
            temporal_plain_year_month_proto: Value::NULL,
            natives: Default::default(),
            main_realm: Value::NULL,
            frames: vec![],
            native_stack_frames: vec![],
            native_root_lists: vec![],
            gc_owned_root_count: 0,
            frame_pool: vec![],
            lowered_tier0: Default::default(),
            pending_call_stacks: vec![],
            js_native_contexts: Vec::new(),
            native_roots: crate::vm::engine_types::NativeRoots::new(),
            lane_stack: Default::default(),
            lane_roots: Vec::new(),
            spare_lane_stacks: Vec::new(),
            execution_mode: ExecutionMode::default(),
            lane_tiers_run: None,
            native_execution_rejections: 0,
            js_tier_pending: FxHashMap::default(),
            #[cfg(feature = "native-execution")]
            native_program_resolver: None,
            execution_interrupt,
            active_call_roots: vec![],
            active_roots: vec![],
            realm_native_proofs: super::iterators::RealmNativeProofs::new(),
            prepared_roots: vec![],
            step_native_proofs: Default::default(),
            with_stack: vec![],
            with_scope_entered: false, scope_names: Default::default(),
            test262_agent: Default::default(),
            shared_async_waiters: Vec::new(),
            programs: ProgramStore::default(),
            program_cache_layouts: Vec::new(),
            free_cache_layouts: FxHashMap::default(),
            active_registers: std::ptr::null_mut(),
            pending_row: None,
            row_stop: None,
            lane_completion: LaneCompletion::None,
            lane_raise_site: 0,
            lane_deferred: None, running_window: None,
            resolved_module_requests: Default::default(),
            resolved_module_names: Default::default(),
            lane_native: None,
            active_context: std::ptr::null(),
            program_contexts: Vec::new(),
            idle_context: Box::new(ProgramContext::idle()),
            active_field_entries: std::ptr::null(),
            cache_epochs: Default::default(),
            active_program: ProgramId::MAIN,
            profile: Default::default(),
            shape_slot_memo: Default::default(),
            shape_attribute_memo: Default::default(),
            plain_copy_shapes: Default::default(),
            plain_assign_shapes: Default::default(),
            adopted_shapes: Default::default(),
            embedding_record_shapes: Default::default(),
            typed_array_geometry: Default::default(),
            function_name_shapes: Default::default(),
            symbol_write_proofs: Default::default(),
            symbol_load_proofs: Default::default(),
            private_site_homes: Default::default(),
            private_classes: Default::default(),
            array_iteration_shape: None,
            collection_iteration_shape: None,
            array_species_proof: None,
            concat_spreadable_proof: None,
            for_in: Default::default(),
            json_plain: Default::default(),
            species_slot_proof: Default::default(),
            iterator_absence_proof: Default::default(),
            then_absence_proof: None,
            promise_species_proof: Default::default(),
            promise_then_memo: Default::default(),
            enumerable_keys: Default::default(),
            enumerable_key_lists: Vec::new(),
            enumerable_key_value_lists: Vec::new(),
            name_atom_memo: Default::default(),
            shapes: Shape::table(),
            shape_constructors: Vec::new(),
            shape_accessors: Vec::new(),
            transitions: Default::default(),
            atom_text: AtomTable::default(),
            atoms: hashbrown::HashTable::new(),
            array_index_atoms: IndexAtoms::default(),
            startup_intrinsics: Default::default(),
            atom_sequence: Rc::new(crate::bytecode::AtomSequence::new(Default::default())),
            regexp_subjects: Default::default(),
            concat_memo: Default::default(),
            regexp_matchers: Default::default(),
            atom_classes: vec![],
            native_table: super::native_table::NativeTable::all(),
            first_chunks: Default::default(), streamed_stubs: Default::default(),
            dynamic_strings: None,
            typeof_names: [Value::UNDEFINED; super::TYPEOF_NAMES],
            symbol_registry: FxHashMap::default(),
            well_known_symbols: WellKnownSymbols::default(),
            bound_parts_layout: std::cell::Cell::new(None),
            call_targets: lazy_table::LazyTable::new(&NO_CALL_TARGETS),
            construct_targets: lazy_table::LazyEntries::new(super::EMPTY_CONSTRUCT_TARGET), frameless: Default::default(),
            field_caches: Default::default(),
            inherited_field_indices: Default::default(),
            accessor_field_indices: Default::default(),
            accessor_fields: Vec::new(),
            field_add_indices: Default::default(),
            field_add_sets: vec![], field_add_chains: Default::default(), json_parse_cache: Default::default(),
            keyed_adds: lazy_table::LazyEntries::new(FieldAddCache::EMPTY),
            megamorphic_field_indices: Default::default(),
            field_cache_writes: Some(Vec::new()),
            megamorphic_fields: vec![],
            field_stubs: Default::default(),
            holder_hints: Default::default(),
            length_atom: u32::MAX,
            prototype_atom: super::builtins::NO_CACHED_ATOM,
            instanceof_constructor_layouts: [None; INSTANCEOF_CONSTRUCTOR_LAYOUTS],
            in_presences: [None; in_presence::IN_PRESENCES],
            instanceof_inheriting_holders: [None; INSTANCEOF_CONSTRUCTOR_LAYOUTS],
            instanceof_proofs: std::array::from_fn(|_| (Value::UNDEFINED, crate::heap::ProtoLink::NULL, None)),
            closure_shapes: [None; 2],
            resolver_function_shape: None,
            bound_function_shape: None,
            closure_templates: lazy_table::LazyEntries::new(None),
            iterator_result_header: None,
            reused_iterator_result: Value::UNDEFINED,
            shared_entry_pair: Value::UNDEFINED,
            share_entry_pair: false,
            spare_await_state: Value::UNDEFINED,
            descriptor_shapes: [None; 2],
            descriptor_layout: None,
            regexp_object_header: None,
            regexp_last_index_layouts: Vec::new(),
            regexp_builtin_layouts: Vec::new(),
            regexp_symbol_layouts: Vec::new(),
            instanceof_has_instance_holder: Value::NULL,
            instanceof_has_instance_link: crate::heap::ProtoLink::NULL,
            capture_walks: lazy_table::LazyEntries::new(CaptureWalk::EMPTY),
            to_fixed_atom: u32::MAX,
            to_precision_atom: u32::MAX,
            runtime_atoms: RuntimeAtoms::default(),
            constructed_shapes: [provenance::ConstructedShape::EMPTY; provenance::CONSTRUCTED_SHAPE_ENTRIES],
            preshaped_receivers: lazy_table::LazyEntries::new(preshaped_receiver::PreshapedReceiver::EMPTY),
            learned_receivers: lazy_table::LazyEntries::new(preshaped_receiver::LearnedReceiver::EMPTY),
            object_templates: Vec::new(),
            method_caches: Default::default(),
            megamorphic_methods: vec![],
            method_cache_atoms: Default::default(),
            method_cache_sites: Default::default(),
            #[cfg(feature = "profile-aggregate")]
            invalidated_methods: FxHashMap::default(),
            object_shapes: Default::default(),
            descriptors: FxHashMap::default(),
            function_values: Default::default(),
            module_functions: Default::default(),
            wasm_exported_functions: FxHashMap::default(),
            private_homes: FxHashMap::default(),
            default_collation: None,
            wasm_js: Default::default(),
            direct_eval: false,
            eval_callers: Vec::new(),
            direct_eval_var_program: None,
            dynamic_functions_to_keep: super::error::DYNAMIC_FUNCTIONS_KEPT,
            parameter_eval: false,
            eval_script_context: false,
            deferred_dependency_batch: false,
            construct_target: None,
            random_state: fresh_random_seed(),
        };
        vm.active_context = &*vm.idle_context;
        vm.intern_typeof_names();
        // Every mode keeps slot representations: an image's typed field reads stand on the slots' types.
        vm.track_slots();

        vm
    }
}
