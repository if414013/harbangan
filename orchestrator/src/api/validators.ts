import { z } from "zod";

export const dispatchTaskSchema = z.object({
  type: z.enum(["plan", "pr-review", "implement", "kanban", "docs", "qa"]),
  priority: z.enum(["high", "normal", "low"]).default("normal"),
  input: z.object({
    description: z.string().optional(),
    pr_number: z.number().int().positive().optional(),
    issue_number: z.number().int().positive().optional(),
    plan_task_id: z.string().optional(),
    command: z.string().optional(),
    scope: z.string().optional(),
    target: z.string().optional(),
    budget_usd: z.number().positive().optional(),
  }),
});

export type DispatchTaskInput = z.infer<typeof dispatchTaskSchema>;

export const listTasksSchema = z.object({
  status: z
    .enum(["pending", "queued", "running", "completed", "failed", "cancelled"])
    .optional(),
  type: z
    .enum(["plan", "pr-review", "implement", "kanban", "docs", "qa"])
    .optional(),
  limit: z.coerce.number().int().positive().max(100).default(50),
  offset: z.coerce.number().int().min(0).default(0),
});

export const slackCommandSchema = z.object({
  token: z.string().optional(),
  command: z.string(),
  text: z.string(),
  response_url: z.string().url().optional(),
  user_id: z.string().optional(),
  user_name: z.string().optional(),
  channel_id: z.string().optional(),
  channel_name: z.string().optional(),
});

export const knowledgeCreateSchema = z.object({
  type: z.enum(["decision", "task_summary", "learning", "incident"]),
  title: z.string().min(1).max(200),
  content: z.string().min(1).max(10000),
  tags: z.array(z.string().max(50)).max(20).optional(),
  source_task_id: z.string().uuid().optional(),
});

export const knowledgeUpdateSchema = z.object({
  title: z.string().min(1).max(200).optional(),
  content: z.string().min(1).max(10000).optional(),
  type: z.enum(["decision", "task_summary", "learning", "incident"]).optional(),
  tags: z.array(z.string().max(50)).max(20).optional(),
});

export const knowledgeSearchSchema = z.object({
  query: z.string().min(1).max(1000),
  type: z.enum(["decision", "task_summary", "learning", "incident"]).optional(),
  limit: z.coerce.number().int().positive().max(20).default(5),
});

export const knowledgeListSchema = z.object({
  type: z.enum(["decision", "task_summary", "learning", "incident"]).optional(),
  limit: z.coerce.number().int().positive().max(100).default(50),
  offset: z.coerce.number().int().min(0).default(0),
});
