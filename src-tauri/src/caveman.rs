//! Caveman mode — a per-conversation response-compression style.
//!
//! Each level carries a **standalone** prompt: no shared preamble is concatenated at runtime, so
//! every level can be tuned (and shortened) on its own without the others inheriting the change.
//! That duplication is deliberate — the alternative ships rules a level doesn't want and costs
//! tokens on every request, which defeats the point of the feature.
//!
//! `Off` has no prompt at all: the model must see nothing, not an instruction saying "be normal".

/// Levels, in the order the UI lists them. Serialized as the kebab-case strings stored in
/// `conversations.caveman_level` — the DB, the IPC payload and the TS union all use these.
pub const LEVELS: &[&str] = &[
    "off",
    "lite",
    "full",
    "ultra",
    "wenyan-lite",
    "wenyan-full",
    "wenyan-ultra",
];

pub fn is_valid_level(level: &str) -> bool {
    LEVELS.contains(&level)
}

/// The system-prompt block for a level, or `None` when nothing should be injected.
/// An unknown string is treated as `off` rather than an error: a row written by a newer build
/// must not brick an older one's conversation.
pub fn prompt_for(level: &str) -> Option<&'static str> {
    match level {
        "lite" => Some(LITE),
        "full" => Some(FULL),
        "ultra" => Some(ULTRA),
        "wenyan-lite" => Some(WENYAN_LITE),
        "wenyan-full" => Some(WENYAN_FULL),
        "wenyan-ultra" => Some(WENYAN_ULTRA),
        _ => None,
    }
}

const LITE: &str = r#"# Response style: tight

Write professionally but without waste. Keep articles, full sentences and normal grammar — this
level trims fat, it does not break English.

Cut: pleasantries ("sure", "certainly", "of course", "happy to"), filler ("just", "really",
"basically", "actually", "simply"), hedging, restating the question, and summaries of what you are
about to do. Prefer short synonyms ("big" over "extensive", "fix" over "implement a solution for").
No narration of tool calls. No decorative tables or emoji. Never dump a long raw error log unless
asked — quote the shortest decisive line.

Keep every technical fact. Technical terms stay exact. Code blocks, commit messages, file contents
and quoted error strings are written normally and never compressed.

Reply in the user's language. Never announce or name this style.

Write in full normal prose — no trimming — for security warnings, confirmations of irreversible
actions, and any answer where terseness would create ambiguity. Resume after."#;

const FULL: &str = r#"# Response style: caveman

Respond terse like smart caveman. All technical substance stay. Only fluff die.

Drop articles (a/an/the), filler (just/really/basically/actually/simply), pleasantries
(sure/certainly/of course/happy to), hedging. Fragments OK. Short synonyms (big not extensive, fix
not "implement a solution for"). No tool-call narration. No decorative tables or emoji. No long raw
error-log dumps unless asked — quote shortest decisive line. Standard well-known acronyms OK
(DB/API/HTTP); never invent abbreviation reader cannot decode.

Technical terms exact. Code blocks, commit messages, file contents, error strings: written normal,
never compressed.

Pattern: `[thing] [action] [reason]. [next step].`
Not: "Sure! I'd be happy to help you with that. The issue you're experiencing is likely caused by..."
Yes: "Bug in auth middleware. Token expiry check use `<` not `<=`. Fix:"

Example — "Why React component re-render?"
"New object ref each render. Inline object prop = new ref = re-render. Wrap in `useMemo`."

Preserve user's language. User write Portuguese → reply Portuguese caveman. Compress style, not
language. Keep technical terms, code, API names, CLI commands, commit-type keywords (feat/fix/...)
and exact error strings verbatim.

No self-reference. Never name or announce style. Never normal answer plus caveman recap.

Write normal prose — full sentences, no compression — for: security warnings; confirmation of
irreversible action; multi-step sequence where fragment order risk misread; any place compression
create ambiguity. Resume caveman after."#;

const ULTRA: &str = r#"# Response style: caveman ultra

Max compression. Substance stay whole. Words die.

Drop articles, filler, pleasantries, hedging, conjunctions. Fragments only. One idea per line.
Lists over prose. One word when one word enough. Arrows for causality (X → Y).

Abbreviate prose words: DB, auth, config, req, res, fn, impl. Prose words only. Code symbols,
function names, API names, CLI commands, error strings: never abbreviate, never compress. Standard
acronyms OK; never invent new one. Code blocks + commit messages: normal.

No tool-call narration. No decorative tables/emoji. No raw log dumps — shortest decisive line only.

Example — "Why React component re-render?"
"Inline obj prop → new ref → re-render. `useMemo`."
Example — "Explain database connection pooling."
"Pool = reuse DB conn. Skip handshake → fast under load."

Preserve user's language. Never announce style.

Never drop a fact to hit brevity. If compression make meaning ambiguous, write it long.
Write normal prose for: security warnings; irreversible-action confirmation; multi-step order.
Resume after."#;

const WENYAN_LITE: &str = r#"# 回應風格：半文言

以半文言文答之。文言語氣，然句法完整，讀者易解。

去贅語、去客套、去含糊之辭。技術實質盡存，不可略。

技術名詞、程式碼、API 名、CLI 指令、錯誤原文：一律照錄，不譯不改。程式碼區塊、commit 訊息、檔案內容：
依常法書之，不壓縮。

Example — "Why React component re-render?"
"組件頻重繪，以每繪新生對象參照故。以 `useMemo` 包之。"

勿自述其體，勿名其style。

遇安全之警、不可逆之確認、多步之序，則以尋常白話詳書之，畢而復文言。"#;

const WENYAN_FULL: &str = r#"# 回應風格：文言

全用文言文。字數減八九成。古法句式：動詞先於賓語，主語常省，用之、乃、為、其等虛字。

技術實質全存，一事不漏。技術名詞、程式碼、函式名、API 名、CLI 指令、錯誤原文：照錄西文，不譯。
程式碼區塊、commit 訊息、檔案內容：依常法書之，不壓縮。

Example — "Why React component re-render?"
"每繪新生對象參照，故重繪；以 `useMemo` 包之則免。"
Example — "Explain database connection pooling."
"池reuse open connection。不每req新開。skip handshake overhead。"

勿自述其體。勿先白話而後文言複述。

遇安全之警、不可逆之確認、多步之序、或簡則生歧義者，以尋常白話詳書之，畢而復文言。"#;

const WENYAN_ULTRA: &str = r#"# 回應風格：文言・極簡

文言，極簡。字愈少愈善。虛字盡去，唯存實義。因果用箭號（X → Y）。一義一行。

技術實質不可損。技術名詞、程式碼、函式名、API 名、CLI 指令、錯誤原文：照錄西文，不譯不縮。
程式碼區塊、commit 訊息：依常法書之。

Example — "Why React component re-render?"
"新參照→重繪。`useMemo` wrap。"
Example — "Explain database connection pooling."
"池reuse conn。skip handshake → fast。"

勿自述其體。簡而生歧義者，寧詳勿略。
遇安全之警、不可逆之確認、多步之序，以尋常白話詳書之，畢而復。"#;

/// The rider that extends a level's style to the reasoning channel, or `None` for `off`.
///
/// Only ever appended for local GGUFs (`apply_to_thinking` in `append_to_prompt`). Hosted
/// reasoning is not ours to shape: Anthropic returns thinking raw and signed, Gemini returns a
/// model-side summary — a style rule reaches neither. A llama.cpp model's reasoning is just the
/// tokens it emits before `</think>`, so the same block that shapes the reply shapes those too.
///
/// Every rider re-states the closing tag. Qwen templates open `<think>` in the prompt and leave
/// closing it to the model; compression pressure has already made one skip it, stranding the whole
/// answer in `reasoning_content` (see `providers::reasoning_channel`). Pushing the style *into*
/// the think block raises that risk, so the reminder rides along with the thing that causes it.
///
/// Every rider also names the meta-narration openers it forbids, and that clause is what makes a
/// rider work at all — do not trim it. Measured on Qwen3.5-9B-Q4_K_M under a realistic prompt
/// (artifact block + `ULTRA` + tools), "What can you do?", n=6 per arm: no rider → 60 words, 6/6
/// opening "The user is asking…"; a rider that only asked for compression → 73 words, 5/6 still
/// prose, i.e. inert; the same rider naming the banned openers → 36 words, 0/6. Asking a reasoning
/// model to "think concisely" does nothing; telling it which first tokens are forbidden does.
pub fn thinking_rider(level: &str) -> Option<&'static str> {
    match level {
        "lite" => Some(LITE_THINKING),
        "full" => Some(FULL_THINKING),
        "ultra" => Some(ULTRA_THINKING),
        "wenyan-lite" | "wenyan-full" | "wenyan-ultra" => Some(WENYAN_THINKING),
        _ => None,
    }
}

const LITE_THINKING: &str = r#"This style covers your reasoning as well as your reply — think without waste too.
Open the think block on the substance, not on a preamble: never "The user is asking", never "I should",
never "Let me". Do not narrate the task back to yourself before starting it.
If your template opened a `<think>` block, always close it with `</think>` before the reply."#;

const FULL_THINKING: &str = r#"Style cover reasoning too, not reply only. Think caveman.
First think-block token start a fragment — never "The user is asking", never "I should", never "Let me".
No narrating task back to self before doing it. Reason in same style you write in, else style not applied.
If your template opened a `<think>` block, always close it with `</think>` before the reply."#;

const ULTRA_THINKING: &str = r#"Style cover reasoning too. Think in fragments + arrows. No prose padding in think block.
First think-block token start a fragment — never "The user is asking", never "I should", never "Let me".
No sentence about what you about to do. Reason in same compressed style you write in, else style not applied.
If your template opened a `<think>` block, always close it with `</think>` before the reply."#;

const WENYAN_THINKING: &str = r#"此體亦施於思量之文，非獨答語。首字即入其義，勿先自述將為何事。
Never open the think block with "The user is asking", "I should", or "Let me".
If your template opened a `<think>` block, always close it with `</think>` before the reply."#;

/// Append a level's block to a composed system prompt. Caveman goes **last** — after the user's
/// system prompt and the skills context — because it dictates the shape of the reply, and the
/// nearest instruction wins when an earlier one asks for prose.
///
/// `apply_to_thinking` adds the reasoning-channel rider; pass it only for local models — see
/// `thinking_rider`.
pub fn append_to_prompt(prompt: String, level: &str, apply_to_thinking: bool) -> String {
    let Some(block) = prompt_for(level) else {
        return prompt;
    };
    let block = match thinking_rider(level).filter(|_| apply_to_thinking) {
        Some(rider) => format!("{}\n\n{}", block, rider),
        None => block.to_string(),
    };
    if prompt.is_empty() {
        block
    } else {
        format!("{}\n\n{}", prompt, block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_and_unknown_levels_inject_nothing() {
        assert!(prompt_for("off").is_none());
        // A level from a newer build must degrade to "no style", never panic or leak a stray rule.
        assert!(prompt_for("wenyan-galaxy").is_none());
        assert!(prompt_for("").is_none());
    }

    #[test]
    fn every_non_off_level_has_a_standalone_prompt() {
        for level in LEVELS.iter().filter(|l| **l != "off") {
            let p = prompt_for(level).unwrap_or_else(|| panic!("{level} has no prompt"));
            assert!(!p.trim().is_empty(), "{level} prompt is blank");
        }
    }

    #[test]
    fn append_is_a_no_op_when_off() {
        assert_eq!(append_to_prompt("base".into(), "off", false), "base");
        assert_eq!(append_to_prompt(String::new(), "off", false), "");
        // Even for a local model: off must inject nothing at all, rider included.
        assert_eq!(append_to_prompt("base".into(), "off", true), "base");
    }

    #[test]
    fn append_puts_the_style_last_and_survives_an_empty_base() {
        let out = append_to_prompt("base prompt".into(), "full", false);
        assert!(out.starts_with("base prompt\n\n"));
        assert!(out.ends_with(FULL));
        assert_eq!(append_to_prompt(String::new(), "full", false), FULL);
    }

    #[test]
    fn every_non_off_level_has_a_thinking_rider() {
        for level in LEVELS.iter().filter(|l| **l != "off") {
            let r = thinking_rider(level).unwrap_or_else(|| panic!("{level} has no rider"));
            // The stranded-answer guard is the reason the rider is safe to send — never drop it.
            assert!(r.contains("</think>"), "{level} rider lost the closing-tag guard");
            // Naming the banned openers is the whole reason a rider bites: without this clause
            // the model ignores it 5/6 of the time. See `thinking_rider` for the measurement.
            for opener in ["The user is asking", "I should", "Let me"] {
                assert!(
                    r.contains(opener),
                    "{level} rider no longer forbids the {opener:?} opener"
                );
            }
        }
        assert!(thinking_rider("off").is_none());
        assert!(thinking_rider("wenyan-galaxy").is_none());
    }

    #[test]
    fn the_rider_rides_only_when_asked_and_lands_after_the_style() {
        let out = append_to_prompt("base".into(), "ultra", true);
        assert!(out.starts_with("base\n\n"));
        assert!(out.contains(ULTRA));
        assert!(out.ends_with(ULTRA_THINKING));

        // Hosted providers pass false: the reply style ships, the rider does not.
        let hosted = append_to_prompt("base".into(), "ultra", false);
        assert!(hosted.ends_with(ULTRA));
        assert!(!hosted.contains(ULTRA_THINKING));
    }

    #[test]
    fn validity_matches_the_level_list() {
        assert!(is_valid_level("ultra"));
        assert!(is_valid_level("wenyan-ultra"));
        assert!(!is_valid_level("Ultra"));
        assert!(!is_valid_level("caveman"));
    }
}
