//! Slash commands: a message that starts with `/name` runs a command instead of going to the
//! model as typed.
//!
//! The app's own commands act on the chat or the settings: `/compact` summarizes the chat so far
//! and `/autocompact` sets when that happens by itself. A skill's commands (from its
//! `commands.json`, see `skills`) are prompt templates: the message the model reads is the
//! template with the arguments filled in, with the skill's instructions in front when the system
//! prompt does not carry them. Commands are never listed in the system prompt, so they cost no
//! context until someone uses one.

use std::sync::Arc;

use serde::Serialize;

use crate::agent::{Agent, compact, prompt};
use crate::bail_msg;
use crate::db::{Chat, CommandUse, Message};
use crate::error::CmdResult;
use crate::models::ModelEntry;
use crate::settings::Settings;
use crate::skills::{Skill, SkillCommand, SkillRegistry};
use crate::state::AppState;

/// The highest threshold `/autocompact` takes: no model reads more.
const MAX_TOKENS: u64 = 10_000_000;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SlashCommand {
    /// What follows the slash.
    pub name: String,
    pub description: String,
    /// What to type after the name, such as `<symbol> [timeframe]`.
    pub args: Option<String>,
    /// The skill that provides it, by id; `None` for the app's own.
    pub skill: Option<String>,
    pub skill_name: Option<String>,
}

/// The app's own commands: name, arguments, description.
const BUILT_IN: &[(&str, &str, &str)] = &[
    (
        "compact",
        "[what to keep]",
        "Summarize the conversation so far, freeing up the model's context",
    ),
    (
        "autocompact",
        "<tokens | off | auto>",
        "When to compact by itself, such as 12k, 12.5k or 12 thousand and a half",
    ),
];

/// Every command there is: the app's own, then those of the enabled skills. A skill command
/// whose name is taken is listed as `skill-id:name`.
fn entries(skills: &SkillRegistry) -> Vec<(SlashCommand, Option<(Skill, SkillCommand)>)> {
    let mut out: Vec<(SlashCommand, Option<(Skill, SkillCommand)>)> = BUILT_IN
        .iter()
        .map(|(name, args, description)| {
            let command = SlashCommand {
                name: name.to_string(),
                description: description.to_string(),
                args: Some(args.to_string()),
                skill: None,
                skill_name: None,
            };
            (command, None)
        })
        .collect();
    for skill in skills.list().into_iter().filter(|s| s.enabled && s.problem.is_none()) {
        for c in &skill.commands {
            let mut name = c.name.clone();
            if out.iter().any(|(o, _)| o.name == name) {
                name = format!("{}:{}", crate::skills::slug(&skill.id), c.name);
            }
            if out.iter().any(|(o, _)| o.name == name) {
                continue;
            }
            let command = SlashCommand {
                name,
                description: if c.description.is_empty() {
                    skill.description.clone()
                } else {
                    c.description.clone()
                },
                args: c.args.clone(),
                skill: Some(skill.id.clone()),
                skill_name: Some(skill.name.clone()),
            };
            out.push((command, Some((skill.clone(), c.clone()))));
        }
    }
    out
}

pub fn list(skills: &SkillRegistry) -> Vec<SlashCommand> {
    entries(skills).into_iter().map(|(c, _)| c).collect()
}

/// A command line split into its name (lowercase, without the slash) and what follows it.
pub fn split(text: &str) -> Option<(String, &str)> {
    let rest = text.trim_start().strip_prefix('/')?;
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let name = &rest[..end];
    (!name.is_empty()).then(|| (name.to_lowercase(), rest[end..].trim()))
}

/// What a command did, for the UI.
// Made once per command and serialized straight away; boxing would only add noise.
#[allow(clippy::large_enum_variant)]
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Outcome {
    /// It ran; `text` says what it changed or found.
    Done {
        title: String,
        text: String,
        settings: Option<Settings>,
    },
    /// It sent a message, as `send_message` does.
    Sent { chat: Chat, message: Message },
    /// It started work in the chat, which streams in like a turn.
    Started,
}

/// Runs the command `text` in `chat_id` (`None` for a chat not started yet) with the model
/// picked in the composer and the files staged in it.
pub fn run(
    state: &Arc<AppState>,
    chat_id: Option<String>,
    text: &str,
    model_id: String,
    attachment_ids: Vec<String>,
) -> CmdResult<Outcome> {
    let Some((name, args)) = split(text) else {
        bail_msg!("A command starts with /, such as /compact.");
    };
    match name.as_str() {
        "compact" => {
            let Some(chat_id) = chat_id else {
                bail_msg!("There is nothing to compact yet.");
            };
            Agent::compact(
                state,
                &chat_id,
                model_id,
                Some(args.to_string()).filter(|a| !a.is_empty()),
            )?;
            Ok(Outcome::Started)
        }
        "autocompact" => autocompact(state, args, &model_id),
        _ => {
            let Some((command, Some((skill, def)))) = entries(&state.skills).into_iter().find(|(c, _)| c.name == name)
            else {
                bail_msg!("There is no /{name} command. Type / to see the commands there are.");
            };
            let context = state
                .models
                .get(&model_id)
                .map_or(32_768, |m| crate::agent::model_context(&m));
            let inline = !state
                .skills
                .instructions_in_prompt(prompt::skills_budget_chars(context));
            let content = expand(&skill, &def, args, inline);
            let used = CommandUse {
                name: command.name,
                args: args.to_string(),
                skill: Some(skill.id),
            };
            let sent = Agent::send(state, chat_id, content, model_id, attachment_ids, Some(used))?;
            Ok(Outcome::Sent {
                chat: sent.chat,
                message: sent.message,
            })
        }
    }
}

/// `/autocompact`: shows the threshold, turns compaction on or off, or sets the tokens at which
/// it happens.
fn autocompact(state: &Arc<AppState>, args: &str, model_id: &str) -> CmdResult<Outcome> {
    let model = state.models.get(model_id);
    let word = args.trim().to_lowercase();
    let settings = match word.trim_end_matches(['.', '!']) {
        "" => {
            return Ok(Outcome::Done {
                title: "Auto-compact".into(),
                text: describe(&state.settings.get(), model.as_ref()),
                settings: None,
            });
        }
        "off" | "disable" | "disabled" | "never" | "no" | "false" | "stop" => {
            state.settings.update(|s| s.auto_compact = false)?
        }
        "on" | "enable" | "enabled" | "yes" | "true" | "start" => state.settings.update(|s| s.auto_compact = true)?,
        "auto" | "automatic" | "default" | "reset" | "clear" => state.settings.update(|s| {
            s.auto_compact = true;
            s.auto_compact_tokens = None;
        })?,
        _ => {
            let Some(tokens) = parse_tokens(&word) else {
                bail_msg!(
                    "\"{}\" is not a number of tokens. Try 12000, 12k, 12.5k or 12 thousand and a half, or off or auto.",
                    args.trim()
                );
            };
            if tokens < compact::MIN_TOKENS as u64 {
                bail_msg!(
                    "Compaction needs a threshold of at least {} tokens.",
                    group(compact::MIN_TOKENS as u64)
                );
            }
            if tokens > MAX_TOKENS {
                bail_msg!("{} tokens is more than any model reads.", group(tokens));
            }
            state.settings.update(|s| {
                s.auto_compact = true;
                s.auto_compact_tokens = Some(tokens as u32);
            })?
        }
    };
    Ok(Outcome::Done {
        title: "Auto-compact".into(),
        text: describe(&settings, model.as_ref()),
        settings: Some(settings),
    })
}

/// When compaction happens under `settings`, and what that means for `model`.
fn describe(settings: &Settings, model: Option<&ModelEntry>) -> String {
    if !settings.auto_compact {
        return "Off. Chats are compacted only when you type /compact.".into();
    }
    let share = (compact::DEFAULT_SHARE * 100.0).round();
    let mut text = match settings.auto_compact_tokens {
        Some(t) => format!("On, at {} tokens.", group(t as u64)),
        None => format!("On, at {share}% of the room the model's context window leaves."),
    };
    if let Some(m) = model {
        let at = compact::threshold_for(m, settings.auto_compact_tokens) as u64;
        match settings.auto_compact_tokens {
            Some(t) if t as u64 > at => text.push_str(&format!(
                " {} has less room than that, so it compacts at {} tokens.",
                m.name,
                group(at)
            )),
            None => text.push_str(&format!(" For {}, that is about {} tokens.", m.name, group(at))),
            _ => {}
        }
    }
    text
}

/// `n` with thousands separators: 12,300.
fn group(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The message a skill command sends: its prompt with the arguments filled in, with the skill's
/// instructions in front when the system prompt does not hold them (`with_instructions`), or a
/// pointer to the skill when the prompt does not name it.
pub fn expand(skill: &Skill, command: &SkillCommand, args: &str, with_instructions: bool) -> String {
    let prompt = fill(&command.prompt, args);
    if with_instructions {
        let mut s = format!("<skill name=\"{}\">\n{}\n", skill.name, skill.body.trim());
        let files = skill.other_files();
        if !files.is_empty() {
            s.push_str(&format!(
                "Files (read them with read_skill_file, skill id {}): {}\n",
                skill.id,
                files.join(", ")
            ));
        }
        s.push_str("</skill>\n\n");
        s.push_str(&prompt);
        s
    } else if prompt.to_lowercase().contains(&skill.name.to_lowercase()) {
        prompt
    } else {
        format!("{prompt}\n\nFollow the {} skill.", skill.name)
    }
}

/// `template` with `$ARGUMENTS` replaced by `args` and `$1` to `$9` by its words (a "quoted
/// phrase" is one word); `$$` is a dollar sign. Arguments the template does not place are added
/// at its end, so none are lost.
pub fn fill(template: &str, args: &str) -> String {
    let args = args.trim();
    let words = split_words(args);
    let mut out = String::new();
    let mut placed = false;
    let mut rest = template;
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        if let Some(tail) = after.strip_prefix("ARGUMENTS") {
            out.push_str(args);
            placed = true;
            rest = tail;
        } else if let Some(n) = after.chars().next().and_then(|c| c.to_digit(10)).filter(|&n| n > 0) {
            out.push_str(words.get(n as usize - 1).map_or("", String::as_str));
            placed = true;
            rest = &after[1..];
        } else if let Some(tail) = after.strip_prefix('$') {
            out.push('$');
            rest = tail;
        } else {
            out.push('$');
            rest = after;
        }
    }
    out.push_str(rest);
    let mut out = out.trim().to_string();
    if !placed && !args.is_empty() {
        out.push_str("\n\n");
        out.push_str(args);
    }
    out
}

/// Words split at spaces, a "quoted phrase" counting as one.
fn split_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => {
                // A closing quote ends its phrase, even an empty one.
                if quoted || !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
                quoted = !quoted;
            }
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// Reads a number of tokens the way people write one: `12000`, `12,000`, `12.000`, `12 000`,
/// `12k`, `12.3k`, `12,5k`, `12k5`, `1.5m`, `twelve thousand`, `12 thousand and a half`,
/// `twelve and a half thousand`, `half a million`, `two hundred and fifty thousand`,
/// `twelve point five thousand`. Words such as "tokens" or "about" around it are ignored.
pub fn parse_tokens(text: &str) -> Option<u64> {
    let lowered = text.trim().to_lowercase();
    let tokens = lex(&lowered)?;
    let value = evaluate(&tokens)?;
    (value.is_finite() && value >= 0.0).then(|| value.round() as u64)
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    /// Digits as written, with their separators.
    Digits(String),
    Num(f64),
    Word(String),
}

/// Characters that group digits in thousands: `12 000`, `12'000`, `12_000`.
fn is_group_mark(c: char) -> bool {
    matches!(c, ' ' | '\'' | '’' | '_' | '\u{a0}' | '\u{202f}')
}

fn lex(text: &str) -> Option<Vec<Tok>> {
    let chars: Vec<char> = text.chars().collect();
    // Each token with where it starts and ends.
    let mut out: Vec<(Tok, usize, usize)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        let digit_at = |j: usize| chars.get(j).is_some_and(|d| d.is_ascii_digit());
        if c.is_ascii_digit() || (c == '.' && digit_at(i + 1)) {
            while i < chars.len() {
                let c = chars[i];
                let separator = (c == '.' || c == ',') && digit_at(i + 1);
                if c.is_ascii_digit() || separator || (is_group_mark(c) && three_digits_at(&chars, i + 1)) {
                    i += 1;
                } else {
                    break;
                }
            }
            let raw: String = chars[start..i].iter().collect();
            // `12k5` is 12.5k: digits right after a scale letter right after digits are its decimals.
            if let [
                ..,
                (Tok::Digits(before), _, before_end),
                (Tok::Word(scale), scale_start, scale_end),
            ] = &mut out[..]
                && *before_end == *scale_start
                && *scale_end == start
                && matches!(scale.as_str(), "k" | "m" | "b")
                && before.chars().all(|c| c.is_ascii_digit())
                && raw.chars().all(|c| c.is_ascii_digit())
            {
                before.push('.');
                before.push_str(&raw);
                continue;
            }
            out.push((Tok::Digits(raw), start, i));
        } else if c.is_alphabetic() {
            while i < chars.len() && chars[i].is_alphabetic() {
                i += 1;
            }
            out.push((Tok::Word(chars[start..i].iter().collect()), start, i));
        } else if c.is_whitespace() || matches!(c, '-' | '~' | '≈' | '.' | ',' | '!' | '?' | '(' | ')') {
            i += 1;
        } else {
            return None;
        }
    }
    // Digits are read once it is known whether a scale follows them: `1.500` is 1,500, but
    // `1.500k` is 1.5k.
    out.iter()
        .enumerate()
        .map(|(n, (t, _, _))| match t {
            Tok::Digits(raw) => {
                let scaled = matches!(out.get(n + 1), Some((Tok::Word(w), _, _)) if scale_of(w).is_some());
                number(raw, scaled).map(Tok::Num)
            }
            other => Some(other.clone()),
        })
        .collect()
}

/// Whether `chars` holds exactly three digits from `at`.
fn three_digits_at(chars: &[char], at: usize) -> bool {
    (at..at + 3).all(|j| chars.get(j).is_some_and(|c| c.is_ascii_digit()))
        && !chars.get(at + 3).is_some_and(|c| c.is_ascii_digit())
}

/// Digits with their separators as a number. A comma or a point before a group of exactly three
/// digits groups thousands, unless a scale word follows (`1.5k`); a single one before fewer
/// digits is a decimal point (`12,5`). With both, the last one is the decimal point.
fn number(raw: &str, scaled: bool) -> Option<f64> {
    let digits: String = raw.chars().filter(|&c| !is_group_mark(c)).collect();
    let groups_of_three = |sep: char| digits.split(sep).skip(1).all(|g| g.len() == 3);
    let (commas, points) = (digits.matches(',').count(), digits.matches('.').count());
    let plain = match (commas, points) {
        (0, 0) => digits,
        (_, 0) | (0, _) => {
            let sep = if commas > 0 { ',' } else { '.' };
            let count = commas + points;
            if count > 1 || (groups_of_three(sep) && !scaled) {
                if !groups_of_three(sep) {
                    return None;
                }
                digits.replace(sep, "")
            } else {
                digits.replace(sep, ".")
            }
        }
        _ => {
            let decimal = if digits.rfind(',') > digits.rfind('.') {
                ','
            } else {
                '.'
            };
            let group = if decimal == ',' { '.' } else { ',' };
            let (whole, fraction) = digits.rsplit_once(decimal)?;
            if whole.contains(decimal) || fraction.contains(group) {
                return None;
            }
            format!("{}.{fraction}", whole.replace(group, ""))
        }
    };
    plain.parse().ok()
}

fn scale_of(word: &str) -> Option<f64> {
    Some(match word {
        "k" | "thousand" | "thousands" | "grand" => 1e3,
        "m" | "mil" | "mio" | "mln" | "mn" | "million" | "millions" => 1e6,
        "b" | "bn" | "billion" | "billions" => 1e9,
        _ => return None,
    })
}

fn unit_of(word: &str) -> Option<f64> {
    const UNITS: [&str; 20] = [
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
    ];
    const TENS: [&str; 8] = [
        "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];
    if let Some(n) = UNITS.iter().position(|&u| u == word) {
        return Some(n as f64);
    }
    TENS.iter().position(|&t| t == word).map(|n| (n as f64 + 2.0) * 10.0)
}

fn fraction_of(word: &str) -> Option<f64> {
    match word {
        "half" | "halves" => Some(0.5),
        "quarter" | "quarters" => Some(0.25),
        _ => None,
    }
}

/// Words that may surround a number without changing it.
fn is_filler(word: &str) -> bool {
    matches!(
        word,
        "tokens"
            | "token"
            | "toks"
            | "tok"
            | "of"
            | "about"
            | "around"
            | "roughly"
            | "approximately"
            | "approx"
            | "at"
            | "some"
    )
}

/// The value of number words and digits read in order, as in "two hundred and fifty thousand".
fn evaluate(tokens: &[Tok]) -> Option<f64> {
    // Finished groups ("12 thousand"), the group being read, and the scale that closed the last.
    let mut total = 0.0f64;
    let mut current = 0.0f64;
    let mut have_current = false;
    let mut last_scale = 1.0f64;
    let mut seen = false;
    let mut after_and = false;
    let mut after_digits = false;
    // After "point": the place of the next decimal digit.
    let mut decimal_place: Option<f64> = None;

    for (n, t) in tokens.iter().enumerate() {
        let word = match t {
            Tok::Num(v) => {
                // "12 5" is not a number; "12 thousand 500" is.
                if after_digits || decimal_place.is_some() {
                    return None;
                }
                current += v;
                have_current = true;
                seen = true;
                after_digits = true;
                after_and = false;
                continue;
            }
            Tok::Word(w) => w.as_str(),
            Tok::Digits(_) => return None,
        };
        let next_scales =
            |from: usize| matches!(tokens.get(from), Some(Tok::Word(w)) if scale_of(w).is_some() || w == "hundred");
        match word {
            "and" => {
                after_and = true;
                continue;
            }
            // "a thousand" is one thousand; the "a" of "and a half" or "half a million" is nothing.
            "a" | "an" => {
                if !have_current && next_scales(n + 1) {
                    current = 1.0;
                    have_current = true;
                    seen = true;
                }
                continue;
            }
            "point" if have_current && decimal_place.is_none() => decimal_place = Some(0.1),
            w if is_filler(w) => continue,
            "hundred" | "hundreds" => {
                current = if have_current { current } else { 1.0 } * 100.0;
                have_current = true;
                seen = true;
                decimal_place = None;
            }
            w if scale_of(w).is_some() => {
                let scale = scale_of(w)?;
                // "thousand" alone is a thousand, but "k" alone is no number.
                if !have_current && w.len() <= 3 {
                    return None;
                }
                let group = if have_current { current } else { 1.0 };
                if scale > last_scale && total > 0.0 {
                    // "two thousand million"
                    total = (total + group) * scale;
                } else {
                    total += group * scale;
                }
                last_scale = scale;
                current = 0.0;
                have_current = false;
                seen = true;
                decimal_place = None;
            }
            w if fraction_of(w).is_some() => {
                let f = fraction_of(w)?;
                if after_and {
                    if have_current {
                        // "12 and a half thousand"
                        current += f;
                    } else if total > 0.0 {
                        // "12 thousand and a half"
                        total += f * last_scale;
                    } else {
                        return None;
                    }
                } else if have_current {
                    // "three quarters of a million"
                    current *= f;
                } else {
                    // "half a million"
                    current = f;
                    have_current = true;
                }
                seen = true;
            }
            w => {
                let v = unit_of(w)?;
                match decimal_place {
                    Some(place) => {
                        if v > 9.0 {
                            return None;
                        }
                        current += v * place;
                        decimal_place = Some(place / 10.0);
                    }
                    None => current += v,
                }
                have_current = true;
                seen = true;
            }
        }
        after_and = false;
        after_digits = false;
    }
    seen.then_some(total + current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_counts_are_read_however_they_are_written() {
        let cases: &[(&str, u64)] = &[
            ("12000", 12_000),
            ("12,000", 12_000),
            ("12.000", 12_000),
            ("12 000", 12_000),
            ("12'000", 12_000),
            ("1,234,567", 1_234_567),
            ("1.234.567", 1_234_567),
            ("1.234,5", 1_235),
            ("12k", 12_000),
            ("12K", 12_000),
            ("12 k", 12_000),
            ("12.3k", 12_300),
            ("12,3k", 12_300),
            ("1.500k", 1_500),
            ("12k5", 12_500),
            ("1k2", 1_200),
            (".5m", 500_000),
            ("1.5m", 1_500_000),
            ("1.5 million", 1_500_000),
            ("12 thousand", 12_000),
            ("12 thousand tokens", 12_000),
            ("about 12k tokens", 12_000),
            ("~12k", 12_000),
            ("twelve thousand", 12_000),
            ("12 thousand and a half", 12_500),
            ("12k and a half", 12_500),
            ("twelve thousand and a half", 12_500),
            ("12 and a half thousand", 12_500),
            ("twelve and a half k", 12_500),
            ("a thousand and a half", 1_500),
            ("one and a half million", 1_500_000),
            ("half a million", 500_000),
            ("a quarter million", 250_000),
            ("three quarters of a million", 750_000),
            ("twenty-five thousand", 25_000),
            ("two hundred and fifty thousand", 250_000),
            ("one hundred twenty thousand", 120_000),
            ("twelve thousand five hundred", 12_500),
            ("12 thousand 500", 12_500),
            ("25 hundred", 2_500),
            ("one million two hundred thousand", 1_200_000),
            ("twelve point five thousand", 12_500),
            ("a thousand", 1_000),
            ("10 grand", 10_000),
            ("12.5 thousand", 12_500),
            ("12k.", 12_000),
            ("thousand", 1_000),
        ];
        for (text, want) in cases {
            assert_eq!(parse_tokens(text), Some(*want), "{text}");
        }
    }

    #[test]
    fn what_is_not_a_number_is_refused() {
        for text in ["", "lots", "12 apples", "12 5", "1,23,4", "12kb", "and a half", "50%", "k"] {
            assert_eq!(parse_tokens(text), None, "{text}");
        }
    }

    #[test]
    fn command_lines_split_into_name_and_arguments() {
        assert_eq!(split("/compact"), Some(("compact".into(), "")));
        assert_eq!(split("  /AutoCompact   12k  "), Some(("autocompact".into(), "12k")));
        assert_eq!(
            split("/compact keep the\nprices"),
            Some(("compact".into(), "keep the\nprices"))
        );
        assert_eq!(split("/"), None);
        assert_eq!(split("hello /compact"), None);
    }

    #[test]
    fn templates_take_their_arguments() {
        assert_eq!(
            fill("Analyze $ARGUMENTS now.", " FX:EURUSD 1d "),
            "Analyze FX:EURUSD 1d now."
        );
        assert_eq!(
            fill("Compare $1 with $2.", "gold \"the S&P 500\""),
            "Compare gold with the S&P 500."
        );
        assert_eq!(fill("Costs $$5, missing: [$3]", "a"), "Costs $5, missing: []");
        assert_eq!(fill("Analyze the market.", "EURUSD"), "Analyze the market.\n\nEURUSD");
        assert_eq!(fill("Analyze $ARGUMENTS", ""), "Analyze");
        assert_eq!(fill("$", ""), "$");
    }

    #[test]
    fn skill_commands_bring_their_skill_along() {
        let skill = Skill {
            id: "market-analysis".into(),
            name: "Market analysis".into(),
            description: "Prices".into(),
            enabled: true,
            folder: String::new(),
            files: vec![
                crate::skills::SkillFile {
                    path: "SKILL.md".into(),
                    size: 1,
                },
                crate::skills::SkillFile {
                    path: "commands.json".into(),
                    size: 1,
                },
                crate::skills::SkillFile {
                    path: "vol.py".into(),
                    size: 1,
                },
            ],
            body: "1. Find the symbol.".into(),
            updated_at: 0,
            author: None,
            problem: None,
            commands: Vec::new(),
            commands_problem: None,
        };
        let command = SkillCommand {
            name: "analyze".into(),
            description: String::new(),
            args: None,
            prompt: "Analyze $ARGUMENTS.".into(),
        };
        assert_eq!(
            expand(&skill, &command, "gold", false),
            "Analyze gold.\n\nFollow the Market analysis skill."
        );
        let inline = expand(&skill, &command, "gold", true);
        assert_eq!(
            inline,
            "<skill name=\"Market analysis\">\n1. Find the symbol.\nFiles (read them with read_skill_file, skill id market-analysis): vol.py\n</skill>\n\nAnalyze gold."
        );
        let named = SkillCommand {
            prompt: "Use the market analysis skill on $1.".into(),
            ..command
        };
        assert_eq!(
            expand(&skill, &named, "gold", false),
            "Use the market analysis skill on gold."
        );
    }

    #[test]
    fn thresholds_read_well() {
        assert_eq!(group(0), "0");
        assert_eq!(group(999), "999");
        assert_eq!(group(12_300), "12,300");
        assert_eq!(group(1_234_567), "1,234,567");
    }
}
