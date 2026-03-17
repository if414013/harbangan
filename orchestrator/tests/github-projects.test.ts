import { describe, it, expect, beforeEach, vi } from "vitest";
import { GitHubProjects } from "../src/store/github-projects.js";

vi.mock("node:child_process", () => ({
  execSync: vi.fn(),
}));

import { execSync } from "node:child_process";
const mockExec = vi.mocked(execSync);

const MOCK_FIELDS_RESPONSE = JSON.stringify({
  data: {
    user: {
      projectV2: {
        id: "PVT_proj1",
        fields: {
          nodes: [
            {
              id: "PVTSSF_status",
              name: "Orchestrator Status",
              options: [
                { id: "opt_pending", name: "pending" },
                { id: "opt_queued", name: "queued" },
                { id: "opt_running", name: "running" },
                { id: "opt_completed", name: "completed" },
                { id: "opt_failed", name: "failed" },
                { id: "opt_cancelled", name: "cancelled" },
              ],
            },
            {
              id: "PVTSSF_type",
              name: "Orchestrator Type",
              options: [
                { id: "opt_plan", name: "plan" },
                { id: "opt_implement", name: "implement" },
                { id: "opt_pr_review", name: "pr-review" },
              ],
            },
            { id: "PVTF_cost", name: "Cost USD" },
            { id: "PVTF_budget", name: "Budget USD" },
            { id: "PVTF_branch", name: "Branch" },
            { id: "PVTF_prurl", name: "PR URL" },
            { id: "PVTF_title", name: "Title" },
            { id: "PVTF_status_builtin", name: "Status" },
          ],
        },
      },
    },
  },
});

describe("GitHubProjects", () => {
  let projects: GitHubProjects;

  beforeEach(() => {
    vi.clearAllMocks();
    projects = new GitHubProjects("fake-token", "if414013", "harbangan", 3);
  });

  describe("initialize", () => {
    it("should resolve project ID and field IDs", async () => {
      mockExec.mockReturnValue((MOCK_FIELDS_RESPONSE));

      await projects.initialize();

      const fields = projects.fieldIds;
      expect(fields).not.toBeNull();
      expect(fields!.projectId).toBe("PVT_proj1");
      expect(fields!.statusFieldId).toBe("PVTSSF_status");
      expect(fields!.typeFieldId).toBe("PVTSSF_type");
      expect(fields!.costFieldId).toBe("PVTF_cost");
      expect(fields!.budgetFieldId).toBe("PVTF_budget");
      expect(fields!.branchFieldId).toBe("PVTF_branch");
      expect(fields!.prUrlFieldId).toBe("PVTF_prurl");
    });

    it("should map status options", async () => {
      mockExec.mockReturnValue((MOCK_FIELDS_RESPONSE));

      await projects.initialize();

      const fields = projects.fieldIds!;
      expect(fields.statusOptions.get("pending")).toBe("opt_pending");
      expect(fields.statusOptions.get("running")).toBe("opt_running");
      expect(fields.statusOptions.get("completed")).toBe("opt_completed");
    });

    it("should throw when project not found", async () => {
      mockExec.mockReturnValue(
        (JSON.stringify({ data: { user: { projectV2: null } } })),
      );

      await expect(projects.initialize()).rejects.toThrow("Project #3 not found");
    });

    it("should throw when required field is missing", async () => {
      const missingField = JSON.stringify({
        data: {
          user: {
            projectV2: {
              id: "PVT_proj1",
              fields: {
                nodes: [
                  { id: "PVTF_cost", name: "Cost USD" },
                  // Missing other required fields
                ],
              },
            },
          },
        },
      });
      mockExec.mockReturnValue((missingField));

      await expect(projects.initialize()).rejects.toThrow(
        'Project field "Orchestrator Status" not found',
      );
    });
  });

  describe("addItemToProject", () => {
    it("should call GraphQL mutation and return item ID", async () => {
      // Initialize first
      mockExec.mockReturnValueOnce((MOCK_FIELDS_RESPONSE));
      await projects.initialize();

      // Mock addItem response
      mockExec.mockReturnValueOnce(
        (
          JSON.stringify({
            data: { addProjectV2ItemById: { item: { id: "PVTI_newitem" } } },
          }),
        ),
      );

      const itemId = projects.addItemToProject("I_issue_node");
      expect(itemId).toBe("PVTI_newitem");
    });

    it("should throw when not initialized", () => {
      expect(() => projects.addItemToProject("I_123")).toThrow(
        "GitHubProjects not initialized",
      );
    });
  });

  describe("updateTaskFields", () => {
    it("should set multiple fields", async () => {
      mockExec.mockReturnValueOnce((MOCK_FIELDS_RESPONSE));
      await projects.initialize();

      // Each field update calls graphql once
      mockExec.mockReturnValue(
        (JSON.stringify({ data: { updateProjectV2ItemFieldValue: { projectV2Item: { id: "PVTI_1" } } } })),
      );

      projects.updateTaskFields("PVTI_item1", {
        status: "running",
        type: "plan",
        cost: 5.0,
        budget: 10,
        branch: "feat/test",
        prUrl: "https://github.com/test/pr/1",
      });

      // Should have called graphql 6 times (1 for each field)
      // +1 for initialize = 7 total
      expect(mockExec).toHaveBeenCalledTimes(7);
    });
  });
});
