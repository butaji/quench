/**
 * API Route - Hono style
 *
 * Returns text from a Hono-style handler.
 * Compiled to native Rust by runts.
 */

import { Context } from "hono";

export const handler = {
  async GET(_req: Request, c: Context): Promise<Response> {
    return c.text("Hello from Hono + runts API!");
  },
};
