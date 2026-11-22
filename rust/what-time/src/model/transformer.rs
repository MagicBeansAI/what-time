//! CPU inference for the transformer tagger.
//!
//! Mirrors the training-time network op for op: summed
//! feature-row embeddings, learned positions, pre-LayerNorm self-attention
//! blocks with tanh-GELU feed-forward, mean-pooled context head. Matrix
//! products accumulate in f32 across several lanes so they vectorize;
//! normalization, softmax and activations run in f64. The exported parity
//! fixtures gate the result. The weights ship int8-quantized (symmetric
//! per-tensor) in `assets/weights-transformer.json`.

use std::sync::OnceLock;

use crate::model::predictions::Predictions;
use crate::tokenizer::{FEATURE_ROWS, ROWS_PER_TOKEN, feature_rows};
use crate::types::RawToken;

/// The tokenizer's row ids, `0..=FEATURE_ROWS`, the last one padding.
const SOURCE_ROWS: usize = FEATURE_ROWS + 1;

pub struct TransformerWeights {
    pub d_model: usize,
    pub heads: usize,
    pub layers: usize,
    pub ffn: usize,
    pub feature_rows: usize,
    pub role_classes: usize,
    pub boundary_threshold: f64,
    pub(crate) embedding: Vec<f32>, // [feature_rows, d_model]
    pub(crate) position: Vec<f32>,  // [max_positions, d_model]
    pub(crate) ln_f: (Vec<f32>, Vec<f32>),
    pub(crate) global: (Vec<f32>, Vec<f32>), // [d, d]
    pub(crate) head: (Vec<f32>, Vec<f32>),   // [2d, 64]
    pub(crate) output: (Vec<f32>, Vec<f32>), // [64, role_classes + 1]
    pub(crate) blocks: Vec<BlockWeights>,
    /// The sentence-intent head, absent from weights trained before it.
    pub(crate) intent: Option<IntentWeights>,
    /// Tokenizer row id -> embedding row; `feature_rows` marks padding.
    pub(crate) row_map: Vec<u16>,
}

/// Sentence intent (request vs mention): a small encoder of its own over
/// the same feature rows as the tagger (so it sees the words, not only the
/// tagger's view of them), its final states pooled three ways (mean, max,
/// attention with one learned query), a tanh layer and one logit. Mirrors
/// `IntentEncoder` in the training code.
pub(crate) struct IntentWeights {
    pub(crate) d_model: usize,
    pub(crate) heads: usize,
    pub(crate) ffn: usize,
    pub(crate) embedding: Vec<f32>,
    pub(crate) position: Vec<f32>,
    pub(crate) blocks: Vec<BlockWeights>,
    pub(crate) ln_f: (Vec<f32>, Vec<f32>),
    pub(crate) query: Vec<f32>,              // [d]
    pub(crate) hidden: (Vec<f32>, Vec<f32>), // [3d, h]
    pub(crate) output: (Vec<f32>, Vec<f32>), // [h, 1]
    pub(crate) width: usize,
    /// Request probability at or above which the label is "request".
    pub(crate) threshold: f32,
}

pub(crate) struct BlockWeights {
    pub(crate) ln1: (Vec<f32>, Vec<f32>),
    pub(crate) qkv: (Vec<f32>, Vec<f32>),      // [3d, d]
    pub(crate) attn_out: (Vec<f32>, Vec<f32>), // [d, d]
    pub(crate) ln2: (Vec<f32>, Vec<f32>),
    pub(crate) ff1: (Vec<f32>, Vec<f32>), // [ffn, d]
    pub(crate) ff2: (Vec<f32>, Vec<f32>), // [d, ffn]
}

fn decode_base64(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for character in text.bytes() {
        let value = match character {
            b'A'..=b'Z' => (character - b'A') as u32,
            b'a'..=b'z' => (character - b'a' + 26) as u32,
            b'0'..=b'9' => (character - b'0' + 52) as u32,
            b'+' => 62,
            b'/' => 63,
            b'\n' | b'\r' => continue,
            b'=' => break, // padding terminates the stream
            _ => return Err("invalid base64 digit".into()),
        };
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Ok(out)
}

fn tensor(
    tensors: &serde_json::Map<String, serde_json::Value>,
    name: &str,
) -> Result<(Vec<f32>, Vec<usize>), String> {
    let entry = tensors
        .get(name)
        .ok_or_else(|| format!("missing tensor {name}"))?;
    let shape: Vec<usize> = entry["shape"]
        .as_array()
        .ok_or_else(|| format!("tensor {name} missing shape"))?
        .iter()
        .map(|value| value.as_u64().unwrap_or(0) as usize)
        .collect();
    let scale = entry["scale"].as_f64().unwrap_or(0.0) as f32;
    let data = decode_base64(entry["data"].as_str().unwrap_or(""))?;
    let count: usize = shape.iter().product();
    if data.len() != count {
        return Err(format!(
            "tensor {name} has {} of {} values",
            data.len(),
            count
        ));
    }
    let values = data
        .iter()
        .map(|byte| (f64::from(*byte as i8) * f64::from(scale)) as f32)
        .collect();
    Ok((values, shape))
}

pub(crate) fn load() -> Result<&'static TransformerWeights, String> {
    static WEIGHTS: OnceLock<Result<TransformerWeights, String>> = OnceLock::new();
    WEIGHTS
        .get_or_init(|| {
            let text = include_str!("../../assets/weights-transformer.json");
            let document: serde_json::Value =
                serde_json::from_str(text).map_err(|error| error.to_string())?;
            if document
                .get("untrained")
                .is_some_and(|flag| flag.as_bool() == Some(true))
            {
                return Err("transformer weights are a placeholder; train first".into());
            }
            let tensors = document["tensors"]
                .as_object()
                .ok_or("missing tensors object")?;
            let d_model = document["dModel"].as_u64().unwrap_or(64) as usize;
            let heads = document["heads"].as_u64().unwrap_or(4) as usize;
            let layers = document["layers"].as_u64().unwrap_or(2) as usize;
            let ffn = document["ffn"].as_u64().unwrap_or(256) as usize;
            let feature_rows = document["featureRows"].as_u64().unwrap_or(324) as usize;
            let role_classes = document["roleClasses"].as_u64().unwrap_or(40) as usize;
            let pair = |name: &str| -> Result<(Vec<f32>, Vec<f32>), String> {
                Ok((
                    tensor(tensors, &format!("{name}.weight"))?.0,
                    tensor(tensors, &format!("{name}.bias"))?.0,
                ))
            };
            // The head parameters are raw tensors, not modules.
            let flat = |name: &str| -> Result<(Vec<f32>, Vec<f32>), String> {
                Ok((
                    tensor(tensors, &format!("{name}_weight"))?.0,
                    tensor(tensors, &format!("{name}_bias"))?.0,
                ))
            };
            let mut blocks = Vec::with_capacity(layers);
            for index in 0..layers {
                let prefix = format!("blocks.{index}");
                blocks.push(BlockWeights {
                    ln1: pair(&format!("{prefix}.ln1"))?,
                    qkv: pair(&format!("{prefix}.qkv"))?,
                    attn_out: pair(&format!("{prefix}.attn_out"))?,
                    ln2: pair(&format!("{prefix}.ln2"))?,
                    ff1: pair(&format!("{prefix}.ff1"))?,
                    ff2: pair(&format!("{prefix}.ff2"))?,
                });
            }
            let intent = match document.get("intentEncoder") {
                Some(config) => {
                    let width = config["hidden"].as_u64().unwrap_or(64) as usize;
                    let intent_layers = config["layers"].as_u64().unwrap_or(2) as usize;
                    let mut intent_blocks = Vec::with_capacity(intent_layers);
                    for index in 0..intent_layers {
                        let prefix = format!("intent.blocks.{index}");
                        intent_blocks.push(BlockWeights {
                            ln1: pair(&format!("{prefix}.ln1"))?,
                            qkv: pair(&format!("{prefix}.qkv"))?,
                            attn_out: pair(&format!("{prefix}.attn_out"))?,
                            ln2: pair(&format!("{prefix}.ln2"))?,
                            ff1: pair(&format!("{prefix}.ff1"))?,
                            ff2: pair(&format!("{prefix}.ff2"))?,
                        });
                    }
                    Some(IntentWeights {
                        d_model: config["dModel"].as_u64().unwrap_or(64) as usize,
                        heads: config["heads"].as_u64().unwrap_or(4) as usize,
                        ffn: config["ffn"].as_u64().unwrap_or(128) as usize,
                        embedding: tensor(tensors, "intent.embedding")?.0,
                        position: tensor(tensors, "intent.position")?.0,
                        blocks: intent_blocks,
                        ln_f: pair("intent.ln_f")?,
                        query: tensor(tensors, "intent.query")?.0,
                        hidden: flat("intent.hidden")?,
                        output: flat("intent.output")?,
                        width,
                        threshold: config["threshold"].as_f64().unwrap_or(0.5) as f32,
                    })
                }
                None => None,
            };
            Ok(TransformerWeights {
                intent,
                d_model,
                heads,
                layers,
                ffn,
                feature_rows,
                role_classes,
                row_map: (0..SOURCE_ROWS)
                    .map(|row| embedding_row(row as u16, feature_rows))
                    .collect(),
                boundary_threshold: document["boundaryThreshold"].as_f64().unwrap_or(1.75),
                embedding: tensor(tensors, "embedding")?.0,
                position: tensor(tensors, "position")?.0,
                ln_f: pair("ln_f")?,
                global: flat("global")?,
                head: flat("head")?,
                output: flat("output")?,
                blocks,
            })
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Whether real transformer weights are bundled (the placeholder fails load).
pub fn available() -> bool {
    load().is_ok()
}

/// Maps a tokenizer row onto a model's embedding table. The current table
/// takes every row as is. Two earlier, smaller tables stay loadable: 580
/// rows (no lexical region) and 324 rows (word hashes folded, consonant
/// hashes dropped).
fn embedding_row(row: u16, feature_rows: usize) -> u16 {
    let padding = feature_rows as u16;
    if feature_rows >= FEATURE_ROWS {
        return row;
    }
    if row >= 580 {
        return padding;
    }
    if feature_rows >= 580 {
        return row;
    }
    if row < 140 {
        row
    } else if row < 396 {
        140 + ((row - 140) % 128)
    } else if row < 524 {
        padding
    } else {
        row - 256
    }
}

/// Lanes in a dot product: independent f32 partial sums the compiler keeps
/// in vector registers. Like PyTorch's own reductions, the sum is f32 but
/// never one long serial chain, so rounding stays well inside the parity
/// gate.
const LANES: usize = 8;

fn dot(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let (a, b) = (&a[..n], &b[..n]);
    let mut lanes = [0f32; LANES];
    let left = a.chunks_exact(LANES);
    let right = b.chunks_exact(LANES);
    let mut sum = 0f32;
    for (x, y) in left.remainder().iter().zip(right.remainder()) {
        sum += x * y;
    }
    for (x, y) in left.zip(right) {
        for lane in 0..LANES {
            lanes[lane] += x[lane] * y[lane];
        }
    }
    let mut pairs = LANES;
    while pairs > 1 {
        pairs /= 2;
        for lane in 0..pairs {
            lanes[lane] += lanes[lane + pairs];
        }
    }
    lanes[0] + sum
}

/// out[j] = x · W[j] + b[j] with W stored [out, in] row-major.
fn linear(x: &[f32], weight: &[f32], bias: &[f32], out_dim: usize, in_dim: usize, out: &mut [f32]) {
    let x = &x[..in_dim];
    for j in 0..out_dim {
        out[j] = bias[j] + dot(x, &weight[j * in_dim..(j + 1) * in_dim]);
    }
}

/// out[j] = x · W[:, j] + b[j] for W stored [in, out] row-major (the three
/// raw head parameters are initialized transposed relative to nn.Linear).
/// Rows of W are added in turn, so every pass reads memory in order.
fn linear_t(
    x: &[f32],
    weight: &[f32],
    bias: &[f32],
    out_dim: usize,
    in_dim: usize,
    out: &mut [f32],
) {
    let out = &mut out[..out_dim];
    out.copy_from_slice(&bias[..out_dim]);
    for (i, value) in x.iter().take(in_dim).enumerate() {
        let row = &weight[i * out_dim..(i + 1) * out_dim];
        for (slot, w) in out.iter_mut().zip(row) {
            *slot += value * w;
        }
    }
}

fn layer_norm(x: &[f32], weight: &[f32], bias: &[f32], out: &mut [f32]) {
    let n = x.len();
    let mut mean = 0f64;
    for value in x {
        mean += f64::from(*value);
    }
    mean /= n as f64;
    let mut variance = 0f64;
    for value in x {
        variance += (f64::from(*value) - mean).powi(2);
    }
    variance /= n as f64;
    let scale = 1.0 / (variance + 1e-5).sqrt();
    for j in 0..n {
        out[j] =
            ((f64::from(x[j]) - mean) * scale * f64::from(weight[j]) + f64::from(bias[j])) as f32;
    }
}

/// tanh-GELU in f32, as the training network computes it.
fn gelu_tanh(value: f32) -> f32 {
    const ROOT_TWO_OVER_PI: f32 = 0.797_884_6;
    let inner = ROOT_TWO_OVER_PI * (value + 0.044715 * value * value * value);
    0.5 * value * (1.0 + inner.tanh())
}

fn sigmoid(value: f32) -> f32 {
    (1.0 / (1.0 + (-(f64::from(value))).exp())) as f32
}

fn softmax(scores: &mut [f32]) {
    let mut max = f32::NEG_INFINITY;
    for value in scores.iter() {
        max = max.max(*value);
    }
    let mut total = 0f64;
    for value in scores.iter_mut() {
        *value = ((f64::from(*value - max)).exp()) as f32;
        total += f64::from(*value);
    }
    for value in scores.iter_mut() {
        *value = (f64::from(*value) / total) as f32;
    }
}

/// The parameters of one encoder: summed feature-row embeddings plus
/// positions, pre-LayerNorm blocks, a final LayerNorm. The tagger and the
/// intent encoder share this forward pass.
pub(crate) struct EncoderView<'a> {
    pub(crate) d: usize,
    pub(crate) heads: usize,
    pub(crate) ffn: usize,
    pub(crate) embedding: &'a [f32],
    pub(crate) position: &'a [f32],
    pub(crate) blocks: &'a [BlockWeights],
    pub(crate) ln_f: &'a (Vec<f32>, Vec<f32>),
}

/// The final (LayerNormed) token states, `[count, d]`, of one sequence.
fn encode(
    view: &EncoderView<'_>,
    rows: &[u16],
    rows_per_token: usize,
    map: &[u16],
    feature_rows: usize,
) -> Vec<f32> {
    let count = rows.len() / rows_per_token;
    let d = view.d;
    let heads = view.heads;
    let d_head = d / heads;
    let mut embedded = vec![0f32; count * d];
    for token in 0..count {
        let mut sum = vec![0f64; d];
        for feature in 0..rows_per_token {
            let source = rows[token * rows_per_token + feature] as usize;
            // Rows past this table (a fixture padded for a wider one) are padding.
            let mapped = map.get(source).copied().unwrap_or(feature_rows as u16);
            if mapped as usize == feature_rows {
                continue;
            }
            let base = mapped as usize * d;
            for (j, slot) in sum.iter_mut().enumerate() {
                *slot += f64::from(view.embedding[base + j]);
            }
        }
        for j in 0..d {
            embedded[token * d + j] = sum[j] as f32 + view.position[token * d + j];
        }
    }

    let mut x = embedded;
    let mut norm = vec![0f32; count * d];
    let mut qkv = vec![0f32; count * 3 * d];
    let mut hidden = vec![0f32; count * view.ffn];
    let mut attn_scores = vec![0f32; heads * count * count];
    let mut attended = vec![0f32; d];
    let mut delta = vec![0f32; d];

    for block in view.blocks {
        for token in 0..count {
            layer_norm(
                &x[token * d..(token + 1) * d],
                &block.ln1.0,
                &block.ln1.1,
                &mut norm[token * d..(token + 1) * d],
            );
            linear(
                &norm[token * d..(token + 1) * d],
                &block.qkv.0,
                &block.qkv.1,
                3 * d,
                d,
                &mut qkv[token * 3 * d..(token + 1) * 3 * d],
            );
        }
        for token in 0..count {
            // residual: x += attn_out(attention(q[token], k[*], v[*]))
            for head in 0..heads {
                let q_base = token * 3 * d + head * d_head;
                let query = &qkv[q_base..q_base + d_head];
                let scale = 1.0 / (d_head as f32).sqrt();
                for other in 0..count {
                    let k_base = other * 3 * d + d + head * d_head;
                    attn_scores[head * count * count + token * count + other] =
                        dot(query, &qkv[k_base..k_base + d_head]) * scale;
                }
            }
            attended.iter_mut().for_each(|value| *value = 0.0);
            for head in 0..heads {
                let base = head * count * count + token * count;
                softmax(&mut attn_scores[base..base + count]);
                let v_offset = 2 * d + head * d_head;
                let target = &mut attended[head * d_head..(head + 1) * d_head];
                for other in 0..count {
                    let weight = attn_scores[base + other];
                    let value = &qkv[other * 3 * d + v_offset..other * 3 * d + v_offset + d_head];
                    for (slot, v) in target.iter_mut().zip(value) {
                        *slot += weight * v;
                    }
                }
            }
            delta.iter_mut().for_each(|value| *value = 0.0);
            linear(
                &attended,
                &block.attn_out.0,
                &block.attn_out.1,
                d,
                d,
                &mut delta,
            );
            for j in 0..d {
                x[token * d + j] += delta[j];
            }
        }
        for token in 0..count {
            layer_norm(
                &x[token * d..(token + 1) * d],
                &block.ln2.0,
                &block.ln2.1,
                &mut norm[token * d..(token + 1) * d],
            );
            linear(
                &norm[token * d..(token + 1) * d],
                &block.ff1.0,
                &block.ff1.1,
                view.ffn,
                d,
                &mut hidden[token * view.ffn..(token + 1) * view.ffn],
            );
            for value in hidden[token * view.ffn..(token + 1) * view.ffn].iter_mut() {
                *value = gelu_tanh(*value);
            }
            delta.iter_mut().for_each(|value| *value = 0.0);
            linear(
                &hidden[token * view.ffn..(token + 1) * view.ffn],
                &block.ff2.0,
                &block.ff2.1,
                d,
                view.ffn,
                &mut delta,
            );
            for j in 0..d {
                x[token * d + j] += delta[j];
            }
        }
    }

    for token in 0..count {
        layer_norm(
            &x[token * d..(token + 1) * d],
            &view.ln_f.0,
            &view.ln_f.1,
            &mut norm[token * d..(token + 1) * d],
        );
    }

    norm
}

/// One sequence of tokenizer feature rows (`ROWS_PER_TOKEN` per token), one
/// label per token. The intent is left out: the tagger asks for it only
/// when a window holds a time expression ([`window_intent`]).
pub fn infer_rows(rows: &[u16]) -> Result<Predictions, crate::Error> {
    let mut predictions = infer_tagger(rows, ROWS_PER_TOKEN)?;
    predictions.intent = None;
    Ok(predictions)
}

/// As [`infer_rows_per_token`] without the intent encoder.
fn infer_tagger(rows: &[u16], rows_per_token: usize) -> Result<Predictions, crate::Error> {
    infer_rows_inner(rows, rows_per_token, false)
}

/// As [`infer_rows`], for row blocks of another width (parity fixtures
/// record the width they were exported with).
pub fn infer_rows_per_token(
    rows: &[u16],
    rows_per_token: usize,
) -> Result<Predictions, crate::Error> {
    infer_rows_inner(rows, rows_per_token, true)
}

fn infer_rows_inner(
    rows: &[u16],
    rows_per_token: usize,
    with_intent: bool,
) -> Result<Predictions, crate::Error> {
    let model = load().map_err(crate::Error::model_unavailable)?;
    let map = &model.row_map;
    let count = rows.len() / rows_per_token;
    let d = model.d_model;
    let heads = model.heads;
    let classes = model.role_classes;

    let mut labels = vec![0u8; count];
    let mut clause_starts = vec![0u8; count];
    let mut scores = vec![0f32; count];
    let mut all_logits = vec![0f32; count * classes];
    let mut all_boundaries = vec![0f32; count];

    let view = EncoderView {
        d,
        heads,
        ffn: model.ffn,
        embedding: &model.embedding,
        position: &model.position,
        blocks: &model.blocks,
        ln_f: &model.ln_f,
    };
    let norm = encode(&view, rows, rows_per_token, map, model.feature_rows);

    // Mean-pooled context gate, then the shared head.
    let mut pooled = vec![0f32; d];
    let mut total = vec![0f64; d];
    for token in 0..count {
        for j in 0..d {
            total[j] += f64::from(norm[token * d + j]);
        }
    }
    for j in 0..d {
        pooled[j] = (total[j] / count as f64) as f32;
    }
    let mut context = vec![0f32; d];
    linear_t(
        &pooled,
        &model.global.0,
        &model.global.1,
        d,
        d,
        &mut context,
    );
    for j in 0..d {
        context[j] = sigmoid(context[j]) * pooled[j];
    }
    let intent = model
        .intent
        .as_ref()
        .filter(|_| with_intent)
        .map(|head| intent_logit(head, rows, rows_per_token, map, model.feature_rows));

    let mut joined = vec![0f32; 2 * d];
    let mut hidden64 = vec![0f32; 64];
    let mut output = vec![0f32; classes + 1];
    for token in 0..count {
        joined[..d].copy_from_slice(&norm[token * d..(token + 1) * d]);
        joined[d..].copy_from_slice(&context);
        linear_t(
            &joined,
            &model.head.0,
            &model.head.1,
            64,
            2 * d,
            &mut hidden64,
        );
        for value in hidden64.iter_mut() {
            *value = f64::from(*value).tanh() as f32;
        }
        linear_t(
            &hidden64,
            &model.output.0,
            &model.output.1,
            classes + 1,
            64,
            &mut output,
        );
        all_boundaries[token] = output[classes];
        all_logits[token * classes..(token + 1) * classes].copy_from_slice(&output[..classes]);

        let mut best = 0usize;
        let mut second = f64::NEG_INFINITY;
        for label in 1..classes {
            if output[label] > output[best] {
                second = f64::from(output[best]);
                best = label;
            } else {
                second = second.max(f64::from(output[label]));
            }
        }
        let mut denominator = 0f64;
        for label in 0..classes {
            denominator += (f64::from(output[label]) - f64::from(output[best])).exp();
        }
        labels[token] = best as u8;
        clause_starts[token] = (f64::from(output[classes]) >= model.boundary_threshold) as u8;
        scores[token] = ((1.0 - (second - f64::from(output[best])).exp()) / denominator) as f32;
    }

    Ok(Predictions {
        labels,
        clause_starts,
        scores,
        logits: Some(all_logits),
        boundary_logits: Some(all_boundaries),
        intent,
    })
}

/// The intent logit of one window: the intent encoder's final states,
/// pooled by mean, max and one learned attention query, then the head.
fn intent_logit(
    head: &IntentWeights,
    rows: &[u16],
    rows_per_token: usize,
    map: &[u16],
    feature_rows: usize,
) -> f32 {
    let d = head.d_model;
    let view = EncoderView {
        d,
        heads: head.heads,
        ffn: head.ffn,
        embedding: &head.embedding,
        position: &head.position,
        blocks: &head.blocks,
        ln_f: &head.ln_f,
    };
    let norm = encode(&view, rows, rows_per_token, map, feature_rows);
    let count = rows.len() / rows_per_token;
    let mut total = vec![0f64; d];
    let mut maximum = vec![f32::MIN; d];
    let mut scores = vec![0f32; count];
    let scale = 1.0 / (d as f32).sqrt();
    for token in 0..count {
        let state = &norm[token * d..(token + 1) * d];
        for j in 0..d {
            total[j] += f64::from(state[j]);
            maximum[j] = maximum[j].max(state[j]);
        }
        scores[token] = dot(state, &head.query) * scale;
    }
    softmax(&mut scores);
    let mut attended = vec![0f32; d];
    for (token, weight) in scores.iter().enumerate() {
        for (slot, value) in attended.iter_mut().zip(&norm[token * d..(token + 1) * d]) {
            *slot += weight * value;
        }
    }
    let mut features = Vec::with_capacity(3 * d);
    features.extend(total.iter().map(|sum| (sum / count.max(1) as f64) as f32));
    features.extend_from_slice(&maximum);
    features.extend_from_slice(&attended);
    let mut hidden = vec![0f32; head.width];
    linear_t(
        &features,
        &head.hidden.0,
        &head.hidden.1,
        head.width,
        3 * d,
        &mut hidden,
    );
    for value in hidden.iter_mut() {
        *value = f64::from(*value).tanh() as f32;
    }
    let mut output = [0f32; 1];
    linear_t(
        &hidden,
        &head.output.0,
        &head.output.1,
        1,
        head.width,
        &mut output,
    );
    output[0]
}

/// The intent logit of one window of tokens, when the weights carry an
/// intent encoder (the GPU path calls this per window).
pub fn window_intent(tokens: &[RawToken]) -> Option<f32> {
    let model = load().ok()?;
    let head = model.intent.as_ref()?;
    let mut rows = vec![FEATURE_ROWS as u16; tokens.len() * ROWS_PER_TOKEN];
    for (index, token) in tokens.iter().enumerate() {
        for (feature, row) in feature_rows(token).iter().enumerate() {
            rows[index * ROWS_PER_TOKEN + feature] = *row;
        }
    }
    Some(intent_logit(
        head,
        &rows,
        ROWS_PER_TOKEN,
        &model.row_map,
        model.feature_rows,
    ))
}

/// The request probability above which a window reads as a request, when
/// the loaded weights carry an intent head.
pub fn intent_threshold() -> Option<f32> {
    load().ok()?.intent.as_ref().map(|head| head.threshold)
}

/// Token-level entry: builds feature rows and runs the forward pass.
pub fn infer(tokens: &[RawToken]) -> Result<Predictions, crate::Error> {
    let mut rows = vec![FEATURE_ROWS as u16; tokens.len() * ROWS_PER_TOKEN];
    for (index, token) in tokens.iter().enumerate() {
        for (feature, row) in feature_rows(token).iter().enumerate() {
            rows[index * ROWS_PER_TOKEN + feature] = *row;
        }
    }
    infer_rows(&rows)
}
