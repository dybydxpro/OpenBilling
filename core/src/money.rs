//! Money is stored as integer cents everywhere; these helpers convert to and
//! from what users type and see.

use crate::error::{Result, invalid};

/// Parses user input such as `1,234.5`, `$ 12`, `-3.25` into cents.
pub fn parse_money(input: &str) -> Result<i64> {
    let cleaned: String = input
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    if cleaned.is_empty() {
        return Ok(0);
    }
    let value: f64 = cleaned
        .parse()
        .or_else(|_| invalid(format!("\"{}\" is not a valid amount.", input.trim())))?;
    if !value.is_finite() || value.abs() > 1e12 {
        return invalid("Amount is out of range.");
    }
    Ok((value * 100.0).round() as i64)
}

pub fn parse_quantity(input: &str) -> Result<f64> {
    let t = input.trim().replace(',', "");
    if t.is_empty() {
        return invalid("Quantity is required.");
    }
    let q: f64 = t
        .parse()
        .or_else(|_| invalid(format!("\"{}\" is not a valid quantity.", input.trim())))?;
    if !q.is_finite() || q <= 0.0 || q > 1e9 {
        return invalid("Quantity must be greater than zero.");
    }
    Ok(q)
}

pub fn parse_percent(input: &str) -> Result<f64> {
    let t = input.trim().trim_end_matches('%').trim();
    if t.is_empty() {
        return Ok(0.0);
    }
    let p: f64 = t
        .parse()
        .or_else(|_| invalid(format!("\"{}\" is not a valid percentage.", input.trim())))?;
    if !(0.0..=100.0).contains(&p) {
        return invalid("Percentage must be between 0 and 100.");
    }
    Ok(p)
}

/// `123456` -> `1,234.56`
pub fn format_amount(cents: i64) -> String {
    let neg = cents < 0;
    let abs = cents.unsigned_abs();
    let whole = (abs / 100).to_string();
    let mut grouped = String::with_capacity(whole.len() + whole.len() / 3);
    for (i, ch) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{}{}.{:02}", if neg { "-" } else { "" }, grouped, abs % 100)
}

/// `123456`, `"$"` -> `$1,234.56`
pub fn format_money(cents: i64, symbol: &str) -> String {
    let amount = format_amount(cents.abs());
    let sign = if cents < 0 { "-" } else { "" };
    // Alphabetic symbols ("Rs", "USD") read better with a space.
    if symbol.chars().last().is_some_and(|c| c.is_alphabetic()) {
        format!("{sign}{symbol} {amount}")
    } else {
        format!("{sign}{symbol}{amount}")
    }
}

/// Drops trailing zeros: `2.0` -> `2`, `1.50` -> `1.5`.
pub fn format_quantity(q: f64) -> String {
    let s = format!("{q:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

pub fn format_percent(p: f64) -> String {
    format_quantity(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_round_trip() {
        assert_eq!(parse_money("1,234.56").unwrap(), 123456);
        assert_eq!(parse_money("$ 12").unwrap(), 1200);
        assert_eq!(parse_money("").unwrap(), 0);
        assert_eq!(parse_money("0.105").unwrap(), 11);
        assert!(parse_money("abc.def.").is_err());
        assert_eq!(format_amount(123456789), "1,234,567.89");
        assert_eq!(format_amount(5), "0.05");
        assert_eq!(format_money(-1050, "$"), "-$10.50");
        assert_eq!(format_money(1050, "Rs"), "Rs 10.50");
        assert_eq!(format_quantity(2.0), "2");
        assert_eq!(format_quantity(1.25), "1.25");
    }
}
