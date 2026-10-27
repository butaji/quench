# TOD-20 local build-headroom preservation manifest

Prepared 2026-10-27 for review only. No archives were created and no source
executables were moved, replaced, or removed. Keep the six originals below and
all census evidence intact while approval is pending.

Current task HEAD: `efc6f355869c67b77b37fdbc7b8d778927ef7da1`.
Current `/tmp` free space: `83,853,312` bytes. Planned release link headroom:
approximately `200,000,000` bytes. Archive destination directory, if later
authorized: `/tmp/tod20-v407-preserved-cli-archives/`.

| Original executable | Bytes | SHA-256 | Intended archive |
| --- | ---: | --- | --- |
| `/tmp/tod20-v406-quench-node` | 40,261,792 | `7ddaef7181aba0a97dfc2e06dfc5305d5e218766eb8462d9b6f1505756d375ad` | `/tmp/tod20-v407-preserved-cli-archives/tod20-v406-quench-node.gz` |
| `/tmp/tod20-v164-session-target/release/quench-node-h2-close-v2-failed` | 40,237,504 | `7e55f4b5ac82fa31b62fa6cdd93cf4b3ebff6111849da910123d965238c91e7a` | `/tmp/tod20-v407-preserved-cli-archives/quench-node-h2-close-v2-failed.gz` |
| `/tmp/tod20-v164-session-target/release/quench-node-h2-fallback-919a` | 40,237,504 | `919a891bc2c4d4af65b85573735c835b0c89fc27a7b47dd69c81638333431234` | `/tmp/tod20-v407-preserved-cli-archives/quench-node-h2-fallback-919a.gz` |
| `/tmp/tod20-v164-session-target/release/quench-node-h2-refined-guard-candidate` | 40,323,136 | `f506c36b128fbf26eaca98bc3ec8881ec3d5ccfb41015d36d169b28326c647f5` | `/tmp/tod20-v407-preserved-cli-archives/quench-node-h2-refined-guard-candidate.gz` |
| `/tmp/tod20-v164-session-target/release/quench-node-nodefaithful-v7-preserved` | 40,323,200 | `91d043ef08e9c3f4a7a31f0b070f96076349fd410e02280f4cbac7183ead6e1b` | `/tmp/tod20-v407-preserved-cli-archives/quench-node-nodefaithful-v7-preserved.gz` |
| `/tmp/tod20-v164-session-target/release/quench-node-v5-rejected-preserved` | 40,237,504 | `b26f573f331b2d824b630c22bc9e6b4ceff4b39c2023ec78339e48f888991158` | `/tmp/tod20-v407-preserved-cli-archives/quench-node-v5-rejected-preserved.gz` |

Raw originals total `241,620,640` bytes. If separately authorized and the
execution control permits it, the recovery operation is: create each gzip in
the destination, decompress and compare each result SHA-256 with this manifest,
record archive hashes and sizes, and only then consider removing an original.
This manifest does not authorize removal. The exact pinned `run-parallel`
binary, v406 result/checkpoint, all baseline binaries, all logs, and all other
candidate artifacts are outside this scope and must remain untouched.
