'use strict';
const fn = function(a, b, c) { return arguments; };
const value = fn(1, 2, 3);
if (value == null || value.length !== 3 || value[1] !== 2) throw new Error('arguments');
