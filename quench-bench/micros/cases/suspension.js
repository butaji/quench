registerMicro({
  id: "suspension",
  question: "What changes with suspension and queued continuation count?",
  requires: ["calls", "iteration"],
  axes: ["size", "continuation form"],
  async: true,
  observations: [
    "time to completed useful work",
    "bounded pending continuation count"
  ],
  explanations: ["Promise creation", "Suspension", "Queue processing"],
  setup: function (n, seed) {
    return { n: n, seed: seed };
  },
  equivalent: [["synchronous", "await", "chain", "queued"]],
  variants: {
    synchronous: function (s) {
      var t = s.seed;
      for (var i = 0; i < s.n; i++) t++;
      return t;
    },
    await: async function (s) {
      var t = s.seed;
      for (var i = 0; i < s.n; i++) t = await Promise.resolve(t + 1);
      return t;
    },
    chain: function (s) {
      var p = Promise.resolve(s.seed);
      for (var i = 0; i < s.n; i++)
        p = p.then(function (x) {
          return x + 1;
        });
      return p;
    },
    queued: async function (s) {
      var a = [];
      for (var i = 0; i < s.n; i++) a.push(Promise.resolve(1));
      var values = await Promise.all(a),
        t = s.seed;
      for (var j = 0; j < values.length; j++) t += values[j];
      return t;
    }
  }
});
