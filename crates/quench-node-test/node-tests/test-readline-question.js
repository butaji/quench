'use strict';
const readline = require('node:readline');
const rl = readline.createInterface({ input: ['answer'], output: process.stdout });
rl.question('prompt: ', (answer) => {
  if (answer !== 'answer') throw new Error('question answer: ' + answer);
  rl.close();
  console.log('readline-question: ok');
});
