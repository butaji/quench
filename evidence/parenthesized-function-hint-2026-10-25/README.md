# Parenthesized function expressions keep their records

Base: e176d06c4 (`qn-cr`, same code as b96aea59b). Host: the lane's Linux cloud container
(x86_64, 4 vCPU, shared), cold mode (`evidence/jetstream3-cold-linux-2026-10-25/tools`: no
compile cache, no snapshot). Interleaved runs, median score, max RSS (KiB).

Finding: with a temporary log of every first-call replay, babel-wtb replayed 1,966 functions
over 12.1 MB of source text, 10.5 MB of it in two functions: webpack's module function for
`@babel/standalone` and the UMD factory inside it (5.25 MB each), both written as
`(function (...) { ... })` and called once while the bundle loads. The load released both, so the
5.25 MB bundle was parsed three times, and each replay's parse arena is sized from its text.

Change: `(function () {})` (and a parenthesized arrow) is read as about to run, the hint V8 and
JSC read too, and keeps its record like a function called where it is made
(`Compiler::note_invoked_callee`). It decides only whether the load keeps a record, never what
the record does. Replayed text on babel-wtb: 12.1 MB -> 1.27 MB.

Result: babel-wtb score +5.5% and max RSS -13% (83.0 -> 72.1 MiB) over nine runs
(`paired-medians-babel-9-runs.txt`); the other startup-bound rows are within noise or ahead
(babylon +5%, chai +4%, babel-minify +3.6%; `paired-medians-5-runs.txt`, whose babel row is the
noisier five-run sample).
