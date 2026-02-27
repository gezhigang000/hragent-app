//! File parser dispatch — route to correct Python script based on file type.
//!
//! Detects the file format and generates a Python script that uses
//! pandas/openpyxl to parse the file and output structured JSON.

use std::path::Path;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use super::runner::PythonRunner;

/// Supported file formats for parsing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileFormat {
    Csv,
    Excel,
    Json,
    Parquet,
    Text,
    Unknown,
}

/// Result of parsing a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseResult {
    pub format: FileFormat,
    pub column_names: Vec<String>,
    pub row_count: u64,
    pub sample_data: serde_json::Value,
    pub schema_summary: String,
}

/// Detect the file format from its extension.
pub fn detect_format(file_path: &Path) -> FileFormat {
    match file_path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref() {
        Some("csv") | Some("tsv") => FileFormat::Csv,
        Some("xlsx") | Some("xls") => FileFormat::Excel,
        Some("json") | Some("jsonl") => FileFormat::Json,
        Some("parquet") => FileFormat::Parquet,
        Some("txt") | Some("log") => FileFormat::Text,
        _ => FileFormat::Unknown,
    }
}

/// Parse a file and return structured metadata + sample data.
///
/// Generates a Python script that reads the file using pandas,
/// extracts column names, row count, and first N rows as sample data,
/// then outputs the result as JSON to stdout.
pub async fn parse_file(runner: &PythonRunner, file_path: &Path) -> Result<ParseResult> {
    let format = detect_format(file_path);
    let file_path_str = file_path.to_string_lossy();

    let read_code = match format {
        FileFormat::Csv => format!(
            r#"df = smart_read_csv(r'{}', nrows=10000)"#,
            file_path_str
        ),
        FileFormat::Excel => format!(
            r#"df = pd.read_excel(r'{}', nrows=10000)"#,
            file_path_str
        ),
        FileFormat::Json => format!(
            r#"
try:
    df = pd.read_json(r'{}')
except ValueError:
    df = pd.read_json(r'{}', lines=True)
"#,
            file_path_str, file_path_str
        ),
        FileFormat::Parquet => format!(
            r#"df = pd.read_parquet(r'{}')"#,
            file_path_str
        ),
        FileFormat::Text => {
            return Ok(ParseResult {
                format: FileFormat::Text,
                column_names: vec![],
                row_count: 0,
                sample_data: serde_json::Value::Null,
                schema_summary: "Plain text file".to_string(),
            });
        }
        FileFormat::Unknown => {
            return Err(anyhow!("Unsupported file format: {}", file_path_str));
        }
    };

    let code = format!(
        r#"
import pandas as pd
import json
import sys

def smart_read_csv(path, **kwargs):
    """Read CSV with encoding auto-detection (UTF-8 → GBK → latin-1)."""
    for enc in ['utf-8', 'utf-8-sig', 'gbk', 'gb2312', 'latin-1']:
        try:
            return pd.read_csv(path, encoding=enc, **kwargs)
        except (UnicodeDecodeError, UnicodeError):
            continue
    return pd.read_csv(path, encoding='latin-1', errors='replace', **kwargs)

try:
    {read_code}

    # Gather metadata
    columns = df.columns.tolist()
    dtypes = {{col: str(dtype) for col, dtype in df.dtypes.items()}}
    row_count = len(df)

    # Sample first 5 rows
    sample = df.head(5).to_dict(orient='records')

    # Convert non-serializable types
    for row in sample:
        for key, val in row.items():
            if pd.isna(val):
                row[key] = None
            elif hasattr(val, 'isoformat'):
                row[key] = val.isoformat()

    # Schema summary
    schema_lines = []
    for col in columns:
        schema_lines.append(f"  {{col}}: {{dtypes[col]}}")
    schema_summary = f"{{row_count}} rows, {{len(columns)}} columns:\n" + "\n".join(schema_lines)

    result = {{
        "columnNames": columns,
        "rowCount": row_count,
        "sampleData": sample,
        "schemaSummary": schema_summary,
    }}

    print(json.dumps(result, ensure_ascii=False, default=str))
except Exception as e:
    print(json.dumps({{"error": str(e)}}), file=sys.stderr)
    sys.exit(1)
"#,
        read_code = read_code.trim()
    );

    let exec_result = runner.execute(&code).await?;

    if exec_result.exit_code != 0 {
        return Err(anyhow!(
            "File parsing failed: {}",
            if exec_result.stderr.is_empty() { &exec_result.stdout } else { &exec_result.stderr }
        ));
    }

    // Parse the JSON output
    let output: serde_json::Value = serde_json::from_str(&exec_result.stdout)
        .map_err(|e| anyhow!("Failed to parse Python output: {} (output: {})", e, exec_result.stdout))?;

    if let Some(error) = output.get("error").and_then(|v| v.as_str()) {
        return Err(anyhow!("Parser error: {}", error));
    }

    Ok(ParseResult {
        format,
        column_names: output.get("columnNames")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
        row_count: output.get("rowCount")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        sample_data: output.get("sampleData").cloned().unwrap_or(serde_json::Value::Null),
        schema_summary: output.get("schemaSummary")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_csv() {
        assert_eq!(detect_format(Path::new("data.csv")), FileFormat::Csv);
        assert_eq!(detect_format(Path::new("DATA.CSV")), FileFormat::Csv);
    }

    #[test]
    fn test_detect_excel() {
        assert_eq!(detect_format(Path::new("data.xlsx")), FileFormat::Excel);
        assert_eq!(detect_format(Path::new("data.xls")), FileFormat::Excel);
    }

    #[test]
    fn test_detect_json() {
        assert_eq!(detect_format(Path::new("data.json")), FileFormat::Json);
    }

    #[test]
    fn test_detect_unknown() {
        assert_eq!(detect_format(Path::new("data.zip")), FileFormat::Unknown);
    }

    #[test]
    fn test_parse_result_serialization() {
        let result = ParseResult {
            format: FileFormat::Csv,
            column_names: vec!["name".to_string(), "salary".to_string()],
            row_count: 100,
            sample_data: serde_json::json!([]),
            schema_summary: "100 rows, 2 columns".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"format\":\"csv\""));
        assert!(json.contains("\"rowCount\":100"));
    }
}
