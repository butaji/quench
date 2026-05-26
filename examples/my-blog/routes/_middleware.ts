// routes/_middleware.ts - Global middleware
import { MiddlewareHandler } from "$fresh/server.ts";

/**
 * Global middleware - runs on every request
 * Demonstrates:
 * - Request/response modification
 * - Timing
 * - State management
 * - Error handling
 */

// Add timing header to all responses
export const handler: MiddlewareHandler = async (req: Request, ctx) => {
  const start = performance.now();
  
  // Log the request
  console.log(`[${new Date().toISOString()}] ${req.method} ${req.url}`);
  
  // Call the next handler
  const response = await ctx.next();
  
  // Add timing header
  const duration = performance.now() - start;
  response.headers.set("X-Response-Time", `${duration.toFixed(2)}ms`);
  response.headers.set("X-Runtime", "Rust + runts");
  
  // Add cache headers for static assets
  if (req.url.includes("/static/")) {
    response.headers.set("Cache-Control", "public, max-age=31536000, immutable");
  }
  
  return response;
};
