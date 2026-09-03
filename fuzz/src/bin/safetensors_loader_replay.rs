//! Standalone replay the `tool` uses as its native safetensors probe.
//!
//! Contract (src/main.rs:45-47, mirrored from the gguf/onnx replays):
//!   exit 0  = `SafeTensors::deserialize` returned Ok (parser accepted the file)
//!   exit 9  = `Err(SafeTensorError)` — the parser cleanly rejected the input; NOT a
//!             finding, never a reproducer
//!   exit 10 = the harness itself could not run (a file it was handed was unreadable)
//! A real memory-safety fault or panic must ABORT by signal — the build links this with
//! `-C panic=abort` so an unwrap/slice-OOB/overflow panic becomes SIGABRT, which the
//! tool's crash oracle counts. `Err` is never conflated with a crash.
use safetensors::tensor::SafeTensors;
use std::process::ExitCode;

const EXIT_OK: u8 = 0;
const EXIT_REJECTED: u8 = 9;
const EXIT_UNAVAILABLE: u8 = 10;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--selftest") {
        // Same shape as gguf's --selftest so an operator can read the contract off the
        // binary itself.
        println!("target: safetensors/v0.7.0");
        println!("exit_codes: ok=0 rejected=9 unavailable=10 crash=signal");
        return ExitCode::from(EXIT_OK);
    }

    if args.is_empty() {
        eprintln!("usage: safetensors_loader_replay <file...> | --selftest");
        return ExitCode::from(EXIT_UNAVAILABLE);
    }

    let mut worst = EXIT_OK;
    for path in &args {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                // Reading the argv file failed: the harness could not run this input,
                // which says nothing about the input. Unavailable, not a crash.
                eprintln!("cannot read {path}: {e}");
                worst = worst.max(EXIT_UNAVAILABLE);
                continue;
            }
        };
        match SafeTensors::deserialize(&bytes) {
            Ok(st) => {
                // Touch the lazy views so the slice-module construction runs too.
                for (_name, view) in st.tensors() {
                    let _ = view.dtype();
                    let _ = view.shape();
                    let _ = view.data();
                }
            }
            Err(e) => {
                eprintln!("rejected {path}: {e:?}");
                worst = worst.max(EXIT_REJECTED);
            }
        }
    }
    ExitCode::from(worst)
}
