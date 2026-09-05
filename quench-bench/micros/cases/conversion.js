registerMicro({
  id: "conversion",
  question: "What changes when arithmetic must perform observable conversion?",
  requires: ["numeric"],
  axes: ["size", "conversion"],
  observations: ["time per conversion", "observable conversion calls"],
  explanations: [
    "Primitive conversion cost",
    "Repeated object conversion",
    "Reentry overhead"
  ],
  setup: function (n, seed) {
    return { n: n, seed: seed };
  },
  variants: {
    number: function (s) {
      var total = 0;
      for (var i = 0; i < s.n; i++) total += Number((i + s.seed) % 31);
      return total;
    },
    string: function (s) {
      var total = 0;
      for (var i = 0; i < s.n; i++) total += Number(String((i + s.seed) % 31));
      return total;
    },
    observable: function (s) {
      var calls = 0,
        value = 0;
      var x = {
        valueOf: function () {
          calls++;
          return value;
        }
      };
      var total = 0;
      for (var i = 0; i < s.n; i++) {
        value = (i + s.seed) % 31;
        total += +x;
      }
      return [total, calls];
    },
    changing: function (s) {
      var total = 0;
      for (var i = 0; i < s.n; i++) {
        var x = (i + s.seed) % 31;
        total += +(i % 17 ? x : String(x));
      }
      return total;
    }
  },
  check: function (result, s, variant) {
    if (variant === "observable" && result[1] !== s.n)
      throw new Error("conversion effects lost");
  }
});
