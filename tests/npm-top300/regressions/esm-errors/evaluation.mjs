globalThis.evaluationEffects = (globalThis.evaluationEffects || 0) + 1;
throw new Error('module evaluation failed');
