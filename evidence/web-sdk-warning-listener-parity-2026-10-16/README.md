# SDK default warning listener qualification

Source b9745ab5b203d0974f17c18f1b5f7bb77a9ae0b6 passes 406 serial Node v26.12.0 API comparisons (275.79s), 43 framework fixtures, nine no-JIT web correctness targets and workspace/all-target check. Before/after-repeat source, CLI, runner, API-test and Node manifests match. Runtime integration passes 672 library tests (separate integration log). Later source is excluded.

The configured SDK owns the default onWarning listener, suppression, redirection and stderr policy. Its console streams are initialized through initializeGlobalConsole. Removing warning listeners removes default output. Startup flag descriptors and repeated --disable-warning options match the oracle. An initial 405/406 run failed in Node itself when a child did not print its PID within the existing 500ms deadline; that failed run, passing unchanged focused recheck and passing unchanged full repeat are all retained. No deadline was relaxed.

This source still uses a duplicate native public emitWarning parser: an options Proxy observes two extra Quench-specific property reads. The next change addresses this confirmed gap. Full Node parity, production performance qualification and a framework benchmark win remain open. Parent instruction evidence does not qualify this source. No speed claim.
