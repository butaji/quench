# Canonical StatFs presentation qualification

Repair source `7b68d182c`. Official oracle: Node v26.12.0.
The frozen executable passes268 serial API comparisons, workspace/all-target checks,
framework-core43/43 and all9 no-JIT web framework correctness combinations. The623
runtime result is reused from `4a952c16c` with unchanged runtime/dependency sources.

Rust owns the kernel statfs operation, raw integer counters, options and path handling.
The configured, unmodified Node internal/fs/utils getStatFsFromBinding function runs
as guest policy in Quench. Its canonical private class owns field assignments and
presentation. One retained function preserves constructor/prototype identity across
calls; the loader captures it before application mutation. No external runtime
executes application behavior and there is no benchmark or fixture dispatch.

Paired checks cover number/BigInt fields, descriptors, constructor name/length,
prototype identity, construction and callability, prototype mutations and setter
exceptions, plus callback and promise projection. Original before-repair output shows
Object constructors in Quench versus StatFs in Node. Numeric counters remain ordered
by the native libuv binding layout; field names come from the Node constructor.

The instruction guard measures all eight cases against the same frozen original
baseline and exact input paths, with baseline reuse recorded. Raw counts and hashes
identify the artifacts. Counts include shared-lane changes and do not isolate repair
cost, production throughput or RSS. TOD-135 retains regressions and earlier baselines.

This is a functional checkpoint. Broad Node parity, asynchronous path snapshots,
complete read/write/descriptor policy, host clone protocols and workers remain open.
No total-parity, optimization or benchmark-win claim is made.
