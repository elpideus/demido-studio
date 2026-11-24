//! A Pine-compatible engine of Demido's own (DECISIONS M3, M4, M5, M17; research note 33): a first part of the
//! language, evaluated a whole series at a time. It is not TradingView's engine, and a script can behave
//! differently there; "Pine Script" is TradingView's trademark, and Demido says "compatible".
//!
//! What it reads: `indicator(…)`, `name = …` (and `var`, `:=`), `input.int/float/bool`, the series of the chart,
//! `x[n]`, arithmetic, comparisons, `and`/`or`/`not`, `?:`, `ta.*`, `math.*`, `nz`, `na`, `plot`, `hline`, colours.
//! What it does not, and says so with the line and column: `if`, `for`, `while`, functions of the script's own,
//! `request.*`, strategies, drawings, tables. Every error is in words a model can act on, so that a model can
//! write a script, run it, read the error and mend it (M3).

use std::collections::HashMap;
use std::rc::Rc;

use serde::Serialize;
use utoipa::ToSchema;

use super::{Candle, ta};

/// What went wrong, and where: line and column from 1.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct PineError {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl std::fmt::Display for PineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}, column {}: {}", self.line, self.col, self.message)
    }
}

/// One line a script plots, a value for each candle (`None`: nothing there).
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Plot {
    pub title: String,
    pub color: String,
    pub values: Vec<Option<f64>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Hline {
    pub price: f64,
    pub title: String,
    pub color: String,
}

/// What a script gave.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Ran {
    pub title: String,
    /// Drawn over the candles; otherwise in a pane of its own.
    pub overlay: bool,
    pub plots: Vec<Plot>,
    pub hlines: Vec<Hline>,
}

// ───────── reading ─────────

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Name(String),
    Color(String),
    Op(&'static str),
    Newline,
}

#[derive(Clone, Debug)]
struct Token {
    tok: Tok,
    line: usize,
    col: usize,
}

fn err<T>(line: usize, col: usize, message: impl Into<String>) -> Result<T, PineError> {
    Err(PineError { line, col, message: message.into() })
}

const OPS: &[&str] = &[":=", "==", "!=", "<=", ">=", "=>", "=", "<", ">", "+", "-", "*", "/", "%", "(", ")", "[", "]", ",", "?", ":"];

fn lex(source: &str) -> Result<Vec<Token>, PineError> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    for (n, raw) in source.lines().enumerate() {
        let line = n + 1;
        let text = raw.split("//").next().unwrap_or("");
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        let indented = chars.first().is_some_and(|c| c.is_whitespace());
        let mut any = false;
        while i < chars.len() {
            let c = chars[i];
            let col = i + 1;
            if c.is_whitespace() {
                i += 1;
                continue;
            }
            any = true;
            if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == 'e' || ((chars[i] == '-' || chars[i] == '+') && chars[i - 1] == 'e')) {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                match s.parse::<f64>() {
                    Ok(v) => out.push(Token { tok: Tok::Num(v), line, col }),
                    Err(_) => return err(line, col, format!("\"{s}\" is not a number")),
                }
                continue;
            }
            if c == '"' || c == '\'' {
                let end = chars[i + 1..].iter().position(|d| *d == c).map(|p| i + 1 + p);
                let Some(end) = end else { return err(line, col, "a text is opened here and never closed on this line") };
                out.push(Token { tok: Tok::Str(chars[i + 1..end].iter().collect()), line, col });
                i = end + 1;
                continue;
            }
            if c == '#' {
                let hex: String = chars[i + 1..].iter().take_while(|d| d.is_ascii_hexdigit()).collect();
                if hex.len() != 6 && hex.len() != 8 {
                    return err(line, col, "a colour is written #rrggbb");
                }
                out.push(Token { tok: Tok::Color(format!("#{}", &hex[..6])), line, col });
                i += 1 + hex.len();
                continue;
            }
            if c.is_alphabetic() || c == '_' {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.') {
                    i += 1;
                }
                out.push(Token { tok: Tok::Name(chars[start..i].iter().collect()), line, col });
                continue;
            }
            let rest: String = chars[i..].iter().take(2).collect();
            let Some(op) = OPS.iter().find(|op| rest.starts_with(**op)) else { return err(line, col, format!("\"{c}\" is not part of the language Demido's engine reads")) };
            match *op {
                "(" | "[" => depth += 1,
                ")" | "]" => depth -= 1,
                _ => {}
            }
            out.push(Token { tok: Tok::Op(op), line, col });
            i += op.len();
        }
        // A line goes on to the next while a bracket is open, or when the next is indented (Pine's way).
        let next_indented = source.lines().nth(n + 1).is_some_and(|l| l.starts_with(' ') || l.starts_with('\t')) && !source.lines().nth(n + 1).is_some_and(|l| l.trim().is_empty());
        let _ = indented;
        if any && depth <= 0 && !next_indented {
            out.push(Token { tok: Tok::Newline, line, col: chars.len() + 1 });
        }
    }
    out.push(Token { tok: Tok::Newline, line: source.lines().count() + 1, col: 1 });
    Ok(out)
}

#[derive(Clone, Debug)]
enum Expr {
    Num(f64),
    Str(String),
    Color(String),
    Name(String, usize, usize),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Call(String, Vec<(Option<String>, Expr)>, usize, usize),
}

struct Parser {
    toks: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.toks[self.at.min(self.toks.len() - 1)]
    }
    fn next(&mut self) -> Token {
        let t = self.peek().clone();
        self.at += 1;
        t
    }
    fn is(&self, op: &str) -> bool {
        matches!(&self.peek().tok, Tok::Op(o) if *o == op)
    }
    fn expect(&mut self, op: &str) -> Result<(), PineError> {
        if self.is(op) {
            self.at += 1;
            Ok(())
        } else {
            let t = self.peek();
            err(t.line, t.col, format!("\"{op}\" was expected here"))
        }
    }

    fn expr(&mut self) -> Result<Expr, PineError> {
        let cond = self.or()?;
        if self.is("?") {
            self.at += 1;
            let a = self.expr()?;
            self.expect(":")?;
            let b = self.expr()?;
            return Ok(Expr::Ternary(Box::new(cond), Box::new(a), Box::new(b)));
        }
        Ok(cond)
    }
    fn word(&self, w: &str) -> bool {
        matches!(&self.peek().tok, Tok::Name(n) if n == w)
    }
    fn or(&mut self) -> Result<Expr, PineError> {
        let mut left = self.and()?;
        while self.word("or") {
            self.at += 1;
            left = Expr::Binary("or", Box::new(left), Box::new(self.and()?));
        }
        Ok(left)
    }
    fn and(&mut self) -> Result<Expr, PineError> {
        let mut left = self.not()?;
        while self.word("and") {
            self.at += 1;
            left = Expr::Binary("and", Box::new(left), Box::new(self.not()?));
        }
        Ok(left)
    }
    fn not(&mut self) -> Result<Expr, PineError> {
        if self.word("not") {
            self.at += 1;
            return Ok(Expr::Unary("not", Box::new(self.not()?)));
        }
        self.compare()
    }
    fn compare(&mut self) -> Result<Expr, PineError> {
        let mut left = self.sum()?;
        loop {
            let op = match &self.peek().tok {
                Tok::Op(o) if ["==", "!=", "<", ">", "<=", ">="].contains(o) => *o,
                _ => return Ok(left),
            };
            self.at += 1;
            left = Expr::Binary(op, Box::new(left), Box::new(self.sum()?));
        }
    }
    fn sum(&mut self) -> Result<Expr, PineError> {
        let mut left = self.product()?;
        loop {
            let op = match &self.peek().tok {
                Tok::Op(o) if *o == "+" || *o == "-" => *o,
                _ => return Ok(left),
            };
            self.at += 1;
            left = Expr::Binary(op, Box::new(left), Box::new(self.product()?));
        }
    }
    fn product(&mut self) -> Result<Expr, PineError> {
        let mut left = self.unary()?;
        loop {
            let op = match &self.peek().tok {
                Tok::Op(o) if ["*", "/", "%"].contains(o) => *o,
                _ => return Ok(left),
            };
            self.at += 1;
            left = Expr::Binary(op, Box::new(left), Box::new(self.unary()?));
        }
    }
    fn unary(&mut self) -> Result<Expr, PineError> {
        if self.is("-") {
            self.at += 1;
            return Ok(Expr::Unary("-", Box::new(self.unary()?)));
        }
        if self.is("+") {
            self.at += 1;
            return self.unary();
        }
        self.postfix()
    }
    fn postfix(&mut self) -> Result<Expr, PineError> {
        let mut e = self.atom()?;
        while self.is("[") {
            self.at += 1;
            let n = self.expr()?;
            self.expect("]")?;
            e = Expr::Index(Box::new(e), Box::new(n));
        }
        Ok(e)
    }
    fn atom(&mut self) -> Result<Expr, PineError> {
        let t = self.next();
        match t.tok {
            Tok::Num(v) => Ok(Expr::Num(v)),
            Tok::Str(s) => Ok(Expr::Str(s)),
            Tok::Color(c) => Ok(Expr::Color(c)),
            Tok::Op("(") => {
                let e = self.expr()?;
                self.expect(")")?;
                Ok(e)
            }
            Tok::Name(name) => {
                if ["if", "for", "while", "switch"].contains(&name.as_str()) {
                    return err(t.line, t.col, format!("\"{name}\" is not in Demido's engine yet. Write the choice with ?: instead (condition ? a : b)"));
                }
                if self.is("(") {
                    self.at += 1;
                    let mut args = Vec::new();
                    while !self.is(")") {
                        let named = match (&self.peek().tok, self.toks.get(self.at + 1).map(|t| &t.tok)) {
                            (Tok::Name(n), Some(Tok::Op("="))) => Some(n.clone()),
                            _ => None,
                        };
                        if named.is_some() {
                            self.at += 2;
                        }
                        args.push((named, self.expr()?));
                        if !self.is(")") {
                            self.expect(",")?;
                        }
                    }
                    self.at += 1;
                    return Ok(Expr::Call(name, args, t.line, t.col));
                }
                Ok(Expr::Name(name, t.line, t.col))
            }
            Tok::Newline => err(t.line, t.col, "the line ends where a value was expected"),
            Tok::Op(o) => err(t.line, t.col, format!("\"{o}\" is where a value was expected")),
        }
    }
}

// ───────── running ─────────

#[derive(Clone, Debug)]
enum Val {
    Num(f64),
    Series(Rc<Vec<f64>>),
    Str(String),
    Color(String),
    Nothing,
}

fn color(name: &str) -> Option<&'static str> {
    Some(match name {
        "color.green" => "#4caf50",
        "color.red" => "#f23645",
        "color.blue" => "#2962ff",
        "color.orange" => "#ff9800",
        "color.purple" => "#9c27b0",
        "color.yellow" => "#ffeb3b",
        "color.gray" => "#787b86",
        "color.white" => "#ffffff",
        "color.black" => "#363a45",
        "color.teal" => "#089981",
        "color.aqua" => "#00bcd4",
        "color.lime" => "#00e676",
        "color.maroon" => "#880e4f",
        "color.navy" => "#311b92",
        "color.olive" => "#808000",
        "color.silver" => "#b2b5be",
        "color.fuchsia" => "#e040fb",
        _ => return None,
    })
}

/// The functions Demido's engine knows, for the error that names an unknown one.
const KNOWN: &str = "ta.sma, ta.ema, ta.rma, ta.rsi, ta.stdev, ta.highest, ta.lowest, ta.change, ta.crossover, ta.crossunder, ta.atr, math.abs, math.max, math.min, math.sqrt, math.pow, math.log, math.exp, math.round, nz, na, input, input.int, input.float, input.bool, color.new, plot, hline, indicator";

struct Run<'a> {
    bars: &'a [Candle],
    names: HashMap<String, Val>,
    out: Ran,
    plotted: usize,
}

impl Run<'_> {
    fn n(&self) -> usize {
        self.bars.len()
    }
    fn series(&self, v: &Val, line: usize, col: usize) -> Result<Rc<Vec<f64>>, PineError> {
        match v {
            Val::Series(s) => Ok(s.clone()),
            Val::Num(x) => Ok(Rc::new(vec![*x; self.n()])),
            Val::Str(_) => err(line, col, "a text is where a number or a series was expected"),
            Val::Color(_) => err(line, col, "a colour is where a number or a series was expected"),
            Val::Nothing => err(line, col, "this gives no value"),
        }
    }
    fn length(&self, v: &Val, line: usize, col: usize, what: &str) -> Result<usize, PineError> {
        match v {
            Val::Num(x) if *x >= 1.0 && x.fract() == 0.0 => Ok(*x as usize),
            Val::Num(x) => err(line, col, format!("{what} must be a whole number of bars, 1 or more, not {x}")),
            _ => err(line, col, format!("{what} must be a number of bars written in the script or by input.int, not a series")),
        }
    }

    fn name(&self, name: &str, line: usize, col: usize) -> Result<Val, PineError> {
        if let Some(v) = self.names.get(name) {
            return Ok(v.clone());
        }
        let b = self.bars;
        let s = |f: fn(&Candle) -> f64| Val::Series(Rc::new(b.iter().map(f).collect()));
        Ok(match name {
            "open" => s(|c| c.o),
            "high" => s(|c| c.h),
            "low" => s(|c| c.l),
            "close" => s(|c| c.c),
            "volume" => s(|c| c.v),
            "time" => s(|c| c.t as f64),
            "hl2" => s(|c| (c.h + c.l) / 2.0),
            "hlc3" => s(|c| (c.h + c.l + c.c) / 3.0),
            "ohlc4" => s(|c| (c.o + c.h + c.l + c.c) / 4.0),
            "bar_index" => Val::Series(Rc::new((0..b.len()).map(|i| i as f64).collect())),
            "true" => Val::Num(1.0),
            "false" => Val::Num(0.0),
            "na" => Val::Num(f64::NAN),
            "ta.tr" => Val::Series(Rc::new(ta::tr(&b.iter().map(|c| c.h).collect::<Vec<_>>(), &b.iter().map(|c| c.l).collect::<Vec<_>>(), &b.iter().map(|c| c.c).collect::<Vec<_>>()))),
            other => match color(other) {
                Some(c) => Val::Color(c.into()),
                None if other.starts_with("request.") || other.starts_with("strategy") => {
                    return err(line, col, format!("\"{other}\" is not in Demido's engine: it reads only the chart it runs on, and has no strategies"));
                }
                None => return err(line, col, format!("\"{other}\" is not defined. Give it a value first (name = …), or use one of open, high, low, close, volume, hl2, hlc3, ohlc4, bar_index")),
            },
        })
    }

    fn eval(&mut self, e: &Expr) -> Result<Val, PineError> {
        match e {
            Expr::Num(v) => Ok(Val::Num(*v)),
            Expr::Str(s) => Ok(Val::Str(s.clone())),
            Expr::Color(c) => Ok(Val::Color(c.clone())),
            Expr::Name(n, l, c) => self.name(n, *l, *c),
            Expr::Unary(op, x) => {
                let v = self.eval(x)?;
                Ok(match (op, v) {
                    (&"-", Val::Num(x)) => Val::Num(-x),
                    (&"not", Val::Num(x)) => Val::Num(if x == 0.0 { 1.0 } else { 0.0 }),
                    (op, v) => {
                        let s = self.series(&v, 0, 0)?;
                        Val::Series(Rc::new(
                            s.iter()
                                .map(|x| {
                                    if *op == "-" {
                                        -x
                                    } else if *x == 0.0 {
                                        1.0
                                    } else {
                                        0.0
                                    }
                                })
                                .collect(),
                        ))
                    }
                })
            }
            Expr::Binary(op, a, b) => {
                let (a, b) = (self.eval(a)?, self.eval(b)?);
                let f = |x: f64, y: f64| -> f64 {
                    match *op {
                        "+" => x + y,
                        "-" => x - y,
                        "*" => x * y,
                        "/" => x / y,
                        "%" => x % y,
                        "==" => ((x - y).abs() < 1e-12) as u8 as f64,
                        "!=" => ((x - y).abs() >= 1e-12) as u8 as f64,
                        "<" => (x < y) as u8 as f64,
                        ">" => (x > y) as u8 as f64,
                        "<=" => (x <= y) as u8 as f64,
                        ">=" => (x >= y) as u8 as f64,
                        "and" => (x != 0.0 && y != 0.0) as u8 as f64,
                        _ => (x != 0.0 || y != 0.0) as u8 as f64,
                    }
                };
                match (&a, &b) {
                    (Val::Num(x), Val::Num(y)) => Ok(Val::Num(f(*x, *y))),
                    _ => {
                        let (x, y) = (self.series(&a, 0, 0)?, self.series(&b, 0, 0)?);
                        Ok(Val::Series(Rc::new(x.iter().zip(y.iter()).map(|(x, y)| f(*x, *y)).collect())))
                    }
                }
            }
            Expr::Ternary(c, a, b) => {
                let (c, a, b) = (self.eval(c)?, self.eval(a)?, self.eval(b)?);
                if let (Val::Num(c), Val::Num(a), Val::Num(b)) = (&c, &a, &b) {
                    return Ok(Val::Num(if *c != 0.0 { *a } else { *b }));
                }
                let (c, a, b) = (self.series(&c, 0, 0)?, self.series(&a, 0, 0)?, self.series(&b, 0, 0)?);
                Ok(Val::Series(Rc::new((0..self.n()).map(|i| if c[i] != 0.0 && !c[i].is_nan() { a[i] } else { b[i] }).collect())))
            }
            Expr::Index(x, n) => {
                let v = self.eval(x)?;
                let back = match self.eval(n)? {
                    Val::Num(k) if k >= 0.0 && k.fract() == 0.0 => k as usize,
                    _ => return err(0, 0, "the number of bars back in x[n] must be a whole number"),
                };
                Ok(Val::Series(Rc::new(ta::back(&self.series(&v, 0, 0)?, back))))
            }
            Expr::Call(name, args, line, col) => self.call(name, args, *line, *col),
        }
    }

    fn call(&mut self, name: &str, args: &[(Option<String>, Expr)], line: usize, col: usize) -> Result<Val, PineError> {
        let mut pos = Vec::new();
        let mut named = HashMap::new();
        for (n, e) in args {
            let v = self.eval(e)?;
            match n {
                Some(n) => {
                    named.insert(n.clone(), v);
                }
                None => pos.push(v),
            }
        }
        let arg = |i: usize, n: &str| -> Option<Val> { named.get(n).cloned().or_else(|| pos.get(i).cloned()) };
        let need = |i: usize, n: &str| -> Result<Val, PineError> { arg(i, n).ok_or_else(|| PineError { line, col, message: format!("{name} needs its \"{n}\"") }) };
        let text = |v: Option<Val>| match v {
            Some(Val::Str(s)) => Some(s),
            _ => None,
        };
        let src_len = |me: &Self, n_at: usize| -> Result<(Rc<Vec<f64>>, usize), PineError> {
            let src = me.series(&need(0, "source")?, line, col)?;
            let len = me.length(&need(n_at, "length")?, line, col, &format!("the length of {name}"))?;
            Ok((src, len))
        };
        let series = |v: Vec<f64>| Ok(Val::Series(Rc::new(v)));
        match name {
            "indicator" | "study" => {
                self.out.title = text(arg(0, "title")).unwrap_or_else(|| "Script".into());
                self.out.overlay = matches!(arg(99, "overlay"), Some(Val::Num(x)) if x != 0.0);
                Ok(Val::Nothing)
            }
            "strategy" => err(line, col, "strategies are not in Demido's engine: write it as an indicator(…)"),
            "input" | "input.int" | "input.float" | "input.bool" | "input.source" => match need(0, "defval")? {
                Val::Num(x) => Ok(Val::Num(x)),
                Val::Series(s) => Ok(Val::Series(s)),
                _ => err(line, col, format!("{name}'s first value must be its default")),
            },
            "ta.sma" => src_len(self, 1).and_then(|(s, n)| series(ta::sma(&s, n))),
            "ta.ema" => src_len(self, 1).and_then(|(s, n)| series(ta::ema(&s, n))),
            "ta.rma" => src_len(self, 1).and_then(|(s, n)| series(ta::rma(&s, n))),
            "ta.rsi" => src_len(self, 1).and_then(|(s, n)| series(ta::rsi(&s, n))),
            "ta.stdev" => src_len(self, 1).and_then(|(s, n)| series(ta::stdev(&s, n))),
            "ta.highest" | "ta.lowest" => {
                // ta.highest(length) is of high; ta.highest(source, length) of the source.
                let (src, len) = if pos.len() == 1 && named.is_empty() {
                    (self.series(&self.name(if name == "ta.highest" { "high" } else { "low" }, line, col)?, line, col)?, self.length(&pos[0], line, col, "the length")?)
                } else {
                    src_len(self, 1)?
                };
                series(if name == "ta.highest" { ta::highest(&src, len) } else { ta::lowest(&src, len) })
            }
            "ta.change" => {
                let src = self.series(&need(0, "source")?, line, col)?;
                let len = arg(1, "length").map(|v| self.length(&v, line, col, "the length of ta.change")).transpose()?.unwrap_or(1);
                series(ta::change(&src, len))
            }
            "ta.crossover" | "ta.crossunder" => {
                let (a, b) = (self.series(&need(0, "source1")?, line, col)?, self.series(&need(1, "source2")?, line, col)?);
                series(if name == "ta.crossover" { ta::crossover(&a, &b) } else { ta::crossunder(&a, &b) })
            }
            "ta.atr" => {
                let len = self.length(&need(0, "length")?, line, col, "the length of ta.atr")?;
                let tr = self.series(&self.name("ta.tr", line, col)?, line, col)?;
                series(ta::rma(&tr, len))
            }
            "math.abs" | "math.sqrt" | "math.log" | "math.exp" | "math.round" => {
                let f: fn(f64) -> f64 = match name {
                    "math.abs" => f64::abs,
                    "math.sqrt" => f64::sqrt,
                    "math.log" => f64::ln,
                    "math.exp" => f64::exp,
                    _ => f64::round,
                };
                match need(0, "number")? {
                    Val::Num(x) => Ok(Val::Num(f(x))),
                    v => series(self.series(&v, line, col)?.iter().map(|x| f(*x)).collect()),
                }
            }
            "math.max" | "math.min" | "math.pow" => {
                let (a, b) = (need(0, "number1")?, need(1, "number2")?);
                let f = |x: f64, y: f64| match name {
                    "math.max" => x.max(y),
                    "math.min" => x.min(y),
                    _ => x.powf(y),
                };
                if let (Val::Num(x), Val::Num(y)) = (&a, &b) {
                    return Ok(Val::Num(f(*x, *y)));
                }
                let (x, y) = (self.series(&a, line, col)?, self.series(&b, line, col)?);
                series(x.iter().zip(y.iter()).map(|(x, y)| f(*x, *y)).collect())
            }
            "nz" => {
                let r = match arg(1, "replacement") {
                    Some(Val::Num(r)) => r,
                    _ => 0.0,
                };
                match need(0, "source")? {
                    Val::Num(x) => Ok(Val::Num(if x.is_nan() { r } else { x })),
                    v => series(self.series(&v, line, col)?.iter().map(|x| if x.is_nan() { r } else { *x }).collect()),
                }
            }
            "na" => match need(0, "x")? {
                Val::Num(x) => Ok(Val::Num(x.is_nan() as u8 as f64)),
                v => series(self.series(&v, line, col)?.iter().map(|x| x.is_nan() as u8 as f64).collect()),
            },
            "color.new" => match need(0, "color")? {
                Val::Color(c) => Ok(Val::Color(c)),
                _ => err(line, col, "color.new's first value must be a colour"),
            },
            "plot" => {
                let values = self.series(&need(0, "series")?, line, col)?;
                self.plotted += 1;
                let title = text(arg(1, "title")).unwrap_or_else(|| format!("Plot {}", self.plotted));
                let color = match arg(2, "color") {
                    Some(Val::Color(c)) => c,
                    _ => "#2962ff".into(),
                };
                self.out.plots.push(Plot { title, color, values: values.iter().map(|v| (!v.is_nan() && v.is_finite()).then_some(*v)).collect() });
                Ok(Val::Nothing)
            }
            "hline" => {
                let price = match need(0, "price")? {
                    Val::Num(p) => p,
                    _ => return err(line, col, "hline's price must be a number"),
                };
                let title = text(arg(1, "title")).unwrap_or_default();
                let color = match arg(2, "color") {
                    Some(Val::Color(c)) => c,
                    _ => "#787b86".into(),
                };
                self.out.hlines.push(Hline { price, title, color });
                Ok(Val::Nothing)
            }
            other
                if other.starts_with("plotshape")
                    || other.starts_with("plotchar")
                    || other.starts_with("bgcolor")
                    || other.starts_with("fill")
                    || other.starts_with("label.")
                    || other.starts_with("line.")
                    || other.starts_with("box.")
                    || other.starts_with("table.") =>
            {
                err(line, col, format!("{other} is not in Demido's engine yet: it draws only plot and hline"))
            }
            other if other.starts_with("request.") => err(line, col, format!("{other} is not in Demido's engine: a script reads only the chart it runs on")),
            other => err(line, col, format!("{other} is not known to Demido's engine. It knows: {KNOWN}")),
        }
    }
}

/// Runs a script on a chart's candles.
pub fn run(source: &str, bars: &[Candle]) -> Result<Ran, PineError> {
    if source.len() > 100_000 {
        return err(1, 1, "the script is longer than 100,000 characters");
    }
    let toks = lex(source)?;
    let mut p = Parser { toks, at: 0 };
    let mut run = Run { bars, names: HashMap::new(), out: Ran { title: "Script".into(), overlay: false, plots: Vec::new(), hlines: Vec::new() }, plotted: 0 };
    let mut statements = 0;
    while p.at < p.toks.len() {
        if matches!(p.peek().tok, Tok::Newline) {
            p.at += 1;
            continue;
        }
        statements += 1;
        if statements > 2_000 {
            return err(p.peek().line, 1, "a script of more than 2,000 statements is not run");
        }
        let start = p.peek().clone();
        // `var name = …`, `name = …`, `name := …`, or a call on its own.
        let mut offset = 0;
        if matches!(&start.tok, Tok::Name(n) if n == "var" || n == "varip") {
            offset = 1;
        }
        let is_assignment =
            matches!(p.toks.get(p.at + offset).map(|t| &t.tok), Some(Tok::Name(_))) && matches!(p.toks.get(p.at + offset + 1).map(|t| &t.tok), Some(Tok::Op("=")) | Some(Tok::Op(":=")));
        // A type before the name (`float x = …`) is read past.
        let typed = !is_assignment
            && matches!(&p.toks.get(p.at + offset).map(|t| &t.tok), Some(Tok::Name(t)) if ["int", "float", "bool", "string", "color"].contains(&t.as_str()))
            && matches!(p.toks.get(p.at + offset + 2).map(|t| &t.tok), Some(Tok::Op("=")) | Some(Tok::Op(":=")));
        if typed {
            offset += 1;
        }
        if is_assignment || typed {
            p.at += offset;
            let Tok::Name(name) = p.next().tok else { unreachable!() };
            if matches!(p.peek().tok, Tok::Op("=>")) {
                return err(start.line, start.col, "functions of the script's own (=>) are not in Demido's engine yet");
            }
            p.at += 1;
            if matches!(p.peek().tok, Tok::Op("=>")) || p.toks.get(p.at.saturating_sub(1)).is_some_and(|t| matches!(t.tok, Tok::Op("=>"))) {
                return err(start.line, start.col, "functions of the script's own (=>) are not in Demido's engine yet");
            }
            let value = p.expr()?;
            let v = run.eval(&value)?;
            run.names.insert(name, v);
        } else {
            // `name(args) =>` is a function of the script's own.
            let e = p.expr()?;
            if matches!(p.peek().tok, Tok::Op("=>")) {
                return err(start.line, start.col, "functions of the script's own (=>) are not in Demido's engine yet");
            }
            run.eval(&e)?;
        }
        match p.peek().tok {
            Tok::Newline => p.at += 1,
            _ => {
                let t = p.peek();
                return err(t.line, t.col, "something follows where the line should end");
            }
        }
    }
    if run.out.plots.is_empty() && run.out.hlines.is_empty() {
        return err(1, 1, "the script plots nothing: end it with plot(…)");
    }
    Ok(run.out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bars(closes: &[f64]) -> Vec<Candle> {
        closes.iter().enumerate().map(|(i, c)| Candle { t: i as i64 * 60_000, o: *c, h: c + 1.0, l: c - 1.0, c: *c, v: 10.0 }).collect()
    }

    #[test]
    fn a_plain_indicator_runs_and_gives_what_the_same_maths_gives() {
        let b = bars(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        let ran = run(
            "//@version=6\nindicator(\"My average\", overlay=true)\nlen = input.int(3, \"Length\")\nfast = ta.sma(close, len)\nslow = ta.ema(close,\n     len + 1)\nplot(fast, \"Fast\", color=color.green)\nplot(slow, title=\"Slow\", color=#ff0000)\nhline(5, \"Middle\")",
            &b,
        )
        .unwrap();
        assert_eq!((ran.title.as_str(), ran.overlay), ("My average", true));
        assert_eq!(ran.plots[0].title, "Fast");
        assert_eq!(ran.plots[0].values[..4], [None, None, Some(2.0), Some(3.0)]);
        assert_eq!(ran.plots[1].color, "#ff0000");
        assert_eq!(ran.plots[1].values[3], Some(2.5), "the EMA of 4, seeded with the mean of the first four");
        assert_eq!(ran.hlines[0], Hline { price: 5.0, title: "Middle".into(), color: "#787b86".into() });
    }

    #[test]
    fn history_choices_and_crossings_work_a_series_at_a_time() {
        let b = bars(&[5.0, 4.0, 6.0, 7.0, 3.0]);
        let ran = run("indicator(\"x\")\nup = close > close[1]\nplot(up ? 1 : -1)\nplot(ta.crossover(close, 5.5))\nplot(nz(close[1], 0) + math.max(close, 6))", &b).unwrap();
        assert_eq!(ran.plots[0].values, [Some(-1.0), Some(-1.0), Some(1.0), Some(1.0), Some(-1.0)]);
        assert_eq!(ran.plots[1].values, [Some(0.0), Some(0.0), Some(1.0), Some(0.0), Some(0.0)]);
        assert_eq!(ran.plots[2].values[0], Some(6.0));
    }

    #[test]
    fn an_error_names_its_line_and_column_and_says_what_to_do() {
        let b = bars(&[1.0, 2.0, 3.0]);
        let e = run("indicator(\"x\")\nplot(ta.wma(close, 3))", &b).unwrap_err();
        assert_eq!((e.line, e.col), (2, 6));
        assert!(e.message.starts_with("ta.wma is not known to Demido's engine. It knows: ta.sma"), "{e}");
        let e = run("indicator(\"x\")\nif close > 1\n    plot(close)", &b).unwrap_err();
        assert!(e.message.contains("Write the choice with ?: instead"), "{e}");
        let e = run("indicator(\"x\")\nplot(ta.sma(close, close))", &b).unwrap_err();
        assert!(e.message.contains("must be a number of bars"), "{e}");
        let e = run("indicator(\"x\")\nx = price + 1\nplot(x)", &b).unwrap_err();
        assert_eq!((e.line, e.message.starts_with("\"price\" is not defined")), (2, true), "{e}");
        let e = run("indicator(\"x\")\nplot(close", &b).unwrap_err();
        assert!(e.message.contains("expected"), "{e}");
        let e = run("indicator(\"x\")\nv = close", &b).unwrap_err();
        assert!(e.message.contains("plots nothing"), "{e}");
        let e = run("f(x) => x * 2\nplot(f(close))", &b).unwrap_err();
        assert!(e.message.contains("functions of the script's own"), "{e}");
        assert!(run("indicator(\"x\")\nplot(request.security(\"BTC\", \"D\", close))", &b).unwrap_err().message.contains("reads only the chart"));
    }

    #[test]
    fn the_built_in_indicators_are_scripts_that_run() {
        let b: Vec<Candle> = bars(&(0..60).map(|i| 100.0 + (i as f64 / 3.0).sin() * 5.0).collect::<Vec<_>>());
        for (_, _, source) in super::super::BUILT_IN {
            let ran = run(source, &b).unwrap_or_else(|e| panic!("{source}: {e}"));
            assert!(!ran.plots.is_empty());
        }
    }
}
