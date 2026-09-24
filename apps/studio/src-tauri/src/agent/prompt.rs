//! What the model reads: the system prompt and the conversation history, fitted to the
//! model's context window.
//!
//! The system prompt deliberately contains the date but not the time: it is identical from one
//! turn to the next, so llama.cpp's prompt cache reuses it instead of re-reading it every time.

use std::collections::HashSet;

use crate::db::{Message, MessageStatus, Role};
use crate::llm::{LlmMessage, ToolSpec};
use crate::models::ModelEntry;
use crate::skills::SkillRegistry;

/// Rough token estimate: good enough for budgeting, never used for billing.
pub fn estimate_tokens(text: &str) -> usize {
    text.len() / 3 + 1
}

pub struct PromptInputs<'a> {
    pub model: &'a ModelEntry,
    pub tools: &'a [ToolSpec],
    pub skills: &'a SkillRegistry,
    pub context_tokens: usize,
}

pub fn system_prompt(p: &PromptInputs<'_>) -> String {
    let today = chrono::Local::now().format("%A, %B %-d, %Y");
    let os = match demido_core::Os::current() {
        demido_core::Os::Windows => "Windows",
        demido_core::Os::Macos => "macOS",
        demido_core::Os::Linux => "Linux",
    };
    let mut s = format!(
        "You are {name}, an AI assistant inside Demido Studio, a desktop app. Today is {today}. The user's computer runs {os}.\n\n\
         Answer clearly and concisely. Use Markdown when it helps: tables for data, code blocks for code. \
         If you are unsure or lack the data, say so instead of guessing.\n",
        name = p.model.name,
    );

    let has = |name: &str| p.tools.iter().any(|t| t.name == name);
    if !p.tools.is_empty() {
        s.push_str(
            "\n## Tools\nUse a tool only when the request needs it; never for greetings or general knowledge. \
             After using tools, answer the user in plain language and mention files you created.\n",
        );
        if has("market_quote") || has("market_candles") {
            s.push_str(
                "- Market data: TradingView symbols look like EXCHANGE:TICKER (FX:EURUSD, OANDA:XAUUSD, NASDAQ:AAPL, BINANCE:BTCUSDT, SP:SPX). \
                 When unsure of a symbol, call market_search first. Candle data is saved as a CSV file in the workspace; \
                 analyse it with run_python instead of reading it all.\n",
            );
        }
        if has("run_python") {
            s.push_str(
                "- Python runs in this chat's workspace folder with numpy, pandas and matplotlib. Open data files by their \
                 relative path (for example pd.read_csv('data/FX_EURUSD_1h_20260101_20260201.csv')). Print the numbers you \
                 need. Save charts with plt.savefig('chart.png'); they are shown to the user.\n",
            );
        }
        if has("create_skill") {
            s.push_str(
                "- When the user asks to turn a task into a skill, call create_skill with numbered steps another assistant \
                 could follow: which tools to call with which parameters, and any Python code saved as a file of the skill.\n",
            );
        }
    }

    let custom = p.model.effective.system_prompt.trim();
    if !custom.is_empty() {
        s.push_str("\n## Instructions from the user\n");
        s.push_str(custom);
        s.push('\n');
    }

    // Skills get up to about a sixth of the context window.
    let budget_chars = (p.context_tokens * 3 / 6).max(2_000);
    if let Some(section) = p.skills.prompt_section(budget_chars) {
        s.push('\n');
        s.push_str(&section);
    }
    s
}

/// Converts stored messages into model history, dropping the oldest turns when the history
/// would not fit in `budget_tokens`.
pub fn history(messages: &[Message], budget_tokens: usize) -> Vec<LlmMessage> {
    // Turn boundaries: every user message starts one.
    let last_user = messages.iter().rposition(|m| m.role == Role::User).unwrap_or(0);
    let mut converted: Vec<(usize, LlmMessage, usize)> = Vec::new(); // (turn, message, tokens)
    let mut turn = 0usize;
    let mut answered: HashSet<String> = HashSet::new();
    for m in messages.iter().filter(|m| m.role == Role::Tool) {
        if let Some(id) = &m.tool_call_id {
            answered.insert(id.clone());
        }
    }
    let recent_from = recent_turn_start(messages);

    for (i, m) in messages.iter().enumerate() {
        match m.role {
            Role::User => {
                turn += 1;
                converted.push((
                    turn,
                    LlmMessage::User {
                        content: m.content.clone(),
                    },
                    estimate_tokens(&m.content),
                ));
            }
            Role::Assistant => {
                if m.status == MessageStatus::Error || m.status == MessageStatus::Streaming {
                    continue;
                }
                if m.content.trim().is_empty() && m.tool_calls.is_empty() {
                    continue;
                }
                let reasoning = if i > last_user { m.reasoning.clone() } else { None };
                let tokens = estimate_tokens(&m.content)
                    + m.tool_calls.iter().map(|c| estimate_tokens(&c.arguments) + 8).sum::<usize>();
                converted.push((
                    turn,
                    LlmMessage::Assistant {
                        content: m.content.clone(),
                        reasoning,
                        tool_calls: m.tool_calls.clone(),
                        provider_meta: m.provider_meta.clone(),
                    },
                    tokens,
                ));
                // Calls left without a result (the turn was stopped) still need an answer.
                for call in m.tool_calls.iter().filter(|c| !answered.contains(&c.id)) {
                    converted.push((
                        turn,
                        LlmMessage::Tool {
                            call_id: call.id.clone(),
                            name: call.name.clone(),
                            content: r#"{"error":"The user stopped this before it ran."}"#.into(),
                        },
                        12,
                    ));
                }
            }
            Role::Tool => {
                let Some(call_id) = m.tool_call_id.clone() else { continue };
                // Older tool output is shortened: the model already acted on it.
                let limit = if i >= recent_from { 12_000 } else { 1_500 };
                let content = if m.content.is_empty() {
                    r#"{"error":"No result."}"#.to_string()
                } else {
                    crate::tools::clip(&m.content, limit)
                };
                let tokens = estimate_tokens(&content);
                converted.push((
                    turn,
                    LlmMessage::Tool {
                        call_id,
                        name: m.tool_name.clone().unwrap_or_default(),
                        content,
                    },
                    tokens,
                ));
            }
        }
    }

    // Drop whole turns from the start until the rest fits; the latest turn always stays.
    let mut total: usize = converted.iter().map(|c| c.2).sum();
    let mut drop_until = 0usize;
    while total > budget_tokens && drop_until < turn.saturating_sub(1) {
        drop_until += 1;
        total = converted.iter().filter(|c| c.0 > drop_until).map(|c| c.2).sum();
    }
    let mut out: Vec<LlmMessage> = converted
        .into_iter()
        .filter(|c| c.0 > drop_until)
        .map(|c| c.1)
        .collect();
    if drop_until > 0 {
        if let Some(LlmMessage::User { content }) = out.first_mut() {
            *content = format!(
                "(Earlier parts of this conversation were left out to fit the context window.)\n\n{content}"
            );
        }
    }
    out
}

/// Index of the first message of the two most recent turns.
fn recent_turn_start(messages: &[Message]) -> usize {
    let users: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == Role::User)
        .map(|(i, _)| i)
        .collect();
    if users.len() >= 2 {
        users[users.len() - 2]
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ToolCall;

    fn msg(role: Role, content: &str) -> Message {
        Message::new("c", 0, role, content)
    }

    #[test]
    fn unanswered_tool_calls_get_a_placeholder() {
        let mut a = msg(Role::Assistant, "");
        a.tool_calls = vec![ToolCall {
            id: "x".into(),
            name: "run_python".into(),
            arguments: "{}".into(),
        }];
        let h = history(&[msg(Role::User, "hi"), a], 10_000);
        assert_eq!(h.len(), 3);
        assert!(matches!(&h[2], LlmMessage::Tool { call_id, .. } if call_id == "x"));
    }

    #[test]
    fn old_turns_are_dropped_to_fit() {
        let big = "x".repeat(3000);
        let messages = vec![
            msg(Role::User, &big),
            msg(Role::Assistant, &big),
            msg(Role::User, "second"),
            msg(Role::Assistant, "ok"),
            msg(Role::User, "third"),
        ];
        let h = history(&messages, 500);
        assert!(matches!(&h[0], LlmMessage::User { content } if content.contains("left out") && content.ends_with("second")));
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn failed_and_empty_assistant_messages_are_skipped() {
        let mut failed = msg(Role::Assistant, "partial");
        failed.status = MessageStatus::Error;
        let h = history(&[msg(Role::User, "q"), failed, msg(Role::Assistant, "")], 10_000);
        assert_eq!(h.len(), 1);
    }
}
