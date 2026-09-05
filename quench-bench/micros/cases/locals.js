registerMicro({
  id: "locals",
  question:
    "Does equal useful work become more expensive with local state or call depth?",
  requires: ["calls"],
  axes: ["size", "local count", "depth"],
  observations: [
    "time per call",
    "initialized frame bytes and environment allocations, if available"
  ],
  explanations: [
    "State initialization",
    "Register traffic",
    "Recursion overhead"
  ],
  setup: function (n, seed) {
    return { n: n, seed: seed };
  },
  equivalent: [["small", "many", "body_size"]],
  variants: {
    small: function (s) {
      function f(x) {
        var a = x + 1;
        return a;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += f(i + s.seed);
      return t;
    },
    many: function (s) {
      function f(x) {
        var a = x,
          b = x,
          c = x,
          d = x,
          e = x,
          f = x,
          g = x,
          h = x;
        return a + 1;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += f(i + s.seed);
      return t;
    },
    body_size: function (s) {
      function f(x) {
        if (x < 0) {
          x += 1;
          x *= 3;
          x -= 7;
          x ^= 3;
          x += 9;
          x *= 5;
          x -= 1;
          x ^= 17;
        }
        return x + 1;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += f(i + s.seed);
      return t;
    },
    shallow: function (s) {
      function f(x, d) {
        return d ? f(x + 1, d - 1) : x;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += f(i, 2);
      return t;
    },
    deep: function (s) {
      function f(x, d) {
        return d ? f(x + 1, d - 1) : x;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += f(i, 32);
      return t;
    }
  }
});
