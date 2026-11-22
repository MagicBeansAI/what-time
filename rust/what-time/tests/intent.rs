//! The sentence-intent encoder, end to end through the public parser: a
//! request to schedule, a mention of time, and a bare schedule phrase.
//! Each case sits far from the cut-off, so the test pins behaviour, not a
//! score. Intent is computed only for text that holds a time expression.

use what_time::{IntentLabel, ParseContext, Parser};

fn intent(text: &str) -> Option<IntentLabel> {
    let context = ParseContext {
        reference: "2026-11-22T10:00:00Z".into(),
        time_zone: "Asia/Kolkata".into(),
        limit: Some(2),
        ..Default::default()
    };
    Parser::new(Default::default())
        .parse(text, &context)
        .unwrap()
        .intent
        .map(|intent| intent.label)
}

#[test]
fn tells_a_request_from_a_mention_of_time() {
    if intent("every Monday at 9").is_none() {
        eprintln!("skipping: the bundled weights have no intent encoder");
        return;
    }
    for request in [
        "remind me every 2 hours to drink water",
        "remind me in 30 minutes",
        "आज रात 10 बजे माँ को फ़ोन करना याद दिलाना",
    ] {
        assert_eq!(intent(request), Some(IntentLabel::Request), "{request}");
    }
    // A mention is never a request: either the intent says so, or the
    // tagger found no time expression and no intent is computed.
    assert_eq!(intent("school closed on Eid"), Some(IntentLabel::Mention));
    assert_ne!(
        intent("I used to jog every morning in college"),
        Some(IntentLabel::Request)
    );
    // A text that is nothing but a schedule phrase is a request.
    assert_eq!(
        intent("sava char se paune paanch tak"),
        Some(IntentLabel::Request)
    );
}
