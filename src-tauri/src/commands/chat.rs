use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use futures::StreamExt;
use crate::storage::database::Database;
use crate::storage::file_manager::FileManager;
use crate::llm::gateway::LlmGateway;
use crate::llm::streaming::{ChatMessage, StopReason, StreamEvent};
use crate::llm::masking::{MaskingContext, MaskingLevel};
use crate::llm::tool_executor::{self, ToolContext};
use crate::llm::orchestrator::{self, AnalysisAction, StepConfig};
use crate::llm::prompts;
use crate::storage::crypto::SecureStorage;
use crate::models::settings::AppSettings;

/// Maximum agent loop iterations for daily consultation mode.
const MAX_TOOL_ITERATIONS: usize = 10;

/// Send a user message and trigger the LLM agent loop.
///
/// The function detects whether the conversation is in analysis mode
/// (structured 5-step workflow) or daily consultation mode, and
/// dispatches accordingly with the appropriate system prompt, tool
/// filter, and iteration limit.
#[tauri::command]
pub async fn send_message(
    db: State<'_, Arc<Database>>,
    gateway: State<'_, Arc<LlmGateway>>,
    file_mgr: State<'_, Arc<FileManager>>,
    crypto: State<'_, Option<Arc<SecureStorage>>>,
    app: AppHandle,
    conversation_id: String,
    content: String,
    file_ids: Vec<String>,
) -> Result<(), String> {
    log::info!("=== send_message START === conversation_id={}, content_len={}, file_ids={:?}",
        conversation_id, content.len(), file_ids);

    // 1. Look up attached file metadata (single batch query)
    let file_attachments = if file_ids.is_empty() {
        Vec::new()
    } else {
        db.get_uploaded_files_by_ids(&file_ids).map_err(|e| e.to_string())?
    };

    // 2. Save user message to DB (including file references)
    let msg_id = uuid::Uuid::new_v4().to_string();
    let content_json = if file_attachments.is_empty() {
        serde_json::json!({ "text": content }).to_string()
    } else {
        let files_meta: Vec<serde_json::Value> = file_attachments.iter().map(|f| {
            serde_json::json!({
                "id": f["id"],
                "fileName": f["originalName"],
                "fileSize": f["fileSize"],
                "fileType": f["fileType"],
                "status": "uploaded",
            })
        }).collect();
        serde_json::json!({ "text": content, "files": files_meta }).to_string()
    };

    db.insert_message(&msg_id, &conversation_id, "user", &content_json)
        .map_err(|e| e.to_string())?;

    // NOTE: We do NOT emit "message:updated" for the user message here.
    // The frontend already adds an optimistic user message to the store
    // before calling this IPC command. Emitting here would cause duplicates.

    // 3. Build LLM content with file references
    let llm_content = if file_attachments.is_empty() {
        content.clone()
    } else {
        let file_refs: Vec<String> = file_attachments.iter().map(|f| {
            let name = f["originalName"].as_str().unwrap_or("unknown");
            let ftype = f["fileType"].as_str().unwrap_or("unknown");
            let fid = f["id"].as_str().unwrap_or("");
            format!("- {} (file_id: \"{}\", 类型: {})", name, fid, ftype)
        }).collect();
        format!(
            "{}\n\n[已上传文件]\n{}\n\n提示：先调用 analyze_file(file_id) 获取文件的 filePath（绝对路径），然后在 execute_python 中使用该 filePath 读取文件。",
            content, file_refs.join("\n")
        )
    };

    // 4. Load settings
    let settings_map = db.get_all_settings().map_err(|e| e.to_string())?;
    let mut settings: AppSettings = if settings_map.is_empty() {
        log::info!("No settings in DB, using defaults");
        AppSettings::default()
    } else {
        log::info!("Settings map has {} keys: {:?}", settings_map.len(),
            settings_map.keys().collect::<Vec<_>>());
        AppSettings::from_string_map(&settings_map)
    };

    log::info!("Settings loaded: primary_model={}, masking={}, auto_routing={}",
        settings.primary_model, settings.data_masking_level, settings.auto_model_routing);

    // Log raw key info (before decryption) for diagnostics
    let raw_pk_len = settings.primary_api_key.len();
    let raw_pk_has_colon = settings.primary_api_key.contains(':');
    log::info!("Raw primary_api_key: len={}, contains_colon={}, first_10='{}'",
        raw_pk_len, raw_pk_has_colon,
        settings.primary_api_key.chars().take(10).collect::<String>());

    // Decrypt API keys if SecureStorage is available
    if let Some(ss) = crypto.as_ref() {
        log::info!("SecureStorage available, decrypting API keys...");
        settings.primary_api_key = decrypt_key(ss, &settings.primary_api_key);
        settings.tavily_api_key = decrypt_key(ss, &settings.tavily_api_key);
    } else {
        log::warn!("SecureStorage NOT available, using raw key values");
    }

    log::info!("After decryption: primary_api_key len={}, first_10='{}'",
        settings.primary_api_key.len(),
        settings.primary_api_key.chars().take(10).collect::<String>());

    // Fall back to built-in default key ONLY for DeepSeek provider
    // (the built-in key is a DeepSeek key, using it for other providers would cause 401)
    if settings.primary_api_key.is_empty() && settings.primary_model == "deepseek-v3" {
        log::info!("Primary API key empty for DeepSeek, falling back to built-in default key");
        let defaults = AppSettings::default();
        settings.primary_api_key = defaults.primary_api_key.clone();
    }

    log::info!("Final primary_api_key: len={}, first_10='{}'",
        settings.primary_api_key.len(),
        settings.primary_api_key.chars().take(10).collect::<String>());

    // Check if API key is configured
    if settings.primary_api_key.is_empty() {
        let provider_name = match settings.primary_model.as_str() {
            "deepseek-v3" => "DeepSeek",
            "qwen-plus" => "通义千问",
            "openai" => "OpenAI",
            "claude" => "Claude",
            "volcano" => "火山引擎",
            _ => &settings.primary_model,
        };
        let error_msg = format!("请先在设置中配置 {} 的 API Key", provider_name);
        app.emit("streaming:error", serde_json::json!({ "error": error_msg }))
            .map_err(|e| e.to_string())?;
        return Err(error_msg);
    }

    // 5. Build initial message history from DB (sliding window: last N messages)
    //    Uses SQL LIMIT to avoid loading the full history for long conversations.
    const MAX_HISTORY_MESSAGES: u32 = 20;
    let db_messages = db.get_recent_messages(&conversation_id, MAX_HISTORY_MESSAGES)
        .map_err(|e| e.to_string())?;
    let mut chat_messages: Vec<ChatMessage> = db_messages.iter().filter_map(|m| {
        let role = m.get("role")?.as_str()?;
        let content_val = m.get("content")?;
        let text = content_val.get("text")?.as_str()?.to_string();
        Some(ChatMessage::text(role, text))
    }).collect();

    // Replace the last user message content with llm_content that includes file references
    if !file_attachments.is_empty() {
        if let Some(last) = chat_messages.last_mut() {
            if last.role == "user" {
                last.content = llm_content;
            }
        }
    }

    log::info!("Chat messages built: count={}, roles={:?}",
        chat_messages.len(),
        chat_messages.iter().map(|m| m.role.as_str()).collect::<Vec<_>>());

    // 6. Determine masking level
    // Always use Strict masking — PII protection is non-negotiable.
    // The setting is kept for forward compatibility but defaults to strict.
    let masking_level = MaskingLevel::Strict;

    // 7. Derive workspace path for tool execution
    let workspace_path = db.get_setting("workspacePath")
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| file_mgr.workspace_path().to_path_buf());

    // 8. Detect analysis mode and determine step configuration
    let has_files = !file_attachments.is_empty();
    let existing_step = orchestrator::get_current_step(&db, &conversation_id);
    let is_analysis = existing_step.is_some()
        || orchestrator::detect_analysis_mode(&chat_messages, has_files);

    let step_config: Option<StepConfig> = if is_analysis {
        // Initialize analysis state if this is a new detection
        if existing_step.is_none() {
            log::info!("New analysis detected for conversation {}, initializing state", conversation_id);
            let _ = orchestrator::init_analysis_state(&db.inner().clone(), &conversation_id);
        }

        // Determine next action from orchestrator
        let action = orchestrator::next_action(&db.inner().clone(), &conversation_id, &content);
        log::info!("Orchestrator action for conversation {}: {:?}",
            conversation_id, std::mem::discriminant(&action));

        match action {
            AnalysisAction::RunStep(config) => {
                log::info!("Running analysis step {} for conversation {}",
                    config.step, conversation_id);
                // Save step as in-progress
                let _ = orchestrator::advance_step(
                    &db.inner().clone(), &conversation_id, config.step, "in_progress",
                );
                Some(config)
            }
            AnalysisAction::AdvanceAndRun(config) => {
                log::info!("Advancing to step {} for conversation {}",
                    config.step, conversation_id);
                // Mark previous step as completed, new step as in-progress
                if config.step > 1 {
                    let _ = orchestrator::advance_step(
                        &db.inner().clone(), &conversation_id, config.step - 1, "completed",
                    );
                }
                let _ = orchestrator::advance_step(
                    &db.inner().clone(), &conversation_id, config.step, "in_progress",
                );
                Some(config)
            }
            AnalysisAction::Complete => {
                log::info!("Analysis complete for conversation {}", conversation_id);
                let _ = orchestrator::advance_step(
                    &db.inner().clone(), &conversation_id, 5, "completed",
                );
                // Fall through to daily mode for any follow-up questions
                None
            }
        }
    } else {
        None
    };

    // Clone everything needed for the background task
    let assistant_id = uuid::Uuid::new_v4().to_string();
    let assistant_id_clone = assistant_id.clone();
    let conversation_id_clone = conversation_id.clone();
    let db_clone = db.inner().clone();
    let gateway_clone = gateway.inner().clone();
    let file_mgr_clone = file_mgr.inner().clone();
    let app_clone = app.clone();

    // 9. Spawn the agent loop in a background task
    log::info!("=== Spawning agent_loop === assistant_id={}, analysis_step={:?}",
        assistant_id, step_config.as_ref().map(|c| c.step));
    tokio::spawn(async move {
        agent_loop(
            db_clone,
            gateway_clone,
            file_mgr_clone,
            app_clone,
            settings,
            chat_messages,
            masking_level,
            workspace_path,
            assistant_id_clone,
            conversation_id_clone,
            step_config,
        ).await;
    });

    Ok(())
}

/// The core agent loop — stream, execute tools, re-stream until done.
///
/// When `step_config` is `Some`, the loop operates in analysis mode:
/// - Uses the step's system prompt
/// - Filters tools to only the step's relevant tools
/// - Respects the step's max_iterations limit
/// - Auto-advances to the next step when the current step completes
///
/// When `step_config` is `None`, operates in daily consultation mode:
/// - Uses the daily system prompt
/// - All tools available
/// - Standard MAX_TOOL_ITERATIONS limit
async fn agent_loop(
    db: Arc<Database>,
    gateway: Arc<LlmGateway>,
    file_mgr: Arc<FileManager>,
    app: AppHandle,
    settings: AppSettings,
    initial_messages: Vec<ChatMessage>,
    masking_level: MaskingLevel,
    workspace_path: std::path::PathBuf,
    assistant_id: String,
    conversation_id: String,
    step_config: Option<StepConfig>,
) {
    let tavily_api_key = if settings.tavily_api_key.is_empty() {
        None
    } else {
        Some(settings.tavily_api_key.clone())
    };

    // Build file context: query ALL uploaded files for this conversation
    // so the LLM always knows what's available (even after sliding window truncation)
    let file_context = match db.get_uploaded_files_for_conversation(&conversation_id) {
        Ok(files) if !files.is_empty() => {
            let refs: Vec<String> = files.iter().map(|f| {
                let name = f["originalName"].as_str().unwrap_or("unknown");
                let fid = f["id"].as_str().unwrap_or("");
                let ftype = f["fileType"].as_str().unwrap_or("unknown");
                format!("- {} (file_id: \"{}\", 类型: {})", name, fid, ftype)
            }).collect();
            format!(
                "\n\n[本次会话的文件]\n{}\n说明：先调用 analyze_file(file_id) 获取文件信息，返回中的 filePath 字段是文件绝对路径。execute_python 中使用该 filePath 读取文件。",
                refs.join("\n")
            )
        }
        _ => String::new(),
    };

    let mut current_step_config = step_config;
    let mut messages = initial_messages;
    let mut current_assistant_id = assistant_id;

    // Outer loop: iterates through analysis steps (or runs once for daily mode)
    loop {
        // Determine system prompt, tool filter, and token budget based on mode
        let (system_prompt, tool_defs_override, max_iterations, token_budget) = match &current_step_config {
            Some(config) => {
                log::info!("Agent loop in ANALYSIS mode: step={}, tools={}, max_iter={}",
                    config.step,
                    config.tool_defs.len(),
                    config.max_iterations);
                (
                    config.system_prompt.clone(),
                    Some(config.tool_defs.clone()),
                    config.max_iterations,
                    8192u32, // analysis steps need more output room for structured data
                )
            }
            None => {
                log::info!("Agent loop in DAILY CONSULTATION mode");
                (
                    prompts::get_system_prompt(None),
                    None, // use all tools
                    MAX_TOOL_ITERATIONS,
                    4096u32, // daily consultation: standard budget
                )
            }
        };

        // Append file context to system prompt so LLM always has file info
        let system_prompt = format!("{}{}", system_prompt, file_context);

        let mut full_content = String::new();
        let mut combined_mask_ctx: Option<MaskingContext> = None;

        for iteration in 0..max_iterations {
            log::info!(
                "=== [AGENT] iteration={}/{} conversation={} messages={} ===",
                iteration, max_iterations, conversation_id, messages.len()
            );
            // Log each message role + content length for debugging
            for (i, m) in messages.iter().enumerate() {
                let has_tc = m.tool_calls.as_ref().map_or(0, |v| v.len());
                let tc_id = m.tool_call_id.as_deref().unwrap_or("-");
                log::debug!(
                    "[AGENT] msg[{}] role={} len={} tool_call_id={} tool_calls={}",
                    i, m.role, m.content.len(), tc_id, has_tc
                );
            }

            // Stream from LLM with system prompt and tool filter
            let stream_start = std::time::Instant::now();
            log::info!("[AGENT] Calling gateway.stream_message() model={} system_prompt_len={}",
                settings.primary_model, system_prompt.len());
            let stream_result = gateway
                .stream_message(
                    &settings,
                    messages.clone(),
                    masking_level.clone(),
                    Some(&system_prompt),
                    tool_defs_override.clone(),
                    token_budget,
                )
                .await;

            let (_task_id, mut stream, mask_ctx) = match stream_result {
                Ok(r) => {
                    log::info!("gateway.stream_message() OK, task_id={}", r.0);
                    r
                }
                Err(e) => {
                    log::error!("gateway.stream_message() FAILED: {}", e);
                    let _ = app.emit(
                        "streaming:error",
                        serde_json::json!({ "error": e.to_string() }),
                    );
                    return;
                }
            };

            // Keep the first iteration's masking context for unmasking the final response.
            if combined_mask_ctx.is_none() {
                combined_mask_ctx = Some(mask_ctx);
            }

            // Collect this iteration's content and tool calls
            let mut iter_content = String::new();
            let mut tool_calls = Vec::new();
            let mut stop_reason = StopReason::EndTurn;
            let mut delta_count: u32 = 0;

            while let Some(event) = stream.next().await {
                match event {
                    StreamEvent::ContentDelta { delta } => {
                        let clean = strip_thinking_markers(&delta);
                        if !clean.is_empty() {
                            delta_count += 1;
                            iter_content.push_str(&clean);
                            full_content.push_str(&clean);
                            let _ = app.emit(
                                "streaming:delta",
                                serde_json::json!({ "delta": clean }),
                            );
                        }
                    }
                    StreamEvent::ThinkingDelta { delta } => {
                        let _ = app.emit(
                            "streaming:delta",
                            serde_json::json!({ "delta": delta }),
                        );
                    }
                    StreamEvent::ToolCallStart { tool_call } => {
                        log::info!(
                            "[AGENT] Tool call received: name='{}' id='{}' args={}",
                            tool_call.name,
                            tool_call.id,
                            serde_json::to_string(&tool_call.arguments)
                                .unwrap_or_else(|_| "??".into())
                        );
                        tool_calls.push(tool_call);
                    }
                    StreamEvent::Done {
                        stop_reason: reason,
                        usage,
                        ..
                    } => {
                        stop_reason = reason;
                        let stream_elapsed = stream_start.elapsed();
                        log::info!(
                            "[AGENT] Stream done: stop_reason={:?}, usage=({} in / {} out), deltas={}, content_len={}, tool_calls={}, elapsed={:?}",
                            stop_reason, usage.input_tokens, usage.output_tokens,
                            delta_count, iter_content.len(), tool_calls.len(), stream_elapsed
                        );
                        break;
                    }
                    StreamEvent::Error { error } => {
                        log::error!("[AGENT] Stream error: {}", error);
                        let _ = app.emit(
                            "streaming:error",
                            serde_json::json!({ "error": error }),
                        );
                        return;
                    }
                }
            }

            // If no tool calls or stop reason is EndTurn, finish this step
            if tool_calls.is_empty() || stop_reason != StopReason::ToolUse {
                log::info!(
                    "[AGENT] Finishing step: stop_reason={:?}, tool_calls={}, total_content_len={}",
                    stop_reason, tool_calls.len(), full_content.len()
                );
                break; // exit inner loop, check step transition below
            }

            // --- Tool execution phase ---
            messages.push(ChatMessage::assistant_with_tool_calls(
                iter_content,
                tool_calls.clone(),
            ));

            let tool_ctx = ToolContext {
                db: db.clone(),
                file_manager: file_mgr.clone(),
                workspace_path: workspace_path.clone(),
                conversation_id: conversation_id.clone(),
                tavily_api_key: tavily_api_key.clone(),
                app_handle: Some(app.clone()),
            };

            for tc in &tool_calls {
                log::info!(
                    "[AGENT] Executing tool '{}' (id={}) with args: {}",
                    tc.name, tc.id,
                    serde_json::to_string(&tc.arguments).unwrap_or_else(|_| "??".into())
                );

                let _ = app.emit(
                    "tool:executing",
                    serde_json::json!({
                        "toolName": tc.name,
                        "toolId": tc.id,
                        "purpose": tc.arguments.get("purpose").and_then(|v| v.as_str()),
                    }),
                );

                let tool_start = std::time::Instant::now();
                let result = tool_executor::execute_tool(&tool_ctx, tc).await;
                let tool_elapsed = tool_start.elapsed();

                log::info!(
                    "[AGENT] Tool '{}' result: is_error={}, content_len={}, elapsed={:?}, preview='{}'",
                    tc.name, result.is_error, result.content.len(), tool_elapsed,
                    truncate_for_ui(&result.content, 300),
                );

                let _ = app.emit(
                    "tool:completed",
                    serde_json::json!({
                        "toolName": tc.name,
                        "toolId": tc.id,
                        "success": !result.is_error,
                        "summary": truncate_for_ui(&result.content, 200),
                    }),
                );

                let masked_result = match combined_mask_ctx.as_mut() {
                    Some(ctx) => ctx.mask_text(&result.content),
                    None => result.content.clone(),
                };
                messages.push(ChatMessage::tool_result(
                    &tc.id,
                    &tc.name,
                    masked_result,
                ));
            }
        }

        // --- Step completion: save assistant message and check auto-advance ---

        // Save current step's assistant message
        finish_agent(
            &db,
            &app,
            &current_assistant_id,
            &conversation_id,
            &full_content,
            combined_mask_ctx.as_ref(),
        );

        // Check if we should auto-advance to the next analysis step
        if let Some(ref config) = current_step_config {
            let completed_step = config.step;
            if completed_step < 5 {
                // Check if the LLM marked this step as completed (via update_progress tool)
                let db_step = orchestrator::get_current_step(&db, &conversation_id).unwrap_or(0);
                log::info!(
                    "[AGENT] Step {} finished, DB step={}, checking auto-advance",
                    completed_step, db_step
                );

                // Auto-advance if the LLM completed the step or the DB shows this step
                // (the LLM typically calls update_progress before EndTurn)
                if db_step >= completed_step {
                    let next_step = completed_step + 1;
                    log::info!(
                        "[AGENT] Auto-advancing from step {} to step {} for conversation {}",
                        completed_step, next_step, conversation_id
                    );

                    // Update DB state
                    let _ = orchestrator::advance_step(
                        &db, &conversation_id, completed_step, "completed",
                    );
                    let _ = orchestrator::advance_step(
                        &db, &conversation_id, next_step, "in_progress",
                    );

                    // Emit step transition event for frontend
                    let _ = app.emit(
                        "analysis:step-transition",
                        serde_json::json!({
                            "fromStep": completed_step,
                            "toStep": next_step,
                            "conversationId": conversation_id,
                        }),
                    );

                    // Prepare for next step: new assistant ID, reuse in-memory messages
                    current_assistant_id = uuid::Uuid::new_v4().to_string();
                    current_step_config = Some(orchestrator::build_step_config(next_step));

                    // Append the just-completed assistant message to in-memory history
                    // instead of re-fetching from DB (saves ~50ms per step transition)
                    if !full_content.is_empty() {
                        let unmasked = match combined_mask_ctx.as_ref() {
                            Some(ctx) => ctx.unmask(&full_content),
                            None => full_content.clone(),
                        };
                        messages.push(ChatMessage::text("assistant", &unmasked));
                    }

                    // Sliding window: keep last N messages
                    const MAX_STEP_MESSAGES: usize = 20;
                    if messages.len() > MAX_STEP_MESSAGES {
                        let skip = messages.len() - MAX_STEP_MESSAGES;
                        messages = messages.into_iter().skip(skip).collect();
                    }

                    continue; // → next iteration of outer loop (next step)
                }
            }

            // Step 5 completed or step not auto-advanced — done
            if completed_step >= 5 {
                let _ = orchestrator::advance_step(
                    &db, &conversation_id, 5, "completed",
                );
                log::info!("[AGENT] All 5 analysis steps completed for conversation {}", conversation_id);
            }
        }

        break; // Daily mode or analysis complete — exit outer loop
    }
}

/// Finalize the agent response — save to DB and emit events.
fn finish_agent(
    db: &Arc<Database>,
    app: &AppHandle,
    assistant_id: &str,
    conversation_id: &str,
    full_content: &str,
    mask_ctx: Option<&MaskingContext>,
) {
    // Unmask PII placeholders before saving to DB
    let unmasked_content = match mask_ctx {
        Some(ctx) => ctx.unmask(full_content),
        None => full_content.to_string(),
    };

    // Save assistant message to DB (with unmasked content)
    let content_json = serde_json::json!({ "text": unmasked_content }).to_string();
    if let Err(e) = db.insert_message(assistant_id, conversation_id, "assistant", &content_json) {
        log::error!("Failed to save assistant message to DB: {:#}", e);
    }

    // Emit full message FIRST so the UI has the assistant message before streaming ends.
    // This prevents a visual flash where the streaming bubble disappears
    // before the final message appears.
    let _ = app.emit(
        "message:updated",
        serde_json::json!({
            "id": assistant_id,
            "conversationId": conversation_id,
            "role": "assistant",
            "content": { "text": unmasked_content },
        }),
    );

    // Then signal that streaming is complete
    let _ = app.emit(
        "streaming:done",
        serde_json::json!({ "messageId": assistant_id }),
    );

    // Auto-generate conversation title from first assistant response
    if let Ok(msgs) = db.get_messages(conversation_id) {
        let assistant_count = msgs.iter()
            .filter(|m| m.get("role").and_then(|v| v.as_str()) == Some("assistant"))
            .count();
        if assistant_count <= 1 && !unmasked_content.is_empty() {
            let title: String = unmasked_content.chars().take(30).collect();
            let title = title.split('\n').next().unwrap_or(&title).trim().to_string();
            let title = if title.len() < unmasked_content.len() {
                format!("{}...", title)
            } else {
                title
            };
            if let Err(e) = db.update_conversation_title(conversation_id, &title) {
                log::warn!("Failed to auto-update conversation title: {}", e);
            }
            // Notify frontend of the title change
            let _ = app.emit(
                "conversation:title-updated",
                serde_json::json!({
                    "conversationId": conversation_id,
                    "title": title,
                }),
            );
        }
    }
}

/// Truncate text for UI display purposes.
fn truncate_for_ui(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        let mut truncated = text.chars().take(max_len).collect::<String>();
        truncated.push_str("...");
        truncated
    }
}

/// Stop the current streaming response.
#[tauri::command]
pub async fn stop_streaming(
    gateway: State<'_, Arc<LlmGateway>>,
) -> Result<(), String> {
    gateway.cancel_streaming().await.map_err(|e| e.to_string())
}

/// Get messages for a conversation.
/// Returns messages as JSON array (the frontend Message[] type).
#[tauri::command]
pub async fn get_messages(
    db: State<'_, Arc<Database>>,
    conversation_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    db.get_messages(&conversation_id).map_err(|e| e.to_string())
}

/// Create a new conversation.
#[tauri::command]
pub async fn create_conversation(
    db: State<'_, Arc<Database>>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db.create_conversation(&id, "New Conversation")
        .map_err(|e| e.to_string())?;
    Ok(id)
}

/// Delete a conversation and clean up associated files on disk.
///
/// 1. Queries all `stored_path` values from `uploaded_files` and
///    `generated_files` for this conversation.
/// 2. Deletes those physical files from the workspace.
/// 3. Deletes the conversation from DB (CASCADE removes related rows).
#[tauri::command]
pub async fn delete_conversation(
    db: State<'_, Arc<Database>>,
    file_mgr: State<'_, Arc<FileManager>>,
    conversation_id: String,
) -> Result<(), String> {
    // 1. Collect physical file paths before CASCADE delete removes DB rows
    let file_paths = db.get_file_paths_for_conversation(&conversation_id)
        .map_err(|e| e.to_string())?;

    // 2. Delete physical files (best-effort — don't fail if a file is already gone)
    let mut deleted = 0usize;
    let mut failed = 0usize;
    for path in &file_paths {
        let full_path = file_mgr.full_path(path);
        match std::fs::remove_file(&full_path) {
            Ok(()) => deleted += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Already gone — fine
            }
            Err(e) => {
                log::warn!("Failed to delete file {:?}: {}", full_path, e);
                failed += 1;
            }
        }
    }
    if !file_paths.is_empty() {
        log::info!(
            "Conversation {} file cleanup: {} deleted, {} failed, {} already gone",
            conversation_id, deleted, failed, file_paths.len() - deleted - failed
        );
    }

    // 3. Delete conversation (CASCADE removes uploaded_files, generated_files, messages, analysis_states)
    db.delete_conversation(&conversation_id)
        .map_err(|e| e.to_string())
}

/// Get all conversations.
#[tauri::command]
pub async fn get_conversations(
    db: State<'_, Arc<Database>>,
) -> Result<Vec<serde_json::Value>, String> {
    db.get_conversations().map_err(|e| e.to_string())
}

/// Attempt to decrypt an API key. Falls back to returning the raw value
/// if it's not in encrypted format or decryption fails.
/// If decryption fails for an encrypted value (contains ':'), return empty
/// string so the caller can fall back to defaults.
fn decrypt_key(ss: &SecureStorage, value: &str) -> String {
    if value.is_empty() || !value.contains(':') {
        log::info!("decrypt_key: value has no colon (len={}), returning as-is", value.len());
        return value.to_string();
    }
    match ss.decrypt(value) {
        Ok(plaintext) => {
            log::info!("decrypt_key: decryption OK, plaintext len={}", plaintext.len());
            plaintext
        }
        Err(e) => {
            log::warn!("Failed to decrypt API key (err={}), returning empty to trigger default fallback", e);
            String::new()
        }
    }
}

/// Strip DeepSeek internal thinking markers from content deltas.
///
/// DeepSeek models sometimes leak `<｜end▁of▁thinking｜>` and similar
/// markers into the regular content stream. These should not be shown
/// to users.
fn strip_thinking_markers(text: &str) -> String {
    text.replace("<｜end▁of▁thinking｜>", "")
        .replace("<｜begin▁of▁thinking｜>", "")
        .replace("<|end▁of▁thinking|>", "")
        .replace("<|begin▁of▁thinking|>", "")
}
