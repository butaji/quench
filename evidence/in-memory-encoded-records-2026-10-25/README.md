# In-memory encoded records for early-released functions (shelved)

Base: b96aea59b. Host: the lane's Linux cloud container (x86_64, 4 vCPU, shared), cold mode
(`tools/` of `evidence/jetstream3-cold-linux-2026-10-25`: no compile cache, no snapshot).
Five interleaved runs per binary, median score, max RSS (KiB).

Idea: when the load releases a replayable function (`Compiler::finish_early`), lower it and keep
its record in this run's own binary encoding (`binary::write_function`, program atom ids), then
publish the buffer as the program's `EncodedRecords`, so a first call decodes instead of
re-parsing and compiling the function again. Not a cache: the bytes are this run's compile output.

Correctness: one difference found and fixed in the patch (a parent names an anonymous definition
after the child's record is written, so the decoded record takes `inferred_name` from its header).
Runtime unit tests 967/967 with the fix.

Result (`paired-medians-keep-all.txt`): scores +2..6% on most startup-bound rows (babel-wtb +3.8%,
babel-minify +4.2%, postcss +6%, prettier +4.6%, typescript-lib +5.7%), but max RSS
+0.5..12 MiB (typescript-lib 172 -> 184 MiB, babel-minify +7%, prettier +5%). Encoded records cost
about what the lowered records do in memory (`record-sizes.txt`: typescript-lib 18,940 records,
12.3 MB; babel-wtb 8,259 records, 7.9 MB; the smallest record is about 200 bytes).

Keeping only records of at most 512 bytes (`paired-medians-keep-up-to-512-bytes.txt`: 53-68% of
functions, 19-29% of bytes) loses: scores 0..-7%. The win comes from large functions, which are
where the memory goes, so no size cut keeps the score gain without the RSS cost.

Decision: shelved. These rows trail Node by 0.42-0.77x on score, so +4% does not change any row's
standing while it gives up a fifth to a half of their RSS margin over Bun (typescript-lib 1.12x ->
about 1.05x). It also runs against TOD-196 stage 2 (compile bodies on first call), which removes
the load-time compile this experiment adds to. `encoded-records.patch` keeps the change, with its
temporary `QUENCH_TMP_*` measurement knobs.
