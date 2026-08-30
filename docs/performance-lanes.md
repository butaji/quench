# Performance lanes

Control runs use an explicit optimized artifact and report repeated raw
per-fixture results. Diagnostic builds may trace, sample, or count the same VM,
but never supply score numbers. Join evidence by artifact/profile/run identity;
unavailable data is unknown, not zero. Measurement failures are fixed in the
measurement or general runtime, never with workload-shaped behavior.
