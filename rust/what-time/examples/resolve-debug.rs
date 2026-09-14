fn main() {
    let ctx = what_time::ParseContext {
        reference: "2026-09-13T10:00:00Z".into(),
        time_zone: "Asia/Kolkata".into(),
        limit: Some(3),
        ..Default::default()
    };
    for (name, json) in [
        (
            "baje-shape",
            r#"{"clauses":[{"date":{"kind":"relativeDay","offset":0},"time":{"start":{"hour":8,"minute":0}}}]}"#,
        ),
        (
            "pm-shape",
            r#"{"clauses":[{"date":{"kind":"relativeDay","offset":0},"time":{"start":{"hour":20,"minute":0}}}]}"#,
        ),
        (
            "time-only",
            r#"{"clauses":[{"time":{"start":{"hour":8,"minute":0}}}]}"#,
        ),
    ] {
        let schedule: what_time::Schedule = serde_json::from_str(json).unwrap();
        match what_time::resolve(&schedule, &ctx) {
            Ok(resolved) => println!(
                "{name}: {:?}",
                resolved
                    .occurrences
                    .iter()
                    .map(|o| o.start.clone())
                    .collect::<Vec<_>>()
            ),
            Err(error) => println!("{name}: ERROR {error:?}"),
        }
    }
}
