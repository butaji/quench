import { caught } from './library.mjs';
try { console.log(caught()); } catch (error) { console.log('escaped', error.message); }
