//! Render EQP report payloads as Markdown, single-page HTML, or PDF.
//!
//! The webserver owns data collection. It supplies `input_data` containing a
//! single EQP report or a bulk index report. This compute module owns only the
//! deterministic rendering and file creation step.

use chrono::Utc;
use printpdf::{
    BuiltinFont, Mm, Op, PdfDocument, PdfFontHandle, PdfPage, PdfSaveOptions, Point, Pt, TextItem,
};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

const PDF_LINES_PER_PAGE: usize = 48;
const PDF_CHARACTERS_PER_LINE: usize = 92;

#[derive(Debug, Error)]
pub enum ConversionError {
    #[error("output filename must be a non-empty filename, not a path")]
    InvalidOutputFilename,

    #[error("could not write converted file: {0}")]
    Io(#[from] std::io::Error),
}

/// Convert an EQP report payload to Markdown.
///
/// `output_filepath` is the takeout root. The generated file is written to
/// `output_filepath/MD/output_filename_timestamp(UTC).md`.
pub fn convert_to_markdown(
    input_data: &str,
    output_filename: &str,
    output_filepath: impl AsRef<Path>,
) -> Result<PathBuf, ConversionError> {
    let output_path = prepare_output_path(output_filename, output_filepath.as_ref(), "MD", "md")?;
    fs::write(&output_path, render_markdown(input_data))?;
    Ok(output_path)
}

/// Convert an EQP report payload to a standalone HTML document.
pub fn convert_to_html(
    input_data: &str,
    output_filename: &str,
    output_filepath: impl AsRef<Path>,
) -> Result<PathBuf, ConversionError> {
    let output_path =
        prepare_output_path(output_filename, output_filepath.as_ref(), "HTML", "html")?;
    fs::write(&output_path, render_html(input_data))?;
    Ok(output_path)
}

/// Convert an EQP report payload to a basic searchable PDF.
pub fn convert_to_pdf(
    input_data: &str,
    output_filename: &str,
    output_filepath: impl AsRef<Path>,
) -> Result<PathBuf, ConversionError> {
    let output_path =
        prepare_output_path(output_filename, output_filepath.as_ref(), "PDF", "pdf")?;
    let text = render_plain_text(input_data);
    let lines = wrap_lines(&text, PDF_CHARACTERS_PER_LINE);
    let mut document = PdfDocument::new("EDMS EQP report");
    let mut pages = Vec::new();

    let page_chunks: Vec<&[String]> = if lines.is_empty() {
        vec![&[]]
    } else {
        lines.chunks(PDF_LINES_PER_PAGE).collect()
    };

    for page_lines in page_chunks {
        let mut operations = vec![
            Op::StartTextSection,
            Op::SetTextCursor {
                pos: Point::new(Mm(18.0), Mm(279.0)),
            },
            Op::SetFont {
                font: PdfFontHandle::Builtin(BuiltinFont::Courier),
                size: Pt(9.0),
            },
            Op::SetLineHeight { lh: Pt(11.0) },
        ];

        for line in page_lines {
            operations.push(Op::ShowText {
                items: vec![TextItem::Text(line.clone())],
            });
            operations.push(Op::AddLineBreak);
        }
        operations.push(Op::EndTextSection);
        pages.push(PdfPage::new(Mm(210.0), Mm(297.0), operations));
    }

    let bytes = document
        .with_pages(pages)
        .save(&PdfSaveOptions::default(), &mut Vec::new());
    fs::write(&output_path, bytes)?;
    Ok(output_path)
}

fn prepare_output_path(
    output_filename: &str,
    output_filepath: &Path,
    format_directory: &str,
    extension: &str,
) -> Result<PathBuf, ConversionError> {
    let supplied = Path::new(output_filename);
    if output_filename.trim().is_empty()
        || supplied.file_name().is_none()
        || supplied.file_name() != Some(supplied.as_os_str())
    {
        return Err(ConversionError::InvalidOutputFilename);
    }

    let stem = supplied
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .ok_or(ConversionError::InvalidOutputFilename)?;
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let directory = output_filepath.join(format_directory);
    fs::create_dir_all(&directory)?;
    Ok(directory.join(format!("{stem}_{timestamp}.{extension}")))
}

fn render_markdown(input_data: &str) -> String {
    match serde_json::from_str::<Value>(input_data) {
        Ok(value) if is_index_report(&value) => render_index_markdown(&value),
        Ok(value) if report_eid(&value).is_some() => render_eqp_markdown(&value),
        Ok(value) => format!(
            "# EDMS export\n\n```json\n{}\n```\n",
            pretty_json(&value)
        ),
        Err(_) => format!("# EDMS export\n\n{input_data}\n"),
    }
}

fn render_eqp_markdown(value: &Value) -> String {
    let eid = report_eid(value).unwrap_or("Unknown");
    let crud = field_string(value, &["crud", "method"]).unwrap_or("UNCLASSIFIED");
    let endpoint = field_string(value, &["endpoint_string", "endpoint_str", "path"])
        .unwrap_or("Unknown");
    let mut output = format!(
        "# EQP Report: {eid}\n\n## Endpoint\n\n- **CRUD:** {crud}\n- **EID:** {eid}\n- **Endpoint String:** `{endpoint}`\n\n## Tags per EQP\n\n"
    );

    let tags = value.get("tags").and_then(Value::as_array);
    if let Some(tags) = tags.filter(|tags| !tags.is_empty()) {
        for tag in tags {
            output.push_str(&format!("- {}\n", tag.as_str().unwrap_or("Unknown")));
        }
    } else {
        output.push_str("No tags supplied.\n");
    }

    output.push_str("\n## QP data for EID\n\n");
    if let Some(qp_data) = value.get("qp_data").or_else(|| value.get("qps")) {
        output.push_str(&format!("```json\n{}\n```\n", pretty_json(qp_data)));
    } else if let Some(count) = value.get("qpPairs").and_then(Value::as_u64) {
        output.push_str(&format!("- **QP pairs:** {count}\n"));
    } else {
        output.push_str("No QP data supplied.\n");
    }
    output
}

fn render_index_markdown(value: &Value) -> String {
    let mut output = String::from("# EQP Report Index\n\n## Stats\n\n");
    output.push_str(&format!(
        "```json\n{}\n```\n\n## Endpoints\n\n| CRUD | EID | Endpoint String |\n|---|---|---|\n",
        pretty_json(value.get("stats").unwrap_or(&Value::Null))
    ));
    if let Some(endpoints) = value.get("endpoints").and_then(Value::as_array) {
        for endpoint in endpoints {
            let crud = field_string(endpoint, &["crud", "method"]).unwrap_or("UNCLASSIFIED");
            let eid = report_eid(endpoint).unwrap_or("Unknown");
            let endpoint_string =
                field_string(endpoint, &["endpoint_string", "endpoint_str", "path"])
                    .unwrap_or("Unknown");
            output.push_str(&format!(
                "| {} | {} | {} |\n",
                escape_markdown_cell(crud),
                escape_markdown_cell(eid),
                escape_markdown_cell(endpoint_string)
            ));
        }
    }
    output
}

fn render_html(input_data: &str) -> String {
    let content = match serde_json::from_str::<Value>(input_data) {
        Ok(value) if is_index_report(&value) => render_index_html(&value),
        Ok(value) if report_eid(&value).is_some() => render_eqp_html(&value),
        Ok(value) => format!("<h1>EDMS export</h1><pre><code>{}</code></pre>", escape_html(&pretty_json(&value))),
        Err(_) => format!("<h1>EDMS export</h1><pre><code>{}</code></pre>", escape_html(input_data)),
    };
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n  <title>EDMS EQP report</title>\n  <style>body{{font:16px/1.5 system-ui,sans-serif;max-width:960px;margin:2rem auto;padding:0 1rem}}table{{border-collapse:collapse;width:100%}}th,td{{border:1px solid #bbb;padding:.5rem;text-align:left}}pre{{background:#f5f5f5;padding:1rem;overflow:auto}}</style>\n</head>\n<body>\n{content}\n</body>\n</html>\n"
    )
}

fn render_eqp_html(value: &Value) -> String {
    let eid = escape_html(report_eid(value).unwrap_or("Unknown"));
    let crud = escape_html(field_string(value, &["crud", "method"]).unwrap_or("UNCLASSIFIED"));
    let endpoint = escape_html(
        field_string(value, &["endpoint_string", "endpoint_str", "path"]).unwrap_or("Unknown"),
    );
    let mut output = format!(
        "<h1>EQP Report: {eid}</h1><h2>Endpoint</h2><dl><dt>CRUD</dt><dd>{crud}</dd><dt>EID</dt><dd>{eid}</dd><dt>Endpoint String</dt><dd><code>{endpoint}</code></dd></dl><h2>Tags per EQP</h2>"
    );
    if let Some(tags) = value.get("tags").and_then(Value::as_array).filter(|tags| !tags.is_empty()) {
        output.push_str("<ul>");
        for tag in tags {
            output.push_str(&format!("<li>{}</li>", escape_html(tag.as_str().unwrap_or("Unknown"))));
        }
        output.push_str("</ul>");
    } else {
        output.push_str("<p>No tags supplied.</p>");
    }
    output.push_str("<h2>QP data for EID</h2>");
    if let Some(qp_data) = value.get("qp_data").or_else(|| value.get("qps")) {
        output.push_str(&format!("<pre><code>{}</code></pre>", escape_html(&pretty_json(qp_data))));
    } else if let Some(count) = value.get("qpPairs").and_then(Value::as_u64) {
        output.push_str(&format!("<p>QP pairs: {count}</p>"));
    } else {
        output.push_str("<p>No QP data supplied.</p>");
    }
    output
}

fn render_index_html(value: &Value) -> String {
    let stats = escape_html(&pretty_json(value.get("stats").unwrap_or(&Value::Null)));
    let mut output = format!(
        "<h1>EQP Report Index</h1><h2>Stats</h2><pre><code>{stats}</code></pre><h2>Endpoints</h2><table><thead><tr><th>CRUD</th><th>EID</th><th>Endpoint String</th></tr></thead><tbody>"
    );
    if let Some(endpoints) = value.get("endpoints").and_then(Value::as_array) {
        for endpoint in endpoints {
            let crud = escape_html(field_string(endpoint, &["crud", "method"]).unwrap_or("UNCLASSIFIED"));
            let eid = escape_html(report_eid(endpoint).unwrap_or("Unknown"));
            let endpoint_string = escape_html(
                field_string(endpoint, &["endpoint_string", "endpoint_str", "path"])
                    .unwrap_or("Unknown"),
            );
            output.push_str(&format!("<tr><td>{crud}</td><td>{eid}</td><td><code>{endpoint_string}</code></td></tr>"));
        }
    }
    output.push_str("</tbody></table>");
    output
}

fn render_plain_text(input_data: &str) -> String {
    match serde_json::from_str::<Value>(input_data) {
        Ok(value) if is_index_report(&value) => {
            let mut output = format!(
                "EQP REPORT INDEX\n\nSTATS\n{}\n\nCRUD | EID | ENDPOINT STRING\n",
                pretty_json(value.get("stats").unwrap_or(&Value::Null))
            );
            if let Some(endpoints) = value.get("endpoints").and_then(Value::as_array) {
                for endpoint in endpoints {
                    output.push_str(&format!(
                        "{} | {} | {}\n",
                        field_string(endpoint, &["crud", "method"]).unwrap_or("UNCLASSIFIED"),
                        report_eid(endpoint).unwrap_or("Unknown"),
                        field_string(endpoint, &["endpoint_string", "endpoint_str", "path"])
                            .unwrap_or("Unknown")
                    ));
                }
            }
            output
        }
        Ok(value) if report_eid(&value).is_some() => {
            let eid = report_eid(&value).unwrap_or("Unknown");
            let crud = field_string(&value, &["crud", "method"]).unwrap_or("UNCLASSIFIED");
            let endpoint = field_string(&value, &["endpoint_string", "endpoint_str", "path"])
                .unwrap_or("Unknown");
            let tags = value
                .get("tags")
                .and_then(Value::as_array)
                .map(|tags| tags.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
                .filter(|tags| !tags.is_empty())
                .unwrap_or_else(|| "None supplied".to_string());
            let qp = value
                .get("qp_data")
                .or_else(|| value.get("qps"))
                .map(pretty_json)
                .or_else(|| value.get("qpPairs").map(|count| format!("QP pairs: {count}")))
                .unwrap_or_else(|| "No QP data supplied.".to_string());
            format!(
                "EQP REPORT: {eid}\n\nCRUD: {crud}\nEID: {eid}\nEndpoint String: {endpoint}\n\nTags per EQP\n{tags}\n\nQP data for EID\n{qp}\n"
            )
        }
        Ok(value) => format!("EDMS EXPORT\n\n{}\n", pretty_json(&value)),
        Err(_) => format!("EDMS EXPORT\n\n{input_data}\n"),
    }
}

fn is_index_report(value: &Value) -> bool {
    value.get("stats").is_some() && value.get("endpoints").and_then(Value::as_array).is_some()
}

fn report_eid(value: &Value) -> Option<&str> {
    field_string(value, &["eid", "endpoint_id"])
}

fn field_string<'a>(value: &'a Value, names: &[&str]) -> Option<&'a str> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_str))
}

fn pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn escape_markdown_cell(input: &str) -> String {
    input.replace('|', "\\|").replace('\n', " ")
}

fn escape_html(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for character in input.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn wrap_lines(input: &str, maximum_characters: usize) -> Vec<String> {
    let mut wrapped = Vec::new();
    for source_line in input.lines() {
        let characters: Vec<char> = source_line.chars().collect();
        if characters.is_empty() {
            wrapped.push(String::new());
            continue;
        }
        for part in characters.chunks(maximum_characters) {
            wrapped.push(part.iter().collect());
        }
    }
    wrapped
}
