//! The program store's part of a snapshot (`crate::snapshot`): every stored program at its id
//! with the heap values it roots. The main program is left out: a restoring VM installs its
//! own entry there. Lazily derived views (lane views, compiled tiers, root slot rules) are
//! rebuilt on first use, as for a program just inserted.

use super::*;
use crate::snapshot::{read_list, snapshot_enum, Reader, Snap, SnapshotError, SnapshotResult, Writer};

snapshot_enum!(program_scope: ProgramScope { Own, Global, GlobalVariables, Caller });

impl ProgramStore {
    pub(crate) fn write_snapshot(&self, writer: &mut Writer) -> SnapshotResult<()> {
        let Self {
            programs,
            global_eval_programs,
            owned_scripts,
            reclaim_passes,
            unchecked_reclaimable,
            // Compiled code is never in a snapshot: its budgets and memos start over.
            js_jit_tracked_functions: _,
            js_jit_tracking_exhaustions: _,
            js_jit_rejected_functions: _,
            jit_code_cache: _,
            js_jit_diagnostics: _,
            // Process configuration a restoring store reads again, and its diagnostics.
            general_baseline_priced: _,
            js_jit_general_dispatch_enabled: _,
            // Hotness counters are process-local and start fresh after restore.
            js_general_dispatch_work: _,
            js_jit_dataview_float64_enabled: _,
            js_jit_dataview_float64_get_entries: _,
            js_jit_dataview_float64_set_entries: _,
            ..
        } = self;
        if !owned_scripts.is_empty() {
            return Err(SnapshotError::unsupported("scripts an embedder compiled and holds"));
        }
        writer.len(programs.len());
        for (index, entry) in programs.iter().enumerate() {
            match entry {
                Some(entry) if index != ProgramId::MAIN.index() => {
                    writer.bool(true);
                    entry.write_snapshot(writer)?;
                }
                _ => writer.bool(false),
            }
        }
        writer.len(global_eval_programs.len());
        for (source, program) in global_eval_programs {
            source.write_snapshot(writer);
            writer.u32(program.raw());
        }
        writer.u64(*reclaim_passes);
        writer.len(*unchecked_reclaimable);
        Ok(())
    }

    /// The store `write_snapshot` wrote, whose programs keep their records in `bytes` (the
    /// snapshot `reader` reads) and share `atoms`. The main program's slot is left empty.
    pub(crate) fn read_snapshot(
        reader: &mut Reader<'_>,
        bytes: &crate::KeptBytes,
        atoms: &Rc<crate::bytecode::AtomSequence>,
    ) -> SnapshotResult<Self> {
        let count = reader.len()?;
        let mut programs = Vec::with_capacity(count);
        for _ in 0..count {
            programs.push(if reader.bool()? { Some(ProgramEntry::read_snapshot(reader, bytes, atoms)?) } else { None });
        }
        let global_eval_programs = read_list(reader, |reader| {
            Ok((JsString::read_snapshot(reader)?, ProgramId::from_raw(reader.u32()?)))
        })?
        .into_iter()
        .collect();
        let mut store = Self::default();
        store.programs = programs;
        store.global_eval_programs = global_eval_programs;
        store.reclaim_passes = reader.u64()?;
        store.unchecked_reclaimable = reader.len()?;
        Ok(store)
    }
}

impl ProgramEntry {
    fn write_snapshot(&self, writer: &mut Writer) -> SnapshotResult<()> {
        let Self {
            residual,
            wasm_signatures,
            constants,
            const_arrays,
            function_sources,
            regexp_literals: _,
            module_environment,
            import_meta,
            module_imports,
            root_slots: _,
            module,
            lane_views: _,
            js_entry_facts: _,
            lane_deferred: _,
            compiled: _,
            jit_tiers: _,
            scope,
            origin,
            script_or_module,
            reclaimable,
            pass,
            roots_pending: _,
        } = self;
        if wasm_signatures.is_some() || residual.kind == crate::bytecode::ProgramKind::Wasm {
            return Err(SnapshotError::unsupported("WebAssembly"));
        }
        writer.bool(residual.source.is_some());
        if let Some(source) = &residual.source {
            source.text.write_snapshot(writer);
        }
        // The sizes a restored entry needs before its residual is decoded.
        writer.len(residual.regexp_literal_sites.len());
        writer.len(residual.functions.len());
        let sites = residual.cache_site_counts();
        writer.u16(sites.fields);
        writer.len(sites.methods);
        writer.len(sites.objects);
        writer.bytes(&residual.encode_snapshot());
        writer.values(constants);
        writer.len(const_arrays.len());
        for array in const_arrays {
            writer.bool(array.is_some());
            if let Some(array) = array {
                writer.values(array);
            }
        }
        function_sources.snap(writer)?;
        module_environment.snap(writer)?;
        import_meta.snap(writer)?;
        writer.len(module_imports.len());
        for (slot, import) in module_imports {
            writer.u16(*slot);
            match import {
                ModuleImport::Value(value) => {
                    writer.u8(0);
                    writer.value(*value);
                }
                ModuleImport::Binding(program, slot, environment) => {
                    writer.u8(1);
                    writer.u32(program.raw());
                    writer.u16(*slot);
                    writer.value(*environment);
                }
            }
        }
        writer.bool(*module);
        program_scope::write(writer, *scope);
        match origin {
            CompilationOrigin::Source => writer.u8(0),
            CompilationOrigin::SharedScript { source } => {
                writer.u8(1);
                writer.u32(source.raw());
            }
            CompilationOrigin::Eval { caller } => {
                writer.u8(2);
                writer.bool(caller.is_some());
                if let Some(caller) = caller {
                    caller.write_snapshot(writer);
                }
            }
            CompilationOrigin::FunctionConstructor { caller } => {
                writer.u8(3);
                caller.write_snapshot(writer);
            }
        }
        script_or_module.snap(writer)?;
        writer.bool(*reclaimable);
        writer.u64(*pass);
        Ok(())
    }

    fn read_snapshot(
        reader: &mut Reader<'_>,
        bytes: &crate::KeptBytes,
        atoms: &Rc<crate::bytecode::AtomSequence>,
    ) -> SnapshotResult<Self> {
        let text = if reader.bool()? { Some(crate::bytecode::SourceText::read_snapshot(reader)?) } else { None };
        let regexp_literal_sites = reader.len()?;
        let functions = reader.len()?;
        let sites = crate::bytecode::CacheSiteCounts { fields: reader.u16()?, methods: reader.len()?, objects: reader.len()? };
        let encoded_len = reader.len()?;
        let encoded_start = reader.position();
        reader.skip(encoded_len)?;
        let window = crate::snapshot::Window::new(bytes.clone(), encoded_start, encoded_len)?;
        ResidualProgram::verify_snapshot(window.as_ref().as_ref())
            .map_err(|error| SnapshotError(format!("snapshot program: {error}")))?;
        let residual = Residual::encoded(sites, EncodedResidual { bytes: Rc::new(window), text, atoms: atoms.clone() });
        let constants = reader.values()?;
        let const_arrays = read_list(reader, |reader| Ok(if reader.bool()? { Some(crate::heap::ArrayElements::from(reader.values()?)) } else { None }))?;
        let function_sources = Snap::unsnap(reader)?;
        let module_environment = Snap::unsnap(reader)?;
        let import_meta = Snap::unsnap(reader)?;
        let module_imports = read_list(reader, |reader| {
            let slot = reader.u16()?;
            let import = match reader.u8()? {
                0 => ModuleImport::Value(reader.value()?),
                1 => ModuleImport::Binding(ProgramId::from_raw(reader.u32()?), reader.u16()?, reader.value()?),
                _ => return Err(SnapshotError("invalid snapshot module import".into())),
            };
            Ok((slot, import))
        })?;
        let module = reader.bool()?;
        let scope = program_scope::read(reader)?;
        let origin = match reader.u8()? {
            0 => CompilationOrigin::Source,
            1 => CompilationOrigin::SharedScript { source: ProgramId::from_raw(reader.u32()?) },
            2 => CompilationOrigin::Eval { caller: if reader.bool()? { Some(JsString::read_snapshot(reader)?) } else { None } },
            3 => CompilationOrigin::FunctionConstructor { caller: JsString::read_snapshot(reader)? },
            _ => return Err(SnapshotError("invalid snapshot program origin".into())),
        };
        let script_or_module = Snap::unsnap(reader)?;
        let reclaimable = reader.bool()?;
        let pass = reader.u64()?;
        Ok(Self {
            regexp_literals: vec![None; regexp_literal_sites],
            lane_views: super::LaneViews::default(),
            js_entry_facts: super::JsEntryFactsTable::default(),
            lane_deferred: vec![std::cell::Cell::new(0); functions],
            residual,
            wasm_signatures: None,
            constants,
            const_arrays,
            function_sources,
            module_environment,
            import_meta,
            module_imports,
            root_slots: std::cell::OnceCell::new(),
            module,
            compiled: None,
            jit_tiers: JitTierManager::default(),
            scope,
            origin,
            script_or_module,
            reclaimable,
            pass,
            roots_pending: true,
        })
    }
}

impl ProgramStore {
    /// Installs `main` in the main program's slot, which a restored store leaves empty.
    pub(crate) fn install_main(&mut self, main: Rc<ResidualProgram>) {
        let restored = std::mem::take(&mut self.programs);
        let module = main.is_module();
        let id = self.insert_shared(main);
        debug_assert_eq!(id, Some(ProgramId::MAIN));
        if module && let Some(entry) = self.entry_mut(ProgramId::MAIN) {
            entry.module = true;
        }
        self.programs.extend(restored.into_iter().skip(1));
    }
}
