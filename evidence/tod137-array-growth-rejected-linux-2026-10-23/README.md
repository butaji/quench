# Dense array growth 1.5x instead of 2x — rejected

Host: Linux 6.18 x86_64 VM, 4 logical CPUs. Plain production `quench-node`, MemoryFirst,
compile cache warm, evicted binaries, 7 alternating runs. Branch: `meas-gc-array-growth`.

At Crypto's largest massif snapshot, `UniqueElements::reserve`, called from
`append_dense_element`, held 419 KiB of the 1.9 MB of small allocations. RSA's digit arrays
grow one append at a time, and doubling leaves up to half of each backing as slack.

With 1.5x growth, Crypto's max RSS fell by 88 KiB without JIT and 100 KiB with it, at the same
Score. Elsewhere it rose: DeltaBlue `--jit` +184 KiB, RayTrace `--jit` +128 KiB, RegExp +108 KiB.
The other fixtures stayed within noise (`ab-7-runs.txt`).

Not kept. Trimming slack at full collections was rejected earlier
(`tod137-array-slack-trim-…`), so the growth factor stays 2x.
