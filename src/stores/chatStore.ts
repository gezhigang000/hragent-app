/**
 * Chat store — conversation and message state management.
 * Based on tech-architecture.md §3.2
 */
import { create } from 'zustand'
import type { Conversation, Message } from '@/types/message'

interface ToolExecution {
  toolName: string
  toolId: string
  status: 'executing' | 'completed' | 'error'
  summary?: string
}

interface ChatState {
  // Conversation list
  conversations: Conversation[]
  activeConversationId: string | null

  // Current conversation messages
  messages: Message[]
  isStreaming: boolean
  streamingContent: string

  // Tool executions
  toolExecutions: ToolExecution[]

  // Actions
  setConversations: (conversations: Conversation[]) => void
  setActiveConversation: (id: string | null) => void
  setMessages: (messages: Message[]) => void
  addMessage: (message: Message) => void
  updateMessage: (id: string, updates: Partial<Message>) => void
  setStreaming: (isStreaming: boolean) => void
  setStreamingContent: (content: string) => void
  appendStreamingContent: (delta: string) => void
  addToolExecution: (execution: ToolExecution) => void
  updateToolExecution: (toolId: string, updates: Partial<ToolExecution>) => void
  clearToolExecutions: () => void
}

export const useChatStore = create<ChatState>((set) => ({
  conversations: [],
  activeConversationId: null,
  messages: [],
  isStreaming: false,
  streamingContent: '',
  toolExecutions: [],

  setConversations: (conversations) => set({ conversations }),

  setActiveConversation: (id) => set({ activeConversationId: id }),

  setMessages: (messages) => set({ messages }),

  addMessage: (message) =>
    set((state) => ({ messages: [...state.messages, message] })),

  updateMessage: (id, updates) =>
    set((state) => ({
      messages: state.messages.map((m) =>
        m.id === id ? { ...m, ...updates } : m,
      ),
    })),

  setStreaming: (isStreaming) => set({ isStreaming }),

  setStreamingContent: (content) => set({ streamingContent: content }),

  appendStreamingContent: (delta) =>
    set((state) => ({
      streamingContent: state.streamingContent + delta,
    })),

  addToolExecution: (execution) =>
    set((state) => ({
      toolExecutions: [...state.toolExecutions, execution],
    })),

  updateToolExecution: (toolId, updates) =>
    set((state) => ({
      toolExecutions: state.toolExecutions.map((t) =>
        t.toolId === toolId ? { ...t, ...updates } : t,
      ),
    })),

  clearToolExecutions: () => set({ toolExecutions: [] }),
}))
