//! Single-phrase CPU latency benchmark (warm, full pipeline including the
//! timezone-aware resolve). Compare with bench-bulk for GPU batch numbers.
fn main() {
    let phrases = [
        "call mom on Sunday evening",
        "कल शाम को आठ बजे मीटिंग",
        "har hafte Tuesday ko gym",
        "15 tareekh se 20 tareekh tak chutti",
        "family dinner on Thanksgiving",
        "review next quarter at 9am",
        "gym every Mon Wed and Fri at 6am",
        "out of office from Dec 22 until Jan 2",
    ];
    let context = what_time::ParseContext {
        reference: "2026-09-16T10:00:00Z".into(),
        time_zone: "Asia/Kolkata".into(),
        limit: Some(3),
        ..Default::default()
    };
    let parser = what_time::Parser::new(what_time::ParserOptions {
        backend: Some(what_time::Backend::Cpu),
        ..Default::default()
    });
    for phrase in phrases {
        let _ = parser.parse(phrase, &context).unwrap();
    }
    let mut all: Vec<f64> = Vec::new();
    for _ in 0..500 {
        for phrase in phrases {
            let started = std::time::Instant::now();
            let _ = parser.parse(phrase, &context).unwrap();
            all.push(started.elapsed().as_secs_f64() * 1000.0);
        }
    }
    all.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean: f64 = all.iter().sum::<f64>() / all.len() as f64;
    println!(
        "n={} · mean {:.3} ms · p50 {:.3} ms · p95 {:.3} ms · max {:.3} ms",
        all.len(),
        mean,
        all[all.len() / 2],
        all[all.len() * 95 / 100],
        all[all.len() - 1]
    );
}
