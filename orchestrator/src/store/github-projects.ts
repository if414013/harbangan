import { execSync } from "node:child_process";
import { createChildLogger } from "../util/logger.js";

const log = createChildLogger("github-projects");

export interface ProjectFieldIds {
  projectId: string;
  statusFieldId: string;
  typeFieldId: string;
  costFieldId: string;
  budgetFieldId: string;
  branchFieldId: string;
  prUrlFieldId: string;
  statusOptions: Map<string, string>; // status name → option ID
  typeOptions: Map<string, string>; // type name → option ID
}

/**
 * GitHub Projects V2 GraphQL API wrapper.
 * Resolves field IDs on init and provides typed field update operations.
 */
export class GitHubProjects {
  private ghToken: string;
  private owner: string;
  private projectNumber: number;
  private fields: ProjectFieldIds | null = null;

  constructor(ghToken: string, owner: string, _repo: string, projectNumber: number) {
    this.ghToken = ghToken;
    this.owner = owner;
    this.projectNumber = projectNumber;
  }

  /** Resolve project ID and all custom field IDs. Call once on startup. */
  async initialize(): Promise<void> {
    const query = `
      query($owner: String!, $number: Int!) {
        user(login: $owner) {
          projectV2(number: $number) {
            id
            fields(first: 50) {
              nodes {
                ... on ProjectV2SingleSelectField {
                  id
                  name
                  options { id name }
                }
                ... on ProjectV2Field {
                  id
                  name
                }
              }
            }
          }
        }
      }
    `;

    const result = this.graphql(query, { owner: this.owner, number: this.projectNumber });
    const project = result.data?.user?.projectV2;
    if (!project) {
      throw new Error(`Project #${this.projectNumber} not found for ${this.owner}`);
    }

    const fields = project.fields.nodes as Array<{
      id: string;
      name: string;
      options?: Array<{ id: string; name: string }>;
    }>;

    const findField = (name: string) => {
      const f = fields.find((f) => f.name === name);
      if (!f) throw new Error(`Project field "${name}" not found. Create it in the project settings.`);
      return f;
    };

    const statusField = findField("Orchestrator Status");
    const typeField = findField("Orchestrator Type");
    const costField = findField("Cost USD");
    const budgetField = findField("Budget USD");
    const branchField = findField("Branch");
    const prUrlField = findField("PR URL");

    const mapOptions = (field: { options?: Array<{ id: string; name: string }> }) => {
      const map = new Map<string, string>();
      for (const opt of field.options ?? []) {
        map.set(opt.name, opt.id);
      }
      return map;
    };

    this.fields = {
      projectId: project.id,
      statusFieldId: statusField.id,
      typeFieldId: typeField.id,
      costFieldId: costField.id,
      budgetFieldId: budgetField.id,
      branchFieldId: branchField.id,
      prUrlFieldId: prUrlField.id,
      statusOptions: mapOptions(statusField),
      typeOptions: mapOptions(typeField),
    };

    log.info(
      { projectId: project.id, fieldCount: fields.length },
      "GitHub Projects V2 fields resolved",
    );
  }

  /** Add an issue to the project and return the item ID. */
  addItemToProject(issueNodeId: string): string {
    this.ensureInitialized();
    const query = `
      mutation($projectId: ID!, $contentId: ID!) {
        addProjectV2ItemById(input: { projectId: $projectId, contentId: $contentId }) {
          item { id }
        }
      }
    `;
    const result = this.graphql(query, {
      projectId: this.fields!.projectId,
      contentId: issueNodeId,
    });
    return result.data.addProjectV2ItemById.item.id;
  }

  /** Set a single select field (status or type). */
  setSelectField(itemId: string, fieldId: string, optionId: string): void {
    this.ensureInitialized();
    const query = `
      mutation($projectId: ID!, $itemId: ID!, $fieldId: ID!, $optionId: String!) {
        updateProjectV2ItemFieldValue(input: {
          projectId: $projectId, itemId: $itemId,
          fieldId: $fieldId,
          value: { singleSelectOptionId: $optionId }
        }) { projectV2Item { id } }
      }
    `;
    this.graphql(query, {
      projectId: this.fields!.projectId,
      itemId,
      fieldId,
      optionId,
    });
  }

  /** Set a number field (cost, budget). */
  setNumberField(itemId: string, fieldId: string, value: number): void {
    this.ensureInitialized();
    const query = `
      mutation($projectId: ID!, $itemId: ID!, $fieldId: ID!, $value: Float!) {
        updateProjectV2ItemFieldValue(input: {
          projectId: $projectId, itemId: $itemId,
          fieldId: $fieldId,
          value: { number: $value }
        }) { projectV2Item { id } }
      }
    `;
    this.graphql(query, {
      projectId: this.fields!.projectId,
      itemId,
      fieldId,
      value,
    });
  }

  /** Set a text field (branch, PR URL). */
  setTextField(itemId: string, fieldId: string, value: string): void {
    this.ensureInitialized();
    const query = `
      mutation($projectId: ID!, $itemId: ID!, $fieldId: ID!, $value: String!) {
        updateProjectV2ItemFieldValue(input: {
          projectId: $projectId, itemId: $itemId,
          fieldId: $fieldId,
          value: { text: $value }
        }) { projectV2Item { id } }
      }
    `;
    this.graphql(query, {
      projectId: this.fields!.projectId,
      itemId,
      fieldId,
      value,
    });
  }

  /** Update all orchestrator fields for an item. */
  updateTaskFields(
    itemId: string,
    updates: {
      status?: string;
      type?: string;
      cost?: number;
      budget?: number;
      branch?: string;
      prUrl?: string;
    },
  ): void {
    this.ensureInitialized();
    const f = this.fields!;

    if (updates.status) {
      const optionId = f.statusOptions.get(updates.status);
      if (optionId) {
        this.setSelectField(itemId, f.statusFieldId, optionId);
      } else {
        log.warn({ status: updates.status }, "Unknown orchestrator status option");
      }
    }
    if (updates.type) {
      const optionId = f.typeOptions.get(updates.type);
      if (optionId) {
        this.setSelectField(itemId, f.typeFieldId, optionId);
      } else {
        log.warn({ type: updates.type }, "Unknown orchestrator type option");
      }
    }
    if (updates.cost !== undefined) {
      this.setNumberField(itemId, f.costFieldId, updates.cost);
    }
    if (updates.budget !== undefined) {
      this.setNumberField(itemId, f.budgetFieldId, updates.budget);
    }
    if (updates.branch) {
      this.setTextField(itemId, f.branchFieldId, updates.branch);
    }
    if (updates.prUrl) {
      this.setTextField(itemId, f.prUrlFieldId, updates.prUrl);
    }
  }

  get fieldIds(): ProjectFieldIds | null {
    return this.fields;
  }

  private ensureInitialized(): void {
    if (!this.fields) {
      throw new Error("GitHubProjects not initialized. Call initialize() first.");
    }
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  private graphql(query: string, variables: Record<string, unknown>): any {
    const varsJson = JSON.stringify(variables);
    const queryOneLine = query.replace(/\n/g, " ").replace(/\s+/g, " ").trim();

    try {
      const output = execSync(
        `gh api graphql -f query='${queryOneLine}' -f variables='${varsJson}'`,
        {
          encoding: "utf-8",
          timeout: 30_000,
          env: { ...process.env, GH_TOKEN: this.ghToken },
        },
      ).trim();
      return JSON.parse(output) as Record<string, unknown>;
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      log.error({ error: msg }, "GraphQL request failed");
      throw new Error(`GitHub Projects GraphQL failed: ${msg}`);
    }
  }
}
