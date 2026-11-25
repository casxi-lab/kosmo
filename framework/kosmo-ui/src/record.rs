//! The event handling record of a round.

/// The event handling record of one round: UTF-8 text, one entry per line,
/// with its fields in a fixed order. It holds no wall-clock time, no
/// address and nothing in hash order, so the same rounds give the same
/// record byte for byte.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Record {
    lines: Vec<String>,
}

impl Record {
    /// The lines of the record, without line ends.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// The record as text: each line followed by a line feed.
    pub fn text(&self) -> String {
        let mut text = String::new();
        for line in &self.lines {
            text.push_str(line);
            text.push('\n');
        }
        text
    }

    /// Appends a line. Line feeds and carriage returns inside it, which a
    /// value's debug form may hold, are written as `\n` and `\r`, so every
    /// entry stays on one line.
    pub(crate) fn push(&mut self, line: String) {
        if line.contains(['\n', '\r']) {
            self.lines
                .push(line.replace('\n', "\\n").replace('\r', "\\r"));
        } else {
            self.lines.push(line);
        }
    }
}
