# 296 — Guarded-page safepoint polling instead of a per-iteration branch

Status: planned

Companion to [[193-composable-gc-safepoint-maps]], which states *what* to report at a
safepoint (live registers/frame slots for root-scanning) but not *how* the mutator
cheaply decides *whether* to stop there at all. The standard production technique
(HotSpot, V8) is a **guarded-page poll**: reserve one page of memory, normally mapped
readable, and have every safepoint-eligible location (loop backedges, function entry)
read from that page as its only "should I stop" check. Under normal execution this read
is a completely ordinary load with no branch and no measurable cost; when the runtime
actually wants every thread to stop (for the once-planned [[38]]/[[162]]/[[198]]
generational GC's collection pause), it `mprotect`s the page to no-access, and the very
next poll faults — a signal handler catches the fault and performs the actual stop —
converting a per-iteration conditional branch into a per-iteration unconditional load,
with the stop-decision cost paid *once* (the `mprotect` call) rather than once per
poll.

This is a materially cheaper mechanism than a per-loop-backedge counter check or
conditional branch, and it directly matters for this project's numeric/dense traced
loops ([[123]]/[[124]]/[[233]]) once any collector needs to interrupt them — without
this mechanism, adding GC safepoints to a hot loop would mean adding a real per-iteration
branch exactly where [[233]]'s bounds-check erasure and [[241]]'s layout work are trying
to remove branches, which would be a direct, avoidable regression against this session's
own erasure-family goals.

Concrete steps:
1. Reserve one guard page at VM startup, normally readable.
2. At every safepoint-eligible location [[193]] already identifies (loop backedges,
   function entry/exit), emit one unconditional load from the guard page as the poll —
   no branch, no counter.
3. Wire a signal handler (`SIGSEGV`/`SIGBUS` on the relevant platform) to catch the fault
   produced when the runtime `mprotect`s the page to request a stop, perform the actual
   safepoint action (using [[193]]'s stack-map data to find live roots), then resume.
4. Verify the poll's steady-state cost is genuinely free (a load already in the
   instruction stream, ideally one the CPU can hide entirely) by disassembling a
   representative traced loop before and after adding the poll.

Acceptance: safepoint polling compiles to one unconditional load with no branch at
every eligible location, verified by disassembly; triggering a stop via `mprotect`
correctly interrupts execution at the next poll and invokes [[193]]'s root-scanning data
correctly; alternating A/B on the traced numeric loops from [[123]]/[[124]]/[[233]]
shows no measurable regression from the added poll (the whole point of this mechanism);
this task is correctly sequenced *after* [[193]]'s stack-map work exists to consume,
and *before* [[38]]/[[162]]/[[198]]'s actual collector needs a way to request a stop —
it is infrastructure for GC, not a GC implementation itself.

Primary sources: HotSpot's safepoint-polling-page design and V8's analogous mechanism
are the standard references for this technique; no single canonical paper — cite as
established JVM/V8 production practice, consistent with this project's discipline of
adopting an already-solved mechanism rather than inventing an equivalent from scratch
(matching this session's earlier `fancy-regex` precedent).
