# Task 61: direct Number dispatch, inline helper (Linux, 2026-10-09)

This candidate routes direct Number `Binary` instructions through a shared `#[inline(always)]` numeric helper in the numeric interpreter. The Node oracle matched for baseline and candidate, and benchmark output matched in all collected pairs.

The all-eight one-pair screen showed mixed Score results. The eleven-pair follow-up found a strong NavierStokes joint win: Score +19 points (95% interval +16 to +22) and maximum RSS −770,048 bytes (interval −880,640 to −585,728). Crypto and DeltaBlue had clear RSS reductions but Score intervals included a tie. RayTrace had no established Score gain and its maximum RSS increased by 585,728 bytes (interval +319,488 to +843,776). Splay maximum RSS fell, but Score moved −67 (interval −219 to +4), with eight lower-score pairs. Reject this broad inline form; test a shared out-of-line helper next.

Node v24.19.0 matched the full numeric edge matrix, including signed zero. Stage 10 was not run because this candidate is rejected and will not be retained. This is not Task 61 qualification: only five fixtures received paired Quench-only follow-up, and Quench still trails the reference engines in the all-eight screen.

See [report.json](report.json), [candidate.patch](candidate.patch), and the raw artifacts under `target/iteration/task61-direct-number-dispatch-inline-linux-2026-10-09/`.
