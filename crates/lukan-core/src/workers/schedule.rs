use anyhow::{Result, bail};

/// Parse a schedule string into a repeat interval in milliseconds.
///
/// Supported formats:
/// - `every:Xu` where X is a positive integer and u is s/m/h
///   e.g. `every:5m` → 300_000 ms
/// - `*/N * * * *` basic cron minute-interval
///   e.g. `*/10 * * * *` → 600_000 ms
///
/// Minimum interval: 10_000 ms (10 seconds)
pub fn parse_schedule_ms(schedule: &str) -> Result<u64> {
    let schedule = schedule.trim();

    // Format 1: every:Xu
    if let Some(rest) = schedule.strip_prefix("every:") {
        let (num_str, unit) = split_number_unit(rest)?;
        let n: u64 = num_str
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid number in schedule: '{num_str}'"))?;
        if n == 0 {
            bail!("Schedule interval must be > 0");
        }
        let ms = match unit {
            's' => n * 1_000,
            'm' => n * 60_000,
            'h' => n * 3_600_000,
            _ => bail!("Invalid schedule unit '{unit}'. Use s, m, or h"),
        };
        return enforce_minimum(ms);
    }

    // Format 2: */N * * * * (cron minute interval)
    if schedule.starts_with("*/") {
        let parts: Vec<&str> = schedule.split_whitespace().collect();
        if parts.len() == 5 && parts[1..] == ["*", "*", "*", "*"] {
            let n_str = parts[0].strip_prefix("*/").unwrap();
            let n: u64 = n_str
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid cron minute interval: '{n_str}'"))?;
            if n == 0 {
                bail!("Cron minute interval must be > 0");
            }
            let ms = n * 60_000;
            return enforce_minimum(ms);
        }
    }

    bail!(
        "Invalid schedule format: '{schedule}'. \
         Use 'every:5m' or '*/10 * * * *'"
    )
}

fn split_number_unit(s: &str) -> Result<(&str, char)> {
    if s.len() < 2 {
        bail!("Schedule too short: '{s}'");
    }
    let unit = s
        .chars()
        .last()
        .ok_or_else(|| anyhow::anyhow!("Empty schedule"))?;
    let num = &s[..s.len() - 1];
    Ok((num, unit))
}

fn enforce_minimum(ms: u64) -> Result<u64> {
    if ms < 10_000 {
        bail!("Schedule interval too short ({ms}ms). Minimum is 10 seconds");
    }
    Ok(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_every_seconds() {
        assert_eq!(parse_schedule_ms("every:30s").unwrap(), 30_000);
        assert_eq!(parse_schedule_ms("every:10s").unwrap(), 10_000);
    }

    #[test]
    fn test_every_minutes() {
        assert_eq!(parse_schedule_ms("every:5m").unwrap(), 300_000);
        assert_eq!(parse_schedule_ms("every:1m").unwrap(), 60_000);
    }

    #[test]
    fn test_every_hours() {
        assert_eq!(parse_schedule_ms("every:1h").unwrap(), 3_600_000);
        assert_eq!(parse_schedule_ms("every:2h").unwrap(), 7_200_000);
    }

    #[test]
    fn test_cron_minutes() {
        assert_eq!(parse_schedule_ms("*/10 * * * *").unwrap(), 600_000);
        assert_eq!(parse_schedule_ms("*/1 * * * *").unwrap(), 60_000);
    }

    #[test]
    fn test_minimum_enforcement() {
        assert!(parse_schedule_ms("every:1s").is_err());
        assert!(parse_schedule_ms("every:9s").is_err());
        assert!(parse_schedule_ms("every:10s").is_ok());
    }

    #[test]
    fn test_invalid_formats() {
        assert!(parse_schedule_ms("").is_err());
        assert!(parse_schedule_ms("5m").is_err());
        assert!(parse_schedule_ms("every:0m").is_err());
        assert!(parse_schedule_ms("every:abc").is_err());
        assert!(parse_schedule_ms("*/0 * * * *").is_err());
    }

    #[test]
    fn test_whitespace_trim() {
        assert_eq!(parse_schedule_ms("  every:5m  ").unwrap(), 300_000);
    }
}
