use crate::dashboard_data::DashboardSnapshot;
use crate::json_utils::{html_escape, json_escape, url_encode};

const DASHBOARD_HTML_TEMPLATE: &str = include_str!("../../templates/dashboard.html");

pub(crate) fn render_dashboard_json(s: &DashboardSnapshot) -> String {
    format!(
        "{{\n  \"schema_version\": \"1.0\",\n  \"generated_at\": {},\n  \"config\": {{\n    \"data_dir\": \"{}\",\n    \"seeds_dir\": \"{}\"\n  }},\n  \"snapshot\": {{\n    \"runs_count\": {},\n    \"triage_count\": {},\n    \"report_count\": {},\n    \"coverage_count\": {},\n    \"latest_run\": \"{}\",\n    \"latest_triage\": \"{}\",\n    \"latest_report\": \"{}\",\n    \"latest_coverage\": \"{}\"\n  }},\n  \"metrics\": {{\n    \"exists\": {},\n    \"successful_runs_per_hour_proxy\": {},\n    \"new_crashes_per_hour\": {},\n    \"valid_crash_ratio\": {},\n    \"valid_crash_ratio_status\": \"{}\",\n    \"valid_crash_ratio_source\": \"{}\",\n    \"valid_crashes\": {},\n    \"total_crashes\": {},\n    \"triage_summary_count\": {},\n    \"global_error_rate_5m\": {}\n  }},\n  \"seeds\": {{\n    \"onnx_count\": {},\n    \"gguf_count\": {},\n    \"safetensors_count\": {},\n    \"total_count\": {}\n  }},\n  \"crash\": {{\n    \"latest_valid_triage\": \"{}\",\n    \"input\": \"{}\",\n    \"signature_top1\": \"{}\",\n    \"summary\": \"{}\",\n    \"report\": \"{}\"\n  }},\n  \"coverage\": {{\n    \"latest\": \"{}\",\n    \"summary\": \"{}\"\n  }}\n}}",
        s.generated_at,
        json_escape(&s.data_dir),
        json_escape(&s.seeds_dir),
        s.runs_count,
        s.triage_count,
        s.report_count,
        s.coverage_count,
        json_escape(&s.latest_run),
        json_escape(&s.latest_triage),
        json_escape(&s.latest_report),
        json_escape(&s.latest_coverage),
        if s.metrics_exists { "true" } else { "false" },
        s.successful_runs_per_hour_proxy,
        s.new_crashes_per_hour,
        valid_crash_ratio_json_literal(s),
        json_escape(&s.valid_crash_ratio_status),
        json_escape(&s.valid_crash_ratio_source),
        s.valid_crashes,
        s.total_crashes,
        s.triage_summary_count,
        s.global_error_rate_5m,
        s.seeds_onnx_count,
        s.seeds_gguf_count,
        s.seeds_safetensors_count,
        s.seeds_total_count,
        json_escape(&s.latest_valid_triage),
        json_escape(&s.latest_valid_input),
        json_escape(&s.latest_valid_signature),
        json_escape(&s.latest_valid_summary),
        json_escape(&s.latest_valid_report),
        json_escape(&s.latest_coverage),
        json_escape(&s.latest_coverage_summary),
    )
}

pub(crate) fn render_dashboard_html(s: &DashboardSnapshot) -> String {
    let metrics_badge = if s.metrics_exists {
        "<span class=\"badge\">available</span>"
    } else {
        "<span class=\"badge\">missing</span>"
    };
    let latest_run_html = id_to_route_link(&s.latest_run, "run");
    let latest_triage_html = id_to_route_link(&s.latest_triage, "triage");
    let latest_report_html = id_to_route_link(&s.latest_report, "report");
    let latest_coverage_html = id_to_route_link(&s.latest_coverage, "coverage");
    let latest_valid_triage_html = id_to_route_link(&s.latest_valid_triage, "triage");
    let latest_valid_summary_html = file_link(&s.latest_valid_summary, &s.latest_valid_summary);
    let latest_valid_report_html = file_link(&s.latest_valid_report, &s.latest_valid_report);
    let latest_coverage_summary_html = file_link(&s.latest_coverage_summary, &s.latest_coverage_summary);
    let recent_triage_rows = render_recent_triage_rows(&s.recent_triage_ids);
    let recent_report_rows = render_recent_report_rows(&s.recent_report_ids);
    let recent_coverage_rows = render_recent_coverage_rows(&s.recent_coverage_ids);

    DASHBOARD_HTML_TEMPLATE
        .replace("{{generated_at}}", &s.generated_at.to_string())
        .replace("{{config_data_dir}}", &html_escape(&s.data_dir))
        .replace("{{config_seeds_dir}}", &html_escape(&s.seeds_dir))
        .replace("{{seeds_onnx_count}}", &s.seeds_onnx_count.to_string())
        .replace("{{seeds_gguf_count}}", &s.seeds_gguf_count.to_string())
        .replace("{{seeds_safetensors_count}}", &s.seeds_safetensors_count.to_string())
        .replace("{{seeds_total_count}}", &s.seeds_total_count.to_string())
        .replace("{{runs_count}}", &s.runs_count.to_string())
        .replace("{{triage_count}}", &s.triage_count.to_string())
        .replace("{{report_count}}", &s.report_count.to_string())
        .replace("{{coverage_count}}", &s.coverage_count.to_string())
        .replace("{{metrics_badge}}", metrics_badge)
        .replace("{{latest_run_html}}", &latest_run_html)
        .replace("{{latest_triage_html}}", &latest_triage_html)
        .replace("{{latest_report_html}}", &latest_report_html)
        .replace("{{latest_coverage_html}}", &latest_coverage_html)
        .replace("{{latest_coverage_summary_html}}", &latest_coverage_summary_html)
        .replace(
            "{{successful_runs_per_hour_proxy}}",
            &html_escape(&s.successful_runs_per_hour_proxy),
        )
        .replace("{{new_crashes_per_hour}}", &html_escape(&s.new_crashes_per_hour))
        .replace("{{valid_crash_ratio}}", &html_escape(&s.valid_crash_ratio))
        .replace(
            "{{valid_crash_ratio_source}}",
            &html_escape(&s.valid_crash_ratio_source),
        )
        .replace("{{valid_crashes}}", &html_escape(&s.valid_crashes))
        .replace("{{total_crashes}}", &html_escape(&s.total_crashes))
        .replace(
            "{{triage_summary_count}}",
            &html_escape(&s.triage_summary_count),
        )
        .replace("{{global_error_rate_5m}}", &html_escape(&s.global_error_rate_5m))
        .replace("{{latest_valid_triage}}", &latest_valid_triage_html)
        .replace("{{latest_valid_input}}", &html_escape(&s.latest_valid_input))
        .replace("{{latest_valid_signature}}", &html_escape(&s.latest_valid_signature))
        .replace("{{latest_valid_summary_html}}", &latest_valid_summary_html)
        .replace("{{latest_valid_report_html}}", &latest_valid_report_html)
        .replace("{{recent_triage_rows}}", &recent_triage_rows)
        .replace("{{recent_report_rows}}", &recent_report_rows)
        .replace("{{recent_coverage_rows}}", &recent_coverage_rows)
}

fn valid_crash_ratio_json_literal(s: &DashboardSnapshot) -> &str {
    if s.valid_crash_ratio_status == "available" {
        &s.valid_crash_ratio
    } else {
        "null"
    }
}

fn id_to_route_link(id_text: &str, route: &str) -> String {
    if id_text == "none" {
        return "none".to_string();
    }
    format!(
        "<a class=\"path-link\" href=\"/{}/{}\">{}</a>",
        html_escape(route),
        url_encode(id_text),
        html_escape(id_text)
    )
}

fn file_link(path: &str, label: &str) -> String {
    if path == "none" {
        return "none".to_string();
    }
    let href = format!("/file?path={}", url_encode(path));
    format!(
        "<a class=\"path-link\" href=\"{}\">{}</a>",
        html_escape(&href),
        html_escape(label)
    )
}

fn render_recent_triage_rows(ids: &[String]) -> String {
    if ids.is_empty() {
        return "<li>none</li>".to_string();
    }
    ids.iter()
        .map(|id| {
            let route = id_to_route_link(id, "triage");
            let summary = format!("./data/triage/{id}/summary.json");
            let summary_link = file_link(&summary, "summary.json");
            format!("<li>{route} <span class=\"sep\">|</span> {summary_link}</li>")
        })
        .collect::<Vec<_>>()
        .join("")
}

fn render_recent_report_rows(ids: &[String]) -> String {
    if ids.is_empty() {
        return "<li>none</li>".to_string();
    }
    ids.iter()
        .map(|id| {
            let route = id_to_route_link(id, "report");
            let report_md = format!("./data/reports/{id}/report.md");
            let report_link = file_link(&report_md, "report.md");
            format!("<li>{route} <span class=\"sep\">|</span> {report_link}</li>")
        })
        .collect::<Vec<_>>()
        .join("")
}

fn render_recent_coverage_rows(ids: &[String]) -> String {
    if ids.is_empty() {
        return "<li>none</li>".to_string();
    }
    ids.iter()
        .map(|id| {
            let route = id_to_route_link(id, "coverage");
            let summary = format!("./data/coverage/{id}/summary.json");
            let summary_link = file_link(&summary, "summary.json");
            format!("<li>{route} <span class=\"sep\">|</span> {summary_link}</li>")
        })
        .collect::<Vec<_>>()
        .join("")
}
