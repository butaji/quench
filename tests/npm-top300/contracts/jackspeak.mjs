import assert from 'node:assert/strict';
import { inspect } from 'node:util';

// Exercise the public builders, parser, validators, environment projections,
// getters, serialization and renderers. Returned/dynamic APIs remain separate.
export function exercise(api) {
  const checks = {};
  let assertions = 0;
  const equal = (id, actual, expected) => {
    assert.deepEqual(actual, expected); assertions++;
    checks[id] = (checks[id] ?? 0) + 1;
  };
  const throws = (id, fn, expected) => {
    assert.throws(fn, expected); assertions++;
    checks[id] = (checks[id] ?? 0) + 1;
  };
  for (const [value, expected] of [['string',true],['number',true],['boolean',true],['object',false],[null,false],[42,false]])
    equal('isConfigType', api.isConfigType(value), expected);
  for (const type of ['string','number','boolean']) for (const multiple of [false,true]) {
    const config = {type,multiple};
    equal('isConfigOptionOfType',api.isConfigOptionOfType(config,type,multiple),true);
    equal('isConfigOptionOfType',api.isConfigOptionOfType(config,type,!multiple),false);
    equal('isConfigOption',api.isConfigOption(config,type,multiple),true);
    equal('isConfigOption',api.isConfigOption({...config,short:3},type,multiple),false);
  }
  equal('isConfigOptionOfType',api.isConfigOptionOfType(null,'string',false),false);
  const env = { APP_NAME:'environment', APP_COUNTS:'2\n3' };
  const options = {env,envPrefix:'APP',usage:'parse-contract [options]'};
  const j = api.jack(options);
  equal('jack',j instanceof api.Jack,true);
  equal('Jack',new api.Jack().parse([]),{values:{},positionals:[]});
  equal('Jack.prototype.jackOptions:get',j.jackOptions,options);
  equal('Jack.prototype.num',j.num({count:{short:'n',default:1}}),j);
  equal('Jack.prototype.numList',j.numList({counts:{}}),j);
  equal('Jack.prototype.opt',j.opt({name:{short:'s'}}),j);
  equal('Jack.prototype.optList',j.optList({names:{}}),j);
  equal('Jack.prototype.flag',j.flag({verbose:{short:'v',default:true}}),j);
  equal('Jack.prototype.flagList',j.flagList({flags:{}}),j);
  equal('Jack.prototype.addFields',j.addFields({custom:{type:'string',default:'custom'}}),j);
  equal('Jack.prototype.definitions:get',Object.getPrototypeOf(j.definitions),null);
  equal('Jack.prototype.definitions:get',j.definitions.count.type,'number');
  equal('Jack.prototype.shorts:get', {...j.shorts},{n:'count',count:'count',s:'name',name:'name',v:'verbose',verbose:'verbose'});
  equal('Jack.prototype.heading',j.heading('Arguments',2),j);
  equal('Jack.prototype.description',j.description('Argument contract'),j);
  equal('Jack.prototype.usageFields:get',j.usageFields.length,9);
  const args=['-n','4','--counts','5','--counts=6','-s','cli','--names=a','--names','b','--no-verbose','--flags','--no-flags','pos'];
  const parsed=j.parseRaw(args);
  equal('Jack.prototype.parseRaw',parsed,{values:{count:4,counts:[5,6],name:'cli',names:['a','b'],verbose:false,flags:[true,false]},positionals:['pos']});
  equal('Jack.prototype.validate',j.validate(parsed.values),undefined);
  throws('Jack.prototype.validate',()=>j.validate({missing:true}),/Unknown config option: missing/);
  throws('Jack.prototype.validate',()=>j.validate({count:'bad'}),/Invalid value/);
  throws('Jack.prototype.setConfigValues',()=>j.setConfigValues({count:'bad'},'settings.json'),{cause:{code:'JACKSPEAK',name:'count',found:'bad',wanted:'number',path:'settings.json'}});
  equal('Jack.prototype.setConfigValues',j.setConfigValues({count:8}),j);
  equal('Jack.prototype.applyDefaults',j.applyDefaults(parsed),undefined);
  equal('Jack.prototype.applyDefaults',parsed.values.custom,'custom');
  equal('Jack.prototype.loadEnvDefaults',j.loadEnvDefaults(),undefined);
  equal('Jack.prototype.loadEnvDefaults',j.definitions.name.default,'environment');
  equal('Jack.prototype.loadEnvDefaults',j.definitions.counts.default,[2,3]);
  equal('Jack.prototype.writeEnv',j.writeEnv(parsed),undefined);
  equal('Jack.prototype.writeEnv',env.APP_COUNT,'4');
  equal('Jack.prototype.writeEnv',env.APP_COUNTS,'5\n6');
  equal('Jack.prototype.parse',j.parse(['--count=9']).values.count,9);
  equal('Jack.prototype.toJSON',j.toJSON().count,{type:'number',short:'n',default:4});
  throws('Jack.prototype.num',()=>j.num({'bad!':{}}),TypeError);
  throws('Jack.prototype.parseRaw',()=>j.parseRaw(['--count=nan']),/Invalid value/);
  const banner=api.jack({usage:'parse-contract [options]'}).heading('Contract').description('Read options.').flag({verbose:{short:'v',description:'Print details.'}});
  equal('Jack.prototype.usage',banner.usage(), 'Contract\nUsage:\n  parse-contract [options]\n\nRead options.\n\n  -v --verbose  Print details.');
  equal('Jack.prototype.usage',banner.usage(),banner.usage());
  equal('Jack.prototype.usageMarkdown',banner.usageMarkdown(), '# Contract\n\nUsage:\n\n```\nparse-contract [options]\n```\n\nRead options.\n\n## `-v --verbose`\n\nPrint details.\n');
  equal('Jack.prototype.usageMarkdown',banner.usageMarkdown(),banner.usageMarkdown());
  equal('Jack.prototype.toJSON',banner.toJSON(),{verbose:{type:'boolean',short:'v',description:'Print details.'}});
  equal('Jack.prototype.toJSON',banner[inspect.custom](0,{}),'Jack '+inspect(banner.toJSON(),{}));
  return {package:'jackspeak',assertions,checks,scope:'Public builder/parser/config/environment/getter/renderer corpus; matching cases do not prove all inputs or returned APIs'};
}
