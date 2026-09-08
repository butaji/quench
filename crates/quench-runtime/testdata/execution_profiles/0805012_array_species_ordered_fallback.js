function copy(source, log) {
  source.constructor = {
    get [Symbol.species]() {
      return function Species() {
        return new Proxy({}, {
          defineProperty: function defineProperty(target, key, descriptor) {
            log.push("define" + key);
            return Reflect.defineProperty(target, key, descriptor);
          },
          set: function set(target, key, value) {
            log.push("set" + key);
            return Reflect.set(target, key, value);
          }
        });
      };
    }
  };
  return source.slice(0);
}

var log = [];
var source = [1, 2];
Object.defineProperty(source, "0", {
  get: function getZero() { log.push("get0"); return 1; }
});
Object.defineProperty(source, "1", {
  get: function getOne() { log.push("get1"); return 2; }
});
copy(source, log);
var result = log.join(",");
if (result !== "get0,define0,get1,define1,setlength") {
  throw new Error("array species order mismatch: " + result);
}
return result;
