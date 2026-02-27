/**
 * ConfirmBlock — human-in-the-loop confirmation card with accent left border.
 * Based on visual-prototype-zh.html .confirm styles.
 */
import type { ConfirmBlock as ConfirmBlockType } from '@/types/message'
import { Button } from '@/components/common/Button'

interface ConfirmBlockProps {
  confirm: ConfirmBlockType
  onConfirm?: (action: string) => void
  onReject?: (action: string) => void
}

export function ConfirmBlock({ confirm, onConfirm, onReject }: ConfirmBlockProps) {
  const isPending = confirm.status === 'pending'

  return (
    <div
      className="my-3 rounded-lg border border-l-[3px] p-4"
      style={{
        background: isPending
          ? 'var(--color-accent-bg-light)'
          : 'var(--color-bg-card)',
        borderColor: isPending
          ? 'var(--color-accent-border)'
          : 'var(--color-border)',
        borderLeftColor: isPending
          ? 'var(--color-accent)'
          : confirm.status === 'confirmed'
            ? 'var(--color-semantic-green)'
            : 'var(--color-semantic-red)',
      }}
    >
      {/* Title */}
      <div
        className="mb-2 flex items-center gap-1.5 text-base font-semibold"
        style={{ color: 'var(--color-accent)' }}
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
          <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-2 15l-5-5 1.41-1.41L10 14.17l7.59-7.59L19 8l-9 9z" />
        </svg>
        {confirm.title}
      </div>

      {/* Action buttons */}
      {isPending ? (
        <div className="flex items-center gap-2">
          <Button
            variant="primary"
            onClick={() => onConfirm?.(confirm.primaryAction)}
          >
            {confirm.primaryLabel}
          </Button>
          {confirm.secondaryLabel && confirm.secondaryAction && (
            <Button
              variant="secondary"
              onClick={() => onReject?.(confirm.secondaryAction!)}
            >
              {confirm.secondaryLabel}
            </Button>
          )}
        </div>
      ) : (
        <div
          className="text-sm font-medium"
          style={{
            color:
              confirm.status === 'confirmed'
                ? 'var(--color-semantic-green)'
                : 'var(--color-semantic-red)',
          }}
        >
          {confirm.status === 'confirmed' ? 'Confirmed' : 'Rejected'}
        </div>
      )}
    </div>
  )
}
