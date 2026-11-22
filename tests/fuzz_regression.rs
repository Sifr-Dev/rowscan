use sifr_query::csv::{write_csv_row, CsvLimits, CsvReader};
use std::io::{BufReader, Cursor};

#[test]
fn deterministic_arbitrary_bytes_never_panic_and_accepted_records_roundtrip() {
    let mut state = 0x4d59_5df4_d0f3_3173u64;
    for case in 0..5_000 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let length = (state as usize) % 128;
        let mut bytes = Vec::with_capacity(length);
        for _ in 0..length {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            bytes.push((state >> 56) as u8);
        }
        let limits = CsvLimits {
            max_field_bytes: 64,
            max_record_bytes: 128,
            max_columns: 32,
        };
        let mut reader = CsvReader::with_limits(
            BufReader::with_capacity(1 + case % 7, Cursor::new(&bytes)),
            limits,
        );
        let mut rows = Vec::new();
        let parsed = loop {
            match reader.next_record() {
                Ok(Some(row)) => rows.push(row),
                Ok(None) => break Ok(rows),
                Err(error) => break Err(error),
            }
        };
        let Ok(rows) = parsed else { continue };
        let mut encoded = Vec::new();
        for row in &rows {
            write_csv_row(&mut encoded, row).unwrap();
        }
        let mut second = CsvReader::new(Cursor::new(encoded));
        let mut roundtrip = Vec::new();
        while let Some(row) = second.next_record().unwrap() {
            roundtrip.push(row);
        }
        assert_eq!(
            rows, roundtrip,
            "failed roundtrip for case {case}: {bytes:?}"
        );
    }
}
