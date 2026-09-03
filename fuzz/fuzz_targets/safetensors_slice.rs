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
            let s = idx_seed[d % idx_seed.len()] as usize;
            // `dim + 1` keeps out-of-range Select indices reachable — that is the
            // surface we are actually testing.
            indexers.push(TensorIndexer::Select(s % (dim + 1)));
        }
        if let Ok(it) = view.sliced_data(&indexers) {
            for _chunk in it {
                // drive SliceIterator::next to completion
            }
        }
    }
});
