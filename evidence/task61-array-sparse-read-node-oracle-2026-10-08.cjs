var a = new Array(8);
for (var i = 0; i < 8; i++) a[i] = i * 3;

Object.defineProperty(Array.prototype, '2', {
  get: function() { return 202; },
  configurable: true,
});
Object.defineProperty(Array.prototype, '3', {
  get: function() { return 303; },
  configurable: true,
});

var inheritedHole = new Array(8)[2];
var ownShadowsInherited = a[3];
delete Array.prototype[2];
delete Array.prototype[3];

Object.defineProperty(a, '4', {
  get: function() { return 404; },
  configurable: true,
});
Object.defineProperty(a, '5', {
  value: 505,
  writable: false,
  configurable: true,
});
delete a[6];

var proxy = new Proxy(a, {
  get: function(target, key, receiver) {
    if (key === '0') return 707;
    return Reflect.get(target, key, receiver);
  },
});

var result = [
  inheritedHole,
  ownShadowsInherited,
  a[4],
  a[5],
  String(a[6]),
  proxy[0],
];
console.log(result.join(','));
