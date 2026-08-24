const collection = {
  elements: [],
  size() {
    return this.elements.length;
  },
};

let total = 0;
for (let i = 0; i < 250000; i++) total += collection.size();
if (total !== 0) throw new Error("array length result");
