var DEVICE_A = 2;
var DEVICE_B = 3;
var calls = 0;

var scheduler = {
  holdCurrent: function () {
    calls += 100;
    return 0;
  },
  release: function (id) {
    calls += id;
    return id;
  },
};

function IdleTask(count, v1) {
  this.count = count;
  this.v1 = v1;
  this.scheduler = scheduler;
}

IdleTask.prototype.run = function (packet) {
  this.count--;
  if (this.count == 0) return this.scheduler.holdCurrent();
  if ((this.v1 & 1) == 0) {
    this.v1 = this.v1 >> 1;
    return this.scheduler.release(DEVICE_A);
  } else {
    this.v1 = (this.v1 >> 1) ^ 0xD008;
    return this.scheduler.release(DEVICE_B);
  }
};

var task = new IdleTask(20001, 1);
var checksum = 0;
for (var i = 0; i < 20000; i++) checksum += task.run(null);
for (var j = 0; j < 1000; j++) checksum += new IdleTask(1, j).run(null);
console.log(checksum + calls + task.count + task.v1);
