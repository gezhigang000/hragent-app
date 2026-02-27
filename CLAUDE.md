# AI小家 — 项目上下文

> 组织咨询专家 — 随叫随到的 HR 智能顾问。Tauri 2.x 桌面应用。

## 项目结构

```
code/
├── src/                          # React + TypeScript 前端
│   ├── App.tsx                   # 入口，挂载 useStreaming + ToastContainer
│   ├── components/
│   │   ├── layout/               # Sidebar, TopBar, ChatArea, InputBar
│   │   ├── chat/                 # MessageList, MessageItem, UserBubble, AiBubble,
│   │   │                         # StreamingBubble, TypingIndicator, StepDivider
│   │   ├── rich-content/         # CodeBlock, DataTable, MetricCards, OptionCards 等
│   │   ├── settings/             # SettingsModal
│   │   └── common/               # Avatar, ToastContainer
│   ├── stores/                   # Zustand: chatStore, settingsStore, analysisStore, notificationStore
│   ├── hooks/                    # useChat, useStreaming, useFileUpload, useTauriEvent
│   ├── lib/                      # tauri.ts (IPC), format.ts
│   ├── types/                    # message.ts, analysis.ts, settings.ts
│   └── styles/                   # globals.css (TailwindCSS 4 Design Tokens)
├── src-tauri/                    # Rust 后端
│   └── src/
│       ├── lib.rs                # Tauri 应用构建 + 命令注册
│       ├── commands/             # chat, file, settings (IPC 命令)
│       ├── llm/                  # gateway, router, providers/, tools, masking, streaming,
│       │                         # prompts (系统提示词库), orchestrator (5步分析编排器)
│       ├── search/               # tavily, searxng
│       ├── storage/              # database (SQLite), crypto, workspace, file_manager
│       ├── python/               # runner, parser, sandbox
│       └── models/               # conversation, message, analysis, settings
└── python/                       # Python 脚本（Phase 7，未实现）
```

## 开发命令

```bash
pnpm install              # 安装前端依赖
pnpm dev                  # 启动 Vite dev server（仅前端）
pnpm tauri dev            # 启动完整 Tauri 开发环境（前端 + Rust 后端）
pnpm tauri build          # 构建生产包
pnpm test                 # 运行前端测试 (vitest)
pnpm test:watch           # 前端测试监听模式
cargo test --manifest-path src-tauri/Cargo.toml  # 运行 Rust 测试
```

## 关键设计决策

1. **Zustand 回调必须用 `getState()`** — `useChatStore()` 返回的 `store` 对象每次渲染都是新引用。`useCallback([store])` 会导致无限循环。所有回调内部必须用 `useChatStore.getState()` 获取最新状态，依赖数组为 `[]`。

2. **后端不发用户消息 `message:updated` 事件** — 前端已乐观添加用户消息。后端 `send_message` 只保存到 DB，不再 emit `message:updated`，避免 ID 不同导致重复。

3. **事件顺序：先 `message:updated` 后 `streaming:done`** — 确保助手消息加入 store 后再清除流式状态，避免 UI 闪烁。

4. **API Key 解密失败回退默认值** — `SecureStorage` 每次启动生成新 master key，旧加密值不可解。`decrypt_key` 失败返回空字符串，`send_message` 回退到 `AppSettings::default()` 内置 key。

5. **流式内容独立渲染** — `StreamingBubble` 组件在 `isStreaming=true` 时渲染 `streamingContent`，与最终 `messages` 数组分离。

6. **Tauri IPC 参数命名** — Tauri 2.x 自动将 Rust snake_case 参数名转为 camelCase。前端 `invoke` 用 `camelCase`（`conversationId`），Rust `#[tauri::command]` 用 `snake_case`（`conversation_id`）。JSON 响应用 `camelCase`（通过 `#[serde(rename_all = "camelCase")]`）。

7. **多模型 Provider 独立 Key 持久化** — 每个 Provider 的 API Key 以 `apiKey:{provider}` 键独立加密存储在 SQLite settings 表中。`update_settings` 保存时自动写入 `apiKey:{primaryModel}`。切换 Provider 时通过 `switch_provider` 命令从对应键加载解密。设置弹窗通过 `update_all_provider_keys` 批量保存所有 Provider Key。前端 `settingsStore` 维护 `configuredProviders` 列表（有 Key 的 Provider），TopBar 在列表长度 >1 时显示下拉切换菜单。

8. **删除会话自动清理物理文件** — `delete_conversation` 命令在 DB CASCADE 删除前，先查询 `uploaded_files` 和 `generated_files` 的 `stored_path`，逐个删除物理文件（best-effort，已删除的跳过）。

9. **DB 查询优化** — `get_recent_messages(id, limit)` 用 SQL LIMIT 只取最近 N 条消息，避免加载全部历史。`get_uploaded_files_by_ids(ids)` 用 `IN (...)` 批量查询替代逐条查询。步骤切换时复用内存中的消息列表，不重新读 DB。

## 数据存储位置

| 数据 | 路径 |
|------|------|
| SQLite 数据库 | `~/Library/Application Support/com.aijia.app/aijia.db` (macOS) |
| 加密密钥 | OS Keychain (`com.aijia.app.secure_storage`) |
| 用户工作目录 | `~/.renlijia/` (默认) |
| 前端设置 | SQLite `settings` 表 |
| 各 Provider API Key | SQLite `settings` 表，键 `apiKey:{provider}`，AES-256-GCM 加密 |

## 命名约定

- **Tauri 事件名**: `streaming:delta`, `streaming:done`, `streaming:error`, `message:updated`, `conversation:title-updated`, `tool:executing`, `tool:completed`
- **Tauri 命令名**: snake_case (`send_message`, `create_conversation`, `switch_provider`, `get_configured_providers`, `get_all_provider_keys`, `update_all_provider_keys`)
- **前端 Store**: camelCase (`useChatStore`, `activeConversationId`)
- **CSS Token 前缀**: `--color-*`, `--spacing-*`, `--radius-*`, `--shadow-*`

## 视觉标准强制规则

> 完整规范见 `docs/visual-standard.md`，以下为开发时必须遵守的硬性约束。

**字号 — 只能用 Token 类名：**

| Token | 大小 | 用途 |
|-------|------|------|
| `text-xs` | 12px | 标注、版本号、Tag、时间戳 |
| `text-sm` | 13px | 辅助信息、表头、表格数据 |
| `text-base` | 14px | 正文、表单输入、卡片标题 |
| `text-md` | 15px | 对话正文、主内容 |
| `text-lg` | 17px | 页面标题 |
| `text-xl` | 20px | 大数字 |
| `text-2xl` | 24px | Metric 大数字 |

- 根字号 `16px`（Apple HIG 标准），禁止修改
- 禁止 `text-[X.XXrem]` 任意值

**颜色 — 只能用 CSS 变量：**

- 语义色：`var(--color-semantic-red)` / `var(--color-semantic-blue)` / ... 及其 `-bg` / `-bg-light` / `-border` 变体
- 主色：`var(--color-accent)` 及其变体
- 新增颜色必须先在 `globals.css` `@theme` 中注册
- 禁止在组件中直接写 `rgba(R,G,B,A)` 语义色值

**圆角 — 只能用 Token：**

- `rounded-xs`(4px) / `rounded-sm`(6px) / `rounded-md`(8px) / `rounded-lg`(12px) / `rounded-xl`(16px) / `rounded-full`
- 禁止 `rounded-[Xpx]` 任意值

**间距 — 统一规则：**

- rich-content 组件外边距：`my-3`（12px）
- 消息气泡间距：`mb-7`（28px）
- 步骤分割线间距：`my-7`（28px）

**阴影/遮罩 — 必须用 Token：**

- 模态框遮罩：`var(--color-overlay)`
- 模态框阴影：`var(--shadow-modal)`
- 输入栏阴影：`var(--shadow-input)`
