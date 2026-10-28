# npm capability repairs, 2026-10-15

The candidate contains integration `500e152c6` and is tested at source
`9cfb428b8f794b281febc4d002474f7245bb850f`. Latest Node v26.11.1 is the oracle.
The matched published repair baseline has 252 API scenario passes; this batch
has 260 API passes, 13 declaration-consumer passes, 26 Quench failures and one
Node platform failure (the musl native artifact on this glibc host).

New matching scenarios are jiti, fast-glob, require-from-string, prettier,
Playwright, OpenTelemetry instrumentation, globby and foreground-child.
The matched baseline comparison has no scenario regressions. Passing scenarios
are not whole-package or all-function qualification. The coverage ledger remains
false, with dynamic/returned APIs, other inputs and full upstream suites still open.

The shared repairs are:

- Native-backed ordinary constructors use ordinary receiver, return-value and
  newTarget semantics. Private traced constructor eligibility does not depend on
  a public prototype property. Plain host callbacks keep their compact representation.
- Module is a callable constructor with parent/children bookkeeping, require and
  public _compile. Public compilation uses the shared five-argument CommonJS
  wrapper, resolver and source-origin calculation, and preserves filename/loaded
  fields and exports behavior. The corpus covers ordinary string CommonJS source,
  shebang and BOM separately. Module load/cache/static hooks, custom search paths,
  alternate compilation formats and require(ESM) remain separate obligations.
- Linux os.cpus reads online CPU records, model/frequency and millisecond times
  from the same OS sources used by libuv. Qualification is on this x86_64 Linux
  host; no new non-Linux CPU implementation is claimed.
- atob/btoa share the byte codec and ordinary UTF-16 ToString. They preserve
  Latin-1 output, lone-surrogate rejection, forgiving decoding rules, original
  DOMException identity, function aliases, arity and construction behavior.
- Stream resume preserves readable-listener buffer ownership. The prior tinyexec
  stdout race reproduced in one of 40 runs; the corrected candidate retained
  stdout in all 40 repetitions. The reduced Node comparison preserves buffer
  contents and flowing state.
- Forward UTF-16 string searches round odd offsets up; byte-view needles retain
  their separate floor alignment. Latest Node exposed eleven differences in the
  merged Buffer corpus. The complete corrected corpus and explicit distinguishing
  cases match Node.
- Host checkpoints preserve requested termination after async callbacks, before
  further ticks or promise-rejection dispatch. This prevents a synthetic rejection
  from changing exit status zero to one. Async exit/reallyExit and foreground-child
  match Node in the retained cases; no complete termination-semantics claim is made.

Fast-glob now has 28 assertions across 15 public callable tags, including its
stream results and POSIX/Windows path utilities. Require-from-string has 12
assertions for compilation, exports, relative/builtin imports, filenames and errors.
Tags qualify only their retained corpus; inherited intrinsics and returned stream
APIs remain explicit obligations. Ranking, installation and discovery limits are
in `tests/npm-top300/README.md`.

Final measurement counts and engine/source/fixture hashes are in `summary.json`
and `host.json`. Six direct regressions extracted from the checked-in Rust test
fixtures match Node. The full upstream semver TAP runner passes on Node and fails
on Quench, rather than being promoted from the smaller semver contract corpus.
The instruction screen and its independent 32 completion checks are retained;
no production speed improvement is claimed.

The environment restarted during final checks. Package reports resume from
completed per-package checkpoints, requiring identical source, inventory, lock
and engine hashes, and preserve inventory order. The instruction run preserves
completed tuples and reruns missing tuples, checking successful status and exact
completion output. Resume scripts and logs are retained. Runtime qualification
restarts from the final source after the reset. Earlier 602- and 603-test snapshots
and the pre-final package/guard results are historical evidence, not substituted
for final qualification.

Remaining observed failures include cyclic ESM, require(ESM), Worker/shared
memory, JavaScript WebAssembly, N-API loading, HTTP2/HTTPS, parseArgs, stack-based
parent-module discovery, stream constructor call initialization and zlib internals.
The larger contract work also exposed assert.rejects constructor matching. These
remain open on TOD-127/TOD-128/TOD-129; no package-name production bypasses are used.
