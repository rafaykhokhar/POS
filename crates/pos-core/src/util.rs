use chrono::{Datelike, Duration, Local, Months, NaiveDate};

pub const TS_FMT: &str = "%Y-%m-%d %H:%M:%S";
pub const DATE_FMT: &str = "%Y-%m-%d";

pub fn now() -> String {
    Local::now().format(TS_FMT).to_string()
}

pub fn today() -> String {
    Local::now().format(DATE_FMT).to_string()
}

pub fn parse_date(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    let d = if s.len() >= 10 { &s[..10] } else { s };
    NaiveDate::parse_from_str(d, DATE_FMT).ok()
}

/// Start of the given day as a timestamp string.
pub fn day_start(date: &str) -> String {
    format!("{} 00:00:00", &date[..date.len().min(10)])
}

/// Start of the day after `date`, used as an exclusive upper bound.
pub fn day_after(date: &str) -> String {
    match parse_date(date) {
        Some(d) => format!("{} 00:00:00", (d + Duration::days(1)).format(DATE_FMT)),
        None => "9999-12-31 00:00:00".into(),
    }
}

pub fn add_months(date: &str, months: i64) -> String {
    if months <= 0 {
        return String::new();
    }
    parse_date(date)
        .and_then(|d| d.checked_add_months(Months::new(months as u32)))
        .map(|d| d.format(DATE_FMT).to_string())
        .unwrap_or_default()
}

pub fn add_days(date: &str, days: i64) -> String {
    parse_date(date)
        .map(|d| (d + Duration::days(days)).format(DATE_FMT).to_string())
        .unwrap_or_default()
}

pub fn month_start(date: &str) -> String {
    parse_date(date)
        .and_then(|d| d.with_day(1))
        .map(|d| d.format(DATE_FMT).to_string())
        .unwrap_or_default()
}

/// "2026-11-12" -> "12 Nov 2026"
pub fn pretty_date(s: &str) -> String {
    match parse_date(s) {
        Some(d) => d.format("%-d %b %Y").to_string(),
        None => String::new(),
    }
}

/// "2026-11-12 14:05:00" -> "12 Nov 2026, 2:05 PM"
pub fn pretty_ts(s: &str) -> String {
    match chrono::NaiveDateTime::parse_from_str(s, TS_FMT) {
        Ok(t) => t.format("%-d %b %Y, %-I:%M %p").to_string(),
        Err(_) => pretty_date(s),
    }
}

/// "2026-11-12 14:05:00" -> "2:05 PM"
pub fn pretty_time(s: &str) -> String {
    match chrono::NaiveDateTime::parse_from_str(s, TS_FMT) {
        Ok(t) => t.format("%-I:%M %p").to_string(),
        Err(_) => String::new(),
    }
}

/// Write rows as CSV (quotes values when needed). Opens directly in Excel.
pub fn to_csv(headers: &[&str], rows: &[Vec<String>]) -> String {
    fn esc(v: &str) -> String {
        if v.contains([',', '"', '\n', '\r']) {
            format!("\"{}\"", v.replace('"', "\"\""))
        } else {
            v.to_string()
        }
    }
    let mut out = String::new();
    out.push_str(&headers.iter().map(|h| esc(h)).collect::<Vec<_>>().join(","));
    out.push_str("\r\n");
    for r in rows {
        out.push_str(&r.iter().map(|v| esc(v)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    out
}

/// Clean a code typed by staff: trimmed, upper-case, no inner spaces.
pub fn clean_code(s: &str) -> String {
    s.trim().split_whitespace().collect::<Vec<_>>().join("").to_uppercase()
}
