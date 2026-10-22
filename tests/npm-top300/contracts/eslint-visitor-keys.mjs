import assert from 'node:assert/strict';
export function exercise(api) {
  let assertions=0;const checks={};
  const equal=(id,actual,expected)=>{assert.deepEqual(actual,expected);assertions++;checks[id]=(checks[id]??0)+1;};
  equal('getKeys',api.getKeys({type:'Identifier',name:'x',parent:{},leadingComments:[],trailingComments:[],_private:1}),['type','name']);
  const object=Object.assign(Object.create({inherited:1}),{own:1});equal('getKeys',api.getKeys(object),['own']);equal('getKeys',api.getKeys({}),[]);
  const additions={Identifier:['custom','name','custom'],NewNode:['left','right']};
  const result=api.unionWith(additions);
  equal('unionWith',result.Identifier,['custom','name']);equal('unionWith',result.NewNode,['left','right']);
  equal('unionWith',result.Program,api.KEYS.Program);equal('unionWith',Object.isFrozen(result),true);equal('unionWith',Object.isFrozen(result.NewNode),true);
  equal('unionWith',additions.Identifier,['custom','name','custom']);equal('unionWith',api.KEYS.Identifier,[]);
  return {package:'eslint-visitor-keys',assertions,checks,scope:'Finite key filtering, prototype exclusion, union ordering, deduplication and freezing corpus; arbitrary AST kinds remain obligations'};
}
