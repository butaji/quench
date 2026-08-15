# Goal: stage 46 residuals

Bring test262 stage 46 (`built-ins/Error`) to 100% through the canonical
runner. Start from the current branch baseline, reproduce the remaining
subclass Error stack receiver failures, and implement the smallest shared
completion/prototype fix. Preserve realms, constructors, descriptors,
accessors, proxies, and observable errors. Do not edit test262 or the harness.
Run stage 46 plus regressions, strict format/clippy, and commit verified work.
