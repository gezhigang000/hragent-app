/**
 * ProgressSteps — inline analysis progress bar with step labels.
 * Based on visual-prototype-zh.html .analysis-progress styles.
 */
import type { ProgressState } from '@/types/message'

interface ProgressStepsProps {
  progress: ProgressState
}

export function ProgressSteps({ progress }: ProgressStepsProps) {
  return (
    <div
      className="my-3 rounded-lg border p-3.5"
      style={{
        background: 'var(--color-bg-card)',
        borderColor: 'var(--color-border)',
      }}
    >
      {/* Title */}
      <div className="mb-2.5 flex items-center gap-2">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="var(--color-accent)">
          <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-2 15l-5-5 1.41-1.41L10 14.17l7.59-7.59L19 8l-9 9z" />
        </svg>
        <span
          className="text-sm font-semibold"
          style={{ color: 'var(--color-text-primary)' }}
        >
          {progress.title}
        </span>
      </div>

      {/* Step pills */}
      <div className="flex flex-wrap gap-1.5">
        {progress.steps.map((step, i) => {
          const isDone = step.status === 'done'
          const isActive = step.status === 'active'

          return (
            <span
              key={i}
              className="flex items-center gap-1 rounded-xl px-2.5 py-1 text-xs font-medium"
              style={{
                background: isDone
                  ? 'rgba(212,168,67,0.12)'
                  : isActive
                    ? 'rgba(91,155,213,0.12)'
                    : 'rgba(168,168,168,0.1)',
                color: isDone
                  ? 'var(--color-accent)'
                  : isActive
                    ? 'var(--color-semantic-blue)'
                    : 'var(--color-text-muted)',
              }}
            >
              {isDone && (
                <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z" />
                </svg>
              )}
              {isActive && (
                <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M12 6v6l4 2" stroke="currentColor" strokeWidth="2" fill="none" />
                </svg>
              )}
              {step.label}
            </span>
          )
        })}
      </div>
    </div>
  )
}
