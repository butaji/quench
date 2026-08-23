# Quench benchmark harness

This directory contains a vendored copy of the `js-engine-benchmark` repository as a git submodule and a small runner for executing the V8-v7 suite on `quench-runtime`.

## Run on quench-runtime

```sh
node quench-bench/run-quench-runtime.mjs
```

Optional flags:

- `--binary PATH` use a specific `quench-node` binary
- `--out FILE` write the benchmark JSON snapshot to `FILE`
- `--only suite1,suite2` run only selected suites (e.g. `richards,deltablue`)
- `--help` show usage

You can also run from npm script:

```sh
npm run bench:quench-runtime
```

The runner generates `quench-bench/dist/*.js` artifacts (ignored by git), executes each suite via `quench-node`, and prints a JSON record containing the raw text and parsed results.
