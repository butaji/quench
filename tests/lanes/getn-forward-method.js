const collection = {
  elements: [],
  size() {
    return this.elements.length;
  },
};
const plan = {
  collection,
  size() {
    return this.collection.size();
  },
};

let total = 0;
for (let i = 0; i < 250000; i++) total += plan.size();
if (total !== 0) throw new Error("forward method result");
