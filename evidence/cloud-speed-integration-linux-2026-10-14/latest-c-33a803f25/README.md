# Latest c head follow-up

Remote c moved to33a803f25 during attribution. It is merged at eb5360153.
SSE2 truncation plus exact bit roundtrip preserves i32 Numbers, negative zero,
NaN canonicalization and out-of-range doubles; other targets retain Rust cast.
Workspace/profile checks pass, runtime582/582, Number edges match Node26.10.0.
The production all-eight stock output smoke is8/8 valid and output-equal.
This one-round check overlapped compilation, so scores/wall are not performance
evidence. Earlier stock Richards baseline remains source fdc843033.
