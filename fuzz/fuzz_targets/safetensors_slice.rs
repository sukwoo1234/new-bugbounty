#![no_main]
//! Side-door target: the residual panic surface. `deserialize` is audited and returns
//! `Err` on malformed input, but the slicing API (`TensorView::sliced_data` /
//! `SliceIterator::next`) takes attacker-influenced indices — exactly where the 2023
//! Trail-of-Bits audit found a deserialize-then-panic-on-access bug. We deserialize a
//! valid-enough buffer, then drive slicing with indices derived from the fuzz input,
//! deliberately reaching out-of-range Select indices (dim + 1). `Err(InvalidSlice)` is
//! an expected rejection; only a panic/abort is a finding.
use libfuzzer_sys::fuzz_target;
use safetensors::slice::TensorIndexer;
use safetensors::tensor::SafeTensors;

fuzz_target!(|data: &[u8]| {
    // First 4 bytes seed the slice indices; the rest is the safetensors buffer.
    if data.len() < 4 {
        return;
    }
    let (idx_seed, buf) = data.split_at(4);
    let Ok(st) = SafeTensors::deserialize(buf) else {
        return;
    };
    for (_name, view) in st.tensors() {
        let shape = view.shape();
        if shape.is_empty() {
            continue;
        }
        let mut indexers = Vec::with_capacity(shape.len());
        for (d, &dim) in shape.iter().enumerate() {
            // Combine 4 seed bytes into a wide index so out-of-range values are
            // reachable for large dims too (a single byte only reaches dim <= 255).
            let mut s = 0usize;
            for k in 0..4 {
                s = (s << 8) | idx_seed[(d + k) % idx_seed.len()] as usize;
            }
            // saturating_add(1) avoids an overflow/divide-by-zero in the HARNESS when
            // dim == usize::MAX (a shape validate() accepts when the element product is
            // 0). Half the time emit Select(dim) outright: dim is one past the valid
            // range [0, dim), i.e. exactly the out-of-range surface we test.
            let idx = if s & 1 == 0 { dim } else { s % dim.saturating_add(1) };
            indexers.push(TensorIndexer::Select(idx));
        }
        if let Ok(it) = view.sliced_data(&indexers) {
            for _chunk in it {
                // drive SliceIterator::next to completion
            }
        }
    }
});
