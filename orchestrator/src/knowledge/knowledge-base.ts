/**
 * ChromaDB client wrapper for project knowledge storage and retrieval.
 * Uses default ONNX MiniLM embeddings (local, no API key required).
 */

import { ChromaClient, type Collection } from "chromadb";
import { v4 as uuidv4 } from "uuid";
import { createChildLogger } from "../util/logger.js";
import type {
  KnowledgeEntry,
  KnowledgeCreateInput,
  KnowledgeUpdateInput,
  KnowledgeSearchResult,
  KnowledgeStats,
  KnowledgeType,
} from "./types.js";

const log = createChildLogger("knowledge");
const COLLECTION_NAME = "project_knowledge";

export class KnowledgeBase {
  private client: ChromaClient;
  private collection: Collection | null = null;

  constructor(chromaUrl: string) {
    this.client = new ChromaClient({ path: chromaUrl });
  }

  async initialize(): Promise<void> {
    try {
      this.collection = await this.client.getOrCreateCollection({
        name: COLLECTION_NAME,
        metadata: { "hnsw:space": "cosine" },
      });
      const count = await this.collection.count();
      log.info({ collection: COLLECTION_NAME, count }, "Knowledge base initialized");
    } catch (err) {
      log.error({ error: String(err) }, "Failed to initialize knowledge base");
      throw err;
    }
  }

  private ensureCollection(): Collection {
    if (!this.collection) {
      throw new Error("Knowledge base not initialized — call initialize() first");
    }
    return this.collection;
  }

  async add(input: KnowledgeCreateInput): Promise<KnowledgeEntry> {
    const collection = this.ensureCollection();
    const now = new Date().toISOString();
    const id = uuidv4();

    const entry: KnowledgeEntry = {
      id,
      type: input.type,
      title: input.title,
      content: input.content,
      tags: input.tags ?? [],
      source_task_id: input.source_task_id ?? null,
      created_at: now,
      updated_at: now,
    };

    await collection.add({
      ids: [id],
      documents: [`${entry.title}\n\n${entry.content}`],
      metadatas: [this.entryToMetadata(entry)],
    });

    log.info({ id, type: entry.type, title: entry.title }, "Knowledge entry added");
    return entry;
  }

  async get(id: string): Promise<KnowledgeEntry | null> {
    const collection = this.ensureCollection();
    const result = await collection.get({ ids: [id] });

    if (!result.ids.length || !result.metadatas?.[0]) {
      return null;
    }

    return this.metadataToEntry(
      result.ids[0],
      result.documents?.[0] ?? "",
      result.metadatas[0] as Record<string, string>,
    );
  }

  async update(id: string, input: KnowledgeUpdateInput): Promise<KnowledgeEntry | null> {
    const collection = this.ensureCollection();
    const existing = await this.get(id);
    if (!existing) return null;

    const updated: KnowledgeEntry = {
      ...existing,
      title: input.title ?? existing.title,
      content: input.content ?? existing.content,
      type: input.type ?? existing.type,
      tags: input.tags ?? existing.tags,
      updated_at: new Date().toISOString(),
    };

    await collection.update({
      ids: [id],
      documents: [`${updated.title}\n\n${updated.content}`],
      metadatas: [this.entryToMetadata(updated)],
    });

    log.info({ id, type: updated.type }, "Knowledge entry updated");
    return updated;
  }

  async delete(id: string): Promise<boolean> {
    const collection = this.ensureCollection();
    const existing = await this.get(id);
    if (!existing) return false;

    await collection.delete({ ids: [id] });
    log.info({ id }, "Knowledge entry deleted");
    return true;
  }

  async search(
    query: string,
    options?: { type?: KnowledgeType; limit?: number },
  ): Promise<KnowledgeSearchResult[]> {
    const collection = this.ensureCollection();
    const limit = options?.limit ?? 5;

    const where = options?.type ? { type: { $eq: options.type } } : undefined;

    const results = await collection.query({
      queryTexts: [query],
      nResults: limit,
      ...(where ? { where } : {}),
    });

    if (!results.ids[0]?.length) {
      return [];
    }

    return results.ids[0].map((id, i) => ({
      entry: this.metadataToEntry(
        id,
        results.documents?.[0]?.[i] ?? "",
        (results.metadatas?.[0]?.[i] ?? {}) as Record<string, string>,
      ),
      score: results.distances?.[0]?.[i] != null ? 1 - results.distances[0][i] : 0,
    }));
  }

  async searchByTypes(
    query: string,
    types: KnowledgeType[],
    limit: number = 5,
  ): Promise<KnowledgeSearchResult[]> {
    const collection = this.ensureCollection();

    const where = types.length === 1
      ? { type: { $eq: types[0] } }
      : { type: { $in: types } };

    const results = await collection.query({
      queryTexts: [query],
      nResults: limit,
      where,
    });

    if (!results.ids[0]?.length) {
      return [];
    }

    return results.ids[0].map((id, i) => ({
      entry: this.metadataToEntry(
        id,
        results.documents?.[0]?.[i] ?? "",
        (results.metadatas?.[0]?.[i] ?? {}) as Record<string, string>,
      ),
      score: results.distances?.[0]?.[i] != null ? 1 - results.distances[0][i] : 0,
    }));
  }

  async list(options?: {
    type?: KnowledgeType;
    limit?: number;
    offset?: number;
  }): Promise<KnowledgeEntry[]> {
    const collection = this.ensureCollection();
    const limit = options?.limit ?? 50;
    const offset = options?.offset ?? 0;

    const where = options?.type ? { type: { $eq: options.type } } : undefined;

    const results = await collection.get({
      ...(where ? { where } : {}),
      limit,
      offset,
    });

    if (!results.ids.length) {
      return [];
    }

    return results.ids.map((id, i) =>
      this.metadataToEntry(
        id,
        results.documents?.[i] ?? "",
        (results.metadatas?.[i] ?? {}) as Record<string, string>,
      ),
    );
  }

  async stats(): Promise<KnowledgeStats> {
    const collection = this.ensureCollection();
    const total = await collection.count();

    const typeNames: KnowledgeType[] = ["decision", "task_summary", "learning", "incident"];
    const by_type: Record<string, number> = {};

    for (const type of typeNames) {
      const result = await collection.get({ where: { type: { $eq: type } } });
      by_type[type] = result.ids.length;
    }

    return { total, by_type: by_type as Record<KnowledgeType, number> };
  }

  private entryToMetadata(entry: KnowledgeEntry): Record<string, string> {
    return {
      type: entry.type,
      title: entry.title,
      tags: JSON.stringify(entry.tags),
      source_task_id: entry.source_task_id ?? "",
      created_at: entry.created_at,
      updated_at: entry.updated_at,
    };
  }

  private metadataToEntry(
    id: string,
    document: string,
    metadata: Record<string, string>,
  ): KnowledgeEntry {
    // Extract content from document (format: "title\n\ncontent")
    const titleEnd = document.indexOf("\n\n");
    const content = titleEnd >= 0 ? document.slice(titleEnd + 2) : document;

    return {
      id,
      type: (metadata.type as KnowledgeType) ?? "learning",
      title: metadata.title ?? "",
      content,
      tags: metadata.tags ? JSON.parse(metadata.tags) : [],
      source_task_id: metadata.source_task_id || null,
      created_at: metadata.created_at ?? new Date().toISOString(),
      updated_at: metadata.updated_at ?? new Date().toISOString(),
    };
  }
}
