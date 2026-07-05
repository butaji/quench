// Object literal with properties
// ECMA-262 sec-object-initializer

let obj = {
    x: 1,
    y: 2,
    getX: function() { return this.x; }
};
obj.getX();
