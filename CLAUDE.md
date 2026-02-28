# AI小家 — 项目上下文

> 组织专家，工作助手 — 随叫随到的 HR 智能顾问。Tauri 2.x 桌面应用。

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
│       │                         # prompts (系统提示词库, STEP0~5), orchestrator (6步分析编排器)
│       ├── search/               # tavily, searxng
│       ├── storage/              # file_store (JSON/JSONL), crypto, workspace, file_manager
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

2. **后端不发用户消息 `message:updated` 事件** — 前端已乐观添加用户消息。后端 `send_message` 只保存到文件存储，不再 emit `message:updated`，避免 ID 不同导致重复。

3. **事件顺序：先 `message:updated` 后 `streaming:done`** — 确保助手消息加入 store 后再清除流式状态，避免 UI 闪烁。

4. **API Key 解密失败回退默认值** — `SecureStorage` 每次启动生成新 master key，旧加密值不可解。`decrypt_key` 失败返回空字符串，`send_message` 回退到 `AppSettings::default()` 内置 key。

5. **流式内容独立渲染** — `StreamingBubble` 组件在 `isStreaming=true` 时渲染 `streamingContent`，与最终 `messages` 数组分离。

6. **Tauri IPC 参数命名** — Tauri 2.x 自动将 Rust snake_case 参数名转为 camelCase。前端 `invoke` 用 `camelCase`（`conversationId`），Rust `#[tauri::command]` 用 `snake_case`（`conversation_id`）。JSON 响应用 `camelCase`（通过 `#[serde(rename_all = "camelCase")]`）。

7. **多模型 Provider 独立 Key 持久化** — 每个 Provider 的 API Key 以 `apiKey:{provider}` 键独立加密存储在 `config.json` 中。`update_settings` 保存时自动写入 `apiKey:{primaryModel}`。切换 Provider 时通过 `switch_provider` 命令从对应键加载解密。设置弹窗通过 `update_all_provider_keys` 批量保存所有 Provider Key。模型切换仅通过设置弹窗操作（Sidebar 底部齿轮入口），TopBar 不显示模型信息。

8. **删除会话自动清理物理文件** — `delete_conversation` 命令先从 `file_index.json` 查询所有文件的 `stored_path`，逐个删除物理文件（best-effort，已删除的跳过），然后删除整个会话目录。

9. **文件存储读取优化** — `get_recent_messages(id, limit)` 从最新分片反向读取，通过 `HashSet<u64>` 计数唯一 seq 确定是否已收集足够消息，避免加载全部历史。`get_uploaded_files_by_ids(ids)` 和 `get_generated_files_by_ids(ids)` 扫描所有会话的 `file_index.json` 按 ID 匹配。步骤切换时复用内存中的消息列表，不重新读文件。

10. **空消息防护 + 文件写入守卫** — `finish_agent()` 仅在 `unmasked_content.trim()` 非空时写入文件存储和发送 `message:updated` 事件，避免分析模式下仅包含 Tool Call 的迭代产生空气泡。`message:updated` 仅在 `insert_message` 成功后发送，防止前端显示未持久化的幽灵消息（写入失败时跳过事件）。`finish_agent()` **不发送** `streaming:done`——该事件仅由 `AgentGuard::clear()` 在整个 agent_loop 结束时发射（见决策 25）。

11. **脱敏级别硬编码 Strict** — `chat.rs` 中 `masking_level` 固定为 `MaskingLevel::Strict`，不受设置面板控制。PII 保护为不可商量的强制要求。

12. **文件路径传递机制** — `analyze_file` 工具返回 `filePath`（绝对路径）和 `storedPath`（相对路径）。LLM 在后续 `execute_python` 中使用 `filePath` 读取文件，无需再拼接路径。

13. **Token 预算** — 分析步骤使用 8192 token 输出预算（结构化数据需要更大空间），日常咨询使用 4096 token。通过 `gateway.stream_message()` 的 `max_tokens` 参数控制。

14. **文件操作双索引查找** — `open_generated_file` 和 `reveal_file_in_folder` 通过 `resolve_stored_path()` 先在 `file_index.json` 中查找上传文件再查找生成文件，确保两种来源的文件都能正确操作。`GeneratedFileCard` 内置 "Open" 和 "Open Folder" 按钮，不依赖 LLM 的 actions 数组。

15. **多会话并发架构** — `LlmGateway` 内部用 `HashMap<String, ActiveTask>` 管理最多 3 个并发 Agent Loop（`MAX_CONCURRENT_AGENTS = 3`）。`set_busy()` 返回 `Result<(), String>`，拒绝同一会话重复提交和超限。每个会话有独立的 `AgentGuard`（包含 `conversation_id`），Drop 时自动清理对应任务。所有 Tauri 事件均携带 `conversationId` 字段路由到前端对应会话状态。`is_agent_busy` 命令返回 `Vec<String>`（所有忙碌会话 ID），`stop_streaming` 命令接受 `conversation_id` 参数。

16. **前端按会话隔离流式状态** — `chatStore` 用 `busyConversations: Set<string>` 追踪忙碌会话，`streamStates: Record<string, ConversationStreamState>` 存储每个会话的 `isStreaming`/`streamingContent`/`toolExecutions`。遗留的全局 `isStreaming`/`streamingContent`/`toolExecutions` 从 `streamStates[activeConversationId]` 派生，保持向后兼容。`useStreaming` 按 `conversationId` 路由事件到对应会话状态。

17. **崩溃恢复** — 会话目录中的 `run.lock` 文件记录正在运行的 Agent 任务（内容为 PID）。`AgentGuard::clear()` 完成时删除。`lib.rs` 启动时扫描所有会话目录的 `run.lock`，检测孤儿锁文件（PID 进程已不存在），重置卡住的 `analysis.json` 步骤状态（`in_progress` → `paused`），清理孤儿锁文件。

18. **HTTP 连接超时 + 流式取消机制** — 所有 Provider 通过 `build_http_client()`（`providers/mod.rs`）创建带 30s `connect_timeout` 的 `reqwest::Client`。`stream_message()` 返回 `(task_id, stream, mask_ctx, cancel_rx)`，不再用 `take_while` 包装。`chat.rs` agent_loop 用 `tokio::select!` 三分支消费流：cancel 信号即时生效（HTTP 卡死也能中断）、90s chunk 超时（`CHUNK_TIMEOUT_SECS`）自动终止、正常 stream 事件处理。

19. **内容区域图标极简化** — rich-content 卡片标题不用装饰性 SVG，通过语义颜色区分（红色=根因、蓝色=洞察、紫色=搜索、金色=确认/进度/摘要）。步骤完成用文字符号（✓ ●）。报告卡片用文字标签（HTML/XLS/PDF）+ 语义色，不用 emoji。SVG 仅限操作按钮。

20. **消息复制功能** — AI 消息 hover 显示"复制"按钮。代码块/表格各有复制按钮。Markdown 代码块通过 `data-copy-code` 属性（base64 编码）+ ChatArea 事件委托实现，避免 Tauri CSP 限制。表格复制为 TSV（可粘贴 Excel）。

21. **子 Agent 架构 + 显式模式状态机（Step 0~5）** — `conv.json` 的 `mode` 字段是分析流程的**唯一真相源**（`'daily'`/`'confirming'`/`'analyzing'`），`analysis.json` 仅追踪"当前在哪一步"。模式转换：`daily` →（检测到分析意图）→ `confirming`（运行 Step 0）→（用户回复）→ `analyzing`（Step 1~5）→（Step 5 确认 + `finalize_analysis()`）→ `daily`。6 步分析流程每步作为独立子 Agent 运行。Step 0 仅有 `analyze_file` + `save_analysis_note` 两个工具，负责识别文件、概括内容、询问分析方向，max_iterations=5。`orchestrator::next_action(conversation_mode, db, conversation_id, last_msg)` 基于 `mode` 字段返回 `AnalysisAction`：`DailyChat` | `StartAnalysis(StepConfig)` | `AdvanceStep(StepConfig)` | `RerunStep(StepConfig)` | `ResumeStep(StepConfig)` | `FinishAnalysis`。步骤切换时（`StartAnalysis`/`AdvanceStep`）消息列表从零构建（仅原始用户消息 + 合成摘要）。关键结论通过 `save_analysis_note` 保存到 `memory.jsonl`（key: `note:{conv_id}:{name}`），注入到下步系统提示词的 `[前序分析记录]` 部分。每步完成后 Agent 停止等待用户确认（`requires_confirmation=true`）。`is_confirmation()` 使用精确匹配列表（约 35 个词，含 Step 0 专用词），20 字符长度截断。`agent_loop` 无自动推进外循环，所有步骤推进通过用户消息触发 `send_message()` → `next_action()` 实现。`compress_tool_result()` 压缩历史消息中 `execute_python` 的冗余头部。`AGENT_TIMEOUT_SECS=900`（15 分钟），`MAX_HISTORY_MESSAGES=30`。

22. **生成文件附着到消息** — `agent_loop` 执行工具后从返回结果中收集 `fileId`（JSON 解析 + 文本匹配双重提取），`finish_agent()` 用 `storage.get_generated_files_by_ids()` 批量查询文件记录，将 `generatedFiles` 数组写入消息 `content_json`（含 `filePath` 绝对路径、空 `actions` 数组）。前端 `AiBubble` → `ContentRenderer` → `GeneratedFileCard` 渲染文件卡片，内置 "Open"（`open_generated_file`）和 "Open Folder"（`reveal_file_in_folder`）按钮。`GeneratedFileCard` 对 `file.actions` 使用 `?? []` 防护空值。

23. **数据真实性铁律（提示词层）** — `SYSTEM_PROMPT_BASE` 中嵌入【数据真实性铁律】6 条规则，禁止 LLM 构造/虚构任何数据。Step 1 额外添加"排除人员展示规则"（名单必须来自 Python stdout）。Step 1~5 每步确认卡点前均有防构造提醒。

24. **异常路径健壮性** — `advance_step()` 所有调用点使用 `if let Err(e)` 记录日志；`set_busy` 成功后 `insert_active_task` 失败自动回滚；`AgentGuard::Drop` 中文件清理同步执行不依赖 spawn；fileId 提取支持灵活空格 + UUID 长度验证；`clearConversationStreamState` 保留 `toolExecutions`，`deleteConversationStreamState` 用于会话删除；`removeConversation` 同步清理 `streamStates`/`busyConversations` 防内存泄漏。

25. **流式事件协议（单点发射原则）** — `streaming:done` **仅由 `AgentGuard::clear()` 发射**，`finish_agent()` 不再发射。新增 `streaming:step-reset` 事件用于跨步骤流式 UI 连续性。前端 `chatStore.resetConversationStreamContent()` 处理此事件。`StreamingBubble` 的 `TypingIndicator` 仅在 `!content && !activeTool` 时显示。

26. **文件存储写锁 + 原子写入** — `AppStorage` 使用 `Mutex<()>` 序列化所有写操作，防止并发读-改-写竞态。读操作无需加锁。所有 JSON 文件写入采用原子模式（写临时文件 → `fs::rename`），防止崩溃导致文件损坏。JSONL 文件使用追加写入，超过容量阈值自动分片（消息 100 条/片，审计日志 2MB/片，记忆 1MB/片）。

## 数据存储位置

| 数据 | 路径 |
|------|------|
| 文件存储根目录 | Tauri `app_data_dir()` (macOS: `~/Library/Application Support/com.aijia.app/`) |
| 加密密钥 | OS Keychain (`com.aijia.app.secure_storage`) |
| 用户工作目录 | `~/.renlijia/` (默认) |
| 应用设置 | `{base_dir}/config.json` |
| 各 Provider API Key | `{base_dir}/config.json`，键 `apiKey:{provider}`，AES-256-GCM 加密 |

## 命名约定

- **Tauri 事件名**: `streaming:delta`, `streaming:done`, `streaming:error`, `streaming:step-reset`, `message:updated`, `conversation:title-updated`, `tool:executing`, `tool:completed`, `analysis:step-transition`, `agent:idle`（所有事件均含 `conversationId` 字段）
- **Tauri 命令名**: snake_case (`send_message`, `create_conversation`, `stop_streaming(conversation_id)`, `is_agent_busy→Vec<String>`, `switch_provider`, `get_configured_providers`, `get_all_provider_keys`, `update_all_provider_keys`, `reveal_file_in_folder`)
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

- 交互色（按钮/选中/Tab）：`var(--color-primary)` 及其 `-hover` / `-active` / `-subtle` / `-muted` 变体（Carbon Black）
- 品牌色（仅 AI 头像/Logo）：`var(--color-accent)` 及其变体（Gold）
- 语义色：`var(--color-semantic-red)` / `var(--color-semantic-blue)` / ... 及其 `-bg` / `-bg-light` / `-border` 变体
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

**图标 — 内容区极简：**

- rich-content 卡片标题**不使用装饰性 SVG 图标**，通过语义颜色 + 背景色区分类型
- 报告/文件类型使用文字标签（HTML/XLS/PDF）+ 语义色，不使用 emoji
- SVG 图标仅限操作区域（Sidebar/TopBar/InputBar 按钮、Toast 关闭）
