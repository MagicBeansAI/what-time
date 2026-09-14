//! One-off benchmark: warm per-parse latency for both tagger backends.
fn main() {
    let phrases = [
        "yoga every Tuesday and Thursday at 7am",
        "day after tomorrow at 8 pm",
        "24th august last year",
        "friday last week",
        "game night every Friday at 7:30pm",
        "from Sep 4 through September 8",
        "the last Friday of each month",
        "can you book it for day after tomorrow",
    ];
    let context = what_time::ParseContext {
        reference: "2026-09-12T15:26:00Z".into(),
        time_zone: "Asia/Kolkata".into(),
        limit: Some(3),
        ..Default::default()
    };
    let run = || -> f64 {
        let parser = what_time::Parser::new(Default::default());
        // warm up
        for phrase in phrases {
            let _ = parser.parse(phrase, &context);
        }
        let started = std::time::Instant::now();
        let rounds = 200;
        for _ in 0..rounds {
            for phrase in phrases {
                let _ = parser.parse(phrase, &context);
            }
        }
        started.elapsed().as_secs_f64() / (rounds * phrases.len()) as f64 * 1000.0
    };
    println!("transformer:  {:.3} ms/parse", run());
}
