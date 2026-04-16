//! File decoding: bytes → one or more `DecodedUnit`s of UTF-8 text.
//!
//! A single source file may expand into multiple indexable units. [`PdfDecoder`] produces one
//! unit per page; [`PlainTextDecoder`] is the fallback that passes UTF-8 files through unchanged.

use anyhow::{anyhow, Result};
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

static PDF: PdfDecoder = PdfDecoder;
static PLAIN_TEXT: PlainTextDecoder = PlainTextDecoder;
static DECODERS: &[&dyn Decoder] = &[&PDF, &PLAIN_TEXT];

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
