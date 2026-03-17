import type { Request, Response, NextFunction } from "express";
import { createChildLogger } from "../util/logger.js";

const log = createChildLogger("middleware");

/**
 * Bearer token authentication middleware.
 * Validates Authorization header against ORCHESTRATOR_API_KEY.
 */
export function authMiddleware(apiKey: string) {
  return (req: Request, res: Response, next: NextFunction): void => {
    // Skip auth for health check
    if (req.path === "/api/v1/health") {
      next();
      return;
    }

    const authHeader = req.headers.authorization;
    if (!authHeader?.startsWith("Bearer ")) {
      res.status(401).json({ error: "Missing or invalid Authorization header" });
      return;
    }

    const token = authHeader.slice(7);
    if (token !== apiKey) {
      log.warn({ ip: req.ip }, "Invalid API key");
      res.status(403).json({ error: "Invalid API key" });
      return;
    }

    next();
  };
}

/**
 * Request logging middleware.
 */
export function requestLogger() {
  return (req: Request, _res: Response, next: NextFunction): void => {
    log.info(
      { method: req.method, path: req.path, ip: req.ip },
      "Request received",
    );
    next();
  };
}

/**
 * Error handling middleware.
 */
export function errorHandler() {
  return (
    err: Error,
    _req: Request,
    res: Response,
    _next: NextFunction,
  ): void => {
    log.error({ error: err.message, stack: err.stack }, "Unhandled error");
    res.status(500).json({ error: "Internal server error" });
  };
}
