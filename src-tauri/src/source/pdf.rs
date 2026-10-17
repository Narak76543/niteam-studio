//! Local PDF access with `lopdf` (`docs/10-document-source.md` §3): page count, page ranges,
//! splitting a range into its own PDF, and the embedded text of each page.

use lopdf::Document;

/// Gemini's PDF limit (50 MB); also our upload limit.
pub const MAX_BYTES: u64 = 50 * 1024 * 1024;
/// Pages one project reads (a page range must fit; 10 §4).
pub const MAX_PAGES: u32 = 150;
/// Pages per Gemini call.
pub const CHUNK_PAGES: u32 = 50;

#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("This file is larger than 50 MB. Compress it, or save only the chapters you need as a new PDF.")]
    TooLarge,
    #[error("This PDF is password-protected. Save a copy without a password and try again.")]
    Encrypted,
    #[error("This file is not a PDF or it is damaged ({0}).")]
    Unreadable(String),
    #[error("This PDF has no pages.")]
    Empty,
    #[error("Choose pages between 1 and {max}, at most {MAX_PAGES} pages.")]
    BadRange { max: u32 },
}

pub struct PdfFile {
    doc: Document,
}

impl PdfFile {
    pub fn load(bytes: &[u8]) -> Result<Self, PdfError> {
        if bytes.len() as u64 > MAX_BYTES {
            return Err(PdfError::TooLarge);
        }
        let doc = Document::load_mem(bytes).map_err(|e| match e {
            lopdf::Error::Decryption(_) => PdfError::Encrypted,
            other => PdfError::Unreadable(other.to_string()),
        })?;
        if doc.is_encrypted() {
            return Err(PdfError::Encrypted);
        }
        if doc.get_pages().is_empty() {
            return Err(PdfError::Empty);
        }
        Ok(Self { doc })
    }

    pub fn page_count(&self) -> u32 {
        u32::try_from(self.doc.get_pages().len()).unwrap_or(u32::MAX)
    }

    /// Embedded text of pages `from..=to` (1-based); a page whose text can't be read is "".
    pub fn page_texts(&self, from: u32, to: u32) -> Vec<String> {
        (from..=to).map(|p| self.doc.extract_text(&[p]).unwrap_or_default()).collect()
    }

    /// A new PDF holding only pages `from..=to`.
    pub fn extract_range(&self, from: u32, to: u32) -> Result<Vec<u8>, PdfError> {
        let mut doc = self.doc.clone();
        let others: Vec<u32> = (1..=self.page_count()).filter(|p| !(from..=to).contains(p)).collect();
        if !others.is_empty() {
            doc.delete_pages(&others);
            doc.prune_objects();
        }
        doc.compress();
        let mut out = Vec::new();
        doc.save_to(&mut out).map_err(|e| PdfError::Unreadable(e.to_string()))?;
        Ok(out)
    }
}

/// Checks a 1-based inclusive page range against the document and [`MAX_PAGES`].
pub fn check_range(from: u32, to: u32, page_count: u32) -> Result<(), PdfError> {
    if from < 1 || to < from || to > page_count || to - from + 1 > MAX_PAGES {
        return Err(PdfError::BadRange { max: page_count });
    }
    Ok(())
}

/// `from..=to` cut into [`CHUNK_PAGES`]-page chunks.
pub fn chunks(from: u32, to: u32) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut start = from;
    while start <= to {
        let end = (start + CHUNK_PAGES - 1).min(to);
        out.push((start, end));
        start = end + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_cut_into_50_page_chunks() {
        assert_eq!(chunks(1, 20), vec![(1, 20)]);
        assert_eq!(chunks(1, 100), vec![(1, 50), (51, 100)]);
        assert_eq!(chunks(40, 141), vec![(40, 89), (90, 139), (140, 141)]);
    }

    #[test]
    fn ranges_are_checked() {
        assert!(check_range(1, 20, 20).is_ok());
        assert!(check_range(40, 189, 300).is_ok());
        assert!(check_range(1, 151, 300).is_err(), "more than 150 pages");
        assert!(check_range(0, 5, 20).is_err());
        assert!(check_range(5, 4, 20).is_err());
        assert!(check_range(1, 21, 20).is_err());
    }

    #[test]
    fn garbage_is_not_a_pdf() {
        assert!(matches!(PdfFile::load(b"hello"), Err(PdfError::Unreadable(_))));
    }
}
