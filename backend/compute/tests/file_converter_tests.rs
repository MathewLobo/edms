use compute::file_converter::{convert_to_html, convert_to_markdown, convert_to_pdf};
use std::fs;
use std::path::Path;

// This fixture mirrors the real E0001-AAA folder and metadata in edms-data.
const SAMPLE_EQP: &str = include_str!("fixtures/E0001-AAA-report.json");

fn assert_timestamped_output(path: &Path, root: &Path, format: &str, extension: &str) {
    let expected_directory = root.join(format);
    assert_eq!(path.parent(), Some(expected_directory.as_path()));
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("UTF-8 output filename");
    assert!(filename.starts_with("E0001-AAA_"));
    assert!(filename.ends_with(extension));
}

#[test]
fn renders_single_eqp_markdown_in_the_takeout_layout() {
    let directory = tempfile::tempdir().expect("create temporary directory");

    let written_path = convert_to_markdown(SAMPLE_EQP, "E0001-AAA", directory.path())
        .expect("convert EQP to Markdown");

    assert_timestamped_output(&written_path, directory.path(), "MD", ".md");
    let markdown = fs::read_to_string(written_path).expect("read Markdown output");
    assert!(markdown.contains("# EQP Report: E0001-AAA"));
    assert!(markdown.contains("**CRUD:** POST"));
    assert!(markdown.contains("**Endpoint String:** `https://dummyjson.com/auth/login`"));
    assert!(markdown.contains("## Tags per EQP"));
    assert!(markdown.contains("- production"));
    assert!(markdown.contains("## QP data for EID"));
    assert!(markdown.contains("\"request_number\": 1"));
    assert!(markdown.contains("\"message\": \"Username and password required\""));
    assert!(markdown.contains("\"status_code\": 400"));
}

#[test]
fn renders_untrusted_text_as_safe_single_page_html() {
    let directory = tempfile::tempdir().expect("create temporary directory");

    let written_path = convert_to_html(
        "<script>alert('EDMS & test')</script>",
        "E0001-AAA",
        directory.path(),
    )
    .expect("convert text to HTML");

    assert_timestamped_output(&written_path, directory.path(), "HTML", ".html");
    let html = fs::read_to_string(written_path).expect("read HTML output");
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("EDMS &amp; test"));
    assert!(!html.contains("<script>"));
}

#[test]
fn creates_a_real_pdf_in_the_takeout_layout() {
    let directory = tempfile::tempdir().expect("create temporary directory");

    let written_path = convert_to_pdf(SAMPLE_EQP, "E0001-AAA", directory.path())
        .expect("convert EQP to PDF");

    assert_timestamped_output(&written_path, directory.path(), "PDF", ".pdf");
    let pdf = fs::read(written_path).expect("read PDF output");
    assert!(pdf.starts_with(b"%PDF-"));
    assert!(pdf.len() > 500);
}

#[test]
fn renders_a_bulk_index_with_stats_and_endpoint_mapping() {
    let directory = tempfile::tempdir().expect("create temporary directory");
    let input = r#"{
        "stats": {"endpoint_count": 2},
        "endpoints": [
            {"method": "GET", "eid": "E0001-BG", "path": "/api/v2/analytics/logout"},
            {"crud": "POST", "eid": "E0002-BG", "endpoint_string": "/api/orders"}
        ]
    }"#;

    let written_path = convert_to_markdown(input, "index", directory.path())
        .expect("convert bulk index to Markdown");
    let markdown = fs::read_to_string(written_path).expect("read bulk index");

    assert!(markdown.contains("# EQP Report Index"));
    assert!(markdown.contains("## Stats"));
    assert!(markdown.contains("| CRUD | EID | Endpoint String |"));
    assert!(markdown.contains("| GET | E0001-BG | /api/v2/analytics/logout |"));
    assert!(markdown.contains("| POST | E0002-BG | /api/orders |"));
}

#[test]
fn rejects_a_filename_that_could_escape_the_takeout_directory() {
    let directory = tempfile::tempdir().expect("create temporary directory");

    let error = convert_to_pdf(SAMPLE_EQP, "../outside", directory.path())
        .expect_err("reject a path-like filename");

    assert!(error.to_string().contains("filename, not a path"));
}
