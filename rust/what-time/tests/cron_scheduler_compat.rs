//! Exported cron lines fire exactly where the preview says, under a real
//! scheduler: every line the exporter writes for the pinned cron corpus is
//! run through the `cron` crate (0.12) the way Magician's task scheduler
//! runs a five-field line — seconds prepended, a year appended, POSIX
//! either-day rule restored by splitting — in the caller's time zone. The
//! next fires must equal the text preview, and `parse_cron` must read each
//! line back to the same fires and the same line.

use chrono::{DateTime, TimeZone, Utc};
use std::str::FromStr;
use what_time::testing::read_jsonl;
use what_time::{ParseContext, Parser};

const FIRES: usize = 6;

/// Magician's `normalize_cron` for a five-field line, then its either-day
/// split. what-time writes weekday names, which need no renumbering.
fn scheduler_fires(line: &str, zone: chrono_tz::Tz, after: DateTime<Utc>) -> Vec<DateTime<Utc>> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(fields.len(), 5, "{line}");
    assert!(
        !fields[4].chars().any(|c| c.is_ascii_digit()) || fields[4] == "*",
        "weekdays are written as names: {line}"
    );
    let normalized = format!(
        "0 {} {} {} {} {} *",
        fields[0], fields[1], fields[2], fields[3], fields[4]
    );
    let parts: Vec<&str> = normalized.split_whitespace().collect();
    let wild = |field: &str| field == "*" || field == "?";
    let variants = if !wild(parts[3]) && !wild(parts[5]) {
        let mut days = parts.clone();
        days[5] = "*";
        let mut weekdays = parts.clone();
        weekdays[3] = "*";
        vec![days.join(" "), weekdays.join(" ")]
    } else {
        vec![normalized.clone()]
    };
    let start = zone.from_utc_datetime(&(after - chrono::Duration::seconds(1)).naive_utc());
    let mut fires: Vec<DateTime<Utc>> = variants
        .iter()
        .flat_map(|variant| {
            cron::Schedule::from_str(variant)
                .unwrap_or_else(|error| {
                    panic!("the cron crate rejects {variant:?} from {line:?}: {error}")
                })
                .after(&start)
                .take(FIRES)
                .map(|fire| fire.with_timezone(&Utc))
                .collect::<Vec<_>>()
        })
        .collect();
    fires.sort();
    fires.dedup();
    fires
}

fn instants(ranges: &[what_time::TimeRange]) -> Vec<DateTime<Utc>> {
    let mut values: Vec<DateTime<Utc>> = ranges
        .iter()
        .map(|range| {
            DateTime::parse_from_rfc3339(&range.start)
                .unwrap()
                .with_timezone(&Utc)
        })
        .collect();
    values.sort();
    values.dedup();
    values
}

#[derive(serde::Deserialize)]
struct CronCase {
    text: String,
}

#[test]
fn exported_cron_fires_where_the_preview_says_under_a_real_scheduler() {
    let parser = Parser::new(Default::default());
    let mut checked = 0;
    let mut problems = Vec::new();
    for (reference, zone) in [
        ("2026-11-22T10:00:00Z", "Asia/Kolkata"),
        // Across the US spring-forward gap.
        ("2026-03-07T12:00:00Z", "America/New_York"),
    ] {
        let tz: chrono_tz::Tz = zone.parse().unwrap();
        let after = DateTime::parse_from_rfc3339(reference)
            .unwrap()
            .with_timezone(&Utc);
        let horizon = after + chrono::Duration::days(365);
        let context = ParseContext {
            reference: reference.into(),
            time_zone: zone.into(),
            limit: Some(FIRES as i64),
            ..Default::default()
        };
        for line in read_jsonl("cron") {
            let case: CronCase = serde_json::from_str(&line).unwrap();
            let result = parser.parse(&case.text, &context).unwrap();
            if result.crons.is_empty()
                || result
                    .diagnostics
                    .iter()
                    .any(|d| d.code == "cron-unbounded")
            {
                continue;
            }
            checked += 1;
            // An all-day occurrence today began before the reference.
            let preview: Vec<_> = instants(&result.occurrences)
                .into_iter()
                .filter(|instant| *instant >= after)
                .collect();
            let mut fired: Vec<_> = result
                .crons
                .iter()
                .flat_map(|line| scheduler_fires(line, tz, after))
                .collect();
            fired.sort();
            fired.dedup();
            let fired: Vec<_> = fired
                .into_iter()
                .filter(|instant| *instant < horizon || preview.contains(instant))
                .take(preview.len())
                .collect();
            if fired != preview {
                problems.push(format!(
                    "{zone} {:?} {:?}: preview {preview:?}, scheduler {fired:?}",
                    case.text, result.crons
                ));
            }
            for line in &result.crons {
                let back = what_time::parse_cron(line, &context).unwrap();
                if back.crons != vec![line.clone()] {
                    problems.push(format!("{line:?} reads back as {:?}", back.crons));
                }
                let read: Vec<_> = instants(&back.occurrences);
                let expected: Vec<_> = scheduler_fires(line, tz, after)
                    .into_iter()
                    .filter(|instant| *instant < horizon || read.contains(instant))
                    .take(read.len())
                    .collect();
                if read != expected {
                    problems.push(format!(
                        "{zone} parse_cron({line:?}): {read:?}, scheduler {expected:?}"
                    ));
                }
            }
        }
    }
    assert!(checked >= 80, "only {checked} phrases exported cron");
    assert!(
        problems.is_empty(),
        "{} disagreements:\n{}",
        problems.len(),
        problems.join("\n")
    );
    eprintln!("scheduler compat: {checked} phrase/zone pairs fire identically");
}
