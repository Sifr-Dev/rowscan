//! sifr-query — zero-dependency streaming CSV query tool.
//!
//!   sifr-query [file.csv] [COMMAND]...
//!   commands: filter <col> <op> <val>   (ops: =, !=, >, >=, <, <=, contains)
//!             select <col>[,col]...
//!             group-by <col> <agg>      (count, sum, avg, min, max)
//!             top <n> <col>             (n largest rows by col, numeric)
//!             count
//!   output: CSV to stdout; --markdown for a table.
use sifr_query::csv::CsvReader;
use sifr_query::query::{run_query, Operation, QueryError};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufReader, Write};
use std::path::PathBuf;

const USAGE: &str = "usage: sifr-query [file.csv|-] [filter col op val] [select cols] [group-by col agg] [top n col] [count] [--markdown]";

struct Config {
    file: Option<PathBuf>,
    operations: Vec<Operation>,
    markdown: bool,
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Config, String> {
    let mut args = args.into_iter();
    let mut file = None;
    let mut operations = Vec::new();
    let mut markdown = false;

    while let Some(argument) = args.next() {
        let command = argument
            .to_str()
            .ok_or_else(|| "command arguments must be valid UTF-8".to_string())?;
        let mut operand = |name: &str| {
            args.next()
                .ok_or_else(|| format!("missing {name} for '{command}'"))
        };
        let text_operand = |name: &str, value: OsString| {
            value
                .into_string()
                .map_err(|_| format!("{name} must be valid UTF-8"))
        };
        match command {
            "--markdown" => markdown = true,
            "filter" => {
                let column = text_operand("column", operand("column")?)?;
                let op = text_operand("operator", operand("operator")?)?;
                let value = text_operand("value", operand("value")?)?;
                operations.push(Operation::Filter { column, op, value });
            }
            "select" => {
                let list = text_operand("column list", operand("column list")?)?;
                let columns: Vec<String> = list
                    .split(',')
                    .map(|column| column.trim().to_string())
                    .collect();
                if columns.iter().any(String::is_empty) {
                    return Err(
                        "select requires a non-empty comma-separated column list".to_string()
                    );
                }
                operations.push(Operation::Select { columns });
            }
            "group-by" => {
                let column = text_operand("column", operand("column")?)?;
                let aggregate = text_operand("aggregate", operand("aggregate")?)?;
                operations.push(Operation::GroupBy { column, aggregate });
            }
            "top" => {
                let count = text_operand("row count", operand("row count")?)?;
                let count = count.parse::<usize>().map_err(|_| {
                    format!("top row count must be a non-negative integer, got '{count}'")
                })?;
                let column = text_operand("column", operand("column")?)?;
                operations.push(Operation::Top { count, column });
            }
            "count" => operations.push(Operation::Count),
            "-" => {
                if file.is_some() {
                    return Err("input file may be specified only once".to_string());
                }
                file = Some(PathBuf::from("-"));
            }
            _ if file.is_none() => file = Some(PathBuf::from(argument)),
            _ => return Err(format!("unexpected argument '{command}'")),
        }
    }

    Ok(Config {
        file,
        operations,
        markdown,
    })
}

fn execute(config: Config) -> Result<(), String> {
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let result = match config.file {
        Some(path) if path.as_os_str() != "-" => {
            let file = File::open(&path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            let mut reader = CsvReader::new(BufReader::new(file));
            run_query(&mut reader, &config.operations, config.markdown, &mut out)
        }
        _ => {
            let stdin = io::stdin();
            let mut reader = CsvReader::new(stdin.lock());
            run_query(&mut reader, &config.operations, config.markdown, &mut out)
        }
    };
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.is_broken_pipe() => {
            // The downstream consumer has gone away: this is normal for `| head`.
            std::process::exit(0)
        }
        Err(QueryError::Output(error)) => Err(format!("output write failed: {error}")),
        Err(error) => Err(error.to_string()),
    }
}

fn main() {
    let result = parse_args(std::env::args_os().skip(1))
        .map_err(|error| format!("{error}\n{USAGE}"))
        .and_then(execute);
    if let Err(error) = result {
        let _ = writeln!(io::stderr().lock(), "sifr-query: {error}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_file_and_commands_without_operand_confusion() {
        let config = parse_args([
            "filter".into(),
            "name".into(),
            "=".into(),
            "count".into(),
            "input.csv".into(),
            "count".into(),
        ])
        .unwrap();
        assert_eq!(config.file, Some(PathBuf::from("input.csv")));
        assert_eq!(
            config.operations,
            vec![
                Operation::Filter {
                    column: "name".to_string(),
                    op: "=".to_string(),
                    value: "count".to_string(),
                },
                Operation::Count,
            ]
        );
    }

    #[test]
    fn rejects_incomplete_and_unknown_commands() {
        assert!(parse_args(["top".into(), "not-a-number".into(), "age".into()]).is_err());
        assert!(parse_args(["filter".into(), "name".into()]).is_err());
        assert!(parse_args(["one.csv".into(), "two.csv".into()]).is_err());
    }
}
