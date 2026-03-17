import { execSync } from "node:child_process";
import { createChildLogger } from "../util/logger.js";

const log = createChildLogger("harbangan-tools");

export interface QualityGateResult {
  gate: string;
  passed: boolean;
  output: string;
  duration_ms: number;
}

/**
 * Harbangan-specific tools for running quality gates and project operations.
 */
export class HarbanganTools {
  /**
   * Run backend quality gates in a given directory.
   * Returns results for each gate (clippy, fmt, test).
   */
  runBackendQualityGates(workdir: string): QualityGateResult[] {
    const results: QualityGateResult[] = [];

    // Clippy
    results.push(this.runGate("cargo clippy", `cargo clippy --all-targets`, workdir));

    // Format check
    results.push(this.runGate("cargo fmt", `cargo fmt --check`, workdir));

    // Tests
    results.push(this.runGate("cargo test", `cargo test --lib`, workdir));

    const passed = results.filter((r) => r.passed).length;
    log.info(
      { passed, total: results.length, workdir },
      "Backend quality gates completed",
    );
    return results;
  }

  /**
   * Run frontend quality gates in a given directory.
   */
  runFrontendQualityGates(workdir: string): QualityGateResult[] {
    const results: QualityGateResult[] = [];
    const frontendDir = `${workdir}/frontend`;

    // Build
    results.push(this.runGate("npm build", `npm run build`, frontendDir));

    // Lint
    results.push(this.runGate("npm lint", `npm run lint`, frontendDir));

    const passed = results.filter((r) => r.passed).length;
    log.info(
      { passed, total: results.length, workdir: frontendDir },
      "Frontend quality gates completed",
    );
    return results;
  }

  /**
   * Run all quality gates for a scope.
   */
  runQualityGates(
    workdir: string,
    scope: "backend" | "frontend" | "both",
  ): QualityGateResult[] {
    const results: QualityGateResult[] = [];

    if (scope === "backend" || scope === "both") {
      const backendDir = `${workdir}/backend`;
      results.push(...this.runBackendQualityGates(backendDir));
    }

    if (scope === "frontend" || scope === "both") {
      results.push(...this.runFrontendQualityGates(workdir));
    }

    return results;
  }

  private runGate(
    name: string,
    command: string,
    cwd: string,
  ): QualityGateResult {
    const start = Date.now();
    try {
      const output = execSync(command, {
        encoding: "utf-8",
        timeout: 300_000, // 5 minute timeout for tests
        cwd,
        env: process.env,
      });
      return {
        gate: name,
        passed: true,
        output: output.slice(-2000), // Last 2000 chars
        duration_ms: Date.now() - start,
      };
    } catch (err) {
      const output =
        err instanceof Error
          ? (err as { stderr?: string; stdout?: string }).stderr ??
            (err as { stdout?: string }).stdout ??
            err.message
          : String(err);
      return {
        gate: name,
        passed: false,
        output: String(output).slice(-2000),
        duration_ms: Date.now() - start,
      };
    }
  }
}
