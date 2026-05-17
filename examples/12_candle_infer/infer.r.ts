import { Device, Tensor, SafeTensors } from "candle_core";
import { Tokenizer } from "tokenizers";

export type LlamaModel = {
  tensors: SafeTensors;
  device: Device;
};

export async function loadLlama(weightsPath: string, device: Device): LlamaModel {
  return {
    tensors: SafeTensors.load(weightsPath),
    device,
  };
}

export function loadTokenizer(path: string): Tokenizer {
  return Tokenizer.fromFile(path);
}

export async function complete(
  model: LlamaModel,
  tokenizer: Tokenizer,
  prompt: string,
  maxLen: number
): string {
  let tokens = tokenizer.encode(prompt).ids();

  for (let pos = 0; pos < maxLen; pos++) {
    const input = Tensor.new(tokens, [1, tokens.length], "u32", model.device);
    const logits = forwardLlama(model, input);
    const nextToken = logits.get(0).argmax(1).toScalarU32();

    if (nextToken === tokenizer.tokenToId("</s>")) {
      break;
    }
    tokens.push(nextToken);
  }

  return tokenizer.decode(tokens, true);
}

function forwardLlama(model: LlamaModel, input: Tensor): Tensor {
  return nativeLlamaForward(model.tensors, input);
}
