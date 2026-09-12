# 344 — Single call-IC entry-read experiment

Status: complete

Fuse the `has_loop` statistics read with the subsequent cached-call entry read so one
`OnceCell<UserCallIc>` lookup and pointer chase serves both purposes. This keeps Task 341's
memoized loop fact and changes no call semantics or site layout.

The five-pair 300 ms complete comparison in
`reports/task344-single-call-ic-entry-read-full-ab-5/comparison.txt` measures 2197.17 ->
2191.04, **-0.28%**, with mixed components. The candidate is rejected and reverted. A
single pointer chase is below the useful granularity of the current measurement and cannot
justify perturbing the call path.

Rejected binary: `/tmp/deegen-task344-single-call-ic-entry-read`, SHA-256
`0f164426fd680499d72cbea92e9295feb2c4aeeec827e514ab7305b37e5d2faa`.

