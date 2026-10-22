import * as a from './a.mjs';
import { entryHoisted } from './entry.mjs';
import config from './config.json' with { type: 'json' };
globalThis.cycleTrace = ['b'];
export let early = false;
try { entryHoisted(); } catch (error) { early = error instanceof ReferenceError; }
export function read() { return a.value; }
export { a as namespace };
export { config };
