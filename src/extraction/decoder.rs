//! File decoding: bytes → one or more `DecodedUnit`s of UTF-8 text.
//!
//! A single source file may expand into multiple indexable units (e.g. future PDF page decoders,
//! spreadsheet sheet decoders, or archive entry decoders). Step 1 ships with only the fallback
//! [`PlainTextDecoder`], which preserves the pre-existing `fs::read_to_string` behavior.

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

static PLAIN_TEXT: PlainTextDecoder = PlainTextDecoder;
static DECODERS: &[&dyn Decoder] = &[&PLAIN_TEXT];

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
}
