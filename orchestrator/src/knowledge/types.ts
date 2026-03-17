/**
 * Types for the ChromaDB-backed project knowledge base.
 */

export type KnowledgeType = "decision" | "task_summary" | "learning" | "incident";

export interface KnowledgeEntry {
  id: string;
  type: KnowledgeType;
  title: string;
  content: string;
  tags: string[];
  source_task_id: string | null;
  created_at: string; // ISO8601
  updated_at: string; // ISO8601
}

export interface KnowledgeCreateInput {
  type: KnowledgeType;
  title: string;
  content: string;
  tags?: string[];
  source_task_id?: string;
}

export interface KnowledgeUpdateInput {
  title?: string;
  content?: string;
  type?: KnowledgeType;
  tags?: string[];
}

export interface KnowledgeSearchResult {
  entry: KnowledgeEntry;
  score: number;
}

export interface KnowledgeContext {
  entries: KnowledgeSearchResult[];
  formatted: string;
}

export interface KnowledgeStats {
  total: number;
  by_type: Record<KnowledgeType, number>;
}
