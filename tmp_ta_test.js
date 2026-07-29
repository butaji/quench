// Check constructor chain
print("Object.prototype.constructor=" + Object.prototype.constructor.name);
var o = {};
print("plain obj constructor=" + o.constructor.name);
var t = new TypeError("test");
print("TypeError instance constructor=" + t.constructor.name);

// Now check what happens when create_js_error_with_type creates an error
try { throw new TypeError("direct"); } catch(e) {
  print("direct throw constructor=" + e.constructor.name);
}
