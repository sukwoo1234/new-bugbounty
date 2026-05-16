mod onnx;

use std::path::Path;

use crate::target::{target_label, TargetKind};

pub(crate) fn run_mutate_pipeline(
    target: &TargetKind,
    input: Option<&Path>,
    out: Option<&Path>,
    input_dir: Option<&Path>,
    out_dir: Option<&Path>,
    count: usize,
    seed: u64,
) -> Result<(), String> {
    if !matches!(target, TargetKind::Onnx) {
        return Err(format!(
            "mutate currently supports only target=onnx; got {}",
            target_label(target)
        ));
    }
    onnx::run(target, input, out, input_dir, out_dir, count, seed)
}
