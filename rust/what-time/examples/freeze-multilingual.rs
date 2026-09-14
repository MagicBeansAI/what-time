//! Freeze the trilingual eval corpus: compile each phrase with the default
//! (transformer) model and emit evals/data/multilingual.jsonl expectations.
fn main() {
    let phrases: &[(&str, &str)] = &[
        ("english", "can you book it for day after tomorrow at 8 pm"),
        ("english", "24th august last year"),
        ("english", "game night every Friday at 7:30pm"),
        ("english", "friday last week"),
        ("hindi", "कल शाम को आठ बजे मीटिंग"),
        ("hindi", "हर सोमवार को सुबह नौ बजे"),
        ("hindi", "अगले महीने की 21st को"),
        ("hindi", "परसों सुबह"),
        ("hindi", "हर हफ़्ते शुक्रवार को शाम"),
        ("hindi", "आज दोपहर तीन बजे"),
        ("hinglish", "kal shaam ko 8 baje call kar dena"),
        ("hinglish", "har hafte Tuesday ko gym"),
        ("hinglish", "parso subah 10 baje"),
        ("hinglish", "agle mahine ki 21st"),
        ("hinglish", "aaj dopahar 3 baje"),
    ];
    let parser = what_time::ScheduleParser::new(Default::default());
    let mut out = String::new();
    for (index, (language, text)) in phrases.iter().enumerate() {
        let result = parser.parse(text).unwrap();
        let schedule = result
            .expressions
            .first()
            .and_then(|expression| expression.schedule.as_ref());
        let schedule_json = serde_json::to_value(schedule).unwrap();
        assert!(
            schedule_json != serde_json::Value::Null,
            "no schedule for {text}"
        );
        let id = format!("multilingual-{:03}", index + 1);
        let row = serde_json::json!({
            "id": id,
            "language": language,
            "text": text,
            "schedule": schedule_json,
        });
        out += &row.to_string();
        out.push('\n');
    }
    let target =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/data/multilingual.jsonl");
    std::fs::write(target, out).unwrap();
    println!("froze {} cases", phrases.len());
}
