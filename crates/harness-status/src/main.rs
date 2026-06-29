mod budget;
mod display;
mod progress;
mod sessions;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "harness-status",
    about = "Unified HOTL status across all harness plugins"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Output as JSON
    #[arg(long, global = true)]
    json: bool,

    /// How many recent sessions to show
    #[arg(long, default_value = "5", global = true)]
    sessions: usize,
}

#[derive(Subcommand)]
enum Command {
    /// Show budget information only
    Budget,
    /// Show recent sessions only
    Sessions,
    /// Show progress file only
    Progress,
}

fn today() -> String {
    // Read from env (testable) or derive from filesystem mtime as a poor-man's clock.
    // Full date via shell timestamp file is a common pattern in subscription-native tools.
    // We parse from the last_ts of the most recent session, or default to a static string.
    // Production: callers can set HARNESS_DATE=YYYY-MM-DD for testing.
    if let Ok(d) = std::env::var("HARNESS_DATE") {
        return d;
    }
    // Fall back to reading system date via file metadata on a temp file (WSL-friendly).
    // This avoids chrono dependency while still being mostly correct.
    let tmp = std::env::temp_dir().join(".harness-status-date");
    let _ = std::fs::write(&tmp, b"");
    if let Ok(meta) = std::fs::metadata(&tmp) {
        if let Ok(modified) = meta.modified() {
            if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                let secs = duration.as_secs();
                let days = secs / 86400;
                // Compute Gregorian date from days since epoch (1970-01-01).
                return days_to_date(days);
            }
        }
    }
    "unknown".to_string()
}

fn days_to_date(days: u64) -> String {
    // Simple Gregorian calendar calculation.
    let mut y = 1970u32;
    let mut d = days as u32;
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if d < days_in_year {
            break;
        }
        d -= days_in_year;
        y += 1;
    }
    let months = [
        31u32,
        if is_leap(y) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 1u32;
    for &mdays in &months {
        if d < mdays {
            break;
        }
        d -= mdays;
        m += 1;
    }
    format!("{y:04}-{m:02}-{:02}", d + 1)
}

fn is_leap(y: u32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn main() {
    let cli = Cli::parse();
    let today = today();
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

    match cli.command {
        Some(Command::Budget) => {
            let b = budget::read(&today);
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&b).unwrap_or_default());
            } else {
                println!(
                    "Today ({}): ${:.4} across {} session(s)",
                    today, b.today_usd, b.session_count_today
                );
            }
        }
        Some(Command::Sessions) => {
            let s = sessions::recent(cli.sessions);
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&s).unwrap_or_default());
            } else {
                for sess in &s {
                    println!(
                        "{} | {} | {} turns | ${:.4}",
                        sess.session_id.get(..8).unwrap_or(&sess.session_id),
                        sess.project,
                        sess.turns,
                        sess.cost_usd
                    );
                }
            }
        }
        Some(Command::Progress) => {
            let p = progress::read(&cwd);
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&p).unwrap_or_default());
            } else if p.exists {
                println!("{}", p.preview.as_deref().unwrap_or("(empty)"));
            } else {
                println!("[no progress file] {}", p.path);
            }
        }
        None => {
            let b = budget::read(&today);
            let s = sessions::recent(cli.sessions);
            let p = progress::read(&cwd);
            if cli.json {
                display::print_json(&today, &b, &s, &p);
            } else {
                display::print_status(&today, &b, &s, &p, &cwd.to_string_lossy());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_is_1970_01_01() {
        assert_eq!(days_to_date(0), "1970-01-01");
    }

    #[test]
    fn known_dates_round_trip() {
        // 2000-01-01 = 10957 days after epoch; 2026-06-23 = 20627.
        assert_eq!(days_to_date(10957), "2000-01-01");
        assert_eq!(days_to_date(20627), "2026-06-23");
    }

    #[test]
    fn leap_year_feb_29_handled() {
        // 2024-02-29 = 19782 days after epoch.
        assert_eq!(days_to_date(19782), "2024-02-29");
        assert_eq!(days_to_date(19783), "2024-03-01");
    }

    #[test]
    fn leap_rule_centuries() {
        assert!(is_leap(2000)); // divisible by 400
        assert!(!is_leap(1900)); // divisible by 100, not 400
        assert!(is_leap(2024));
        assert!(!is_leap(2026));
    }
}
