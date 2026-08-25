function Packet() {
  this.link = null;
}

Packet.prototype.addTo = function (queue) {
  this.link = null;
  if (queue == null) return this;
  var peek;
  var next = queue;
  while ((peek = next.link) != null) next = peek;
  next.link = this;
  return queue;
};

var checksum = 0;
for (var i = 0; i < 20000; i++) {
  var head = new Packet();
  var middle = new Packet();
  var tail = new Packet();
  head.link = middle;
  tail.addTo(head);
  checksum += head.link === middle && middle.link === tail && tail.link === null ? 1 : 0;
}

var nullPacket = new Packet();
checksum += nullPacket.addTo(null) === nullPacket && nullPacket.link === null ? 1 : 0;

var aliasPacket = new Packet();
checksum += aliasPacket.addTo(aliasPacket) === aliasPacket && aliasPacket.link === aliasPacket ? 1 : 0;

var gets = 0;
var sets = 0;
var stored;
var accessorQueue = {};
Object.defineProperty(accessorQueue, "link", {
  get: function () {
    gets++;
    return null;
  },
  set: function (value) {
    sets++;
    stored = value;
  },
});
var accessorPacket = new Packet();
checksum +=
  accessorPacket.addTo(accessorQueue) === accessorQueue &&
  gets === 1 &&
  sets === 1 &&
  stored === accessorPacket
    ? 1
    : 0;
console.log(checksum);
