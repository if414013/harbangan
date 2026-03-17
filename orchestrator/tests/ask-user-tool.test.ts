import { describe, it, expect, vi } from "vitest";

// Mock the SDK before importing the module under test
vi.mock("@anthropic-ai/claude-agent-sdk", () => ({
  createSdkMcpServer: vi.fn((opts: { name: string; tools: unknown[] }) => ({
    type: "sdk" as const,
    name: opts.name,
    tools: opts.tools,
  })),
  tool: vi.fn((name: string, description: string, schema: unknown, handler: unknown) => ({
    name,
    description,
    inputSchema: schema,
    handler,
  })),
}));

import { createAskUserMcpServer } from "../src/slack/ask-user-tool.js";

describe("createAskUserMcpServer", () => {
  it("should create an MCP server with ask_user tool", () => {
    const mockSlackService = {
      askUserInThread: vi.fn(),
    } as any;

    const server = createAskUserMcpServer("task-1", mockSlackService);
    expect(server).toBeDefined();
    expect(server.name).toBe("slack-interaction");
  });

  it("should call askUserInThread when tool handler is invoked", async () => {
    const mockSlackService = {
      askUserInThread: vi.fn().mockResolvedValue("user's answer"),
    } as any;

    const server = createAskUserMcpServer("task-1", mockSlackService);
    const tools = (server as any).tools as Array<{ handler: (args: { question: string }) => Promise<any> }>;
    const askUserHandler = tools[0].handler;

    const result = await askUserHandler({ question: "What color?" });
    expect(mockSlackService.askUserInThread).toHaveBeenCalledWith("task-1", "What color?");
    expect(result.content[0].text).toBe("user's answer");
  });

  it("should return error when askUserInThread fails", async () => {
    const mockSlackService = {
      askUserInThread: vi.fn().mockRejectedValue(new Error("Timeout")),
    } as any;

    const server = createAskUserMcpServer("task-1", mockSlackService);
    const tools = (server as any).tools as Array<{ handler: (args: { question: string }) => Promise<any> }>;
    const askUserHandler = tools[0].handler;

    const result = await askUserHandler({ question: "Quick?" });
    expect(result.isError).toBe(true);
    expect(result.content[0].text).toContain("Timeout");
  });
});
