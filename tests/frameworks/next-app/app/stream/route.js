export const dynamic = 'force-dynamic';
export async function GET() { const e = new TextEncoder(); return new Response(new ReadableStream({ start(k) { for (const x of 'abc') k.enqueue(e.encode(x)); k.close(); } })); }
