import { read } from './b.mjs';
export let value = 1;
export function hoisted() { return read(); }
export function bump() { value++; }
globalThis.cycleTrace.push('a');
