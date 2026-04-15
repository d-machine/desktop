//! Generic PDF table extraction helpers — pure Rust, zero native dependencies.
//!
//! Uses `pdf-extract` (backed by `lopdf`) which implements the full PDF
//! graphics state machine and calls `output_character` with each glyph's
//! Text Rendering Matrix (TRM).  We accumulate glyphs into spans, cluster
//! spans into rows by Y coordinate, and assign columns by X coordinate.
//!
//! No pdfium / native library required.

use pdf_extract::{MediaBox, OutputDev, OutputError};

// ─── TextSpan ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TextSpan {
    pub text: String,
    pub x: f32,     // left edge (PDF points from page left)
    pub y: f32,     // baseline Y (PDF points from page bottom)
    pub right: f32, // right edge (approximate)
}

// ─── Character collector ─────────────────────────────────────────────────────

/// Collects (x, y, char) tuples from the PDF text stream.
struct CharCollector {
    chars: Vec<(f32, f32, String)>,
}

impl CharCollector {
    fn new() -> Self {
        Self { chars: Vec::new() }
    }
}

impl OutputDev for CharCollector {
    fn begin_page(
        &mut self,
        _page_num: u32,
        _media_box: &MediaBox,
        _art_box: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), OutputError> {
        // Flush chars from previous page — called per page, we process
        // them in extract_page_spans() before this is called again.
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn output_character(
        &mut self,
        trm: &pdf_extract::Transform,
        width: f64,
        _spacing: f64,
        _font_size: f64,
        char: &str,
    ) -> Result<(), OutputError> {
        if char.trim().is_empty() && char != " " {
            return Ok(());
        }
        // trm.m31 = x (points from left), trm.m32 = y (points from bottom)
        self.chars.push((trm.m31 as f32, trm.m32 as f32, char.to_string()));
        let _ = width; // width used for right-edge tracking in span builder
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Extract text spans from every page of a PDF file.
///
/// Returns a `Vec<Vec<TextSpan>>` — one inner `Vec` per page.
pub fn extract_all_page_spans(file_path: &str) -> Result<Vec<Vec<TextSpan>>, String> {
    let doc = lopdf::Document::load(file_path)
        .map_err(|e| format!("Failed to open PDF: {e}"))?;

    // We run pdf-extract page-by-page so we can keep chars-per-page.
    let pages: Vec<u32> = doc.get_pages().keys().copied().collect();
    let mut result: Vec<Vec<TextSpan>> = Vec::new();

    for page_num in pages {
        let mut collector = CharCollector::new();
        pdf_extract::output_doc_page(&doc, &mut collector, page_num)
            .map_err(|e| format!("Page {page_num} extraction failed: {e}"))?;

        let spans = chars_to_spans(collector.chars);
        let rows = group_rows(spans, 5.0);
        // Flatten rows back into spans (callers re-group as needed)
        let flat: Vec<TextSpan> = rows.into_iter().flatten().collect();
        result.push(flat);
    }

    Ok(result)
}

/// Re-group a page's spans into rows, then let callers do column assignment.
///
/// Returns rows sorted top-to-bottom; spans within each row sorted left-to-right.
pub fn page_spans_to_rows(spans: Vec<TextSpan>, y_tolerance: f32) -> Vec<Vec<TextSpan>> {
    group_rows(spans, y_tolerance)
}

/// Assign spans to column buckets defined by `col_x` (sorted left-edge X values).
///
/// Each span is placed in the rightmost column whose boundary is ≤ span.x + 1.
/// Spans within the same column are joined with a space.
pub fn spans_to_cells(row: &[TextSpan], col_x: &[f32]) -> Vec<String> {
    let n = col_x.len();
    let mut cells: Vec<String> = vec![String::new(); n];

    for span in row {
        let col = col_x
            .partition_point(|&cx| cx <= span.x + 1.0)
            .saturating_sub(1)
            .min(n - 1);

        if !cells[col].is_empty() {
            cells[col].push(' ');
        }
        cells[col].push_str(&span.text);
    }

    cells
}

// ─── Internal helpers ────────────────────────────────────────────────────────

/// Accumulate (x, y, char) tuples into word-level spans.
///
/// Characters are joined as long as:
///   - The Y gap is ≤ 2 pt (same line)
///   - The X gap from the previous char's right edge is ≤ 6 pt
fn chars_to_spans(chars: Vec<(f32, f32, String)>) -> Vec<TextSpan> {
    const X_GAP: f32 = 6.0;
    const Y_TOL: f32 = 2.0;
    // Approximate char width: we don't have exact glyph metrics here, so
    // use a heuristic of font_size * 0.5.  We track prev_right manually.
    const CHAR_WIDTH_APPROX: f32 = 6.0; // ~6 pt for typical 10-11pt body text

    let mut spans: Vec<TextSpan> = Vec::new();
    let mut buf = String::new();
    let mut span_x = 0.0f32;
    let mut span_y = 0.0f32;
    let mut span_right = 0.0f32;
    let mut prev_right = f32::NEG_INFINITY;
    let mut prev_y = f32::NAN;

    for (x, y, c) in &chars {
        let x = *x;
        let y = *y;
        let char_right = x + CHAR_WIDTH_APPROX;

        let new_line = !prev_y.is_nan() && (y - prev_y).abs() > Y_TOL;
        let big_gap = !buf.is_empty() && x - prev_right > X_GAP;

        if new_line || big_gap {
            let trimmed = buf.trim().to_string();
            if !trimmed.is_empty() {
                spans.push(TextSpan {
                    text: trimmed,
                    x: span_x,
                    y: span_y,
                    right: span_right,
                });
            }
            buf.clear();
        }

        if buf.is_empty() {
            span_x = x;
            span_y = y;
            span_right = char_right;
        }

        buf.push_str(c);
        if char_right > span_right {
            span_right = char_right;
        }
        if new_line {
            span_y = y;
        }
        prev_right = char_right;
        prev_y = y;
    }

    let trimmed = buf.trim().to_string();
    if !trimmed.is_empty() {
        spans.push(TextSpan {
            text: trimmed,
            x: span_x,
            y: span_y,
            right: span_right,
        });
    }

    spans
}

/// Group spans into rows using a rolling-window Y tolerance.
fn group_rows(mut spans: Vec<TextSpan>, y_tolerance: f32) -> Vec<Vec<TextSpan>> {
    if spans.is_empty() {
        return Vec::new();
    }

    // Sort top-to-bottom (highest PDF-Y first)
    spans.sort_by(|a, b| b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal));

    let mut rows: Vec<Vec<TextSpan>> = Vec::new();
    let mut cur: Vec<TextSpan> = Vec::new();
    let mut last_y = spans[0].y;

    for span in spans {
        if (span.y - last_y).abs() <= y_tolerance {
            last_y = span.y;
            cur.push(span);
        } else {
            if !cur.is_empty() {
                cur.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
                rows.push(cur);
                cur = Vec::new();
            }
            last_y = span.y;
            cur.push(span);
        }
    }

    if !cur.is_empty() {
        cur.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
        rows.push(cur);
    }

    rows
}
