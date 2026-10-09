//! What Qwen3-ASR writes, made into the transcript: it starts with the language it heard,
//! `language English<asr_text>What is the price of gold today?`, and with `language None<asr_text>`
//! when it heard no speech.

const TAG: &str = "<asr_text>";
const LANGUAGE: &str = "language";
/// Longest start that can still be `language X<asr_text>`.
const MAX_HEAD: usize = 64;

/// The language and the words of a whole answer.
pub fn clean(raw: &str) -> (Option<String>, String) {
    let mut c = Cleaner::default();
    let mut text = c.push(raw);
    text.push_str(&c.finish());
    (c.language, text.trim().to_string())
}

/// Cleans an answer as it streams: holds back its start until the tag has come, or until it can
/// no longer be the language line (a model that writes no such line), then passes the words on.
#[derive(Default)]
pub struct Cleaner {
    head: String,
    past_head: bool,
    /// Whether any words were passed on yet: the first ones lose their leading spaces.
    started: bool,
    pub language: Option<String>,
}

impl Cleaner {
    /// The words of `delta` that can be shown now.
    pub fn push(&mut self, delta: &str) -> String {
        if self.past_head {
            return self.words(delta);
        }
        self.head.push_str(delta);
        if let Some(at) = self.head.find(TAG) {
            let line = self.head[..at].trim();
            let language = line.strip_prefix(LANGUAGE).unwrap_or(line).trim();
            self.language =
                (!language.is_empty() && !language.eq_ignore_ascii_case("none")).then(|| language.to_string());
            let rest = self.head[at + TAG.len()..].to_string();
            self.head.clear();
            self.past_head = true;
            return self.words(&rest);
        }
        if !could_be_head(&self.head) {
            self.past_head = true;
            let all = std::mem::take(&mut self.head);
            return self.words(&all);
        }
        String::new()
    }

    /// What was held back when the answer ended: a start that never reached the tag is words,
    /// unless it was only the language line.
    pub fn finish(&mut self) -> String {
        if self.past_head {
            return String::new();
        }
        self.past_head = true;
        let head = std::mem::take(&mut self.head);
        let line = head.trim();
        if (line.starts_with(LANGUAGE) && !line.contains(char::is_whitespace)) || is_language_line(line) {
            return String::new();
        }
        self.words(&head)
    }

    fn words(&mut self, s: &str) -> String {
        if self.started {
            return s.to_string();
        }
        let s = s.trim_start();
        self.started = !s.is_empty();
        s.to_string()
    }
}

/// Whether `head` can still turn out to be `language X<asr_text>`: a start of "language", or
/// "language" and a word, then perhaps the start of the tag.
fn could_be_head(head: &str) -> bool {
    let head = head.trim_start();
    if head.len() > MAX_HEAD || head.contains('\n') {
        return false;
    }
    if head.len() <= LANGUAGE.len() {
        return LANGUAGE.starts_with(head);
    }
    let Some(rest) = head.strip_prefix(LANGUAGE) else {
        return false;
    };
    // " English" and then some of "<asr_text>".
    let rest = rest.trim_start();
    match rest.find('<') {
        Some(at) => !rest[..at].contains(char::is_whitespace) && TAG.starts_with(&rest[at..]),
        None => !rest.contains(char::is_whitespace),
    }
}

/// `language English`, and nothing else.
fn is_language_line(line: &str) -> bool {
    line.strip_prefix(LANGUAGE)
        .map(str::trim)
        .is_some_and(|l| !l.is_empty() && !l.contains(char::is_whitespace))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_language_line_is_taken_off() {
        assert_eq!(
            clean("language English<asr_text>What is the price of gold today?"),
            (Some("English".into()), "What is the price of gold today?".into())
        );
        assert_eq!(clean("language None<asr_text>"), (None, String::new()), "silence");
        assert_eq!(
            clean("language Chinese<asr_text> 今天金价多少？\n"),
            (Some("Chinese".into()), "今天金价多少？".into())
        );
        // A model that writes the words alone.
        assert_eq!(clean("Hello there."), (None, "Hello there.".into()));
        assert_eq!(clean("language is hard"), (None, "language is hard".into()));
    }

    #[test]
    fn streamed_words_appear_once_the_tag_has_come() {
        let mut c = Cleaner::default();
        let shown: Vec<String> = ["language", " English", "<asr", "_text>", "What", " is", " gold", "?"]
            .iter()
            .map(|d| c.push(d))
            .collect();
        assert_eq!(shown, ["", "", "", "", "What", " is", " gold", "?"]);
        assert_eq!(c.finish(), "");
        assert_eq!(c.language.as_deref(), Some("English"));

        // Words that never were a language line are held back no longer than it takes to tell.
        let mut c = Cleaner::default();
        assert_eq!(c.push("lang"), "");
        assert_eq!(c.push("uage models are"), "language models are");
        assert_eq!(c.push(" big"), " big");

        let mut c = Cleaner::default();
        assert_eq!(c.push("Hi"), "Hi");

        // A silent clip, streamed.
        let mut c = Cleaner::default();
        assert_eq!(c.push("language None"), "");
        assert_eq!(c.push("<asr_text>"), "");
        assert_eq!(c.finish(), "");
        assert_eq!(c.language, None);

        // An answer cut off after its language line.
        let mut c = Cleaner::default();
        assert_eq!(c.push("language English"), "");
        assert_eq!(c.finish(), "");
    }
}
