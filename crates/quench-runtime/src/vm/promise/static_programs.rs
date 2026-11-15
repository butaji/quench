//! Compiling the static modules a program links: their analyses, plans, early errors and the
//! programs kept in the compile cache, collected for a graph's evaluation.

use super::*;

impl<H: Host> Vm<H> {
    /// `Engine::static_module_analysis` of a module, kept across runs in the host's compile
    /// cache for its exact text, as its compiled program is: a process otherwise parses every
    /// static module it links once more than it compiles it.
    pub(super) fn static_module_analysis(&mut self, module: &ModuleSource) -> Rc<crate::compile::StaticModuleAnalysis> {
        let (source, name) = (module.source.as_ref(), module.name.as_str());
        let program = self.realm.promise.modules.get(&module_cache_key(name, "javascript"))
            .and_then(|record| record.program)
            .and_then(|id| self.programs.get(id))
            .filter(|program| program.source_text() == Some(source));
        let host = &mut self.host;
        crate::Engine::static_module_analysis_with(source, name, module.typescript, |analyze| {
            // Derive the loader's memoized view from its admitted compiler result.
            if let Some(program) = program {
                return crate::compile::StaticModuleAnalysis {
                    early_error: None,
                    plan: Some(crate::compile::StaticModulePlan {
                        link_plan: program.module_link_plan.clone(),
                        requests: program.module_requests.clone(),
                        has_top_level_await: program.function(0).is_async,
                    }),
                };
            }
            let parts = [STATIC_MODULE_ANALYSIS_GOAL, module.compilation_goal(), source];
            if let Some(analysis) = host
                .kept_compiled_program(name, &parts)
                .and_then(|bytes| crate::bytecode::decode_static_module_analysis((*bytes).as_ref()).ok())
            {
                return analysis;
            }
            let analysis = analyze();
            host.keep_compiled_program(name, &parts, &crate::bytecode::encode_static_module_analysis(&analysis));
            analysis
        })
    }

    pub(super) fn static_module_plan(&mut self, module: &ModuleSource) -> Option<crate::compile::StaticModulePlan> {
        self.static_module_analysis(module).plan.clone()
    }

    pub(super) fn static_module_early_error(&mut self, module: &ModuleSource) -> Option<String> {
        self.static_module_analysis(module).early_error.clone()
    }

    /// A static module's program kept by the host's compile cache
    /// (`Host::kept_compiled_program`) for its exact text: compiled as this VM specializes,
    /// after its early errors were ruled out, so neither repeats.
    pub(super) fn kept_static_module_program(&mut self, module: &ModuleSource) -> Option<ResidualProgram> {
        let bytes = self.host.kept_compiled_program(
            &module.name,
            &[self.specialization_part(), module.compilation_goal(), &module.source],
        )?;
        let text = crate::bytecode::SourceText::Shared(module.source.clone());
        let specialized = self.specialized;
        self.decode_cached_program(bytes, text, 0, &module.name, specialized).ok()
    }

    /// Offers the host the compiled program of a static module (`kept_static_module_program`),
    /// before its store releases its records, which encoding would compile again.
    pub(super) fn keep_static_module_program(&mut self, module: &ModuleSource, residual: &ResidualProgram) {
        if let Some(bytes) = self.encode_cached_program(residual) {
            self.host
                .keep_compiled_program(&module.name, &[self.specialization_part(), module.compilation_goal(), &module.source], &bytes);
        }
    }

    pub(super) fn collect_static_module_programs(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        precompiled: Option<ProgramId>,
        graph: &mut Vec<(ModuleSource, ProgramId)>,
    ) -> Result<(), JsError> {
        let key = module_cache_key(&module.name, "javascript");
        let existing = self.realm.promise.modules.get(&key);
        if existing.is_some_and(|record| {
            matches!(
                record.outcome,
                ModuleOutcome::Evaluated(_)
                    | ModuleOutcome::Deferred(_)
                    | ModuleOutcome::Errored(_)
            ) || record.program.is_some_and(|program| {
                record.phase() != ModulePhase::Loading
                    || graph.iter().any(|(_, visited)| *visited == program)
            })
        }) {
            return Ok(());
        }
        let precompiled = precompiled.or_else(|| existing.and_then(|record| record.program));
        #[cfg(feature = "native-execution")]
        let precompiled = match precompiled {
            Some(program) => Some(program),
            None => self.kept_native_module_program(module)
                .inspect_err(|_| self.record_native_execution_rejection())?,
        };
        let _stack =
            quench_stack::StackGuard::enter().map_err(|crate::stack::StackExhausted| self.stack_exhaustion_error())?;
        let program = match precompiled {
            Some(program) => program,
            None if let Some(program) = self.kept_static_module_program(module) => self
                .store_module_program(program)
                .ok_or_else(|| self.type_error(p, "module program store is full".into()))?,
            None => {
                self.require_source_compilation(p)?;
                if let Some(message) = self.static_module_early_error(module) {
                    return self
                        .syntax_error_result(p, &message)
                        .map(|_| ());
                }
                let residual = crate::Engine::specialize_module_with_atom_prefix(
                    &module.source,
                    &module.name,
                    module.typescript,
                    &*self,
                    self.released_records(true),
                )
                .map_err(|diagnostics| {
                    if diagnostics
                        .iter()
                        .any(crate::compile::Diagnostic::is_stack_exhausted)
                    {
                        self.stack_exhaustion_error()
                    } else {
                        self.type_error(
                            p,
                            format!("dynamic module compilation failed: {diagnostics:?}"),
                        )
                    }
                })?;
                // Encoded with this VM's atoms, which storing it then finds interned.
                let residual = self.intern_program_atoms(residual);
                self.keep_static_module_program(module, &residual);
                self
                    .store_module_program(residual)
                    .ok_or_else(|| self.type_error(p, "module program store is full".into()))?
            }
        };
        let residual = self
            .programs
            .get(program)
            .ok_or_else(|| self.type_error(p, "module program is unavailable".into()))?;
        self.instantiate_module_environment(&residual, program)?;
        let namespace = match self
            .realm
            .promise
            .modules
            .get(&key)
            .and_then(ModuleRecord::pending_namespace)
        {
            Some(namespace) => namespace,
            None => self.module_namespace(Vec::new())?,
        };
        let record = self
            .realm
            .promise
            .modules
            .entry(key)
            .or_insert_with(ModuleRecord::loading_static);
        record.program = Some(program);
        record.cache_pending_namespace(namespace);
        // Graph instantiation can allocate many environments before the first
        // guest instruction. The record and program store now own all roots.
        self.maybe_collect(p);
        graph.push((module.clone(), program));
        let dependencies = residual
            .module_requests
            .iter()
            .map(|request| {
                self.resolve_module_request(p, &module.name, request)?
                    .ok_or_else(|| {
                        self.type_error(p, "static module request was not resolved".into())
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.realm
            .promise
            .modules
            .get_mut(&module_cache_key(&module.name, "javascript"))
            .expect("module program registered before resolution")
            .resolved_requests = dependencies
            .iter()
            .map(|dependency| dependency.name.clone())
            .collect();
        for (request, dependency) in residual.module_requests.iter().zip(dependencies) {
            if request.phase == crate::bytecode::ModuleRequestPhase::Evaluation
                && request.module_type.as_deref().unwrap_or("javascript") == "javascript"
            {
                self.collect_static_module_programs(p, &dependency, None, graph)?;
            }
        }
        Ok(())
    }
}
