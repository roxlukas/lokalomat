/** Engines and chat state (M2), fed by IPC and backend events. */
import {
  EVENTS,
  ipc,
  onEvent,
  type ChatDelta,
  type EngineStatus,
  type Message,
  type ModelView,
  type Session,
} from './ipc';
import { readLocal, writeLocal } from './storage';
import { logToApp } from './errors';
import { errorMessage } from './ipc';

const FINISHED = new Set(['done', 'failed', 'cancelled']);

export function isFinished(m: Message): boolean {
  return FINISHED.has(m.status);
}

class EnginesStore {
  list = $state<EngineStatus[]>([]);

  llama = $derived(this.list.find((e) => e.id === 'llama-cpp') ?? null);

  async load(): Promise<void> {
    try {
      this.list = await ipc.getEngines();
    } catch (e) {
      logToApp('warn', `get_engines failed: ${errorMessage(e)}`);
    }
  }

  async listen(): Promise<void> {
    await onEvent<EngineStatus>(EVENTS.enginesChanged, (status) => {
      const i = this.list.findIndex((e) => e.id === status.id);
      if (i >= 0) this.list[i] = status;
      else this.list.push(status);
    });
  }
}

export interface ChatModelChoice {
  modelId: string;
  variantId: string;
}

const MODEL_KEY = 'lokalsi.chat-model';

/** Installed chat models as `{ modelId, variantId, label }`. */
export function chatModelOptions(models: ModelView[]): (ChatModelChoice & { label: string; tools: boolean })[] {
  return models
    .filter((m) => m.modality === 'llm')
    .flatMap((m) =>
      m.variants
        .filter((v) => v.state === 'installed')
        .map((v) => ({
          modelId: m.id,
          variantId: v.id,
          label: `${m.name} · ${v.label}`,
          // Only models marked `tools` in the catalog receive MCP tools (ADR 0012).
          tools: m.features.includes('tools'),
        })),
    );
}

class ChatStore {
  sessions = $state<Session[]>([]);
  query = $state('');
  currentId = $state<number | null>(null);
  messages = $state<Message[]>([]);
  model = $state<ChatModelChoice | null>(readLocal<ChatModelChoice | null>(MODEL_KEY, null));

  current = $derived(this.sessions.find((s) => s.id === this.currentId) ?? null);
  active = $derived(this.messages.find((m) => m.role === 'assistant' && !isFinished(m)) ?? null);

  setModel(choice: ChatModelChoice | null): void {
    this.model = choice;
    writeLocal(MODEL_KEY, choice);
  }

  async loadSessions(): Promise<void> {
    try {
      this.sessions = await ipc.listSessions(this.query || undefined);
    } catch (e) {
      logToApp('warn', `list_sessions failed: ${errorMessage(e)}`);
    }
  }

  async open(id: number): Promise<void> {
    this.currentId = id;
    this.messages = await ipc.getMessages(id);
  }

  startNew(): void {
    this.currentId = null;
    this.messages = [];
  }

  /** Sends `text`; throws command errors (e.g. no model, engine missing) to the caller. */
  async send(text: string): Promise<void> {
    const sent = await ipc.chatSend(this.currentId, text, this.model ?? undefined);
    if (this.currentId !== sent.session.id) {
      this.currentId = sent.session.id;
      this.messages = [];
    }
    this.messages.push(sent.user, sent.assistant);
    void this.loadSessions();
  }

  async cancel(messageId: number): Promise<void> {
    await ipc.chatCancel(messageId);
  }

  async rename(id: number, title: string): Promise<void> {
    await ipc.renameSession(id, title);
    await this.loadSessions();
  }

  async pin(id: number, pinned: boolean): Promise<void> {
    await ipc.pinSession(id, pinned);
    await this.loadSessions();
  }

  async remove(id: number): Promise<void> {
    await ipc.deleteSession(id);
    if (this.currentId === id) this.startNew();
    await this.loadSessions();
  }

  async listen(): Promise<void> {
    await onEvent<ChatDelta>(EVENTS.chatDelta, (d) => {
      const m = this.messages.find((x) => x.id === d.message_id);
      if (!m) return;
      m.content += d.content;
      m.reasoning += d.reasoning;
      if (m.status !== 'running') m.status = 'running';
    });
    await onEvent<Message>(EVENTS.chatUpdated, (update) => {
      const i = this.messages.findIndex((x) => x.id === update.id);
      if (i < 0) return;
      const local = this.messages[i]!;
      // While streaming the database row lags behind the deltas; take its text only when finished.
      this.messages[i] = isFinished(update) ? update : { ...update, content: local.content, reasoning: local.reasoning };
      if (isFinished(update)) void this.loadSessions();
    });
  }
}

export const engines = new EnginesStore();
export const chat = new ChatStore();

export async function initChat(): Promise<void> {
  await Promise.all([engines.listen(), chat.listen()]);
  await Promise.all([engines.load(), chat.loadSessions()]);
}
