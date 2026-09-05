registerMicro({
  id: "calls",
  question:
    "What is the cost of call boundaries, arguments, receivers, and changing targets?",
  requires: ["numeric"],
  axes: ["size", "call shape"],
  observations: [
    "time per call",
    "allocations and argument transfers per call, if available"
  ],
  explanations: [
    "Call setup",
    "Argument handling",
    "Target diversity",
    "Receiver handling"
  ],
  setup: function (n, seed) {
    return {
      n: n,
      seed: seed,
      f: function (x) {
        return (x * 33 + 7) | 0;
      },
      g: function (x) {
        return (x * 33 + 7) | 0;
      }
    };
  },
  equivalent: [
    ["inline", "direct", "changing", "receiver", "bound", "arguments"]
  ],
  variants: {
    inline: function (s) {
      var x = s.seed;
      for (var i = 0; i < s.n; i++) x = (x * 33 + 7) | 0;
      return x;
    },
    direct: function (s) {
      var x = s.seed;
      for (var i = 0; i < s.n; i++) x = s.f(x);
      return x;
    },
    changing: function (s) {
      var x = s.seed;
      for (var i = 0; i < s.n; i++) x = (i % 7 ? s.f : s.g)(x);
      return x;
    },
    receiver: function (s) {
      var o = {
        bias: 7,
        f: function (x) {
          return (x * 33 + this.bias) | 0;
        }
      };
      var x = s.seed;
      for (var i = 0; i < s.n; i++) x = o.f(x);
      return x;
    },
    bound: function (s) {
      var f = s.f.bind(null),
        x = s.seed;
      for (var i = 0; i < s.n; i++) x = f(x);
      return x;
    },
    arguments: function (s) {
      function f(a, b, c, d, e, f) {
        return (a * b + c + d + e + f) | 0;
      }
      var x = s.seed;
      for (var i = 0; i < s.n; i++) x = f(x, 33, 1, 2, 3, 1);
      return x;
    }
  }
});
