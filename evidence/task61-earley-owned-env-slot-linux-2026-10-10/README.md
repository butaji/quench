# Earley owned environment-slot fast read rejected

The candidate read an owned environment slot from its environment cell
immediately, avoiding the current two-stage `environment_slot_owner` then
second heap lookup sequence. Shared slots retained the existing owner lookup.
The Node v24.19.0 and Quench oracle outputs matched for captured parameters,
mutations, nested and per-iteration block closures, and captured rest arguments.

Eleven alternating Linux production pairs used the pinned materialized
EarleyBoyer input (`aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`).
All runs exited successfully and produced equal semantic output. Median Score
moved from 380 to 385 (+6 paired points; bootstrap 95% interval 0 to +15).
Median maximum RSS rose 77,824 bytes (95% interval -4,096 to +126,976); RSS
was lower in only four of eleven candidate runs. The score interval includes a
tie, and memory did not improve. The candidate was removed.

The patch, raw paired report, and Node oracle are preserved beside this record.
The test candidate SHA-256 was
`4f82a11eec505125e40f93e9bebf4ac04c198583040f924bd1b155f86232cded`; the
baseline was
`f3940bcc9cff03f9bf3c6888864e3d96586d4991f4c56df4cc1e87d829e9fe00`.
