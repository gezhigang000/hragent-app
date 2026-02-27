/**
 * StreamingBubble — shows the AI response as it streams in,
 * with a typing indicator when waiting for the first token.
 */
import { Avatar } from '@/components/common/Avatar'
import { useChatStore } from '@/stores/chatStore'
import { TypingIndicator } from './TypingIndicator'
import { markdownToHtml } from '@/lib/markdown'

const TOOL_LABELS: Record<string, string> = {
  execute_python: '执行 Python 代码',
  analyze_file: '解析文件',
  save_analysis_note: '保存分析结果',
  update_progress: '更新进度',
  web_search: '联网搜索',
  generate_report: '生成报告',
  export_data: '导出数据',
  hypothesis_test: '假设检验',
  detect_anomalies: '异常检测',
  generate_chart: '生成图表',
}

interface StreamingBubbleProps {
  content: string
}

export function StreamingBubble({ content }: StreamingBubbleProps) {
  const toolExecutions = useChatStore((s) => s.toolExecutions)
  const activeTool = toolExecutions.find((t) => t.status === 'executing')

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
        {content ? (
          <div
            className="text-md leading-relaxed"
            dangerouslySetInnerHTML={{ __html: markdownToHtml(content) }}
          />
        ) : null}
        {activeTool ? (
          <div
            className="mt-2 flex items-center gap-2 text-xs"
            style={{ color: 'var(--color-text-muted)' }}
          >
            <svg className="h-3.5 w-3.5 animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
              <circle cx="12" cy="12" r="10" strokeDasharray="50" strokeDashoffset="20" strokeLinecap="round" />
            </svg>
            <span>{TOOL_LABELS[activeTool.toolName] || activeTool.toolName}</span>
          </div>
        ) : (
          <TypingIndicator />
        )}
      </div>
    </div>
  )
}
