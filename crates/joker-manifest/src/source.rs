//! Source files and byte spans, so every diagnostic points at the layer and
//! the text where the key or value was written (ref/01 §11.4).

use std::path::{Component, Path, PathBuf};

/// The index of a file in [`Sources`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(u32);

/// A byte range in one source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    /// The file.
    pub file: FileId,
    /// The first byte.
    pub start: usize,
    /// One past the last byte.
    pub end: usize,
}

impl Span {
    /// A span of `start..end` in `file`.
    pub const fn new(file: FileId, start: usize, end: usize) -> Self {
        Self { file, start, end }
    }

    /// A sub-span, given as a byte range relative to the start of this span.
    pub fn slice(self, range: std::ops::Range<usize>) -> Self {
        let start = (self.start + range.start).min(self.end);
        let end = (self.start + range.end).clamp(start, self.end);
        Self::new(self.file, start, end)
    }
}

/// One loaded file: its path relative to the project root and its text.
#[derive(Debug, Clone)]
pub struct SourceFile {
    path: PathBuf,
    text: String,
    line_starts: Vec<usize>,
}

impl SourceFile {
    /// The path relative to the project root, as shown in diagnostics.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The full text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The 1-based line and column (in characters) of a byte offset.
    pub fn position(&self, offset: usize) -> (usize, usize) {
        let line = self.line_starts.partition_point(|&start| start <= offset);
        let start = self
            .line_starts
            .get(line.wrapping_sub(1))
            .copied()
            .unwrap_or(0);
        let column = self
            .text
            .get(start..offset.min(self.text.len()))
            .map_or(0, |prefix| prefix.chars().count());
        (line.max(1), column + 1)
    }

    /// The text of a 1-based line, without its line break.
    pub fn line(&self, line: usize) -> &str {
        let start = self
            .line_starts
            .get(line.wrapping_sub(1))
            .copied()
            .unwrap_or(0);
        let end = self
            .line_starts
            .get(line)
            .copied()
            .unwrap_or(self.text.len());
        self.text
            .get(start..end)
            .unwrap_or("")
            .trim_end_matches(['\n', '\r'])
    }
}

/// Every file read while checking one manifest.
#[derive(Debug, Clone, Default)]
pub struct Sources {
    files: Vec<SourceFile>,
}

impl Sources {
    /// Adds a file and returns its id.
    pub fn add(&mut self, path: PathBuf, text: String) -> FileId {
        let line_starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        let id = FileId(u32::try_from(self.files.len()).unwrap_or(u32::MAX));
        self.files.push(SourceFile {
            path,
            text,
            line_starts,
        });
        id
    }

    /// The file with an id.
    pub fn get(&self, id: FileId) -> Option<&SourceFile> {
        usize::try_from(id.0).ok().and_then(|i| self.files.get(i))
    }

    /// The path of a file, or an empty path for an unknown id.
    pub fn path(&self, id: FileId) -> &Path {
        self.get(id).map_or(Path::new(""), SourceFile::path)
    }

    /// The text a span covers.
    pub fn text(&self, span: Span) -> &str {
        self.get(span.file)
            .and_then(|file| file.text.get(span.start..span.end))
            .unwrap_or("")
    }
}

/// Normalises a path lexically: removes `.` and resolves `..` against the
/// preceding component, without touching the file system.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_count_characters() {
        let mut sources = Sources::default();
        let id = sources.add("a.toml".into(), "x = 1\nµ = \"é\"\n".to_owned());
        let file = sources.get(id).unwrap();
        assert_eq!(file.position(0), (1, 1));
        assert_eq!(file.position(6), (2, 1));
        assert_eq!(file.position(10), (2, 4));
        assert_eq!(file.line(2), "µ = \"é\"");
    }

    #[test]
    fn paths_normalize_lexically() {
        assert_eq!(normalize(Path::new("a/./b/../c")), PathBuf::from("a/c"));
        assert_eq!(normalize(Path::new("../a")), PathBuf::from("../a"));
    }
}
