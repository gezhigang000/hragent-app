//! Analysis orchestrator — manages the 5-step compensation analysis workflow.
//!
//! The orchestrator sits between `send_message` and `agent_loop`, detecting
//! when a conversation should enter (or continue) the structured analysis
//! flow, and providing step-specific configuration (system prompts, tool
//! filters, confirmation checkpoints) to the agent loop.
#![allow(dead_code)]

use crate::llm::prompts;
use crate::llm::streaming::ChatMessage;
use crate::llm::tools;
use crate::llm::streaming::ToolDefinition;
use crate::storage::database::Database;
use std::sync::Arc;

/// Configuration for a single analysis step.
#[derive(Debug, Clone)]
pub struct StepConfig {
    /// Step number (1–5).
    pub step: u32,
    /// Full system prompt (BASE + step-specific) from `prompts.rs`.
    pub system_prompt: String,
    /// Tool definitions available during this step.
    pub tool_defs: Vec<ToolDefinition>,
    /// Max tool-loop iterations within this step.
    pub max_iterations: usize,
    /// Whether the step requires user confirmation before advancing.
    pub requires_confirmation: bool,
}

/// What the orchestrator tells `send_message` to do next.
#[derive(Debug)]
pub enum AnalysisAction {
    /// Run this step's LLM interaction with the given config.
    RunStep(StepConfig),
    /// The user's message is a confirmation — advance to the next step.
    AdvanceAndRun(StepConfig),
    /// Analysis is complete (all 5 steps done).
    Complete,
}

// ────────────────────────────────────────────────────────────────
// Detection
// ────────────────────────────────────────────────────────────────

/// Check if a conversation should enter (or is already in) analysis mode.
///
/// Returns `true` when:
/// - The conversation has uploaded files AND salary-related keywords appear, OR
/// - The user explicitly asks for compensation analysis.
pub fn detect_analysis_mode(messages: &[ChatMessage], has_files: bool) -> bool {
    let last_user = messages.iter().rev().find(|m| m.role == "user");
    let text = match last_user {
        Some(msg) => msg.content.to_lowercase(),
        None => return false,
    };

    // Explicit analysis request keywords
    let explicit_keywords = [
        "薪酬分析", "薪酬诊断", "公平性分析", "薪酬公平",
        "开始分析", "帮我分析", "做一次分析", "深度分析",
        "compensation analysis", "pay equity", "salary analysis",
        "fairness analysis",
    ];
    if explicit_keywords.iter().any(|kw| text.contains(kw)) {
        return true;
    }

    // File upload + salary-related keywords
    if has_files {
        let salary_keywords = [
            "工资", "薪酬", "薪资", "工资表", "薪酬表",
            "salary", "compensation", "payroll", "wage",
        ];
        if salary_keywords.iter().any(|kw| text.contains(kw)) {
            return true;
        }

        // Check for file reference patterns that suggest salary data
        let file_patterns = [
            "工资表", "薪酬", "薪资", "salary", "payroll",
        ];
        if file_patterns.iter().any(|kw| text.contains(kw)) {
            return true;
        }
    }

    false
}

// ────────────────────────────────────────────────────────────────
// State management
// ────────────────────────────────────────────────────────────────

/// Load the current analysis step from the database.
///
/// Returns `None` if the conversation has no analysis state.
/// Returns `Some(0)` if analysis is detected but hasn't started yet.
pub fn get_current_step(db: &Database, conversation_id: &str) -> Option<u32> {
    match db.get_analysis_state(conversation_id) {
        Ok(Some(state)) => {
            let step = state["currentStep"].as_i64().unwrap_or(0) as u32;
            Some(step)
        }
        _ => None,
    }
}

/// Save step progress to the database.
pub fn advance_step(
    db: &Arc<Database>,
    conversation_id: &str,
    step: u32,
    status: &str,
) -> Result<(), String> {
    let step_status = format!(r#"{{"step{}_status":"{}"}}"#, step, status);
    let state_data = "{}";
    db.upsert_analysis_state(conversation_id, step as i32, &step_status, state_data)
        .map_err(|e| e.to_string())
}

/// Initialize analysis state for a conversation (step 0 = not started).
pub fn init_analysis_state(
    db: &Arc<Database>,
    conversation_id: &str,
) -> Result<(), String> {
    db.upsert_analysis_state(conversation_id, 0, r#"{"status":"initialized"}"#, "{}")
        .map_err(|e| e.to_string())
}

// ────────────────────────────────────────────────────────────────
// Step config builder
// ────────────────────────────────────────────────────────────────

/// Build the [`StepConfig`] for a given step number.
pub fn build_step_config(step: u32) -> StepConfig {
    StepConfig {
        step,
        system_prompt: prompts::get_system_prompt(Some(step)),
        tool_defs: tools::get_tool_definitions_for_step(step),
        max_iterations: match step {
            1 => 15, // data cleaning: load + field mapping + cleaning + quality + export + save
            2 => 15, // job normalization: industry detect + clustering + validation
            3 => 15, // level inference: framework + 3 phases + export
            4 => 20, // 6-dimension analysis is the most complex
            5 => 15, // report generation + 3 scenarios + export
            _ => 10,
        },
        requires_confirmation: true, // all steps need user confirmation
    }
}

// ────────────────────────────────────────────────────────────────
// Action routing
// ────────────────────────────────────────────────────────────────

/// Determine the next action based on the current state and user's message.
///
/// This is the main entry point called from `send_message`.
pub fn next_action(
    db: &Arc<Database>,
    conversation_id: &str,
    last_user_message: &str,
) -> AnalysisAction {
    let current_step = get_current_step(db, conversation_id);

    match current_step {
        None | Some(0) => {
            // Analysis just detected — start step 1
            AnalysisAction::RunStep(build_step_config(1))
        }
        Some(step) if step >= 5 => {
            // Check if user is confirming step 5 (final step)
            if is_confirmation(last_user_message) {
                AnalysisAction::Complete
            } else {
                // Re-run step 5 with user feedback
                AnalysisAction::RunStep(build_step_config(5))
            }
        }
        Some(step) => {
            // Mid-analysis: check if the user is confirming the current step
            if is_confirmation(last_user_message) {
                let next_step = step + 1;
                AnalysisAction::AdvanceAndRun(build_step_config(next_step))
            } else {
                // User has modifications or questions — re-run current step
                AnalysisAction::RunStep(build_step_config(step))
            }
        }
    }
}

/// Check if the user's message is a confirmation to proceed.
fn is_confirmation(text: &str) -> bool {
    let text = text.trim().to_lowercase();

    // Short confirmations
    let confirm_phrases = [
        "确认", "继续", "好的", "可以", "没问题", "ok", "okay",
        "yes", "好", "行", "对", "是的", "确定", "通过",
        "下一步", "继续吧", "没有问题", "同意", "认可",
        "proceed", "continue", "confirm", "next",
        "lgtm", "looks good",
    ];

    // Check exact or near-exact match for short messages
    if text.len() < 30 {
        if confirm_phrases.iter().any(|p| text.contains(p)) {
            return true;
        }
    }

    false
}

/// Build the message array with the system prompt prepended.
///
/// The system prompt is inserted as the first message with role "system".
/// This is used by the agent loop to inject step-specific guidance.
pub fn build_step_messages(
    base_messages: &[ChatMessage],
    system_prompt: &str,
) -> Vec<ChatMessage> {
    let mut messages = Vec::with_capacity(base_messages.len() + 1);

    // Prepend system prompt
    messages.push(ChatMessage::text("system", system_prompt));

    // Append all existing messages
    messages.extend_from_slice(base_messages);

    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_messages(texts: &[(&str, &str)]) -> Vec<ChatMessage> {
        texts
            .iter()
            .map(|(role, content)| ChatMessage::text(role, *content))
            .collect()
    }

    // ── detect_analysis_mode ──

    #[test]
    fn test_detect_explicit_analysis_request() {
        let msgs = make_messages(&[("user", "请帮我做一次薪酬公平性分析")]);
        assert!(detect_analysis_mode(&msgs, false));
    }

    #[test]
    fn test_detect_explicit_english() {
        let msgs = make_messages(&[("user", "Run a compensation analysis on this data")]);
        assert!(detect_analysis_mode(&msgs, false));
    }

    #[test]
    fn test_detect_file_with_salary_keyword() {
        let msgs = make_messages(&[("user", "这是我们的工资表，帮我看看")]);
        assert!(detect_analysis_mode(&msgs, true));
    }

    #[test]
    fn test_no_detect_general_chat() {
        let msgs = make_messages(&[("user", "你好，请问社保基数是多少？")]);
        assert!(!detect_analysis_mode(&msgs, false));
    }

    #[test]
    fn test_no_detect_file_without_salary() {
        let msgs = make_messages(&[("user", "这是我们的组织架构图")]);
        assert!(!detect_analysis_mode(&msgs, true));
    }

    #[test]
    fn test_no_detect_empty_messages() {
        let msgs: Vec<ChatMessage> = vec![];
        assert!(!detect_analysis_mode(&msgs, false));
    }

    // ── is_confirmation ──

    #[test]
    fn test_confirmation_chinese() {
        assert!(is_confirmation("确认"));
        assert!(is_confirmation("好的，继续"));
        assert!(is_confirmation("没问题"));
        assert!(is_confirmation("可以，下一步"));
    }

    #[test]
    fn test_confirmation_english() {
        assert!(is_confirmation("ok"));
        assert!(is_confirmation("Yes"));
        assert!(is_confirmation("LGTM"));
        assert!(is_confirmation("continue"));
    }

    #[test]
    fn test_not_confirmation_modification() {
        // Long message with specific feedback is not a simple confirmation
        assert!(!is_confirmation(
            "把品质合并到生产里，重新调整岗位族方案，我觉得 6 个族太多了，减少到 5 个"
        ));
    }

    #[test]
    fn test_not_confirmation_question() {
        assert!(!is_confirmation(
            "为什么张三被标记为偏低？他的绩效一直很好啊"
        ));
    }

    // ── build_step_messages ──

    #[test]
    fn test_build_step_messages_prepends_system() {
        let base = make_messages(&[
            ("user", "Hello"),
            ("assistant", "Hi there"),
        ]);
        let result = build_step_messages(&base, "You are a helpful assistant.");
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].role, "system");
        assert_eq!(result[0].content, "You are a helpful assistant.");
        assert_eq!(result[1].role, "user");
        assert_eq!(result[2].role, "assistant");
    }

    // ── build_step_config ──

    #[test]
    fn test_step_config_has_correct_prompt() {
        let config = build_step_config(1);
        assert_eq!(config.step, 1);
        assert!(config.system_prompt.contains("Step 1"));
        assert!(config.requires_confirmation);
    }

    #[test]
    fn test_step_config_tools_not_empty() {
        for step in 1..=5 {
            let config = build_step_config(step);
            assert!(
                !config.tool_defs.is_empty(),
                "Step {} should have tool definitions",
                step
            );
        }
    }

    #[test]
    fn test_step4_has_most_iterations() {
        let config = build_step_config(4);
        assert_eq!(config.max_iterations, 20);
    }
}
