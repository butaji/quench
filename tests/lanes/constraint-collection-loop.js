function Item(satisfied) { this.satisfied = satisfied; }
Item.prototype.isSatisfied = function () { return this.satisfied; };
function Items(values) { this.values = values; }
Items.prototype.size = function () { return this.values.length; };
Items.prototype.at = function (index) { return this.values[index]; };
const constraints = new Items([
  new Item(true),
  new Item(true),
  new Item(true),
  new Item(false),
]);
const variable = { determinedBy: constraints.values[3], constraints };
function Output() { this.values = []; }
Output.prototype.add = function (value) { this.values.push(value); };
const collection = new Output();

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

for (let i = 0; i < 6000; i++) planner.collect(variable, collection);
if (collection.values.length !== 18000) throw new Error("wrong filtered output");
