//! Shared Cranelift target and executable-code backend for JavaScript and Wasm.
//!
//! Frontends retain their own bytecode semantics, but all tiers request a
//! target ISA from this cache and publish code through the same W^X memory
//! implementation in `compiled.rs`.

use cranelift_codegen::settings::{self, Configurable};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};

mod select;
pub mod relocatable_code;
mod worker;
mod isa;
pub use isa::{IsaBaseline, NativeTarget};
pub mod builder;
#[allow(unused_imports)]
pub use worker::{OffThread, run_in_background};

type TargetIsa = dyn cranelift_codegen::isa::TargetIsa;

/// Reuse immutable host target configuration across JS function tiers and
/// Wasm module compilation, one per optimization level.
/// Whether Cranelift verifies the IR of every compile, and regalloc2 checks every allocation
/// it makes: in debug builds, where tests check each lowering, and in the tier-stress build,
/// which compiles everything it can (the ISA is shared by every VM of the process); not in release, where the verifier cost about a quarter
/// of every JIT compile's instructions (npm semver/browserslist `--jit`, 2026-10-24).
fn verify_ir() -> bool { cfg!(debug_assertions) || cfg!(feature = "tier-stress") }

/// Publication determines relocation policy and code-generation effort, not language
/// semantics or lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CodePublication {
    #[cfg(feature = "jit")]
    ExecutableMemory,
    RelocatableObject,
    /// Copy-and-patch stencils: executable memory whose continuations are
    /// tail calls, which Cranelift emits only with frame pointers.
    #[cfg(feature = "jit")]
    Stencil,
}

impl CodePublication {
    /// The Cranelift settings a publication fixes.
    fn settings(self) -> &'static [(&'static str, &'static str)] {
        match self {
            #[cfg(feature = "jit")]
            Self::ExecutableMemory => &[("is_pic", "false")],
            Self::RelocatableObject => &[("is_pic", "true")],
            #[cfg(feature = "jit")]
            Self::Stencil => &[("is_pic", "false"), ("preserve_frame_pointers", "true")],
        }
    }
}

/// How much effort one compile spends. `Optimized` is the shipping configuration; `Quick`
/// compiles a body at a fraction of the cost (no mid-end optimization, one-pass register
/// allocation) for code that has not proven itself hot. A caller chooses per compile; every
/// compile uses `Optimized` until a tier policy asks otherwise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CompileLevel {
    Quick,
    /// A body too large for the backtracking allocator's scratch memory: `Optimized` with
    /// one-pass register allocation.
    Large,
    #[default]
    Optimized,
}

impl CompileLevel {
    /// The level a compile uses unless its caller chooses: `Optimized`, or `Quick` while the
    /// `quick-compile` experiment measures that level.
    pub fn run_default() -> Self {
        if crate::jit_mode::quick_only() || crate::trace::Dev::QuickCompile.on() { Self::Quick } else { Self::Optimized }
    }

    /// The Cranelift settings a level fixes, applied after the publication's.
    fn settings(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Quick => &[("opt_level", "none")],
            Self::Large => &[("regalloc_algorithm", "single_pass")],
            Self::Optimized => &[],
        }
    }
}

/// The Cranelift opt level of JavaScript JIT code. Measured on JetStream 3 `--jit` (2 interleaved
/// rounds, TOD-1467, 2026-11-07): while lane bodies compiled on the main thread, `none` won
/// +4.5% over the JavaScript tests; once they compiled on the code-generator thread, `none` for
/// main-thread compiles measured +1.2% and for every compile -0.1%, both within noise, and the
/// Wasm tests lost 5-6%. The single-pass register allocator lost 2.7% (Wasm -25%) for all JIT
/// code, and left 46% of lane-body machine code as stack spill traffic (richards bodies,
/// 2026-11-10: 91981 instructions against 69653 with the backtracking allocator; richards +34%,
/// delta-blue +12% in lane mode, other six V8-v7 tests within noise; peak RSS +30% on the two
/// largest bodies' tests). `unwind_info=false` measured nothing.
const JS_JIT_OPT_LEVEL: &str = "speed";

/// Declares the symbol `(namespace, key)` in `function`, a relocation the loader patches: the
/// one form every position-independent constant takes (stencil holes, `lane_region::relocs`).
pub fn declare_symbol(function: &mut cranelift_codegen::ir::Function, namespace: u32, key: u32) -> cranelift_codegen::ir::GlobalValue {
    use cranelift_codegen::ir;
    let name = function.declare_imported_user_function(ir::UserExternalName::new(namespace, key));
    function.create_global_value(ir::GlobalValueData::Symbol {
        name: ir::ExternalName::user(name),
        offset: ir::immediates::Imm64::new(0),
        colocated: false,
        tls: false,
    })
}

/// The function's reference to `signature`: the one it imported already, or a new import.
/// Cranelift keeps one `SigRef` per imported signature and lowers each one's ABI, so the call
/// sites of one shape share a reference instead of importing the signature again each.
pub fn signature_ref(function: &mut cranelift_codegen::ir::Function, signature: cranelift_codegen::ir::Signature) -> cranelift_codegen::ir::SigRef {
    match function.dfg.signatures.iter().find(|(_, imported)| **imported == signature) {
        Some((reference, _)) => reference,
        None => function.import_signature(signature),
    }
}

/// The address of symbol `(namespace, key)` as a 64-bit value built at `cursor`.
pub fn symbol_in(cursor: &mut cranelift_codegen::cursor::FuncCursor<'_>, namespace: u32, key: u32) -> cranelift_codegen::ir::Value {
    use cranelift_codegen::ir::{InstBuilder, types};
    let symbol = declare_symbol(cursor.func, namespace, key);
    cursor.ins().symbol_value(types::I64, symbol)
}

/// The executable-memory ISA of every JavaScript JIT tier.
pub fn target_isa() -> Option<Arc<TargetIsa>> {
    #[cfg(feature = "jit")]
    { target_isa_level(CompileLevel::run_default()) }
    #[cfg(not(feature = "jit"))]
    { None }
}

/// The ISA of an AOT lane body for `target`: the JIT's, with the frame pointers its leaves' tail
/// calls to the base body's entries need, as the stencils that base body is built from have, and the
/// IR verifier on: a lane body is an extra tier, so a body whose IR is invalid is dropped (the
/// compile's error) rather than aborting the ahead compile, which can afford the check.
pub fn ahead_lane_isa(target: &NativeTarget) -> Option<Arc<TargetIsa>> {
    #[cfg(feature = "jit")]
    {
        static ISAS: OnceLock<Mutex<Vec<(NativeTarget, Arc<TargetIsa>)>>> = OnceLock::new();
        let mut isas = ISAS.get_or_init(Default::default).lock().ok()?;
        if let Some((_, isa)) = isas.iter().find(|(built, _)| built == target) { return Some(Arc::clone(isa)); }
        let isa = build_isa_verified(JS_JIT_OPT_LEVEL, CodePublication::Stencil, CompileLevel::run_default(), target, true).ok()?;
        isas.push((target.clone(), Arc::clone(&isa)));
        Some(isa)
    }
    #[cfg(not(feature = "jit"))]
    { let _ = target; None }
}

/// The executable-memory ISA at `opt_level`: the Wasm tier's, which picks its own level.
pub fn target_isa_at(_opt_level: &'static str) -> Option<Arc<TargetIsa>> {
    #[cfg(feature = "jit")]
    { target_isa_for(_opt_level, CodePublication::ExecutableMemory) }
    #[cfg(not(feature = "jit"))]
    { None }
}

pub fn target_isa_for(
    opt_level: &'static str,
    publication: CodePublication,
) -> Option<Arc<TargetIsa>> {
    target_isa_with(opt_level, publication, CompileLevel::Optimized)
}

/// The executable-memory ISA of JavaScript JIT code compiled at `level`.
#[cfg(feature = "jit")]
pub fn target_isa_level(level: CompileLevel) -> Option<Arc<TargetIsa>> {
    target_isa_with(JS_JIT_OPT_LEVEL, CodePublication::ExecutableMemory, level)
}

/// Without the JIT no code is published to executable memory.
#[cfg(not(feature = "jit"))]
pub fn target_isa_level(_level: CompileLevel) -> Option<Arc<TargetIsa>> { None }

fn target_isa_with(
    opt_level: &'static str,
    publication: CodePublication,
    level: CompileLevel,
) -> Option<Arc<TargetIsa>> {
    // Keyed by the level's static name: every compile asks, and an owned key allocated a string each time.
    type IsaCache = HashMap<(&'static str, CodePublication, CompileLevel), Arc<TargetIsa>>;
    static CACHE: OnceLock<Mutex<IsaCache>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = (opt_level, publication, level);
    let mut entries = cache.lock().ok()?;
    if let Some(isa) = entries.get(&key) {
        return Some(Arc::clone(isa));
    }

    let target = match publication {
        CodePublication::RelocatableObject => NativeTarget::default(),
        #[cfg(feature = "jit")]
        _ => NativeTarget::host(),
    };
    let isa = build_isa(opt_level, publication, level, &target).ok()?;
    entries.insert(key, Arc::clone(&isa));
    Some(isa)
}

/// The relocatable ISA of AOT code on `target`: its CPU baseline and triple, never the host's CPU.
pub fn relocatable_isa(target: &NativeTarget) -> Result<Arc<TargetIsa>, String> {
    build_isa("speed", CodePublication::RelocatableObject, CompileLevel::Optimized, target)
}

/// The ISA of `publication` at `opt_level` and `level` on `target`, with the flags every
/// publication and level fixes.
fn build_isa(
    opt_level: &str,
    publication: CodePublication,
    level: CompileLevel,
    target: &NativeTarget,
) -> Result<Arc<TargetIsa>, String> {
    build_isa_verified(opt_level, publication, level, target, verify_ir())
}

/// `build_isa`, with the IR verifier and register-allocation checker on where `verify` says.
fn build_isa_verified(
    opt_level: &str,
    publication: CodePublication,
    level: CompileLevel,
    target: &NativeTarget,
    verify: bool,
) -> Result<Arc<TargetIsa>, String> {
    let mut flags = settings::builder();
    flags.set("opt_level", opt_level).map_err(|error| error.to_string())?;
    for (name, value) in publication.settings().iter().chain(level.settings()) {
        flags.set(name, value).map_err(|error| error.to_string())?;
    }
    for check in ["enable_verifier", "regalloc_checker"] {
        let value = if verify { "true" } else { "false" };
        flags.set(check, value).map_err(|error| error.to_string())?;
    }
    target.isa(settings::Flags::new(flags))
}

/// Compilation failures shared by the native publishers. Relocation admission
/// and publication failures belong to the consumer, not Cranelift lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeCompileRejection {
    Cranelift,
    Relocations,
    EmptyCode,
    CodeSizeBudget,
    Publish,
}

impl NativeCompileRejection {
    /// Whether replanning with less inlined code can address this rejection.
    pub const fn exceeds_code_budget(self) -> bool {
        match self {
            Self::CodeSizeBudget => true,
            _ => false,
        }
    }

    pub const fn reason(self) -> &'static str {
        match self {
            Self::Cranelift => "cranelift-compile-error",
            Self::Relocations => "unresolved-relocations",
            Self::EmptyCode => "empty-code",
            Self::CodeSizeBudget => "code-size-budget",
            Self::Publish => "code-publication-failed",
        }
    }
}

/// Lower once, retaining relocations and their symbolic name table until the
/// consumer has linked or published the artifact. This avoids an extra code copy.
/// A missing size limit preserves the Wasm compiler's existing admission policy.
/// The consumer reads the function's parameters only (its relocation names): its instructions
/// are dropped once lowered (`Context::release_ir`).
pub fn compile_function<T>(
    function: cranelift_codegen::ir::Function,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    max_code_bytes: Option<usize>,
    consume: impl FnOnce(
        &cranelift_codegen::CompiledCode,
        &cranelift_codegen::ir::Function,
    ) -> Result<T, NativeCompileRejection>,
) -> Result<T, NativeCompileRejection> {
    compile_function_reading(function, isa, max_code_bytes, true, consume)
}

/// `compile_function` for a consumer that reads the compiled function's instructions too
/// (`release_ir` false).
pub fn compile_function_reading<T>(
    mut function: cranelift_codegen::ir::Function,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    max_code_bytes: Option<usize>,
    release_ir: bool,
    consume: impl FnOnce(
        &cranelift_codegen::CompiledCode,
        &cranelift_codegen::ir::Function,
    ) -> Result<T, NativeCompileRejection>,
) -> Result<T, NativeCompileRejection> {
    // A build without the JIT (a static image's support) links no code generator: its native code is
    // the image's, lowered by `quench compile`.
    if !cfg!(feature = "jit") {
        return Err(NativeCompileRejection::Cranelift);
    }
    crate::vm::engine_calls::declare_regions(&mut function);
    select::convert(&mut function);
    let mut context = COMPILE_CONTEXT.with(|held| held.take()).unwrap_or_else(cranelift_codegen::Context::new);
    context.clear();
    context.func = function;
    context.release_ir = release_ir;
    let compiled = context.compile(isa, &mut cranelift_codegen::control::ControlPlane::default()).map(|compiled| compiled.code_buffer().len());
    let outcome = match compiled {
        Err(_error) => {
            #[cfg(test)]
            eprintln!("shared native compile rejected by Cranelift: {_error:?}");
            if crate::trace::Channel::AotLanes.on() {
                let text = match &_error.inner {
                    cranelift_codegen::CodegenError::Verifier(errors) => errors.0.first().map(|error| format!("{}: {}", error.location, error.message)).unwrap_or_default(),
                    other => format!("{other}"),
                }.replace('\n', " ");
                eprintln!("aot-lanes: Cranelift error: {}", text.chars().take(240).collect::<String>());
            }
            Err(NativeCompileRejection::Cranelift)
        }
        Ok(0) => Err(NativeCompileRejection::EmptyCode),
        Ok(size) if max_code_bytes.is_some_and(|limit| size > limit) => Err(NativeCompileRejection::CodeSizeBudget),
        Ok(_) => consume(context.compiled_code().expect("successful compilation"), &context.func),
    };
    // One context per compiling thread keeps its scratch for the next compile, as Cranelift
    // means a `Context` to be reused: `clear` keeps every table's capacity, so the next
    // compile of a body of the same size allocates nothing anew (lane bodies are 8k to 17k
    // instructions, and dropping their scratch after each compile reallocated all of it).
    context.clear();
    COMPILE_CONTEXT.with(|held| held.set(Some(context)));
    outcome
}

thread_local! {
    static COMPILE_CONTEXT: std::cell::Cell<Option<cranelift_codegen::Context>> = const { std::cell::Cell::new(None) };
}

/// Drops the compiling thread's held context (`compile_function`): its tables are a cache sized
/// by the largest body compiled, megabytes the thread would otherwise keep through every idle
/// period. The thread calls this when its queue drains, so the next burst allocates them anew.
pub fn release_compile_context() {
    drop(COMPILE_CONTEXT.with(std::cell::Cell::take));
}


#[cfg(all(test, feature = "jit", target_arch = "x86_64"))]
mod vendor_patch_tests;

#[cfg(all(test, feature = "jit"))]
mod branch_shape_tests;

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
                let cranelift_codegen::RelocTarget::ExternalName(ir::ExternalName::User(
                    name,
                )) = &relocation.target
                else {
                    return false;
                };
                function
                    .params
                    .user_named_funcs()
                    .get(*name)
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
