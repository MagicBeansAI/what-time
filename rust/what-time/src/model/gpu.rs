//! Batched GPU inference for the transformer, via wgpu (Metal/Vulkan/DX12,
//! and WebGPU in the browser). One WGSL kernel set (`kernels.wgsl`) mirrors
//! `transformer.rs` op for op; weights upload once as a single packed f32
//! buffer. Padding tokens in a batch bucket are inert: their rows map to
//! the padding feature, attention keys stop at each window's real length,
//! and pooling divides by it.

use std::sync::OnceLock;

use wgpu::util::DeviceExt as _;

use crate::model::predictions::Predictions;
use crate::model::transformer::{self, TransformerWeights};
use crate::types::RawToken;

const ROWS_PER_TOKEN: usize = 17;
/// Above this many windows, `Backend::Auto` prefers the GPU path.
pub const GPU_BATCH_THRESHOLD: usize = 64;
/// Windows per dispatch, bounding buffer sizes.
const CHUNK: usize = 512;
/// Buckets mirror the trainer's padding lengths.
const BUCKETS: [usize; 3] = [32, 64, 112];

struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    params: wgpu::Buffer,
    uniform_layout: wgpu::BindGroupLayout,
    storage_layout: wgpu::BindGroupLayout,
    layout: wgpu::PipelineLayout,
    embed: wgpu::ComputePipeline,
    qkv: wgpu::ComputePipeline,
    attn: wgpu::ComputePipeline,
    attnout_res: wgpu::ComputePipeline,
    ff: wgpu::ComputePipeline,
    lnf: wgpu::ComputePipeline,
    head: wgpu::ComputePipeline,
}

fn pack_params(model: &TransformerWeights) -> Vec<f32> {
    let mut out = Vec::with_capacity(144_105);
    out.extend_from_slice(&model.embedding);
    out.extend_from_slice(&model.position);
    for block in &model.blocks {
        out.extend_from_slice(&block.ln1.0);
        out.extend_from_slice(&block.ln1.1);
        out.extend_from_slice(&block.qkv.0);
        out.extend_from_slice(&block.qkv.1);
        out.extend_from_slice(&block.attn_out.0);
        out.extend_from_slice(&block.attn_out.1);
        out.extend_from_slice(&block.ln2.0);
        out.extend_from_slice(&block.ln2.1);
        out.extend_from_slice(&block.ff1.0);
        out.extend_from_slice(&block.ff1.1);
        out.extend_from_slice(&block.ff2.0);
        out.extend_from_slice(&block.ff2.1);
    }
    out.extend_from_slice(&model.ln_f.0);
    out.extend_from_slice(&model.ln_f.1);
    out.extend_from_slice(&model.global.0);
    out.extend_from_slice(&model.global.1);
    out.extend_from_slice(&model.head.0);
    out.extend_from_slice(&model.head.1);
    out.extend_from_slice(&model.output.0);
    out.extend_from_slice(&model.output.1);
    debug_assert_eq!(
        out.len(),
        144_105,
        "params layout must match kernels.wgsl offsets"
    );
    out
}

fn context() -> Result<&'static GpuContext, crate::Error> {
    static CONTEXT: OnceLock<Result<GpuContext, String>> = OnceLock::new();
    CONTEXT
        .get_or_init(|| {
            let model = transformer::load().map_err(|error| error.to_string())?;
            let params = pack_params(model);
            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let adapter =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                    apply_limit_buckets: false,
                }))
                .map_err(|error| format!("no GPU adapter: {error}"))?;
            let (device, queue) =
                pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                    .map_err(|error| format!("GPU device unavailable: {error}"))?;
            let module = device.create_shader_module(wgpu::include_wgsl!("kernels.wgsl"));

            let uniform_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("what-time-uniforms"),
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    }],
                });

            let mut binding = 0u32;
            let mut storage = |read: bool| wgpu::BindGroupLayoutEntry {
                binding: {
                    binding += 1;
                    binding - 1
                },
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: read },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            };
            // kernels.wgsl declares: params, rows, lens (read-only) and one
            // read-write scratch buffer holding every intermediate.
            let storage_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("what-time-storage"),
                    entries: &[storage(true), storage(true), storage(true), storage(false)],
                });

            fn pipeline(
                device: &wgpu::Device,
                layout: &wgpu::PipelineLayout,
                module: &wgpu::ShaderModule,
                entry: &str,
            ) -> wgpu::ComputePipeline {
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: Some(layout),
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    cache: None,
                })
            }
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("what-time-layout"),
                bind_group_layouts: &[Some(&uniform_layout), Some(&storage_layout)],
                immediate_size: 0,
            });

            let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("what-time-params"),
                contents: bytemuck::cast_slice(&params),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            });

            let embed = pipeline(&device, &layout, &module, "embed");
            let qkv = pipeline(&device, &layout, &module, "stage_qkv");
            let attn = pipeline(&device, &layout, &module, "attn");
            let attnout_res = pipeline(&device, &layout, &module, "attnout_res");
            let ff = pipeline(&device, &layout, &module, "ff");
            let lnf = pipeline(&device, &layout, &module, "lnf");
            let head = pipeline(&device, &layout, &module, "head");

            Ok(GpuContext {
                device,
                queue,
                params: params_buffer,
                uniform_layout,
                storage_layout,
                layout,
                embed,
                qkv,
                attn,
                attnout_res,
                ff,
                lnf,
                head,
            })
        })
        .as_ref()
        .map_err(Clone::clone)
        .map_err(crate::Error::model_unavailable)
}

/// Mapped rows (17 per token, padding = 324) for one window's tokens.
fn window_rows(tokens: &[RawToken], padded: usize) -> Vec<u32> {
    let map = transformer::feature_map();
    let mut rows = vec![324u32; padded * ROWS_PER_TOKEN];
    for (index, token) in tokens.iter().enumerate() {
        for (feature, row) in crate::tokenizer::feature_rows(token.features)
            .iter()
            .enumerate()
        {
            rows[index * ROWS_PER_TOKEN + feature] = u32::from(map[*row as usize]);
        }
    }
    rows
}

/// Batched inference over many windows. Returns one Predictions per window,
/// in order. Dispatches bucket by padded length in bounded chunks.
pub fn infer_windows(windows: &[Vec<RawToken>]) -> Result<Vec<Predictions>, crate::Error> {
    let gpu = context()?;
    let model = transformer::load().map_err(crate::Error::model_unavailable)?;
    let role_classes = model.role_classes;

    let mut results: Vec<Option<Predictions>> = windows.iter().map(|_| None).collect();
    for &padded in &BUCKETS {
        // indices of windows that fit this bucket (and not a smaller one)
        let indices: Vec<usize> = windows
            .iter()
            .enumerate()
            .filter(|(_, tokens)| {
                let len = tokens.len();
                padded == BUCKETS[0] && len <= padded
                    || padded == BUCKETS[1] && BUCKETS[0] < len && len <= padded
                    || padded == BUCKETS[2] && BUCKETS[1] < len && len <= padded
            })
            .map(|(index, _)| index)
            .collect();
        for chunk in indices.chunks(CHUNK) {
            if chunk.is_empty() {
                continue;
            }
            let b = chunk.len() as u32;
            let t = padded as u32;
            let mut rows = Vec::with_capacity(chunk.len() * padded * ROWS_PER_TOKEN);
            let mut lens = Vec::with_capacity(chunk.len());
            for &index in chunk {
                let tokens = &windows[index];
                rows.extend(window_rows(tokens, padded));
                lens.push(tokens.len() as u32);
            }
            let dispatch = run_bucket(gpu, model, &rows, &lens, b, t, role_classes)?;
            for (slot, &index) in chunk.iter().enumerate() {
                let real = lens[slot] as usize;
                let base = slot * t as usize;
                results[index] = Some(Predictions {
                    labels: dispatch.labels[base..base + real].to_vec(),
                    clause_starts: dispatch.clause_starts[base..base + real].to_vec(),
                    scores: dispatch.scores[base..base + real].to_vec(),
                    logits: None,
                    boundary_logits: None,
                });
            }
        }
    }
    // any window longer than the largest bucket falls back to CPU
    let mut out = Vec::with_capacity(windows.len());
    for (index, tokens) in windows.iter().enumerate() {
        match results[index].take() {
            Some(predictions) => out.push(predictions),
            None => out.push(transformer::infer(tokens)?),
        }
    }
    Ok(out)
}

struct BucketOutput {
    labels: Vec<u8>,
    clause_starts: Vec<u8>,
    scores: Vec<f32>,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::NoUninit)]
struct Uniforms {
    b: u32,
    t: u32,
    block: u32,
    lens_len: u32,
    boundary_threshold: f32,
    role_classes: f32,
}

fn run_bucket(
    gpu: &GpuContext,
    model: &TransformerWeights,
    rows: &[u32],
    lens: &[u32],
    b: u32,
    t: u32,
    role_classes: usize,
) -> Result<BucketOutput, crate::Error> {
    let tokens = b as usize * t as usize;

    let rows_buffer = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("what-time-rows"),
            contents: bytemuck::cast_slice(rows),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let lens_buffer = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("what-time-lens"),
            contents: bytemuck::cast_slice(lens),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let scratch = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("what-time-scratch"),
        size: (tokens * (64 + 192 + 64 + 256 + 41 + 1 + 1) * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let labels_size = (tokens * 4) as u64;
    let scores_size = (tokens * 4) as u64;

    let uniforms = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 24,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("what-time-readback"),
        size: labels_size + scores_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let off_lab_bytes = ((tokens * (64 + 192 + 64 + 256 + 41)) * 4) as u64;
    let off_sc_bytes = off_lab_bytes + labels_size;

    let uniform_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("what-time-uniforms"),
        layout: &gpu.uniform_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniforms.as_entire_binding(),
        }],
    });
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("what-time-batch"),
        layout: &gpu.storage_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: gpu.params.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: rows_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: lens_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: scratch.as_entire_binding(),
            },
        ],
    });

    let write_uniforms = |block: u32| {
        gpu.queue.write_buffer(
            &uniforms,
            0,
            bytemuck::bytes_of(&Uniforms {
                b,
                t,
                block,
                lens_len: lens.len() as u32,
                boundary_threshold: model.boundary_threshold as f32,
                role_classes: role_classes as f32,
            }),
        );
    };

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    let dispatch =
        |pass: &mut wgpu::ComputePass<'_>, pipeline: &wgpu::ComputePipeline, threads: u32| {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &uniform_group, &[]);
            pass.set_bind_group(1, &group, &[]);
            pass.dispatch_workgroups(threads.div_ceil(64), 1, 1);
        };

    let run = |pipelines: &[(&wgpu::ComputePipeline, u32)], block: u32| {
        write_uniforms(block);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            for (pipeline, threads) in pipelines {
                dispatch(&mut pass, pipeline, *threads);
            }
        }
        gpu.queue.submit(Some(encoder.finish()));
    };

    run(&[(&gpu.embed, tokens as u32)], 0);
    for block in 0..model.layers as u32 {
        run(&[(&gpu.qkv, tokens as u32)], block);
        run(&[(&gpu.attn, b * 4 * t)], block);
        run(&[(&gpu.attnout_res, tokens as u32)], block);
        run(&[(&gpu.ff, tokens as u32)], block);
    }
    run(&[(&gpu.lnf, tokens as u32)], 0);
    run(&[(&gpu.head, tokens as u32)], 0);

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    encoder.copy_buffer_to_buffer(&scratch, off_lab_bytes, &staging, 0, labels_size);
    encoder.copy_buffer_to_buffer(&scratch, off_sc_bytes, &staging, labels_size, scores_size);
    gpu.queue.submit(Some(encoder.finish()));

    let (sender, receiver) = std::sync::mpsc::channel();
    staging
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    gpu.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .map_err(|_| crate::Error::model_unavailable("GPU wait failed".into()))?;
    receiver
        .recv()
        .map_err(|_| crate::Error::model_unavailable("GPU readback channel closed".into()))?
        .map_err(|error| crate::Error::model_unavailable(format!("GPU map failed: {error}")))?;
    let data = staging
        .slice(..)
        .get_mapped_range()
        .map_err(|error| crate::Error::model_unavailable(format!("GPU map failed: {error}")))?;
    let labels_raw: Vec<u32> = bytemuck::cast_slice(&data[..labels_size as usize]).to_vec();
    let scores_raw: Vec<f32> = bytemuck::cast_slice(&data[labels_size as usize..]).to_vec();
    drop(data);
    staging.unmap();

    // the kernel packs clause_start into bit 8 of the label word
    let labels_out: Vec<u8> = labels_raw.iter().map(|v| (v & 0xff) as u8).collect();
    let clause_out: Vec<u8> = labels_raw.iter().map(|v| ((v >> 8) & 1) as u8).collect();
    Ok(BucketOutput {
        labels: labels_out,
        clause_starts: clause_out,
        scores: scores_raw,
    })
}

/// Test-only: tokenize texts the way the tagger would (collapsed
/// whitespace, edge flags) so parity tests can compare against windows.
pub fn debug_windows(texts: &[&str]) -> Vec<Vec<crate::types::RawToken>> {
    texts
        .iter()
        .map(|text| {
            let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let mut tokens = crate::tokenizer::tokenize(&collapsed);
            // mirror windows_for: mark the window edges the model expects
            let last = tokens.len().saturating_sub(1);
            for (index, token) in tokens.iter_mut().enumerate() {
                if index == 0 {
                    token.features.1 |= 1 << 10;
                }
                if index == last {
                    token.features.1 |= 1 << 11;
                }
            }
            tokens
        })
        .collect()
}

/// Test-only: run the batched GPU path over prebuilt windows.
pub fn infer_windows_debug(
    windows: &[Vec<crate::types::RawToken>],
) -> Result<Vec<crate::model::predictions::Predictions>, crate::Error> {
    infer_windows(windows)
}
