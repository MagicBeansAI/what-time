fn main() {
    for text in ["हफ़्ते स्टैंडअप कल शाम को आठ बजे", "८ बजे", "परसों"]
    {
        let tokens: Vec<String> = what_time::testing::tokenize(text)
            .into_iter()
            .map(|t| format!("{}:{}", t.text, t.kind))
            .collect();
        println!("{text:?} -> {tokens:?}");
    }
}
