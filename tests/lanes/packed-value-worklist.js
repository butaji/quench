function Ticket(weight) {
  this.weight = weight;
}

function Worklist() {
  this.items = [];
}

Worklist.prototype.add = function (item) {
  this.items.push(item);
};

Worklist.prototype.take = function () {
  return this.items.pop();
};

Worklist.prototype.size = function () {
  return this.items.length;
};

const tickets = [];
for (let index = 0; index < 32; index++) tickets.push(new Ticket(index));

const work = new Worklist();
let total = 0;
for (let round = 0; round < 5000; round++) {
  for (let index = 0; index < tickets.length; index++) work.add(tickets[index]);
  while (work.size() > 0) total += work.take().weight;
}

console.log(total);
