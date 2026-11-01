(function(api) {
  'use strict';
  const out = [];
  function observe(name, run) {
    try { out.push([name, run()]); }
    catch (e) { out.push([name, e.name, e.code || null, e.message]); }
  }
  const { createHash, createHmac } = api;
  const Hash = createHash('sha256').constructor;
  const Hmac = createHmac('sha256', 'key').constructor;
  const hex = value => typeof value === 'string' ? value : Array.from(value);
  observe('one-shot metadata', () => [api.hash.name, api.hash.length]);
  for (const algorithm of ['sha256', 'sha512', 'sha1', 'md5']) {
    for (const options of [undefined, 'hex', 'base64', 'base64url', 'buffer', 'UTF8', '', {outputEncoding:null}]) {
      observe('one-shot '+algorithm+' '+String(options), () => hex(api.hash(algorithm,'é水🦄',options)));
    }
  }
  for (const options of [null, true, 2, [], 'bogus', {outputEncoding:3}, {outputLength:-1}, {outputLength:8}]) {
    observe('one-shot invalid '+JSON.stringify(options), () => hex(api.hash('sha256','abc',options)));
  }
  for (const input of [undefined, null, false, 3, {}, new ArrayBuffer(1)]) {
    observe('one-shot invalid input '+String(input), () => api.hash('sha256',input));
  }
  for (const length of [0, 8, 32]) {
    observe('one-shot shake '+length, () => api.hash('shake128','abc',{outputLength:length}));
  }
  observe('one-shot option getter order', () => {
    const events=[];
    const result=api.hash('sha256','abc',{
      get outputLength(){events.push('length');return undefined;},
      get outputEncoding(){events.push('encoding');return 'hex';},
      get functionName(){events.push('function');return undefined;},
      get customization(){events.push('customization');return undefined;}
    });
    return [events,result];
  });
  for (const algorithm of ['sha256', 'sha512', 'sha1', 'md5']) {
    observe('hash '+algorithm, () => createHash(algorithm).update('é水🦄').update(new Uint8Array([0, 255, 128])).digest('hex'));
    observe('constructor '+algorithm, () => Hash(algorithm).update('abc').digest('base64'));
    observe('copy '+algorithm, () => {
      const hash = new Hash(algorithm).update('prefix'); const copy = hash.copy();
      return [hash.update('one').digest('hex'), copy.update('two').digest('hex')];
    });
    for (const key of ['secret', new Uint8Array([0, 255, 128]), new Uint8Array([1, 2, 3]).buffer]) {
      observe('hmac '+algorithm+' '+String(key), () => createHmac(algorithm, key).update('é水🦄').digest('hex'));
    }
  }
  observe('hmac callable', () => Hmac('sha256', 'secret').update('abc').digest('hex'));
  observe('shared methods', () => [Hash.prototype.update === Hmac.prototype.update, Hash.prototype._transform === Hmac.prototype._transform]);
  observe('constructor metadata', () => [api.Hash.name, api.Hash.length, api.Hmac.name, api.Hmac.length, createHash.name, createHash.length, createHmac.name, createHmac.length]);
  observe('prototypes', () => [Object.getPrototypeOf(api.Hash).name, Object.getPrototypeOf(api.Hash.prototype).constructor.name,
    Object.getPrototypeOf(api.Hmac).name, Object.getPrototypeOf(api.Hmac.prototype).constructor.name]);
  observe('method metadata', () => [Hash, Hmac].map(C => Object.keys(C.prototype).map(k => [k, C.prototype[k].name, C.prototype[k].length])));
  observe('instance layout', () => {
    const value = new Hash('sha256', { highWaterMark: 3 });
    return Reflect.ownKeys(value).map(k => {
      const d = Object.getOwnPropertyDescriptor(value, k);
      return [String(k), d.writable, d.enumerable, d.configurable];
    });
  });
  observe('options retained', () => { const opts = {}; return new Hash('sha256', opts)._options === opts; });
  observe('lazy stream state', () => {
    const h=createHash('sha256',{highWaterMark:7});
    const before=[Object.hasOwn(h,'_readableState'),Object.hasOwn(h,'_writableState')];
    const writable=h._writableState;
    return [before,Object.hasOwn(h,'_readableState'),Object.hasOwn(h,'_writableState'),writable.decodeStrings,h.readableHighWaterMark,h.writableHighWaterMark];
  });
  observe('stream writes', () => {
    const h=createHash('sha256'); const values=[];
    h.on('data',value=>values.push(value.toString('hex')));
    h.write('ab'); h.end('c');
    return values;
  });
  observe('update receiver', () => { const h = new Hash('sha256'); return h.update('a') === h; });
  observe('subclass', () => { class Custom extends Hash {} const h = new Custom('sha256'); return [h instanceof Custom, h instanceof Hash, h.update('x').digest('hex')]; });
  for (const encoding of [undefined, 'hex', 'base64', 'base64url', 'latin1', 'buffer', 'bogus', false, 0, null]) {
    observe('digest '+String(encoding), () => hex(new Hash('sha256').update('abc').digest(encoding)));
  }
  for (const [input, encoding] of [['616263', 'hex'], ['YWJj', 'base64'], ['é', 'latin1'], ['x', 'hex'], ['abc', 'bogus']]) {
    observe('input '+input+' '+encoding, () => new Hash('sha256').update(input, encoding).digest('hex'));
  }
  for (const input of [undefined, null, 3, false, {}, new ArrayBuffer(3)]) {
    observe('invalid update '+String(input), () => new Hash('sha256').update(input));
  }
  for (const algorithm of [undefined, null, 3, false, 'not-a-digest']) {
    observe('invalid algorithm '+String(algorithm), () => new Hash(algorithm));
  }
  for (const length of [-1, 1.5, NaN, Infinity, '3', 0, 8]) {
    observe('shake output '+String(length), () => hex(new Hash('shake128', { outputLength: length }).update('abc').digest('hex')));
  }
  observe('hash finalized digest', () => { const h=new Hash('sha256'); h.digest(); return h.digest(); });
  observe('hash finalized update', () => { const h=new Hash('sha256'); h.digest(); return h.update(null); });
  observe('hash finalized copy', () => { const h=new Hash('sha256'); h.digest(); return h.copy(); });
  observe('hmac finalized digest', () => { const h=new Hmac('sha256', 'key'); return [h.digest('hex'), h.digest('hex'), hex(h.digest()), hex(h.digest('buffer'))]; });
  observe('hmac finalized update', () => { const h=new Hmac('sha256', 'key'); h.digest(); return h.update('a'); });
  observe('digest coercion failure preserves state', () => {
    const h=new Hash('sha256').update('abc'); let failure;
    try { h.digest(Symbol('encoding')); } catch(e) { failure=[e.name,e.message]; }
    return [failure,h.digest('hex')];
  });
  observe('digest coercion reentry', () => {
    const h=new Hash('sha256').update('abc'); let inner;
    const outer=h.digest({toString(){inner=h.digest('hex'); return 'hex';}});
    return [inner,outer];
  });
  observe('view offsets', () => {
    const bytes=new Uint8Array([0,1,2,3,255]);
    return new Hash('sha256').update(new DataView(bytes.buffer,1,3)).digest('hex');
  });
  observe('detached view', () => {
    const bytes=new Uint8Array(3); bytes.buffer.transfer();
    return new Hash('sha256').update(bytes).digest('hex');
  });
  observe('argument effects', () => {
    const events=[];
    const h=new Hash('sha256', { get outputLength(){events.push('length');return undefined;}, get functionName(){events.push('function');return undefined;},get customization(){events.push('custom');return undefined;} });
    h.digest({toString(){events.push('string'); return 'hex';}});
    return events;
  });
  observe('function parameter effects', () => {
    const events=[];
    try { new Hash('sha256', { get functionName(){events.push('function'); return 3;}, get customization(){events.push('custom'); return undefined;} }); }
    catch(e) { events.push(e.name,e.code); }
    return events;
  });
  observe('manual transform and flush', () => {
    const h=new Hash('sha256'); const events=[];
    h.push=bytes=>events.push(hex(bytes));
    h._transform('abc','utf8',function(...args){events.push(['transform',args.length,this===undefined]);});
    h._flush(function(...args){events.push(['flush',args.length,this===undefined]);});
    return events;
  });
  observe('hmac flush finalizes', () => {
    const h=new Hmac('sha256','key'); h.push=()=>{}; h._flush(()=>{}); return h.digest('hex');
  });
  observe('deprecated export calls', () => {
    const events=[];
    let threw=false;
    process.emitWarning=(message, kind, code, cutoff)=>{
      events.push([message,kind,code,cutoff && cutoff.name,
        cutoff === (code === 'DEP0179' ? api.Hash : code === 'DEP0181' ? api.Hmac : undefined)]);
      if(code==='DEP0179' && !threw) { threw=true; throw new Error('warning hook'); }
    };
    process.noDeprecation=true;
    new api.Hash('sha256'); new api.Hmac('sha256','key');
    events.push(['suppressed',events.length]);
    process.noDeprecation=false;
    try { new api.Hash('sha256'); } catch(e) { events.push(['hook',e.message]); }
    for(let i=0;i<2;i++) {
      const h=new api.Hash('sha256');
      events.push([h instanceof api.Hash,h instanceof Hash,h.update('abc').digest('hex')]);
      const m=api.Hmac('sha256','key');
      events.push([m instanceof api.Hmac,m instanceof Hmac,m.update('abc').digest('hex')]);
    }
    for(let i=0;i<2;i++) events.push(createHash('shake128').update('abc').digest('hex'));
    events.push(api.hash('shake256','abc'));
    return events;
  });
  return out.map(item => JSON.stringify(item)).join('\n');
})
