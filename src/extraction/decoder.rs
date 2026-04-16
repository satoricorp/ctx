//! File decoding: bytes → one or more `DecodedUnit`s of UTF-8 text.
//!
//! A single source file may expand into multiple indexable units. [`PdfDecoder`] produces one
//! unit per page; [`XlsxDecoder`] produces one unit per worksheet; [`PlainTextDecoder`] is the
//! fallback that passes UTF-8 files through unchanged.

use anyhow::{anyhow, Result};
use calamine::{Data, Range, Reader};
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// One decoded text unit extracted from a source file.
///
/// `virtual_path` is a **relative suffix** appended to the caller's source path so multiple units
/// from one source can be distinguished downstream (chunker, classifier, manifest). An empty path
/// means "this unit represents the entire source; use the source path unchanged."
#[derive(Debug)]
pub struct DecodedUnit {
    pub virtual_path: PathBuf,
    pub text: String,
}

pub trait Decoder: Sync {
    fn can_decode(&self, path: &Path, bytes: &[u8]) -> bool;
    fn decode(&self, path: &Path, bytes: &[u8]) -> Result<Vec<DecodedUnit>>;
}

/// Fallback decoder: interprets the full file as UTF-8 text. Errors on invalid UTF-8.
pub struct PlainTextDecoder;

impl Decoder for PlainTextDecoder {
    fn can_decode(&self, _path: &Path, _bytes: &[u8]) -> bool {
        true
    }

    fn decode(&self, _path: &Path, bytes: &[u8]) -> Result<Vec<DecodedUnit>> {
        let text = String::from_utf8(bytes.to_vec())
            .map_err(|_| anyhow!("failed to decode as utf-8 text"))?;
        Ok(vec![DecodedUnit {
            virtual_path: PathBuf::new(),
            text,
        }])
    }
}

/// Per-page PDF text extraction via [`pdf_extract`]. Produces one [`DecodedUnit`] per page
/// with a stable, zero-padded `page-NNN.txt` virtual path so chunk ordering and drift
/// resolution remain deterministic across re-ingests. Whitespace-only pages are dropped
/// while their 1-based page numbers are preserved in the remaining units' virtual paths.
pub struct PdfDecoder;

impl Decoder for PdfDecoder {
    fn can_decode(&self, path: &Path, bytes: &[u8]) -> bool {
        let ext_is_pdf = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("pdf"))
            .unwrap_or(false);
        ext_is_pdf || bytes.starts_with(b"%PDF-")
    }

    fn decode(&self, path: &Path, bytes: &[u8]) -> Result<Vec<DecodedUnit>> {
        let pages = pdf_extract::extract_text_from_mem_by_pages(bytes)
            .map_err(|err| anyhow!("pdf text extraction failed for {}: {err}", path.display()))?;

        let pad_width = page_pad_width(pages.len());
        Ok(pages
            .into_iter()
            .enumerate()
            .filter(|(_, text)| !text.trim().is_empty())
            .map(|(idx, text)| DecodedUnit {
                virtual_path: PathBuf::from(format!(
                    "page-{:0>width$}.txt",
                    idx + 1,
                    width = pad_width,
                )),
                text,
            })
            .collect())
    }
}

fn page_pad_width(total: usize) -> usize {
    match total {
        0..=9 => 1,
        10..=99 => 2,
        100..=999 => 3,
        _ => 4,
    }
}

/// Per-worksheet spreadsheet decoding via [`calamine`]. Handles xlsx, xlsm, xlsb, xls, and ods.
/// Each non-empty sheet becomes a [`DecodedUnit`] rendered as a minimal markdown table so the
/// existing prose-oriented chunker can split large sheets along row boundaries.
pub struct XlsxDecoder;

const SPREADSHEET_EXTENSIONS: &[&str] = &["xlsx", "xlsm", "xlsb", "xls", "ods"];

impl Decoder for XlsxDecoder {
    fn can_decode(&self, path: &Path, _bytes: &[u8]) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                SPREADSHEET_EXTENSIONS
                    .iter()
                    .any(|known| ext.eq_ignore_ascii_case(known))
            })
            .unwrap_or(false)
    }

    fn decode(&self, path: &Path, bytes: &[u8]) -> Result<Vec<DecodedUnit>> {
        let cursor = Cursor::new(bytes.to_vec());
        let mut workbook = calamine::open_workbook_auto_from_rs(cursor)
            .map_err(|err| anyhow!("spreadsheet open failed for {}: {err}", path.display()))?;

        let names: Vec<String> = workbook.sheet_names().to_vec();
        let pad_width = page_pad_width(names.len());

        let mut units: Vec<DecodedUnit> = Vec::new();
        for (idx, name) in names.iter().enumerate() {
            let range = workbook.worksheet_range(name).map_err(|err| {
                anyhow!(
                    "spreadsheet read failed for {} sheet {}: {err}",
                    path.display(),
                    name
                )
            })?;
            let text = format_sheet_markdown(name, &range);
            if text.trim().is_empty() {
                continue;
            }
            let virtual_path = PathBuf::from(format!(
                "sheet-{:0>width$}-{name}.md",
                idx + 1,
                width = pad_width,
                name = sanitize_sheet_name(name),
            ));
            units.push(DecodedUnit { virtual_path, text });
        }
        Ok(units)
    }
}

fn sanitize_sheet_name(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    out = out.trim_matches('_').to_string();
    if out.is_empty() {
        out.push('_');
    }
    if out.len() > 60 {
        out.truncate(60);
    }
    out
}

fn format_sheet_markdown(name: &str, range: &Range<Data>) -> String {
    let rows: Vec<Vec<String>> = range
        .rows()
        .map(|row| row.iter().map(format_cell).collect())
        .collect();
    let max_cols = rows.iter().map(|row| row.len()).max().unwrap_or(0);
    if max_cols == 0 {
        return String::new();
    }

    let mut out = format!("# Sheet: {name}\n\n");
    let header = &rows[0];
    out.push_str(&render_row(header, max_cols));
    out.push_str(&render_separator(max_cols));
    for row in rows.iter().skip(1) {
        out.push_str(&render_row(row, max_cols));
    }
    out
}

fn format_cell(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        other => other.to_string(),
    }
}

fn render_row(cells: &[String], max_cols: usize) -> String {
    let mut out = String::from("|");
    for i in 0..max_cols {
        let value = cells.get(i).map(String::as_str).unwrap_or("");
        let escaped = value.replace('|', "\\|").replace('\n', " ");
        out.push(' ');
        out.push_str(&escaped);
        out.push_str(" |");
    }
    out.push('\n');
    out
}

fn render_separator(max_cols: usize) -> String {
    let mut out = String::from("|");
    for _ in 0..max_cols {
        out.push_str(" --- |");
    }
    out.push('\n');
    out
}

static PDF: PdfDecoder = PdfDecoder;
static XLSX: XlsxDecoder = XlsxDecoder;
static PLAIN_TEXT: PlainTextDecoder = PlainTextDecoder;
static DECODERS: &[&dyn Decoder] = &[&PDF, &XLSX, &PLAIN_TEXT];

/// Dispatch `(path, bytes)` to the first registered decoder that accepts it.
pub fn decode_file(path: &Path, bytes: &[u8]) -> Result<Vec<DecodedUnit>> {
    for decoder in DECODERS {
        if decoder.can_decode(path, bytes) {
            return decoder.decode(path, bytes);
        }
    }
    Err(anyhow!("no decoder accepted {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_roundtrips_utf8() {
        let units = decode_file(Path::new("note.md"), b"hello\n").expect("decode");
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].text, "hello\n");
        assert!(units[0].virtual_path.as_os_str().is_empty());
    }

    #[test]
    fn plain_text_rejects_invalid_utf8() {
        let err = decode_file(Path::new("bin.dat"), &[0xFFu8, 0xFE, 0xFD])
            .expect_err("non-utf8 must fail");
        assert!(err.to_string().to_lowercase().contains("utf-8"));
    }

    #[test]
    fn pdf_decoder_claims_by_extension() {
        assert!(PdfDecoder.can_decode(Path::new("report.pdf"), b""));
        assert!(PdfDecoder.can_decode(Path::new("REPORT.PDF"), b""));
        assert!(PdfDecoder.can_decode(Path::new("Mixed.Pdf"), b""));
    }

    #[test]
    fn pdf_decoder_claims_by_magic_bytes() {
        assert!(PdfDecoder.can_decode(Path::new("no-extension"), b"%PDF-1.4\n..."));
    }

    #[test]
    fn pdf_decoder_rejects_non_pdf() {
        assert!(!PdfDecoder.can_decode(Path::new("note.md"), b"# hello"));
        assert!(!PdfDecoder.can_decode(Path::new("data.bin"), &[0x00, 0x01, 0x02]));
    }

    #[test]
    fn xlsx_decoder_claims_supported_extensions() {
        for ext in ["xlsx", "XLSX", "xlsm", "xlsb", "xls", "ods"] {
            let path = format!("book.{ext}");
            assert!(
                XlsxDecoder.can_decode(Path::new(&path), b""),
                "should accept {ext}"
            );
        }
    }

    #[test]
    fn xlsx_decoder_rejects_other_extensions() {
        assert!(!XlsxDecoder.can_decode(Path::new("notes.md"), b""));
        assert!(!XlsxDecoder.can_decode(Path::new("report.pdf"), b""));
        assert!(!XlsxDecoder.can_decode(Path::new("no-ext"), b""));
    }

    #[test]
    fn sanitize_sheet_name_replaces_unsafe_chars() {
        assert_eq!(sanitize_sheet_name("Revenue 2025"), "Revenue_2025");
        assert_eq!(sanitize_sheet_name("A/B:C"), "A_B_C");
        assert_eq!(sanitize_sheet_name("___"), "_");
        assert_eq!(sanitize_sheet_name(""), "_");
        assert_eq!(sanitize_sheet_name("plain"), "plain");
    }

    #[test]
    fn render_row_pads_and_escapes() {
        let row = vec!["a".to_string(), "b|c".to_string()];
        let rendered = render_row(&row, 3);
        assert_eq!(rendered, "| a | b\\|c |  |\n");
    }

    #[test]
    fn page_pad_width_scales_with_total() {
        assert_eq!(page_pad_width(0), 1);
        assert_eq!(page_pad_width(1), 1);
        assert_eq!(page_pad_width(9), 1);
        assert_eq!(page_pad_width(10), 2);
        assert_eq!(page_pad_width(99), 2);
        assert_eq!(page_pad_width(100), 3);
        assert_eq!(page_pad_width(999), 3);
        assert_eq!(page_pad_width(1000), 4);
    }
}
