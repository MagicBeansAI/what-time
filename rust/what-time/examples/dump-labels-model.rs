//! One-off inspection: per-token labels under a chosen model.
fn main() {
    let parser = what_time::ScheduleParser::new(Default::default());
    for text in std::env::args().skip(1) {
        println!("=== {text:?}");
        for token in parser.parse(&text).unwrap().tokens.iter().flatten() {
            println!("  {:>10}  label={}", token.text, token.label);
        }
    }
}
