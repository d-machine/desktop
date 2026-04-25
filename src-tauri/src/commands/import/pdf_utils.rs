//! Generic PDF table extraction helpers — pure Rust, zero native dependencies.
//!
//! Uses `pdf-extract` (backed by `lopdf`) which implements the full PDF
//! graphics state machine and calls `output_character` with each glyph's
//! Text Rendering Matrix (TRM).  We accumulate glyphs into spans, cluster
//! spans into rows by Y coordinate, and assign columns by X coordinate.
//!
//! No pdfium / native library required.

use pdf_extract::{MediaBox, OutputDev, OutputError};
use once_cell;

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
// (x, y, text, right_edge) — right_edge = x + advance_in_page_coords
type Char = (f32, f32, String, f32);

struct CharCollector {
    chars: Vec<Char>,
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
        // Skip vertically-rendered text (page stamps, watermarks): m11 ≈ 0
        // means the text is rotated ~90° and its X position carries no
        // left-to-right meaning in reading order.
        if trm.m11.abs() < 0.1 {
            return Ok(());
        }
        let x = trm.m31 as f32;
        let y = trm.m32 as f32;
        // Advance in page coords = glyph advance × horizontal TRM scale.
        // w0 is in normalised text space (0–1 per em); trm.m11 encodes font_size × CTM_scale.
        let advance = (width * trm.m11.abs()) as f32;
        let right_edge = x + advance.max(1.0);
        self.chars.push((x, y, char.to_string(), right_edge));
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
    extract_all_page_spans_pwd(file_path, None)
}

/// Like `extract_all_page_spans` but accepts an optional decryption password.
pub fn extract_all_page_spans_pwd(file_path: &str, password: Option<&str>) -> Result<Vec<Vec<TextSpan>>, String> {
    let doc = lopdf::Document::load(file_path)
        .map_err(|e| format!("Failed to open PDF: {e}"))?;

    if let Some(pwd) = password {
        if doc.is_encrypted() {
            doc.authenticate_raw_password(pwd.as_bytes())
                .map_err(|e| format!("PDF decryption failed (check your PAN): {e}"))?;
        }
    }

    extract_spans_from_doc(&doc)
}

/// Extract spans from an already-loaded lopdf Document.
pub fn extract_spans_from_doc(doc: &lopdf::Document) -> Result<Vec<Vec<TextSpan>>, String> {
    extract_spans_from_doc_with_boundaries(doc, &[])
}

/// Like `extract_spans_from_doc` but forces span breaks at the given X column boundaries.
/// Use this for table PDFs where values in adjacent columns have no inter-column gap.
pub fn extract_spans_from_doc_with_boundaries(
    doc: &lopdf::Document,
    col_boundaries: &[f32],
) -> Result<Vec<Vec<TextSpan>>, String> {
    let pages: Vec<u32> = doc.get_pages().keys().copied().collect();
    let mut result: Vec<Vec<TextSpan>> = Vec::new();

    for page_num in pages {
        let mut collector = CharCollector::new();
        pdf_extract::output_doc_page(doc, &mut collector, page_num)
            .map_err(|e| format!("Page {page_num} extraction failed: {e}"))?;

        let spans = chars_to_spans(collector.chars, col_boundaries);
        let rows = group_rows(spans, 5.0);
        let flat: Vec<TextSpan> = rows.into_iter().flatten().collect();
        result.push(flat);
    }

    Ok(result)
}

/// Extract spans with column boundaries from a file path.
pub fn extract_all_page_spans_with_boundaries(
    file_path: &str,
    col_boundaries: &[f32],
) -> Result<Vec<Vec<TextSpan>>, String> {
    let doc = lopdf::Document::load(file_path)
        .map_err(|e| format!("Failed to open PDF: {e}"))?;
    extract_spans_from_doc_with_boundaries(&doc, col_boundaries)
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

/// Split spans that straddle any of the given X column boundaries.
///
/// In table PDFs the opening parenthesis of a negative value (e.g. `(25,974.81)`)
/// often starts at the exact column boundary — no inter-column gap — so the span
/// builder merges it with the preceding column's text.  Calling this after span
/// extraction forces a break at each boundary.
///
/// The text is split proportionally by character count when we lack per-char X data.
pub fn split_at_x_boundaries(spans: Vec<TextSpan>, boundaries: &[f32]) -> Vec<TextSpan> {
    let mut out = Vec::with_capacity(spans.len() + 4);
    for span in spans {
        let mut cur = span;
        for &bx in boundaries {
            // Only split if boundary falls strictly inside the span's x range.
            if bx <= cur.x || bx >= cur.right || cur.text.is_empty() {
                continue;
            }
            let total_w = cur.right - cur.x;
            let frac    = (bx - cur.x) / total_w;
            // Find the nearest whitespace to the proportional char index, or split hard.
            let n     = cur.text.len();
            let ideal = ((frac * n as f32) as usize).max(1).min(n - 1);
            // Prefer splitting at whitespace within ±3 chars of ideal.
            let split = (ideal.saturating_sub(3)..=(ideal + 3).min(n - 1))
                .find(|&i| cur.text.as_bytes().get(i) == Some(&b' '))
                .unwrap_or(ideal);

            let left_txt  = cur.text[..split].trim_end().to_string();
            let right_txt = cur.text[split..].trim_start().to_string();

            if !left_txt.is_empty() {
                out.push(TextSpan { text: left_txt, x: cur.x, y: cur.y, right: bx });
            }
            cur = TextSpan { text: right_txt, x: bx, y: cur.y, right: cur.right };
        }
        if !cur.text.is_empty() {
            out.push(cur);
        }
    }
    out
}

// ─── Internal helpers ────────────────────────────────────────────────────────

/// Accumulate (x, y, text, right_edge) tuples into word-level spans.
///
/// Two-pass algorithm to avoid stream-order artefacts:
///   Pass 1 — group chars into rows by Y proximity.
///   Pass 2 — within each row, sort left→right, split on X gap OR column boundary.
///
/// `col_boundaries` — sorted list of X values at which to force a span break
/// even when the inter-character gap is less than X_GAP.  Pass an empty slice
/// for generic (boundary-unaware) extraction.
///
/// After building spans, any span whose text begins with a date pattern
/// (DD-Mon-YYYY) immediately followed by non-space is split at the date boundary.
fn chars_to_spans(mut chars: Vec<Char>, col_boundaries: &[f32]) -> Vec<TextSpan> {
    const X_GAP: f32 = 6.0;
    const Y_TOL: f32 = 2.0;

    if chars.is_empty() {
        return Vec::new();
    }

    // Pass 1 — sort by Y descending, then group into rows.
    chars.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut char_rows: Vec<Vec<Char>> = Vec::new();
    let mut cur: Vec<Char> = Vec::new();
    let mut row_y = chars[0].1;

    for ch in chars {
        if (ch.1 - row_y).abs() <= Y_TOL {
            cur.push(ch);
        } else {
            if !cur.is_empty() {
                char_rows.push(std::mem::take(&mut cur));
            }
            row_y = ch.1;
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        char_rows.push(cur);
    }

    // Pass 2 — within each row, sort left→right, build spans on X gap.
    let mut spans: Vec<TextSpan> = Vec::new();

    for row in char_rows {
        let mut row = row;
        row.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut buf = String::new();
        let mut span_x = 0.0f32;
        let mut span_y = 0.0f32;
        let mut span_right = 0.0f32;
        let mut prev_right = f32::NEG_INFINITY;

        macro_rules! flush {
            () => {
                let t = buf.trim().to_string();
                if !t.is_empty() {
                    spans.push(TextSpan { text: t, x: span_x, y: span_y, right: span_right });
                }
                buf.clear();
            };
        }

        for (x, y, c, right_edge) in &row {
            let x = *x;
            let y = *y;

            // Break on X gap OR crossing a column boundary.
            // Column-boundary breaks only fire when the incoming character is a
            // digit or '(' — i.e., a numeric/negative column value is starting.
            // This avoids splitting header words like "Balance" that happen to
            // straddle a column boundary.
            let gap_break = !buf.is_empty() && x - prev_right > X_GAP;
            let new_ch = c.chars().next().unwrap_or(' ');
            let col_break = !buf.is_empty()
                && (new_ch.is_ascii_digit() || new_ch == '(')
                && col_boundaries.iter().any(|&bx| x >= bx && span_x < bx);
            if gap_break || col_break {
                flush!();
            }

            if buf.is_empty() {
                span_x = x;
                span_y = y;
                span_right = *right_edge;
            }
            buf.push_str(c);
            if *right_edge > span_right {
                span_right = *right_edge;
            }
            prev_right = *right_edge;
        }
        flush!();
    }

    // Pass 3 — split any span that starts with "DD-Mon-YYYY" immediately
    // followed by more text (no inter-column gap in the PDF stream).
    split_date_prefixed(spans)
}

/// If a span starts with a date ("DD-Mon-YYYY") and the rest of the text is
/// non-empty, split them into two separate spans.  The date part keeps the
/// original x; the remainder starts at the estimated date right-edge.
fn split_date_prefixed(spans: Vec<TextSpan>) -> Vec<TextSpan> {
    // Matches "DD-Mon-YYYY" at start, rest is anything non-empty.
    let re = once_cell::sync::Lazy::force(&DATE_SPLIT_RE);
    let mut out = Vec::with_capacity(spans.len());
    for span in spans {
        if let Some(cap) = re.captures(&span.text) {
            let date_part = cap[1].to_string();        // "DD-Mon-YYYY"
            let rest_part = cap[2].trim_start().to_string();
            // Estimate date right edge proportionally (date is always 11 chars).
            let total_chars = span.text.len() as f32;
            let date_chars  = date_part.len() as f32;
            let date_right  = span.x + (span.right - span.x) * (date_chars / total_chars);
            out.push(TextSpan { text: date_part, x: span.x,         y: span.y, right: date_right });
            if !rest_part.is_empty() {
                out.push(TextSpan { text: rest_part, x: date_right + 2.0, y: span.y, right: span.right });
            }
        } else {
            out.push(span);
        }
    }
    out
}

static DATE_SPLIT_RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
    regex::Regex::new(r"^(\d{2}-[A-Za-z]{3}-\d{4})(\S.*)").unwrap()
});

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
