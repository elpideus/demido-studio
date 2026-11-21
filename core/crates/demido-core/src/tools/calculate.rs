//! Arithmetic for the `calculate` tool: the sums a small model gets wrong when it does them in its head.
//!
//! It reads what models and people actually write: `437 * 4 + 35`, `2,340 x 18%`, `18% of 2340`,
//! `1299 + 22%` (as a pocket calculator means it: 22 percent more), `4.1 million / 12`, `sqrt(2) ^ 2`.
//! What it does not know (a unit, a name) it says by name, so the model can write the sum again without it.
//!
//! It also counts the days between two dates, written as one minus the other. That is what a model asked
//! for by itself, the first time it was given the clock and a question about a date.

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f64),
    Word(String),
    Sign(char),
}

const HELP: &str = "Use numbers and + - * / ^ ( ) % only";

fn tokens(text: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = text.trim().trim_end_matches(['=', '?']).chars().collect();
    let mut out = Vec::new();
    // For each open bracket: whether it opened the arguments of a function, where a comma separates.
    let mut brackets: Vec<bool> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            _ if c.is_whitespace() => i += 1,
            '$' | '€' | '£' => i += 1,
            _ if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) => {
                let mut number = String::new();
                while i < chars.len() {
                    let d = chars[i];
                    let three_digits_follow = (1..=3).all(|k| chars.get(i + k).is_some_and(char::is_ascii_digit)) && !chars.get(i + 4).is_some_and(char::is_ascii_digit);
                    if d.is_ascii_digit() || (d == '.' && !number.contains('.')) {
                        number.push(d);
                    } else if d == '_' || (d == ',' && !number.contains('.') && three_digits_follow && brackets.last() != Some(&true)) {
                        // 2,340 and 1_000_000 are one number each. Inside a function's brackets a comma separates.
                    } else if (d == 'e' || d == 'E') && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit() || ((*n == '-' || *n == '+') && chars.get(i + 2).is_some_and(char::is_ascii_digit))) {
                        number.push('e');
                        number.push(chars[i + 1]);
                        i += 1;
                    } else {
                        break;
                    }
                    i += 1;
                }
                out.push(Token::Number(number.parse().map_err(|_| format!("\"{number}\" is not a number"))?));
            }
            _ if c.is_alphabetic() => {
                let mut word = String::new();
                while i < chars.len() && chars[i].is_alphabetic() {
                    word.extend(chars[i].to_lowercase());
                    i += 1;
                }
                out.push(Token::Word(word));
            }
            '*' if chars.get(i + 1) == Some(&'*') => {
                out.push(Token::Sign('^'));
                i += 2;
            }
            '(' => {
                brackets.push(matches!(out.last(), Some(Token::Word(_))));
                out.push(Token::Sign('('));
                i += 1;
            }
            ')' => {
                brackets.pop();
                out.push(Token::Sign(')'));
                i += 1;
            }
            '+' | '-' | '*' | '/' | '^' | '%' | ',' => {
                out.push(Token::Sign(c));
                i += 1;
            }
            '×' | '·' => {
                out.push(Token::Sign('*'));
                i += 1;
            }
            '÷' => {
                out.push(Token::Sign('/'));
                i += 1;
            }
            '−' | '–' => {
                out.push(Token::Sign('-'));
                i += 1;
            }
            other => return Err(format!("\"{other}\" is not something calculate knows. {HELP}")),
        }
    }
    Ok(out)
}

struct Sum {
    tokens: Vec<Token>,
    at: usize,
    depth: u32,
}

/// A value, and whether it was written as a bare percentage (`22%`), which `+` and `-` take as "of what came before".
type Read = Result<(f64, bool), String>;

impl Sum {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }
    fn sign(&self, c: char) -> bool {
        self.peek() == Some(&Token::Sign(c))
    }
    fn word(&self, w: &str) -> bool {
        matches!(self.peek(), Some(Token::Word(found)) if found == w)
    }

    fn sum(&mut self) -> Read {
        self.depth += 1;
        if self.depth > 60 {
            return Err("the expression is nested too deep".into());
        }
        let (mut value, mut percent) = self.product()?;
        loop {
            let add = if self.sign('+') || self.word("plus") {
                true
            } else if self.sign('-') || self.word("minus") {
                false
            } else {
                break;
            };
            self.at += 1;
            let (right, right_percent) = self.product()?;
            // 1299 + 22% is 22 percent more than 1299, as on a pocket calculator.
            let right = if right_percent { value * right } else { right };
            value = if add { value + right } else { value - right };
            percent = false;
        }
        self.depth -= 1;
        Ok((value, percent))
    }

    fn product(&mut self) -> Read {
        let (mut value, mut percent) = self.signed()?;
        loop {
            let op = if self.sign('*') || self.word("x") || self.word("times") || self.word("of") {
                '*'
            } else if self.sign('/') {
                '/'
            } else if self.sign('%') || self.word("mod") {
                '%'
            } else if self.sign('(') {
                // 2(3 + 4)
                let (right, _) = self.signed()?;
                value *= right;
                percent = false;
                continue;
            } else {
                break;
            };
            self.at += 1;
            let (right, _) = self.signed()?;
            if op != '*' && right == 0.0 {
                return Err("that divides by zero".into());
            }
            value = match op {
                '*' => value * right,
                '/' => value / right,
                _ => value % right,
            };
            percent = false;
        }
        Ok((value, percent))
    }

    fn signed(&mut self) -> Read {
        if self.sign('-') {
            self.at += 1;
            let (value, percent) = self.signed()?;
            return Ok((-value, percent));
        }
        if self.sign('+') {
            self.at += 1;
            return self.signed();
        }
        let (base, percent) = self.percent()?;
        if self.sign('^') {
            self.at += 1;
            let (power, _) = self.signed()?;
            return Ok((base.powf(power), false));
        }
        Ok((base, percent))
    }

    fn percent(&mut self) -> Read {
        let mut value = self.single()?;
        // 4.1 million
        while let Some(scale) = self.scale() {
            self.at += 1;
            value *= scale;
        }
        let mut percent = false;
        // A % with something to its right that it could divide by is a remainder (10 % 3). Otherwise it is a percentage.
        while self.sign('%') && !matches!(self.tokens.get(self.at + 1), Some(Token::Number(_)) | Some(Token::Sign('('))) {
            self.at += 1;
            value /= 100.0;
            percent = true;
        }
        Ok((value, percent))
    }

    /// The word for a size that may follow a number, as what it multiplies by.
    fn scale(&self) -> Option<f64> {
        match self.peek() {
            Some(Token::Word(w)) => match w.as_str() {
                "thousand" => Some(1e3),
                "million" => Some(1e6),
                "billion" => Some(1e9),
                "trillion" => Some(1e12),
                _ => None,
            },
            _ => None,
        }
    }

    fn single(&mut self) -> Result<f64, String> {
        match self.peek().cloned() {
            Some(Token::Number(n)) => {
                self.at += 1;
                Ok(n)
            }
            Some(Token::Sign('(')) => {
                self.at += 1;
                let (value, _) = self.sum()?;
                self.close()?;
                Ok(value)
            }
            Some(Token::Word(name)) => {
                self.at += 1;
                if !self.sign('(') {
                    return match name.as_str() {
                        "pi" => Ok(std::f64::consts::PI),
                        "e" => Ok(std::f64::consts::E),
                        _ => Err(format!("\"{name}\" is not something calculate knows. {HELP}")),
                    };
                }
                self.at += 1;
                let mut given = Vec::new();
                if !self.sign(')') {
                    loop {
                        given.push(self.sum()?.0);
                        if self.sign(',') {
                            self.at += 1;
                        } else {
                            break;
                        }
                    }
                }
                self.close()?;
                function(&name, &given)
            }
            Some(Token::Sign(c)) => Err(format!("there is a \"{c}\" where a number should be")),
            None => Err("the expression ends where a number should be".into()),
        }
    }

    fn close(&mut self) -> Result<(), String> {
        if self.sign(')') {
            self.at += 1;
            Ok(())
        } else {
            Err("a bracket is not closed".into())
        }
    }
}

fn function(name: &str, given: &[f64]) -> Result<f64, String> {
    let one = || given.first().copied().filter(|_| given.len() == 1).ok_or_else(|| format!("{name} takes one number"));
    let some = || if given.is_empty() { Err(format!("{name} needs at least one number")) } else { Ok(given) };
    match name {
        "sqrt" => one().and_then(|v| if v < 0.0 { Err("that is the square root of a negative number".into()) } else { Ok(v.sqrt()) }),
        "abs" => one().map(f64::abs),
        "floor" => one().map(f64::floor),
        "ceil" => one().map(f64::ceil),
        "round" => match given {
            [v] => Ok(v.round()),
            [v, places] if (0.0..=12.0).contains(places) => {
                let by = 10f64.powi(*places as i32);
                Ok((v * by).round() / by)
            }
            _ => Err("round takes a number, and how many decimal places".into()),
        },
        "min" => some().map(|g| g.iter().copied().fold(f64::INFINITY, f64::min)),
        "max" => some().map(|g| g.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
        "sum" => some().map(|g| g.iter().sum()),
        "avg" | "average" | "mean" => some().map(|g| g.iter().sum::<f64>() / g.len() as f64),
        _ => Err(format!("\"{name}\" is not something calculate knows. It knows: sqrt, abs, round, floor, ceil, min, max, sum, avg")),
    }
}

/// The number as a person writes it: no noise from binary fractions, no trailing zeros.
fn shown(value: f64) -> Result<String, String> {
    if !value.is_finite() {
        return Err("the result is too large to write".into());
    }
    // Twelve digits that count. That hides 0.30000000000000004 and keeps every sum of money exact.
    let rounded: f64 = format!("{value:.11e}").parse().map_err(|_| "the result cannot be written".to_string())?;
    Ok(if rounded == 0.0 { "0".into() } else { format!("{rounded}") })
}

/// Days since a fixed day long ago, for a date of the calendar in use today. `None` for a date that does not exist.
pub(super) fn day_number(year: i64, month: i64, day: i64) -> Option<i64> {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days_in_month = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month[month as usize - 1] {
        return None;
    }
    // Counted from 1 March, so that the leap day is the last day of the year.
    let (y, m) = if month <= 2 { (year - 1, month + 9) } else { (year, month - 3) };
    Some(365 * y + y.div_euclid(4) - y.div_euclid(100) + y.div_euclid(400) + (153 * m + 2) / 5 + day - 1)
}

/// Which way a date without a year is taken from today: `25 December` is the next one when counting the
/// days until it, and the last one when counting the days since.
#[derive(Clone, Copy, PartialEq)]
enum Lean {
    Next,
    Last,
}

/// A date as people and models write it: `25 December 2026`, `December 25th, 2026`, `2026-12-25`, `today`,
/// with a weekday in front or a time after it, in quotes or not. `None` for anything that is not plainly
/// one date. `today` is a year, a month and a day.
fn date(text: &str, today: Option<(i64, i64, i64)>, lean: Lean) -> Option<i64> {
    const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let text = text.trim().trim_matches(['\'', '"', '`', '(', ')', '.', ',', ' ']).to_lowercase();
    let today_number = today.and_then(|(y, m, d)| day_number(y, m, d));
    if ["today", "now", "the current date", "current date", "today's date"].contains(&text.as_str()) {
        return today_number;
    }
    // A time and what follows it are not part of the date.
    let text = text.split_once(':').map_or(text.as_str(), |(before, _)| before.rsplit_once(|c: char| !c.is_ascii_digit()).map_or("", |(date, _)| date));
    let (mut month, mut numbers) = (None, Vec::new());
    for word in text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        let digits = word.trim_end_matches(|c: char| c.is_alphabetic());
        if let (true, Ok(n)) = (!digits.is_empty() && ["", "st", "nd", "rd", "th"].contains(&&word[digits.len()..]), digits.parse::<i64>()) {
            numbers.push((n, digits.len()));
        } else if let Some(at) = MONTHS.iter().position(|m| word.starts_with(m) && word.len() <= 9) {
            if month.is_some() {
                return None;
            }
            month = Some(at as i64 + 1);
        } else if !["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday", "mon", "tue", "wed", "thu", "fri", "sat", "sun", "of", "the"].contains(&word) {
            return None;
        }
    }
    match (month, numbers.as_slice()) {
        // 2026-12-25
        (None, [(y, 4), (m, _), (d, _)]) => day_number(*y, *m, *d),
        // A day and a year, either way round, with the month as a word.
        (Some(m), [(a, la), (b, lb)]) if (*la == 4) != (*lb == 4) => {
            if *la == 4 {
                day_number(*a, m, *b)
            } else {
                day_number(*b, m, *a)
            }
        }
        // No year: the nearest such day in the direction asked for.
        (Some(m), [(d, _)]) => {
            let (year, now) = (today?.0, today_number?);
            let near = [year - 1, year, year + 1].into_iter().filter_map(|y| day_number(y, m, *d));
            match lean {
                Lean::Next => near.filter(|n| *n >= now).min(),
                Lean::Last => near.filter(|n| *n <= now).max(),
            }
        }
        _ => None,
    }
}

/// The days from one date to another, however it is asked: `25 December 2026 - 21 November 2026`,
/// `days between 21 November and 25 December`, `days until 25 December`, `days since 1 March 2026`.
/// `None` when the expression is not about two dates.
fn days_between(expression: &str, today: Option<(i64, i64, i64)>) -> Option<String> {
    let text = expression.trim().trim_start_matches('#').trim_end_matches(['=', '?', '.']).trim().to_lowercase();
    let said = |days: i64| Some(format!("{days} {}", if days.abs() == 1 { "day" } else { "days" }));
    let now = today.and_then(|(y, m, d)| day_number(y, m, d));
    let counted = ["number of ", "the number of ", "how many ", "the "].iter().find_map(|p| text.strip_prefix(p)).unwrap_or(&text);
    if let Some(rest) = counted.strip_prefix("days ") {
        let rest = ["left ", "remaining "].iter().find_map(|p| rest.strip_prefix(p)).unwrap_or(rest);
        if let Some((one, other)) = rest.strip_prefix("between ").and_then(|r| r.split_once(" and "))
            && let (Some(one), Some(other)) = (date(one, today, Lean::Next), date(other, today, Lean::Next))
        {
            return said((one - other).abs());
        }
        if let Some((from, to)) = rest.strip_prefix("from ").and_then(|r| r.split_once(" to ").or_else(|| r.split_once(" until ")))
            && let (Some(from), Some(to)) = (date(from, today, Lean::Next), date(to, today, Lean::Next))
        {
            return said(to - from);
        }
        if let Some(until) = ["until ", "till ", "to ", "before "].iter().find_map(|p| rest.strip_prefix(p))
            && let (Some(until), Some(now)) = (date(until, today, Lean::Next), now)
        {
            return said(until - now);
        }
        if let Some(since) = rest.strip_prefix("since ")
            && let (Some(since), Some(now)) = (date(since, today, Lean::Last), now)
        {
            return said(now - since);
        }
    }
    for (between, later_first) in [(" - ", true), (" − ", true), (" – ", true), (" minus ", true), (" until ", false), (" to ", false)] {
        if let Some((left, right)) = text.split_once(between)
            && let (Some(left), Some(right)) = (date(left, today, Lean::Next), date(right, today, Lean::Next))
        {
            return said(if later_first { left - right } else { right - left });
        }
    }
    None
}

/// A time of day, `17:30` or `5:30 pm`, as minutes since midnight. `now`, or `time()` as models write it,
/// is the time it is.
fn time_of_day(text: &str, now: Option<i64>) -> Option<i64> {
    let text = text.trim().trim_matches(['\'', '"', '`', ' ']).to_lowercase();
    if ["now", "time()", "time", "the current time", "current time", "now()"].contains(&text.as_str()) {
        return now;
    }
    let text = text.trim_matches(['(', ')']).to_string();
    let (clock, half) = match (text.strip_suffix("pm"), text.strip_suffix("am")) {
        (Some(clock), _) => (clock.trim(), Some(true)),
        (_, Some(clock)) => (clock.trim(), Some(false)),
        _ => (text.as_str(), None),
    };
    let (hours, minutes) = clock.split_once(':')?;
    let (hours, minutes): (i64, i64) = (hours.parse().ok()?, minutes.parse().ok()?);
    if minutes > 59 || hours > if half.is_some() { 12 } else { 24 } {
        return None;
    }
    Some(match half {
        Some(pm) => (hours % 12 + if pm { 12 } else { 0 }) * 60 + minutes,
        None => hours * 60 + minutes,
    })
}

/// `17:30 - 10:19`, `10:19 until 17:30`, `17:30 - now`, `time until 17:30`: how long from one time of day
/// to another. `None` when the expression is not two times. `now` is the time it is, in minutes since midnight.
fn time_between(expression: &str, now: Option<i64>) -> Option<String> {
    let text = expression.trim().trim_end_matches(['=', '?', '.']).trim().to_lowercase();
    // "time until 17:30" is "now until 17:30".
    let until = ["time until ", "how long until ", "time left until ", "time to ", "until "].iter().find_map(|p| text.strip_prefix(p)).map(|then| format!("now until {then}"));
    let text = until.unwrap_or(text);
    for (between, later_first) in [(" - ", true), (" − ", true), (" – ", true), (" minus ", true), (" until ", false), (" to ", false)] {
        if let Some((left, right)) = text.split_once(between)
            && let (Some(left), Some(right)) = (time_of_day(left, now), time_of_day(right, now))
        {
            let minutes = if later_first { left - right } else { right - left };
            let (sign, hours, rest) = (if minutes < 0 { "-" } else { "" }, minutes.abs() / 60, minutes.abs() % 60);
            let words = |n: i64, one: &str| format!("{n} {one}{}", if n == 1 { "" } else { "s" });
            return Some(match (hours, rest) {
                (0, rest) => format!("{sign}{}", words(rest, "minute")),
                (hours, 0) => format!("{sign}{}", words(hours, "hour")),
                (hours, rest) => format!("{sign}{} {}", words(hours, "hour"), words(rest, "minute")),
            });
        }
    }
    None
}

/// Works out an expression. `Err` says what is wrong with it, in words. `today` (a year, a month, a day)
/// is for sums about dates that say "today" or leave the year out; `now` (minutes since midnight) for
/// sums about times that say "now".
pub fn work_out(expression: &str, today: Option<(i64, i64, i64)>, now: Option<i64>) -> Result<String, String> {
    if expression.chars().count() > 500 {
        return Err("the expression is too long. Split it into smaller sums".into());
    }
    if let Some(days) = days_between(expression, today) {
        return Ok(days);
    }
    if let Some(time) = time_between(expression, now) {
        return Ok(time);
    }
    let tokens = tokens(expression)?;
    if tokens.is_empty() {
        return Err("there is nothing to work out".into());
    }
    let mut sum = Sum { tokens, at: 0, depth: 0 };
    let (value, _) = sum.sum()?;
    match sum.peek() {
        None => shown(value),
        Some(Token::Word(w)) => Err(format!("\"{w}\" is not something calculate knows. {HELP}")),
        Some(Token::Number(n)) => Err(format!("there is a {n} with nothing between it and what comes before")),
        Some(Token::Sign(c)) => Err(format!("there is a \"{c}\" that does not belong")),
    }
}

#[cfg(test)]
mod tests {
    /// As if it were 10:19 on Saturday, 21 November 2026.
    fn work_out(expression: &str) -> Result<String, String> {
        super::work_out(expression, Some((2026, 11, 21)), Some(10 * 60 + 19))
    }

    fn is(expression: &str, want: &str) {
        assert_eq!(work_out(expression).as_deref(), Ok(want), "{expression}");
    }

    #[test]
    fn plain_sums() {
        is("437 * 4 + 35", "1783");
        is("2 + 3 * 4", "14");
        is("(2 + 3) * 4", "20");
        is("98765 / 432", "228.622685185");
        is("3 * 24 * 60 * 60 + 7 * 60 * 60", "284400");
        is("-3 - -2", "-1");
        is("2 ^ 10", "1024");
        is("2 ** 0.5 ** 2", "1.189207115");
        is("-2 ^ 2", "-4");
        is("10 % 3", "1");
        is("17 mod 5", "2");
        is("2(3 + 4)", "14");
        is("1e3 + 1", "1001");
        is(".5 + .25", "0.75");
    }

    #[test]
    fn no_noise_from_binary_fractions() {
        is("0.1 + 0.2", "0.3");
        is("1299 * 1.22", "1584.78");
        is("2.40 + 1.15 + 6.80", "10.35");
        is("1 / 3", "0.333333333333");
        is("5 - 5", "0");
        is("123456789 * 1000", "123456789000");
    }

    #[test]
    fn what_models_and_people_write() {
        is("2,340 * 0.18", "421.2");
        is("18% of 2,340", "421.2");
        is("2340 x 18%", "421.2");
        is("1299 + 22%", "1584.78");
        is("200 - 15%", "170");
        is("50%", "0.5");
        is("12% of 4.1 million", "492000");
        is("1_000_000 / 4", "250000");
        is("€1,299 × 2", "2598");
        is("3 times 4 plus 2 minus 1", "13");
        is("144 ÷ 12 =", "12");
        is("sqrt(16) + abs(-2) + round(2.567, 2)", "8.57");
        is("max(1,200) + min(3, 4.5) + sum(1, 2, 3) + avg(2, 4)", "212");
        is("2 * pi", "6.28318530718");
    }

    #[test]
    fn the_days_between_two_dates() {
        is("25 December 2026 - 21 November 2026", "34 days");
        is("#25 December 2026 - Saturday, 21 November 2026, 10:08 (Europe/Rome, UTC+01:00)", "34 days");
        is("21 November 2026 until December 25th, 2026", "34 days");
        is("2026-11-21 to 2026-11-22", "1 day");
        is("1 Jan 2027 - 31 Dec 2026", "1 day");
        is("1 March 2028 - 28 February 2028", "2 days");
        is("1 March 2027 - 28 February 2027", "1 day");
        is("21 Nov 2026 - 25 Dec 2026", "-34 days");
        is("1 January 2000 - 1 January 1900", "36524 days");
        // Asked in words, with today, and without a year: the next such day, or the last one.
        is("days between 25 December and today", "34 days");
        is("days between '2026-12-25' and '2026-11-21'", "34 days");
        is("days until 25 December", "34 days");
        is("number of days left until December 25th", "34 days");
        is("days until 1 March", "100 days");
        is("days since 1 March", "265 days");
        is("days from today to 1 January 2027", "41 days");
        is("25 December - today", "34 days");
        is("days until today", "0 days");
        assert!(super::work_out("days until 25 December", None, None).is_err(), "without knowing today there is nothing to count from");
        // From one time of day to another. A colon is never read as a division.
        is("17:30 - 10:19", "7 hours 11 minutes");
        is("10:19 until 17:30", "7 hours 11 minutes");
        is("5:30 pm - 10:19 am", "7 hours 11 minutes");
        is("12:00 - 11:59", "1 minute");
        is("13:00 - 12:00", "1 hour");
        is("10:00 - 12:30", "-2 hours 30 minutes");
        // With the time it is, however a model writes that.
        is("17:30 - now", "7 hours 11 minutes");
        is("17:30 - time()", "7 hours 11 minutes");
        is("time() - '17:30'", "-7 hours 11 minutes");
        is("time until 17:30", "7 hours 11 minutes");
        is("now until 11:00", "41 minutes");
        assert!(super::work_out("17:30 - now", None, None).is_err());
        assert_eq!(work_out("17:30 / 2").unwrap_err(), "\":\" is not something calculate knows. Use numbers and + - * / ^ ( ) % only");
        assert!(work_out("25:00 - 10:00").is_err());
        // What is not plainly two dates is a sum, or is refused as one.
        is("25 - 12", "13");
        is("2026 - 12 - 25", "1989");
        assert!(work_out("30 February 2026 - 1 January 2026").is_err());
        assert!(work_out("25 December 2026 - my birthday").is_err());
        assert!(work_out("days between Easter and today").is_err());
    }

    #[test]
    fn what_is_wrong_is_said_by_name() {
        let wrong = |expression: &str| work_out(expression).unwrap_err();
        assert_eq!(wrong("12 euros * 3"), "\"euros\" is not something calculate knows. Use numbers and + - * / ^ ( ) % only");
        assert_eq!(wrong("len(a)"), "\"a\" is not something calculate knows. Use numbers and + - * / ^ ( ) % only");
        assert_eq!(wrong("size of a.txt"), "\".\" is not something calculate knows. Use numbers and + - * / ^ ( ) % only");
        assert_eq!(wrong("hours in a week"), "\"hours\" is not something calculate knows. Use numbers and + - * / ^ ( ) % only");
        assert_eq!(wrong("1 / 0"), "that divides by zero");
        assert_eq!(wrong("(1 + 2"), "a bracket is not closed");
        assert_eq!(wrong("2 +"), "the expression ends where a number should be");
        assert_eq!(wrong("2 3"), "there is a 3 with nothing between it and what comes before");
        assert_eq!(wrong("* 3"), "there is a \"*\" where a number should be");
        assert_eq!(wrong(""), "there is nothing to work out");
        assert_eq!(wrong("sqrt(-1)"), "that is the square root of a negative number");
        assert!(wrong("10 ^ 400").contains("too large"));
        assert!(wrong(&"1+".repeat(300)).contains("too long"));
        assert!(wrong(&format!("{}1{}", "(".repeat(80), ")".repeat(80))).contains("too deep"));
    }
}
