/**
 * Middleware
 *
 * Adds timing header to all responses.
 */

import { HandlerContext } from "$fresh/server.ts";

export const handler = async (req: Request, ctx: HandlerContext) => {
  const start = Date.now();
  const resp = await ctx.next();
  resp.headers.set("X-Response-Time", `${Date.now() - start}ms`);
  return resp;
};
