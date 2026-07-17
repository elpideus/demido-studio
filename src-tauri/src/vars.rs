//! `${VAR}` interpolation for the composed system prompt.
//!
//! Applied once, in `commands.rs`, to `effective_prompt` — that string is already system prompt +
//! skills context, so one call covers both. Deliberately *not* applied to tool results or to files
//! the model reads: `read_file` on SKILL.md must return the bytes on disk, or the same path would
//! read differently depending on who opened it.
//!
//! Rules match the `$name` substitution skill commands already use (`src/stores/skills.ts`):
//! unknown `${FOO}` is left verbatim (a prompt that talks *about* `${FOO}` keeps working), and
//! `\${FOO}` escapes to a literal `${FOO}`.

use chrono::Local;

/// Values that vary per message. Built at the send_message call site, where provider/model/folder
/// are already resolved; time fields are read here so every var in one prompt shares a timestamp.
pub struct VarContext {
    pub working_dir: Option<String>,
    pub provider_id: String,
    pub model_id: String,
}

impl VarContext {
    fn lookup(&self, name: &str) -> Option<String> {
        let now = Local::now();
        Some(match name {
            "CURRENT_DATE" => now.format("%Y-%m-%d").to_string(),
            "CURRENT_TIME" => now.format("%H:%M").to_string(),
            "CURRENT_DATETIME" => now.format("%Y-%m-%d %H:%M:%S %:z").to_string(),
            "CURRENT_YEAR" => now.format("%Y").to_string(),
            "CURRENT_WEEKDAY" => now.format("%A").to_string(),
            "OS" => std::env::consts::OS.to_string(),
            "WORKING_DIR" => self.working_dir.clone().unwrap_or_else(|| "(none set)".into()),
            "PROVIDER_ID" => self.provider_id.clone(),
            "MODEL_ID" => self.model_id.clone(),
            _ => return None,
        })
    }
}

/// Names a prompt author can use, for the Settings hint list. Keep in sync with `lookup`.
pub const KNOWN_VARS: &[&str] = &[
    "CURRENT_DATE",
    "CURRENT_TIME",
    "CURRENT_DATETIME",
    "CURRENT_YEAR",
    "CURRENT_WEEKDAY",
    "OS",
    "WORKING_DIR",
    "PROVIDER_ID",
    "MODEL_ID",
];

pub fn expand(text: &str, ctx: &VarContext) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        // `\${` -> literal `${`, so a prompt can document the syntax without triggering it.
        if bytes[i] == b'\\' && bytes[i + 1..].starts_with(b"${") {
            out.push_str("${");
            i += 3;
            continue;
        }
        if !bytes[i..].starts_with(b"${") {
            let ch = text[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        match text[i + 2..].find('}') {
            Some(rel) => {
                let name = &text[i + 2..i + 2 + rel];
                match ctx.lookup(name) {
                    Some(v) => out.push_str(&v),
                    None => out.push_str(&text[i..i + 3 + rel]),
                }
                i += 3 + rel;
            }
            // Unclosed `${` — emit the rest untouched rather than scanning forever.
            None => {
                out.push_str(&text[i..]);
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> VarContext {
        VarContext {
            working_dir: Some("S:\\proj".into()),
            provider_id: "anthropic".into(),
            model_id: "claude-opus-4-8".into(),
        }
    }

    #[test]
    fn substitutes_known_vars() {
        let out = expand("model=${MODEL_ID} dir=${WORKING_DIR}", &ctx());
        assert_eq!(out, "model=claude-opus-4-8 dir=S:\\proj");
    }

    #[test]
    fn current_date_is_iso() {
        let out = expand("${CURRENT_DATE}", &ctx());
        assert_eq!(out.len(), 10);
        assert_eq!(out.matches('-').count(), 2);
    }

    #[test]
    fn unknown_var_left_verbatim() {
        assert_eq!(expand("cost ${PRICE} and ${MODEL_ID}", &ctx()), "cost ${PRICE} and claude-opus-4-8");
    }

    #[test]
    fn backslash_escapes() {
        assert_eq!(expand("write \\${CURRENT_DATE} to insert date", &ctx()), "write ${CURRENT_DATE} to insert date");
    }

    #[test]
    fn unclosed_brace_is_not_an_error() {
        assert_eq!(expand("${CURRENT_DATE unclosed", &ctx()), "${CURRENT_DATE unclosed");
    }

    #[test]
    fn no_working_dir_reads_as_none_set() {
        let c = VarContext { working_dir: None, ..ctx() };
        assert_eq!(expand("${WORKING_DIR}", &c), "(none set)");
    }

    #[test]
    fn non_ascii_text_survives() {
        assert!(expand("日期 ${CURRENT_YEAR} 年", &ctx()).contains("日期"));
    }

    #[test]
    fn known_vars_list_matches_lookup() {
        for name in KNOWN_VARS {
            assert!(ctx().lookup(name).is_some(), "{name} advertised but not resolved");
        }
    }
}
