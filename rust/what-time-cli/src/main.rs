//! Command-line interface for the what-time schedule parser.

use std::process::ExitCode;
use what_time::ParseContext;

const USAGE: &str = "\
what-time — parse English, Hindi, and Hinglish schedule expressions into dates and recurrence rules

USAGE:
    what-time [OPTIONS] <TEXT>

ARGS:
    <TEXT>    The schedule expression to parse

OPTIONS:
    -r, --reference <INSTANT>    ISO instant with Z or an offset [default: now]
    -t, --time-zone <ZONE>       IANA time zone name [default: UTC]
    -l, --limit <COUNT>          Maximum previewed occurrences, 1..=1000 [default: 30]
    -j, --json                   Print the full parse result as JSON
    -h, --help                   Print this help

Text may also be piped: what-time -j -r <INSTANT> < phrase.txt
Exit codes: 0 parsed, 1 no schedule found, 2 bad invocation.
";

struct Args {
    text: Option<String>,
    reference: String,
    time_zone: String,
    limit: i64,
    json: bool,
}

fn now_reference() -> String {
    // RFC 3339 with an explicit offset, matching the reference contract.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is after the epoch");
    let millis = now.as_millis() as i64;
    let seconds = millis.div_euclid(1000);
    let subsecond = millis.rem_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let time = seconds.rem_euclid(86_400);
    let date = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        date.0,
        date.1,
        date.2,
        time / 3600,
        time / 60 % 60,
        time % 60,
        subsecond
    )
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        text: None,
        reference: String::new(),
        time_zone: "UTC".to_string(),
        limit: 30,
        json: false,
    };
    let mut values = std::env::args().skip(1);
    let mut reference_set = false;
    while let Some(arg) = values.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("what-time {}", what_time::VERSION);
                std::process::exit(0);
            }
            "-j" | "--json" => args.json = true,
            "-r" | "--reference" => {
                args.reference = values.next().ok_or("--reference needs a value")?;
                reference_set = true;
            }
            "-t" | "--time-zone" => {
                args.time_zone = values.next().ok_or("--time-zone needs a value")?;
            }
            "-l" | "--limit" => {
                args.limit = values
                    .next()
                    .ok_or("--limit needs a value")?
                    .parse()
                    .map_err(|_| "--limit must be an integer".to_string())?;
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option: {other}"));
            }
            other => {
                if args.text.is_some() {
                    return Err("only one text argument is allowed".into());
                }
                args.text = Some(other.to_string());
            }
        }
    }
    if !reference_set {
        args.reference = now_reference();
    }
    Ok(args)
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("error: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let text = match args.text.as_deref() {
        Some("-") | None => {
            let mut buffer = String::new();
            if std::io::stdin().read_line(&mut buffer).is_err() || buffer.trim().is_empty() {
                eprintln!("error: pass a text argument or pipe input on stdin\n\n{USAGE}");
                return ExitCode::from(2);
            }
            buffer.trim_end().to_string()
        }
        Some(text) => text.to_string(),
    };

    let context = ParseContext {
        reference: args.reference,
        time_zone: args.time_zone,
        limit: Some(args.limit),
        ..Default::default()
    };

    let parser = what_time::Parser::new(Default::default());
    match parser.parse(&text, &context) {
        Ok(result) => {
            if args.json {
                match serde_json::to_string_pretty(&result) {
                    Ok(json) => println!("{json}"),
                    Err(error) => {
                        eprintln!("error: {error}");
                        return ExitCode::FAILURE;
                    }
                }
            } else {
                if result.occurrences.is_empty() {
                    println!("no occurrences");
                }
                for occurrence in &result.occurrences {
                    match &occurrence.end {
                        Some(end) => println!(
                            "{} → {}{} (all day: {})",
                            occurrence.start,
                            end,
                            occurrence
                                .open
                                .map(|open| format!(" (open at {open:?})"))
                                .unwrap_or_default(),
                            occurrence.all_day
                        ),
                        None => println!(
                            "{}{} (all day: {})",
                            occurrence.start,
                            occurrence
                                .open
                                .map(|open| format!(" (open at {open:?})"))
                                .unwrap_or_default(),
                            occurrence.all_day
                        ),
                    }
                }
                for rule in &result.rrules {
                    println!("{rule}");
                }
                for diagnostic in &result.diagnostics {
                    if diagnostic.severity == what_time::Severity::error {
                        eprintln!("error: [{}] {}", diagnostic.code, diagnostic.message);
                    }
                }
            }
            // Agent-friendly exit codes: 0 parsed something, 1 nothing
            // parseable (also when an error diagnostic rejected the input).
            if result.occurrences.is_empty() {
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
