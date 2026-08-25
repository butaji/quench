const constraints = {
  values: [
    { isSatisfied() { return true; } },
    { isSatisfied() { return true; } },
    { isSatisfied() { return true; } },
    { isSatisfied() { return false; } },
  ],
  size() { return this.values.length; },
  at(index) { return this.values[index]; },
};
const variable = { determinedBy: constraints.values[3], constraints };
const collection = { add(value) {} };

const planner = {
  collect(v, coll) {
    var determining = v.determinedBy;
    var cc = v.constraints;
    for (var i = 0; i < cc.size(); i++) {
      var c = cc.at(i);
      if (c != determining && c.isSatisfied()) coll.add(c);
    }
  },
};

for (let i = 0; i < 30000; i++) planner.collect(variable, collection);
