//! Shared Cranelift target and executable-code backend for JavaScript and Wasm.
//!
//! Frontends retain their own bytecode semantics, but all tiers request a
//! target ISA from this cache and publish code through the same W^X memory
//! implementation in `compiled.rs`.

#[cfg(any(feature = "jit", feature = "aot"))]
use cranelift_codegen::settings::{self, Configurable};
#[cfg(any(feature = "jit", feature = "aot"))]
use std::collections::HashMap;
use std::sync::Arc;
#[cfg(any(feature = "jit", feature = "aot"))]
use std::sync::{Mutex, OnceLock};

#[cfg(any(feature = "jit", feature = "aot"))]
mod select;
mod worker;
pub(super) use worker::{Pending, run_in_background};

type TargetIsa = dyn cranelift_codegen::isa::TargetIsa;

/// Reuse immutable host target configuration across JS function tiers and
/// Wasm module compilation, one per optimization level.
/// Whether Cranelift verifies the IR of every compile, and regalloc2 checks every allocation
/// it makes: in debug builds, where tests check each lowering, and in the tier-stress build,
/// which compiles everything it can; not in release, where the verifier cost about a quarter
/// of every JIT compile's instructions (npm semver/browserslist `--jit`, 2026-10-24).
#[cfg(any(feature = "jit", feature = "aot"))]
const VERIFY_IR: bool = cfg!(debug_assertions) || super::compiled::tier_policy::STRESS;

/// Publication determines relocation policy and code-generation effort, not language
/// semantics or lowering.
#[cfg(any(feature = "jit", feature = "aot"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum CodePublication {
    #[cfg(feature = "jit")]
    ExecutableMemory,
    /// Executable memory for a lane body (`compiled::lane_compile`), whose registers are
    /// allocated in one pass. Lane bodies are the largest units and compile on the code
    /// generator thread while the interpreter runs them: JetStream 3 `--jit`, 25 tests, 6
    /// interleaved rounds (2026-11-07) measured +3.8% geomean (richards +16%, Air +11%,
    /// delta-blue +10%; earley-boyer -7%). Single-pass for every JIT unit measured -0.7%
    /// (navier-stokes -22%, stanford-crypto-aes -17%): small hot loops keep backtracking.
    #[cfg(feature = "jit")]
    LaneBody,
    #[cfg(feature = "aot")]
    RelocatableObject,
    /// Copy-and-patch stencils: executable memory whose continuations are
    /// tail calls, which Cranelift emits only with frame pointers.
    #[cfg(all(feature = "aot", feature = "jit"))]
    Stencil,
}

#[cfg(any(feature = "jit", feature = "aot"))]
impl CodePublication {
    /// The Cranelift settings a publication fixes.
    fn settings(self) -> &'static [(&'static str, &'static str)] {
        match self {
            #[cfg(feature = "jit")]
            Self::ExecutableMemory => &[("is_pic", "false")],
            #[cfg(feature = "jit")]
            Self::LaneBody => &[("is_pic", "false"), ("regalloc_algorithm", "single_pass")],
            #[cfg(feature = "aot")]
            Self::RelocatableObject => &[("is_pic", "true")],
            #[cfg(all(feature = "aot", feature = "jit"))]
            Self::Stencil => &[("is_pic", "false"), ("preserve_frame_pointers", "true")],
        }
    }
}

/// The Cranelift opt level of JavaScript JIT code. Measured on JetStream 3 `--jit` (2 interleaved
/// rounds, TOD-1467, 2026-11-07): while lane bodies compiled on the main thread, `none` won
/// +4.5% over the JavaScript tests; once they compiled on the code-generator thread, `none` for
/// main-thread compiles measured +1.2% and for every compile -0.1%, both within noise, and the
/// Wasm tests lost 5-6%. The single-pass register allocator lost 2.7% (Wasm -25%), and
/// `unwind_info=false` measured nothing.
const JS_JIT_OPT_LEVEL: &str = "speed";

/// The executable-memory ISA of every JavaScript JIT tier.
pub(super) fn target_isa() -> Option<Arc<TargetIsa>> {
    target_isa_at(JS_JIT_OPT_LEVEL)
}

/// The ISA of lane bodies: JavaScript JIT code whose register allocation runs in one pass.
pub(super) fn lane_body_isa() -> Option<Arc<TargetIsa>> {
    #[cfg(feature = "jit")]
    { target_isa_for(JS_JIT_OPT_LEVEL, CodePublication::LaneBody) }
    #[cfg(not(feature = "jit"))]
    { None }
}

/// The executable-memory ISA at `opt_level`: the Wasm tier's, which picks its own level.
pub(super) fn target_isa_at(_opt_level: &str) -> Option<Arc<TargetIsa>> {
    #[cfg(feature = "jit")]
    { target_isa_for(_opt_level, CodePublication::ExecutableMemory) }
    #[cfg(not(feature = "jit"))]
    { None }
}

#[cfg(any(feature = "jit", feature = "aot"))]
pub(super) fn target_isa_for(
    opt_level: &str,
    publication: CodePublication,
) -> Option<Arc<TargetIsa>> {
    type IsaCache = HashMap<(String, CodePublication), Arc<TargetIsa>>;
    static CACHE: OnceLock<Mutex<IsaCache>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = (opt_level.to_owned(), publication);
    let mut entries = cache.lock().ok()?;
    if let Some(isa) = entries.get(&key) {
        return Some(Arc::clone(isa));
    }

    let mut flags = settings::builder();
    flags.set("opt_level", opt_level).ok()?;
    for (name, value) in publication.settings() {
        flags.set(name, value).ok()?;
    }
    for check in ["enable_verifier", "regalloc_checker"] {
        flags.set(check, if VERIFY_IR { "true" } else { "false" }).ok()?;
    }
    let isa = cranelift_native::builder()
        .ok()?
        .finish(settings::Flags::new(flags))
        .ok()?;
    entries.insert(key, Arc::clone(&isa));
    Some(isa)
}

/// Compilation failures shared by the native publishers. Relocation admission
/// and publication failures belong to the consumer, not Cranelift lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NativeCompileRejection {
    #[cfg(any(feature = "jit", feature = "aot"))]
    Cranelift,
    #[cfg(not(any(feature = "jit", feature = "aot")))]
    BackendUnavailable,
    Relocations,
    #[cfg(any(feature = "jit", feature = "aot"))]
    EmptyCode,
    #[cfg(any(feature = "jit", feature = "aot"))]
    CodeSizeBudget,
    Publish,
}

impl NativeCompileRejection {
    /// Whether replanning with less inlined code can address this rejection.
    pub(super) const fn exceeds_code_budget(self) -> bool {
        match self {
            #[cfg(any(feature = "jit", feature = "aot"))]
            Self::CodeSizeBudget => true,
            _ => false,
        }
    }

    pub(super) const fn reason(self) -> &'static str {
        match self {
            #[cfg(any(feature = "jit", feature = "aot"))]
            Self::Cranelift => "cranelift-compile-error",
            #[cfg(not(any(feature = "jit", feature = "aot")))]
            Self::BackendUnavailable => "native-backend-unavailable",
            Self::Relocations => "unresolved-relocations",
            #[cfg(any(feature = "jit", feature = "aot"))]
            Self::EmptyCode => "empty-code",
            #[cfg(any(feature = "jit", feature = "aot"))]
            Self::CodeSizeBudget => "code-size-budget",
            Self::Publish => "code-publication-failed",
        }
    }
}

/// Lower once, retaining relocations and their symbolic name table until the
/// consumer has linked or published the artifact. This avoids an extra code copy.
/// A missing size limit preserves the Wasm compiler's existing admission policy.
#[cfg(any(feature = "jit", feature = "aot"))]
pub(super) fn compile_function<T>(
    mut function: cranelift_codegen::ir::Function,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    max_code_bytes: Option<usize>,
    consume: impl FnOnce(
        &cranelift_codegen::CompiledCode,
        &cranelift_codegen::ir::Function,
    ) -> Result<T, NativeCompileRejection>,
) -> Result<T, NativeCompileRejection> {
    select::convert(&mut function);
    let mut context = cranelift_codegen::Context::for_function(function);
    let compiled = context
        .compile(
            isa,
            &mut cranelift_codegen::control::ControlPlane::default(),
        )
        .map_err(|_error| {
            #[cfg(test)]
            eprintln!("shared native compile rejected by Cranelift: {_error:?}");
            NativeCompileRejection::Cranelift
        })?;
    let size = compiled.code_buffer().len();
    if size == 0 {
        return Err(NativeCompileRejection::EmptyCode);
    }
    if max_code_bytes.is_some_and(|limit| size > limit) {
        return Err(NativeCompileRejection::CodeSizeBudget);
    }
    consume(
        context.compiled_code().expect("successful compilation"),
        &context.func,
    )
}

#[cfg(not(any(feature = "jit", feature = "aot")))]
pub(super) fn compile_function<T>(
    _function: cranelift_codegen::ir::Function,
    _isa: &dyn cranelift_codegen::isa::TargetIsa,
    _max_code_bytes: Option<usize>,
    _consume: impl FnOnce(&cranelift_codegen::CompiledCode, &cranelift_codegen::ir::Function)
        -> Result<T, NativeCompileRejection>,
) -> Result<T, NativeCompileRejection> {
    Err(NativeCompileRejection::BackendUnavailable)
}

#[cfg(all(test, feature = "jit"))]
mod tests {
    use super::*;
    use cranelift_codegen::ir::{self, AbiParam, InstBuilder, types};
    use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

    fn function(isa: &TargetIsa, calls_import: bool) -> ir::Function {
        let mut signature = ir::Signature::new(isa.default_call_conv());
        signature.returns.push(AbiParam::new(types::I32));
        let mut function =
            ir::Function::with_name_signature(ir::UserFuncName::user(0, 0), signature.clone());
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut function, &mut context);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);
        let result = if calls_import {
            let signature = builder.import_signature(signature);
            let name = builder
                .func
                .declare_imported_user_function(ir::UserExternalName::new(0, 1));
            let callee = builder.import_function(ir::ExtFuncData {
                name: ir::ExternalName::user(name),
                signature,
                colocated: false,
                patchable: false,
            });
            let call = builder.ins().call(callee, &[]);
            builder.inst_results(call)[0]
        } else {
            builder.ins().iconst(types::I32, 0)
        };
        builder.ins().return_(&[result]);
        builder.finalize(isa.frontend_config());
        function
    }

    #[test]
    fn symbolic_calls_survive_compilation() {
        let isa = target_isa().expect("host target");
        compile_function(function(&*isa, true), &*isa, None, |compiled, function| {
            assert!(!compiled.buffer.relocs().is_empty());
            assert!(compiled.buffer.relocs().iter().any(|relocation| {
                let cranelift_codegen::FinalizedRelocTarget::ExternalName(ir::ExternalName::User(
                    name,
                )) = relocation.target
                else {
                    return false;
                };
                function
                    .params
                    .user_named_funcs()
                    .get(name)
                    .is_some_and(|name| name.namespace == 0 && name.index == 1)
            }));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn code_budget_is_checked_before_publication_and_admits_exact_size() {
        let isa = target_isa().expect("host target");
        let size = compile_function(function(&*isa, false), &*isa, None, |code, _| {
            Ok(code.code_buffer().len())
        })
        .unwrap();
        let rejected = compile_function(function(&*isa, false), &*isa, Some(size - 1), |_, _| {
            panic!("oversized code must not be published")
        });
        assert_eq!(
            rejected,
            Err::<(), _>(NativeCompileRejection::CodeSizeBudget)
        );
        compile_function(function(&*isa, false), &*isa, Some(size), |_, _| Ok(())).unwrap();
    }

    #[test]
    fn publication_failure_is_preserved() {
        let isa = target_isa().expect("host target");
        let result = compile_function(function(&*isa, false), &*isa, None, |_, _| {
            Err::<(), _>(NativeCompileRejection::Publish)
        });
        assert_eq!(result, Err(NativeCompileRejection::Publish));
    }
}

#[cfg(all(test, not(feature = "jit")))]
mod execution_only_tests {
    #[test]
    fn executable_code_publication_is_unavailable_without_jit() {
        for level in ["none", "speed", "speed_and_size"] {
            assert!(super::target_isa_at(level).is_none());
        }
    }
}
