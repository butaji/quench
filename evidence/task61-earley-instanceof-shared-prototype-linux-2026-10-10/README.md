# EarleyBoyer shared ordinary-prototype path for `instanceof` (2026-10-10)

The residual profile records about 16.8 million `instanceof` operations on
EarleyBoyer; 16.5 million have heap values on both sides. I changed the
ordinary `[[GetPrototypeOf]]` case to read the prototype stored in the cell's
object header. The shared `Cell::ordinary_prototype` accessor excludes
proxies, and those still use Quench's generic path so `getPrototypeOf` traps,
revocation, and exceptions remain observable.

The ECMAScript [OrdinaryHasInstance algorithm](https://tc39.es/ecma262/#sec-ordinaryhasinstance)
iterates through the instance's `[[GetPrototypeOf]]` chain. Quench's ordinary
object implementation returns the stored header prototype; the optimized path
uses that same representation directly. Node v24.19.0 matched both binaries
on direct and multi-hop instances, prototype mutation, primitive operands,
custom `@@hasInstance` getters and calls, proxy instances and constructors,
revoked proxies, thrown-object identity, and subclass chains.

On the materialized V8-v7 EarleyBoyer fixture, all 11 alternating production
pairs had equal output. Median Score rose from 397 to 414; the paired median
delta was +13 points (100,000-resample 95% interval [+7, +19]). Score improved
in 10/11 pairs. Median maximum RSS was 42,749,952 bytes on baseline and
42,778,624 bytes on candidate (paired delta +77,824 bytes; 95% interval
[−192,512, +151,552]); candidate RSS was lower in 5/11 pairs. This is a
repeatable Score win with no clear RSS change, not a memory win or Stage B
qualification. I retained the speed improvement and am continuing to reduce
RSS on this same fixture.

Baseline binary SHA-256:
`ef44efdbab5c74177f23b610c72e65395a97f42627c5d9e088f9c7b757d8339f`.
Candidate binary SHA-256:
`a06f934e20c188c152490a25e86c4d64991b1083631f338e7dbceb0f00307c3a`.
Fixture SHA-256: `aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`.
Node oracle SHA-256:
`a56d55f22cb6e6fbdc856a83045f32c1225f5e4fe6cf163ecaf996197967b004`.

The measurement ran on the Linux x86_64 host in [host.txt](host.txt), with the
instrumentation-free `production` profile. The [paired report](paired-11.json)
and [raw rows](paired-11.jsonl), runner, Node oracle, patch, and per-run output
are preserved here. The earlier narrow immediate-prototype pilot remains a
separate rejected result; this retest uses the later `v2-cloud` accumulated
candidate, the current scoped profile, and a generic ordinary-object walk.
