// Batched transformer forward pass. Layout contract with model/gpu.rs:
// one params buffer of f32 (offsets below), one scratch buffer holding all
// per-token intermediates at computed offsets, rows pre-mapped (324 =
// padding). Four storage buffers total — within the 8-buffer stage limit.

const D: u32 = 64u;
const H: u32 = 4u;
const DH: u32 = 16u;
const FFN: u32 = 256u;

const OFF_EMB: u32 = 0u;
const OFF_POS: u32 = 324u * D;
const OFF_BLOCKS: u32 = (324u + 128u) * D;
// per-block order: ln1.w ln1.b qkv.w qkv.b ao.w ao.b ln2.w ln2.b ff1.w ff1.b ff2.w ff2.b
const BLOCK_STRIDE: u32 = 49984u;
const B_LN1: u32 = 0u;
const B_QKV_W: u32 = 128u;
const B_QKV_B: u32 = 12416u;
const B_AO_W: u32 = 12608u;
const B_AO_B: u32 = 16704u;
const B_LN2: u32 = 16768u;
const B_FF1_W: u32 = 16896u;
const B_FF1_B: u32 = 33280u;
const B_FF2_W: u32 = 33536u;
const B_FF2_B: u32 = 49920u;
const OFF_LNF: u32 = OFF_BLOCKS + 2u * BLOCK_STRIDE;
const OFF_GW: u32 = OFF_LNF + 128u;
const OFF_GB: u32 = OFF_GW + 4096u;
const OFF_HW: u32 = OFF_GB + 64u;
const OFF_HB: u32 = OFF_HW + 8192u;
const OFF_OW: u32 = OFF_HB + 64u;
const OFF_OB: u32 = OFF_OW + 2624u;

struct Uni {
    b: u32,
    t: u32,
    block: u32,
    lens_len: u32,
    boundary_threshold: f32,
    role_classes: f32,
}

@group(0) @binding(0) var<uniform> u: Uni;
@group(1) @binding(0) var<storage, read> params: array<f32>;
@group(1) @binding(1) var<storage, read> rows: array<u32>;
@group(1) @binding(2) var<storage, read> lens: array<u32>;
@group(1) @binding(3) var<storage, read_write> scratch: array<f32>;

fn n_tokens() -> u32 { return u.b * u.t; }
fn off_x() -> u32 { return 0u; }
fn off_qkv() -> u32 { return n_tokens() * 64u; }
fn off_att() -> u32 { return off_qkv() + n_tokens() * 192u; }
fn off_ff() -> u32 { return off_att() + n_tokens() * 64u; }
fn off_out() -> u32 { return off_ff() + n_tokens() * 256u; }
fn off_lab() -> u32 { return off_out() + n_tokens() * 41u; }
fn off_sc() -> u32 { return off_lab() + n_tokens(); }

fn block_base() -> u32 {
    return OFF_BLOCKS + u.block * BLOCK_STRIDE;
}

@compute @workgroup_size(64)
fn embed(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= n_tokens()) { return; }
    let t = i % u.t;
    for (var d: u32 = 0u; d < D; d = d + 1u) {
        scratch[off_x() + i * D + d] = params[OFF_POS + t * D + d];
    }
    for (var f: u32 = 0u; f < 17u; f = f + 1u) {
        let row = rows[i * 17u + f];
        if (row == 324u) { continue; }
        for (var d: u32 = 0u; d < D; d = d + 1u) {
            scratch[off_x() + i * D + d] = scratch[off_x() + i * D + d] + params[row * D + d];
        }
    }
}

@compute @workgroup_size(64)
fn stage_qkv(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= n_tokens()) { return; }
    let base = block_base();
    var mean = 0.0;
    for (var d: u32 = 0u; d < D; d = d + 1u) { mean = mean + scratch[off_x() + i * D + d]; }
    mean = mean / f32(D);
    var variance = 0.0;
    for (var d: u32 = 0u; d < D; d = d + 1u) {
        let diff = scratch[off_x() + i * D + d] - mean;
        variance = variance + diff * diff;
    }
    variance = variance / f32(D);
    let scale = 1.0 / sqrt(variance + 1e-5);
    for (var j: u32 = 0u; j < 3u * D; j = j + 1u) {
        var acc = params[base + B_QKV_B + j];
        for (var d: u32 = 0u; d < D; d = d + 1u) {
            let normalized = (scratch[off_x() + i * D + d] - mean) * scale * params[base + B_LN1 + d]
                + params[base + B_LN1 + 64u + d];
            acc = acc + normalized * params[base + B_QKV_W + j * D + d];
        }
        scratch[off_qkv() + i * 3u * D + j] = acc;
    }
}

@compute @workgroup_size(64)
fn attn(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= u.b * H * u.t) { return; }
    let bt = i / H;
    let head = i % H;
    let b = bt / u.t;
    let real = lens[b];
    var s: array<f32, 112>;
    var smax = -3.4e38;
    for (var j: u32 = 0u; j < real; j = j + 1u) {
        var dot = 0.0;
        for (var d: u32 = 0u; d < DH; d = d + 1u) {
            dot = dot + scratch[off_qkv() + bt * 3u * D + head * DH + d]
                * scratch[off_qkv() + (b * u.t + j) * 3u * D + D + head * DH + d];
        }
        s[j] = dot * 0.25;
        smax = max(smax, s[j]);
    }
    var total = 0.0;
    for (var j: u32 = 0u; j < real; j = j + 1u) {
        s[j] = exp(s[j] - smax);
        total = total + s[j];
    }
    for (var d: u32 = 0u; d < DH; d = d + 1u) {
        var acc = 0.0;
        for (var j: u32 = 0u; j < real; j = j + 1u) {
            acc = acc + s[j] * scratch[off_qkv() + (b * u.t + j) * 3u * D + 2u * D + head * DH + d];
        }
        scratch[off_att() + bt * D + head * DH + d] = acc / total;
    }
}

@compute @workgroup_size(64)
fn attnout_res(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= n_tokens()) { return; }
    let base = block_base();
    let b = i / u.t;
    if (i % u.t >= lens[b]) {
        // keep padding slots inert after every block
        for (var d: u32 = 0u; d < D; d = d + 1u) { scratch[off_x() + i * D + d] = 0.0; }
        return;
    }
    for (var j: u32 = 0u; j < D; j = j + 1u) {
        var acc = params[base + B_AO_B + j];
        for (var d: u32 = 0u; d < D; d = d + 1u) {
            acc = acc + scratch[off_att() + i * D + d] * params[base + B_AO_W + j * D + d];
        }
        scratch[off_x() + i * D + j] = scratch[off_x() + i * D + j] + acc;
    }
}

@compute @workgroup_size(64)
fn ff(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= n_tokens()) { return; }
    let base = block_base();
    var mean = 0.0;
    for (var d: u32 = 0u; d < D; d = d + 1u) { mean = mean + scratch[off_x() + i * D + d]; }
    mean = mean / f32(D);
    var variance = 0.0;
    for (var d: u32 = 0u; d < D; d = d + 1u) {
        let diff = scratch[off_x() + i * D + d] - mean;
        variance = variance + diff * diff;
    }
    variance = variance / f32(D);
    let scale = 1.0 / sqrt(variance + 1e-5);
    for (var j: u32 = 0u; j < FFN; j = j + 1u) {
        var acc = params[base + B_FF1_B + j];
        for (var d: u32 = 0u; d < D; d = d + 1u) {
            let normalized = (scratch[off_x() + i * D + d] - mean) * scale * params[base + B_LN2 + d]
                + params[base + B_LN2 + 64u + d];
            acc = acc + normalized * params[base + B_FF1_W + j * D + d];
        }
        let inner = 0.7978845608028654 * (acc + 0.044715 * acc * acc * acc);
        scratch[off_ff() + i * FFN + j] = 0.5 * acc * (1.0 + tanh(inner));
    }
    for (var j: u32 = 0u; j < D; j = j + 1u) {
        var acc = params[base + B_FF2_B + j];
        for (var k: u32 = 0u; k < FFN; k = k + 1u) {
            acc = acc + scratch[off_ff() + i * FFN + k] * params[base + B_FF2_W + j * FFN + k];
        }
        scratch[off_x() + i * D + j] = scratch[off_x() + i * D + j] + acc;
    }
}

@compute @workgroup_size(64)
fn lnf(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= n_tokens()) { return; }
    var mean = 0.0;
    for (var d: u32 = 0u; d < D; d = d + 1u) { mean = mean + scratch[off_x() + i * D + d]; }
    mean = mean / f32(D);
    var variance = 0.0;
    for (var d: u32 = 0u; d < D; d = d + 1u) {
        let diff = scratch[off_x() + i * D + d] - mean;
        variance = variance + diff * diff;
    }
    variance = variance / f32(D);
    let scale = 1.0 / sqrt(variance + 1e-5);
    for (var d: u32 = 0u; d < D; d = d + 1u) {
        scratch[off_att() + i * D + d] = (scratch[off_x() + i * D + d] - mean) * scale
            * params[OFF_LNF + d] + params[OFF_LNF + 64u + d];
    }
}

@compute @workgroup_size(64)
fn head(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= n_tokens()) { return; }
    let b = i / u.t;
    let real = lens[b];
    var pooled: array<f32, 64>;
    for (var d: u32 = 0u; d < D; d = d + 1u) {
        var acc = 0.0;
        for (var j: u32 = 0u; j < real; j = j + 1u) {
            acc = acc + scratch[off_att() + (b * u.t + j) * D + d];
        }
        pooled[d] = acc / f32(real);
    }
    var context: array<f32, 64>;
    for (var j: u32 = 0u; j < D; j = j + 1u) {
        var acc = params[OFF_GB + j];
        for (var d: u32 = 0u; d < D; d = d + 1u) {
            acc = acc + pooled[d] * params[OFF_GW + j * D + d];
        }
        context[j] = 1.0 / (1.0 + exp(-acc)) * pooled[j];
    }
    var hidden: array<f32, 64>;
    for (var j: u32 = 0u; j < 64u; j = j + 1u) {
        var acc = params[OFF_HB + j];
        for (var d: u32 = 0u; d < D; d = d + 1u) {
            acc = acc + scratch[off_att() + i * D + d] * params[OFF_HW + d * 64u + j];
        }
        for (var d: u32 = 0u; d < D; d = d + 1u) {
            acc = acc + context[d] * params[OFF_HW + (D + d) * 64u + j];
        }
        hidden[j] = tanh(acc);
    }
    let classes = u32(u.role_classes);
    var best = 0u;
    var best_v = -3.4e38;
    var second_v = -3.4e38;
    for (var c: u32 = 0u; c < classes + 1u; c = c + 1u) {
        var acc = params[OFF_OB + c];
        for (var j: u32 = 0u; j < 64u; j = j + 1u) {
            acc = acc + hidden[j] * params[OFF_OW + j * (classes + 1u) + c];
        }
        scratch[off_out() + i * 41u + c] = acc;
        if (c < classes) {
            if (acc > best_v) {
                second_v = best_v;
                best_v = acc;
                best = c;
            } else if (acc > second_v) {
                second_v = acc;
            }
        }
    }
    if (i % u.t >= real) { return; }
    var denominator = 0.0;
    for (var c: u32 = 0u; c < classes; c = c + 1u) {
        denominator = denominator + exp(scratch[off_out() + i * 41u + c] - best_v);
    }
    let boundary = scratch[off_out() + i * 41u + classes];
    var packed = best;
    if (boundary >= u.boundary_threshold) {
        packed = packed | 0x100u;
    }
    scratch[off_lab() + i] = bitcast<f32>(packed);
    scratch[off_sc() + i] = (1.0 - exp(second_v - best_v)) / denominator;
}
