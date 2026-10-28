# Rejected EarleyBoyer deferred frame-PC publication

The candidate skipped the per-dispatch `Frame.pc` write only for a small
whitelist of plain operations: `Nop`, `Wide`, `LoadConst`, plain-local load,
store and copy, `Move`, `JumpFalse`, and `Return`. It kept publication for
effectful instructions, and it explicitly excluded ordinary `LoadLocal` and
`StoreLocal` because those can hit TDZ exceptions. The production target used
the `profile-memory` feature.

Node v24.19.0 matched the candidate and baseline exactly on the focused
oracle, including getter and proxy traces, caught TDZ errors, thrown-object
identity, `finally`, and allocation pressure. The pinned EarleyBoyer fixture
output matched in all three alternating production pairs.

The candidate lost consistently:

- Median Score was 418 baseline and 367 candidate; paired median delta was
  -51 points. Candidate Score was lower in all three pairs.
- Median maximum RSS was 41,811,968 bytes baseline and 47,603,712 candidate;
  paired median delta was +5,804,032 bytes. Candidate RSS was higher in all
  three pairs.
- Median wall time rose from 22.14s to 25.33s.

This is a three-pair rejection screen, not qualification evidence. The
candidate is removed. The captured patch, oracle, exact fixture/binary hashes,
pair rows, and runner are kept here.

Online research checked V8's primary Ignition interpreter design document,
which describes the bytecode offset in an interpreter frame. That confirms
the general role of frame PC state; it does not support skipping updates in
Quench. The Quench implementation and its measured regression determine this
rejection. See [V8 Ignition](https://github.com/v8/v8/blob/main/docs/interpreter/interpreter-ignition.md).
