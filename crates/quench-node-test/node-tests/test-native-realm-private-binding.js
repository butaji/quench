// Candidate-only: Node does not expose internal/bootstrap/realm to require().
const assert=require('assert');
const expected=require('internal/test/binding').internalBinding('util');
const realm=require('internal/bootstrap/realm');
assert.strictEqual(realm.internalBinding('util'),expected);
assert.strictEqual(realm.internalBinding('util').privateSymbols,expected.privateSymbols);
for(const name of Object.keys(expected.privateSymbols))assert.strictEqual(realm.internalBinding('util').privateSymbols[name],expected.privateSymbols[name]);
console.log('native-realm-binding-identity-ok');
