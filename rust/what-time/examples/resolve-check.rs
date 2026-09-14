fn main() {
    let ctx = what_time::ParseContext {
        reference: "2026-09-13T10:00:00Z".into(),
        time_zone: "Asia/Kolkata".into(),
        limit: Some(3),
        ..Default::default()
    };
    for (name, json) in [
        (
            "daily-h8",
            r#"{"clauses":[{"time":{"start":{"hour":8,"minute":0}},"recurrence":{"freq":"daily","interval":1}}]}"#,
        ),
        (
            "timeonly-h8",
            r#"{"clauses":[{"time":{"start":{"hour":8,"minute":0}}}]}"#,
        ),
        (
            "daily-h7am",
            r#"{"clauses":[{"time":{"start":{"hour":7,"minute":0,"meridiem":"am"}},"recurrence":{"freq":"daily","interval":1}}]}"#,
        ),
    ] {
        let schedule: what_time::Schedule = serde_json::from_str(json).unwrap();
        match what_time::resolve(&schedule, &ctx) {
            Ok(r) => println!(
                "{name}: {:?}",
                r.occurrences
                    .iter()
                    .map(|o| o.start.clone())
                    .collect::<Vec<_>>()
            ),
            Err(e) => println!("{name}: ERR {e:?}"),
        }
    }
}
