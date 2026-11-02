//! The PDF view of a decision report (ADR 0104): built from the node's
//! `text/csv` (ref/09 §5.7) and nothing else, A4 landscape, Courier.

use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str};

/// What the head of every page says besides the title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    /// The node.
    pub node: String,
    /// The filters as sent.
    pub filters: String,
    /// When the report was made, UTC.
    pub made: String,
}

const PAGE_W: f32 = 842.0;
const PAGE_H: f32 = 595.0;
const MARGIN: f32 = 28.0;
const SIZE: f32 = 6.0;
const LEADING: f32 = 8.0;
const HEAD_LINES: usize = 6;

/// The width of each column of ref/09 §5.7, in characters.
const WIDTHS: [usize; 14] = [24, 24, 9, 18, 10, 9, 10, 12, 9, 10, 7, 24, 16, 26];

/// The rows of a CSV text (RFC 4180: quoted cells, doubled quotes, line
/// breaks inside quotes).
pub fn rows(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                cell.push('"');
                chars.next();
            }
            (true, '"') => quoted = false,
            (true, c) => cell.push(c),
            (false, '"') => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut cell)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            (false, c) => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

/// One cell at its column's width: cut with a final `~` when longer,
/// printable ASCII only (anything else is `?`), padded with spaces.
fn fit(text: &str, width: usize) -> String {
    let clean: String = text
        .chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() {
                c
            } else {
                '?'
            }
        })
        .collect();
    let mut out: String = if clean.chars().count() > width {
        let mut cut: String = clean.chars().take(width.saturating_sub(1)).collect();
        cut.push('~');
        cut
    } else {
        clean
    };
    while out.len() < width {
        out.push(' ');
    }
    out
}

fn line(cells: &[String]) -> String {
    WIDTHS
        .iter()
        .enumerate()
        .map(|(i, w)| fit(cells.get(i).map_or("", String::as_str), *w))
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end()
        .to_owned()
}

fn ascii(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() {
                c
            } else {
                '?'
            }
        })
        .collect()
}

/// The PDF of a decision report: the head on every page, then the CSV
/// header and one line per entry, `page <n> of <m>` at the foot.
pub fn pdf(csv: &str, meta: &Meta) -> Vec<u8> {
    let all = rows(csv);
    let (header, entries) = match all.split_first() {
        Some((h, e)) => (h.clone(), e.to_vec()),
        None => (Vec::new(), Vec::new()),
    };
    let usable = ((PAGE_H - 2.0 * MARGIN) / LEADING) as usize;
    let per_page = usable.saturating_sub(HEAD_LINES + 3).max(1);
    let chunks: Vec<&[Vec<String>]> = if entries.is_empty() {
        vec![&[]]
    } else {
        entries.chunks(per_page).collect()
    };
    let pages = chunks.len();
    let catalog = Ref::new(1);
    let tree = Ref::new(2);
    let regular = Ref::new(3);
    let bold = Ref::new(4);
    let page_ids: Vec<Ref> = (0..pages)
        .map(|i| Ref::new(5 + 2 * i32::try_from(i).unwrap_or(0)))
        .collect();
    let mut doc = Pdf::new();
    doc.catalog(catalog).pages(tree);
    doc.pages(tree)
        .kids(page_ids.iter().copied())
        .count(i32::try_from(pages).unwrap_or(i32::MAX));
    doc.type1_font(regular)
        .base_font(Name(b"Courier"))
        .encoding_predefined(Name(b"WinAnsiEncoding"));
    doc.type1_font(bold)
        .base_font(Name(b"Courier-Bold"))
        .encoding_predefined(Name(b"WinAnsiEncoding"));
    let head = [
        format!("Node: {}", ascii(&meta.node)),
        format!("Filters: {}", ascii(&meta.filters)),
        format!("Entries: {}", entries.len()),
        format!("Made: {}", ascii(&meta.made)),
    ];
    for (n, (chunk, id)) in chunks.iter().zip(&page_ids).enumerate() {
        let content_id = Ref::new(id.get() + 1);
        let mut page = doc.page(*id);
        page.media_box(Rect::new(0.0, 0.0, PAGE_W, PAGE_H));
        page.parent(tree);
        page.contents(content_id);
        page.resources()
            .fonts()
            .pair(Name(b"R"), regular)
            .pair(Name(b"B"), bold);
        page.finish();
        let mut c = Content::new();
        c.begin_text();
        c.set_font(Name(b"B"), 12.0);
        c.next_line(MARGIN, PAGE_H - MARGIN - 12.0);
        c.show(Str(b"Decision log"));
        c.set_font(Name(b"R"), SIZE + 1.0);
        c.set_leading(LEADING + 1.0);
        for h in &head {
            c.next_line_using_leading();
            c.show(Str(h.as_bytes()));
        }
        c.set_font(Name(b"B"), SIZE);
        c.set_leading(LEADING);
        c.next_line_using_leading();
        c.next_line_using_leading();
        c.show(Str(line(&header).as_bytes()));
        c.set_font(Name(b"R"), SIZE);
        for row in *chunk {
            c.next_line_using_leading();
            c.show(Str(line(row).as_bytes()));
        }
        c.end_text();
        c.begin_text();
        c.set_font(Name(b"R"), SIZE);
        c.next_line(PAGE_W - MARGIN - 60.0, MARGIN / 2.0);
        c.show(Str(format!("page {} of {pages}", n + 1).as_bytes()));
        c.end_text();
        doc.stream(content_id, &c.finish());
    }
    doc.finish()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    const CSV: &str = "ts,output,kind,writer,level,proposed,outcome,reason,applied,active_level,repeats,last_ts,principal,request_id\n\
        2026-09-24T03:10:00.000000000Z,pcs.p_set,proposal,app:peak_shaver,control:12,,accepted,,\"50,0\",control:12,3,,,\n";

    fn meta() -> Meta {
        Meta {
            node: "unix:/run/joker/api.sock".into(),
            filters: "output=pcs.p_set".into(),
            made: "2026-11-02T10:00:00Z".into(),
        }
    }

    #[test]
    fn rows_follow_rfc_4180() {
        let r = rows("a,\"b,c\",\"d\"\"e\"\n1,,\"x\ny\"\n");
        assert_eq!(r[0], vec!["a", "b,c", "d\"e"]);
        assert_eq!(r[1], vec!["1", "", "x\ny"]);
        assert_eq!(rows("a,b").len(), 1);
    }

    #[test]
    fn long_cells_are_cut_with_a_tilde_and_non_ascii_is_a_question_mark() {
        assert_eq!(fit("abcdef", 4), "abc~");
        assert_eq!(fit("°C", 4), "?C  ");
    }

    #[test]
    fn a_report_is_a_pdf_with_its_head_and_entries_and_the_same_input_gives_the_same_bytes() {
        let a = pdf(CSV, &meta());
        assert!(a.starts_with(b"%PDF-"));
        let text = String::from_utf8_lossy(&a);
        assert!(text.contains("(Decision log)"));
        assert!(text.contains("Entries: 1"));
        assert!(text.contains("pcs.p_set"));
        assert!(text.contains("50,0"));
        assert!(text.contains("page 1 of 1"));
        assert!(text.contains("/Courier"));
        assert_eq!(a, pdf(CSV, &meta()));
    }

    #[test]
    fn many_entries_span_pages_and_an_empty_report_has_one_page() {
        let mut csv = String::from(CSV.lines().next().unwrap());
        csv.push('\n');
        for i in 0..200 {
            csv.push_str(&format!("2026-09-24T03:{:02}:00Z,o{i},proposal\n", i % 60));
        }
        let text = String::from_utf8_lossy(&pdf(&csv, &meta())).to_string();
        assert!(text.contains("page 1 of 4"), "{}", text.len());
        assert!(text.contains(" o199"));
        let empty = String::from_utf8_lossy(&pdf("", &meta())).to_string();
        assert!(empty.contains("page 1 of 1"));
        assert!(empty.contains("Entries: 0"));
    }
}
