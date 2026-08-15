# Goal: 100% stable test262 conformance

Bring Quench to **100% passing stable test262 conformance** through the canonical `quench-test262` runner. `staging` is outside this goal and must never be counted as stable conformance.

This is the campaign's working charter, not a progress ledger. Assignments, pass rates, failure inventories, and command output belong in Herdr and the terminal, never in this repository. [`docs/STAGES.md`](docs/STAGES.md) is the authoritative stage map.

## Non-negotiable boundaries

- `crates/quench-test262` alone owns test262 metadata, harness composition, runner contracts, staging selection, and completion classification. It loads declared harness sources exactly as test262 specifies; neither it nor the runtime may modify, replace, shim, or short-circuit them.
- `crates/quench-runtime` is a pure JavaScript runtime. It contains no test262 policy, fixture, harness, metadata, or conformance-specific behavior.
- Never modify `tests/test262/`. A passing result comes from implementing JavaScript semantics, including completion ordering and observable errors.
- Preserve the frozen OXC-facts residual-VM doctrine in `AGENTS.md`: one canonical completion-aware semantic path; OXC owns syntax, scopes, and symbols; facts are `Proven`, `Guarded`, or `Unknown`; runtime is flat residual Ops over compact heap references, shapes, slots, and stack frames.
- A fix may not optimize through proxies, accessors, coercion, `Symbol.toPrimitive`, dynamic prototype mutation, direct `eval`, realms, or completion ordering. Generic semantics precede guarded specialization.

## Team topology

Herdr is the multiplexer and the single source of truth for live coordination. It hosts one coordinator and ten low-Luna Codex workers. The coordinator works only in this checkout on `main`; workers work only in their assigned worktrees:

```text
main coordinator  ->  ../quench
workers 01..10     ->  ../quench-branch-01 ... ../quench-branch-10
```

Before assigning work, the coordinator checks the actual layout with `git worktree list --porcelain` and checks every target worktree is clean. Workers never commit on, rebase, or otherwise modify `main`. The coordinator is the only writer to `main` and the only person who consolidates changes. After every consolidation, each worker updates and rebases its own assigned branch onto the published `main` SHA before accepting another assignment; the coordinator verifies that base in the assignment handoff.

Use the ten workers continuously. Start with disjoint stage ranges from `docs/STAGES.md`, balancing by current failure volume rather than stage count. When failures reveal a common semantic cause, regroup related stages under one explicit owner. A worker may fix a clearly shared root cause that its investigation proves necessary; otherwise it reports the family to Herdr and the coordinator reassigns it. No two workers edit the same semantic area without an explicit coordinator decision.

## Worker loop

Every assignment follows this loop:

1. Update and rebase the worker's assigned branch from the current coordinator-provided `main` SHA. Do not start from a stale baseline; report the resulting base SHA to Herdr before the baseline run.
2. Run the canonical stage runner for the owned range, normally:

   ```bash
   cargo run -p quench-test262 --bin run-stages -- --from <first> --to <last> --continue
   ```

   Use `triage` only to cluster or focus failures; it is diagnostic evidence, not the conformance gate:

   ```bash
   cargo run -p quench-test262 --bin triage -- <test262-subdir>
   ```

3. Report the baseline, failure family, intended semantic owner, and likely overlap to Herdr before editing. The report is a message, not a file.
4. Reproduce the family, make the smallest complete semantic fix, and avoid speculative generalization. Fix related failures as one family.
5. Add a Rust unit test only for a bug reproducer, a core invariant the runner cannot observe, or a refactor pin. Never duplicate a test262 assertion and never begin feature work by writing a failing unit test.
6. Re-run owned stages and directly affected earlier regressions. Then run the quality gate below. Leave the worktree clean apart from the intended committed change.
7. Create one focused commit with a semantic message. Hand Herdr the commit SHA, base SHA, owned-stage result, regression result, checks run, changed files, and any remaining overlap risk.

There are no checkpoint files, pass-rate tables, task ledgers, or generated failure reports committed to the repository. Commit history and ephemeral runner output are the durable evidence.

## Consolidation loop

The coordinator maintains a linear, verified `main` by selectively rebasing/cherry-picking each focused worker commit. Do not merge worker branches wholesale.

For each handoff, the coordinator:

1. Confirms the recorded base, clean worktree, focused diff, and verification evidence.
2. Checks that the implementation respects the runtime/test262 boundary and the one-canonical-path doctrine.
3. Applies the commit to current `main`, resolves only coordinator-owned integration decisions, and reruns affected stages.
4. Immediately publishes the new `main` SHA through Herdr so workers refresh their bases before their next assignment.

When `main` has advanced, the original worker rebases and resolves mechanical conflicts in its change. The coordinator resolves ownership conflicts: cases where competing changes choose different semantics or touch the same canonical owner. Never hide a semantic conflict with a mechanical resolution.

Commit and push small, verified consolidation steps as they land. Do not leave a finished verified step uncommitted. Push only the coordinator-controlled `main` history.

## Required quality gate

Before a worker hands off a change, and again after the coordinator integrates it, run the owned/affected canonical stage range plus:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings -D clippy::cognitive_complexity
tools/lint-rust.sh
```

`tools/lint-rust.sh` is the final local law: it also enforces boundaries, formatting, zero warnings, the 500-line file limit, the 40-line function limit, and cognitive complexity at most 10. Run relevant Rust unit tests where the change has a justified unit regression guard.

## Campaign completion

The campaign completes only on `main`, after all stable stages in `docs/STAGES.md` have reached zero failures through the unmodified canonical runner and a fresh full-suite run succeeds:

```bash
cargo run -p quench-test262 --bin run-all
cargo test --workspace
tools/lint-rust.sh
```

The final report must show no failures, skips, or unexpected results. Commit and push the verified final `main` state. Do not claim 100% from accumulated partial runs, from a worker branch, or from any non-canonical harness path.
