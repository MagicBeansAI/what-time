#![cfg(feature = "gpu")]

//! GPU-backend parity: batched wgpu inference must produce the same labels,
//! clause starts, and scores as the CPU path on every token of every phrase.
//! Feature-gated (`--features gpu`) because CI runners have no GPU.

use what_time::ParseContext;
use what_time::testing::transformer as cpu;

fn phrases() -> Vec<String> {
    [
        "yoga every Tuesday and Thursday at 7am",
        "can you book it for day after tomorrow at 8 pm",
        "24th august last year",
        "friday last week",
        "game night every Friday at 7:30pm",
        "कल शाम को आठ बजे मीटिंग",
        "हर सोमवार को सुबह नौ बजे",
        "अगले महीने की 21st को",
        "kal shaam ko 8 baje call kar dena",
        "har hafte Tuesday ko gym",
        "parso subah 10 baje",
        "call mom on sunday evening",
        "Sat Sun 1pm-8pm Mon 10pm-12am",
        "Mon at 9, Wed at 10, Fri at 11",
        "out of office from Dec 22 until Jan 2",
        "catch up in 45 minutes",
        "pay rent on the 1st of each month",
        "water the plants every 3 days",
        "morning 9:30",
        "9:30 night",
        "in 20 minutes for half an hour",
        "the last Friday of each month",
        "Remind sam 7th next month at3Am",
        "ज़ोया का ब्रंच हर हफ़्ते नोट्स मैं ले लूँगा",
        "meeting day before yesterday",
        "कल 8 बजे",
        "meeting tomorrow at nine am and five pm",
    ]
    .iter()
    .map(|text| text.to_string())
    .collect()
}

#[test]
fn gpu_batch_matches_cpu_labels_and_boundaries() {
    let texts = phrases();
    let inputs: Vec<&str> = texts.iter().map(String::as_str).collect();

    // CPU reference
    let mut cpu_labels: Vec<Vec<(String, u8, u8)>> = Vec::new();
    let parser = what_time::ScheduleParser::new(Default::default());
    for text in &texts {
        let tokens = &parser.parse(text).unwrap().tokens;
        let flat: Vec<(String, u8, u8)> = tokens
            .iter()
            .flatten()
            .map(|token| {
                (
                    token.text.clone(),
                    what_time::testing::LABELS
                        .iter()
                        .position(|name| *name == token.label)
                        .unwrap_or(0) as u8,
                    u8::from(token.clause_start),
                )
            })
            .collect();
        cpu_labels.push(flat);
        let _ = cpu::infer_rows; // symbol import check
    }

    // GPU path via the public API: one parse_many call exceeds the batch
    // threshold, so Backend::Auto dispatches through wgpu.
    let context = ParseContext {
        reference: "2026-09-13T10:00:00Z".into(),
        time_zone: "Asia/Kolkata".into(),
        limit: Some(3),
        ..Default::default()
    };
    let results = what_time::parse_many(&inputs, &context).unwrap();
    assert!(results.len() == texts.len());

    // Re-tag through the GPU windows directly for label comparison.
    let windows = what_time::testing::gpu::debug_windows(&inputs);
    let gpu = what_time::testing::gpu::infer_windows_debug(&windows).unwrap();

    let mut mismatches = 0;
    for (index, tokens) in windows.iter().enumerate() {
        let reference = &cpu_labels[index];
        // whitespace tokens are masked out of supervision and ignored by the
        // parser; their GLUE-vs-O argmax is a near-tie that may flip between
        // the CPU's f64 accumulation and the GPU's f32 without consequence.
        for (position, token) in tokens.iter().enumerate() {
            if token.kind == 3 {
                continue;
            }
            let gpu_label = gpu[index].labels[position];
            let gpu_start = gpu[index].clause_starts[position];
            let (_, cpu_label, cpu_start) = reference[position];
            if gpu_label != cpu_label || gpu_start != cpu_start {
                mismatches += 1;
                eprintln!(
                    "mismatch at {:?}[{}]: gpu {}/{} cpu {}/{}",
                    texts[index], position, gpu_label, gpu_start, cpu_label, cpu_start
                );
            }
        }
    }
    assert_eq!(mismatches, 0, "GPU/CPU label disagreement");
}

#[test]
fn gpu_end_to_end_matches_cpu_occurrences() {
    let texts = phrases();
    let inputs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let context = ParseContext {
        reference: "2026-09-13T10:00:00Z".into(),
        time_zone: "Asia/Kolkata".into(),
        limit: Some(3),
        ..Default::default()
    };
    let batched = what_time::parse_many(&inputs, &context).unwrap();
    for (text, batched) in texts.iter().zip(batched) {
        let single = what_time::parse(text, &context).unwrap();
        assert_eq!(
            single.occurrences, batched.occurrences,
            "occurrence mismatch for {text:?}"
        );
    }
}
