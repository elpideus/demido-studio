//! When a schedule runs (DECISIONS J1, J9), and reading that from the person's sentence.
//!
//! The rule is kept in code and read by code, not by a model: "every morning at 7" is a fact about time that
//! must come out the same every time, and a small model's reading of it can be taken in by the rest of the
//! sentence. What the code does not understand it says so, and the window lets the person set it by hand.
//! Times are this computer's own, in its time zone: "at 7" is 7 o'clock here, summer time or not.

use jiff::civil::{Date, Weekday};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Not more often than this (J9): a schedule that runs more often than every 15 minutes is a program, not a
/// schedule, and it would ask the sites it reads too often.
pub const LEAST_MINUTES: u32 = 15;
/// A monitor that was not told how often: often enough for a price, rarely enough for the shop.
pub const MONITOR_MINUTES: u32 = 6 * 60;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum When {
    /// Every so many minutes, from the last run: at least 15.
    Every { minutes: u32 },
    /// Every day, at a time of day: `07:00`.
    Daily { at: String },
    /// On some days of the week, at a time of day. 1 is Monday, 7 Sunday.
    Weekly { days: Vec<u8>, at: String },
    /// Once, at a moment (milliseconds since 1970).
    Once { at_ms: i64 },
}

/// `07:30` as hours and minutes.
fn time_of(at: &str) -> Option<(i8, i8)> {
    let (h, m) = at.split_once(':')?;
    let (h, m) = (h.parse::<i8>().ok()?, m.parse::<i8>().ok()?);
    ((0..24).contains(&h) && (0..60).contains(&m) && at.len() == 5).then_some((h, m))
}

const DAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

impl When {
    /// Whether the rule can be kept: a real time of day, real days, not more often than every 15 minutes.
    pub fn check(&self) -> Result<(), String> {
        match self {
            Self::Every { minutes } if *minutes < LEAST_MINUTES => Err(format!("A schedule runs at most every {LEAST_MINUTES} minutes.")),
            Self::Every { minutes } if *minutes > 31 * 24 * 60 => Err("Every so many minutes is for up to a month; for longer, choose days of the week.".into()),
            Self::Daily { at } | Self::Weekly { at, .. } if time_of(at).is_none() => Err(format!("\"{at}\" is not a time of day: write it as 07:30.")),
            Self::Weekly { days, .. } if days.is_empty() || days.iter().any(|d| !(1..=7).contains(d)) => Err("Choose at least one day of the week.".into()),
            _ => Ok(()),
        }
    }

    /// The first moment after `after_ms` when it is due, in this time zone. `None`: it is not due again.
    pub fn next_after(&self, after_ms: i64, tz: &TimeZone) -> Option<i64> {
        let on = |date: Date, at: &str| -> Option<i64> {
            let (h, m) = time_of(at)?;
            // A time that does not exist on the day summer time starts is taken as the moment after the gap.
            Some(date.at(h, m, 0, 0).to_zoned_datetime(tz.clone()).ok()?.timestamp().as_millisecond())
        };
        let today = Timestamp::from_millisecond(after_ms).ok()?.to_zoned_datetime(tz.clone()).date();
        let days_from_today = |n: i64| today.checked_add(n.days()).ok();
        match self {
            Self::Every { minutes } => Some(after_ms + i64::from(*minutes) * 60_000),
            Self::Daily { at } => (0..=2).filter_map(days_from_today).filter_map(|d| on(d, at)).find(|&t| t > after_ms),
            Self::Weekly { days, at } => (0..=8).filter_map(days_from_today).filter(|d| days.contains(&(d.weekday().to_monday_one_offset() as u8))).filter_map(|d| on(d, at)).find(|&t| t > after_ms),
            Self::Once { at_ms } => (*at_ms > after_ms).then_some(*at_ms),
        }
    }

    /// The rule in words: "Every day at 07:00", "On weekdays at 08:30", "Every 2 hours".
    pub fn said(&self, tz: &TimeZone) -> String {
        match self {
            Self::Every { minutes } if minutes % 60 == 0 && *minutes == 60 => "Every hour".into(),
            Self::Every { minutes } if minutes % 60 == 0 => format!("Every {} hours", minutes / 60),
            Self::Every { minutes } => format!("Every {minutes} minutes"),
            Self::Daily { at } => format!("Every day at {at}"),
            Self::Weekly { days, at } => {
                let mut days = days.clone();
                days.sort_unstable();
                days.dedup();
                match days.as_slice() {
                    [1, 2, 3, 4, 5] => format!("On weekdays at {at}"),
                    [6, 7] => format!("At weekends at {at}"),
                    [1, 2, 3, 4, 5, 6, 7] => format!("Every day at {at}"),
                    _ => {
                        let names: Vec<&str> = days.iter().filter_map(|d| DAYS.get(usize::from(d.saturating_sub(1)))).copied().collect();
                        let named = match names.as_slice() {
                            [one] => one.to_string(),
                            [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
                            [] => String::new(),
                        };
                        format!("Every {named} at {at}")
                    }
                }
            }
            Self::Once { at_ms } => match Timestamp::from_millisecond(*at_ms) {
                Ok(t) => format!("Once, on {}", t.to_zoned_datetime(tz.clone()).strftime("%A %-d %B %Y at %H:%M")),
                Err(_) => "Once".into(),
            },
        }
    }
}

/// What a monitor watches for, on the page the person gave (J5). The price is the shop's own, read by code
/// from the product data it publishes for search engines; the comparison is code, never a model's yes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Watch {
    /// The page, as the person gave it.
    pub url: String,
    pub price: Price,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Price {
    /// Under a number the person gave.
    Below { below: f64 },
    /// Over a number the person gave.
    Above { above: f64 },
    /// Lower than it was when the monitor first read it: "when it goes on sale".
    Drops,
}

impl Price {
    /// Whether a price now is what the monitor watches for. `first`: the price at its first reading.
    pub fn holds(&self, now: f64, first: Option<f64>) -> bool {
        match self {
            Self::Below { below } => now < *below,
            Self::Above { above } => now > *above,
            Self::Drops => first.is_some_and(|first| now < first),
        }
    }
}

impl Watch {
    /// In words: "the price at shop.example is under 70".
    pub fn said(&self) -> String {
        let site = url::Url::parse(&self.url).ok().and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string())).unwrap_or_else(|| self.url.clone());
        match &self.price {
            Price::Below { below } => format!("The price at {site} is under {}", number(*below)),
            Price::Above { above } => format!("The price at {site} is over {}", number(*above)),
            Price::Drops => format!("The price at {site} drops below its first reading"),
        }
    }
}

/// A number as a person writes it: `70`, `69.99`.
pub fn number(n: f64) -> String {
    if n.fract() == 0.0 { format!("{n:.0}") } else { format!("{n:.2}") }
}

/// What a sentence was read as, for the person to see before it is kept ("Understood as …", J1).
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Understood {
    /// What to do, in the person's words, without the part that said when.
    pub what: String,
    /// When; `null` when the sentence did not say, and the person chooses.
    pub when: Option<When>,
    /// The same, in words.
    pub when_said: Option<String>,
    /// For a monitor: what it watches for.
    pub watch: Option<Watch>,
    pub watch_said: Option<String>,
    /// What the code could not read, or changed, said plainly.
    pub notes: Vec<String>,
}

/// A word of the sentence, with where it stands.
#[derive(Clone, Debug)]
struct Word {
    text: String,
    start: usize,
    end: usize,
}

fn words_of(sentence: &str) -> Vec<Word> {
    let mut words = Vec::new();
    let mut start = None;
    // Letters, digits, and what a time, a price or an address holds stay in one word.
    let keeps = |c: char| c.is_alphanumeric() || matches!(c, ':' | '.' | ',' | '\'' | '€' | '$' | '£' | '/' | '-' | '_' | '?' | '=' | '&' | '%' | '#' | '+' | '~');
    for (i, c) in sentence.char_indices().chain(std::iter::once((sentence.len(), ' '))) {
        match (keeps(c), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                let raw = &sentence[s..i];
                // A word ends without the full stop or comma that ends the clause.
                let trimmed = raw.trim_end_matches(['.', ',', '?']);
                words.push(Word { text: trimmed.to_lowercase(), start: s, end: s + trimmed.len() });
                start = None;
            }
            _ => {}
        }
    }
    words
}

/// A time of day as written: `7`, `7:30`, `19:00`, `7am`, `7.30pm`, with `am` or `pm` as the next word.
/// The second value: whether it said am or pm.
fn time_word(word: &str, next: Option<&str>) -> Option<((i8, i8), bool, bool)> {
    let (body, half, joined) = match ["am", "pm", "a.m", "p.m"].iter().find(|s| word.ends_with(*s) && word.len() > s.len()) {
        Some(s) => (&word[..word.len() - s.len()], Some(s.starts_with('p')), true),
        None => match next {
            Some("am" | "a.m" | "a.m.") => (word, Some(false), false),
            Some("pm" | "p.m" | "p.m.") => (word, Some(true), false),
            _ => (word, None, false),
        },
    };
    let (h, m) = match body.split_once([':', '.']) {
        Some((h, m)) => (h.parse::<i8>().ok()?, m.parse::<i8>().ok().filter(|_| m.len() == 2)?),
        None => (body.parse::<i8>().ok()?, 0),
    };
    if !(0..60).contains(&m) {
        return None;
    }
    let h = match half {
        Some(pm) if (1..=12).contains(&h) => (h % 12) + if pm { 12 } else { 0 },
        Some(_) => return None,
        None if (0..24).contains(&h) => h,
        None => return None,
    };
    Some(((h, m), half.is_some(), half.is_some() && !joined))
}

fn day_of(word: &str) -> Option<u8> {
    let w = word.trim_end_matches('s');
    DAYS.iter()
        .position(|d| {
            let d = d.to_lowercase();
            w == d || (w.len() >= 3 && d.starts_with(w) && w.len() <= 4)
        })
        .map(|i| i as u8 + 1)
}

/// A price as written: `70`, `69.99`, `69,99`, `€70`, `70€`, `$69.99`.
fn price_word(word: &str) -> Option<f64> {
    let body = word.trim_matches(|c: char| matches!(c, '€' | '$' | '£'));
    let body = if body.matches(',').count() == 1 && !body.contains('.') && body.rsplit(',').next().is_some_and(|d| d.len() == 2) { body.replace(',', ".") } else { body.replace(',', "") };
    body.parse::<f64>().ok().filter(|n| n.is_finite() && *n > 0.0)
}

/// Reads when, and for a monitor what to watch for, from the person's sentence (J1). `now_ms` is when it is
/// read, for "tomorrow" and "in an hour".
pub fn understand(sentence: &str, now_ms: i64, tz: &TimeZone) -> Understood {
    let words = words_of(sentence);
    let w = |i: usize| words.get(i).map(|w| w.text.as_str());
    let mut used = vec![false; words.len()];
    let mut notes = Vec::new();
    let mut every_minutes: Option<u32> = None;
    let mut days: Vec<u8> = Vec::new();
    let mut daily = false;
    let mut time: Option<(i8, i8)> = None;
    let mut part_of_day: Option<i8> = None; // the hour it means when no time is said: morning 8, evening 19
    let mut once_day: Option<i64> = None; // days from today: 0 today, 1 tomorrow
    let mut once_in: Option<i64> = None; // minutes from now

    fn take(from: usize, to: usize, used: &mut [bool]) {
        (from..=to.min(used.len() - 1)).for_each(|k| used[k] = true);
    }
    let mut i = 0;
    while i < words.len() {
        let word = words[i].text.as_str();
        match word {
            "every" | "each" => {
                let n = w(i + 1).and_then(|n| {
                    n.parse::<u32>().ok().or(match n {
                        "a" | "an" | "one" => Some(1),
                        "two" => Some(2),
                        "three" => Some(3),
                        "six" => Some(6),
                        "twelve" => Some(12),
                        _ => None,
                    })
                });
                let unit_at = if n.is_some() { i + 2 } else { i + 1 };
                let n = n.unwrap_or(1);
                match w(unit_at) {
                    Some("minute" | "minutes" | "min" | "mins") => {
                        every_minutes = Some(n);
                        take(i, unit_at, &mut used);
                    }
                    Some("hour" | "hours" | "hr" | "hrs") => {
                        every_minutes = Some(n * 60);
                        take(i, unit_at, &mut used);
                    }
                    Some("half") if matches!(w(unit_at + 1), Some("hour")) || (w(unit_at + 1) == Some("an") && w(unit_at + 2) == Some("hour")) => {
                        every_minutes = Some(30);
                        take(i, if w(unit_at + 1) == Some("an") { unit_at + 2 } else { unit_at + 1 }, &mut used);
                    }
                    Some("day" | "days") if n == 1 => {
                        daily = true;
                        take(i, unit_at, &mut used);
                    }
                    Some("morning" | "evening" | "night" | "afternoon") => {
                        daily = true;
                        part_of_day = Some(match w(unit_at) {
                            Some("morning") => 8,
                            Some("afternoon") => 15,
                            Some("evening") => 19,
                            _ => 21,
                        });
                        take(i, unit_at, &mut used);
                    }
                    Some("weekday" | "weekdays") => {
                        days = vec![1, 2, 3, 4, 5];
                        take(i, unit_at, &mut used);
                    }
                    Some("weekend" | "weekends") => {
                        days = vec![6, 7];
                        take(i, unit_at, &mut used);
                    }
                    Some("week") => {
                        if days.is_empty() {
                            days = vec![Timestamp::from_millisecond(now_ms).map(|t| t.to_zoned_datetime(tz.clone()).weekday()).unwrap_or(Weekday::Monday).to_monday_one_offset() as u8];
                        }
                        take(i, unit_at, &mut used);
                    }
                    Some(d) if day_of(d).is_some() => {
                        take(i, i, &mut used);
                    }
                    _ => {}
                }
            }
            "daily" => {
                daily = true;
                take(i, i, &mut used);
            }
            "hourly" => {
                every_minutes = Some(60);
                take(i, i, &mut used);
            }
            "weekdays" => {
                days = vec![1, 2, 3, 4, 5];
                take(i.saturating_sub(usize::from(matches!(w(i.wrapping_sub(1)), Some("on")))), i, &mut used);
            }
            "weekends" => {
                days = vec![6, 7];
                take(i.saturating_sub(usize::from(matches!(w(i.wrapping_sub(1)), Some("on" | "at")))), i, &mut used);
            }
            "mornings" | "evenings" => {
                daily = true;
                part_of_day = Some(if word == "mornings" { 8 } else { 19 });
                take(i, i, &mut used);
            }
            "today" | "tonight" | "tomorrow" => {
                once_day = Some(if word == "tomorrow" { 1 } else { 0 });
                if word == "tonight" {
                    part_of_day = Some(21);
                }
                take(i, i, &mut used);
                if matches!(w(i + 1), Some("morning" | "evening" | "afternoon" | "night")) {
                    part_of_day = Some(match w(i + 1) {
                        Some("morning") => 8,
                        Some("afternoon") => 15,
                        Some("evening") => 19,
                        _ => 21,
                    });
                    take(i + 1, i + 1, &mut used);
                }
            }
            "in" if matches!(w(i + 2), Some("minute" | "minutes" | "min" | "mins" | "hour" | "hours")) || (matches!(w(i + 1), Some("an" | "a")) && w(i + 2) == Some("hour")) => {
                let n = match w(i + 1) {
                    Some("an" | "a") => Some(1),
                    Some(n) => n.parse::<i64>().ok(),
                    None => None,
                };
                if let Some(n) = n {
                    once_in = Some(if matches!(w(i + 2), Some("hour" | "hours")) { n * 60 } else { n });
                    take(i, i + 2, &mut used);
                }
            }
            "at" | "around" if w(i + 1).is_some_and(|t| t == "noon" || t == "midnight" || time_word(t, w(i + 2)).is_some()) => {
                let t = w(i + 1).unwrap_or_default();
                let found = match t {
                    "noon" => Some(((12, 0), true, false)),
                    "midnight" => Some(((0, 0), true, false)),
                    _ => time_word(t, w(i + 2)),
                };
                if let Some((hm, said_half, separate)) = found {
                    time = Some(hm);
                    if !said_half {
                        part_of_day.get_or_insert(-1);
                    }
                    let mut to = i + 1 + usize::from(separate);
                    if matches!(w(to + 1), Some("o'clock" | "oclock")) {
                        to += 1;
                    }
                    take(i, to, &mut used);
                    // "at 7 in the morning"
                    if w(to + 1) == Some("in") && w(to + 2) == Some("the") && matches!(w(to + 3), Some("morning" | "evening" | "afternoon")) {
                        part_of_day = Some(if w(to + 3) == Some("morning") { 8 } else { 19 });
                        take(to + 1, to + 3, &mut used);
                    }
                }
            }
            d if day_of(d).is_some() && (d.len() >= 6 || matches!(w(i.wrapping_sub(1)), Some("on" | "every" | "each" | "and" | ","))) => {
                let day = day_of(d).unwrap_or(1);
                if !days.contains(&day) {
                    days.push(day);
                }
                let from = if matches!(w(i.wrapping_sub(1)), Some("on" | "and")) { i - 1 } else { i };
                take(from, i, &mut used);
            }
            _ => {}
        }
        i += 1;
    }

    // "at 7" in the evening is 19:00; written with am or pm, or as 19:00, it is what it says.
    if let (Some((h, m)), Some(part)) = (time, part_of_day)
        && part >= 15
        && h < 12
    {
        time = Some((h + 12, m));
    }
    let at = |hm: (i8, i8)| format!("{:02}:{:02}", hm.0, hm.1);
    let default_time = part_of_day.filter(|p| *p >= 0).map(|p| (p, 0));

    // What a monitor watches for: an address, and a price under, over, or lower than it was.
    let address = crate::web::guard::urls_in(sentence, true).into_iter().next().map(|u| u.to_string());
    let lower = sentence.to_lowercase();
    let watching = words.iter().any(|w| matches!(w.text.as_str(), "monitor" | "watch" | "price" | "sale" | "notify" | "alert")) || lower.contains("tell me when") || lower.contains("let me know when");
    let mut price: Option<Price> = None;
    for (k, word) in words.iter().enumerate().filter(|_| watching) {
        let currency = |j: usize| {
            words.get(j + 1).is_some_and(|w| matches!(w.text.as_str(), "euro" | "euros" | "eur" | "€" | "dollar" | "dollars" | "usd" | "$" | "pound" | "pounds" | "gbp" | "£" | "francs" | "chf"))
        };
        let after = |n: usize| (k + n..(k + n + 2).min(words.len())).find_map(|j| price_word(&words[j].text).map(|p| (if currency(j) { j + 1 } else { j }, p)));
        match word.text.as_str() {
            "under" | "below" | "beneath" | "cheaper" | "less" | "lower" => {
                let skip = usize::from(matches!(w(k + 1), Some("than")));
                if let Some((j, p)) = after(1 + skip) {
                    price = Some(Price::Below { below: p });
                    (k..=j).for_each(|x| used[x] = true);
                } else if word.text == "cheaper" && price.is_none() {
                    price = Some(Price::Drops);
                }
            }
            "over" | "above" | "more" | "higher" => {
                let skip = usize::from(matches!(w(k + 1), Some("than")));
                if let Some((j, p)) = after(1 + skip) {
                    price = Some(Price::Above { above: p });
                    (k..=j).for_each(|x| used[x] = true);
                }
            }
            "sale" if price.is_none() => price = Some(Price::Drops),
            "drops" if price.is_none() && after(1).is_none() => price = Some(Price::Drops),
            _ => {}
        }
    }
    let watch = match (watching && price.is_some(), &address) {
        (true, Some(url)) => price.clone().map(|price| Watch { url: url.clone(), price }),
        (true, None) => {
            notes.push("A monitor reads the price from the shop's own page: give its address in the sentence.".into());
            None
        }
        _ => None,
    };

    let mut when = if let Some(minutes) = every_minutes {
        Some(When::Every { minutes })
    } else if let Some(minutes) = once_in {
        Some(When::Once { at_ms: now_ms + minutes * 60_000 })
    } else if let Some(day) = once_day {
        let hm = time.or(default_time).unwrap_or((9, 0));
        let date = Timestamp::from_millisecond(now_ms).ok().map(|t| t.to_zoned_datetime(tz.clone()).date()).and_then(|d| d.checked_add(day.days()).ok());
        date.and_then(|d| d.at(hm.0, hm.1, 0, 0).to_zoned_datetime(tz.clone()).ok()).map(|z| When::Once { at_ms: z.timestamp().as_millisecond() })
    } else if !days.is_empty() {
        days.sort_unstable();
        Some(When::Weekly { days, at: at(time.or(default_time).unwrap_or((8, 0))) })
    } else if daily || time.is_some() {
        Some(When::Daily { at: at(time.or(default_time).unwrap_or((8, 0))) })
    } else if watch.is_some() {
        notes.push(format!("No time was said, so the monitor looks every {} hours.", MONITOR_MINUTES / 60));
        Some(When::Every { minutes: MONITOR_MINUTES })
    } else {
        notes.push("The sentence does not say when. Say it (\"every morning at 7\"), or choose it here.".into());
        None
    };
    if let Some(When::Every { minutes }) = &when
        && *minutes < LEAST_MINUTES
    {
        notes.push(format!("Not more often than every {LEAST_MINUTES} minutes: set to every {LEAST_MINUTES}."));
        when = Some(When::Every { minutes: LEAST_MINUTES });
    }
    if let Some(When::Once { at_ms }) = &when
        && *at_ms <= now_ms
    {
        notes.push("That time has passed today: it is set for the same time tomorrow.".into());
        when = Some(When::Once { at_ms: at_ms + 86_400_000 });
    }

    // What is left is what to do: the words that said when taken out, and the "and" or "at" they leave.
    let mut what = String::new();
    let mut last_end = 0;
    for (k, word) in words.iter().enumerate() {
        if used[k] {
            what.push_str(&sentence[last_end..word.start]);
            last_end = word.end;
        }
    }
    what.push_str(&sentence[last_end..]);
    let mut what = what.split_whitespace().collect::<Vec<_>>().join(" ");
    loop {
        let trimmed = what.trim_end_matches([' ', ',', '.', ';']).to_string();
        let lower = trimmed.to_lowercase();
        let cut = ["and", "at", "on", "from", "starting", "every", "each", "please"].iter().find(|tail| lower.ends_with(&format!(" {tail}")));
        what = match cut {
            Some(tail) => trimmed[..trimmed.len() - tail.len() - 1].to_string(),
            None => trimmed,
        };
        if cut.is_none() {
            break;
        }
    }
    let what = what.trim_start_matches(|c: char| c == ',' || c.is_whitespace()).to_string();
    let what = what.strip_prefix("Please ").or_else(|| what.strip_prefix("please ")).unwrap_or(&what).to_string();
    let mut chars = what.chars();
    let what = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    };
    Understood { when_said: when.as_ref().map(|w| w.said(tz)), watch_said: watch.as_ref().map(Watch::said), what, when, watch, notes }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rome() -> TimeZone {
        TimeZone::get("Europe/Rome").unwrap()
    }
    /// A moment in Rome's time, as milliseconds.
    fn at(text: &str) -> i64 {
        text.parse::<jiff::civil::DateTime>().unwrap().to_zoned_datetime(rome()).unwrap().timestamp().as_millisecond()
    }
    fn local(ms: i64) -> String {
        Timestamp::from_millisecond(ms).unwrap().to_zoned_datetime(rome()).strftime("%a %Y-%m-%d %H:%M").to_string()
    }

    #[test]
    fn the_next_run_is_the_first_moment_after_and_follows_the_clock_here() {
        let tz = rome();
        // Sunday 22 November 2026, 10:00.
        let now = at("2026-11-22T10:00");
        let daily = When::Daily { at: "07:00".into() };
        assert_eq!(local(daily.next_after(now, &tz).unwrap()), "Mon 2026-11-23 07:00");
        assert_eq!(local(daily.next_after(at("2026-11-22T06:59"), &tz).unwrap()), "Sun 2026-11-22 07:00");
        let weekdays = When::Weekly { days: vec![1, 2, 3, 4, 5], at: "08:30".into() };
        assert_eq!(local(weekdays.next_after(now, &tz).unwrap()), "Mon 2026-11-23 08:30", "the weekend is skipped");
        assert_eq!(local(When::Weekly { days: vec![7], at: "09:00".into() }.next_after(now, &tz).unwrap()), "Sun 2026-11-29 09:00", "today's 09:00 has passed: next Sunday");
        assert_eq!(When::Every { minutes: 30 }.next_after(now, &tz), Some(now + 30 * 60_000));
        assert_eq!(When::Once { at_ms: now + 5 }.next_after(now, &tz), Some(now + 5));
        assert_eq!(When::Once { at_ms: now }.next_after(now, &tz), None, "once, and it is past: not again");
        // Summer time: on 29 March 2026 Rome's clocks go from 02:00 to 03:00, and back on 25 October.
        let night = When::Daily { at: "02:30".into() };
        assert_eq!(local(night.next_after(at("2026-03-28T12:00"), &tz).unwrap()), "Sun 2026-03-29 03:30", "a time that does not exist that night is the moment after the gap");
        let seven = When::Daily { at: "07:00".into() };
        let before = seven.next_after(at("2026-10-24T12:00"), &tz).unwrap();
        assert_eq!(local(before), "Sun 2026-10-25 07:00");
        assert_eq!(seven.next_after(before, &tz).unwrap() - before, 24 * 3_600_000, "07:00 is 07:00 on either side of the change");
    }

    #[test]
    fn a_rule_says_itself_in_words_and_is_checked() {
        let tz = rome();
        assert_eq!(When::Every { minutes: 60 }.said(&tz), "Every hour");
        assert_eq!(When::Every { minutes: 120 }.said(&tz), "Every 2 hours");
        assert_eq!(When::Every { minutes: 45 }.said(&tz), "Every 45 minutes");
        assert_eq!(When::Daily { at: "07:00".into() }.said(&tz), "Every day at 07:00");
        assert_eq!(When::Weekly { days: vec![5, 1, 3], at: "09:00".into() }.said(&tz), "Every Monday, Wednesday and Friday at 09:00");
        assert_eq!(When::Weekly { days: vec![1, 2, 3, 4, 5], at: "08:30".into() }.said(&tz), "On weekdays at 08:30");
        assert_eq!(When::Once { at_ms: at("2026-11-23T15:00") }.said(&tz), "Once, on Monday 23 November 2026 at 15:00");
        assert!(When::Every { minutes: 14 }.check().is_err() && When::Every { minutes: 15 }.check().is_ok());
        assert!(When::Daily { at: "25:00".into() }.check().is_err() && When::Daily { at: "7:00".into() }.check().is_err());
        assert!(When::Weekly { days: vec![], at: "07:00".into() }.check().is_err() && When::Weekly { days: vec![8], at: "07:00".into() }.check().is_err());
    }

    #[test]
    fn a_sentence_is_read_for_when_and_the_rest_is_what_to_do() {
        let tz = rome();
        let now = at("2026-11-22T10:00");
        let read = |s: &str| understand(s, now, &tz);
        let u = read("Give me my email report every morning at 7");
        assert_eq!((u.what.as_str(), u.when_said.as_deref()), ("Give me my email report", Some("Every day at 07:00")));
        assert_eq!(read("Every evening at 7, summarise the news").when_said.as_deref(), Some("Every day at 19:00"));
        assert_eq!(read("Every evening at 7, summarise the news").what, "Summarise the news");
        assert_eq!(read("Check the build every 30 minutes").when, Some(When::Every { minutes: 30 }));
        assert_eq!(read("Check the build every 30 minutes").what, "Check the build");
        assert_eq!(read("Check the build every 5 minutes").when, Some(When::Every { minutes: 15 }), "never more often than every 15 minutes");
        assert!(read("Check the build every 5 minutes").notes[0].starts_with("Not more often"));
        assert_eq!(read("Check the build every hour").when_said.as_deref(), Some("Every hour"));
        assert_eq!(read("Remind me to stretch hourly").what, "Remind me to stretch");
        assert_eq!(read("Plan my week on weekdays at 8:30am").when_said.as_deref(), Some("On weekdays at 08:30"));
        assert_eq!(read("Every Monday and Friday at 9 send me the open tasks").when_said.as_deref(), Some("Every Monday and Friday at 09:00"));
        assert_eq!(read("Every Monday and Friday at 9 send me the open tasks").what, "Send me the open tasks");
        assert_eq!(read("Water the plants reminder every day at 6pm").when_said.as_deref(), Some("Every day at 18:00"));
        assert_eq!(read("Tell me the weather at 7:30 every day").when_said.as_deref(), Some("Every day at 07:30"));
        assert_eq!(read("Look at the backups tomorrow at 9").when, Some(When::Once { at_ms: at("2026-11-23T09:00") }));
        assert_eq!(read("Look at the backups tomorrow at 9").what, "Look at the backups");
        assert_eq!(read("Remind me in 2 hours to call Mira").when, Some(When::Once { at_ms: now + 120 * 60_000 }));
        assert_eq!(read("Remind me in 2 hours to call Mira").what, "Remind me to call Mira");
        assert_eq!(read("Send me a summary every week").when_said.as_deref(), Some("Every Sunday at 08:00"), "every week: on the day it was made");
        let none = read("Summarise my notes");
        assert_eq!((none.when, none.what.as_str()), (None, "Summarise my notes"));
        assert!(none.notes[0].contains("does not say when"));
        // The time of day is a time of day, not a number in the task.
        assert_eq!(read("Count the 3 files at noon every day").when_said.as_deref(), Some("Every day at 12:00"));
        assert_eq!(read("Count the 3 files at noon every day").what, "Count the 3 files");
        assert_eq!(read("List the tasks under 5 minutes every day at 8").what, "List the tasks under 5 minutes", "a number in a task is not a price");
    }

    #[test]
    fn a_monitor_is_an_address_and_a_price_read_by_code() {
        let tz = rome();
        let now = at("2026-11-22T10:00");
        let u = understand("Monitor the price of this sweater https://shop.example.com/p/merino and tell me when it goes under €70", now, &tz);
        assert_eq!(u.watch, Some(Watch { url: "https://shop.example.com/p/merino".into(), price: Price::Below { below: 70.0 } }));
        assert_eq!(u.watch_said.as_deref(), Some("The price at shop.example.com is under 70"));
        assert_eq!(u.when, Some(When::Every { minutes: MONITOR_MINUTES }), "not told how often: every few hours");
        assert!(u.notes.iter().any(|n| n.starts_with("No time was said")));
        let sale = understand("Tell me when https://shop.example.com/p/merino goes on sale, check every day at 9", now, &tz);
        assert_eq!(sale.watch.as_ref().map(|w| &w.price), Some(&Price::Drops));
        assert_eq!(sale.when_said.as_deref(), Some("Every day at 09:00"));
        assert_eq!(understand("Let me know when the price at https://shop.example.com/x is below 69,99", now, &tz).watch.unwrap().price, Price::Below { below: 69.99 });
        assert_eq!(understand("Watch https://shop.example.com/x and alert me if it goes above $120", now, &tz).watch.unwrap().price, Price::Above { above: 120.0 });
        let worded = understand("Tell me when the sweater at https://shop.example.com/x goes under 70 euros", now, &tz);
        assert_eq!(
            (worded.watch.unwrap().price, worded.what.as_str()),
            (Price::Below { below: 70.0 }, "Tell me when the sweater at https://shop.example.com/x goes"),
            "the currency goes with its number"
        );
        let lost = understand("Tell me when the sweater goes on sale", now, &tz);
        assert!(lost.watch.is_none() && lost.notes.iter().any(|n| n.contains("give its address")));
        assert!(Price::Below { below: 70.0 }.holds(69.0, None) && !Price::Below { below: 70.0 }.holds(70.0, None));
        assert!(Price::Drops.holds(64.0, Some(79.0)) && !Price::Drops.holds(79.0, Some(79.0)) && !Price::Drops.holds(64.0, None));
    }
}
