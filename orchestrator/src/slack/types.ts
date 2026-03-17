/**
 * Types for Slack interactive workflows.
 */

/** Tracks a pending user interaction within a Slack thread. */
export interface PendingInteraction {
  resolve: (value: string) => void;
  reject: (reason: Error) => void;
  timer: ReturnType<typeof setTimeout>;
  question: string;
  createdAt: number;
}

/** Maps a task to its Slack thread for interactive communication. */
export interface ThreadBinding {
  channelId: string;
  threadTs: string;
  taskId: string;
  pending: PendingInteraction[];
}

/** Options for creating an interaction bridge. */
export interface InteractionBridgeOptions {
  /** Timeout in ms for waiting on user replies. Default: 30 min. */
  askUserTimeoutMs: number;
}

/** Slack service configuration. */
export interface SlackServiceConfig {
  appToken: string;
  botToken: string;
}
