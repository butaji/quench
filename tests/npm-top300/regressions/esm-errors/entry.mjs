import assert from 'node:assert/strict';
globalThis.moduleEffects = 0;
globalThis.parentEffects = 0;
let parentError, repeatedError, directError;
try { await import('./parent.mjs'); } catch (error) { parentError = error; }
try { await import('./parent.mjs'); } catch (error) { repeatedError = error; }
assert(parentError instanceof SyntaxError);
assert.notEqual(repeatedError, parentError);
assert.equal(parentError.message, "The requested module './effect.mjs' does not provide an export named 'missing'");
assert.equal(repeatedError.message, parentError.message);
assert.equal(moduleEffects, 0);
assert.equal(parentEffects, 0);
try { await import('./bad.mjs'); } catch (error) { directError = error; }
assert(directError instanceof SyntaxError);
assert.notEqual(directError, parentError);
try { await import('./bad.mjs'); assert.fail('invalid import accepted'); }
catch (error) {
  assert(error instanceof SyntaxError);
  assert.notEqual(error, directError);
  assert.equal(error.message, parentError.message);
}
assert.equal(moduleEffects, 0);
const concurrent = await Promise.allSettled([import('./bad.mjs'), import('./bad.mjs')]);
for (const result of concurrent) {
  assert.equal(result.status, 'rejected');
  assert(result.reason instanceof SyntaxError);
  assert.equal(result.reason.message, parentError.message);
}
assert.notEqual(concurrent[0].reason, concurrent[1].reason);
const parse = await Promise.allSettled([import('./parse.mjs'), import('./parse.mjs')]);
assert(parse.every(result => result.status === 'rejected' && result.reason instanceof SyntaxError));
assert.notEqual(parse[0].reason, parse[1].reason);
const evaluation = await Promise.allSettled([import('./evaluation.mjs'), import('./evaluation.mjs')]);
assert(evaluation.every(result => result.status === 'rejected' && result.reason instanceof Error));
assert.equal(evaluation[0].reason, evaluation[1].reason);
assert.equal(evaluation[0].reason.message, 'module evaluation failed');
assert.equal(evaluationEffects, 1);
const effect = await import('./effect.mjs');
assert.equal(effect.value, 43);
assert.equal(moduleEffects, 1);
assert.equal(parentEffects, 0);
assert.equal(await import('./effect.mjs'), effect);
assert.equal(moduleEffects, 1);
console.log('esm-link-error-retry');
