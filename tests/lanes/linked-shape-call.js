function Item(value, next) {
  this.value = value;
  this.next = next;
  this.paused = false;
}

Item.prototype.isPaused = function () {
  return this.paused;
};

Item.prototype.advance = function () {
  return this.next;
};

function WorkList(head) {
  this.head = head;
  this.cursor = null;
  this.selected = 0;
}

WorkList.prototype.drain = function () {
  this.cursor = this.head;
  while (this.cursor != null) {
    if (this.cursor.isPaused()) {
      this.cursor = this.cursor.next;
    } else {
      this.selected = this.cursor.value;
      this.cursor = this.cursor.advance();
    }
  }
};

var list = new WorkList(new Item(1, new Item(2, new Item(3, null))));
var checksum = 0;
for (var iteration = 0; iteration < 50000; iteration++) {
  list.drain();
  checksum += list.selected;
}
console.log(checksum);
