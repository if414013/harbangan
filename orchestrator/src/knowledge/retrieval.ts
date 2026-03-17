/**
 * Builds formatted context strings from knowledge base search results
 * for injection into workflow prompts.
 */

import { createChildLogger } from "../util/logger.js";
import type { KnowledgeBase } from "./knowledge-base.js";
import type { KnowledgeContext, KnowledgeType } from "./types.js";

const log = createChildLogger("knowledge:retrieval");

const MAX_RESULTS = 5;
const MAX_ENTRY_CHARS = 1500;

export class KnowledgeRetriever {
  constructor(private kb: KnowledgeBase) {}

  /**
   * Retrieve and format relevant knowledge for a given query and type filter.
   * Returns both raw results and a formatted string ready for prompt injection.
   */
  async getContext(
    query: string,
    types: KnowledgeType[],
  ): Promise<KnowledgeContext> {
    try {
      const results = await this.kb.searchByTypes(query, types, MAX_RESULTS);

      if (!results.length) {
        return { entries: [], formatted: "" };
      }

      const formatted = this.formatContext(results);

      log.debug(
        { query: query.slice(0, 80), types, resultCount: results.length },
        "Knowledge context retrieved",
      );

      return { entries: results, formatted };
    } catch (err) {
      log.warn(
        { query: query.slice(0, 80), error: String(err) },
        "Knowledge retrieval failed — proceeding without context",
      );
      return { entries: [], formatted: "" };
    }
  }

  /**
   * Format search results into a markdown section for prompt injection.
   */
  private formatContext(
    results: KnowledgeContext["entries"],
  ): string {
    const sections = results.map((r, i) => {
      const content = r.entry.content.length > MAX_ENTRY_CHARS
        ? r.entry.content.slice(0, MAX_ENTRY_CHARS) + "..."
        : r.entry.content;

      const tags = r.entry.tags.length ? ` [${r.entry.tags.join(", ")}]` : "";

      return `### ${i + 1}. ${r.entry.title}${tags}
**Type**: ${r.entry.type} | **Relevance**: ${Math.round(r.score * 100)}%

${content}`;
    });

    return `## Relevant Project Knowledge

The following entries from the project knowledge base may be relevant to this task:

${sections.join("\n\n---\n\n")}`;
  }
}
