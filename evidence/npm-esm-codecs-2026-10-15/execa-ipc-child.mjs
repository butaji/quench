import {getOneMessage,sendMessage,getEachMessage,getCancelSignal} from '/workspace/quench/tests/npm-top300/node_modules/execa/index.js';
const first=await getOneMessage();
await sendMessage({first});
for await (const message of getEachMessage()) {
 await sendMessage({second:message,cancelAborted:getCancelSignal()?.aborted??null});
 break;
}
