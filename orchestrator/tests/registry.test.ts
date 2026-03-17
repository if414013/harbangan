import { describe, it, expect } from "vitest";
import { AgentRegistry, WORKFLOW_TOOLS, SCOPE_AGENTS } from "../src/agents/registry.js";

describe("AgentRegistry", () => {
  const registry = new AgentRegistry();

  it("should load all 8 agents", () => {
    expect(registry.size).toBe(8);
  });

  it("should get agent by name", () => {
    const agent = registry.get("rust-backend-engineer");
    expect(agent).toBeDefined();
    expect(agent!.name).toBe("rust-backend-engineer");
    expect(agent!.model).toBe("claude-opus-4-6");
  });

  it("should return undefined for unknown agent", () => {
    expect(registry.get("nonexistent")).toBeUndefined();
  });

  it("should list all agents", () => {
    const agents = registry.listAll();
    expect(agents).toHaveLength(8);
    expect(agents.map((a) => a.name)).toContain("rust-backend-engineer");
    expect(agents.map((a) => a.name)).toContain("kanban-master");
  });

  it("should get agents for plan workflow", () => {
    const agents = registry.getForWorkflow("plan");
    expect(agents.length).toBeGreaterThan(0);
    expect(agents.every((a) => a.workflows.includes("plan"))).toBe(true);
  });

  it("should get agents for qa workflow", () => {
    const agents = registry.getForWorkflow("qa");
    expect(agents.length).toBeGreaterThan(0);
    expect(agents.every((a) => a.workflows.includes("qa"))).toBe(true);
  });

  it("should get agents for backend scope", () => {
    const agents = registry.getForScope("backend");
    expect(agents).toHaveLength(2);
    expect(agents.map((a) => a.name)).toContain("rust-backend-engineer");
    expect(agents.map((a) => a.name)).toContain("backend-qa");
  });

  it("should get agents for fullstack scope", () => {
    const agents = registry.getForScope("fullstack");
    expect(agents).toHaveLength(3);
    expect(agents.map((a) => a.name)).toContain("rust-backend-engineer");
    expect(agents.map((a) => a.name)).toContain("react-frontend-engineer");
    expect(agents.map((a) => a.name)).toContain("backend-qa");
  });

  it("should get tools for each workflow type", () => {
    expect(registry.getToolsForWorkflow("plan")).toContain("Read");
    expect(registry.getToolsForWorkflow("plan")).toContain("Grep");
    expect(registry.getToolsForWorkflow("plan")).not.toContain("Write");

    expect(registry.getToolsForWorkflow("implement")).toContain("Write");
    expect(registry.getToolsForWorkflow("implement")).toContain("Edit");
  });
});

describe("WORKFLOW_TOOLS", () => {
  it("plan should be read-only", () => {
    expect(WORKFLOW_TOOLS.plan).not.toContain("Write");
    expect(WORKFLOW_TOOLS.plan).not.toContain("Edit");
    expect(WORKFLOW_TOOLS.plan).toContain("Read");
  });

  it("pr-review should be read-only", () => {
    expect(WORKFLOW_TOOLS["pr-review"]).not.toContain("Write");
    expect(WORKFLOW_TOOLS["pr-review"]).not.toContain("Edit");
  });

  it("implement should have full access", () => {
    expect(WORKFLOW_TOOLS.implement).toContain("Read");
    expect(WORKFLOW_TOOLS.implement).toContain("Write");
    expect(WORKFLOW_TOOLS.implement).toContain("Edit");
    expect(WORKFLOW_TOOLS.implement).toContain("Bash");
  });
});

describe("SCOPE_AGENTS", () => {
  it("should define all scopes", () => {
    const scopes = Object.keys(SCOPE_AGENTS);
    expect(scopes).toContain("backend");
    expect(scopes).toContain("frontend");
    expect(scopes).toContain("fullstack");
    expect(scopes).toContain("database");
    expect(scopes).toContain("infrastructure");
    expect(scopes).toContain("documentation");
    expect(scopes).toContain("board");
  });
});
