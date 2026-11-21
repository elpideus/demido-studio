//! What a site asks of programs: its `robots.txt`, read as RFC 9309 says.
//!
//! Demido reads one page because a person's model asked for it. That is closer to a browser than to a
//! crawler, and some makers of such readers say the file does not bind them. Demido follows it all the same:
//! it costs one small request a day for each site, and a site that says "not for programs" has said it.
//!
//! The rules that apply are those written for `DemidoStudio`, and where there are none, those for every
//! program (`*`). Of the rules that match a path, the longest decides; when an allowing and a refusing one
//! are as long as each other, the page may be read.

/// The name Demido's rules are looked up under.
pub const NAME: &str = "demidostudio";

#[derive(Clone, Debug, PartialEq)]
struct Rule {
    allows: bool,
    path: String,
}

/// The rules of one site that apply to Demido.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Robots {
    rules: Vec<Rule>,
}

/// Whether a rule's path matches a page's. `*` stands for anything, `$` at the end for "ends here".
fn matches(pattern: &str, path: &str) -> bool {
    let (pattern, to_end) = match pattern.strip_suffix('$') {
        Some(p) => (p, true),
        None => (pattern, false),
    };
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    if !path.starts_with(first) {
        return false;
    }
    let mut at = first.len();
    let rest: Vec<&str> = parts.collect();
    for (i, part) in rest.iter().enumerate() {
        let last = i + 1 == rest.len();
        if last && to_end {
            return path[at..].ends_with(part);
        }
        match path[at..].find(part) {
            Some(found) => at += found + part.len(),
            None => return false,
        }
    }
    !to_end || at == path.len()
}

impl Robots {
    /// Reads a `robots.txt`. What cannot be understood is skipped, as the RFC asks.
    pub fn read(text: &str) -> Self {
        // The groups: the names each is for, and its rules. Several "User-agent" lines in a row share a group.
        let mut groups: Vec<(Vec<String>, Vec<Rule>)> = Vec::new();
        let mut naming = false;
        for line in text.lines().take(5000) {
            let line = line.split('#').next().unwrap_or_default().trim();
            let Some((key, value)) = line.split_once(':') else { continue };
            let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());
            match key.as_str() {
                "user-agent" => {
                    if !naming {
                        groups.push((Vec::new(), Vec::new()));
                    }
                    groups.last_mut().expect("just pushed").0.push(value.to_ascii_lowercase());
                    naming = true;
                }
                "allow" | "disallow" => {
                    naming = false;
                    // An empty "Disallow:" refuses nothing.
                    if let Some(group) = groups.last_mut()
                        && !value.is_empty()
                    {
                        group.1.push(Rule { allows: key == "allow", path: value.to_string() });
                    }
                }
                _ => naming = false,
            }
        }
        let for_name = |wanted: &dyn Fn(&str) -> bool| -> Option<Vec<Rule>> {
            let found: Vec<Rule> = groups.iter().filter(|(names, _)| names.iter().any(|n| wanted(n))).flat_map(|(_, rules)| rules.clone()).collect();
            groups.iter().any(|(names, _)| names.iter().any(|n| wanted(n))).then_some(found)
        };
        let rules = for_name(&|n| n == NAME).or_else(|| for_name(&|n| n == "*")).unwrap_or_default();
        Self { rules }
    }

    /// Whether the page at this path (with what follows its `?`) may be read.
    pub fn allows(&self, path: &str) -> bool {
        let mut best: Option<&Rule> = None;
        for rule in self.rules.iter().filter(|r| matches(&r.path, path)) {
            let longer = best.is_none_or(|b| rule.path.len() > b.path.len() || (rule.path.len() == b.path.len() && rule.allows));
            if longer {
                best = Some(rule);
            }
        }
        best.is_none_or(|rule| rule.allows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rules_for_every_program_apply_and_the_longest_decides() {
        // DuckDuckGo's own file for its main site, shortened: read on 2026-11-21.
        let robots = Robots::read("User-agent: *\nDisallow: /lite\nDisallow: /html\n\n# comment\nDisallow: /*?\nAllow: /?*\nAllow: /search.html\n\nUser-agent: ia_archiver\nDisallow: /\n");
        assert!(!robots.allows("/html/?q=rust"));
        assert!(!robots.allows("/lite"));
        assert!(robots.allows("/?q=rust"), "the allowing rule is as long as the refusing one, and then the page may be read");
        assert!(!robots.allows("/about?x=1"));
        assert!(robots.allows("/about"));
        assert!(robots.allows("/search.html"));
    }

    #[test]
    fn rules_written_for_demido_come_before_those_for_everyone() {
        let robots = Robots::read("User-agent: *\nDisallow: /\n\nUser-agent: DemidoStudio\nUser-agent: OtherBot\nDisallow: /private/\nAllow: /private/open.html\n");
        assert!(robots.allows("/anything"));
        assert!(!robots.allows("/private/notes.html"));
        assert!(robots.allows("/private/open.html"));
        let closed = Robots::read("User-agent: *\nDisallow: /\n");
        assert!(!closed.allows("/") && !closed.allows("/page"));
    }

    #[test]
    fn stars_and_the_end_mark_are_understood_and_nonsense_is_skipped() {
        let robots = Robots::read("this is not a rule\nUser-agent: *\nDisallow: /*.pdf$\nDisallow: /a/*/c\nDisallow:\nCrawl-delay: 10\nSitemap: https://example.com/sitemap.xml\n");
        assert!(!robots.allows("/files/paper.pdf"));
        assert!(robots.allows("/files/paper.pdf?download=1"), "the rule ends at .pdf");
        assert!(!robots.allows("/a/b/c") && !robots.allows("/a/b/b/c/d"));
        assert!(robots.allows("/a/c"));
        assert!(Robots::read("").allows("/anything"), "no file, no rules");
        assert!(Robots::read("<html>not found</html>").allows("/anything"));
    }
}
