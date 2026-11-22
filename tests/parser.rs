use sifr_query::csv::{write_csv_row, CsvError, CsvLimits, CsvReader};
use std::io::{BufReader, Cursor};

fn read_all(input: &[u8]) -> Result<Vec<Vec<String>>, CsvError> {
    let mut reader = CsvReader::new(BufReader::with_capacity(3, Cursor::new(input)));
    let mut rows = Vec::new();
    while let Some(row) = reader.next_record()? {
        rows.push(row);
    }
    Ok(rows)
}

fn strings(rows: &[&[&str]]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|row| row.iter().map(|field| field.to_string()).collect())
        .collect()
}

#[test]
fn quoted_delimiters_and_all_newline_styles_roundtrip() {
    let input = b"name,note\r\n\"a,b\",\"line 1\r\nline 2\rline 3\nline 4\"\r\n\"a\"\"b\",x";
    let expected = strings(&[
        &["name", "note"],
        &["a,b", "line 1\r\nline 2\rline 3\nline 4"],
        &["a\"b", "x"],
    ]);
    assert_eq!(read_all(input).unwrap(), expected);

    let mut encoded = Vec::new();
    for row in &expected {
        write_csv_row(&mut encoded, row).unwrap();
    }
    assert_eq!(read_all(&encoded).unwrap(), expected);
}

#[test]
fn tiny_buffers_and_empty_fields_match_whole_buffer_parsing() {
    let input = b"a,,c\r\n\r\n,\"\"\r\n,\n";
    let expected = strings(&[&["a", "", "c"], &[], &["", ""], &["", ""]]);
    assert_eq!(read_all(input).unwrap(), expected);
    let mut reader = CsvReader::new(Cursor::new(input));
    let mut actual = Vec::new();
    while let Some(row) = reader.next_record().unwrap() {
        actual.push(row);
    }
    assert_eq!(actual, expected);
}

#[test]
fn empty_input_and_terminal_newline_are_distinct() {
    assert!(read_all(b"").unwrap().is_empty());
    assert_eq!(read_all(b"\n").unwrap(), vec![Vec::<String>::new()]);
    assert_eq!(read_all(b"a\n").unwrap(), strings(&[&["a"]]));
    assert_eq!(read_all(b"a\r\n").unwrap(), strings(&[&["a"]]));
    assert_eq!(read_all(b"a\r").unwrap(), strings(&[&["a"]]));
    assert_eq!(read_all(b",").unwrap(), strings(&[&["", ""]]));
    assert_eq!(read_all(b"a,").unwrap(), strings(&[&["a", ""]]));
    assert_eq!(
        read_all(b"a,b\n1,\n").unwrap(),
        strings(&[&["a", "b"], &["1", ""]])
    );
    let mut one_empty_field = Vec::new();
    write_csv_row(&mut one_empty_field, &[String::new()]).unwrap();
    assert_eq!(one_empty_field, b"\"\"\n");
    assert_eq!(read_all(&one_empty_field).unwrap(), strings(&[&[""]]));
}

#[test]
fn strict_malformed_quote_errors_are_not_silently_repaired() {
    for input in [
        b"a,b\n\"unterminated,x".as_slice(),
        b"a,b\n\"closed\"junk,x".as_slice(),
    ] {
        let error = read_all(input).unwrap_err();
        assert!(matches!(error, CsvError::Syntax { .. }), "{error}");
    }
    // A quote in an unquoted field is literal, like Python csv.
    assert_eq!(read_all(b"a\"b,c\n").unwrap(), strings(&[&["a\"b", "c"]]));
}

#[test]
fn limits_cover_fields_records_and_columns() {
    let limits = CsvLimits {
        max_field_bytes: 4,
        max_record_bytes: 8,
        max_columns: 2,
    };
    for input in [
        b"abcde,x\n".as_slice(),
        b"ab,cd,efg\n".as_slice(),
        b"a,b,c\n".as_slice(),
    ] {
        let mut reader = CsvReader::with_limits(Cursor::new(input), limits);
        let error = reader.next_record().unwrap_err();
        assert!(matches!(error, CsvError::Limit { .. }), "{error}");
    }
    let mut exactly_at_limit = CsvReader::with_limits(Cursor::new(b"abcd,efg\n"), limits);
    assert_eq!(
        exactly_at_limit.next_record().unwrap(),
        Some(strings(&[&["abcd", "efg"]]).remove(0))
    );
}

#[test]
fn invalid_utf8_is_an_error_not_lossy_data() {
    let error = read_all(b"a,b\n\xff,x\n").unwrap_err();
    assert!(matches!(error, CsvError::InvalidUtf8 { .. }));
}
