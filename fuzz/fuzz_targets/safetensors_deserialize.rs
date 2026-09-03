#![no_main]
//! Front-door target: drive the whole `SafeTensors::deserialize` parser surface, then
//! read each lazily-built `TensorView`'s dtype/shape/data getters. (The `slice` module
//! is a separate surface, exercised by safetensors_slice.rs, not this target.) Every
//! `SafeTensorError` is an EXPECTED rejection and is swallowed; libFuzzer only reports a
//! real abort (panic/OOM/ASan). safetensors is memory-safe, audited Rust, so a clean run
//! finding nothing is the honest, expected outcome.
use libfuzzer_sys::fuzz_target;
use safetensors::tensor::SafeTensors;

fuzz_target!(|data: &[u8]| {
    if let Ok(st) = SafeTensors::deserialize(data) {
        for (_name, view) in st.tensors() {
            let _ = view.dtype();
            let _ = view.shape();
            let _ = view.data();
        }
    }
});
