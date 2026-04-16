//! File decoding: bytes → one or more `DecodedUnit`s of UTF-8 text.
//!
//! A single source file may expand into multiple indexable units. [`PdfDecoder`] produces one
//! unit per page; [`XlsxDecoder`] produces one unit per worksheet; [`HtmlDecoder`] strips HTML
//! markup; [`JupyterDecoder`] extracts markdown + source cells from `.ipynb`;
//! [`RtfDecoder`] flattens RTF to plain text; [`DocxDecoder`] extracts body text from Word
//! documents; [`PptxDecoder`] produces one unit per slide; [`PlainTextDecoder`] is the fallback
//! that passes UTF-8 files through unchanged.

use anyhow::{anyhow, Context, Result};
use calamine::{Data, Range, Reader};
use quick_xml::events::Event;
use quick_xml::reader::Reader as XmlReader;
use std::io::{Cursor, Read};
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

/// HTML → plain text via [`html2text`]. Strips tags, inlines links as `text [url]`, and renders
/// list/heading structure as readable prose so downstream chunking sees meaningful content rather
/// than raw markup. Claims both `.html` / `.htm` / `.xhtml` by extension and any byte stream that
/// opens with a `<!DOCTYPE html` / `<html` / `<?xml ... ?>` hint so saved pages without extensions
/// are still cleaned up.
pub struct HtmlDecoder;

const HTML_RENDER_WIDTH: usize = 100;

impl Decoder for HtmlDecoder {
    fn can_decode(&self, path: &Path, bytes: &[u8]) -> bool {
        let ext_matches = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "html" | "htm" | "xhtml"
                )
            })
            .unwrap_or(false);
        if ext_matches {
            return true;
        }
        looks_like_html(bytes)
    }

    fn decode(&self, path: &Path, bytes: &[u8]) -> Result<Vec<DecodedUnit>> {
        let text = html2text::from_read(bytes, HTML_RENDER_WIDTH)
            .map_err(|err| anyhow!("html render failed for {}: {err}", path.display()))?;
        Ok(vec![DecodedUnit {
            virtual_path: PathBuf::new(),
            text,
        }])
    }
}

fn looks_like_html(bytes: &[u8]) -> bool {
    // Skip leading whitespace / BOM before the sniff so saved pages aren't missed.
    let trimmed = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace() && *b != 0xEF && *b != 0xBB && *b != 0xBF)
        .map(|idx| &bytes[idx..])
        .unwrap_or(bytes);
    let prefix_lower: Vec<u8> = trimmed.iter().take(64).map(|b| b.to_ascii_lowercase()).collect();
    prefix_lower.starts_with(b"<!doctype html")
        || prefix_lower.starts_with(b"<html")
        || prefix_lower.starts_with(b"<?xml")
}

/// Jupyter notebook (`.ipynb`) decoder. Extracts the ordered sequence of `markdown`, `code`, and
/// `raw` cells and drops outputs (base64 images, execution counts, and ANSI spew) so retrieval
/// sees what the author wrote, not what the kernel printed. Code cells are fenced with the
/// kernel's declared language (defaults to plaintext) so LLMs can parse them correctly.
pub struct JupyterDecoder;

impl Decoder for JupyterDecoder {
    fn can_decode(&self, path: &Path, _bytes: &[u8]) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("ipynb"))
            .unwrap_or(false)
    }

    fn decode(&self, path: &Path, bytes: &[u8]) -> Result<Vec<DecodedUnit>> {
        let notebook: serde_json::Value = serde_json::from_slice(bytes).map_err(|err| {
            anyhow!("notebook json parse failed for {}: {err}", path.display())
        })?;

        let default_language = notebook
            .pointer("/metadata/kernelspec/language")
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                notebook
                    .pointer("/metadata/language_info/name")
                    .and_then(serde_json::Value::as_str)
            })
            .unwrap_or("")
            .to_string();

        let cells = notebook
            .get("cells")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| anyhow!("notebook {} has no cells array", path.display()))?;

        let mut sections: Vec<String> = Vec::new();
        for cell in cells {
            let kind = cell.get("cell_type").and_then(|v| v.as_str()).unwrap_or("");
            let source = read_notebook_source(cell.get("source"));
            if source.trim().is_empty() {
                continue;
            }
            match kind {
                "markdown" | "raw" => sections.push(source),
                "code" => sections.push(format!("```{default_language}\n{source}\n```")),
                _ => {}
            }
        }

        if sections.is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![DecodedUnit {
            virtual_path: PathBuf::new(),
            text: sections.join("\n\n"),
        }])
    }
}

fn read_notebook_source(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(parts)) => parts
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<String>(),
        _ => String::new(),
    }
}

/// Rich Text Format (`.rtf`) decoder via [`rtf_parser`]. RTF is ASCII-compatible with escape
/// sequences for non-ASCII glyphs, so we interpret bytes as UTF-8 (lossy) and return the
/// flattened plain-text body with formatting stripped.
pub struct RtfDecoder;

impl Decoder for RtfDecoder {
    fn can_decode(&self, path: &Path, bytes: &[u8]) -> bool {
        let ext_matches = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("rtf"))
            .unwrap_or(false);
        ext_matches || bytes.starts_with(br"{\rtf")
    }

    fn decode(&self, _path: &Path, bytes: &[u8]) -> Result<Vec<DecodedUnit>> {
        let rtf = String::from_utf8_lossy(bytes).into_owned();
        let document = rtf_parser::document::parse_rtf(rtf);
        let text = document.get_text();
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![DecodedUnit {
            virtual_path: PathBuf::new(),
            text,
        }])
    }
}

/// Word `.docx` decoder. Reads `word/document.xml` out of the OOXML zip and walks it with
/// [`quick_xml`] to extract body text from `<w:t>` runs while turning `<w:p>` and `<w:br>`
/// into line breaks. Ignores headers, footers, footnotes, comments, and styling metadata.
pub struct DocxDecoder;

impl Decoder for DocxDecoder {
    fn can_decode(&self, path: &Path, _bytes: &[u8]) -> bool {
        matches_extension(path, &["docx", "docm"])
    }

    fn decode(&self, path: &Path, bytes: &[u8]) -> Result<Vec<DecodedUnit>> {
        let xml = read_zip_entry(bytes, "word/document.xml").with_context(|| {
            format!("reading word/document.xml from {}", path.display())
        })?;
        let text = extract_docx_text(&xml)
            .with_context(|| format!("parsing word/document.xml from {}", path.display()))?;
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![DecodedUnit {
            virtual_path: PathBuf::new(),
            text,
        }])
    }
}

fn matches_extension(path: &Path, allowed: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            allowed
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
        .unwrap_or(false)
}

/// Read a single file by exact name out of a zip archive in memory.
fn read_zip_entry(zip_bytes: &[u8], entry_name: &str) -> Result<String> {
    let cursor = Cursor::new(zip_bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|err| anyhow!("zip open failed: {err}"))?;
    read_zip_entry_in(&mut archive, entry_name)
}

fn read_zip_entry_in<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    entry_name: &str,
) -> Result<String> {
    let mut file = archive
        .by_name(entry_name)
        .map_err(|err| anyhow!("zip entry {entry_name} not found: {err}"))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|err| anyhow!("zip entry {entry_name} read failed: {err}"))?;
    Ok(contents)
}

/// Pull text out of a DOCX `word/document.xml`. `<w:p>` paragraphs and `<w:br>` breaks become
/// line breaks; `<w:tab>` becomes a tab; text nodes inside `<w:t>` are concatenated verbatim.
fn extract_docx_text(xml: &str) -> Result<String> {
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut out = String::new();
    let mut in_text = false;
    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|err| anyhow!("docx xml parse error: {err}"))?
        {
            Event::Start(ref e) => {
                if local_name_eq(e.name().as_ref(), b"t") {
                    in_text = true;
                }
            }
            Event::End(ref e) => {
                if local_name_eq(e.name().as_ref(), b"t") {
                    in_text = false;
                } else if local_name_eq(e.name().as_ref(), b"p") {
                    out.push('\n');
                }
            }
            Event::Empty(ref e) => {
                let name = e.name();
                if local_name_eq(name.as_ref(), b"br") {
                    out.push('\n');
                } else if local_name_eq(name.as_ref(), b"tab") {
                    out.push('\t');
                }
            }
            Event::Text(e) if in_text => {
                let decoded = e
                    .xml_content()
                    .map_err(|err| anyhow!("docx text decode: {err}"))?;
                out.push_str(&decoded);
            }
            Event::GeneralRef(e) if in_text => {
                if let Some(resolved) = resolve_general_ref(&e) {
                    out.push_str(&resolved);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

/// Resolve the five predefined XML entities and numeric character references. Unknown named
/// entities are dropped silently (Word never emits custom entities in document.xml).
fn resolve_general_ref(e: &quick_xml::events::BytesRef<'_>) -> Option<String> {
    if let Ok(Some(ch)) = e.resolve_char_ref() {
        return Some(ch.to_string());
    }
    let name = e.decode().ok()?;
    match name.as_ref() {
        "amp" => Some("&".into()),
        "lt" => Some("<".into()),
        "gt" => Some(">".into()),
        "quot" => Some("\"".into()),
        "apos" => Some("'".into()),
        _ => None,
    }
}

/// Match an XML qualified name against an unqualified local name, ignoring any namespace prefix.
fn local_name_eq(qualified: &[u8], local: &[u8]) -> bool {
    let stripped = qualified
        .iter()
        .rposition(|b| *b == b':')
        .map(|i| &qualified[i + 1..])
        .unwrap_or(qualified);
    stripped == local
}

/// PowerPoint `.pptx` decoder. Produces one [`DecodedUnit`] per slide rendered from
/// `ppt/slides/slideN.xml`. Slides are ordered by their filename index (`slide1.xml`,
/// `slide2.xml`, …). This matches the visible order for presentations that have never been
/// reordered through the UI; reordered decks will still surface every slide's content but may
/// present it in creation rather than display order.
pub struct PptxDecoder;

impl Decoder for PptxDecoder {
    fn can_decode(&self, path: &Path, _bytes: &[u8]) -> bool {
        matches_extension(path, &["pptx", "pptm"])
    }

    fn decode(&self, path: &Path, bytes: &[u8]) -> Result<Vec<DecodedUnit>> {
        let cursor = Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor)
            .map_err(|err| anyhow!("pptx zip open failed for {}: {err}", path.display()))?;

        let mut slides: Vec<(u32, String)> = archive
            .file_names()
            .filter_map(|name| parse_slide_index(name).map(|idx| (idx, name.to_string())))
            .collect();
        slides.sort_by_key(|(idx, _)| *idx);

        let pad_width = page_pad_width(slides.len());
        let mut units: Vec<DecodedUnit> = Vec::new();
        for (idx, name) in &slides {
            let xml = read_zip_entry_in(&mut archive, name).with_context(|| {
                format!("reading {} from {}", name, path.display())
            })?;
            let text = extract_docx_text(&xml).with_context(|| {
                format!("parsing {} from {}", name, path.display())
            })?;
            if text.trim().is_empty() {
                continue;
            }
            units.push(DecodedUnit {
                virtual_path: PathBuf::from(format!(
                    "slide-{:0>width$}.md",
                    idx,
                    width = pad_width,
                )),
                text,
            });
        }
        Ok(units)
    }
}

/// Parse `ppt/slides/slideN.xml` → `Some(N)`; anything else returns None.
fn parse_slide_index(entry_name: &str) -> Option<u32> {
    let stem = entry_name.strip_prefix("ppt/slides/slide")?;
    let digits = stem.strip_suffix(".xml")?;
    digits.parse().ok()
}

static PDF: PdfDecoder = PdfDecoder;
static XLSX: XlsxDecoder = XlsxDecoder;
static HTML: HtmlDecoder = HtmlDecoder;
static JUPYTER: JupyterDecoder = JupyterDecoder;
static RTF: RtfDecoder = RtfDecoder;
static DOCX: DocxDecoder = DocxDecoder;
static PPTX: PptxDecoder = PptxDecoder;
static PLAIN_TEXT: PlainTextDecoder = PlainTextDecoder;
static DECODERS: &[&dyn Decoder] = &[
    &PDF,
    &XLSX,
    &HTML,
    &JUPYTER,
    &RTF,
    &DOCX,
    &PPTX,
    &PLAIN_TEXT,
];

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
    fn html_decoder_claims_by_extension() {
        for ext in ["html", "HTM", "xhtml"] {
            let path = format!("page.{ext}");
            assert!(HtmlDecoder.can_decode(Path::new(&path), b""));
        }
    }

    #[test]
    fn html_decoder_claims_by_sniff() {
        assert!(HtmlDecoder.can_decode(Path::new("no-ext"), b"<!DOCTYPE html><html>..."));
        assert!(HtmlDecoder.can_decode(Path::new("no-ext"), b"  \n<html><body>x</body></html>"));
        assert!(HtmlDecoder.can_decode(Path::new("no-ext"), b"<?xml version=\"1.0\"?><html/>"));
        assert!(!HtmlDecoder.can_decode(Path::new("note.md"), b"# heading"));
    }

    #[test]
    fn html_decoder_strips_tags() {
        let units = decode_file(
            Path::new("page.html"),
            b"<html><body><h1>Hi</h1><p>A <b>bold</b> claim.</p></body></html>",
        )
        .expect("decode html");
        assert_eq!(units.len(), 1);
        let text = &units[0].text;
        assert!(text.contains("Hi"), "missing heading: {text:?}");
        assert!(text.contains("bold"), "missing emphasized word: {text:?}");
        assert!(!text.contains("<h1>"), "tags leaked: {text:?}");
    }

    #[test]
    fn jupyter_decoder_keeps_markdown_and_code_drops_outputs() {
        let bytes = br##"{
            "cells": [
                {"cell_type": "markdown", "source": ["# Title\n", "intro para\n"]},
                {"cell_type": "code", "source": "print('hi')\n",
                 "outputs": [{"output_type":"stream","text":"hi\n"}]},
                {"cell_type": "raw", "source": "raw line"},
                {"cell_type": "code", "source": ""}
            ],
            "metadata": {"kernelspec": {"language": "python"}}
        }"##;
        let units = decode_file(Path::new("nb.ipynb"), bytes).expect("decode ipynb");
        assert_eq!(units.len(), 1);
        let text = &units[0].text;
        assert!(text.contains("# Title"), "missing markdown: {text}");
        assert!(text.contains("```python"), "missing code fence: {text}");
        assert!(text.contains("print('hi')"), "missing source: {text}");
        assert!(text.contains("raw line"), "missing raw cell: {text}");
        assert!(
            !text.contains("output_type"),
            "leaked outputs metadata: {text}"
        );
    }

    #[test]
    fn jupyter_decoder_falls_back_when_language_missing() {
        let bytes = br##"{
            "cells": [{"cell_type":"code","source":"x=1\n"}],
            "metadata": {}
        }"##;
        let units = decode_file(Path::new("nb.ipynb"), bytes).expect("decode");
        assert_eq!(units.len(), 1);
        assert!(units[0].text.starts_with("```\n"), "{}", units[0].text);
    }

    #[test]
    fn rtf_decoder_claims_by_extension_and_magic() {
        assert!(RtfDecoder.can_decode(Path::new("note.rtf"), b""));
        assert!(RtfDecoder.can_decode(Path::new("NOTE.RTF"), b""));
        assert!(RtfDecoder.can_decode(Path::new("no-ext"), br"{\rtf1\ansi hi}"));
        assert!(!RtfDecoder.can_decode(Path::new("note.md"), b"# hi"));
    }

    #[test]
    fn rtf_decoder_extracts_plain_text() {
        let rtf = br"{\rtf1\ansi\deff0 {\fonttbl{\f0 Times;}}\b hello\b0  world.}";
        let units = decode_file(Path::new("note.rtf"), rtf).expect("decode rtf");
        assert_eq!(units.len(), 1);
        let text = &units[0].text;
        assert!(text.contains("hello"), "missing bold word: {text:?}");
        assert!(text.contains("world"), "missing plain word: {text:?}");
        assert!(!text.contains("\\b"), "formatting leaked: {text:?}");
    }

    #[test]
    fn docx_decoder_claims_by_extension() {
        assert!(DocxDecoder.can_decode(Path::new("report.docx"), b""));
        assert!(DocxDecoder.can_decode(Path::new("MACRO.DOCM"), b""));
        assert!(!DocxDecoder.can_decode(Path::new("report.doc"), b""));
        assert!(!DocxDecoder.can_decode(Path::new("page.html"), b""));
    }

    #[test]
    fn docx_text_extraction_joins_runs_and_breaks_paragraphs() {
        let xml = r#"<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>Hello </w:t><w:t>world.</w:t></w:r></w:p>
    <w:p><w:r><w:t>Line A</w:t><w:br/><w:t>Line B</w:t></w:r></w:p>
    <w:p><w:r><w:t>Tab</w:t><w:tab/><w:t>sep</w:t></w:r></w:p>
    <w:p><w:r><w:t xml:space="preserve">With &amp; entity</w:t></w:r></w:p>
  </w:body>
</w:document>"#;
        let text = extract_docx_text(xml).expect("parse");
        assert!(text.contains("Hello world."), "runs not joined: {text:?}");
        assert!(text.contains("Line A\nLine B"), "br not line break: {text:?}");
        assert!(text.contains("Tab\tsep"), "tab missing: {text:?}");
        assert!(text.contains("With & entity"), "entity not decoded: {text:?}");
        let paragraphs = text.split('\n').filter(|s| !s.trim().is_empty()).count();
        assert!(paragraphs >= 4, "paragraphs not split: {text:?}");
    }

    #[test]
    fn local_name_eq_ignores_prefix() {
        assert!(local_name_eq(b"w:t", b"t"));
        assert!(local_name_eq(b"t", b"t"));
        assert!(local_name_eq(b"a:tab", b"tab"));
        assert!(!local_name_eq(b"w:t", b"p"));
    }

    #[test]
    fn pptx_decoder_claims_by_extension() {
        assert!(PptxDecoder.can_decode(Path::new("deck.pptx"), b""));
        assert!(PptxDecoder.can_decode(Path::new("deck.PPTM"), b""));
        assert!(!PptxDecoder.can_decode(Path::new("deck.ppt"), b""));
        assert!(!PptxDecoder.can_decode(Path::new("report.docx"), b""));
    }

    #[test]
    fn parse_slide_index_handles_common_names() {
        assert_eq!(parse_slide_index("ppt/slides/slide1.xml"), Some(1));
        assert_eq!(parse_slide_index("ppt/slides/slide42.xml"), Some(42));
        assert_eq!(parse_slide_index("ppt/slides/_rels/slide1.xml.rels"), None);
        assert_eq!(parse_slide_index("ppt/slideLayouts/slideLayout1.xml"), None);
        assert_eq!(parse_slide_index("ppt/slides/slide.xml"), None);
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
