export default [
  {
    ignores: [
      "tests/node/**",
      "tests/node/**/*",
      // The pinned Node source set (quench-node build input), upstream code.
      "crates/quench-node/node/**",
      "tests/node-compat/**",
      "tests/test262/**",
      "crates/quench-runtime/testdata/**",
      "crates/quench-node-test/node-tests/**",
      "crates/quench-node-test/examples/**",
      "crates/quench-wasm-test/testsuite/**",
      "target/**",
      "target-*/**",
      "node_modules/**",
      "quench-bench/**",
      "evidence/**",
      "work/**",
      "tests/frameworks/**",
      "tests/npm-*/**",
    ],
  },
];
