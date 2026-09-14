//! CPU inference for the experimental transformer tagger.
//!
//! Mirrors the training-time network op for op: summed
//! feature-row embeddings, learned positions, pre-LayerNorm self-attention
//! blocks with tanh-GELU feed-forward, mean-pooled context head. Like
//! `cpu.rs`, dots accumulate in f64 and round to f32 so the exported parity
//! fixtures match within the eval gate. The weights ship int8-quantized
//! (symmetric per-tensor) in `assets/weights-transformer.json`.

use std::sync::OnceLock;

use crate::model::predictions::Predictions;
use crate::tokenizer::feature_rows;
use crate::types::RawToken;

const ROWS_PER_TOKEN: usize = 17;
const SOURCE_ROWS: usize = 581; // 0..=580, with 580 as padding

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
            Ok(TransformerWeights {
                d_model,
                heads,
                layers,
                ffn,
                feature_rows,
                role_classes,
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

fn compact_feature(row: u16) -> u16 {
    if row < 140 {
        row
    } else if row < 396 {
        140 + ((row - 140) % 128)
    } else if row < 524 {
        324
    } else {
        row - 256
    }
}

pub(crate) fn feature_map() -> &'static Vec<u16> {
    static MAP: OnceLock<Vec<u16>> = OnceLock::new();
    MAP.get_or_init(|| {
        (0..SOURCE_ROWS)
            .map(|row| compact_feature(row as u16))
            .collect()
    })
}

/// out[j] = x · W[j] + b[j] with W stored [out, in] row-major.
fn linear(x: &[f32], weight: &[f32], bias: &[f32], out_dim: usize, in_dim: usize, out: &mut [f32]) {
    for j in 0..out_dim {
        let mut sum = f64::from(bias[j]);
        let base = j * in_dim;
        for (i, value) in x.iter().take(in_dim).enumerate() {
            sum += f64::from(*value) * f64::from(weight[base + i]);
        }
        out[j] = sum as f32;
    }
}

/// out[j] = x · W[:, j] + b[j] for W stored [in, out] row-major (the three
/// raw head parameters are initialized transposed relative to nn.Linear).
fn linear_t(
    x: &[f32],
    weight: &[f32],
    bias: &[f32],
    out_dim: usize,
    in_dim: usize,
    out: &mut [f32],
) {
    for j in 0..out_dim {
        let mut sum = f64::from(bias[j]);
        for i in 0..in_dim {
            sum += f64::from(x[i]) * f64::from(weight[i * out_dim + j]);
        }
        out[j] = sum as f32;
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

fn gelu_tanh(value: f32) -> f32 {
    let v = f64::from(value);
    let inner = (2.0 / std::f64::consts::PI).sqrt() * (v + 0.044715 * v.powi(3));
    (0.5 * v * (1.0 + inner.tanh())) as f32
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

/// One sequence of mapped feature rows (17 per token), one label per token.
pub fn infer_rows(rows: &[u16]) -> Result<Predictions, crate::Error> {
    let model = load().map_err(crate::Error::model_unavailable)?;
    let map = feature_map();
    let count = rows.len() / ROWS_PER_TOKEN;
    let d = model.d_model;
    let heads = model.heads;
    let d_head = d / heads;
    let classes = model.role_classes;

    let mut labels = vec![0u8; count];
    let mut clause_starts = vec![0u8; count];
    let mut scores = vec![0f32; count];
    let mut all_logits = vec![0f32; count * classes];
    let mut all_boundaries = vec![0f32; count];

    let mut embedded = vec![0f32; count * d];
    for token in 0..count {
        let mut sum = vec![0f64; d];
        for feature in 0..ROWS_PER_TOKEN {
            let mapped = map[rows[token * ROWS_PER_TOKEN + feature] as usize];
            if mapped as usize == model.feature_rows {
                continue;
            }
            let base = mapped as usize * d;
            for (j, slot) in sum.iter_mut().enumerate() {
                *slot += f64::from(model.embedding[base + j]);
            }
        }
        for j in 0..d {
            embedded[token * d + j] = sum[j] as f32 + model.position[token * d + j];
        }
    }

    let mut x = embedded;
    let mut norm = vec![0f32; count * d];
    let mut qkv = vec![0f32; count * 3 * d];
    let mut hidden = vec![0f32; count * model.ffn];
    let mut attn_scores = vec![0f32; heads * count * count];
    let mut attended = vec![0f32; d];
    let mut delta = vec![0f32; d];

    for block in &model.blocks {
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
                for other in 0..count {
                    let k_base = other * 3 * d + d + head * d_head;
                    let mut dot = 0f64;
                    for i in 0..d_head {
                        dot += f64::from(qkv[q_base + i]) * f64::from(qkv[k_base + i]);
                    }
                    attn_scores[head * count * count + token * count + other] =
                        (dot / (d_head as f64).sqrt()) as f32;
                }
            }
            attended.iter_mut().for_each(|value| *value = 0.0);
            for head in 0..heads {
                let base = head * count * count + token * count;
                softmax(&mut attn_scores[base..base + count]);
                let v_offset = 2 * d + head * d_head;
                for i in 0..d_head {
                    let mut sum = 0f64;
                    for other in 0..count {
                        sum += f64::from(attn_scores[base + other])
                            * f64::from(qkv[other * 3 * d + v_offset + i]);
                    }
                    attended[head * d_head + i] = sum as f32;
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
                model.ffn,
                d,
                &mut hidden[token * model.ffn..(token + 1) * model.ffn],
            );
            for value in hidden[token * model.ffn..(token + 1) * model.ffn].iter_mut() {
                *value = gelu_tanh(*value);
            }
            delta.iter_mut().for_each(|value| *value = 0.0);
            linear(
                &hidden[token * model.ffn..(token + 1) * model.ffn],
                &block.ff2.0,
                &block.ff2.1,
                d,
                model.ffn,
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
            &model.ln_f.0,
            &model.ln_f.1,
            &mut norm[token * d..(token + 1) * d],
        );
    }

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
    })
}

/// Token-level entry: builds feature rows and runs the forward pass.
pub fn infer(tokens: &[RawToken]) -> Result<Predictions, crate::Error> {
    let mut rows = vec![580u16; tokens.len() * ROWS_PER_TOKEN];
    for (index, token) in tokens.iter().enumerate() {
        for (feature, row) in feature_rows(token.features).iter().enumerate() {
            rows[index * ROWS_PER_TOKEN + feature] = *row;
        }
    }
    infer_rows(&rows)
}
