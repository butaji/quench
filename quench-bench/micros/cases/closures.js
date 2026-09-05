registerMicro({
  id: "closures",
  question: "How do mutation, capture count, and escape affect closure cost?",
  requires: ["calls", "locals"],
  axes: ["size", "capture", "lifetime"],
  memory: true,
  observations: [
    "time per invocation",
    "retained RSS",
    "environment allocations, if available"
  ],
  explanations: ["Closure creation", "Capture access", "Retained environments"],
  setup: function (n, seed) {
    return { n: n, seed: seed, retained: [] };
  },
  variants: {
    readonly: function (s) {
      var value = s.seed;
      function f(x) {
        return x + value;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += f(i);
      return t;
    },
    mutable: function (s) {
      var value = s.seed;
      function f() {
        return ++value;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += f();
      return t;
    },
    many_captures: function (s) {
      var a = s.seed,
        b = a + 1,
        c = a + 2,
        d = a + 3;
      function f(x) {
        return x + a + b + c + d;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += f(i);
      return t;
    },
    escaping: function (s) {
      var a = [];
      function make(x) {
        return function () {
          return x;
        };
      }
      for (var i = 0; i < s.n; i++) a.push(make(i + s.seed));
      s.retained = a;
      var t = 0;
      for (var j = 0; j < a.length; j++) t += a[j]();
      return t;
    }
  },
  release: function (s) {
    s.retained = [];
  }
});
