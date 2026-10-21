//! The `replay` generator (ref/03 §2.4.5): recorded values from a CSV file,
//! held from each row to the next (step interpolation).
//!
//! The driver does no I/O: the core reads the file named by `file` and
//! passes its bytes in the device's `config` (ADR 0054). Parquet is reserved
//! for a later version.

use std::string::String;
use std::vec::Vec;

/// A recorded series: strictly increasing times and their values.
#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    /// Times in ns after the model starts.
    pub times: Vec<u64>,
    /// The value of each row, in the signal's unit.
    pub values: Vec<f64>,
    /// Repeat the series after its end.
    pub looped: bool,
}

impl Series {
    /// The length of one loop: from the first row to the end of the last,
    /// which lasts as long as the interval before it.
    pub fn period_ns(&self) -> u64 {
        match self.times.as_slice() {
            [first, .., last] => {
                let prev = self.times[self.times.len() - 2];
                last - first + (last - prev)
            }
            _ => 0,
        }
    }

    /// The value at `t_ns`: the last row at or before it, or the first row.
    pub fn at(&self, t_ns: u64) -> f64 {
        let Some(&first) = self.times.first() else {
            return f64::NAN;
        };
        let period = self.period_ns();
        let t = if self.looped && period > 0 && t_ns >= first {
            first + (t_ns - first) % period
        } else {
            t_ns
        };
        let k = self.times.partition_point(|x| *x <= t).saturating_sub(1);
        self.values.get(k).copied().unwrap_or(f64::NAN)
    }
}

/// A CSV file split into a header and rows of trimmed cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Csv {
    /// Column names.
    pub header: Vec<String>,
    /// Rows, each with one cell per column.
    pub rows: Vec<Vec<String>>,
}

fn cells(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(core::mem::take(&mut cur).trim().to_owned()),
            other => cur.push(other),
        }
    }
    out.push(cur.trim().to_owned());
    out
}

/// Parses CSV text with a header row; blank lines and lines starting with
/// `#` are skipped.
pub fn parse_csv(text: &str) -> Result<Csv, String> {
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'));
    let header = cells(lines.next().ok_or("the file is empty")?);
    let mut rows = Vec::new();
    for (k, line) in lines.enumerate() {
        let row = cells(line);
        if row.len() != header.len() {
            return Err(std::format!(
                "row {} has {} cells and the header {}",
                k + 1,
                row.len(),
                header.len()
            ));
        }
        rows.push(row);
    }
    Ok(Csv { header, rows })
}

impl Csv {
    /// The cells of column `name`.
    pub fn column(&self, name: &str) -> Option<Vec<&str>> {
        let i = self.header.iter().position(|h| h == name)?;
        Some(self.rows.iter().map(|r| r[i].as_str()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_with_quotes_and_comments() {
        let csv =
            parse_csv("# recorded\nt, p, note\n0, 1.5, \"a, b\"\n\n1s, 2, \"say \"\"hi\"\"\"\n")
                .unwrap();
        assert_eq!(csv.header, ["t", "p", "note"]);
        assert_eq!(csv.column("p").unwrap(), ["1.5", "2"]);
        assert_eq!(csv.column("note").unwrap(), ["a, b", "say \"hi\""]);
        assert!(parse_csv("t,p\n1\n").is_err());
        assert!(parse_csv("").is_err());
    }

    #[test]
    fn step_interpolation_and_loop() {
        let mut s = Series {
            times: std::vec![0, 10, 30],
            values: std::vec![1.0, 2.0, 3.0],
            looped: false,
        };
        assert_eq!(
            [s.at(0), s.at(9), s.at(10), s.at(29), s.at(500)],
            [1.0, 1.0, 2.0, 2.0, 3.0]
        );
        s.looped = true;
        assert_eq!(s.period_ns(), 50);
        assert_eq!([s.at(50), s.at(60), s.at(85)], [1.0, 2.0, 3.0]);
    }
}
