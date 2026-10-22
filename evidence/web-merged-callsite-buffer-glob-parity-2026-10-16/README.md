# Merged runtime and Node API verification

Frozen source 74c05400b9f4dc7f099686aa0338936300d5db8f integrates shared c35ca413f with the eval-origin/source repairs. All 658 runtime tests, workspace/all-target checking, 374 serial stable Node v26.12.0 API comparisons (266.40 seconds), 43 framework fixtures and nine no-JIT web correctness targets pass. CLI, runner and test executable hashes remain unchanged through the API run. Source and executable identities are retained in qualification.json.

Both parents’ complete Node comparison tests survive the integration. Coverage includes the incoming Buffer pooling/concat/latin1/public policy, glob/Dirent, shadow-stack and dense-array changes, alongside the eval/source tests. The initial API build command names the wrong package and exits before compilation; the corrected quench-node-test build succeeds. Runtime tests use CARGO_INCREMENTAL=0 and two build jobs.

This checkpoint has no new performance measurement. Parent 725f91d33 instruction evidence does not qualify subsequent shared changes. Total Node parity, a throughput win, constructor debug-name/prototype-transition parity and the broader conformance backlog remain open. Later stashed constructor work is excluded.
