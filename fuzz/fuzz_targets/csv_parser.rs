#![no_main]

use libfuzzer_sys::fuzz_target;
use sifr_query::csv::{write_csv_row, CsvLimits, CsvReader};
use std::io::{BufReader, Cursor};

fuzz_target!(|data: &[u8]| {
    let limits = CsvLimits {
        max_field_bytes: 64 * 1024,
        max_record_bytes: 128 * 1024,
        max_columns: 1024,
    };
    let mut reader = CsvReader::with_limits(
        BufReader::with_capacity(1 + data.len() % 128, Cursor::new(data)),
        limits,
    );
    let mut rows = Vec::new();
    loop {
        match reader.next_record() {
            Ok(Some(row)) => rows.push(row),
            Ok(None) => break,
            Err(_) => return,
        }
    }
    let mut encoded = Vec::new();
    for row in &rows {
        write_csv_row(&mut encoded, row).expect("writing to a vector cannot fail");
    }
    let mut second = CsvReader::new(Cursor::new(encoded));
    let mut decoded = Vec::new();
    while let Some(row) = second.next_record().expect("our writer must produce valid CSV") {
        decoded.push(row);
    }
    assert_eq!(rows, decoded, "writer/parser roundtrip mismatch");
});
