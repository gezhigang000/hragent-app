/**
 * Avatar — AI gold circle or User purple circle.
 * Based on visual-prototype-zh.html .av styles.
 */

interface AvatarProps {
  variant: 'ai' | 'user'
  label?: string
}

export function Avatar({ variant, label }: AvatarProps) {
  const isAI = variant === 'ai'

  return (
    <div
      className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-sm font-bold"
      style={{
        background: isAI ? 'var(--color-accent)' : 'var(--color-user-avatar)',
        color: isAI ? 'var(--color-text-on-accent)' : '#fff',
      }}
    >
      {label ?? (isAI ? '家' : 'H')}
    </div>
  )
}
