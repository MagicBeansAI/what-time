//! Featurize {text, spans} JSONL into the training pipeline's binary format
//! using the Rust runtime tokenizer — so multilingual corpora train on
//! exactly the tokenization inference uses, including the Devanagari mark
//! handling. Spans are
//! UTF-16 code-unit offsets and are converted to the tokenizer's byte
//! offsets before matching.
//!
//! Usage: featurize <input.jsonl> <output-prefix>

use std::io::{BufRead, Write};

use what_time::testing::{LABELS, tokenize};

#[derive(serde::Deserialize)]
struct Span {
    start: usize,
    end: usize,
    label: String,
    #[serde(default, rename = "clauseStart")]
    clause_start: bool,
}

#[derive(serde::Deserialize)]
struct Example {
    #[allow(dead_code)]
    id: String,
    text: String,
    spans: Vec<Span>,
}

fn label_id(name: &str) -> Option<u8> {
    LABELS
        .iter()
        .position(|label| *label == name)
        .map(|id| id as u8)
}

/// UTF-16 code-unit offset -> byte offset for every boundary in `text`.
fn utf16_byte_map(text: &str) -> Vec<(usize, usize)> {
    let mut map = Vec::with_capacity(text.len() / 2 + 2);
    let mut units = 0usize;
    map.push((0, 0));
    for (offset, character) in text.char_indices() {
        units += character.len_utf16();
        map.push((units, offset + character.len_utf8()));
    }
    map
}

fn to_byte(map: &[(usize, usize)], units: usize) -> usize {
    match map.binary_search_by_key(&units, |(u, _)| *u) {
        Ok(index) => map[index].1,
        Err(_) => map[map.len() - 1].1,
    }
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let (input, prefix) = match (arguments.next(), arguments.next()) {
        (Some(input), Some(prefix)) => (input, prefix),
        _ => {
            eprintln!("usage: featurize <input.jsonl> <output-prefix>");
            std::process::exit(2);
        }
    };
    let directory = std::path::Path::new(&prefix)
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    std::fs::create_dir_all(&directory).ok();

    let mut rows: Vec<u16> = Vec::new();
    let mut labels: Vec<u8> = Vec::new();
    let mut boundaries: Vec<u8> = Vec::new();
    let mut kinds: Vec<u8> = Vec::new();
    let mut neighbors: Vec<i16> = Vec::new();
    let mut offsets: Vec<u32> = vec![0];
    let mut skipped = 0u64;
    let mut written = 0u64;
    let mut misaligned = 0u64;

    let file = std::fs::File::open(&input).expect("open input");
    for line in std::io::BufReader::new(file).lines() {
        let line = line.expect("read line");
        if line.trim().is_empty() {
            continue;
        }
        let example: Example = serde_json::from_str(&line).expect("parse example");
        let tokens = tokenize(&example.text);
        if tokens.is_empty() || tokens.len() > 128 {
            skipped += 1;
            continue;
        }
        let map = utf16_byte_map(&example.text);
        let spans: Vec<(usize, usize, &str, bool)> = example
            .spans
            .iter()
            .map(|span| {
                (
                    to_byte(&map, span.start),
                    to_byte(&map, span.end),
                    span.label.as_str(),
                    span.clause_start,
                )
            })
            .collect();

        // Each parallel array keeps its own base: rows advance 17 slots per
        // token, the token-indexed arrays one, neighbors two.
        let row_base = rows.len();
        let neighbor_base = neighbors.len();
        let base = labels.len();
        rows.resize(row_base + tokens.len() * 17, 580);
        labels.resize(base + tokens.len(), 0);
        boundaries.resize(base + tokens.len(), 0);
        kinds.resize(base + tokens.len(), 0);
        neighbors.resize(neighbor_base + tokens.len() * 2, -1);

        let mut span_index = 0usize;
        let mut previous: i64 = -1;
        let mut aligned = true;
        for (index, token) in tokens.iter().enumerate() {
            for (feature, row) in what_time::testing::feature_rows(token.features)
                .iter()
                .enumerate()
            {
                rows[row_base + index * 17 + feature] = *row;
            }
            kinds[base + index] = token.kind;
            neighbors[neighbor_base + index * 2] = previous as i16;
            if token.kind != 3 {
                previous = index as i64;
            }
            if token.kind == 3 {
                continue;
            }
            while span_index < spans.len() && spans[span_index].1 <= token.start {
                span_index += 1;
            }
            let Some(&(span_start, span_end, label, clause_start)) = spans.get(span_index) else {
                aligned = false;
                break;
            };
            if token.start < span_start || token.end > span_end {
                aligned = false;
                break;
            }
            let Some(id) = label_id(label) else {
                eprintln!("unknown label {label} in {}", example.id);
                aligned = false;
                break;
            };
            labels[base + index] = id;
            boundaries[base + index] = u8::from(clause_start && token.start == span_start);
        }
        if !aligned {
            misaligned += 1;
            // Drop the partially written row block.
            rows.truncate(row_base);
            neighbors.truncate(neighbor_base);
            labels.truncate(base);
            boundaries.truncate(base);
            kinds.truncate(base);
            neighbors.truncate(base);
            continue;
        }
        let mut next: i64 = -1;
        for index in (0..tokens.len()).rev() {
            neighbors[neighbor_base + index * 2 + 1] = next as i16;
            if tokens[index].kind != 3 {
                next = index as i64;
            }
        }
        offsets.push(offsets.last().unwrap() + tokens.len() as u32);
        written += 1;
    }

    write_out(format!("{prefix}.rows.bin"), &u16_slice(&rows));
    write_out(format!("{prefix}.labels.bin"), &labels);
    write_out(format!("{prefix}.boundaries.bin"), &boundaries);
    write_out(format!("{prefix}.kinds.bin"), &kinds);
    {
        let bytes: Vec<u8> = neighbors.iter().flat_map(|v| v.to_le_bytes()).collect();
        write_out(format!("{prefix}.neighbors.bin"), &bytes);
    }
    {
        let bytes: Vec<u8> = offsets.iter().flat_map(|v| v.to_le_bytes()).collect();
        write_out(format!("{prefix}.offsets.bin"), &bytes);
    }
    let manifest = serde_json::json!({
        "sequences": written,
        "skipped": skipped,
        "misaligned": misaligned,
        "featurizer": "rust",
    });
    std::fs::write(format!("{prefix}.json"), manifest.to_string()).expect("manifest");
    println!(
        "{}",
        serde_json::json!({"stage": "featurize", "written": written, "skipped": skipped, "misaligned": misaligned})
    );
}

fn write_out(path: String, bytes: &[u8]) {
    let mut file = std::fs::File::create(&path).expect("create output");
    file.write_all(bytes).expect("write output");
}

fn u16_slice(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
