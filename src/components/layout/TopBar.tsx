/**
 * TopBar — title, LLM provider badge with dropdown switcher, network indicator.
 * Based on visual-prototype-zh.html top-bar section.
 */
import { useEffect, useRef, useState } from 'react'
import { useChatStore } from '@/stores/chatStore'
import { useSettingsStore } from '@/stores/settingsStore'
import { LLM_PROVIDER_LABELS } from '@/types/settings'
import type { LlmProvider } from '@/types/settings'
import { getConfiguredProviders, switchProvider, getSettings } from '@/lib/tauri'

interface TopBarProps {
  onOpenSettings: () => void
}

export function TopBar({ onOpenSettings }: TopBarProps) {
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const conversations = useChatStore((s) => s.conversations)
  const primaryModel = useSettingsStore((s) => s.primaryModel)
  const configuredProviders = useSettingsStore((s) => s.configuredProviders)
  const [dropdownOpen, setDropdownOpen] = useState(false)
  const dropdownRef = useRef<HTMLDivElement>(null)

  const activeConversation = conversations.find(
    (c) => c.id === activeConversationId,
  )
  const title = activeConversation?.title ?? 'AI小家'

  // Load configured providers when primaryModel changes
  useEffect(() => {
    ;(async () => {
      try {
        const providers = await getConfiguredProviders()
        useSettingsStore.getState().setConfiguredProviders(providers as LlmProvider[])
      } catch (err) {
        console.error('Failed to load configured providers:', err)
      }
    })()
  }, [primaryModel])

  // Click-outside to close dropdown
  useEffect(() => {
    if (!dropdownOpen) return
    const handleMouseDown = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setDropdownOpen(false)
      }
    }
    document.addEventListener('mousedown', handleMouseDown)
    return () => document.removeEventListener('mousedown', handleMouseDown)
  }, [dropdownOpen])

  const handleBadgeClick = () => {
    if (configuredProviders.length > 1) {
      setDropdownOpen((prev) => !prev)
    } else {
      onOpenSettings()
    }
  }

  const handleSwitchProvider = async (provider: LlmProvider) => {
    setDropdownOpen(false)
    if (provider === primaryModel) return
    try {
      await switchProvider(provider)
      // Reload settings from backend to reflect the switch
      const updated = await getSettings()
      useSettingsStore.getState().setSettings(updated)
    } catch (err) {
      console.error('Failed to switch provider:', err)
    }
  }

  return (
    <header
      className="flex h-[52px] shrink-0 items-center border-b px-6"
      style={{ borderColor: 'var(--color-border)' }}
    >
      <h2
        className="text-lg font-semibold"
        style={{ color: 'var(--color-text-primary)' }}
      >
        {title}
      </h2>

      <div className="ml-auto flex items-center gap-2">
        <div className="relative" ref={dropdownRef}>
          <button
            className="flex cursor-pointer items-center gap-1 rounded-xl border py-[3px] px-2.5 transition-all duration-150"
            style={{
              fontSize: 'var(--text-xs)',
              background: 'var(--color-bg-card)',
              borderColor: 'var(--color-border)',
              color: 'var(--color-text-muted)',
            }}
            title="点击切换模型"
            onClick={handleBadgeClick}
          >
            <span
              className="h-1.5 w-1.5 rounded-full"
              style={{ background: 'var(--color-semantic-green)' }}
            />
            <span>{LLM_PROVIDER_LABELS[primaryModel]}</span>
            {configuredProviders.length > 1 && (
              <svg
                className="h-3 w-3 opacity-60"
                viewBox="0 0 20 20"
                fill="currentColor"
              >
                <path
                  fillRule="evenodd"
                  d="M5.23 7.21a.75.75 0 011.06.02L10 11.168l3.71-3.938a.75.75 0 111.08 1.04l-4.25 4.5a.75.75 0 01-1.08 0l-4.25-4.5a.75.75 0 01.02-1.06z"
                  clipRule="evenodd"
                />
              </svg>
            )}
          </button>

          {/* Dropdown menu */}
          {dropdownOpen && (
            <div
              className="absolute right-0 top-full z-50 mt-1 min-w-[180px] overflow-hidden rounded-lg border"
              style={{
                background: 'var(--color-bg-card)',
                borderColor: 'var(--color-border)',
                boxShadow: 'var(--shadow-modal)',
              }}
            >
              <div className="py-1">
                {configuredProviders.map((provider) => (
                  <button
                    key={provider}
                    className="flex w-full cursor-pointer items-center gap-2 border-none px-3 py-2 text-sm transition-colors duration-100"
                    style={{
                      background: 'transparent',
                      color: provider === primaryModel
                        ? 'var(--color-text-primary)'
                        : 'var(--color-text-secondary)',
                      fontWeight: provider === primaryModel ? 500 : 400,
                    }}
                    onMouseEnter={(e) => {
                      e.currentTarget.style.background = 'var(--color-bg-card-hover)'
                    }}
                    onMouseLeave={(e) => {
                      e.currentTarget.style.background = 'transparent'
                    }}
                    onClick={() => handleSwitchProvider(provider)}
                  >
                    <span
                      className="h-1.5 w-1.5 rounded-full"
                      style={{
                        background: provider === primaryModel
                          ? 'var(--color-semantic-green)'
                          : 'transparent',
                      }}
                    />
                    <span className="flex-1 text-left">
                      {LLM_PROVIDER_LABELS[provider] ?? provider}
                    </span>
                    {provider === primaryModel && (
                      <svg
                        className="h-3.5 w-3.5 shrink-0"
                        viewBox="0 0 20 20"
                        fill="currentColor"
                        style={{ color: 'var(--color-accent)' }}
                      >
                        <path
                          fillRule="evenodd"
                          d="M16.704 4.153a.75.75 0 01.143 1.052l-8 10.5a.75.75 0 01-1.127.075l-4.5-4.5a.75.75 0 011.06-1.06l3.894 3.893 7.48-9.817a.75.75 0 011.05-.143z"
                          clipRule="evenodd"
                        />
                      </svg>
                    )}
                  </button>
                ))}
              </div>

              {/* Divider + Settings entry */}
              <div
                className="border-t"
                style={{ borderColor: 'var(--color-border)' }}
              >
                <button
                  className="flex w-full cursor-pointer items-center gap-2 border-none px-3 py-2 text-sm transition-colors duration-100"
                  style={{
                    background: 'transparent',
                    color: 'var(--color-text-muted)',
                  }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.background = 'var(--color-bg-card-hover)'
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.background = 'transparent'
                  }}
                  onClick={() => {
                    setDropdownOpen(false)
                    onOpenSettings()
                  }}
                >
                  <svg
                    className="h-3.5 w-3.5 opacity-60"
                    viewBox="0 0 24 24"
                    fill="currentColor"
                  >
                    <path d="M19.14 12.94c.04-.3.06-.61.06-.94 0-.32-.02-.64-.07-.94l2.03-1.58a.49.49 0 00.12-.61l-1.92-3.32a.49.49 0 00-.59-.22l-2.39.96c-.5-.38-1.03-.7-1.62-.94l-.36-2.54a.484.484 0 00-.48-.41h-3.84c-.24 0-.43.17-.47.41l-.36 2.54c-.59.24-1.13.57-1.62.94l-2.39-.96c-.22-.08-.47 0-.59.22L2.74 8.87c-.12.21-.08.47.12.61l2.03 1.58c-.05.3-.07.62-.07.94s.02.64.07.94l-2.03 1.58a.49.49 0 00-.12.61l1.92 3.32c.12.22.37.29.59.22l2.39-.96c.5.38 1.03.7 1.62.94l.36 2.54c.05.24.24.41.48.41h3.84c.24 0 .44-.17.47-.41l.36-2.54c.59-.24 1.13-.56 1.62-.94l2.39.96c.22.08.47 0 .59-.22l1.92-3.32c.12-.22.07-.47-.12-.61l-2.01-1.58zM12 15.6c-1.98 0-3.6-1.62-3.6-3.6s1.62-3.6 3.6-3.6 3.6 1.62 3.6 3.6-1.62 3.6-3.6 3.6z" />
                  </svg>
                  <span>设置...</span>
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </header>
  )
}
