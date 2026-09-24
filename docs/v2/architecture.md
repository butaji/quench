# Staged interpreter architecture

RQJ treats compilation as Futamura's first projection: specialize one
general ES5-subset interpreter with respect to a known source program, then
execute the residual bytecode. It generates no native guest code. The design
keeps three binding times explicit.

On Unix, the CLI writes that versioned residual data and replaces itself with
a residual-only invocation before execution. This is still one measured
process—not an unmeasured compilation child—and makes the phase boundary
physical: OXC pages need not overlap the interpreter's RSS high-water.

- Static facts exist only while OXC ASTs are compiled. Literals become
  `Constant` entries; task 10's `AbstractConstant` lattice derives safe root
  bindings and invalidates facts captured by writes. Static facts are dataflow
  results, never function-name or source-shape guesses.
- Specialization-environment facts are resolved once and indexed compactly.
  `Operand` tags register, constant, and field inputs in one representation;
  `FieldSite` and method-site tables hold property paths, cache IDs, and call
  arguments behind small integer IDs. Bytecode carries those IDs and the VM
  reads the tables many times.
- Dynamic facts are values, shapes, and effects observed for each execution.
  The interpreter resolves tagged operands, updates bounded inline caches,
  and confines allocation, host calls, and GC to explicit edges. `Op` effect
  metadata is static and lets the declarative rewrite engine reject a local
  fusion that would discard a dynamic heap or control effect.

This staging discipline is also the performance-integrity boundary from task
01. Specialization may depend on syntax, local bytecode patterns, constants,
and general runtime feedback at an instruction site. It must never recognize
benchmark names, function IDs, whole-function instruction templates, or
domain semantics and substitute a native implementation. Such a substitution
is not P1 residualization; it is a second, benchmark-specific program hidden
behind the interpreter.

New optimizations should therefore add a data variant, a sound derivation, or
a bounded local state transition. They should remain correct after arbitrary
identifier renaming and small source edits, and their generic fallback must be
observable in tests and profiling.

## The specializer is a cogen-style generating extension

An interpretive partial evaluator would receive both an interpreter and a
guest program, then repeatedly walk the interpreter's own semantics to decide
what can be reduced. RQJ does not do that at runtime. `Engine::specialize` is
a hand-written *generating extension*: the specialization decisions for this
fixed ES5-subset interpreter have already been compiled into Rust, so each
source program is mapped directly from OXC syntax and derived binding facts to
residual bytecode. In partial-evaluation terminology, this is the cogen-style
form of Futamura P1, with the compiler-generator step performed ahead of time
by the implementation rather than interpretively for every guest program.

The `opcodes!` declaration is the first deliberately derived portion of that
generating extension. One data row produces the opcode identity, stable name,
and effect table instead of maintaining three independent hand-written views.
The rewrite-rule table applies the same principle to local residualization:
patterns are data and the fixed-point engine is generic. Tasks that derive
more compiler output from binding/effect data extend this cogen; runtime
respecialization remains a distinct mechanism and must not be confused with
native-code generation. Every result is still residual bytecode interpreted
by the Rust VM.

`compile/binding_time.rs` is the canonical binding-time analysis. Its single
`BindingTime<T>` lattice classifies source literals and carries root-bytecode
facts through one dataflow scan; `StaticValue` currently contains constant-pool
and function targets. Captured writes invalidate the same fact vector before
constants and known calls are materialized. A future invariant-shape or richer
call-target proof belongs as another `StaticValue` variant and transfer rule in
this pass—not as a new expression-specific matcher or benchmark-shaped path.
