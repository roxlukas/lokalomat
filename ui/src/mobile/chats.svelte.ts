/**
 * The phone's chat list (ADR 0039): past chats kept on the phone itself, for
 * the chat on the phone and the chat with the computer alike. Only the phone
 * keeps them; nothing is sent anywhere.
 */
import { readLocal, writeLocal } from '../lib/storage';
import type { Turn } from './api';

export interface PhoneChat {
  id: string;
  title: string;
  /** Milliseconds since the epoch, newest first in the list. */
  updated: number;
  messages: Turn[];
}

const KEY = 'lokalsi.mobile.chats';
/** localStorage holds a few MB: the oldest chats go first. */
export const MAX_CHATS = 50;
const TITLE_LEN = 48;

/** The first question, shortened, names the chat. */
export function titleOf(messages: Turn[]): string {
  const first = messages.find((m) => m.role === 'user')?.content.trim().replace(/\s+/g, ' ') ?? '';
  return first.length > TITLE_LEN ? `${first.slice(0, TITLE_LEN - 1)}…` : first;
}

/** Puts `chat` first, drops its old copy and anything past `max`. */
export function upsert(list: PhoneChat[], chat: PhoneChat, max = MAX_CHATS): PhoneChat[] {
  return [chat, ...list.filter((c) => c.id !== chat.id)].slice(0, max);
}

function newId(): string {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

function readList(): PhoneChat[] {
  const list = readLocal<PhoneChat[]>(KEY, []);
  return Array.isArray(list) ? list.filter((c) => c && typeof c.id === 'string' && Array.isArray(c.messages)) : [];
}

class Chats {
  list = $state<PhoneChat[]>(readList());
  /** The open chat; a new one gets an id once its first message is saved. */
  currentId = $state<string | null>(null);
  messages = $state<Turn[]>([]);

  /** Keeps the open chat (call after each question and reply). */
  save() {
    const messages = this.messages.filter((m) => m.role !== 'assistant' || m.content);
    if (messages.length === 0) return;
    const id = this.currentId ?? newId();
    this.currentId = id;
    this.list = upsert(this.list, { id, title: titleOf(messages), updated: Date.now(), messages: $state.snapshot(messages) });
    writeLocal(KEY, this.list);
  }

  open(id: string) {
    const chat = this.list.find((c) => c.id === id);
    if (!chat) return;
    this.currentId = id;
    this.messages = structuredClone($state.snapshot(chat.messages));
  }

  startNew() {
    this.currentId = null;
    this.messages = [];
  }

  remove(id: string) {
    this.list = this.list.filter((c) => c.id !== id);
    writeLocal(KEY, this.list);
    if (this.currentId === id) this.startNew();
  }

  /** The trash button: the open chat is gone, an empty one takes its place. */
  clear() {
    if (this.currentId) this.remove(this.currentId);
    else this.startNew();
  }
}

export const chats = new Chats();
