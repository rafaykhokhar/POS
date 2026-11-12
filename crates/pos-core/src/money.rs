//! Money is stored as integer minor units (1/100 of the currency) so totals never drift.

use serde::{Deserialize, Serialize};

pub type Money = i64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MoneyFmt {
    pub symbol: String,
    pub decimals: u8,
    pub symbol_after: bool,
}

impl Default for MoneyFmt {
    fn default() -> Self {
        MoneyFmt { symbol: "Rs".into(), decimals: 0, symbol_after: false }
    }
}

impl MoneyFmt {
    /// Number only, e.g. "12,500" or "12,500.50".
    pub fn plain(&self, m: Money) -> String {
        let neg = m < 0;
        let a = m.unsigned_abs();
        let s = if self.decimals == 0 {
            group((a + 50) / 100)
        } else {
            format!("{}.{:02}", group(a / 100), a % 100)
        };
        if neg && s.chars().any(|c| c.is_ascii_digit() && c != '0') {
            format!("-{s}")
        } else {
            s
        }
    }

    /// With currency symbol, e.g. "Rs 12,500".
    pub fn fmt(&self, m: Money) -> String {
        let p = self.plain(m.abs());
        let body = if self.symbol.is_empty() {
            p
        } else if self.symbol_after {
            format!("{p} {}", self.symbol)
        } else {
            format!("{} {p}", self.symbol)
        };
        if m < 0 && self.plain(m).starts_with('-') {
            format!("-{body}")
        } else {
            body
        }
    }

    /// Value suitable for an edit box (no grouping), e.g. "12500" or "12500.50".
    pub fn edit(&self, m: Money) -> String {
        if m % 100 == 0 {
            format!("{}", m / 100)
        } else {
            let neg = m < 0;
            let a = m.unsigned_abs();
            format!("{}{}.{:02}", if neg { "-" } else { "" }, a / 100, a % 100)
        }
    }
}

fn group(n: u64) -> String {
    let s = n.to_string();
    let len = s.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Parse "1,250.5" -> 125050. Empty string -> Some(0). Invalid -> None.
pub fn parse_money(s: &str) -> Option<Money> {
    let t: String = s.trim().chars().filter(|c| !matches!(c, ',' | ' ')).collect();
    if t.is_empty() {
        return Some(0);
    }
    let neg = t.starts_with('-');
    let t = t.trim_start_matches('-');
    let (whole, frac) = match t.split_once('.') {
        Some((w, f)) => (w, f),
        None => (t, ""),
    };
    if whole.is_empty() && frac.is_empty() {
        return None;
    }
    if !whole.chars().all(|c| c.is_ascii_digit())
        || !frac.chars().all(|c| c.is_ascii_digit())
        || frac.len() > 2
    {
        return None;
    }
    let w: i64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
    let f: i64 = match frac.len() {
        0 => 0,
        1 => frac.parse::<i64>().ok()? * 10,
        _ => frac.parse().ok()?,
    };
    let v = w.checked_mul(100)?.checked_add(f)?;
    Some(if neg { -v } else { v })
}

/// "17.5" -> 1750 basis points.
pub fn parse_percent(s: &str) -> Option<i64> {
    let t = s.trim().trim_end_matches('%');
    let v = parse_money(t)?;
    if (0..=10000).contains(&v) {
        Some(v)
    } else {
        None
    }
}

/// 1750 -> "17.5%"
pub fn fmt_percent(bp: i64) -> String {
    let whole = bp / 100;
    let frac = (bp % 100).abs();
    if frac == 0 {
        format!("{whole}%")
    } else if frac % 10 == 0 {
        format!("{whole}.{}%", frac / 10)
    } else {
        format!("{whole}.{frac:02}%")
    }
}

/// Integer division rounding half away from zero.
pub fn round_div(n: i128, d: i128) -> i64 {
    if d == 0 {
        return 0;
    }
    let q = n / d;
    let r = n % d;
    let adj = if (r.abs() * 2) >= d.abs() {
        if (n < 0) ^ (d < 0) {
            -1
        } else {
            1
        }
    } else {
        0
    };
    (q + adj) as i64
}

/// amount * bp / 10000
pub fn mul_bp(amount: Money, bp: i64) -> Money {
    round_div(amount as i128 * bp as i128, 10_000)
}

/// Margin in basis points: profit / revenue.
pub fn margin_bp(profit: Money, revenue: Money) -> i64 {
    if revenue == 0 {
        0
    } else {
        round_div(profit as i128 * 10_000, revenue as i128)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        let f = MoneyFmt { symbol: "Rs".into(), decimals: 0, symbol_after: false };
        assert_eq!(f.fmt(1_250_000), "Rs 12,500");
        assert_eq!(f.fmt(-1_250_000), "-Rs 12,500");
        assert_eq!(f.fmt(0), "Rs 0");
        let f2 = MoneyFmt { symbol: "$".into(), decimals: 2, symbol_after: false };
        assert_eq!(f2.fmt(123_456_789), "$ 1,234,567.89");
        assert_eq!(f2.edit(1050), "10.50");
        assert_eq!(f2.edit(1000), "10");
    }

    #[test]
    fn parses() {
        assert_eq!(parse_money("1,250.5"), Some(125_050));
        assert_eq!(parse_money("  12500 "), Some(1_250_000));
        assert_eq!(parse_money(""), Some(0));
        assert_eq!(parse_money(".5"), Some(50));
        assert_eq!(parse_money("abc"), None);
        assert_eq!(parse_money("1.234"), None);
        assert_eq!(parse_percent("17"), Some(1700));
        assert_eq!(parse_percent("17.5%"), Some(1750));
        assert_eq!(fmt_percent(1750), "17.5%");
        assert_eq!(fmt_percent(1700), "17%");
    }

    #[test]
    fn rounding() {
        assert_eq!(round_div(5, 2), 3);
        assert_eq!(round_div(-5, 2), -3);
        assert_eq!(round_div(4, 3), 1);
        assert_eq!(mul_bp(10_000, 1700), 1700);
    }
}
