/**
 * useStreaming — Listens to Tauri streaming events and pipes them
 * into the chat store.
 *
 * This hook should be mounted once at a high level (e.g. in the main
 * chat layout) so that streaming events are processed for the lifetime
 * of the application.
 *
 * Events handled:
 *  - streaming:delta  — appends token content to the streaming buffer
 *  - streaming:done   — finalises the streamed message
 *  - streaming:error  — surfaces the error to the user
 *  - message:updated  — upserts the full message object in the store
 */
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import {
  onStreamingDelta,
  onStreamingDone,
  onStreamingError,
  onMessageUpdated,
  onToolExecuting,
  onToolCompleted,
  onAnalysisStepChanged,
} from '@/lib/tauri'
import { useAnalysisStore } from '@/stores/analysisStore'
import type { StepStatus } from '@/types/analysis'
import { useTauriEvent } from './useTauriEvent'

/**
 * Registers all streaming-related Tauri event listeners.
 *
 * Call this hook once in a top-level component. It does not return
 * anything — all side-effects flow through the Zustand stores.
 */
export function useStreaming() {
  // NOTE: We intentionally do NOT destructure store state here.
  // The Tauri event callbacks below are registered once via useTauriEvent([]),
  // so any captured references would be stale. Instead, we call getState()
  // inside each callback to always access fresh store state.

  // --- streaming:delta -------------------------------------------------
  useTauriEvent(() =>
    onStreamingDelta(({ delta }: { delta: string }) => {
      console.log('[streaming:delta]', delta.slice(0, 80))
      useChatStore.getState().appendStreamingContent(delta)
    }),
  )

  // --- streaming:done --------------------------------------------------
  useTauriEvent(() =>
    onStreamingDone(({ messageId }: { messageId: string }) => {
      console.log('[streaming:done] messageId:', messageId)
      const store = useChatStore.getState()
      store.setStreaming(false)
      store.setStreamingContent('')
      store.clearToolExecutions()
    }),
  )

  // --- streaming:error -------------------------------------------------
  useTauriEvent(() =>
    onStreamingError(({ error }: { error: string }) => {
      console.error('[streaming:error]', error)
      const store = useChatStore.getState()
      store.setStreaming(false)
      store.setStreamingContent('')
      store.clearToolExecutions()

      useNotificationStore.getState().push({
        level: 'error',
        title: 'Streaming Error',
        message: error ?? 'An unknown error occurred while streaming the response.',
        actions: [],
        dismissible: true,
        autoHide: 8,
        context: 'toast',
      })
    }),
  )

  // --- message:updated -------------------------------------------------
  useTauriEvent(() =>
    onMessageUpdated((message) => {
      console.log('[message:updated] id:', message.id, 'role:', message.role)
      const store = useChatStore.getState()
      const exists = store.messages.some((m) => m.id === message.id)
      if (exists) {
        store.updateMessage(message.id, message)
      } else {
        store.addMessage(message)
      }
    }),
  )

  // --- tool:executing ---------------------------------------------------
  useTauriEvent(() =>
    onToolExecuting(({ toolName, toolId, purpose }) => {
      console.log('[tool:executing]', toolName, toolId, purpose)
      useChatStore.getState().addToolExecution({ toolName, toolId, status: 'executing', summary: purpose })
    }),
  )

  // --- tool:completed ---------------------------------------------------
  useTauriEvent(() =>
    onToolCompleted(({ toolId, success, summary }) => {
      console.log('[tool:completed]', toolId, success, summary)
      useChatStore.getState().updateToolExecution(toolId, {
        status: success ? 'completed' : 'error',
        summary,
      })
    }),
  )

  // --- analysis:step-changed --------------------------------------------
  useTauriEvent(() =>
    onAnalysisStepChanged(({ step, status }) => {
      console.log('[analysis:step-changed]', step, status)
      const store = useAnalysisStore.getState()
      store.setCurrentStep(step)
      store.setStepStatus(step, status as StepStatus)
    }),
  )
}
