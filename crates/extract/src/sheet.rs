//! Spreadsheets (xlsx, xlsm, xlsb, xls, ods) through calamine: every sheet as CSV rows under a
//! marker line saying its name and size, so a model can read the table and Python can be pointed
//! at the file for the rest.
//!
//! Excel's XML and binary sheets are read cell by cell. calamine's ranges are dense grids of the
//! used area, so one stray value at the far corner of a sheet would ask for gigabytes; here a
//! far-away cell costs only itself, and reading stops once the text cap is reached.

use std::collections::BTreeMap;
use std::io::Cursor;

use calamine::{Data, Reader, Sheets, open_workbook_auto_from_rs};

use crate::out::TextOut;
use crate::{Extracted, MAX_TEXT_CHARS, count, group};

const DAMAGED: &str = "This spreadsheet could not be read. It may be damaged or password-protected.";

pub(crate) fn read(base: Extracted, bytes: &[u8]) -> Extracted {
    let Ok(mut book) = open_workbook_auto_from_rs(Cursor::new(bytes)) else {
        return base.with_note(DAMAGED);
    };
    let names = book.sheet_names();
    if names.is_empty() {
        return base.with_note(DAMAGED);
    }
    let mut out = TextOut::new();
    let mut unreadable = 0usize;
    for name in &names {
        if out.is_full() {
            break;
        }
        let Some(sheet) = read_sheet(&mut book, name) else {
            unreadable += 1;
            out.page(&format!("--- Sheet \"{}\" (could not be read) ---", marker_name(name)));
            continue;
        };
        let (rows, cols) = sheet.size();
        out.page(&format!(
            "--- Sheet \"{}\" ({} × {}) ---",
            marker_name(name),
            count(rows, "row", "rows"),
            count(cols, "column", "columns")
        ));
        for line in sheet.lines() {
            if out.is_full() {
                break;
            }
            out.line(&line);
        }
    }
    let mut read = base.with_text(out);
    read.pages = Some(u32::try_from(names.len()).unwrap_or(u32::MAX));
    if unreadable > 0 {
        read = read.with_note(format!(
            "{} of {} sheets could not be read.",
            group(unreadable),
            group(names.len())
        ));
    }
    if read.text.is_none() && unreadable == 0 {
        return read.with_note("The spreadsheet is empty.");
    }
    read
}

/// Feeds a calamine cell reader into `cells`; None when the sheet cannot be read.
macro_rules! stream {
    ($reader:expr, $cells:ident) => {{
        let Ok(mut reader) = $reader else {
            return None;
        };
        loop {
            match reader.next_cell() {
                Ok(Some(c)) => {
                    let (row, col) = c.get_position();
                    let value: Data = c.get_value().clone().into();
                    if !$cells.push(row, col, cell(&value)) {
                        break;
                    }
                }
                Ok(None) => break,
                Err(_) => return None,
            }
        }
    }};
}

fn read_sheet(book: &mut Sheets<Cursor<&[u8]>>, name: &str) -> Option<SparseSheet> {
    let mut cells = SparseSheet::default();
    match book {
        Sheets::Xlsx(x) => stream!(x.worksheet_cells_reader(name), cells),
        Sheets::Xlsb(x) => stream!(x.worksheet_cells_reader(name), cells),
        // Old .xls sheets have at most 65,536 × 256 cells, and OpenDocument has no cell reader.
        other => {
            let range = other.worksheet_range(name).ok()?;
            let (top, left) = range.start().unwrap_or((0, 0));
            'rows: for (r, row) in range.rows().enumerate() {
                for (c, value) in row.iter().enumerate() {
                    if !cells.push(top + r as u32, left + c as u32, cell(value)) {
                        break 'rows;
                    }
                }
            }
        }
    }
    Some(cells)
}

/// The cells of a sheet that hold something, by row.
#[derive(Default)]
struct SparseSheet {
    rows: BTreeMap<u32, Vec<(u32, String)>>,
    chars: usize,
    first_col: Option<u32>,
    last_col: u32,
}

impl SparseSheet {
    /// Adds a cell; false once the sheet holds as much text as a file may.
    fn push(&mut self, row: u32, col: u32, text: String) -> bool {
        if text.is_empty() {
            return true;
        }
        self.chars += text.chars().count() + 1;
        self.first_col = Some(self.first_col.map_or(col, |c| c.min(col)));
        self.last_col = self.last_col.max(col);
        self.rows.entry(row).or_default().push((col, text));
        self.chars < MAX_TEXT_CHARS
    }

    /// Rows and columns of the area the values span.
    fn size(&self) -> (usize, usize) {
        match (self.rows.keys().next(), self.rows.keys().next_back(), self.first_col) {
            (Some(&top), Some(&bottom), Some(left)) => {
                ((bottom - top) as usize + 1, (self.last_col - left) as usize + 1)
            }
            _ => (0, 0),
        }
    }

    /// Each row as a CSV line from the first column with a value; empty cells in between are
    /// empty fields.
    fn lines(self) -> impl Iterator<Item = String> {
        let left = self.first_col.unwrap_or(0);
        self.rows.into_values().map(move |mut cells| {
            cells.sort_by_key(|(col, _)| *col);
            let mut line = String::new();
            // Column of the last field written: a cell at `col` follows it after one comma per
            // column between them.
            let mut last: Option<u32> = None;
            for (col, text) in cells {
                let from = match last {
                    Some(prev) if col <= prev => continue,
                    Some(prev) => prev,
                    None => left,
                };
                for _ in from..col {
                    line.push(',');
                }
                line.push_str(&csv_field(&text));
                last = Some(col);
            }
            line
        })
    }
}

/// A sheet name inside the marker's quotes.
fn marker_name(name: &str) -> String {
    name.replace('"', "'").replace(['\n', '\r'], " ")
}

/// A cell as a person reads it: whole numbers without ".0", dates as dates.
fn cell(value: &Data) -> String {
    match value {
        Data::Empty => String::new(),
        Data::Float(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", *f as i64),
        Data::DateTime(dt) => {
            if dt.is_duration() {
                dt.as_duration()
                    .map(|d| {
                        let secs = d.num_seconds();
                        format!("{}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60)
                    })
                    .unwrap_or_else(|| dt.to_string())
            } else {
                dt.as_datetime()
                    .map(|d| {
                        if d.time() == chrono::NaiveTime::MIN {
                            d.format("%Y-%m-%d").to_string()
                        } else {
                            d.format("%Y-%m-%d %H:%M:%S").to_string()
                        }
                    })
                    .unwrap_or_else(|| dt.to_string())
            }
        }
        Data::Error(e) => format!("#{e:?}"),
        other => other.to_string(),
    }
}

/// A CSV field: quoted when it holds a comma, a quote or a line break.
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Kind;
    use rust_xlsxwriter::{ExcelDateTime, Format, Workbook};

    fn xlsx() -> Vec<u8> {
        let mut book = Workbook::new();
        let sales = book.add_worksheet().set_name("Sales").unwrap();
        sales.write_string(0, 0, "Region").unwrap();
        sales.write_string(0, 1, "Revenue").unwrap();
        sales.write_string(0, 2, "Date").unwrap();
        sales.write_string(1, 0, "North, East").unwrap();
        sales.write_number(1, 1, 1200.0).unwrap();
        let date = ExcelDateTime::from_ymd(2026, 3, 14).unwrap();
        sales
            .write_datetime_with_format(1, 2, &date, &Format::new().set_num_format("yyyy-mm-dd"))
            .unwrap();
        sales.write_string(2, 0, "South").unwrap();
        sales.write_number(2, 1, 99.5).unwrap();
        let notes = book.add_worksheet().set_name("Notes").unwrap();
        notes.write_string(0, 0, "Checked by \"Ana\"").unwrap();
        book.save_to_buffer().unwrap()
    }

    #[test]
    fn sheets_become_csv_under_markers() {
        let read = read(Extracted::new(Kind::Data, crate::detect::XLSX), &xlsx());
        let text = read.text.expect("text");
        assert_eq!(read.pages, Some(2));
        assert_eq!(read.page_starts.len(), 2);
        assert!(
            text.starts_with("--- Sheet \"Sales\" (3 rows × 3 columns) ---\nRegion,Revenue,Date\n"),
            "{text}"
        );
        assert!(text.contains("\"North, East\",1200,2026-03-14\n"), "{text}");
        assert!(text.contains("South,99.5\n"), "{text}");
        assert!(
            text.contains("--- Sheet \"Notes\" (1 row × 1 column) ---\n\"Checked by \"\"Ana\"\"\""),
            "{text}"
        );
    }

    #[test]
    fn a_far_away_cell_costs_only_itself() {
        // calamine's dense range for this sheet would be 1,048,576 × 16,384 cells.
        let mut book = Workbook::new();
        let sheet = book.add_worksheet();
        sheet.write_string(0, 0, "top left").unwrap();
        sheet.write_string(2, 2, "middle").unwrap();
        sheet.write_string(1_048_575, 16_383, "far corner").unwrap();
        let bytes = book.save_to_buffer().unwrap();
        let started = std::time::Instant::now();
        let read = read(Extracted::new(Kind::Data, crate::detect::XLSX), &bytes);
        assert!(started.elapsed().as_secs() < 5);
        let text = read.text.expect("text");
        assert!(
            text.contains("(1,048,576 rows × 16,384 columns) ---\ntop left\n,,middle\n"),
            "{}",
            &text[..200]
        );
        assert!(text.ends_with(&format!("{}far corner", ",".repeat(16_383))));
    }

    #[test]
    fn garbage_is_not_a_crash() {
        let read = read(
            Extracted::new(Kind::Data, crate::detect::XLSX),
            b"PK\x03\x04 not a workbook",
        );
        assert_eq!(read.text, None);
        assert!(read.note.unwrap().contains("could not be read"));
    }
}
