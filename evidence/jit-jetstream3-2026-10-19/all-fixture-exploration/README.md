# JetStream 3 JIT exploration

The all-fixture report is a one-round exploratory run from clean commit
`de6bce02ad2102979e91965539471d5bb0364f44` on Linux x86_64. It ran all 34
pinned fixtures, but is not a qualification report: `argon2-wasm` produced no
scores and the two JSON inspector fixtures failed semantic equality, leaving 31
valid fixtures. One round is useful for ranking only.

The shared Node/Bun comparison shows Quench loses score to both on all valid
JavaScript fixtures. Some Wasm fixtures are more competitive: `gcc-loops-wasm`
scored 46.17 for Quench versus 169.97 Node and 158.79 Bun, with lower RSS than
both. `quicksort-wasm` was closest to a score win in the exploratory sweep at
88.25 versus 93.44 Node and 111.22 Bun, with lower RSS than both.

The focused 11-round quicksort report is valid and semantically equal on all
rounds. Median score / maximum RSS:

| Engine | Score | Max RSS |
| --- | ---: | ---: |
| Quench JIT | 108.62 | 42.82 MiB |
| Node JIT | 139.23 | 59.70 MiB |
| Bun JIT | 122.78 | 47.52 MiB |

Quench wins RSS, but its paired 95% score ratio interval is below 1 against
both peers: 0.8279–0.9425 versus Bun and 0.6962–0.9068 versus Node. This fixture
does not qualify yet.

The diagnostic quicksort report is profiling-only. Quench compiles 17 of 20
Wasm functions; compiled executions still make thousands of imports through
the VM. The hot target is WASI `fd_write` in the Emscripten-generated runtime.
A safe guarded import fast path or broader Wasm-to-host lowering is the next
candidate, subject to preserving mutable JS import behavior.


## Missing compressed inputs

The initial full sweep predates the corpus preparation step, so its two JSON
inspectors and `argon2-wasm` could not find their decompressed inputs. The
pinned submodule stores those resources as `.z` files. The focused follow-up
[`jetstream3-missing-inputs-jit.json`](jetstream3-missing-inputs-jit.json) ran
after inflating those assets and validates all three fixtures. After that run,
the benchmark prelude was updated to inflate a sibling `.z` on demand without
modifying the corpus; [`jetstream3-json-inspectors-jit-autodecompress.json`](jetstream3-json-inspectors-jit-autodecompress.json)
confirms both JSON cases work through that path. The benchmark preparation
change is in source but does not alter engine scoring or JetStream iterations.

Together, the exploratory and follow-up reports cover all 34 fixtures with at
least one valid round. They are separate runs and are useful for ranking only;
they do not establish suite-wide score or RSS qualification.
