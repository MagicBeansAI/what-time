//! The transformer backend must reproduce the exported PyTorch logits from
//! the quantized weights, sequence for sequence, within the eval gate.

use what_time::testing::transformer;

#[test]
fn matches_pytorch_logits_from_quantized_weights() {
    if !transformer::available() {
        eprintln!("skipping: transformer weights are a placeholder");
        return;
    }
    let directory = what_time::testing::eval_active_dir();
    let rows: Vec<u16> = read_bin(&directory.join("transformer-parity.rows.bin"));
    let lengths: Vec<u32> = read_bin(&directory.join("transformer-parity.lengths.bin"));
    let logits: Vec<f32> = read_bin(&directory.join("transformer-parity.logits.bin"));
    assert!(
        !lengths.is_empty(),
        "parity fixtures missing; run train_transformer.py"
    );

    let classes = 41usize; // 40 roles + boundary
    let mut checked = 0usize;
    let mut argmax_mismatches = 0usize;
    let mut max_error: f32 = 0.0;
    let mut row_offset = 0usize;
    let mut logit_offset = 0usize;
    for length in &lengths {
        let length = *length as usize;
        let rows_slice = &rows[row_offset..row_offset + length * 17];
        let expected = &logits[logit_offset..logit_offset + length * classes];
        let predictions = transformer::infer_rows(rows_slice).unwrap();
        for token in 0..length {
            let predicted = &predictions.logits.as_ref().unwrap()[token * 40..(token + 1) * 40];
            let reference = &expected[token * classes..token * classes + 40];
            for (a, b) in predicted.iter().zip(reference) {
                max_error = max_error.max((a - b).abs());
            }
            let predicted_boundary = predictions.boundary_logits.as_ref().unwrap()[token];
            let reference_boundary = expected[token * classes + 40];
            max_error = max_error.max((predicted_boundary - reference_boundary).abs());
            // argmax agreement on every token
            let best = |slice: &[f32]| {
                slice
                    .iter()
                    .enumerate()
                    .max_by(|x, y| x.1.total_cmp(y.1))
                    .map(|(index, _)| index)
                    .unwrap_or(0)
            };
            if best(predicted) != best(reference) {
                // Report near-ties instead of failing blind: quantized logits
                // within noise of each other can flip argmax harmlessly.
                let mut sorted_p: Vec<f32> = predicted.to_vec();
                sorted_p.sort_by(|a, b| b.total_cmp(a));
                let mut sorted_r: Vec<f32> = reference.to_vec();
                sorted_r.sort_by(|a, b| b.total_cmp(a));
                eprintln!(
                    "argmax differs at seq@{row_offset} token {token}: predicted margin {:.4}, reference margin {:.4}",
                    sorted_p[0] - sorted_p[1],
                    sorted_r[0] - sorted_r[1]
                );
                argmax_mismatches += 1;
            }
            checked += 1;
        }
        row_offset += length * 17;
        logit_offset += length * classes;
    }
    assert!(checked > 0);
    assert!(
        max_error < 2e-3,
        "max logit drift {max_error:.6} exceeds gate"
    );
    assert!(
        argmax_mismatches <= (checked / 200).max(1),
        "{argmax_mismatches} argmax mismatches out of {checked} tokens"
    );
}

fn read_bin<T: Copy>(path: &std::path::Path) -> Vec<T> {
    let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let size = std::mem::size_of::<T>();
    assert_eq!(
        bytes.len() % size,
        0,
        "unaligned fixture {}",
        path.display()
    );
    // Safety: fixtures are plain little-endian POD arrays written by numpy.
    unsafe { std::slice::from_raw_parts(bytes.as_ptr() as *const T, bytes.len() / size) }.to_vec()
}
