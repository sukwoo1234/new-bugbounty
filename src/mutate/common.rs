use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use crate::{
    json_utils::json_escape,
    target::{target_label, TargetKind},
};

pub(crate) struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    pub(crate) fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0xa076_1d64_78bd_642f,
        }
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        // The raw LCG state is unusable modulo a small number, and both failures were
        // measured, not theorised:
        //   - its low bit strictly alternates, so index(2) returned 1,0,1,0,... for
        //     every seed. A two-way choice was decided by how many draws happened to
        //     precede it, and a two-element table could only ever yield one entry.
        //   - consecutive seeds give first outputs that differ by a constant, so a
        //     batch seeding entry i with seed+i samples on a lattice: over 3000
        //     mutations the gguf kv_insert value type came out chi2=98.6 against
        //     uniform (12 dof), drawing the array type 6 times where 25 were due.
        // Scrambling the state on the way out keeps the stream fully deterministic
        // and reproducible from its seed, and makes every bit of it usable.
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub(crate) fn index(&mut self, len: usize) -> usize {
        if len <= 1 {
            0
        } else {
            (self.next_u64() as usize) % len
        }
    }
}

pub(crate) struct MutationOutput {
    pub(crate) bytes: Vec<u8>,
    pub(crate) operator_params: Vec<(&'static str, String)>,
    pub(crate) parse_preserving: &'static str,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OperatorError {
    NoApplicableField,
}

pub(crate) struct MutationReport {
    pub(crate) strategy: &'static str,
    pub(crate) input_size: usize,
    pub(crate) output_size: usize,
}

pub(crate) struct BatchMutationReport {
    pub(crate) generated: usize,
    pub(crate) requested: usize,
    pub(crate) input_count: usize,
    pub(crate) out_dir: String,
    pub(crate) manifest_path: String,
    /// Seeds that were tried and yielded nothing. A batch prints `generated: N` and
    /// exits 0 whether or not a given seed contributed, so a seed every operator
    /// declines is invisible - which is how a 4.8 MB gguf sat unmutatable behind a
    /// version check for months with nothing anywhere saying so.
    pub(crate) unproductive_inputs: Vec<String>,
}

pub(crate) struct MutationManifestEntry {
    pub(crate) index: usize,
    pub(crate) source_seed: String,
    pub(crate) source_hash: String,
    pub(crate) output_path: String,
    pub(crate) output_hash: String,
    pub(crate) operator: &'static str,
    pub(crate) operator_params: Vec<(&'static str, String)>,
    pub(crate) mutation_level: u32,
    pub(crate) parse_preserving: &'static str,
    pub(crate) validation_status: &'static str,
    pub(crate) seed: u64,
    pub(crate) input_size: usize,
    pub(crate) output_size: usize,
}

pub(crate) fn flip_value_byte(
    out: &mut [u8],
    start: usize,
    end: usize,
    rng: &mut DeterministicRng,
) -> Option<usize> {
    let end = end.min(out.len());
    if start >= end {
        return None;
    }
    let span = end - start;
    let idx = start + rng.index(span);
    let bit = rng.index(8) as u32;
    let mask = 1u8 << bit;
    out[idx] ^= mask;
    Some(idx)
}

pub(crate) fn select_operator(set: &[&'static str], rng: &mut DeterministicRng) -> &'static str {
    set[rng.index(set.len())]
}

pub(crate) fn operator_set<'a>(
    operators: &'a [&'static str],
    default_set: &'a [&'static str],
) -> &'a [&'static str] {
    if operators.is_empty() {
        default_set
    } else {
        operators
    }
}

pub(crate) fn write_output(out: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create output dir '{}': {e}", parent.display()))?;
        }
    }
    fs::write(out, bytes).map_err(|e| format!("failed to write output '{}': {e}", out.display()))
}

pub(crate) fn single_manifest_path(out: &Path) -> PathBuf {
    let file_name = out
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("mutation");
    let parent = out.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!("{}.manifest.json", file_name))
}

pub(crate) fn now_unix_string() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "not_available".to_string())
}

pub(crate) fn tool_commit() -> String {
    use std::process::Command;
    match Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "not_available".to_string(),
    }
}

pub(crate) fn captured_command() -> String {
    std::env::args().collect::<Vec<_>>().join(" ")
}

pub(crate) fn print_mutation_report(
    target: &TargetKind,
    input: &Path,
    out: &Path,
    seed: u64,
    report: &MutationReport,
) {
    println!("[mutate] done");
    println!("target: {}", target_label(target));
    println!("input: {}", input.display());
    println!("out: {}", out.display());
    println!("seed: {seed}");
    println!("strategy: {}", report.strategy);
    println!("input_size: {}", report.input_size);
    println!("output_size: {}", report.output_size);
}

pub(crate) fn print_batch_mutation_report(
    target: &TargetKind,
    input_dir: &Path,
    seed: u64,
    report: &BatchMutationReport,
) {
    println!("[mutate] done");
    println!("mode: batch");
    println!("target: {}", target_label(target));
    println!("input_dir: {}", input_dir.display());
    println!("out_dir: {}", report.out_dir);
    println!("seed: {seed}");
    println!("requested: {}", report.requested);
    println!("generated: {}", report.generated);
    println!("input_count: {}", report.input_count);
    println!("manifest: {}", report.manifest_path);
    for input in &report.unproductive_inputs {
        println!("WARN seed produced no mutants: {input} (every operator tried declined it)");
    }
}

/// The attempted inputs that produced nothing, in a stable order. Inputs that were
/// never reached (count smaller than the seed set) are not in the map and so are not
/// reported - "not tried" is not "not mutatable".
pub(crate) fn unproductive_inputs(attempts: &BTreeMap<PathBuf, usize>) -> Vec<String> {
    attempts
        .iter()
        .filter(|(_, produced)| **produced == 0)
        .map(|(path, _)| path.display().to_string())
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn write_mutation_manifest(
    manifest_path: &Path,
    target: &TargetKind,
    mode: &str,
    source_path: &str,
    source_lineage: &str,
    out_dir_path: &str,
    batch_id: &str,
    requested: usize,
    seed: u64,
    operators_requested: &[&'static str],
    generator: &str,
    entries: &[MutationManifestEntry],
) -> Result<PathBuf, String> {
    let generated = entries.len();
    let total_bytes: usize = entries.iter().map(|e| e.output_size).sum();
    let generated_at = now_unix_string();
    let tool_commit_str = tool_commit();
    let command = captured_command();

    let operators_json = operators_requested
        .iter()
        .map(|op| format!("\"{}\"", json_escape(op)))
        .collect::<Vec<_>>()
        .join(", ");

    let file_hashes_json = entries
        .iter()
        .map(|entry| {
            let file_name = Path::new(&entry.output_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("not_available");
            format!(
                "    \"{}\": \"{}\"",
                json_escape(file_name),
                json_escape(&entry.output_hash)
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");

    let entries_json = entries
        .iter()
        .map(|entry| {
            let params_json = entry
                .operator_params
                .iter()
                .map(|(k, v)| format!("\"{}\": \"{}\"", json_escape(k), json_escape(v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "    {{\"index\": {}, \"source_seed\": \"{}\", \"source_hash\": \"{}\", \"output_path\": \"{}\", \"output_hash\": \"{}\", \"operator\": \"{}\", \"operator_params\": {{{}}}, \"mutation_level\": {}, \"parse_preserving\": \"{}\", \"validation_status\": \"{}\", \"seed\": {}, \"input_size\": {}, \"output_size\": {}}}",
                entry.index,
                json_escape(&entry.source_seed),
                json_escape(&entry.source_hash),
                json_escape(&entry.output_path),
                json_escape(&entry.output_hash),
                json_escape(entry.operator),
                params_json,
                entry.mutation_level,
                json_escape(entry.parse_preserving),
                json_escape(entry.validation_status),
                entry.seed,
                entry.input_size,
                entry.output_size
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");

    let body = format!(
        "{{\n  \"schema_version\": \"2.0\",\n  \"corpus_class\": \"mutated\",\n  \"target\": \"{}\",\n  \"mode\": \"{}\",\n  \"source_path\": \"{}\",\n  \"source_lineage\": \"{}\",\n  \"out_dir\": \"{}\",\n  \"batch_id\": \"{}\",\n  \"requested\": {},\n  \"generated\": {},\n  \"file_count\": {},\n  \"total_bytes\": {},\n  \"seed\": {},\n  \"operators_requested\": [{}],\n  \"generator\": \"{}\",\n  \"generator_version\": \"{}\",\n  \"tool_commit\": \"{}\",\n  \"command\": \"{}\",\n  \"generated_at\": {},\n  \"machine_label\": \"not_available\",\n  \"notes\": \"not_available\",\n  \"validation_status\": \"skipped\",\n  \"file_hashes\": {{\n{}\n  }},\n  \"entries\": [\n{}\n  ]\n}}\n",
        json_escape(target_label(target)),
        json_escape(mode),
        json_escape(source_path),
        json_escape(source_lineage),
        json_escape(out_dir_path),
        json_escape(batch_id),
        requested,
        generated,
        generated,
        total_bytes,
        seed,
        operators_json,
        json_escape(generator),
        env!("CARGO_PKG_VERSION"),
        json_escape(&tool_commit_str),
        json_escape(&command),
        generated_at,
        file_hashes_json,
        entries_json
    );
    fs::write(manifest_path, body).map_err(|e| {
        format!(
            "failed to write mutation manifest '{}': {e}",
            manifest_path.display()
        )
    })?;
    Ok(manifest_path.to_path_buf())
}

#[cfg(test)]
mod rng_tests {
    use super::DeterministicRng;

    // The generator has to stay reproducible: a manifest records a seed, and that seed
    // has to reproduce the same mutation later.
    #[test]
    fn the_same_seed_replays_the_same_stream() {
        let a: Vec<u64> = {
            let mut r = DeterministicRng::new(12345);
            (0..16).map(|_| r.next_u64()).collect()
        };
        let b: Vec<u64> = {
            let mut r = DeterministicRng::new(12345);
            (0..16).map(|_| r.next_u64()).collect()
        };
        assert_eq!(a, b);
        let c: Vec<u64> = {
            let mut r = DeterministicRng::new(12346);
            (0..16).map(|_| r.next_u64()).collect()
        };
        assert_ne!(a, c);
    }

    // Measured defect: the raw LCG's low bit alternates, so index(2) produced
    // 1,0,1,0,... for every seed. A two-way choice was then decided by the parity of
    // the draw count, and a two-element table could only ever return one of its
    // entries - gguf's same-width retype could never pick its second alternative.
    #[test]
    fn a_two_way_choice_is_not_a_parity_counter() {
        for seed in [1u64, 3, 7, 99, 4242] {
            let mut rng = DeterministicRng::new(seed);
            let bits: Vec<usize> = (0..32).map(|_| rng.index(2)).collect();
            let alternating: Vec<usize> = (0..32).map(|i| (i + 1) % 2).collect();
            assert_ne!(bits, alternating, "seed {seed}: index(2) still alternates");
            assert!(
                bits.contains(&0) && bits.contains(&1),
                "seed {seed}: index(2) never produced both values"
            );
        }
    }

    // Measured defect: a batch seeds entry i with seed+i. With the raw LCG the third
    // draw modulo 13 covered 4 of 13 values over a 300-entry batch and came out
    // chi2=98.6 over 3000 - the gguf value type that reaches the array-arity assert
    // was drawn 6 times where 25 were due.
    #[test]
    fn consecutive_seeds_do_not_sample_on_a_lattice() {
        let mut counts = [0usize; 13];
        for s in 0..3000u64 {
            let mut rng = DeterministicRng::new(4242 + s);
            rng.index(9); // operator choice
            rng.index(2); // key choice
            counts[rng.index(13)] += 1;
        }
        let total: usize = counts.iter().sum();
        let expected = total as f64 / 13.0;
        let chi2: f64 = counts
            .iter()
            .map(|&c| {
                let d = c as f64 - expected;
                d * d / expected
            })
            .sum();
        assert!(
            counts.iter().all(|&c| c > 0),
            "a value was never drawn at all: {counts:?}"
        );
        // 32.9 is p=0.001 for 12 degrees of freedom.
        assert!(
            chi2 < 32.9,
            "chi2 {chi2:.1} over 12 dof: the draws are not uniform ({counts:?})"
        );
    }
}
