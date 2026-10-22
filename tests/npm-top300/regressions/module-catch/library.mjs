function bad() { throw Error('marker'); }
export function caught() { try { return bad(); } catch { return null; } }
