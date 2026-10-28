# Descriptor writes and native error parity

Frozen source `75cbf5280` passed 288 serial API differentials against Node 26.12.0, workspace/all-target checking, 43 framework fixtures and all nine no-JIT web correctness targets. Its Core is identical to 700bf9ba5, whose 626-test runtime qualification is retained in the read-sync evidence.

Public writeSync now preserves raw byte views, safe-number position rules, intrinsic encoding fallbacks, native binding error classes and genuine partial write counts. A real nonblocking FIFO verifies partial progress, EAGAIN and bytes read back. Unix positional writes leave the shared offset unchanged. Filesystem error names derive from the existing platform errno authority; metadata/stream error descriptions share the filesystem renderer. Read/write forwarding does not invoke public argument iterators or discard iterator replacements.

Earlier failures are retained: an unsafe position created a huge sparse file and failed the subsequent bounded read; byte-view hooks, native error prototypes and incomplete errno maps also differed. The files named write-sync-policy-qualified-differential.log and fs-errno-platform-differential.log are initial failures, superseded by the successful focused and full runs above. The sparse file was removed by the probe's cleanup; no large file is an artifact.

All 32 fresh matched Callgrind executions produced valid work with unchanged identities. DeltaBlue (+5.9236%) exceeds the 1% limit; the other seven rows pass, including Earley-Boyer (+0.6644%). This is integrated instruction evidence only, not isolated attribution, production throughput or RSS. Total Node parity and a benchmark win remain open. Subsequent process memory and host-function metadata changes are excluded from this frozen snapshot.
