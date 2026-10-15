import assert from 'node:assert/strict';
globalThis.moduleEffects = 0;
globalThis.parentEffects = 0;
let parentError, repeatedError, directError;
try { await import('./parent.mjs'); } catch (error) { parentError = error; }
try { await import('./parent.mjs'); } catch (error) { repeatedError = error; }
assert(parentError instanceof SyntaxError);
assert.equal(repeatedError, parentError);
assert.equal(moduleEffects, 0);
assert.equal(parentEffects, 0);
try { await import('./bad.mjs'); } catch (error) { directError = error; }
assert(directError instanceof SyntaxError);
assert.notEqual(directError, parentError);
try { await import('./bad.mjs'); assert.fail('invalid import accepted'); }
catch (error) { assert.equal(error, directError); }
assert.equal(moduleEffects, 0);
const effect = await import('./effect.mjs');
assert.equal(effect.value, 43);
assert.equal(moduleEffects, 1);
assert.equal(parentEffects, 0);
assert.equal(await import('./effect.mjs'), effect);
assert.equal(moduleEffects, 1);
console.log('esm-link-error-retry');
