use std::fs;
use std::path::Path;

use crate::dashboard_data::{read_run_status, RunStatusView, TriageVerdictCounts};
use crate::dashboard_discovery::recent_prefixed_dir_names;
use crate::json_utils::extract_json_string_literal;

pub(crate) const CHART_SERIES_LIMIT: usize = 30;

#[derive(Clone, Debug)]
pub(crate) struct RunResultPoint {
    pub(crate) run_id: String,
    pub(crate) success: u64,
    pub(crate) failed: u64,
    pub(crate) timeout: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct ThroughputProxyPoint {
    pub(crate) run_id: String,
    pub(crate) success: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct CrashIntakePoint {
    pub(crate) triage_id: String,
    pub(crate) verdict: String,
}

#[derive(Clone, Debug)]
pub(crate) struct VerdictBreakdownPoint {
    pub(crate) verdict: String,
    pub(crate) count: usize,
}

pub(crate) fn collect_recent_run_statuses(
    runs_root: &Path,
    limit: usize,
) -> Result<Vec<(String, RunStatusView)>, String> {
    if !runs_root.exists() || limit == 0 {
        return Ok(Vec::new());
    }
    let recent_ids = recent_prefixed_dir_names(runs_root, "run-", limit)?;
    let mut rows = Vec::with_capacity(recent_ids.len());
    for id in recent_ids {
        if let Some(view) = read_run_status(runs_root, &id)? {
            rows.push((id, view));
        }
    }
    rows.reverse();
    Ok(rows)
}

pub(crate) fn build_run_result_series(
    statuses: &[(String, RunStatusView)],
) -> Vec<RunResultPoint> {
    statuses
        .iter()
        .map(|(id, view)| RunResultPoint {
            run_id: id.clone(),
            success: view.success.parse::<u64>().unwrap_or(0),
            failed: view.failed.parse::<u64>().unwrap_or(0),
            timeout: view.timeout.parse::<u64>().unwrap_or(0),
        })
        .collect()
}

pub(crate) fn build_throughput_proxy_series(
    statuses: &[(String, RunStatusView)],
) -> Vec<ThroughputProxyPoint> {
    statuses
        .iter()
        .map(|(id, view)| ThroughputProxyPoint {
            run_id: id.clone(),
            success: view.success.parse::<u64>().unwrap_or(0),
        })
        .collect()
}

pub(crate) fn collect_recent_triage_verdicts(
    triage_root: &Path,
    limit: usize,
) -> Result<Vec<(String, String)>, String> {
    if !triage_root.exists() || limit == 0 {
        return Ok(Vec::new());
    }
    let recent_ids = recent_prefixed_dir_names(triage_root, "triage-", limit)?;
    let mut rows = Vec::with_capacity(recent_ids.len());
    for id in recent_ids {
        let summary_path = triage_root.join(&id).join("summary.json");
        if !summary_path.is_file() {
            rows.push((id, "not_available".to_string()));
            continue;
        }
        let body = fs::read_to_string(&summary_path)
            .map_err(|e| format!("failed to read '{}': {e}", summary_path.display()))?;
        let verdict = extract_json_string_literal(&body, "verdict")
            .unwrap_or_else(|| "not_available".to_string());
        rows.push((id, verdict));
    }
    rows.reverse();
    Ok(rows)
}

pub(crate) fn build_crash_intake_series(
    verdicts: &[(String, String)],
) -> Vec<CrashIntakePoint> {
    verdicts
        .iter()
        .map(|(id, verdict)| CrashIntakePoint {
            triage_id: id.clone(),
            verdict: verdict.clone(),
        })
        .collect()
}

pub(crate) fn build_triage_verdict_breakdown(
    counts: &TriageVerdictCounts,
) -> Vec<VerdictBreakdownPoint> {
    vec![
        VerdictBreakdownPoint {
            verdict: "reproduced".to_string(),
            count: counts.reproduced,
        },
        VerdictBreakdownPoint {
            verdict: "manual_review".to_string(),
            count: counts.manual_review,
        },
        VerdictBreakdownPoint {
            verdict: "not_reproduced".to_string(),
            count: counts.not_reproduced,
        },
        VerdictBreakdownPoint {
            verdict: "timeout".to_string(),
            count: counts.timeout,
        },
        VerdictBreakdownPoint {
            verdict: "infra_oom".to_string(),
            count: counts.infra_oom,
        },
        VerdictBreakdownPoint {
            verdict: "flaky".to_string(),
            count: counts.flaky,
        },
        VerdictBreakdownPoint {
            verdict: "other".to_string(),
            count: counts.other,
        },
    ]
}
