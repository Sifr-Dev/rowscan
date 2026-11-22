use std::fmt;
use std::io::{self, BufRead, Write};

pub const DEFAULT_MAX_FIELD_BYTES: usize = 1024 * 1024;
pub const DEFAULT_MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_MAX_COLUMNS: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CsvLimits {
    pub max_field_bytes: usize,
    pub max_record_bytes: usize,
    pub max_columns: usize,
}

impl Default for CsvLimits {
    fn default() -> Self {
        Self {
            max_field_bytes: DEFAULT_MAX_FIELD_BYTES,
            max_record_bytes: DEFAULT_MAX_RECORD_BYTES,
            max_columns: DEFAULT_MAX_COLUMNS,
        }
    }
}

#[derive(Debug)]
pub enum CsvError {
    Io(io::Error),
    Syntax { line: u64, message: String },
    Limit { line: u64, message: String },
    InvalidUtf8 { line: u64 },
}

impl CsvError {
    pub fn is_broken_pipe(&self) -> bool {
        matches!(self, Self::Io(error) if error.kind() == io::ErrorKind::BrokenPipe)
    }
}

impl fmt::Display for CsvError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "input read failed: {error}"),
            Self::Syntax { line, message } => {
                write!(formatter, "CSV error on line {line}: {message}")
            }
            Self::Limit { line, message } => {
                write!(formatter, "CSV limit on line {line}: {message}")
            }
            Self::InvalidUtf8 { line } => write!(
                formatter,
                "CSV error on line {line}: input is not valid UTF-8"
            ),
        }
    }
}

impl std::error::Error for CsvError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for CsvError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    FieldStart,
    Unquoted,
    Quoted,
    AfterQuote,
}

/// A strict, incremental RFC 4180-style reader.
///
/// Records are yielded one at a time. Quoted CR, LF, and CRLF are preserved,
/// and a quote in an unquoted field is literal (matching Python's csv module).
pub struct CsvReader<R> {
    reader: R,
    limits: CsvLimits,
    line: u64,
    previous_was_cr: bool,
    skip_lf: bool,
}

impl<R: BufRead> CsvReader<R> {
    pub fn new(reader: R) -> Self {
        Self::with_limits(reader, CsvLimits::default())
    }

    pub fn with_limits(reader: R, limits: CsvLimits) -> Self {
        Self {
            reader,
            limits,
            line: 1,
            previous_was_cr: false,
            skip_lf: false,
        }
    }

    fn read_byte(&mut self) -> Result<Option<u8>, CsvError> {
        let buffer = self.reader.fill_buf()?;
        let Some(&byte) = buffer.first() else {
            return Ok(None);
        };
        self.reader.consume(1);

        if byte == b'\r' {
            self.line = self.line.saturating_add(1);
            self.previous_was_cr = true;
        } else {
            if byte == b'\n' && !self.previous_was_cr {
                self.line = self.line.saturating_add(1);
            }
            self.previous_was_cr = false;
        }
        Ok(Some(byte))
    }

    fn syntax(&self, message: impl Into<String>) -> CsvError {
        CsvError::Syntax {
            line: self.line,
            message: message.into(),
        }
    }

    fn limit(&self, message: impl Into<String>) -> CsvError {
        CsvError::Limit {
            line: self.line,
            message: message.into(),
        }
    }

    /// Reads the next logical CSV record. Empty physical lines yield empty
    /// records; EOF immediately after a record terminator yields `None`.
    pub fn next_record(&mut self) -> Result<Option<Vec<String>>, CsvError> {
        let start_line = self.line;
        let mut state = State::FieldStart;
        let mut field = Vec::new();
        let mut record = Vec::new();
        let mut record_bytes = 0usize;
        let mut saw_anything = false;

        loop {
            let Some(byte) = self.read_byte()? else {
                if state == State::Quoted {
                    return Err(self.syntax("unterminated quoted field"));
                }
                if !saw_anything && record.is_empty() && field.is_empty() {
                    // A pending CRLF can leave EOF after an already emitted row.
                    self.skip_lf = false;
                    return Ok(None);
                }
                return finish_record(record, field, start_line, self.limits).map(Some);
            };

            if self.skip_lf {
                self.skip_lf = false;
                if byte == b'\n' {
                    continue;
                }
            }

            let is_record_end = state != State::Quoted && (byte == b'\n' || byte == b'\r');
            if !is_record_end {
                saw_anything = true;
                record_bytes = record_bytes
                    .checked_add(1)
                    .ok_or_else(|| self.limit("record is too large"))?;
                if record_bytes > self.limits.max_record_bytes {
                    return Err(self.limit(format!(
                        "record exceeds {} bytes",
                        self.limits.max_record_bytes
                    )));
                }
            }

            match state {
                State::FieldStart => match byte {
                    b'"' => state = State::Quoted,
                    b',' => finish_field(&mut record, &mut field, start_line, self.limits)?,
                    b'\n' | b'\r' => {
                        self.skip_lf = byte == b'\r';
                        if !saw_anything && record.is_empty() && field.is_empty() {
                            return Ok(Some(Vec::new()));
                        }
                        return finish_record(record, field, start_line, self.limits).map(Some);
                    }
                    _ => {
                        push_field_byte(&mut field, byte, start_line, self.limits)?;
                        state = State::Unquoted;
                    }
                },
                State::Unquoted => match byte {
                    b',' => {
                        finish_field(&mut record, &mut field, start_line, self.limits)?;
                        state = State::FieldStart;
                    }
                    b'\n' | b'\r' => {
                        self.skip_lf = byte == b'\r';
                        return finish_record(record, field, start_line, self.limits).map(Some);
                    }
                    _ => push_field_byte(&mut field, byte, start_line, self.limits)?,
                },
                State::Quoted => match byte {
                    b'"' => state = State::AfterQuote,
                    _ => push_field_byte(&mut field, byte, start_line, self.limits)?,
                },
                State::AfterQuote => match byte {
                    b'"' => {
                        push_field_byte(&mut field, b'"', start_line, self.limits)?;
                        state = State::Quoted;
                    }
                    b',' => {
                        finish_field(&mut record, &mut field, start_line, self.limits)?;
                        state = State::FieldStart;
                    }
                    b'\n' | b'\r' => {
                        self.skip_lf = byte == b'\r';
                        return finish_record(record, field, start_line, self.limits).map(Some);
                    }
                    _ => {
                        return Err(self
                            .syntax("expected a comma or record terminator after closing quote"));
                    }
                },
            }
        }
    }
}

fn push_field_byte(
    field: &mut Vec<u8>,
    byte: u8,
    line: u64,
    limits: CsvLimits,
) -> Result<(), CsvError> {
    if field.len() == limits.max_field_bytes {
        return Err(CsvError::Limit {
            line,
            message: format!("field exceeds {} bytes", limits.max_field_bytes),
        });
    }
    field.push(byte);
    Ok(())
}

fn finish_field(
    record: &mut Vec<Vec<u8>>,
    field: &mut Vec<u8>,
    line: u64,
    limits: CsvLimits,
) -> Result<(), CsvError> {
    if record.len() == limits.max_columns {
        return Err(CsvError::Limit {
            line,
            message: format!("record exceeds {} columns", limits.max_columns),
        });
    }
    record.push(std::mem::take(field));
    Ok(())
}

pub fn write_csv_row(out: &mut impl Write, row: &[String]) -> io::Result<()> {
    for (index, field) in row.iter().enumerate() {
        if index > 0 {
            out.write_all(b",")?;
        }
        if field.contains([',', '"', '\r', '\n']) || (row.len() == 1 && field.is_empty()) {
            out.write_all(b"\"")?;
            let mut start = 0;
            for (index, character) in field.char_indices() {
                if character == '"' {
                    out.write_all(&field.as_bytes()[start..index])?;
                    out.write_all(b"\"\"")?;
                    start = index + character.len_utf8();
                }
            }
            out.write_all(&field.as_bytes()[start..])?;
            out.write_all(b"\"")?;
        } else {
            out.write_all(field.as_bytes())?;
        }
    }
    out.write_all(b"\n")
}

fn finish_record(
    mut record: Vec<Vec<u8>>,
    field: Vec<u8>,
    line: u64,
    limits: CsvLimits,
) -> Result<Vec<String>, CsvError> {
    let mut field = field;
    finish_field(&mut record, &mut field, line, limits)?;
    let bytes = record;
    bytes
        .into_iter()
        .map(|field| String::from_utf8(field).map_err(|_| CsvError::InvalidUtf8 { line }))
        .collect()
}
