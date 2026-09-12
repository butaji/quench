# 349 — Static same-owner hoisted leaf inlining experiment

Status: complete

Test the first bounded implementation slice of [[20]] without runtime heat or benchmark
identity. After OXC lowering and before `DynJitCode::build`, capture each used same-owner
hoisted closure in a hidden caller register, guard a call site's actual callee against
that exact value, and splice an alpha-renamed straight-line exact-arity leaf body into
the caller quote. Guard failure executes the unchanged ordinary call.

The rewrite is deliberately a pure `DynCode -> DynCode` transformation. The current VM
does not retain the unforced caller/callee `StencilNode` DAG assumed by the original
Task 20 sketch; after `DynJitCode::build` begins, PC-indexed CFG, liveness, IC, site,
label, and relocation data already exist and may not be safely rewritten.

Named bounds restrict callee instructions, callee frame slots, targets, and sites. The
initial subset rejects control flow, nested calls/construction, dynamic `arguments`, and
nested closure creation. A dedicated rustc/LLVM-cooked exact-value guard has success-next
and failure-branch relocations and no Rust slow helper.

Correctness evidence required before measurement:

- duplicate hoisted declarations select the last declaration;
- arguments, result, and continuation are preserved;
- reassignment takes the ordinary-call fallback;
- a free lexical name resolves through the owning caller environment;
- the rewritten CFG and region plan remain valid.

Performance gate: compare a frozen pre-change binary against the candidate with all eight
V8v7 components, structured JIT statistics, and binary hashes. Retain only if a longer
alternating run improves the aggregate without violating the component floor. If rejected,
remove the implementation while preserving this result.

Baseline SHA-256:
`7ad3063c7406e7a81cffad4a5b49a3c4e7dbda9ed1b42a5abbf921fcaf4f8d35`.

Candidate SHA-256:
`98872d396251e96d9cc36ef44168c48e34dffc8d9825592b089945a0e05ce8a9`.

## Result: rejected and removed

The five focused semantic tests passed. A preliminary three-pair 200 ms comparison is
recorded in `reports/task349-static-hoisted-leaf-inlining/ab-3x200/comparison.txt` and
measured 2221.91 -> 2212.19 (-0.44%). More importantly, the complete-suite smoke's
structured statistics reported **zero** `compiled_inlined_calls`, zero added inline
instructions, and zero added registers across all eight suites. The implemented resolver
therefore had no V8v7 reach: the timing difference is noise, not evidence about inlining.

The longer confirmation run was stopped once zero reach made it unable to validate the
mechanism. The new guard opcode, AOT template, rewrite, and counters were removed. The
frozen baseline remains the accepted implementation.

This falsifies the target resolver, not cross-function inlining. Task 343's dynamic call-IC
census includes global, sibling, and prototype-installed functions, while this experiment
could resolve only same-owner hoisted declarations reached by a block-local load. The next
slice must consume [[317]]'s canonical call recipe and preserve an exact code/environment
identity for those broader call classes before cloning any body.
