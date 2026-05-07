//! File decoding: bytes → one or more `DecodedUnit`s of UTF-8 text.
//!
//! A single source file may expand into multiple indexable units. [`PdfDecoder`] produces one
//! unit per page; [`XlsxDecoder`] produces one unit per worksheet; [`HtmlDecoder`] strips HTML
//! markup; [`JupyterDecoder`] extracts markdown + source cells from `.ipynb`;
//! [`RtfDecoder`] flattens RTF to plain text; [`DocxDecoder`] extracts body text from Word
//! documents; [`PptxDecoder`] produces one unit per slide; [`EpubDecoder`] produces one unit per
//! spine item; [`PlainTextDecoder`] is the fallback that passes UTF-8 files through unchanged.

use anyhow::{anyhow, Context, Result};
use calamine::{Data, Range, Reader};
use quick_xml::events::Event;
use quick_xml::reader::Reader as XmlReader;
use std::any::Any;
use std::collections::HashMap;
use std::io::{BufRead, Cursor, Read};
use std::panic::AssertUnwindSafe;
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

/// Fallback decoder: passes UTF-8 text through, strips a BOM when present, and falls back to
/// encoding detection for legacy/non-UTF-8 text. Errors only when nothing lands cleanly.
pub struct PlainTextDecoder;

impl Decoder for PlainTextDecoder {
    fn can_decode(&self, _path: &Path, _bytes: &[u8]) -> bool {
        true
    }

    fn decode(&self, _path: &Path, bytes: &[u8]) -> Result<Vec<DecodedUnit>> {
        let text = decode_text_bytes(bytes)?;
        Ok(vec![DecodedUnit {
            virtual_path: PathBuf::new(),
            text,
        }])
    }
}

/// Maximum bytes fed to the encoding sniffer before guessing. Large enough to stabilise the guess
/// on realistic prose/log files while keeping detection O(1) on multi-GB inputs.
const ENCODING_SNIFF_BYTES: usize = 1024 * 1024;

/// Decode an in-memory byte buffer into UTF-8 text. Handles UTF-8/UTF-16 BOMs up front, then
/// tries strict UTF-8, then falls back to [`chardetng`] + [`encoding_rs`] for legacy encodings
/// (Latin-1, Windows-1252, GBK, Shift-JIS, …). Returns an error only when no candidate decodes
/// cleanly, so callers can still warn-and-skip on true binary soup.
pub fn decode_text_bytes(bytes: &[u8]) -> Result<String> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return std::str::from_utf8(rest).map(str::to_string).map_err(|_| {
            anyhow!("failed to decode as utf-8 text (BOM followed by invalid utf-8)")
        });
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_with(encoding_rs::UTF_16LE, rest);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_with(encoding_rs::UTF_16BE, rest);
    }

    if let Ok(s) = std::str::from_utf8(bytes) {
        return Ok(s.to_string());
    }

    let sample_end = bytes.len().min(ENCODING_SNIFF_BYTES);
    let sample = &bytes[..sample_end];
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    detector.feed(sample, sample_end == bytes.len());
    let encoding = detector.guess(None, chardetng::Utf8Detection::Deny);
    if encoding == encoding_rs::UTF_8 {
        return Err(anyhow!("failed to decode as utf-8 text"));
    }
    decode_with(encoding, bytes)
}

fn decode_with(encoding: &'static encoding_rs::Encoding, bytes: &[u8]) -> Result<String> {
    let (text, _, had_errors) = encoding.decode(bytes);
    if had_errors {
        return Err(anyhow!(
            "failed to decode as {} text",
            encoding.name().to_ascii_lowercase()
        ));
    }
    Ok(text.into_owned())
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
        // `pdf-extract` may panic on malformed PDFs (e.g. invalid content streams) instead of
        // returning `Err`. Treat panics like decode failures so indexing can skip and continue.
        let path_display = path.display().to_string();
        let extract = std::panic::catch_unwind(AssertUnwindSafe(|| {
            pdf_extract::extract_text_from_mem_by_pages(bytes)
        }));
        let pages = match extract {
            Ok(Ok(pages)) => pages,
            Ok(Err(err)) => {
                return Err(anyhow!(
                    "pdf text extraction failed for {path_display}: {err}"
                ));
            }
            Err(payload) => {
                return Err(anyhow!(
                    "pdf text extraction failed for {path_display}: {}",
                    panic_payload_string(payload)
                ));
            }
        };

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

fn panic_payload_string(payload: Box<dyn Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    String::from("panic in dependency (no message)")
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
            .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "html" | "htm" | "xhtml"))
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
    let prefix_lower: Vec<u8> = trimmed
        .iter()
        .take(64)
        .map(|b| b.to_ascii_lowercase())
        .collect();
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
        let notebook: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|err| anyhow!("notebook json parse failed for {}: {err}", path.display()))?;

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
        let xml = read_zip_entry(bytes, "word/document.xml")
            .with_context(|| format!("reading word/document.xml from {}", path.display()))?;
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
        .map(|ext| allowed.iter().any(|known| ext.eq_ignore_ascii_case(known)))
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
            let xml = read_zip_entry_in(&mut archive, name)
                .with_context(|| format!("reading {} from {}", name, path.display()))?;
            let text = extract_docx_text(&xml)
                .with_context(|| format!("parsing {} from {}", name, path.display()))?;
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

/// EPUB decoder. Walks the OPF spine in reading order, rendering each chapter's XHTML through the
/// same HTML-to-text path as [`HtmlDecoder`] so downstream chunking sees prose rather than markup.
/// Produces one [`DecodedUnit`] per non-empty spine item with a zero-padded
/// `chapter-NNN.md` virtual path.
pub struct EpubDecoder;

impl Decoder for EpubDecoder {
    fn can_decode(&self, path: &Path, _bytes: &[u8]) -> bool {
        matches_extension(path, &["epub"])
    }

    fn decode(&self, path: &Path, bytes: &[u8]) -> Result<Vec<DecodedUnit>> {
        let cursor = Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor)
            .map_err(|err| anyhow!("epub zip open failed for {}: {err}", path.display()))?;

        let container_xml = read_zip_entry_in(&mut archive, "META-INF/container.xml")
            .with_context(|| format!("reading META-INF/container.xml from {}", path.display()))?;
        let opf_path = find_opf_path(&container_xml)
            .ok_or_else(|| anyhow!("no rootfile in container.xml for {}", path.display()))?;

        let opf_xml = read_zip_entry_in(&mut archive, &opf_path)
            .with_context(|| format!("reading {} from {}", opf_path, path.display()))?;
        let opf = parse_opf(&opf_xml)
            .with_context(|| format!("parsing {} from {}", opf_path, path.display()))?;

        let base_dir = Path::new(&opf_path).parent().map(Path::to_path_buf);
        let pad_width = page_pad_width(opf.spine.len());

        let mut units: Vec<DecodedUnit> = Vec::new();
        for (idx, idref) in opf.spine.iter().enumerate() {
            let Some(href) = opf.manifest.get(idref) else {
                continue;
            };
            let resolved = match &base_dir {
                Some(dir) => dir
                    .join(href)
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/"),
                None => href.clone(),
            };
            let xhtml = read_zip_entry_in(&mut archive, &resolved).with_context(|| {
                format!("reading spine item {} from {}", resolved, path.display())
            })?;
            let text = html2text::from_read(xhtml.as_bytes(), HTML_RENDER_WIDTH)
                .map_err(|err| anyhow!("epub html render for {}: {err}", resolved))?;
            if text.trim().is_empty() {
                continue;
            }
            units.push(DecodedUnit {
                virtual_path: PathBuf::from(format!(
                    "chapter-{:0>width$}.md",
                    idx + 1,
                    width = pad_width,
                )),
                text,
            });
        }
        Ok(units)
    }
}

/// Minimal OPF projection: the ordered spine of idrefs plus the manifest `id → href` lookup.
struct OpfPackage {
    manifest: HashMap<String, String>,
    spine: Vec<String>,
}

/// Pull the `full-path` attribute from the first `<rootfile>` in `META-INF/container.xml`.
fn find_opf_path(container_xml: &str) -> Option<String> {
    let mut reader = XmlReader::from_str(container_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        let event = reader.read_event_into(&mut buf).ok()?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e)
                if local_name_eq(e.name().as_ref(), b"rootfile") =>
            {
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref() == b"full-path" {
                        return Some(String::from_utf8_lossy(&attr.value).into_owned());
                    }
                }
            }
            Event::Eof => return None,
            _ => {}
        }
        buf.clear();
    }
}

/// Walk an OPF package: collect manifest (`<item id="..." href="..."/>`) and spine
/// (`<itemref idref="..."/>` in document order).
fn parse_opf(opf_xml: &str) -> Result<OpfPackage> {
    let mut reader = XmlReader::from_str(opf_xml);
    reader.config_mut().trim_text(true);
    let mut manifest: HashMap<String, String> = HashMap::new();
    let mut spine: Vec<String> = Vec::new();
    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|err| anyhow!("opf parse error: {err}"))?
        {
            Event::Start(ref e) | Event::Empty(ref e) => {
                if local_name_eq(e.name().as_ref(), b"item") {
                    let mut id: Option<String> = None;
                    let mut href: Option<String> = None;
                    for attr in e.attributes().flatten() {
                        match attr.key.as_ref() {
                            b"id" => id = Some(String::from_utf8_lossy(&attr.value).into_owned()),
                            b"href" => {
                                href = Some(String::from_utf8_lossy(&attr.value).into_owned())
                            }
                            _ => {}
                        }
                    }
                    if let (Some(id), Some(href)) = (id, href) {
                        manifest.insert(id, href);
                    }
                } else if local_name_eq(e.name().as_ref(), b"itemref") {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"idref" {
                            spine.push(String::from_utf8_lossy(&attr.value).into_owned());
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(OpfPackage { manifest, spine })
}

static PDF: PdfDecoder = PdfDecoder;
static XLSX: XlsxDecoder = XlsxDecoder;
static HTML: HtmlDecoder = HtmlDecoder;
static JUPYTER: JupyterDecoder = JupyterDecoder;
static RTF: RtfDecoder = RtfDecoder;
static DOCX: DocxDecoder = DocxDecoder;
static PPTX: PptxDecoder = PptxDecoder;
static EPUB: EpubDecoder = EpubDecoder;
static PLAIN_TEXT: PlainTextDecoder = PlainTextDecoder;
static DECODERS: &[&dyn Decoder] = &[
    &PDF,
    &XLSX,
    &HTML,
    &JUPYTER,
    &RTF,
    &DOCX,
    &PPTX,
    &EPUB,
    &PLAIN_TEXT,
];

/// Maximum in-memory payload handed to a binary-format decoder (PDF, XLSX, DOCX, PPTX, EPUB, …).
/// Plain-text content is exempt and uses the streaming path for large inputs.
pub const MAX_BINARY_DECODER_BYTES: u64 = 256 * 1024 * 1024;

/// Files larger than this get routed to the streaming plain-text path instead of slurped whole.
pub const PLAIN_TEXT_STREAM_THRESHOLD: u64 = 8 * 1024 * 1024;

/// Target size for each plain-text streaming unit.
pub const PLAIN_TEXT_UNIT_BYTES: usize = 1024 * 1024;

/// Bytes read from the head of a file to decide whether any binary decoder will claim it.
pub const PEEK_SNIFF_BYTES: usize = 4096;

/// Returns true when one of the registered binary decoders (everything except the plain-text
/// fallback) will accept the file given its path and the first [`PEEK_SNIFF_BYTES`] of content.
pub fn any_binary_decoder_claims(path: &Path, peek: &[u8]) -> bool {
    let binary = &DECODERS[..DECODERS.len() - 1];
    binary.iter().any(|d| d.can_decode(path, peek))
}

/// Streaming plain-text chunker. Reads the underlying `BufRead` incrementally and yields
/// [`DecodedUnit`]s with `chunk-NNNN.txt` virtual paths. It prefers newline-aligned flushes but
/// force-flushes at `unit_bytes` if needed, so single giant lines cannot grow memory unbounded.
pub struct PlainTextUnitStream<R: BufRead> {
    reader: R,
    unit_bytes: usize,
    buffer: Vec<u8>,
    chunk_idx: usize,
    done: bool,
}

impl<R: BufRead> PlainTextUnitStream<R> {
    pub fn new(reader: R) -> Self {
        Self::with_unit_bytes(reader, PLAIN_TEXT_UNIT_BYTES)
    }

    pub fn with_unit_bytes(reader: R, unit_bytes: usize) -> Self {
        Self {
            reader,
            unit_bytes: unit_bytes.max(1),
            buffer: Vec::new(),
            chunk_idx: 0,
            done: false,
        }
    }

    /// Pull the next fully-decoded unit, or `Ok(None)` at EOF.
    pub fn next_unit(&mut self) -> Result<Option<DecodedUnit>> {
        if self.done {
            return Ok(None);
        }
        loop {
            let available = self
                .reader
                .fill_buf()
                .map_err(|err| anyhow!("streaming read failed: {err}"))?;
            if available.is_empty() {
                self.done = true;
                if self.buffer.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(self.flush_buffer()?));
            }

            if let Some(newline_pos) = available.iter().position(|byte| *byte == b'\n') {
                let take = newline_pos + 1;
                self.buffer.extend_from_slice(&available[..take]);
                self.reader.consume(take);
                if self.buffer.len() >= self.unit_bytes {
                    return Ok(Some(self.flush_buffer()?));
                }
                continue;
            }

            if self.buffer.len() >= self.unit_bytes {
                return Ok(Some(self.flush_buffer()?));
            }

            let remaining = self.unit_bytes.saturating_sub(self.buffer.len()).max(1);
            let take = available.len().min(remaining);
            self.buffer.extend_from_slice(&available[..take]);
            self.reader.consume(take);
            if self.buffer.len() >= self.unit_bytes {
                return Ok(Some(self.flush_buffer()?));
            }
        }
    }

    fn flush_buffer(&mut self) -> Result<DecodedUnit> {
        self.chunk_idx += 1;
        let text = decode_text_bytes(&self.buffer)?;
        self.buffer.clear();
        Ok(DecodedUnit {
            virtual_path: PathBuf::from(format!("chunk-{:0>4}.txt", self.chunk_idx)),
            text,
        })
    }
}

/// Dispatch `(path, bytes)` to the first registered decoder that accepts it.
pub fn decode_file(path: &Path, bytes: &[u8]) -> Result<Vec<DecodedUnit>> {
    decode_file_with_cap(path, bytes, MAX_BINARY_DECODER_BYTES)
}

/// Same as [`decode_file`] but with a caller-supplied cap. Exposed for tests so we don't have to
/// allocate hundreds of megabytes to exercise the "too large" branch.
pub fn decode_file_with_cap(
    path: &Path,
    bytes: &[u8],
    binary_cap: u64,
) -> Result<Vec<DecodedUnit>> {
    for (idx, decoder) in DECODERS.iter().enumerate() {
        if decoder.can_decode(path, bytes) {
            let is_plain_text = idx + 1 == DECODERS.len();
            if !is_plain_text && (bytes.len() as u64) > binary_cap {
                return Err(anyhow!(
                    "file exceeds {} byte cap for binary decoders; skipping {}",
                    binary_cap,
                    path.display()
                ));
            }
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
    fn plain_text_strips_utf8_bom() {
        let units = decode_file(Path::new("note.md"), b"\xEF\xBB\xBFhello").expect("decode bom");
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].text, "hello");
    }

    #[test]
    fn plain_text_decodes_utf16_le_with_bom() {
        // "hi" as UTF-16 LE: 0x68 0x00 0x69 0x00, with BOM 0xFF 0xFE.
        let bytes = [0xFFu8, 0xFE, 0x68, 0x00, 0x69, 0x00];
        let units = decode_file(Path::new("x.txt"), &bytes).expect("decode utf16le");
        assert_eq!(units[0].text, "hi");
    }

    #[test]
    fn plain_text_decodes_utf16_be_with_bom() {
        // "hi" as UTF-16 BE: 0x00 0x68 0x00 0x69, with BOM 0xFE 0xFF.
        let bytes = [0xFEu8, 0xFF, 0x00, 0x68, 0x00, 0x69];
        let units = decode_file(Path::new("x.txt"), &bytes).expect("decode utf16be");
        assert_eq!(units[0].text, "hi");
    }

    #[test]
    fn plain_text_decodes_legacy_windows_1252() {
        // "café" in Windows-1252 / Latin-1: é is 0xE9.
        let bytes = b"caf\xE9";
        let units = decode_file(Path::new("legacy.txt"), bytes).expect("decode cp1252");
        assert_eq!(units[0].text, "café");
    }

    #[test]
    fn binary_decoder_rejects_oversize_payload() {
        // PDF magic so PdfDecoder claims the file, then padding past a tiny cap.
        let mut bytes = b"%PDF-1.4\n".to_vec();
        bytes.resize(1024, 0);
        let err = decode_file_with_cap(Path::new("big.pdf"), &bytes, 512)
            .expect_err("should reject oversize binary");
        assert!(
            err.to_string().contains("exceeds"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn plain_text_stream_splits_on_unit_bytes() {
        use std::io::BufReader;
        // Build a 10-line input with each line ~40 bytes, so total ≈ 400 bytes.
        let source: String = (0..10)
            .map(|i| format!("line {i:02} - padding to make the line about 40 bytes long\n"))
            .collect();
        let total_len = source.len();
        let reader = BufReader::new(source.as_bytes());
        let mut stream = PlainTextUnitStream::with_unit_bytes(reader, 100);
        let mut units = Vec::new();
        while let Some(unit) = stream.next_unit().expect("stream") {
            units.push(unit);
        }
        assert!(
            units.len() >= 3,
            "expected multiple chunks, got {}",
            units.len()
        );
        let joined: String = units.iter().map(|u| u.text.as_str()).collect();
        assert_eq!(joined.len(), total_len);
        assert_eq!(joined, source);
        for (idx, unit) in units.iter().enumerate() {
            let expected = PathBuf::from(format!("chunk-{:0>4}.txt", idx + 1));
            assert_eq!(unit.virtual_path, expected);
        }
    }

    #[test]
    fn plain_text_stream_handles_empty_input() {
        use std::io::BufReader;
        let reader = BufReader::new(&b""[..]);
        let mut stream = PlainTextUnitStream::new(reader);
        assert!(stream.next_unit().expect("stream").is_none());
    }

    #[test]
    fn plain_text_stream_force_flushes_newline_free_input() {
        use std::io::BufReader;
        let source = "a".repeat(350_000);
        let reader = BufReader::new(source.as_bytes());
        let mut stream = PlainTextUnitStream::with_unit_bytes(reader, 100_000);
        let mut units = Vec::new();
        while let Some(unit) = stream.next_unit().expect("stream") {
            units.push(unit);
        }
        assert!(
            units.len() >= 4,
            "expected forced chunking without newlines, got {}",
            units.len()
        );
        assert!(
            units.iter().all(|u| u.text.len() <= 100_000),
            "unexpected oversized chunk in force-flush path"
        );
        let joined: String = units.iter().map(|u| u.text.as_str()).collect();
        assert_eq!(joined, source);
    }

    #[test]
    fn any_binary_decoder_claims_respects_extension_and_magic() {
        assert!(any_binary_decoder_claims(Path::new("report.pdf"), b""));
        assert!(any_binary_decoder_claims(Path::new("no-ext"), b"%PDF-1.4"));
        assert!(any_binary_decoder_claims(Path::new("deck.pptx"), b""));
        assert!(!any_binary_decoder_claims(
            Path::new("log.txt"),
            b"plain text"
        ));
        assert!(!any_binary_decoder_claims(
            Path::new("notes.md"),
            b"# heading"
        ));
    }

    #[test]
    fn plain_text_decoder_exempt_from_binary_cap() {
        // Plain text larger than the binary cap still decodes (only binary decoders are capped).
        let bytes = vec![b'a'; 2048];
        let units = decode_file_with_cap(Path::new("big.txt"), &bytes, 512)
            .expect("plain text must not be capped");
        assert_eq!(units[0].text.len(), 2048);
    }

    #[test]
    fn plain_text_rejects_truncated_utf16() {
        // UTF-16 LE BOM followed by a single odd byte: guaranteed to fail decoding.
        let err = decode_file(Path::new("bad.dat"), &[0xFFu8, 0xFE, 0x41])
            .expect_err("odd-length utf16 must fail");
        assert!(err.to_string().to_lowercase().contains("utf-16"));
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
        assert!(
            text.contains("Line A\nLine B"),
            "br not line break: {text:?}"
        );
        assert!(text.contains("Tab\tsep"), "tab missing: {text:?}");
        assert!(
            text.contains("With & entity"),
            "entity not decoded: {text:?}"
        );
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
    fn epub_decoder_claims_by_extension() {
        assert!(EpubDecoder.can_decode(Path::new("book.epub"), b""));
        assert!(EpubDecoder.can_decode(Path::new("BOOK.EPUB"), b""));
        assert!(!EpubDecoder.can_decode(Path::new("book.pdf"), b""));
        assert!(!EpubDecoder.can_decode(Path::new("book.mobi"), b""));
    }

    #[test]
    fn find_opf_path_reads_rootfile() {
        let xml = r#"<?xml version="1.0"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;
        assert_eq!(find_opf_path(xml), Some("OEBPS/content.opf".to_string()));
    }

    #[test]
    fn parse_opf_collects_manifest_and_spine() {
        let xml = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <manifest>
    <item id="intro" href="Text/intro.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch1" href="Text/chapter1.xhtml" media-type="application/xhtml+xml"/>
    <item id="cover-img" href="Images/cover.png" media-type="image/png"/>
  </manifest>
  <spine>
    <itemref idref="intro"/>
    <itemref idref="ch1"/>
  </spine>
</package>"#;
        let pkg = parse_opf(xml).expect("parse");
        assert_eq!(pkg.manifest.len(), 3);
        assert_eq!(
            pkg.manifest.get("ch1").map(String::as_str),
            Some("Text/chapter1.xhtml")
        );
        assert_eq!(pkg.spine, vec!["intro".to_string(), "ch1".to_string()]);
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
