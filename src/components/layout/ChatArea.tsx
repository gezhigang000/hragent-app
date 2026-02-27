/**
 * ChatArea — scrollable message container with auto-scroll.
 * Based on visual-prototype-zh.html chat-area section.
 */
import { useEffect, useRef } from 'react'
import { useChatStore } from '@/stores/chatStore'
import { MessageList } from '@/components/chat/MessageList'

export function ChatArea() {
  const messages = useChatStore((s) => s.messages)
  const isStreaming = useChatStore((s) => s.isStreaming)
  const streamingContent = useChatStore((s) => s.streamingContent)
  const bottomRef = useRef<HTMLDivElement>(null)

  // Auto-scroll when messages change or streaming content updates
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages.length, isStreaming, streamingContent])

  return (
    <div
      className="flex-1 overflow-y-auto"
      style={{ background: 'var(--color-bg-main)' }}
    >
      <div className="mx-auto max-w-[860px] px-6 pt-6 pb-40">
        {messages.length === 0 ? <WelcomeMessage /> : <MessageList />}
        <div ref={bottomRef} />
      </div>
    </div>
  )
}

function WelcomeMessage() {
  return (
    <div className="animate-[fadeUp_0.3s_ease]">
      <div className="mb-2 flex items-center gap-2">
        <div
          className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-sm font-bold"
          style={{
            background: 'var(--color-accent)',
            color: 'var(--color-text-on-accent)',
          }}
        >
          家
        </div>
        <span
          className="text-base font-semibold"
          style={{ color: 'var(--color-text-primary)' }}
        >
          AI小家
        </span>
      </div>
      <div className="pl-9">
        <p
          className="mb-2.5 text-md leading-relaxed"
          style={{ color: 'var(--color-text-secondary)' }}
        >
          你好，我是{' '}
          <strong style={{ color: 'var(--color-text-primary)' }}>AI小家</strong>{' '}
          — 你的组织咨询专家。
        </p>
        <p
          className="text-md leading-relaxed"
          style={{ color: 'var(--color-text-secondary)' }}
        >
          有什么可以帮你的？你可以直接问我任何薪酬和组织问题，也可以上传文件（Excel/Word/PDF）让我帮你分析。
        </p>
      </div>
    </div>
  )
}
