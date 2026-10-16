# Constructor-stack qualification regression

The 992e960e2 source passes 649 runtime tests, workspace/all-target checking and focused Node v26.12.0 comparisons for constructor CallSites, inferred names and post-capture name mutation. Its complete serial API run fails: 347/348 pass, exit 101. Test and runner hashes remain unchanged before/after the run.

The existing native_inspection_class_fallback_uses_slots_and_data_tags_without_public_getters comparison expects Array for a detached array; Quench reports Object. The shared constructor-name helper matches legacy Cell::Array but omits the direct-record array arena. df33c1767 repairs it through the authoritative heap.array projection, removing the redundant legacy-only branch. Fresh complete qualification is separate.

Framework/web/instruction checks were cancelled before starting after the API gate failed. There are no throughput, RSS, instruction-delta or benchmark-win claims for this snapshot. These selected passes do not establish total Node parity.
