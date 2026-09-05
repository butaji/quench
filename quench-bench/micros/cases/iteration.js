registerMicro({
  id: "iteration",
  question: "What cost comes from iterator protocols, suspension, and closing?",
  requires: ["arrays", "calls"],
  axes: ["size", "protocol"],
  observations: ["time per yielded value", "iterator close effects"],
  explanations: ["Protocol overhead", "Result allocation", "Suspension cost"],
  setup: function (n) {
    var a = [];
    for (var i = 0; i < n; i++) a.push(i);
    return { n: n, a: a };
  },
  equivalent: [["indexed", "builtin", "custom", "generator"]],
  variants: {
    indexed: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.a[i];
      return t;
    },
    builtin: function (s) {
      var t = 0;
      for (var x of s.a) t += x;
      return t;
    },
    custom: function (s) {
      var iterable = {};
      iterable[Symbol.iterator] = function () {
        var i = 0;
        return {
          next: function () {
            return { value: i, done: i++ >= s.n };
          }
        };
      };
      var t = 0;
      for (var x of iterable) t += x;
      return t;
    },
    generator: function (s) {
      function* values() {
        for (var i = 0; i < s.n; i++) yield i;
      }
      var t = 0;
      for (var x of values()) t += x;
      return t;
    },
    close: function (s) {
      var closed = 0;
      function* values() {
        try {
          for (var i = 0; i < s.n; i++) yield i;
        } finally {
          closed++;
        }
      }
      var t = 0;
      for (var x of values()) {
        t += x;
        if (x === s.n >> 1) break;
      }
      return [t, closed];
    },
    throw_close: function (s) {
      var closed = 0;
      function* values() {
        try {
          yield s.n;
        } finally {
          closed++;
        }
      }
      try {
        for (var x of values()) throw x;
      } catch (x) {
        return [x, closed];
      }
    }
  },
  check: function (r, s, v) {
    if ((v === "close" || v === "throw_close") && r[1] !== 1)
      throw new Error("iterator close");
  }
});
