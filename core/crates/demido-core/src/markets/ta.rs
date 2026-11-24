//! Indicators, worked out here (DECISIONS M2): each takes a series and gives one of the same length, with `NaN`
//! where there is not enough yet (Pine's `na`). As TradingView's `ta.*` work them out, so that a Pine script
//! gives the same numbers in Demido's engine for these.

/// The mean of the last `n`.
pub fn sma(x: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; x.len()];
    if n == 0 {
        return out;
    }
    for i in (n - 1)..x.len() {
        let w = &x[i + 1 - n..=i];
        out[i] = if w.iter().any(|v| v.is_nan()) { f64::NAN } else { w.iter().sum::<f64>() / n as f64 };
    }
    out
}

/// An exponential mean with `alpha`, seeded with the plain mean of the first `n` that are there.
fn smoothed(x: &[f64], n: usize, alpha: f64) -> Vec<f64> {
    let mut out = vec![f64::NAN; x.len()];
    if n == 0 {
        return out;
    }
    let mut prev = f64::NAN;
    let mut seed = Vec::new();
    for (i, v) in x.iter().enumerate() {
        if v.is_nan() {
            continue;
        }
        if prev.is_nan() {
            seed.push(*v);
            if seed.len() == n {
                prev = seed.iter().sum::<f64>() / n as f64;
                out[i] = prev;
            }
        } else {
            prev = alpha * v + (1.0 - alpha) * prev;
            out[i] = prev;
        }
    }
    out
}

/// `ta.ema`: alpha = 2 / (n + 1).
pub fn ema(x: &[f64], n: usize) -> Vec<f64> {
    smoothed(x, n, 2.0 / (n as f64 + 1.0))
}

/// `ta.rma`, Wilder's: alpha = 1 / n.
pub fn rma(x: &[f64], n: usize) -> Vec<f64> {
    smoothed(x, n, 1.0 / n.max(1) as f64)
}

/// The population standard deviation of the last `n` (`ta.stdev`).
pub fn stdev(x: &[f64], n: usize) -> Vec<f64> {
    let means = sma(x, n);
    let mut out = vec![f64::NAN; x.len()];
    for i in 0..x.len() {
        if means[i].is_nan() {
            continue;
        }
        let w = &x[i + 1 - n..=i];
        out[i] = (w.iter().map(|v| (v - means[i]).powi(2)).sum::<f64>() / n as f64).sqrt();
    }
    out
}

/// `ta.rsi`: Wilder's smoothing of the gains and the losses.
pub fn rsi(x: &[f64], n: usize) -> Vec<f64> {
    let change = change(x, 1);
    let up: Vec<f64> = change.iter().map(|c| if c.is_nan() { f64::NAN } else { c.max(0.0) }).collect();
    let down: Vec<f64> = change.iter().map(|c| if c.is_nan() { f64::NAN } else { (-c).max(0.0) }).collect();
    let (u, d) = (rma(&up, n), rma(&down, n));
    u.iter()
        .zip(&d)
        .map(|(u, d)| {
            if u.is_nan() || d.is_nan() {
                f64::NAN
            } else if *d == 0.0 {
                100.0
            } else if *u == 0.0 {
                0.0
            } else {
                100.0 - 100.0 / (1.0 + u / d)
            }
        })
        .collect()
}

/// The highest of the last `n`.
pub fn highest(x: &[f64], n: usize) -> Vec<f64> {
    window(x, n, |w| w.iter().copied().fold(f64::NEG_INFINITY, f64::max))
}

/// The lowest of the last `n`.
pub fn lowest(x: &[f64], n: usize) -> Vec<f64> {
    window(x, n, |w| w.iter().copied().fold(f64::INFINITY, f64::min))
}

fn window(x: &[f64], n: usize, f: impl Fn(&[f64]) -> f64) -> Vec<f64> {
    let mut out = vec![f64::NAN; x.len()];
    if n == 0 {
        return out;
    }
    for i in (n - 1)..x.len() {
        let w = &x[i + 1 - n..=i];
        if !w.iter().any(|v| v.is_nan()) {
            out[i] = f(w);
        }
    }
    out
}

/// The value now less the value `n` bars back (`ta.change`).
pub fn change(x: &[f64], n: usize) -> Vec<f64> {
    (0..x.len()).map(|i| if i >= n { x[i] - x[i - n] } else { f64::NAN }).collect()
}

/// `x` shifted `n` bars back (`x[n]`).
pub fn back(x: &[f64], n: usize) -> Vec<f64> {
    (0..x.len()).map(|i| if i >= n { x[i - n] } else { f64::NAN }).collect()
}

/// `ta.crossover`: `a` was at or under `b`, and is over it now. 1 or 0.
pub fn crossover(a: &[f64], b: &[f64]) -> Vec<f64> {
    (0..a.len()).map(|i| if i > 0 && a[i] > b[i] && a[i - 1] <= b[i - 1] { 1.0 } else { 0.0 }).collect()
}

pub fn crossunder(a: &[f64], b: &[f64]) -> Vec<f64> {
    (0..a.len()).map(|i| if i > 0 && a[i] < b[i] && a[i - 1] >= b[i - 1] { 1.0 } else { 0.0 }).collect()
}

/// `ta.tr`: the true range.
pub fn tr(h: &[f64], l: &[f64], c: &[f64]) -> Vec<f64> {
    (0..h.len()).map(|i| if i == 0 { h[0] - l[0] } else { (h[i] - l[i]).max((h[i] - c[i - 1]).abs()).max((l[i] - c[i - 1]).abs()) }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &[f64], b: &[f64]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x.is_nan() && y.is_nan()) || (x - y).abs() < 1e-9)
    }

    #[test]
    fn means_and_spreads_are_worked_out_as_pine_works_them_out() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        assert!(close(&sma(&x, 3), &[f64::NAN, f64::NAN, 2.0, 3.0, 4.0, 5.0]));
        // Seeded with the mean of the first three (2), then alpha = 0.5.
        assert!(close(&ema(&x, 3), &[f64::NAN, f64::NAN, 2.0, 3.0, 4.0, 5.0]));
        assert!(close(&rma(&x, 2), &[f64::NAN, 1.5, 2.25, 3.125, 4.0625, 5.03125]));
        let s = stdev(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0], 8);
        assert!((s[7] - 2.0).abs() < 1e-12, "{s:?}");
        assert!(close(&highest(&x, 2), &[f64::NAN, 2.0, 3.0, 4.0, 5.0, 6.0]));
        assert!(close(&change(&x, 2), &[f64::NAN, f64::NAN, 2.0, 2.0, 2.0, 2.0]));
        assert!(close(&crossover(&[1.0, 3.0, 2.0], &[2.0, 2.0, 2.0]), &[0.0, 1.0, 0.0]));
    }

    #[test]
    fn rsi_is_all_the_way_up_when_nothing_fell_and_halfway_when_it_rose_and_fell_alike() {
        let up: Vec<f64> = (0..20).map(|i| i as f64).collect();
        assert_eq!(rsi(&up, 14)[19], 100.0);
        let saw: Vec<f64> = (0..40).map(|i| if i % 2 == 0 { 10.0 } else { 11.0 }).collect();
        let r = rsi(&saw, 14);
        assert!((r[39] - 50.0).abs() < 5.0, "{}", r[39]);
        assert!(r[13].is_nan() && !r[14].is_nan());
    }
}
