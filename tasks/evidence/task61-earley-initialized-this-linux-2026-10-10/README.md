# EarleyBoyer initialized-this fast path

`checked_this_binding` now returns the frame's current `this` directly when it
is not the deleted pre-`super()` sentinel. `initialize_this_binding` updates
live frames when `super()` initializes a derived constructor's binding, so
only the sentinel case needs to search dynamic environments.

Node v24.19.0 matched the production candidate for strict and undefined
receivers, inherited setters that throw, derived-constructor TDZ access,
post-`super()` access through direct eval, and arrow captures. The existing
`regression_constructor_eval_uses_the_callers_super_and_this_binding` Rust
regression passed 1/1. Its first test build was blocked by two duplicate test
names in `vm/tests.rs`; I removed the identical duplicate and gave the second
RegExp test a distinct name before rerunning it.

Thirty-two alternating production pairs on EarleyBoyer matched observable
output. Against the committed strict local-setter fusion binary, median Score
rose from 401 to 407. The paired median delta was +5 points (95% bootstrap
interval +3 to +7); Score was lower in 7/32 pairs. Median maximum RSS fell
from 42,727,424 to 42,274,816 bytes. The paired median delta was −464,896
bytes (95% interval −520,192 to −372,736), with lower candidate RSS in all
32 pairs. This is a joint Quench-only EarleyBoyer improvement, not the
reference-engine Stage B qualification.

Baseline binary SHA-256: `fc9011c45104a9389710e7f075f1fc41c36b40c7d073eb996c8cc6fbe2bdaf86`.
Candidate binary SHA-256: `88e7706b6e85bc036dfe5fe7a020e86e86ae3e9d8789fb506b2aea6c3fa6590a`.
Fixture SHA-256: `aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`.
The candidate was built from source revision `89bd5ecac` plus the patch in
this directory. The paired reports, raw samples, runners and Node oracle are
also preserved here.
