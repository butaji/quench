use super::*;

/// Throughput pacing, the default (TOD-167): the JetStream lane's table (v2-cloud-jet), which
/// the other lanes' measurements bound. Young collections run every eighth of a live set, at
/// least `THROUGHPUT_MINIMUM_YOUNG` and at most `THROUGHPUT_MAXIMUM_YOUNG` weight
/// (`THROUGHPUT_SURVIVING_YOUNG_FACTOR` times that past a quarter's young survival on a steady
/// heap); a full one once the
/// old generation has doubled or one live set of allocation has passed since the last one, at
/// least `THROUGHPUT_MINIMUM_FULL_INTERVAL` apart; an unproductive full collection doubles the
/// allocation cap, at most twice; past half survival, full collections with one live set
/// between them, probing survival every fourth one.
///
/// - The eighth: the npm lane's young share (v2-npm abd3484b9, npm top-300, x86_64 4 CPUs,
///   matched against the memory-first table: median wall 63.1 -> 57.9 ms, fastest of three
///   143 -> 164 of 299, median peak RSS 14.4 -> 14.7 MiB) and the JetStream lane's (UniPoker
///   Score 12.6-13.1 -> 18.4-19.2 at max RSS 12 -> 16-17 MB, stanford-crypto-aes 25.7-27.5 ->
///   29.4-32.1 at 22.5 -> 32 MB, Bun no-JIT 47 MB; x86_64 4 vCPU, 2026-10-21). The Score and
///   web lanes ran eight live sets per young collection; with `MAXIMUM_YOUNG` (web) that is
///   the cap on any live set past 2^14, and this table reaches the same cap from 2^20.
/// - Full collections: one live set of growth or allocation, as under memory-first, with the
///   cap doubling after full collections that reclaim under an eighth of their expected live
///   weight. Two doublings reach the Score and web lanes' four live sets only where full
///   collections do not pay (v2-cloud-jet, x86_64 4 vCPU, 2026-10-22, median of 3-5 against
///   no back-off: source-map-wtb 6.58 -> 7.46 at max RSS 56 -> 58 MB, chai-wtb 14.6 -> 15.8,
///   web-ssr 18.9 -> 19.6 at 103 -> 110 MB, Bun 119.5; three doublings took web-ssr to 154 MB).
/// The arena growth floor (`GenerationalPacing::minimum_arena_growth`): 8 MB of values, so a
/// small heap's arena never paces its collections.
pub(super) const MINIMUM_ARENA_GROWTH: usize = 1 << 20;

pub(super) const THROUGHPUT_PACING: GenerationalPacing = GenerationalPacing {
    young: Ratio::new(1, 8),
    minimum_young: THROUGHPUT_MINIMUM_YOUNG,
    maximum_young: THROUGHPUT_MAXIMUM_YOUNG,
    surviving_young_from: SURVIVING_YOUNG_FROM,
    surviving_young_factor: THROUGHPUT_SURVIVING_YOUNG_FACTOR,
    steady_live_growth: STEADY_LIVE_GROWTH,
    old_growth: THROUGHPUT_OLD_SIZING.widest(),
    old_sizing: THROUGHPUT_OLD_SIZING,
    minimum_old_growth: THROUGHPUT_MINIMUM_FULL_INTERVAL,
    allocation_cap: THROUGHPUT_OLD_SIZING.widest(),
    minimum_allocation_cap: THROUGHPUT_MINIMUM_FULL_INTERVAL,
    productive_full: Ratio::new(1, 8),
    maximum_cap_backoff: 2,
    survival_limit: THROUGHPUT_SURVIVAL_LIMIT,
    probe_interval: 4,
    weight_growth: Ratio::new(0, 1),
    minimum_external_budget: THROUGHPUT_MINIMUM_EXTERNAL_BUDGET,
    minimum_program_growth: MINIMUM_PROGRAM_GROWTH,
    arena_growth: Ratio::new(1, 8),
    minimum_arena_growth: MINIMUM_ARENA_GROWTH,
    promotion_age: THROUGHPUT_PROMOTION_AGE,
    promoted_garbage_aging: THROUGHPUT_PROMOTED_GARBAGE_AGING,
    aging_interval: THROUGHPUT_AGING_INTERVAL,
    ages_from_start: THROUGHPUT_AGES_FROM_START,
};

/// Memory-first pacing on the owner's direction of 2026-10-17 (TOD-123), selected by
/// `MemoryPolicy::MemoryFirst` since TOD-167: young collections every sixty-fourth of a live
/// set (at least 384 weight; from an eighth, with read-only pages released after every
/// collection: V8-v7 max RSS RegExp 7244-7500 -> 6852 KiB, EarleyBoyer 7792-8000 -> 9040-9160,
/// the floor-bound fixtures unchanged, static x86_64 unbolted, 2026-10-18; RSS is this
/// table's only criterion), a full collection once the old generation has doubled or one live
/// set of allocation has passed since the last one, and, when more than half of young
/// allocations survive, full collections with one live set between them, probing survival
/// with a minor collection every fourth one. The full-collection minimums are the pre-generational 384-cell
/// headroom. The Score lane's table (eight live sets per young collection, four per full one,
/// 2^15 and 2^18 minimums) raised npm peak RSS by up to 17 MiB per package (semver 27.6 ->
/// 45.2, typescript-eslint 188 -> 226).
///
/// The young floor is `YOUNG_MINIMUM_ALLOCATION`, not the 384 headroom: at a sixty-fourth of a
/// small live set the floor decides the interval, and 384 collected small programs every few
/// hundred allocations without lowering their peak (TOD-161, quench-node scorecard, x86_64
/// 4 vCPU, 2026-10-20: 768 against 384 gave V8-v7 Score x1.161 with RayTrace x1.54 and
/// RegExp x1.60 in 7/7 pairs, frameworks x1.023, max RSS geomean x0.997 and x0.999).
pub(super) const MEMORY_FIRST_PACING: GenerationalPacing = GenerationalPacing {
    young: Ratio::new(1, 64),
    minimum_young: YOUNG_MINIMUM_ALLOCATION,
    maximum_young: MAXIMUM_YOUNG,
    // RSS is this table's first criterion (`MEMORY_FIRST_SURVIVING_YOUNG_FACTOR`).
    surviving_young_from: SURVIVING_YOUNG_FROM,
    surviving_young_factor: MEMORY_FIRST_SURVIVING_YOUNG_FACTOR,
    steady_live_growth: STEADY_LIVE_GROWTH,
    old_growth: MEMORY_FIRST_OLD_SIZING.widest(),
    old_sizing: MEMORY_FIRST_OLD_SIZING,
    minimum_old_growth: MEMORY_FIRST_MINIMUM_ALLOCATION,
    allocation_cap: MEMORY_FIRST_OLD_SIZING.widest(),
    minimum_allocation_cap: MEMORY_FIRST_MINIMUM_ALLOCATION,
    productive_full: Ratio::new(1, 8),
    // Memory-first: the cap never backs off (`maximum_cap_backoff`), as an unproductive full
    // collection's back-off trades max RSS for fewer collections (v2-cloud-jet 551f170f7).
    maximum_cap_backoff: 0,
    survival_limit: SURVIVAL_SCALE / 2,
    probe_interval: 4,
    weight_growth: Ratio::new(0, 1),
    minimum_external_budget: MEMORY_FIRST_MINIMUM_EXTERNAL_BUDGET,
    minimum_program_growth: MINIMUM_PROGRAM_GROWTH,
    arena_growth: Ratio::new(1, 8),
    minimum_arena_growth: MINIMUM_ARENA_GROWTH,
    promotion_age: MEMORY_FIRST_PROMOTION_AGE,
    promoted_garbage_aging: NO_AGING,
    aging_interval: 0,
    ages_from_start: false,
};

/// The external budget's floor (`GenerationalPacing::minimum_external_budget`) under
/// throughput. Like V8's external memory limit it bounds dead buffers without collecting at
/// every chunk: on the SaltyAom video route (64 KiB chunks, Linux x86_64, 4 vCPU) one
/// collection per 64 KiB buffer cut peak RSS 112 -> 47 MB and throughput by two thirds. A
/// stream that reads each chunk into a fresh buffer (`fs.createReadStream`) left the floor
/// of dead buffers resident: at 8 MiB a fastify server's video phase added 3.5-5.8 MB, and
/// web's board found 2-4 MiB as low as 1 MiB there. Below 4 MiB hono's video route, which
/// collects about ninety times a second anyway, paid for the extra collections: at 1 MiB
/// 1447-1680 collections in 10 s against 923-961 at 8 MiB and 898-959 at 4 MiB, 3.5% fewer
/// requests a second and a 1.4 MB higher peak (pinned server, bombardier 10 connections,
/// 4 rounds, plain x86_64 build, 2026-10-26).
pub(super) const THROUGHPUT_MINIMUM_EXTERNAL_BUDGET: usize = 4 << 20;
/// The external budget's floor under memory-first: the lowest fastify video-phase peak of the
/// sweep above, run under throughput pacing (final VmHWM 39.0-40.2 MB against 39.8-41.3 at
/// 1 MiB and 39.4-43.1 at 8 MiB); 256 KiB gave no lower peak. Memory-first trades the extra
/// collections for it.
pub(super) const MEMORY_FIRST_MINIMUM_EXTERNAL_BUDGET: usize = 512 << 10;

/// The program growth floor (`GenerationalPacing::minimum_program_growth`) under either policy.
/// At Throughput's 4 MiB external budget, a test that evaluates its source each iteration kept
/// every iteration's program (0.5-0.7 MB of footprint each) until one full collection a run
/// (JetStream 3 no-JIT max RSS, x86_64, 2026-11-15, 5-9 runs: mobx-startup 21.8 -> 18.4 MB,
/// web-ssr 46.0 -> 40.6, prismjs 18.6 -> 18.1; typescript-lib, jsdom, threejs, inspectors and
/// octane-code-load equal; scores within noise). At 768 KiB mobx-startup kept them.
pub(super) const MINIMUM_PROGRAM_GROWTH: usize = 512 << 10;

/// The fewest allocations between collections under memory-first, whatever survived: the
/// pre-generational minimum headroom.
pub(super) const MEMORY_FIRST_MINIMUM_ALLOCATION: usize = 384;
/// The most allocations between young collections, however large the live set, under
/// memory-first. Without it the young budget is eight live sets: a server whose startup leaves a
/// few hundred thousand live cells (the Node library plus a framework) allocated millions
/// between collections, and its peak RSS was mostly garbage. Measured with the `gc-tuning`
/// build, Linux x86_64, 4 vCPU, 500 connections:
/// peak RSS hono 84 -> 53 MB, fastify 96 -> 72 MB, express 85 -> 53 MB; V8-v7 earley-boyer
/// 46 -> 28 MB and splay 161 -> 92 MB, with Scores within the run-to-run band. 2^16 cut little
/// more memory.
pub(super) const MAXIMUM_YOUNG: usize = 1 << 17;

/// The fewest allocations between young collections under memory-first. At 768 (twice the
/// headroom, TOD-161) small heaps collected thousands of times, and collection-time page
/// release refaulted the run's code after each: V8-v7 Score x0.80 of throughput's, x0.82 with
/// `--jit`. At 2^13 that is x0.93 and x0.98 with max RSS geomean x0.64 and x0.65 of
/// throughput's against x0.63 and x0.62 at 768; 2^12 gave x0.89 and x0.94 (TOD-167 sweep,
/// static + BOLT x86_64, 4 vCPU, 2026-10-22, 5 and 3 runs; releasing pages only near the
/// peak added nothing at 2^13). 1024 raised koa's max RSS by 1.1 MB under the earlier table.
/// Since the first-call and memo-table changes, 2^12 against 2^13 lowers median max RSS on 14
/// of 16 V8-v7 rows by 76-416 KiB (Splay flat) and the worst run on 14, for Score x0.86-0.88 on
/// DeltaBlue, RayTrace and RegExp, x0.91 on EarleyBoyer and none elsewhere; this table's
/// criterion is RSS (one gc-tuning binary, 9 interleaved runs, x86_64 4 vCPU, 2026-10-23;
/// 2^11 cost RayTrace x0.65 for little more).
pub(super) const YOUNG_MINIMUM_ALLOCATION: usize = 1 << 12;

/// The fewest allocations between young collections under throughput: above the npm lane's
/// 2^14 (measured only against memory-first's 768) and below the 2^17 that doubled small-heap
/// RSS (v2-cloud-jet: stanford-crypto-aes 29 -> 60 MB); memory-first's 768 left UniPoker at
/// 0.7x this table's Score, and the Score lane's 2^15 against 2^17 was within noise on V8-v7
/// (v2-cloud-c). Three quarters of 2^16: JetStream rows whose young generations are all
/// garbage peak lower with no Score cost (v2-cloud-jet, one gc-tuning binary, cold, 7
/// interleaved runs, x86_64 4 vCPU, 2026-10-25; Score / max RSS): stanford-crypto-aes +10% /
/// 34.8 -> 30.4 MB, bigint-noble-ed25519 +4% / 31.4 -> 28.7, UniPoker flat / 22.1 -> 19.9,
/// hash-map flat / 27.3 -> 25.9, pbkdf2 and sha256 within noise, acorn-wtb +4% / +2.6 MB.
/// Lowered to 2^12 (TOD-1229): at three quarters of 2^16 a small V8-v7 heap such as Richards
/// never collected, so its whole run's garbage stayed resident (max RSS 5304 -> 3584-3732 KiB).
/// Raised to 2^14: npm packages' live sets (about 10k weight) sit on the floor, and at 2^12
/// react-ssr ran 12151 young collections in 14.5 s. npm-perf no-JIT, 40000 rounds, 5
/// interleaved runs: geomean x1.047 (next x1.09, zod x1.10, ws x0.94); V8-v7 max RSS (rss-exec)
/// Richards 5436 -> 6208 KiB, DeltaBlue 8312 -> 6864, others +0.1-0.8 MB (x86_64, 2026-11-08).
pub(super) const THROUGHPUT_MINIMUM_YOUNG: usize = 1 << 14;
/// The most allocations between young collections under throughput: 2^19, for Score, which
/// this table serves (memory-first keeps RSS first). V8-v7 fixed work, gc-tuning build,
/// callgrind main thread, 2^19 young with the full interval at 2^21 against 3 << 14 and 2^18
/// (v2-cloud-c, x86_64 4 vCPU, 2026-10-28): EarleyBoyer x0.838, RegExp x0.883, Splay x0.889,
/// Crypto x0.974, DeltaBlue x0.980, RayTrace x0.982, Richards x0.994, NavierStokes x0.993 of
/// instructions; stock Score, 7 CPU-alternating pairs: EarleyBoyer x1.08, RayTrace x1.05,
/// Splay x1.03. A ceiling, not a floor: as the floor too it made every young generation that
/// size, and a server's young generations are mostly garbage held while hundreds of requests
/// are in flight (TOD-167, gc-tuning build, 500 connections, ping/query/body/video, final
/// VmHWM, 2 rounds): fastify 81.5 / 78.1 MB and hono 90.7 / 86.3 MB with 2^19 fixed and the
/// full interval at 2^21; 80.4 / 78.3 and 89.0 / 89.8 with only the old-growth floor back at
/// 2^18; 42.2 / 40.7 and 41.6 / 39.4 with this ceiling over `THROUGHPUT_MINIMUM_YOUNG` and
/// `THROUGHPUT_MINIMUM_FULL_INTERVAL` at 2^18 (before the raise: 43.9 and 45.0 MB), request
/// rates within the run spread. Raised to 2^20 with `THROUGHPUT_PROMOTION_AGE`: fastify
/// 37.6 / 37.8 -> 37.1 / 37.2 MB, hono 36.2 / 34.2 -> 33.4 / 33.4 MB (2 rounds); a live set
/// past 2^23 reaches it, which no V8-v7 row does (EarleyBoyer and Splay unchanged).
pub(super) const THROUGHPUT_MAXIMUM_YOUNG: usize = 1 << 20;
/// The fewest allocations, or old-generation growth, between full collections under
/// throughput: the JetStream, Score and web lanes' common minimum (v2-cloud-jet, v2-cloud-c,
/// v2-web). Against memory-first's 384 the Score lane's table, which shares it, ran the Earley
/// half of EarleyBoyer in about 1190M instructions instead of 2318M, with 816 fewer
/// collections of the whole live set (v2-cloud-c, x86_64 4 vCPU). At 2^21 a server's promoted
/// request objects, which die soon after their first young collection, wait that long for a
/// full one (`THROUGHPUT_MAXIMUM_YOUNG`'s rows).
pub(super) const THROUGHPUT_MINIMUM_FULL_INTERVAL: usize = 1 << 18;
/// Smoothed young survival past which throughput collects fully (`Regime::FullOnly`): three
/// quarters. npm module loading keeps about 52% of each young generation (`QUENCH_TRACE=gc`,
/// typescript-eslint: 531/1024), and at half the heap went FullOnly and fully re-marked the
/// growing module graph four times (live weight 159K -> 248K -> 383K -> 603K). At three
/// quarters it stays generational there (v2-npm, Throughput, no JIT, warm snapshot, 4-vCPU
/// x86_64 VM, 2026-10-24, 9 interleaved runs, CPU / max RSS): typescript-eslint
/// 841 -> 767 ms / 126.4 -> 122.6 MB (fulls 4 -> 2, full mark 115 -> 28 ms),
/// @typescript-eslint/eslint-plugin 940 -> 841 / 125.8 -> 124.1, @testing-library/dom
/// 728 -> 719 / 97.6 -> 93.6, playwright 476 -> 472 / 77.9 -> 77.9, jsdom (median of 15)
/// 500 -> 428 / 85.6 -> 83.8; packages without full collections within noise.
pub(super) const THROUGHPUT_SURVIVAL_LIMIT: u32 = SURVIVAL_SCALE * 3 / 4;
/// Smoothed young survival from which a steady heap's young budget widens, under either
/// policy (`GenerationalPacing::surviving_young_from`): a quarter. V8-v7 survival
/// (`QUENCH_TRACE=gc`, per 1024): RayTrace 1, Crypto 5, DeltaBlue 20, RegExp 116,
/// EarleyBoyer 322; Splay's 950 collects fully (`THROUGHPUT_SURVIVAL_LIMIT`). A larger young
/// budget for every heap cost RayTrace x0.89 of Score (its young generations are all garbage
/// and a larger one only leaves the caches), where EarleyBoyer, whose minor collections each
/// marked about 4.8M instructions' worth of survivors, gained.
pub(super) const SURVIVING_YOUNG_FROM: u32 = SURVIVAL_SCALE / 4;
/// Live-set growth between full collections within which a heap counts as steady
/// (`GenerationalPacing::steady_live_growth`): an eighth. EarleyBoyer's live set after each
/// full collection moves within 6-10K weight in both directions; a loading jsdom grows
/// 121K -> 188K between its two.
pub(super) const STEADY_LIVE_GROWTH: Ratio = Ratio::new(1, 8);
/// The young budget's factor past `SURVIVING_YOUNG_FROM` under throughput, on a steady heap
/// only (`PacingState::steady`): two. Eight gave EarleyBoyer Score x1.025 at max RSS
/// 11.1 -> 15.1 MB (pinned, 11 runs, 2026-10-26; ungated x1.047), and on npm's heaviest rows
/// the steady gate kept max RSS within 0.5 MB of the unwidened table and CPU within 5%
/// (TOD-197; ungated, the factor raised it 5-16 MB). On JetStream's steady heaps eight was
/// mostly garbage: against two, max RSS geomean x1.07 over the 34 default rows for Score
/// x1.006, web-ssr 106 -> 163 MB, prettier-wtb 42 -> 65, sync-fs 25 -> 42, hash-map 31 -> 45,
/// doxbee-async 45 -> 54 (median of 3, Score +6.7%); one against two gave RSS x0.99 more
/// for doxbee-async's Score x0.94 (one gc-tuning binary, cold, interleaved, x86_64 4 vCPU,
/// 2026-10-27, v2-cloud-jet).
pub(super) const THROUGHPUT_SURVIVING_YOUNG_FACTOR: usize = 2;
/// The young budget's factor past `SURVIVING_YOUNG_FROM` under memory-first: one, so
/// memory-first's budget does not widen. Four, on a steady heap, gave EarleyBoyer Score
/// 4002 -> 4629 at 7328 -> 7740 KiB but RegExp 6316 -> 6932 KiB for no Score (pinned, 7 runs,
/// 2026-10-26): one warm-up window of RegExp crosses a quarter's survival. Neither requiring
/// up to four steady full collections nor a steady tolerance up to one live set separates the
/// two; a survival threshold does only in a 288-320 band, too narrow to hold beyond V8-v7.
pub(super) const MEMORY_FIRST_SURVIVING_YOUNG_FACTOR: usize = 1;
