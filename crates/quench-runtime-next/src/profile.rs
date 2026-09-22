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
#[derive(Default)]
pub(crate) struct Profile {
    pub opcodes: Vec<u64>,
    pub pairs: Vec<u64>,
    pub pair_sites: rustc_hash::FxHashMap<(u32, u32), u64>,
    pub last_locations: Vec<Option<(u32, usize, usize)>>,
    pub functions: Vec<u64>,
    pub site_counts: Vec<Vec<u64>>,
    pub regional_binary_inputs: rustc_hash::FxHashMap<(u32, u32), [u64; 2]>,
    pub gc_frame_pcs: rustc_hash::FxHashMap<(u32, u32, bool), u64>,
    pub allocations: u64,
    pub collections: u64,
    pub peak_live: u64,
    pub peak_survivors: u64,
    pub max_gc_threshold: u64,
    pub shape_transition_hits: u64,
    pub shape_transition_misses: u64,
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
    pub operand_tags: [u64; 4],
    pub binary_ops: [u64; 20],
    pub binary_operand_modes: Vec<u64>,
    pub method_argc: [u64; 9],
    pub call_sources: [u64; 5],
    pub call_targets: [u64; 3],
    pub call_target_argc: [[u64; 9]; 3],
    pub numeric_argument_sources: [u64; 2],
    pub terminal_calls: [u64; 3],
    pub index_gets: [u64; 8],
    pub index_sets: [u64; 8],
    pub index_dispatches: [u64; 4],
    pub array_write_ownership: [u64; 2],
    pub numeric_binary_paths: [[u64; 20]; 3],
    pub branch_values: [[u64; 6]; 2],
    #[cfg(feature = "profile-trace")]
    pub trace: Vec<u8>,
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
    pub fn field_cache_hit(&mut self, tier: usize, depth: u8) {
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
                self.binary_operand_modes.resize(20 * 4 * 4, 0);
            }
            self.binary_operand_modes[op * 16 + left * 4 + right] += 1;
        }
        #[cfg(not(feature = "profile-aggregate"))]
        let _ = (op, left, right);
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
    pub fn numeric_arguments(&mut self, registers: bool) {
        #[cfg(feature = "profile-aggregate")]
        {
            self.numeric_argument_sources[usize::from(registers)] += 1;
        }
        #[cfg(not(feature = "profile-aggregate"))]
        let _ = registers;
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
            "{{\"kind\":\"rqj-profile\",\"allocations\":{},\"allocation_kinds\":{{\"names\":[\"object\",\"array\",\"map\",\"set\",\"iterator\",\"weak_map\",\"weak_set\",\"weak_ref\",\"function\",\"environment\",\"string\",\"bigint\",\"symbol\",\"date\",\"error\"],\"size_buckets\":[0,7,15,31,63,127,255,null],\"counts\":{:?},\"payload_bytes\":{:?},\"bucket_counts\":{:?}}},\"collections\":{},\"peak_live\":{},\"peak_survivors\":{},\"max_gc_threshold\":{},\"gc\":{{\"roots\":{},\"work_items\":{},\"max_worklist\":{},\"marked\":{},\"freed\":{},\"sweep_slots\":{},\"mark_nanos\":{},\"sweep_nanos\":{},\"marked_kinds\":{:?}}},\"shape_transitions\":{{\"hits\":{},\"misses\":{}}},\"field_cache\":{{\"hits\":{},\"misses\":{},\"tiers\":{:?},\"depths\":{:?}}},\"method_cache\":{{\"hits\":{},\"misses\":{},\"tiers\":{:?},\"refill_names\":[\"first\",\"post_gc\",\"post_mutation\"],\"refills\":{:?},\"same_target_names\":[\"gc\",\"mutation\"],\"same_targets\":{:?},\"invalidation_names\":[\"gc_candidates\",\"mutation_cleared\"],\"invalidation_entries\":{:?},\"dead_after_gc\":{}}},\"dynamic_atoms\":{},\"dynamic_strings\":{{\"hits\":{},\"misses\":{}}},\"string_concats\":{{\"coercing\":{},\"both_strings\":{},\"cache_hits\":{},\"cache_misses\":{},\"size_buckets\":{:?},\"max_bytes\":{}}},\"operand_tags\":{:?},\"binary_ops\":{:?},\"numeric_binary_paths\":{{\"names\":[\"fast_hit\",\"integer_operator_miss\",\"type_miss\"],\"counts\":{:?}}},\"branch_values\":{{\"names\":[\"undefined\",\"null\",\"boolean\",\"integer\",\"double\",\"heap\"],\"outcome_names\":[\"falsey\",\"truthy\"],\"counts\":{:?}}},\"method_argc\":{:?},\"calls\":{{\"source_names\":[\"dynamic\",\"known\",\"method\",\"this_method\",\"construct\"],\"sources\":{:?},\"target_names\":[\"native\",\"user\",\"numeric_user\"],\"targets\":{:?},\"target_argc\":{:?},\"numeric_argument_names\":[\"values\",\"registers\"],\"numeric_arguments\":{:?}}},\"terminal_calls\":{:?},\"indexed_access\":{{\"get_names\":[\"int_dense\",\"int_sparse\",\"int_missing\",\"wide_dense\",\"wide_sparse\",\"wide_missing\",\"numeric_non_array\",\"property\"],\"gets\":{:?},\"set_names\":[\"int_replace\",\"int_grow\",\"int_sparse\",\"wide_replace\",\"wide_grow\",\"wide_sparse\",\"numeric_non_array\",\"property\"],\"sets\":{:?},\"dispatch_names\":[\"get_general\",\"get_numeric\",\"set_general\",\"set_numeric\"],\"dispatches\":{:?}}},\"array_writes\":{{\"names\":[\"unique\",\"shared\"],\"counts\":{:?}}},\"dispatch_classes\":[{},{}],\"opcodes\":{{",
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
            self.numeric_argument_sources,
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
            let path = std::env::var_os("RQJ_TRACE").unwrap_or_else(|| "rqj.trace".into());
            let mut data = b"P1TR\x01\0\0\0".to_vec();
            data.extend_from_slice(&self.trace);
            if let Err(error) = std::fs::write(path, data) {
                eprintln!("rqj: cannot write trace: {error}");
            }
        }
    }

    #[cfg(not(feature = "profile-aggregate"))]
    pub fn report(&mut self, _: &crate::heap::Heap, _: &crate::bytecode::ResidualProgram) {}
}
