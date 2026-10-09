mod dependencies;
mod dispatch_opcodes;
#[cfg(feature = "profile-aggregate")]
mod dispatch_pairs;
#[cfg(feature = "profile-aggregate")]
mod instruction_words;
#[cfg(feature = "profile-aggregate")]
pub(crate) use instruction_words::fits as instruction_word_domains_fit;
#[cfg(feature = "profile-aggregate")]
mod method_cache;
#[cfg(feature = "profile-aggregate")]
mod regional;
#[cfg(feature = "profile-aggregate")]
mod root_maps;
mod virtual_opcode;

#[cfg(feature = "profile-aggregate")]
const BINARY_OPERATOR_COUNT: usize = oxc_ast::ast::BinaryOperator::Instanceof as usize + 1;

#[derive(Clone, Copy)]
#[repr(usize)]
pub(crate) enum BinaryValuePath {
    Fallback,
    IntegerFastPath,
    NumberFastPath,
}

#[cfg(feature = "profile-aggregate")]
impl BinaryValuePath {
    const COUNT: usize = Self::NumberFastPath as usize + 1;
    const NAMES: [&'static str; Self::COUNT] =
        ["fallback", "integer_fast_path", "number_fast_path"];
}

#[cfg(feature = "profile-aggregate")]
#[derive(Default)]
pub(crate) struct Profile {
    pub opcodes: Vec<u64>,
    pub dispatched_opcodes: Vec<u64>,
    pub pairs: Vec<u64>,
    pub pair_sites: rustc_hash::FxHashMap<(u32, u32), u64>,
    pub last_locations: Vec<Option<(u32, usize, usize)>>,
    pub functions: Vec<u64>,
    pub site_counts: Vec<Vec<u64>>,
    pub object_literal_site_counts: rustc_hash::FxHashMap<(u32, u32, usize), u64>,
    pub object_literal_dispatch_counts: rustc_hash::FxHashMap<(u32, usize), u64>,
    pub regional_binary_inputs: rustc_hash::FxHashMap<(u32, u32), [u64; 2]>,
    pub gc_frame_pcs: rustc_hash::FxHashMap<(u32, u32, bool), u64>,
    pub allocations: u64,
    pub collections: u64,
    pub peak_live: u64,
    pub peak_survivors: u64,
    pub max_gc_threshold: u64,
    pub shape_transition_hits: u64,
    pub shape_transition_misses: u64,
    #[cfg(feature = "profile-aggregate")]
    pub dictionary_transitions: [u64; crate::vm::DictionaryTrigger::COUNT],
    pub dynamic_atoms: u64,
    pub dynamic_string_hits: u64,
    pub dynamic_string_misses: u64,
    pub string_concats: [u64; 2],
    pub concat_size_buckets: [u64; 8],
    pub max_concat_bytes: u64,
    pub concat_cache_hits: u64,
    pub concat_cache_misses: u64,
    pub field_cache_hits: u64,
    pub field_cache_misses: u64,
    pub field_cache_tiers: [u64; 3],
    pub field_cache_depths: [u64; 3],
    pub method_cache_hits: u64,
    pub method_cache_misses: u64,
    pub method_cache_tiers: [u64; 3],
    pub method_cache_refills: [u64; 3],
    pub method_cache_same_targets: [u64; 2],
    pub method_cache_cleared: [u64; 2],
    pub method_cache_dead_after_gc: u64,
    pub operand_tags: [u64; crate::bytecode::OperandKind::COUNT],
    pub binary_ops: [u64; BINARY_OPERATOR_COUNT],
    pub binary_operand_modes: Vec<u64>,
    pub binary_value_paths: [[[[u64; BinaryValuePath::COUNT]; crate::value::ProfileKind::COUNT];
        crate::value::ProfileKind::COUNT]; BINARY_OPERATOR_COUNT],
    pub method_argc: [u64; 9],
    pub call_sources: [u64; 5],
    pub call_targets: [u64; 3],
    pub call_target_argc: [[u64; 9]; 3],
    pub terminal_calls: [u64; 3],
    pub index_gets: [u64; 8],
    pub index_sets: [u64; 8],
    pub index_dispatches: [u64; 4],
    pub array_write_ownership: [u64; 2],
    pub numeric_binary_paths: [[u64; BINARY_OPERATOR_COUNT]; 3],
    pub branch_values: [[u64; 6]; 2],
    #[cfg(feature = "profile-trace")]
    pub trace: Vec<u8>,
}

#[cfg(feature = "profile-aggregate")]
struct ObjectLiteralSiteExecution {
    function: usize,
    pc: usize,
    dispatch_op: crate::bytecode::Op,
    site_op: crate::bytecode::Op,
    site: usize,
    key_count: usize,
    executions: u64,
}

#[cfg(feature = "profile-aggregate")]
fn object_literal_site(
    encoded: crate::bytecode::Instr,
    function: &crate::bytecode::Function,
    program: &crate::bytecode::ResidualProgram,
) -> Option<(crate::bytecode::Op, crate::bytecode::Op, usize)> {
    use crate::bytecode::Op;

    let (op, site) = if encoded.is_wide() {
        let instruction = function.wide[encoded.wide_index()];
        match instruction.op() {
            Op::MakeObject2 | Op::MakeObjectLiteral => {
                (instruction.op(), instruction.object_site_index())
            }
            Op::SuperConstArrayObject2 => (instruction.op(), instruction.superinstruction_index()),
            _ => return None,
        }
    } else {
        match encoded.op() {
            Op::MakeObject2 | Op::MakeObjectLiteral => (encoded.op(), encoded.object_site_index()),
            Op::SuperConstArrayObject2 => (encoded.op(), encoded.superinstruction_index()),
            _ => return None,
        }
    };
    let object =
        (op == Op::SuperConstArrayObject2).then(|| program.superinstructions[site].code[3]);
    let site_op = object.map_or(op, |instruction| instruction.op());
    let site = object.map_or(site, |instruction| instruction.object_site_index());
    Some((op, site_op, site))
}

#[cfg(feature = "profile-aggregate")]
fn object_literal_site_executions(
    profile: &Profile,
    program_id: u32,
    program: &crate::bytecode::ResidualProgram,
) -> Vec<ObjectLiteralSiteExecution> {
    let mut sites = Vec::new();
    for (function_id, function) in program.functions.iter().enumerate() {
        for (pc, encoded) in function.code.iter().copied().enumerate() {
            let Some((dispatch_op, site_op, site)) =
                object_literal_site(encoded, function, program)
            else {
                continue;
            };
            sites.push(ObjectLiteralSiteExecution {
                function: function_id,
                pc,
                dispatch_op,
                site_op,
                site,
                key_count: program.object_sites[site].atoms.len(),
                executions: profile
                    .object_literal_site_counts
                    .get(&(program_id, function_id as u32, pc))
                    .copied()
                    .unwrap_or_default(),
            });
        }
    }
    sites
}

#[cfg(not(feature = "profile-aggregate"))]
#[derive(Default)]
pub(crate) struct Profile;

impl Profile {
    #[cfg(not(feature = "profile-aggregate"))]
    #[inline(always)]
    pub fn function(&mut self, _id: usize) {}

    #[cfg(feature = "profile-aggregate")]
    #[inline(always)]
    pub fn opcode(&mut self, opcode: usize, frame: usize, function: u32, pc: usize) {
        self.site(function, pc);
        if self.dispatched_opcodes.len() <= opcode {
            self.dispatched_opcodes.resize(opcode + 1, 0);
        }
        self.dispatched_opcodes[opcode] = self.dispatched_opcodes[opcode].saturating_add(1);
        if self.opcodes.len() <= opcode {
            self.opcodes.resize(opcode + 1, 0);
        }
        self.opcodes[opcode] = self.opcodes[opcode].saturating_add(1);
        if self.pairs.len() < crate::bytecode::Op::COUNT * crate::bytecode::Op::COUNT {
            self.pairs
                .resize(crate::bytecode::Op::COUNT * crate::bytecode::Op::COUNT, 0);
        }
        if self.last_locations.len() <= frame {
            self.last_locations.resize(frame + 1, None);
        }
        if let Some((last_function, last_pc, last_opcode)) = self.last_locations[frame]
            && last_function == function
            && last_pc + 1 == pc
        {
            self.pairs[last_opcode * crate::bytecode::Op::COUNT + opcode] += 1;
            *self
                .pair_sites
                .entry((function, last_pc as u32))
                .or_default() += 1;
        }
        self.last_locations[frame] = Some((function, pc, opcode));
        #[cfg(feature = "profile-trace")]
        if self.trace.len() < 4_000_000 {
            self.trace.extend_from_slice(&(opcode as u32).to_le_bytes());
        }
    }

    #[cfg(feature = "profile-aggregate")]
    pub fn report_dispatch_census_if_enabled(&self) -> bool {
        if std::env::var_os("QUENCH_OPCODE_CENSUS").is_none() {
            return false;
        }
        let total = self.dispatched_opcodes.iter().sum::<u64>();
        let site_total = self.site_counts.iter().flatten().sum::<u64>();
        assert_eq!(total, site_total, "dispatch opcode/site counters diverged");
        eprint!(
            "{{\"kind\":\"quench-dispatch-opcode-census\",\"total\":{total},\"site_total\":{site_total},\"binary_value_paths\":{{\"operator_order\":\"oxc_ast::ast::BinaryOperator discriminant\",\"value_tag_order\":{:?},\"path_order\":{:?},\"counts\":{:?}}},\"counts\":{{",
            crate::value::ProfileKind::NAMES,
            BinaryValuePath::NAMES,
            self.binary_value_paths,
        );
        for opcode in 0..crate::bytecode::Op::COUNT {
            if opcode != 0 {
                eprint!(",");
            }
            eprint!(
                "\"{}\":{}",
                crate::bytecode::Op::NAMES[opcode],
                self.dispatched_opcodes.get(opcode).copied().unwrap_or(0)
            );
        }
        eprintln!("}}}}");
        true
    }

    #[cfg(feature = "profile-aggregate")]
    pub fn object_literal_instruction(
        &mut self,
        program_id: u32,
        function_id: u32,
        pc: usize,
        opcode: crate::bytecode::Op,
    ) {
        use crate::bytecode::Op;

        if !matches!(
            opcode,
            Op::MakeObject2 | Op::MakeObjectLiteral | Op::SuperConstArrayObject2
        ) {
            return;
        }
        *self
            .object_literal_site_counts
            .entry((program_id, function_id, pc))
            .or_default() += 1;
        *self
            .object_literal_dispatch_counts
            .entry((program_id, opcode as usize))
            .or_default() += 1;
    }

    #[cfg(feature = "profile-aggregate")]
    fn report_object_literal_sites_for_program(
        &self,
        program_id: u32,
        program: &crate::bytecode::ResidualProgram,
    ) {
        eprint!(
            "{{\"kind\":\"quench-object-literal-sites\",\"program_id\":{program_id},\"dispatch_counts\":{{"
        );
        for (index, opcode) in [
            crate::bytecode::Op::MakeObject2,
            crate::bytecode::Op::MakeObjectLiteral,
            crate::bytecode::Op::SuperConstArrayObject2,
        ]
        .into_iter()
        .enumerate()
        {
            if index > 0 {
                eprint!(",");
            }
            let count = self
                .object_literal_dispatch_counts
                .get(&(program_id, opcode as usize))
                .copied()
                .unwrap_or_default();
            eprint!(
                "\"{}\":{count}",
                crate::bytecode::Op::NAMES[opcode as usize]
            );
        }
        eprint!("}},\"sites\":[");
        for (index, site) in object_literal_site_executions(self, program_id, program)
            .iter()
            .enumerate()
        {
            if index > 0 {
                eprint!(",");
            }
            eprint!(
                "{{\"function\":{},\"pc\":{},\"dispatch_op\":\"{}\",\"site_op\":\"{}\",\"site\":{},\"key_count\":{},\"executions\":{}}}",
                site.function,
                site.pc,
                crate::bytecode::Op::NAMES[site.dispatch_op as usize],
                crate::bytecode::Op::NAMES[site.site_op as usize],
                site.site,
                site.key_count,
                site.executions,
            );
        }
        eprintln!("]}}");
    }

    #[cfg(feature = "profile-aggregate")]
    pub fn report_object_literal_sites(
        &self,
        program_id: u32,
        program: &crate::bytecode::ResidualProgram,
    ) {
        self.report_object_literal_sites_for_program(program_id, program);
    }

    #[cfg(not(feature = "profile-aggregate"))]
    #[inline(always)]
    pub fn opcode(&mut self, _opcode: usize) {}

    #[inline(always)]
    pub fn shape_transition(&mut self, hit: bool) {
        #[cfg(feature = "profile-aggregate")]
        if hit {
            self.shape_transition_hits += 1;
        } else {
            self.shape_transition_misses += 1;
        }
        #[cfg(not(feature = "profile-aggregate"))]
        let _ = hit;
    }

    #[inline(always)]
    pub fn dictionary_transition(&mut self, trigger: crate::vm::DictionaryTrigger) {
        #[cfg(feature = "profile-aggregate")]
        {
            self.dictionary_transitions[trigger.index()] += 1;
        }
        #[cfg(not(feature = "profile-aggregate"))]
        let _ = trigger;
    }

    #[inline(always)]
    pub fn dynamic_atom(&mut self) {
        #[cfg(feature = "profile-aggregate")]
        {
            self.dynamic_atoms += 1;
        }
    }

    #[cfg(feature = "profile-aggregate")]
    #[inline(always)]
    pub fn dynamic_string(&mut self, hit: bool) {
        if hit {
            self.dynamic_string_hits += 1;
        } else {
            self.dynamic_string_misses += 1;
        }
    }

    #[cfg(feature = "profile-aggregate")]
    #[inline(always)]
    pub fn string_concat(&mut self, both_strings: bool) {
        self.string_concats[usize::from(both_strings)] += 1;
    }

    #[cfg(feature = "profile-aggregate")]
    #[inline(always)]
    pub fn string_concat_size(&mut self, bytes: usize) {
        let bucket = match bytes {
            0 => 0,
            1..=7 => 1,
            8..=15 => 2,
            16..=31 => 3,
            32..=63 => 4,
            64..=127 => 5,
            128..=255 => 6,
            _ => 7,
        };
        self.concat_size_buckets[bucket] += 1;
        self.max_concat_bytes = self.max_concat_bytes.max(bytes as u64);
    }

    #[cfg(feature = "profile-aggregate")]
    #[inline(always)]
    pub fn concat_cache(&mut self, hit: bool) {
        if hit {
            self.concat_cache_hits += 1;
        } else {
            self.concat_cache_misses += 1;
        }
    }

    #[inline(always)]
    pub fn field_cache(&mut self, hit: bool) {
        #[cfg(feature = "profile-aggregate")]
        if hit {
            self.field_cache_hits += 1;
        } else {
            self.field_cache_misses += 1;
        }
        #[cfg(not(feature = "profile-aggregate"))]
        let _ = hit;
    }

    #[inline(always)]
    pub fn field_cache_hit(&mut self, tier: usize, depth: u16) {
        #[cfg(feature = "profile-aggregate")]
        {
            self.field_cache_hits += 1;
            self.field_cache_tiers[tier] += 1;
            self.field_cache_depths[usize::from(depth).min(2)] += 1;
        }
        #[cfg(not(feature = "profile-aggregate"))]
        let _ = (tier, depth);
    }

    #[inline(always)]
    pub fn method_cache(&mut self, hit: bool) {
        #[cfg(feature = "profile-aggregate")]
        if hit {
            self.method_cache_hits += 1;
        } else {
            self.method_cache_misses += 1;
        }
        #[cfg(not(feature = "profile-aggregate"))]
        let _ = hit;
    }

    #[cfg(feature = "profile-aggregate")]
    #[inline(always)]
    pub fn method_cache_tier(&mut self, tier: Option<usize>) {
        if let Some(tier) = tier {
            self.method_cache_tiers[tier] += 1;
        }
    }

    #[inline(always)]
    pub fn operand(&mut self, tag: usize) {
        #[cfg(feature = "profile-aggregate")]
        {
            self.operand_tags[tag] += 1;
        }
        #[cfg(not(feature = "profile-aggregate"))]
        let _ = tag;
    }

    #[inline(always)]
    pub fn binary(&mut self, op: usize, left: u16, right: u16) {
        #[cfg(feature = "profile-aggregate")]
        {
            self.binary_ops[op] += 1;
            let left = crate::bytecode::Operand(left).tag() as usize;
            let right = crate::bytecode::Operand(right).tag() as usize;
            if self.binary_operand_modes.is_empty() {
                self.binary_operand_modes.resize(
                    BINARY_OPERATOR_COUNT
                        * crate::bytecode::OperandKind::COUNT
                        * crate::bytecode::OperandKind::COUNT,
                    0,
                );
            }
            let operand_kinds = crate::bytecode::OperandKind::COUNT;
            self.binary_operand_modes
                [op * operand_kinds * operand_kinds + left * operand_kinds + right] += 1;
        }
        #[cfg(not(feature = "profile-aggregate"))]
        let _ = (op, left, right);
    }

    #[cfg(feature = "profile-aggregate")]
    #[inline(always)]
    pub fn binary_value_path(
        &mut self,
        op: usize,
        left: crate::value::ProfileKind,
        right: crate::value::ProfileKind,
        path: BinaryValuePath,
    ) {
        self.binary_value_paths[op][left as usize][right as usize][path as usize] += 1;
    }

    #[inline(always)]
    pub fn method_args(&mut self, argc: usize) {
        #[cfg(feature = "profile-aggregate")]
        {
            self.method_argc[argc.min(8)] += 1;
        }
        #[cfg(not(feature = "profile-aggregate"))]
        let _ = argc;
    }

    #[inline(always)]
    pub fn call_source(&mut self, source: usize) {
        #[cfg(feature = "profile-aggregate")]
        {
            self.call_sources[source] += 1;
        }
        #[cfg(not(feature = "profile-aggregate"))]
        let _ = source;
    }

    #[inline(always)]
    pub fn call_target(&mut self, target: usize, argc: usize) {
        #[cfg(feature = "profile-aggregate")]
        {
            self.call_targets[target] += 1;
            self.call_target_argc[target][argc.min(8)] += 1;
        }
        #[cfg(not(feature = "profile-aggregate"))]
        let _ = (target, argc);
    }

    #[inline(always)]
    pub fn terminal_call(&mut self, kind: usize) {
        #[cfg(feature = "profile-aggregate")]
        {
            self.terminal_calls[kind] += 1;
        }
        #[cfg(not(feature = "profile-aggregate"))]
        let _ = kind;
    }

    #[cfg(feature = "profile-aggregate")]
    #[inline(always)]
    pub fn index_get(&mut self, kind: usize) {
        self.index_gets[kind] += 1;
    }

    #[cfg(feature = "profile-aggregate")]
    #[inline(always)]
    pub fn index_set(&mut self, kind: usize) {
        self.index_sets[kind] += 1;
    }

    #[cfg(feature = "profile-aggregate")]
    #[inline(always)]
    pub fn array_write_ownership(&mut self, shared: bool) {
        self.array_write_ownership[usize::from(shared)] += 1;
    }

    #[cfg(feature = "profile-aggregate")]
    #[inline(always)]
    pub fn index_dispatch(&mut self, set: bool, numeric: bool) {
        self.index_dispatches[usize::from(set) * 2 + usize::from(numeric)] += 1;
    }

    #[cfg(feature = "profile-aggregate")]
    #[inline(always)]
    pub fn numeric_binary_path(&mut self, op: usize, hit: bool, integer_pair: bool) {
        let path = if hit {
            0
        } else if integer_pair {
            1
        } else {
            2
        };
        self.numeric_binary_paths[path][op] += 1;
    }

    #[cfg(feature = "profile-aggregate")]
    #[inline(always)]
    pub fn branch_value(&mut self, kind: usize, truthy: bool) {
        self.branch_values[usize::from(truthy)][kind] += 1;
    }

    #[cfg(feature = "profile-aggregate")]
    pub fn report(&mut self, heap: &crate::heap::Heap, program: &crate::bytecode::ResidualProgram) {
        let heap_stats = heap.stats();
        let gc = heap.gc_profile();
        self.allocations = heap_stats.0;
        self.collections = heap_stats.1;
        self.peak_live = heap_stats.2 as u64;
        self.peak_survivors = heap_stats.3 as u64;
        self.max_gc_threshold = heap_stats.4 as u64;
        let mut pairs: Vec<_> = self
            .pairs
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, n)| *n > 0)
            .collect();
        pairs.sort_unstable_by_key(|(_, n)| std::cmp::Reverse(*n));
        let mut pair_sites: Vec<_> = self.pair_sites.iter().collect();
        pair_sites.sort_unstable_by_key(|(_, count)| std::cmp::Reverse(**count));
        let mut binary_modes: Vec<_> = self
            .binary_operand_modes
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, count)| *count != 0)
            .collect();
        binary_modes.sort_unstable_by_key(|(_, count)| std::cmp::Reverse(*count));
        let numeric_dispatches = program
            .functions
            .iter()
            .filter(|function| function.dispatch == crate::bytecode::DispatchClass::Numeric)
            .count();
        let binary_dependencies = dependencies::binary_pairs(&self.pair_sites, program);
        regional::report(self, program);
        eprint!(
            "{{\"kind\":\"quench-profile\",\"allocations\":{},\"allocation_kinds\":{{\"names\":[\"object\",\"array\",\"map\",\"set\",\"iterator\",\"weak_map\",\"weak_set\",\"weak_ref\",\"function\",\"environment\",\"string\",\"bigint\",\"symbol\",\"date\",\"error\"],\"size_buckets\":[0,7,15,31,63,127,255,null],\"counts\":{:?},\"payload_bytes\":{:?},\"bucket_counts\":{:?}}},\"collections\":{},\"peak_live\":{},\"peak_survivors\":{},\"max_gc_threshold\":{},\"gc\":{{\"roots\":{},\"work_items\":{},\"max_worklist\":{},\"marked\":{},\"freed\":{},\"sweep_slots\":{},\"mark_nanos\":{},\"sweep_nanos\":{},\"marked_kinds\":{:?}}},\"shape_transitions\":{{\"hits\":{},\"misses\":{}}},\"dictionary_transitions\":{{\"names\":[\"property_count\",\"deletion_pattern\",\"prototype_use\"],\"counts\":{:?}}},\"field_cache\":{{\"hits\":{},\"misses\":{},\"tiers\":{:?},\"depths\":{:?}}},\"method_cache\":{{\"hits\":{},\"misses\":{},\"tiers\":{:?},\"refill_names\":[\"first\",\"post_gc\",\"post_mutation\"],\"refills\":{:?},\"same_target_names\":[\"gc\",\"mutation\"],\"same_targets\":{:?},\"invalidation_names\":[\"gc_candidates\",\"mutation_cleared\"],\"invalidation_entries\":{:?},\"dead_after_gc\":{}}},\"dynamic_atoms\":{},\"dynamic_strings\":{{\"hits\":{},\"misses\":{}}},\"string_concats\":{{\"coercing\":{},\"both_strings\":{},\"cache_hits\":{},\"cache_misses\":{},\"size_buckets\":{:?},\"max_bytes\":{}}},\"operand_tags\":{:?},\"binary_ops\":{:?},\"numeric_binary_paths\":{{\"names\":[\"fast_hit\",\"integer_operator_miss\",\"type_miss\"],\"counts\":{:?}}},\"branch_values\":{{\"names\":[\"undefined\",\"null\",\"boolean\",\"integer\",\"double\",\"heap\"],\"outcome_names\":[\"falsey\",\"truthy\"],\"counts\":{:?}}},\"method_argc\":{:?},\"calls\":{{\"source_names\":[\"dynamic\",\"known\",\"method\",\"this_method\",\"construct\"],\"sources\":{:?},\"target_names\":[\"native\",\"user\",\"numeric_user\"],\"targets\":{:?},\"target_argc\":{:?}}},\"terminal_calls\":{:?},\"indexed_access\":{{\"get_names\":[\"int_dense\",\"int_sparse\",\"int_missing\",\"wide_dense\",\"wide_sparse\",\"wide_missing\",\"numeric_non_array\",\"property\"],\"gets\":{:?},\"set_names\":[\"int_replace\",\"int_grow\",\"int_sparse\",\"wide_replace\",\"wide_grow\",\"wide_sparse\",\"numeric_non_array\",\"property\"],\"sets\":{:?},\"dispatch_names\":[\"get_general\",\"get_numeric\",\"set_general\",\"set_numeric\"],\"dispatches\":{:?}}},\"array_writes\":{{\"names\":[\"unique\",\"shared\"],\"counts\":{:?}}},\"dispatch_classes\":[{},{}],\"opcodes\":{{",
            self.allocations,
            gc.allocated_kinds,
            gc.allocated_payload_bytes,
            gc.allocated_size_buckets,
            self.collections,
            self.peak_live,
            self.peak_survivors,
            self.max_gc_threshold,
            gc.roots,
            gc.work_items,
            gc.max_worklist,
            gc.marked,
            gc.freed,
            gc.sweep_slots,
            gc.mark_nanos,
            gc.sweep_nanos,
            gc.marked_kinds,
            self.shape_transition_hits,
            self.shape_transition_misses,
            self.dictionary_transitions,
            self.field_cache_hits,
            self.field_cache_misses,
            self.field_cache_tiers,
            self.field_cache_depths,
            self.method_cache_hits,
            self.method_cache_misses,
            self.method_cache_tiers,
            self.method_cache_refills,
            self.method_cache_same_targets,
            self.method_cache_cleared,
            self.method_cache_dead_after_gc,
            self.dynamic_atoms,
            self.dynamic_string_hits,
            self.dynamic_string_misses,
            self.string_concats[0],
            self.string_concats[1],
            self.concat_cache_hits,
            self.concat_cache_misses,
            self.concat_size_buckets,
            self.max_concat_bytes,
            self.operand_tags,
            self.binary_ops,
            self.numeric_binary_paths,
            self.branch_values,
            self.method_argc,
            self.call_sources,
            self.call_targets,
            self.call_target_argc,
            self.terminal_calls,
            self.index_gets,
            self.index_sets,
            self.index_dispatches,
            self.array_write_ownership,
            program.functions.len() - numeric_dispatches,
            numeric_dispatches
        );
        for (index, count) in self.opcodes.iter().enumerate() {
            if index != 0 {
                eprint!(",");
            }
            eprint!("\"{}\":{}", crate::bytecode::Op::NAMES[index], count);
        }
        eprint!("}}");
        dispatch_opcodes::report(self, program);
        dispatch_pairs::report(self, program);
        eprint!(
            ",\"binary_pair_dependencies\":{{\"names\":[\"none\",\"left\",\"right\",\"both\"],\"counts\":{:?}}},\"top_pairs\":[",
            binary_dependencies
        );
        for (position, (id, count)) in pairs.iter().take(20).enumerate() {
            if position != 0 {
                eprint!(",");
            }
            eprint!(
                "[\"{}\",\"{}\",{}]",
                crate::bytecode::Op::NAMES[id / crate::bytecode::Op::COUNT],
                crate::bytecode::Op::NAMES[id % crate::bytecode::Op::COUNT],
                count
            );
        }
        eprint!("],\"binary_operand_modes\":[");
        for (position, (mode, count)) in binary_modes.iter().enumerate() {
            if position != 0 {
                eprint!(",");
            }
            eprint!("[{}, {}, {}, {}]", mode / 16, mode / 4 % 4, mode % 4, count);
        }
        eprint!("],\"top_pair_sites\":[");
        for (position, ((function, pc), count)) in pair_sites.iter().take(20).enumerate() {
            if position != 0 {
                eprint!(",");
            }
            let code = &program.functions[*function as usize].code;
            eprint!(
                "{{\"function\":{},\"pc\":{},\"first\":\"{}\",\"second\":\"{}\",\"count\":{}}}",
                function,
                pc,
                crate::bytecode::Op::NAMES[code[*pc as usize].op() as usize],
                crate::bytecode::Op::NAMES[code[*pc as usize + 1].op() as usize],
                count
            );
        }
        eprint!("]");
        instruction_words::report(program);
        root_maps::report(self, program);
        regional::report_functions(self, program);
        #[cfg(feature = "profile-trace")]
        {
            let path = std::env::var_os("QUENCH_TRACE").unwrap_or_else(|| "Quench.trace".into());
            let mut data = b"P1TR\x01\0\0\0".to_vec();
            data.extend_from_slice(&self.trace);
            if let Err(error) = std::fs::write(path, data) {
                eprintln!("Quench: cannot write trace: {error}");
            }
        }
    }

    #[cfg(not(feature = "profile-aggregate"))]
    pub fn report(&mut self, _: &crate::heap::Heap, _: &crate::bytecode::ResidualProgram) {}
}

#[cfg(all(test, feature = "profile-aggregate"))]
mod tests {
    use super::Profile;
    use crate::bytecode::Op;

    #[test]
    fn physical_dispatch_counts_exclude_virtual_fusion_steps() {
        let mut profile = Profile::default();
        profile.opcode(Op::NumericAdd as usize, 0, 0, 0);
        profile.virtual_opcode(Op::Binary as usize);

        assert_eq!(profile.dispatched_opcodes[Op::NumericAdd as usize], 1);
        assert_eq!(profile.dispatched_opcodes[Op::Binary as usize], 0);
        assert_eq!(profile.opcodes[Op::Binary as usize], 1);
    }
}
