fn main() {
    let ctx = what_time::ParseContext {
        reference: "2026-09-13T10:00:00Z".into(),
        time_zone: "Asia/Kolkata".into(),
        limit: Some(3),
        ..Default::default()
    };
    let parser = what_time::Parser::new(Default::default());
    for text in ["shaam ko 8 baje", "har din 8 baje", "8 baje"] {
        let inner = what_time::ScheduleParser::new(Default::default())
            .parse(text)
            .unwrap();
        let expression = inner.expressions.first().unwrap();
        println!(
            "{text:?}: confidence {:.3} schedule {}",
            expression.confidence,
            serde_json::to_string(&expression.schedule).unwrap()
        );
        match parser.parse(text, &ctx) {
            Ok(result) => println!(
                "  -> {} occurrences, diags {:?}",
                result.occurrences.len(),
                result
                    .diagnostics
                    .iter()
                    .map(|d| d.code.as_str())
                    .collect::<Vec<_>>()
            ),
            Err(error) => println!("  -> ERROR {error:?}"),
        }
    }
}
