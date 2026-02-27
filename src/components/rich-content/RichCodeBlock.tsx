/**
 * RichCodeBlock — syntax-highlighted code block with header.
 * Based on visual-prototype-zh.html .code-block styles.
 */
import type { CodeBlock, CodeResult } from '@/types/message'

interface RichCodeBlockProps {
  block: CodeBlock
  result?: CodeResult
}

const STATUS_INDICATOR: Record<CodeBlock['status'], { label: string; color: string }> = {
  pending: { label: 'Pending', color: 'var(--color-text-muted)' },
  running: { label: 'Running...', color: 'var(--color-semantic-blue)' },
  success: { label: 'Done', color: 'var(--color-semantic-green)' },
  error: { label: 'Error', color: 'var(--color-semantic-red)' },
}

export function RichCodeBlock({ block, result }: RichCodeBlockProps) {
  const status = STATUS_INDICATOR[block.status]

  return (
    <div
      className="my-3 overflow-hidden rounded-lg border"
      style={{
        background: 'var(--color-bg-code)',
        borderColor: 'var(--color-border)',
      }}
    >
      {/* Header */}
      <div
        className="flex items-center justify-between border-b px-3.5 py-2"
        style={{
          background: 'rgba(0,0,0,0.02)',
          borderColor: 'var(--color-border)',
        }}
      >
        <span
          className="flex items-center gap-1.5 text-xs font-semibold"
          style={{ color: 'var(--color-text-muted)' }}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="var(--color-semantic-green)">
            <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-2 15l-5-5 1.41-1.41L10 14.17l7.59-7.59L19 8l-9 9z" />
          </svg>
          {block.language}
          {block.purpose && <span className="font-normal"> — {block.purpose}</span>}
        </span>
        <span className="text-xs font-medium" style={{ color: status.color }}>
          {status.label}
        </span>
      </div>

      {/* Code body */}
      <pre
        className="overflow-x-auto whitespace-pre px-3.5 py-3 font-mono text-sm leading-[1.65]"
        style={{ color: 'var(--color-text-code)' }}
      >
        {block.code}
      </pre>

      {/* Result output */}
      {result && (
        <div
          className="border-t px-3.5 py-2.5 font-mono text-sm leading-[1.6]"
          style={{
            borderColor: 'var(--color-border)',
            background: result.isError ? 'var(--color-semantic-red-bg-light)' : 'var(--color-semantic-green-bg-light)',
            color: result.isError ? 'var(--color-semantic-red)' : 'var(--color-text-secondary)',
          }}
        >
          <pre className="whitespace-pre-wrap">{result.output}</pre>
        </div>
      )}
    </div>
  )
}
