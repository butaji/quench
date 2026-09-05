registerMicro({
  id: "numeric",
  question:
    "How do numeric representation and dependency chains affect useful arithmetic?",
  axes: ["size", "representation", "dependency"],
  requires: [],
  observations: [
    "execution time per iteration",
    "numeric conversion and decode counts, if available"
  ],
  explanations: [
    "Representation-dependent costs",
    "Dependency-limited execution",
    "Repeated conversion"
  ],
  setup: function (n, seed) {
    return { n: n, seed: seed };
  },
  variants: {
    integer: function (s) {
      var a = s.seed;
      for (var i = 0; i < s.n; i++) a = (a * 33 + i) | 0;
      return a;
    },
    floating: function (s) {
      var a = s.seed / 17;
      for (var i = 0; i < s.n; i++) a = a * 0.999 + (i % 17) / 19;
      return a;
    },
    bitwise: function (s) {
      var a = s.seed;
      for (var i = 0; i < s.n; i++) a = ((a << 5) ^ (a >>> 3) ^ i) | 0;
      return a;
    },
    independent: function (s) {
      var a = s.seed,
        b = s.seed + 1;
      for (var i = 0; i < s.n; i++) {
        a = (a * 33 + i) | 0;
        b = (b * 33 + i) | 0;
      }
      return [a, b];
    },
    mixed: function (s) {
      var a = s.seed;
      for (var i = 0; i < s.n; i++) a = (a + (i % 31 === 0 ? 0.5 : 1)) % 100003;
      return a;
    },
    bigint: function (s) {
      var a = BigInt(s.seed);
      for (var i = 0; i < s.n; i++) a = (a * 33n + BigInt(i)) & 0xffffffffn;
      return a;
    }
  }
});
