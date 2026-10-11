function mapped(a) {
  arguments[0] = 9;
  return [a, arguments[0], arguments.callee === mapped];
}
function sloppyDefault(a = 1) {
  arguments[0] = 9;
  let throws = false;
  try { void arguments.callee; } catch (e) { throws = e instanceof TypeError; }
  const d = Object.getOwnPropertyDescriptor(arguments, 'callee');
  return [a, arguments[0], throws, d.get === d.set, d.enumerable, d.configurable];
}
function strict(a) {
  'use strict';
  const d = Object.getOwnPropertyDescriptor(arguments, 'callee');
  let throws = false;
  try { void arguments.callee; } catch (e) { throws = e instanceof TypeError; }
  return [throws, d.get === d.set, d.enumerable, d.configurable];
}
function inspect(a, b) {
  return [Reflect.ownKeys(arguments).map(String), [...arguments],
    Object.getOwnPropertyDescriptor(arguments, 'length').enumerable,
    Object.getOwnPropertyDescriptor(arguments, Symbol.iterator).enumerable];
}
function mutate() {
  Object.defineProperty(arguments, 'callee', { value: 12, configurable: true });
  delete arguments[Symbol.iterator];
  return [arguments.callee, Reflect.ownKeys(arguments).map(String)];
}
function fresh() {
  return [arguments.callee === fresh, [...arguments], Reflect.ownKeys(arguments).map(String)];
}
console.log(JSON.stringify([mapped(1), sloppyDefault(2), strict(1), inspect(3, 4), mutate(), fresh(5, 6)]));
