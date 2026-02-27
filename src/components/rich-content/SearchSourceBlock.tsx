/**
 * SearchSourceBlock — purple-tinted search source references.
 * Based on visual-prototype-zh.html .search-source styles.
 */
import type { SearchSource } from '@/types/message'

interface SearchSourceBlockProps {
  source: SearchSource
}

export function SearchSourceBlock({ source }: SearchSourceBlockProps) {
  return (
    <div
      className="my-3 rounded-lg border p-3.5 text-sm"
      style={{
        background: 'var(--color-semantic-purple-bg-light)',
        borderColor: 'var(--color-semantic-purple-border)',
      }}
    >
      <div
        className="mb-1.5 flex items-center gap-1.5 font-semibold"
        style={{ color: 'var(--color-semantic-purple)' }}
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
          <path d="M15.5 14h-.79l-.28-.27C15.41 12.59 16 11.11 16 9.5 16 5.91 13.09 3 9.5 3S3 5.91 3 9.5 5.91 16 9.5 16c1.61 0 3.09-.59 4.23-1.57l.27.28v.79l5 4.99L20.49 19l-4.99-5zm-6 0C7.01 14 5 11.99 5 9.5S7.01 5 9.5 5 14 7.01 14 9.5 11.99 14 9.5 14z" />
        </svg>
        {source.title}
      </div>

      {source.items.map((item, idx) => (
        <div
          key={idx}
          className="mb-0.5 leading-snug"
          style={{ color: 'var(--color-text-muted)' }}
        >
          <span className="font-medium" style={{ color: 'var(--color-text-secondary)' }}>
            {item.source}
          </span>
          {' — '}
          {item.snippet}
          {item.url && (
            <>
              {' '}
              <a
                href={item.url}
                target="_blank"
                rel="noopener noreferrer"
                className="underline"
                style={{ color: 'var(--color-semantic-purple)' }}
              >
                link
              </a>
            </>
          )}
        </div>
      ))}

      {source.warning && (
        <div
          className="mt-1.5 text-xs italic"
          style={{ color: 'var(--color-semantic-orange)' }}
        >
          {source.warning}
        </div>
      )}
    </div>
  )
}
