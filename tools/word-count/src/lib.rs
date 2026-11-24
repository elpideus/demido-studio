//! `word_count`: the words, lines and characters of a file in the session's folder. A tool of Demido's own,
//! written to `tools/wit/tool.wit` like any other, to prove the package format.

wit_bindgen::generate!({ world: "tool", path: "../wit" });

use demido::tool::host;

struct WordCount;

impl Guest for WordCount {
    fn run(args: String) -> Result<String, String> {
        let args: serde_json::Value = serde_json::from_str(&args).map_err(|_| "the arguments are not a JSON object".to_string())?;
        let path = args.get("path").and_then(|p| p.as_str()).ok_or("word_count needs \"path\", the file to count")?;
        let text = host::read_file(path)?;
        host::log(&format!("counted {path}"));
        let words = text.split_whitespace().count();
        let lines = text.lines().count();
        let chars = text.chars().count();
        Ok(format!("{path}: {words} words, {lines} lines, {chars} characters."))
    }
}

export!(WordCount);
