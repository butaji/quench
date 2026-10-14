import assert from 'node:assert/strict';

// Contracts for the pinned package's public Linter and SourceCode methods.
// Returned scopes/ASTs and inherited token-store APIs have separate obligations.
export function exercise(api) {
  const checks = {};
  let assertions = 0;
  const equal = (id, actual, expected) => {
    assert.deepEqual(actual, expected);
    checks[id] = (checks[id] ?? 0) + 1;
    assertions++;
  };
  const throws = (id, action, error) => {
    assert.throws(action, error);
    checks[id] = (checks[id] ?? 0) + 1;
    assertions++;
  };
  const linter = new api.Linter();
  equal('Linter', linter instanceof api.Linter, true);
  throws('Linter', () => new api.Linter({ flags: ['definitely_unknown'] }), /Unknown flag/);
  equal('Linter.version:get', api.Linter.version, '10.12.0');
  equal('Linter.prototype.hasFlag', linter.hasFlag('definitely_unknown'), false);
  const text = 'var value=1;\nvalue;\nmissing;';
  const config = [{ rules: { 'no-undef': 'error' } }];
  const messages = linter.verify(text, config);
  equal('Linter.prototype.verify', messages.map(m => [m.ruleId, m.severity, m.line, m.column, m.messageId]),
    [['no-undef', 2, 3, 1, 'undef']]);
  const source = linter.getSourceCode();
  equal('Linter.prototype.getSourceCode', source instanceof api.SourceCode, true);
  equal('Linter.prototype.getSourceCode', source.text, text);
  equal('Linter.prototype.getTimes', linter.getTimes(), { passes: [] });
  equal('Linter.prototype.getFixPassCount', linter.getFixPassCount(), 0);
  equal('Linter.prototype.getSuppressedMessages', linter.getSuppressedMessages(), []);
  equal('SourceCode', new api.SourceCode(text, source.ast).text, text);
  throws('SourceCode', () => new api.SourceCode(), TypeError);
  equal('SourceCode.prototype.getText', source.getText(), text);
  equal('SourceCode.prototype.getText', source.getText(source.ast.body[0]), 'var value=1;');
  equal('SourceCode.prototype.getLines', source.getLines(), ['var value=1;', 'value;', 'missing;']);
  equal('SourceCode.prototype.getAllComments', source.getAllComments(), []);
  equal('SourceCode.prototype.getNodeByRangeIndex', source.getNodeByRangeIndex(4).type, 'Identifier');
  equal('SourceCode.prototype.isSpaceBetween', source.isSpaceBetween(source.getFirstToken(source.ast), source.getTokens(source.ast)[1]), true);
  equal('SourceCode.prototype.getLocFromIndex', source.getLocFromIndex(4), { line: 1, column: 4 });
  equal('SourceCode.prototype.getIndexFromLoc', source.getIndexFromLoc({ line: 1, column: 4 }), 4);
  equal('SourceCode.prototype.getScope', source.getScope(source.ast).type, 'global');
  equal('SourceCode.prototype.getDeclaredVariables', source.getDeclaredVariables(source.ast.body[0]).map(v => v.name), ['value']);
  equal('SourceCode.prototype.getAncestors', source.getAncestors(source.ast.body[1].expression).map(n => n.type), ['Program', 'ExpressionStatement']);
  equal('SourceCode.prototype.isGlobalReference', source.isGlobalReference(source.ast.body[1].expression), false);
  equal('SourceCode.prototype.getLoc', [source.getLoc(source.ast).start.line, source.getLoc(source.ast).end.line], [1, 3]);
  equal('SourceCode.prototype.getRange', source.getRange(source.ast), [0, text.length]);
  equal('SourceCode.prototype.markVariableAsUsed', source.markVariableAsUsed('value', source.ast), true);
  equal('SourceCode.prototype.markVariableAsUsed', source.markVariableAsUsed('notDeclared', source.ast), false);
  equal('SourceCode.prototype.getInlineConfigNodes', source.getInlineConfigNodes(), []);
  equal('SourceCode.prototype.getDisableDirectives', source.getDisableDirectives(), { problems: [], directives: [] });
  equal('SourceCode.prototype.applyInlineConfig', source.applyInlineConfig(), { configs: [], problems: [] });
  equal('SourceCode.prototype.applyLanguageOptions', source.applyLanguageOptions({ ecmaVersion: 2026, sourceType: 'script', globals: { injected: 'readonly' } }), undefined);
  equal('SourceCode.prototype.finalize', source.finalize(), undefined);
  equal('SourceCode.prototype.finalize', source.getScope(source.ast).variables.some(v => v.name === 'injected'), true);
  const traversal = source.traverse();
  equal('SourceCode.prototype.traverse', traversal.filter(step => step.kind === 1).map(step => [step.phase, step.target.type]),
    [[1,'Program'],[1,'VariableDeclaration'],[1,'VariableDeclarator'],[1,'Identifier'],[2,'Identifier'],[1,'Literal'],[2,'Literal'],[2,'VariableDeclarator'],[2,'VariableDeclaration'],[1,'ExpressionStatement'],[1,'Identifier'],[2,'Identifier'],[2,'ExpressionStatement'],[1,'ExpressionStatement'],[1,'Identifier'],[2,'Identifier'],[2,'ExpressionStatement'],[2,'Program']]);
  const fixed = linter.verifyAndFix('var value=1', [{ rules: { semi: ['error', 'always'] } }]);
  equal('Linter.prototype.verifyAndFix', fixed, { fixed: true, messages: [], output: 'var value=1;' });
  equal('Linter.prototype.verify', linter.verify('/* eslint-disable no-undef */\nmissing;', config), []);
  equal('Linter.prototype.getSuppressedMessages', linter.getSuppressedMessages().map(m => [m.ruleId, m.suppressions.map(s => s.kind)]), [['no-undef', ['directive']]]);
  return { package: 'eslint', assertions, checks,
    scope: 'Pinned public Linter and SourceCode contract corpus; internal/inherited/returned APIs and other exports remain independent obligations' };
}
