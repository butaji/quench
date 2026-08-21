export default [
  {
    ignores: [
      "tests/node/**",
      "tests/node/**/*",
      "target/**",
      "node_modules/**",
    ],
  },
  {
    rules: {
      "complexity": ["error", { max: 10 }],
      "max-lines": ["error", {
        max: 500,
        skipBlankLines: true,
        skipComments: true,
      }],
      "max-lines-per-function": ["error", {
        max: 40,
        skipBlankLines: true,
        skipComments: true,
      }],
    },
  },
];
