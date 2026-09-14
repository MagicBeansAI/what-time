fn main() {
    let parser = what_time::ScheduleParser::new(Default::default());
    for text in std::env::args().skip(1) {
        let result = parser.parse(&text).unwrap();
        for expression in &result.expressions {
            println!(
                "{text:?} -> {} confidence={:.3} diags={:?}",
                serde_json::to_string(&expression.schedule).unwrap_or_default(),
                expression.confidence,
                expression
                    .diagnostics
                    .iter()
                    .map(|d| d.code.as_str())
                    .collect::<Vec<_>>()
            );
        }
        if result.expressions.is_empty() {
            println!("{text:?} -> NO EXPRESSIONS");
        }
    }
}
