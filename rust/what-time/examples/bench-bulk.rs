//! Bulk batch benchmark: CPU loop vs GPU dispatch over many phrases.
//! Build with --features gpu. Usage: bench-bulk [count]
fn main() {
    let count: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(10_000);

    let templates = [
        "yoga every Tuesday and Thursday at 7am",
        "can you book it for day after tomorrow at 8 pm",
        "कल शाम को आठ बजे मीटिंग",
        "har hafte Tuesday ko gym",
        "game night every Friday at 7:30pm",
        "call mom on sunday evening",
        "24th august last year",
        "pay rent on the 1st of each month",
        "friday last week",
        "parso subah 10 baje",
    ];
    let phrases: Vec<String> = (0..count)
        .map(|i| templates[i % templates.len()].to_string())
        .collect();
    let refs: Vec<&str> = phrases.iter().map(String::as_str).collect();

    let context = what_time::ParseContext {
        reference: "2026-09-13T10:00:00Z".into(),
        time_zone: "Asia/Kolkata".into(),
        limit: Some(3),
        ..Default::default()
    };

    // CPU-forced run
    let cpu_parser = what_time::Parser::new(what_time::ParserOptions {
        backend: Some(what_time::Backend::Cpu),
        ..Default::default()
    });
    let started = std::time::Instant::now();
    let cpu_results = cpu_parser.parse_many(&refs, &context).unwrap();
    let cpu_elapsed = started.elapsed();

    // Auto run: dispatches through the GPU above the batch threshold
    let gpu_parser = what_time::Parser::new(Default::default());
    let started = std::time::Instant::now();
    let gpu_results = gpu_parser.parse_many(&refs, &context).unwrap();
    let gpu_elapsed = started.elapsed();

    let agree = cpu_results
        .iter()
        .zip(gpu_results.iter())
        .filter(|(a, b)| a.occurrences == b.occurrences)
        .count();
    println!(
        "count {count}: cpu {:.0} ms ({:.1} µs/phrase) · gpu {:.0} ms ({:.1} µs/phrase incl. warmup) · speedup {:.2}x · occurrence agreement {agree}/{count}",
        (cpu_elapsed.as_secs_f64() * 1000.0),
        cpu_elapsed.as_micros() as f64 / count as f64,
        (gpu_elapsed.as_secs_f64() * 1000.0),
        gpu_elapsed.as_micros() as f64 / count as f64,
        cpu_elapsed.as_secs_f64() / gpu_elapsed.as_secs_f64(),
    );

    // warmed second pass (adapter/pipelines already initialized)
    let started = std::time::Instant::now();
    let _ = gpu_parser.parse_many(&refs, &context).unwrap();
    let warm = started.elapsed();
    println!(
        "gpu warmed: {:.0} ms ({:.1} µs/phrase) · speedup {:.2}x",
        (warm.as_secs_f64() * 1000.0),
        warm.as_micros() as f64 / count as f64,
        cpu_elapsed.as_secs_f64() / warm.as_secs_f64(),
    );
}
