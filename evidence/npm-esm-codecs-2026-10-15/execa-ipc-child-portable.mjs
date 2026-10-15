import {pathToFileURL} from 'node:url';
import {resolve} from 'node:path';
const {getOneMessage,sendMessage,getEachMessage,getCancelSignal}=await import(pathToFileURL(resolve(process.argv[2],'index.js')).href);
const first=await getOneMessage();
await sendMessage({first});
for await (const message of getEachMessage()) {
 await sendMessage({second:message,cancelAborted:getCancelSignal()?.aborted??null});
 break;
}
