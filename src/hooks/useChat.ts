/**
 * useChat — High-level chat actions connecting the Zustand store to
 * the Tauri IPC layer.
 *
 * Provides conversation CRUD, message sending, and streaming control.
 *
 * IMPORTANT: All callbacks use useChatStore.getState() to read the latest
 * state inside the callback, rather than capturing the `store` snapshot
 * from render time. This keeps dependencies stable ([]) and avoids
 * infinite re-render loops.
 */
import { useCallback } from 'react'
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import {
  sendMessage,
  stopStreaming,
  getMessages,
  createConversation,
  deleteConversation,
  getConversations,
} from '@/lib/tauri'
import type { Conversation, Message } from '@/types/message'

/** Generate a unique ID without requiring the `uuid` package. */
function generateId(): string {
  return crypto.randomUUID?.() ?? `${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 9)}`
}

/** File info passed from InputBar to sendUserMessage. */
export interface PendingFileInfo {
  id: string
  fileName: string
  fileType: 'excel' | 'csv' | 'word' | 'pdf' | 'json'
  fileSize: number
}

/**
 * Hook that exposes every chat-related action the UI needs.
 *
 * All functions use stable `useCallback(fn, [])` — they read fresh state
 * via `useChatStore.getState()` inside the callback body.
 */
export function useChat() {
  // Subscribe to state slices for re-rendering
  const conversations = useChatStore((s) => s.conversations)
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const messages = useChatStore((s) => s.messages)
  const isStreaming = useChatStore((s) => s.isStreaming)
  const streamingContent = useChatStore((s) => s.streamingContent)

  /**
   * Create a brand-new conversation and make it active.
   */
  const createNewConversation = useCallback(async () => {
    const store = useChatStore.getState()
    const optimisticId = generateId()
    const now = new Date().toISOString()

    const conversation: Conversation = {
      id: optimisticId,
      title: 'New Conversation',
      createdAt: now,
      updatedAt: now,
      isArchived: false,
    }

    // Optimistic store update
    store.setConversations([conversation, ...store.conversations])
    store.setActiveConversation(optimisticId)
    store.setMessages([])

    try {
      const backendId = await createConversation()
      console.log('[useChat] createConversation OK, backendId:', backendId)

      // Replace optimistic ID with the backend-generated ID
      if (backendId && backendId !== optimisticId) {
        const current = useChatStore.getState()
        current.setConversations(
          current.conversations.map((c) =>
            c.id === optimisticId ? { ...c, id: backendId } : c,
          ),
        )
        current.setActiveConversation(backendId)
        return backendId
      }
    } catch (err) {
      console.error('[useChat] createConversation IPC failed:', err)
      // Rollback
      const current = useChatStore.getState()
      current.setConversations(current.conversations.filter((c) => c.id !== optimisticId))
      current.setActiveConversation(null)
    }

    return optimisticId
  }, [])

  /**
   * Delete a conversation by id.
   */
  const removeConversation = useCallback(async (id: string) => {
    console.log('[useChat] deleteConversation called, id:', id)
    const store = useChatStore.getState()

    store.setConversations(store.conversations.filter((c) => c.id !== id))

    if (store.activeConversationId === id) {
      store.setActiveConversation(null)
      store.setMessages([])
    }

    try {
      await deleteConversation(id)
      console.log('[useChat] deleteConversation IPC succeeded')
    } catch (err) {
      console.error('[useChat] deleteConversation IPC failed:', err)
      // Rollback: reload conversations from backend
      try {
        const raw = await getConversations()
        const convs: Conversation[] = raw.map((c) => ({
          id: (c.id as string) ?? '',
          title: (c.title as string) ?? 'New Conversation',
          createdAt: (c.createdAt as string) ?? new Date().toISOString(),
          updatedAt: (c.updatedAt as string) ?? new Date().toISOString(),
          isArchived: (c.isArchived as boolean) ?? false,
        }))
        useChatStore.getState().setConversations(convs)
      } catch {
        // If re-fetch also fails, nothing more we can do
      }
    }
  }, [])

  /**
   * Switch the active conversation and load its messages from the backend.
   */
  const switchConversation = useCallback(async (id: string) => {
    console.log('[useChat] switchConversation, id:', id)
    const store = useChatStore.getState()
    store.setActiveConversation(id)
    store.setMessages([])
    store.setStreamingContent('')
    store.setStreaming(false)

    try {
      const msgs = await getMessages(id)
      console.log('[useChat] getMessages OK, count:', msgs.length)
      useChatStore.getState().setMessages(msgs)
    } catch (err) {
      console.error('[useChat] getMessages IPC failed:', err)
    }
  }, [])

  /**
   * Send a user message in the currently active conversation.
   *
   * @param text  - The user's plain-text input.
   * @param files - Optional list of attached file info objects.
   */
  const sendUserMessage = useCallback(async (text: string, files?: PendingFileInfo[]) => {
    let store = useChatStore.getState()
    let conversationId = store.activeConversationId
    console.log('[useChat] sendUserMessage, conversationId:', conversationId, 'text:', text.slice(0, 50))

    // Auto-create a conversation if none is active
    if (!conversationId) {
      try {
        const backendId = await createConversation()
        console.log('[useChat] Auto-created conversation:', backendId)
        const now = new Date().toISOString()
        store = useChatStore.getState()
        store.setConversations([
          { id: backendId, title: 'New Conversation', createdAt: now, updatedAt: now, isArchived: false },
          ...store.conversations,
        ])
        store.setActiveConversation(backendId)
        store.setMessages([])
        conversationId = backendId
      } catch (err) {
        console.error('[useChat] Failed to auto-create conversation:', err)
        return
      }
    }

    const messageId = generateId()
    const now = new Date().toISOString()

    // Build the optimistic user message
    const userMessage: Message = {
      id: messageId,
      conversationId,
      role: 'user',
      createdAt: now,
      content: {
        text,
        files: files?.map((f) => ({
          id: f.id,
          fileName: f.fileName,
          fileSize: f.fileSize,
          fileType: f.fileType,
          status: 'uploaded' as const,
        })),
      },
    }

    store = useChatStore.getState()
    store.addMessage(userMessage)
    store.setStreaming(true)
    store.setStreamingContent('')

    try {
      const fileIds = files?.map((f) => f.id)
      console.log('[useChat] Calling sendMessage IPC, fileIds:', fileIds)
      await sendMessage(conversationId, text, fileIds)
      console.log('[useChat] sendMessage IPC returned OK')
    } catch (err) {
      console.error('[useChat] sendMessage IPC failed:', err)
      const s = useChatStore.getState()
      s.setStreaming(false)
      s.setStreamingContent('')
      // Show error toast so user knows the message failed
      useNotificationStore.getState().push({
        level: 'error',
        title: '发送失败',
        message: String(err) || '消息发送失败，请检查网络和设置。',
        actions: [],
        dismissible: true,
        autoHide: 8,
        context: 'toast',
      })
    }
  }, [])

  /**
   * Stop the currently active streaming response.
   */
  const stopCurrentStream = useCallback(() => {
    console.log('[useChat] stopCurrentStream')
    const store = useChatStore.getState()
    store.setStreaming(false)
    store.setStreamingContent('')

    stopStreaming().catch((err) => {
      console.error('[useChat] stopStreaming IPC failed:', err)
    })
  }, [])

  /**
   * Load the initial list of conversations from the backend.
   */
  const loadConversations = useCallback(async () => {
    console.log('[useChat] loadConversations')
    try {
      const raw = await getConversations()
      const convs: Conversation[] = raw.map((c) => ({
        id: (c.id as string) ?? '',
        title: (c.title as string) ?? 'New Conversation',
        createdAt: (c.createdAt as string) ?? new Date().toISOString(),
        updatedAt: (c.updatedAt as string) ?? new Date().toISOString(),
        isArchived: (c.isArchived as boolean) ?? false,
      }))
      console.log('[useChat] loadConversations OK, count:', convs.length)
      useChatStore.getState().setConversations(convs)
    } catch (err) {
      console.error('[useChat] getConversations IPC failed:', err)
    }
  }, [])

  return {
    // State (subscribed for re-rendering)
    conversations,
    activeConversationId,
    messages,
    isStreaming,
    streamingContent,

    // Actions (stable references)
    createNewConversation,
    deleteConversation: removeConversation,
    switchConversation,
    sendUserMessage,
    stopCurrentStream,
    loadConversations,
  }
}
