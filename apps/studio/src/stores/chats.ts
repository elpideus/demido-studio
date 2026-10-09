// Conversations: the chat list, each chat's messages, and the live turn state. The backend is
// the source of truth; this store mirrors it from command results and `chat://event`s.

import { create } from 'zustand';

import { api, errorText } from '@/lib/api';
import type { Chat, ChatEvent, Message, ModelEntry } from '@/lib/types';
import { useApp } from './app';
import { toast } from './toasts';

/** A busy model's answer that is about to be asked for again. */
export interface Retry {
  /** Retry number, from 1, of `attempts`. */
  attempt: number;
  attempts: number;
  /** When it is asked for again, in ms since the epoch. */
  at: number;
  reason: string;
}

interface ChatsStore {
  chats: Chat[];
  /** The chat on screen; null is a new, not yet started chat. */
  activeId: string | null;
  messages: Record<string, Message[]>;
  running: Record<string, boolean>;
  turnError: Record<string, string | null>;
  /** Model picked in the composer for the chat on screen, overriding the chat's last model. */
  pickedModelId: string | null;
  /** Tool calls the model is still writing, by assistant message id. */
  pendingTool: Record<string, string>;
  /** Answers waiting for a busy model, by message id. */
  retrying: Record<string, Retry>;
  loadChats: () => Promise<void>;
  open: (id: string | null) => Promise<void>;
  /** Sends a message with the staged files `attachmentIds`; false when the backend refused it. */
  send: (text: string, modelId: string, attachmentIds?: string[]) => Promise<boolean>;
  /** Runs the slash command `text`; false when it failed. */
  runCommand: (text: string, modelId: string, attachmentIds?: string[]) => Promise<boolean>;
  stop: () => Promise<void>;
  regenerate: (modelId: string) => Promise<void>;
  edit: (messageId: string, text: string, modelId: string) => Promise<void>;
  rename: (id: string, title: string) => Promise<void>;
  remove: (id: string) => Promise<void>;
  pin: (id: string, pinned: boolean) => Promise<void>;
  pickModel: (id: string | null) => void;
  apply: (event: ChatEvent) => void;
}

function without<T>(record: Record<string, T>, key: string): Record<string, T> {
  if (!(key in record)) return record;
  const rest = { ...record };
  delete rest[key];
  return rest;
}

function sortChats(chats: Chat[]): Chat[] {
  return [...chats].sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.updatedAt - a.updatedAt);
}

function upsertMessage(list: Message[] | undefined, message: Message): Message[] {
  const base = (list ?? []).filter((m) => m.id !== message.id);
  base.push(message);
  return base.sort((a, b) => a.seq - b.seq);
}

export const useChats = create<ChatsStore>((set, get) => ({
  chats: [],
  activeId: null,
  messages: {},
  running: {},
  turnError: {},
  pickedModelId: null,
  pendingTool: {},
  retrying: {},

  loadChats: async () => {
    const [chats, running] = await Promise.all([api.listChats(), api.runningTurns()]);
    set({ chats: sortChats(chats), running: Object.fromEntries(running.map((id) => [id, true])) });
  },

  open: async (id) => {
    set({ activeId: id, pickedModelId: null });
    void api.updateSettings({ lastChatId: id });
    if (id === null || get().messages[id]) return;
    try {
      const list = await api.getMessages(id);
      // Events may have arrived while loading; keep whichever copy is newer.
      set((s) => {
        const live = s.messages[id] ?? [];
        const merged = list.map((m) => live.find((l) => l.id === m.id) ?? m);
        for (const l of live) if (!merged.some((m) => m.id === l.id)) merged.push(l);
        return { messages: { ...s.messages, [id]: merged.sort((a, b) => a.seq - b.seq) } };
      });
    } catch (e) {
      toast.error('Could not open the chat', errorText(e));
    }
  },

  send: async (text, modelId, attachmentIds = []) => {
    const { activeId } = get();
    try {
      const { chat, message } = await api.sendMessage(activeId, text, modelId, attachmentIds);
      showSent(chat, message, activeId === null);
      return true;
    } catch (e) {
      toast.error('Could not send the message', errorText(e));
      return false;
    }
  },

  runCommand: async (text, modelId, attachmentIds = []) => {
    const { activeId } = get();
    try {
      const outcome = await api.runSlashCommand(activeId, text, modelId, attachmentIds);
      switch (outcome.kind) {
        case 'sent':
          showSent(outcome.chat, outcome.message, activeId === null);
          break;
        case 'done':
          if (outcome.settings) {
            useApp.setState({ settings: outcome.settings });
            toast.success(outcome.title, outcome.text);
          } else {
            toast.info(outcome.title, outcome.text);
          }
          break;
        case 'started':
          // It streams into the chat as a turn does.
          break;
      }
      return true;
    } catch (e) {
      const name = text.trim().split(/\s/, 1)[0];
      toast.error(`Could not run ${name}`, errorText(e));
      return false;
    }
  },

  stop: async () => {
    const id = get().activeId;
    if (id) await api.stopTurn(id);
  },

  regenerate: async (modelId) => {
    const id = get().activeId;
    if (!id) return;
    try {
      await api.regenerate(id, modelId);
    } catch (e) {
      toast.error('Could not regenerate', errorText(e));
    }
  },

  edit: async (messageId, text, modelId) => {
    const id = get().activeId;
    if (!id) return;
    try {
      await api.editMessage(id, messageId, text, modelId);
    } catch (e) {
      toast.error('Could not edit the message', errorText(e));
    }
  },

  rename: async (id, title) => {
    try {
      const chat = await api.renameChat(id, title);
      set((s) => ({ chats: sortChats(s.chats.map((c) => (c.id === id ? chat : c))) }));
    } catch (e) {
      toast.error('Could not rename the chat', errorText(e));
    }
  },

  remove: async (id) => {
    try {
      await api.deleteChat(id);
      set((s) => {
        const messages = { ...s.messages };
        delete messages[id];
        return {
          chats: s.chats.filter((c) => c.id !== id),
          messages,
          activeId: s.activeId === id ? null : s.activeId,
        };
      });
    } catch (e) {
      toast.error('Could not delete the chat', errorText(e));
    }
  },

  pin: async (id, pinned) => {
    const chat = await api.pinChat(id, pinned);
    set((s) => ({ chats: sortChats(s.chats.map((c) => (c.id === id ? chat : c))) }));
  },

  pickModel: (id) => set({ pickedModelId: id }),

  apply: (event) => {
    switch (event.type) {
      case 'message':
        set((s) => {
          const done = event.message.status !== 'streaming';
          return {
            messages: { ...s.messages, [event.chatId]: upsertMessage(s.messages[event.chatId], event.message) },
            pendingTool: done ? without(s.pendingTool, event.message.id) : s.pendingTool,
            retrying: done ? without(s.retrying, event.message.id) : s.retrying,
          };
        });
        break;
      case 'delta':
        set((s) => {
          const list = s.messages[event.chatId];
          if (!list) return {};
          return {
            retrying: without(s.retrying, event.messageId),
            messages: {
              ...s.messages,
              [event.chatId]: list.map((m) =>
                m.id === event.messageId
                  ? {
                      ...m,
                      content: m.content + event.content,
                      reasoning: event.reasoning ? (m.reasoning ?? '') + event.reasoning : m.reasoning,
                    }
                  : m,
              ),
            },
          };
        });
        break;
      case 'toolCall':
        set((s) => ({
          pendingTool: { ...s.pendingTool, [event.messageId]: event.name },
          retrying: without(s.retrying, event.messageId),
        }));
        break;
      case 'retrying':
        // What streamed of the failed attempt is void: the next one starts the answer afresh.
        set((s) => {
          const list = s.messages[event.chatId];
          const retry = {
            attempt: event.attempt,
            attempts: event.attempts,
            at: Date.now() + event.waitMs,
            reason: event.reason,
          };
          return {
            retrying: { ...s.retrying, [event.messageId]: retry },
            pendingTool: without(s.pendingTool, event.messageId),
            messages: list
              ? {
                  ...s.messages,
                  [event.chatId]: list.map((m) =>
                    m.id === event.messageId ? { ...m, content: '', reasoning: null } : m,
                  ),
                }
              : s.messages,
          };
        });
        break;
      case 'truncated':
        set((s) => {
          const list = s.messages[event.chatId];
          if (!list) return {};
          return { messages: { ...s.messages, [event.chatId]: list.filter((m) => m.seq < event.fromSeq) } };
        });
        break;
      case 'turnStarted':
        set((s) => ({
          running: { ...s.running, [event.chatId]: true },
          turnError: { ...s.turnError, [event.chatId]: null },
        }));
        break;
      case 'turnFinished':
        set((s) => ({
          running: { ...s.running, [event.chatId]: false },
          turnError: { ...s.turnError, [event.chatId]: event.error },
        }));
        break;
      case 'chat':
        set((s) => ({ chats: sortChats([event.chat, ...s.chats.filter((c) => c.id !== event.chat.id)]) }));
        break;
      case 'notice':
        if (event.kind === 'tradingviewLogin') {
          toast.warning(
            'Sign in to TradingView',
            'A sign-in window opened so the assistant can read live market data.',
          );
        } else {
          toast.info(event.text);
        }
        break;
    }
  },
}));

/** Shows a message just sent, in a chat that may have been started by it. */
function showSent(chat: Chat, message: Message, started: boolean) {
  useChats.setState((s) => ({
    activeId: chat.id,
    chats: sortChats([chat, ...s.chats.filter((c) => c.id !== chat.id)]),
    messages: { ...s.messages, [chat.id]: upsertMessage(s.messages[chat.id], message) },
    turnError: { ...s.turnError, [chat.id]: null },
  }));
  if (started) void api.updateSettings({ lastChatId: chat.id });
}

/** The model the composer will send with. */
export function currentModel(
  models: ModelEntry[],
  chat: Chat | undefined,
  picked: string | null,
): ModelEntry | undefined {
  const usable = (id: string | null | undefined) => (id ? models.find((m) => m.id === id && m.enabled) : undefined);
  return (
    usable(picked) ??
    usable(chat?.modelId) ??
    models.find((m) => m.isDefault && m.enabled) ??
    models.find((m) => m.enabled)
  );
}
