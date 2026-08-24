const url = require('url');
if (typeof url.Url !== 'function') throw new Error('Url=' + typeof url.Url);
const empty = new url.Url();
if (typeof empty !== 'object') throw new Error('empty=' + typeof empty);
