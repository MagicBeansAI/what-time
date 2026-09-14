//! One-off inspection: print per-token model labels for phrases given as args.
fn main() {
    for text in std::env::args().skip(1) {
        println!("=== {text:?}");
        for (token, label) in what_time::testing::tag_labels(&text) {
            println!("  {token:>10}  label={label}");
        }
    }
}
