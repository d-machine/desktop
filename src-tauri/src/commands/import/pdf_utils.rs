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

// ─── Default extraction parameters ───────────────────────────────────────────

/// Default maximum vertical distance (pt) between spans considered part of the
/// same logical row.  Each parser overrides this with its own local `ROW_Y_TOL`.
pub const DEFAULT_ROW_Y_TOL: f32 = 5.0;

// ─── Public API ──────────────────────────────────────────────────────────────

/// Like `extract_all_page_spans` with caller-supplied span-building parameters.
/// Use this when a parser needs different gap or line-merge thresholds.
/// Load a PDF, handling encryption.
/// Returns `Err("PASSWORD_REQUIRED")` if encrypted and no password given,
/// or `Err("PASSWORD_REQUIRED")` if the supplied password is wrong.
pub fn load_pdf(file_path: &str, password: Option<&str>) -> Result<lopdf::Document, String> {
    let doc = lopdf::Document::load(file_path)
        .map_err(|e| format!("Failed to open PDF: {e}"))?;
    if doc.is_encrypted() {
        match password {
            None => return Err("PASSWORD_REQUIRED".to_string()),
            Some(pwd) => {
                doc.authenticate_raw_password(pwd.as_bytes())
                    .map_err(|_| "PASSWORD_REQUIRED".to_string())?;
            }
        }
    }
    Ok(doc)
}

pub fn extract_all_page_spans_cfg(
    file_path: &str,
    x_gap: f32,
    char_y_tol: f32,
) -> Result<Vec<Vec<TextSpan>>, String> {
    extract_all_page_spans_pwd_cfg(file_path, None, x_gap, char_y_tol)
}

/// Password + custom span-building parameters.
pub fn extract_all_page_spans_pwd_cfg(
    file_path: &str,
    password: Option<&str>,
    x_gap: f32,
    char_y_tol: f32,
) -> Result<Vec<Vec<TextSpan>>, String> {
    let doc = load_pdf(file_path, password)?;
    extract_spans_from_doc_cfg(&doc, &[], x_gap, char_y_tol)
}

/// Full-control variant: column boundaries + custom span-building parameters.
pub fn extract_spans_from_doc_cfg(
    doc: &lopdf::Document,
    col_boundaries: &[f32],
    x_gap: f32,
    char_y_tol: f32,
) -> Result<Vec<Vec<TextSpan>>, String> {
    let pages: Vec<u32> = doc.get_pages().keys().copied().collect();
    let mut result: Vec<Vec<TextSpan>> = Vec::new();

    for page_num in pages {
        let mut collector = CharCollector::new();
        pdf_extract::output_doc_page(doc, &mut collector, page_num)
            .map_err(|e| format!("Page {page_num} extraction failed: {e}"))?;

        let spans = chars_to_spans(collector.chars, col_boundaries, x_gap, char_y_tol);
        let rows = group_rows(spans, DEFAULT_ROW_Y_TOL);
        let flat: Vec<TextSpan> = rows.into_iter().flatten().collect();
        result.push(flat);
    }

    Ok(result)
}

/// Like `extract_all_page_spans_with_boundaries` with caller-supplied parameters.
pub fn extract_all_page_spans_with_boundaries_cfg(
    file_path: &str,
    col_boundaries: &[f32],
    x_gap: f32,
    char_y_tol: f32,
) -> Result<Vec<Vec<TextSpan>>, String> {
    extract_all_page_spans_with_boundaries_pwd_cfg(file_path, None, col_boundaries, x_gap, char_y_tol)
}

/// Password-aware variant of `extract_all_page_spans_with_boundaries_cfg`.
pub fn extract_all_page_spans_with_boundaries_pwd_cfg(
    file_path: &str,
    password: Option<&str>,
    col_boundaries: &[f32],
    x_gap: f32,
    char_y_tol: f32,
) -> Result<Vec<Vec<TextSpan>>, String> {
    let doc = load_pdf(file_path, password)?;
    extract_spans_from_doc_cfg(&doc, col_boundaries, x_gap, char_y_tol)
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
fn chars_to_spans(mut chars: Vec<Char>, col_boundaries: &[f32], x_gap: f32, char_y_tol: f32) -> Vec<TextSpan> {

    if chars.is_empty() {
        return Vec::new();
    }

    // Pass 1 — sort by Y descending, then group into rows.
    chars.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut char_rows: Vec<Vec<Char>> = Vec::new();
    let mut cur: Vec<Char> = Vec::new();
    let mut row_y = chars[0].1;

    for ch in chars {
        if (ch.1 - row_y).abs() <= char_y_tol {
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
            let gap_break = !buf.is_empty() && x - prev_right > x_gap;
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

// ─── Line / border extraction ─────────────────────────────────────────────────

/// A single straight-line segment in PDF user space (points from page bottom-left).
#[derive(Debug, Clone)]
pub struct LineSegment {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

impl LineSegment {
    pub fn is_horizontal(&self, tol: f32) -> bool { (self.y2 - self.y1).abs() <= tol }
    pub fn is_vertical(&self, tol: f32)   -> bool { (self.x2 - self.x1).abs() <= tol }
    pub fn length(&self) -> f32 {
        let dx = self.x2 - self.x1; let dy = self.y2 - self.y1;
        (dx * dx + dy * dy).sqrt()
    }
}

/// Extract all **stroked** line segments from a single page of `doc`.
///
/// Parses the content stream for `m`, `l`, `re` path operators, tracks the
/// Current Transformation Matrix (CTM) through `q`/`Q`/`cm`, and commits
/// segments when a stroking operator (`S`, `B`, `b`, …) is encountered.
/// Fill-only paths are discarded.  All coordinates are in page user space.
pub fn extract_page_lines(doc: &lopdf::Document, page_num: u32) -> Vec<LineSegment> {
    use lopdf::content::Content;
    use lopdf::Object;

    fn obj_f32(o: &Object) -> Option<f32> {
        match o {
            Object::Integer(i) => Some(*i as f32),
            Object::Real(r)    => Some(*r as f32),
            _                  => None,
        }
    }

    // Affine matrix stored as [a, b, c, d, e, f] (PDF convention).
    type Mat = [f32; 6];
    const IDENTITY: Mat = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

    fn mat_mul(m: Mat, n: Mat) -> Mat {
        let [a1,b1,c1,d1,e1,f1] = m;
        let [a2,b2,c2,d2,e2,f2] = n;
        [a1*a2+b1*c2, a1*b2+b1*d2, c1*a2+d1*c2, c1*b2+d1*d2,
         e1*a2+f1*c2+e2, e1*b2+f1*d2+f2]
    }

    fn xfm(m: Mat, x: f32, y: f32) -> (f32, f32) {
        let [a,b,c,d,e,f] = m;
        (a*x + c*y + e, b*x + d*y + f)
    }

    let mut lines:   Vec<LineSegment>          = Vec::new();
    let mut ctm_stack: Vec<Mat>                = vec![IDENTITY];
    let mut current:   Option<(f32, f32)>      = None; // current point
    let mut start:     Option<(f32, f32)>      = None; // subpath start (for 'h')
    let mut pending:   Vec<(f32,f32,f32,f32)>  = Vec::new();

    let pages = doc.get_pages();
    let page_id = match pages.get(&page_num) { Some(&id) => id, None => return lines };

    let bytes = match doc.get_page_content(page_id) { Ok(b) => b, Err(_) => return lines };
    let content = match Content::decode(&bytes) { Ok(c) => c, Err(_) => return lines };

    for op in &content.operations {
        let ctm = *ctm_stack.last().unwrap_or(&IDENTITY);
        match op.operator.as_str() {
            "q" => { ctm_stack.push(ctm); }
            "Q" => { if ctm_stack.len() > 1 { ctm_stack.pop(); } }
            "cm" if op.operands.len() == 6 => {
                if let (Some(a),Some(b),Some(c),Some(d),Some(e),Some(f)) = (
                    obj_f32(&op.operands[0]), obj_f32(&op.operands[1]),
                    obj_f32(&op.operands[2]), obj_f32(&op.operands[3]),
                    obj_f32(&op.operands[4]), obj_f32(&op.operands[5]),
                ) {
                    *ctm_stack.last_mut().unwrap() = mat_mul([a,b,c,d,e,f], ctm);
                }
            }
            "m" if op.operands.len() == 2 => {
                if let (Some(x), Some(y)) = (obj_f32(&op.operands[0]), obj_f32(&op.operands[1])) {
                    let pt = xfm(ctm, x, y);
                    current = Some(pt);
                    start   = Some(pt);
                }
            }
            "l" if op.operands.len() == 2 => {
                if let (Some(x), Some(y)) = (obj_f32(&op.operands[0]), obj_f32(&op.operands[1])) {
                    let pt = xfm(ctm, x, y);
                    if let Some((cx, cy)) = current { pending.push((cx, cy, pt.0, pt.1)); }
                    current = Some(pt);
                }
            }
            "h" => {
                if let (Some((cx,cy)), Some((sx,sy))) = (current, start) {
                    if (cx-sx).abs() > 0.01 || (cy-sy).abs() > 0.01 {
                        pending.push((cx, cy, sx, sy));
                    }
                }
                current = start;
            }
            "re" if op.operands.len() == 4 => {
                if let (Some(x),Some(y),Some(w),Some(h)) = (
                    obj_f32(&op.operands[0]), obj_f32(&op.operands[1]),
                    obj_f32(&op.operands[2]), obj_f32(&op.operands[3]),
                ) {
                    let (x0,y0) = xfm(ctm, x,   y);
                    let (x1,y1) = xfm(ctm, x+w, y);
                    let (x2,y2) = xfm(ctm, x+w, y+h);
                    let (x3,y3) = xfm(ctm, x,   y+h);
                    pending.extend_from_slice(&[(x0,y0,x1,y1),(x1,y1,x2,y2),(x2,y2,x3,y3),(x3,y3,x0,y0)]);
                    current = Some((x0, y0));
                    start   = Some((x0, y0));
                }
            }
            // Stroking operators — commit pending segments
            "S" | "s" | "B" | "B*" | "b" | "b*" => {
                lines.extend(pending.drain(..).map(|(x1,y1,x2,y2)| LineSegment { x1, y1, x2, y2 }));
                current = None; start = None;
            }
            // Fill-only / path-end — discard
            "f" | "F" | "f*" | "n" => { pending.clear(); current = None; start = None; }
            _ => {}
        }
    }

    lines
}

// ─── Border-based grid helpers ────────────────────────────────────────────────

/// `(row, col) → [(y, text)]` — spans within a bordered cell, sorted Y-desc.
pub type CellMap = std::collections::HashMap<(usize, usize), Vec<(f32, String)>>;

/// Cluster a flat list of f32 values into representative medians.
/// Values within `gap` of the previous one join the same cluster.
pub fn cluster_coords(mut vals: Vec<f32>, gap: f32) -> Vec<f32> {
    if vals.is_empty() { return Vec::new(); }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut groups: Vec<Vec<f32>> = Vec::new();
    let mut cur: Vec<f32> = vec![vals[0]];
    for v in vals.into_iter().skip(1) {
        if v - *cur.last().unwrap() > gap { groups.push(std::mem::take(&mut cur)); }
        cur.push(v);
    }
    groups.push(cur);
    groups.into_iter()
        .map(|mut g| { g.sort_by(|a,b| a.partial_cmp(b).unwrap()); g[g.len()/2] })
        .collect()
}

/// Derive `(row_ys, col_xs)` from stroked border line segments.
/// Both vecs are sorted ascending; `row_ys` are horizontal-line Y positions,
/// `col_xs` are vertical-line X positions.
pub fn grid_from_lines(
    lines:       &[LineSegment],
    min_h_len:   f32,
    min_v_len:   f32,
    cluster_gap: f32,
) -> (Vec<f32>, Vec<f32>) {
    let h_ys: Vec<f32> = lines.iter()
        .filter(|l| l.is_horizontal(1.0) && l.length() >= min_h_len)
        .map(|l| l.y1)
        .collect();
    let v_xs: Vec<f32> = lines.iter()
        .filter(|l| l.is_vertical(1.0) && l.length() >= min_v_len)
        .map(|l| l.x1)
        .collect();
    (cluster_coords(h_ys, cluster_gap), cluster_coords(v_xs, cluster_gap))
}

// ─── Banded grid (one grid per table) ────────────────────────────────────────

/// A single table's grid derived from its own V/H-lines within a Y band.
#[derive(Debug)]
#[allow(dead_code)]
pub struct TableBand {
    pub y_min:   f32,        // lowest Y of the band (PDF coords, from page bottom)
    pub y_max:   f32,        // highest Y of the band
    pub col_xs:  Vec<f32>,   // clustered column left-edge X positions (sorted ascending)
    pub row_ys:  Vec<f32>,   // clustered row boundary Y positions (sorted ascending)
}

/// Detect independent table grids on a page by grouping V-lines into non-overlapping
/// Y bands, then deriving separate col_xs / row_ys for each band.
///
/// Algorithm:
///   1. Collect V-lines with length >= min_v_len; record (y_min, y_max, x).
///   2. Sort by y_min and merge overlapping/adjacent Y intervals (gap <= cluster_gap).
///   3. For each merged band, gather the X positions of V-lines that overlap it
///      and the Y positions of H-lines whose Y falls inside it.
///   4. Cluster both with cluster_gap to get col_xs / row_ys.
///
/// This ensures that a page with multiple stacked tables (equity + derivative,
/// each with different column layouts) produces independent grids instead of one
/// merged column list that corrupts cell assignment for all tables.
pub fn grid_from_lines_banded(
    lines:       &[LineSegment],
    min_h_len:   f32,
    min_v_len:   f32,
    cluster_gap: f32,
) -> Vec<TableBand> {
    // Step 1 — qualifying V-lines as (y_min, y_max, x)
    let mut v_segs: Vec<(f32, f32, f32)> = lines.iter()
        .filter(|l| l.is_vertical(1.0) && l.length() >= min_v_len)
        .map(|l| (l.y1.min(l.y2), l.y1.max(l.y2), l.x1))
        .collect();

    if v_segs.is_empty() { return Vec::new(); }

    v_segs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // Step 2 — merge Y intervals that touch or overlap
    let mut band_ranges: Vec<(f32, f32)> = Vec::new();
    let (mut cur_min, mut cur_max) = (v_segs[0].0, v_segs[0].1);
    for &(y_min, y_max, _) in v_segs.iter().skip(1) {
        if y_min <= cur_max + cluster_gap {
            cur_max = cur_max.max(y_max);
        } else {
            band_ranges.push((cur_min, cur_max));
            cur_min = y_min;
            cur_max = y_max;
        }
    }
    band_ranges.push((cur_min, cur_max));

    // Qualifying H-line Y values
    let h_ys: Vec<f32> = lines.iter()
        .filter(|l| l.is_horizontal(1.0) && l.length() >= min_h_len)
        .map(|l| l.y1)
        .collect();

    // Step 3 — build each band
    band_ranges.into_iter().map(|(y_min, y_max)| {
        let band_xs: Vec<f32> = v_segs.iter()
            .filter(|&&(vy_min, vy_max, _)| vy_min <= y_max + cluster_gap && vy_max >= y_min - cluster_gap)
            .map(|&(_, _, x)| x)
            .collect();
        let col_xs = cluster_coords(band_xs, cluster_gap);

        let band_ys: Vec<f32> = h_ys.iter()
            .filter(|&&y| y >= y_min - cluster_gap && y <= y_max + cluster_gap)
            .copied()
            .collect();
        let row_ys = cluster_coords(band_ys, cluster_gap);

        TableBand { y_min, y_max, col_xs, row_ys }
    }).collect()
}

/// Assign each span to a `(row, col)` cell determined by the border grid.
///
/// A span at `(x, y)` lands in:
/// - the column whose left-boundary is the largest `col_x ≤ x + col_snap`
/// - the row whose lower-boundary is the smallest `row_y ≥ y`
///
/// Spans within a cell are kept sorted by Y descending (top-to-bottom read order).
pub fn build_cell_map(
    spans:    &[TextSpan],
    row_ys:   &[f32],
    col_xs:   &[f32],
    col_snap: f32,
) -> CellMap {
    let mut map: CellMap = std::collections::HashMap::new();
    for span in spans {
        let col = col_xs.partition_point(|&cx| cx <= span.x + col_snap).saturating_sub(1);
        let row = row_ys.partition_point(|&ry| ry < span.y);
        map.entry((row, col)).or_default().push((span.y, span.text.clone()));
    }
    for v in map.values_mut() {
        v.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    }
    map
}

/// Return all text in a `(row, col)` cell joined top-to-bottom with no separator.
pub fn cell_text(map: &CellMap, row: usize, col: usize) -> String {
    map.get(&(row, col))
       .map(|v| v.iter().map(|(_, t)| t.trim()).filter(|t| !t.is_empty())
                .collect::<Vec<_>>().join(""))
       .unwrap_or_default()
}

// ─── Internal helpers ────────────────────────────────────────────────────────

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bajaj_pdf_open() {
        let path = r"C:\Users\SUMIT\Desktop\harsh\APRIL-26\1.4.2026.pdf";
        let password = "GYEPS4368P";

        if !std::path::Path::new(path).exists() {
            println!("SKIP: file not found at {path}");
            return;
        }

        match load_pdf(path, Some(password)) {
            Ok(doc) => {
                let pages = doc.get_pages();
                println!("SUCCESS — lopdf opened the PDF, {} page(s)", pages.len());
                // Try extracting spans from page 1
                match extract_spans_from_doc_cfg(&doc, &[], 6.0, 3.5) {
                    Ok(spans) => {
                        let total: usize = spans.iter().map(|p| p.len()).sum();
                        println!("Extracted {total} spans across {} pages", spans.len());
                        if let Some(p1) = spans.first() {
                            let preview: String = p1.iter().take(20)
                                .map(|s| s.text.as_str())
                                .collect::<Vec<_>>()
                                .join(" | ");
                            println!("Page 1 first spans: {preview}");
                        }
                    }
                    Err(e) => println!("Span extraction failed: {e}"),
                }
            }
            Err(e) => println!("FAIL — lopdf could not open PDF: {e}"),
        }
    }
}
