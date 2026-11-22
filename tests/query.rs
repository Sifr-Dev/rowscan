use sifr_query::csv::CsvReader;
use sifr_query::query::{run_query, Operation, QueryError};
use std::io::Cursor;

fn run(input: &str, operations: Vec<Operation>) -> Result<String, QueryError> {
    let mut reader = CsvReader::new(Cursor::new(input));
    let mut output = Vec::new();
    run_query(&mut reader, &operations, false, &mut output)?;
    Ok(String::from_utf8(output).unwrap())
}

fn filter(column: &str, op: &str, value: &str) -> Operation {
    Operation::Filter {
        column: column.to_string(),
        op: op.to_string(),
        value: value.to_string(),
    }
}

fn group(column: &str, aggregate: &str) -> Operation {
    Operation::GroupBy {
        column: column.to_string(),
        aggregate: aggregate.to_string(),
    }
}

#[test]
fn top_is_descending_stable_and_bounded() {
    let input = "id,n\nfirst,3\nsecond,1\nthird,3\nfourth,2\nfifth,9\nsixth,3\n";
    let output = run(
        input,
        vec![Operation::Top {
            count: 3,
            column: "n".to_string(),
        }],
    )
    .unwrap();
    assert_eq!(output, "id,n\nfifth,9\nfirst,3\nthird,3\n");
    let output = run(
        input,
        vec![Operation::Top {
            count: 0,
            column: "n".to_string(),
        }],
    )
    .unwrap();
    assert_eq!(output, "id,n\n");
    assert_eq!(
        run(
            "id,n\nnegative,-0\npositive,0\n",
            vec![Operation::Top {
                count: 1,
                column: "n".to_string(),
            }]
        )
        .unwrap(),
        "id,n\nnegative,-0\n",
        "numerically equal signed zeroes must retain input order",
    );
}

#[test]
fn groups_are_sorted_and_composable() {
    assert_eq!(
        run("n\n1\n1\n2\n2\n", vec![group("n", "sum")]).unwrap(),
        "n,sum\n1,2\n2,4\n"
    );
    assert_eq!(
        run(
            "n\n1\n1\n2\n2\n",
            vec![group("n", "sum"), filter("sum", ">", "3"), Operation::Count],
        )
        .unwrap(),
        "1\n"
    );
    assert_eq!(
        run(
            "id,country,n\na,ke,2\nb,us,3\nc,ke,4\n",
            vec![
                filter("n", ">=", "3"),
                group("country", "count"),
                Operation::Select {
                    columns: vec!["count".to_string()],
                },
                Operation::Top {
                    count: 1,
                    column: "count".to_string(),
                },
            ],
        )
        .unwrap(),
        "count\n1\n"
    );
}

#[test]
fn numeric_and_structural_errors_are_explicit() {
    for (input, operations) in [
        ("a,b\n1\n", vec![]),
        ("a,b\n1,2,3\n", vec![]),
        ("a\noops\n", vec![group("a", "sum")]),
        (
            "a\nNaN\n",
            vec![Operation::Top {
                count: 1,
                column: "a".to_string(),
            }],
        ),
        ("a\n1e999\n", vec![group("a", "sum")]),
        ("a\n1e308\n1e308\n", vec![group("a", "sum")]),
        ("a\n1\n", vec![filter("a", ">", "invalid")]),
    ] {
        assert!(
            run(input, operations).is_err(),
            "input should fail: {input}"
        );
    }
    assert!(run("", vec![]).is_err());
    assert!(run("\n1\n", vec![]).is_err());
    assert_eq!(run("a,b\n", vec![]).unwrap(), "a,b\n");
    assert_eq!(run("a,b\n", vec![Operation::Count]).unwrap(), "0\n");
}

#[test]
fn count_is_terminal() {
    assert!(run("a\n1\n", vec![Operation::Count, filter("a", "=", "1")],).is_err());
}
