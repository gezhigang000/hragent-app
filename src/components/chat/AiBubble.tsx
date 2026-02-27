/**
 * AiBubble — AI message that renders MessageContent fields
 * in the fixed MESSAGE_CONTENT_RENDER_ORDER.
 * Based on visual-prototype-zh.html .msg-body styles.
 */
import type {
  Message,
  MessageContent,
  CodeBlock,
  DataTable,
  MetricCard,
  OptionGroup,
  AnomalyItem,
  InsightBlock as InsightBlockType,
  RootCauseBlock as RootCauseBlockType,
  ConfirmBlock as ConfirmBlockType,
  ProgressState,
  SearchSource,
  ExecSummary,
  ReportCard,
  GeneratedFile,
} from '@/types/message'
import { MESSAGE_CONTENT_RENDER_ORDER } from '@/types/message'
import { Avatar } from '@/components/common/Avatar'
import {
  RichCodeBlock,
  RichDataTable,
  MetricCards,
  OptionCards,
  AnomalyList,
  InsightBlock,
  RootCauseBlock,
  ConfirmBlock,
  ProgressSteps,
  SearchSourceBlock,
  ExecSummaryCard,
  ReportCards,
  GeneratedFileCard,
} from '@/components/rich-content'
import { TypingIndicator } from './TypingIndicator'
import { useChatStore } from '@/stores/chatStore'
import { sendMessage } from '@/lib/tauri'
import { openGeneratedFile } from '@/lib/tauri'
import { useCallback } from 'react'
import { markdownToHtml } from '@/lib/markdown'

interface AiBubbleProps {
  message: Message
  isStreaming?: boolean
}

export function AiBubble({ message, isStreaming }: AiBubbleProps) {
  const { content } = message
  const conversationId = useChatStore((s) => s.activeConversationId)

  /** Send a user choice back to the agent loop as a message. */
  const handleUserResponse = useCallback(
    (responseText: string) => {
      if (!conversationId) return
      sendMessage(conversationId, responseText).catch((err) =>
        console.error('[AiBubble] Failed to send user response:', err),
      )
    },
    [conversationId],
  )

  /** Open a generated report file via system default app. */
  const handleOpenFile = useCallback((fileId: string) => {
    if (!conversationId) return
    openGeneratedFile(fileId, conversationId).catch((err) =>
      console.error('[AiBubble] Failed to open file:', err),
    )
  }, [conversationId])

  return (
    <div className="mb-7 animate-[fadeUp_0.3s_ease]">
      {/* Header: avatar + name */}
      <div className="mb-2 flex items-center gap-2">
        <Avatar variant="ai" />
        <span
          className="text-sm font-semibold"
          style={{ color: 'var(--color-text-primary)' }}
        >
          AI小家
        </span>
      </div>

      {/* Body — offset by avatar width */}
      <div style={{ paddingLeft: '36px' }}>
        {MESSAGE_CONTENT_RENDER_ORDER.map((field) => {
          const value = content[field]
          if (value === undefined || value === null) return null
          return (
            <ContentRenderer
              key={field}
              field={field}
              value={value}
              content={content}
              onUserResponse={handleUserResponse}
              onOpenFile={handleOpenFile}
            />
          )
        })}

        {isStreaming && <TypingIndicator />}
      </div>
    </div>
  )
}

/**
 * ContentRenderer dispatches each MessageContent field to
 * the appropriate rich content component.
 */
function ContentRenderer({
  field,
  value,
  content,
  onUserResponse,
  onOpenFile,
}: {
  field: keyof MessageContent
  value: NonNullable<MessageContent[keyof MessageContent]>
  content: MessageContent
  onUserResponse: (text: string) => void
  onOpenFile: (fileId: string) => void
}) {
  switch (field) {
    case 'text':
      return <TextRenderer text={value as string} />

    case 'progress':
      return <ProgressSteps progress={value as ProgressState} />

    case 'codeBlocks':
      return (
        <>
          {(value as CodeBlock[]).map((block) => (
            <RichCodeBlock
              key={block.id}
              block={block}
              result={content.codeResults?.find((r) => r.codeBlockId === block.id)}
            />
          ))}
        </>
      )

    case 'codeResults':
      // Rendered inline with codeBlocks above
      return null

    case 'tables':
      return (
        <>
          {(value as DataTable[]).map((table) => (
            <RichDataTable key={table.id} table={table} />
          ))}
        </>
      )

    case 'metrics':
      return <MetricCards metrics={value as MetricCard[]} />

    case 'options':
      return (
        <>
          {(value as OptionGroup[]).map((group) => (
            <OptionCards
              key={group.id}
              group={group}
              onSelect={(optionId) => {
                const opt = group.options.find((o) => o.id === optionId)
                if (opt) onUserResponse(`[选择] ${opt.title}`)
              }}
            />
          ))}
        </>
      )

    case 'anomalies':
      return <AnomalyList anomalies={value as AnomalyItem[]} />

    case 'insights':
      return (
        <>
          {(value as InsightBlockType[]).map((insight) => (
            <InsightBlock key={insight.id} insight={insight} />
          ))}
        </>
      )

    case 'rootCauses':
      return (
        <>
          {(value as RootCauseBlockType[]).map((rc) => (
            <RootCauseBlock key={rc.id} rootCause={rc} />
          ))}
        </>
      )

    case 'generatedFiles':
      return (
        <>
          {(value as GeneratedFile[]).map((file) => (
            <GeneratedFileCard key={file.id} file={file} />
          ))}
        </>
      )

    case 'reports':
      return (
        <ReportCards
          reports={value as ReportCard[]}
          onOpen={(reportId) => onOpenFile(reportId)}
        />
      )

    case 'searchSources':
      return (
        <>
          {(value as SearchSource[]).map((source) => (
            <SearchSourceBlock key={source.id} source={source} />
          ))}
        </>
      )

    case 'execSummary':
      return <ExecSummaryCard summary={value as ExecSummary} />

    case 'confirmations':
      return (
        <>
          {(value as ConfirmBlockType[]).map((c) => (
            <ConfirmBlock
              key={c.id}
              confirm={c}
              onConfirm={(action) => onUserResponse(`[确认] ${action}`)}
              onReject={(action) => onUserResponse(`[拒绝] ${action}`)}
            />
          ))}
        </>
      )

    default:
      return null
  }
}

/** Renders text content with full markdown support (tables, headings, lists, code). */
function TextRenderer({ text }: { text: string }) {
  return (
    <div
      className="text-md leading-relaxed"
      dangerouslySetInnerHTML={{ __html: markdownToHtml(text) }}
    />
  )
}
