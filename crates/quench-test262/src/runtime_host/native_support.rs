//! Which native image support a case runs under: an explicit choice, or the one its
//! programs' source demands select (`quench_runtime::SourceDemands`).

use super::{ExecutionMode, NativeImageSupport, Test262RuntimeHost};
use super::{Engine, ModuleSource, NativeCasePrograms, Runtime, SourceKind};

/// Whether the harness, entry or any declared module can reach source first known at execution.
pub(super) fn requires_dynamic_source(
    harness: &[&str], source: &str, kind: SourceKind, modules: &[(ModuleSource, quench_runtime::ResidualProgram)],
) -> bool {
    harness.iter().any(|script| Engine::source_requires_dynamic_source(script, SourceKind::Script))
        || Engine::source_requires_dynamic_source(source, kind)
        || modules.iter().any(|(module, _)| Engine::source_requires_dynamic_source(&module.source, SourceKind::Module))
}

impl Test262RuntimeHost {
    /// Fix the support of native-only runs: dynamic source retains the shared
    /// interpreter/JIT beside the precompiled static roots, harness and declared
    /// dependencies. `None` leaves it to the program's source demands.
    pub fn with_native_support(mut self, support: Option<NativeImageSupport>) -> Result<Self, String> {
        if support.is_some_and(NativeImageSupport::allows_dynamic_source)
            && (self.execution_mode != ExecutionMode::NativeOnly || !crate::NATIVE_SUPPORT_HAS_JIT) {
            return Err("dynamic native source requires AOT compilation and JIT support".into());
        }
        self.native_request = support;
        self.native_support = support.unwrap_or_default();
        self.native_compilation.dynamic_source = self.native_support.allows_dynamic_source();
        Ok(self)
    }

    /// Settle automatic support once the prepared programs' demands are known.
    pub(super) fn resolve_native_support(&mut self, requires_dynamic_source: bool) {
        self.native_support = NativeImageSupport::select(self.native_request, requires_dynamic_source);
        self.native_compilation.dynamic_source = self.native_support.allows_dynamic_source();
    }

    pub(super) fn runtime_execution_mode(&self) -> ExecutionMode {
        if self.execution_mode == ExecutionMode::NativeOnly { self.native_support.execution_mode() } else { self.execution_mode }
    }

    /// Prepare the static graph, then run it under the support its demands selected.
    pub(super) fn prepare_for_runtime(
        runtime: &mut Runtime<Self>, harness: &[&str], source: &str, name: &str, kind: SourceKind,
    ) -> Result<NativeCasePrograms, String> {
        let prepared = runtime.host_mut().prepare_native_programs(harness, source, name, kind)?;
        let mode = runtime.host_mut().runtime_execution_mode();
        runtime.set_execution_mode(mode);
        Ok(prepared)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_native_support_follows_the_programs_source_demands() {
        let source = "const add = new Function('a', 'b', 'return a + b'); if (eval('6 * 7') !== 42 || add(40, 2) !== 42) throw Error('dynamic source');";
        let mut host = Test262RuntimeHost::new(ExecutionMode::NativeOnly);
        let prepared = host.prepare_native_programs(&[], source, "automatic-dynamic.js", SourceKind::Script).unwrap();
        assert!(prepared.requires_dynamic_source && host.native_support.allows_dynamic_source());
        host.execute(source, "automatic-dynamic.js").unwrap();
        assert_eq!(host.native_compilation.rejections, 0);
        let closed = "const o = { a: 1 }; if (o.a !== 1) throw Error('closed');";
        let prepared = host.prepare_native_programs(&[], closed, "automatic-static.js", SourceKind::Script).unwrap();
        assert!(!prepared.requires_dynamic_source && !host.native_support.allows_dynamic_source());
    }
}
