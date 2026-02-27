/**
 * RootCauseBlock — red-tinted root cause analysis card.
 * Based on visual-prototype-zh.html .rootcause styles.
 */
import type { RootCauseBlock as RootCauseBlockType } from '@/types/message'

interface RootCauseBlockProps {
  rootCause: RootCauseBlockType
}

export function RootCauseBlock({ rootCause }: RootCauseBlockProps) {
  return (
    <div
      className="my-3 rounded-lg border p-4"
      style={{
        background: 'var(--color-semantic-red-bg-light)',
        borderColor: 'var(--color-semantic-red-border)',
      }}
    >
      {/* Title */}
      <div
        className="mb-2.5 flex items-center gap-1.5 text-base font-semibold"
        style={{ color: 'var(--color-semantic-red)' }}
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
          <path d="M1 21h22L12 2 1 21zm12-3h-2v-2h2v2zm0-4h-2v-4h2v4z" />
        </svg>
        {rootCause.title}
      </div>

      {/* Items */}
      {rootCause.items.map((item, idx) => (
        <div
          key={idx}
          className="py-2.5"
          style={{
            borderBottom:
              idx < rootCause.items.length - 1
                ? '1px solid rgba(0,0,0,0.06)'
                : 'none',
          }}
        >
          <div className="mb-1 flex items-center gap-2">
            <span
              className="rounded-xl px-2 py-0.5 text-xs font-bold"
              style={{
                background: 'rgba(239,68,68,0.12)',
                color: 'var(--color-semantic-red)',
              }}
            >
              {item.count}
            </span>
            <span
              className="text-sm font-semibold"
              style={{ color: 'var(--color-text-primary)' }}
            >
              {item.label}
            </span>
          </div>
          <div
            className="mt-1 text-sm leading-snug"
            style={{ color: 'var(--color-text-muted)' }}
          >
            {item.detail}
          </div>
          <div
            className="mt-1 text-sm font-medium"
            style={{ color: 'var(--color-accent)' }}
          >
            {item.action}
          </div>
        </div>
      ))}
    </div>
  )
}
