use crate::csv::{write_csv_row, CsvError, CsvReader};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap};
use std::fmt;
use std::io::{self, BufRead, Write};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Operation {
    Filter {
        column: String,
        op: String,
        value: String,
    },
    Select {
        columns: Vec<String>,
    },
    GroupBy {
        column: String,
        aggregate: String,
    },
    Top {
        count: usize,
        column: String,
    },
    Count,
}

#[derive(Debug)]
pub enum QueryError {
    Input(CsvError),
    Output(io::Error),
    Invalid(String),
    NumericOverflow { group: String },
}

impl QueryError {
    pub fn is_broken_pipe(&self) -> bool {
        match self {
            Self::Input(error) => error.is_broken_pipe(),
            Self::Output(error) => error.kind() == io::ErrorKind::BrokenPipe,
            _ => false,
        }
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(error) => error.fmt(formatter),
            Self::Output(error) => write!(formatter, "output write failed: {error}"),
            Self::Invalid(message) => formatter.write_str(message),
            Self::NumericOverflow { group } => {
                write!(
                    formatter,
                    "numeric overflow in aggregate for group '{group}'"
                )
            }
        }
    }
}

impl std::error::Error for QueryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Input(error) => Some(error),
            Self::Output(error) => Some(error),
            _ => None,
        }
    }
}

impl From<CsvError> for QueryError {
    fn from(error: CsvError) -> Self {
        Self::Input(error)
    }
}

impl From<io::Error> for QueryError {
    fn from(error: io::Error) -> Self {
        Self::Output(error)
    }
}

pub fn col_idx(header: &[String], name: &str) -> Result<usize, QueryError> {
    header
        .iter()
        .position(|column| column.eq_ignore_ascii_case(name))
        .ok_or_else(|| QueryError::Invalid(format!("unknown column '{name}'")))
}

pub fn parse_number(value: &str) -> Option<f64> {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
}

fn invalid_number(value: &str, context: &str) -> QueryError {
    QueryError::Invalid(format!("{context} requires a finite number, got '{value}'"))
}

fn cmp_val(a: &str, op: &str, b: &str) -> Result<bool, QueryError> {
    if op == "contains" {
        return Ok(a.contains(b));
    }

    let (a_number, b_number) = (parse_number(a), parse_number(b));
    match op {
        "=" => Ok(match (a_number, b_number) {
            (Some(a), Some(b)) => a == b,
            _ => a == b,
        }),
        "!=" => Ok(match (a_number, b_number) {
            (Some(a), Some(b)) => a != b,
            _ => a != b,
        }),
        ">" | ">=" | "<" | "<=" => {
            let a = a_number.ok_or_else(|| invalid_number(a, "ordered filter"))?;
            let b = b_number.ok_or_else(|| invalid_number(b, "ordered filter"))?;
            Ok(match op {
                ">" => a > b,
                ">=" => a >= b,
                "<" => a < b,
                "<=" => a <= b,
                _ => unreachable!(),
            })
        }
        _ => Err(QueryError::Invalid(format!(
            "unknown filter operator '{op}' (=|!=|>|>=|<|<=|contains)"
        ))),
    }
}

#[derive(Clone, Copy)]
struct GroupState {
    count: u64,
    numeric_count: u64,
    sum: f64,
    compensation: f64,
    min: f64,
    max: f64,
}

impl GroupState {
    fn new() -> Self {
        Self {
            count: 0,
            numeric_count: 0,
            sum: 0.0,
            compensation: 0.0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
        }
    }

    fn add(&mut self, key: &str, value: Option<f64>) -> Result<(), QueryError> {
        self.count = self
            .count
            .checked_add(1)
            .ok_or_else(|| QueryError::Invalid("row count overflow".to_string()))?;
        if let Some(value) = value {
            // Neumaier compensated summation retains a low-order error term.
            let next = self.sum + value;
            if self.sum.abs() >= value.abs() {
                self.compensation += (self.sum - next) + value;
            } else {
                self.compensation += (value - next) + self.sum;
            }
            self.sum = next;
            if !self.sum.is_finite() || !self.compensation.is_finite() {
                return Err(QueryError::NumericOverflow {
                    group: key.to_string(),
                });
            }
            self.numeric_count += 1;
            self.min = self.min.min(value);
            self.max = self.max.max(value);
        }
        Ok(())
    }

    fn value(self, key: &str, aggregate: &str) -> Result<String, QueryError> {
        match aggregate {
            "count" => Ok(self.count.to_string()),
            "sum" => {
                self.require_numeric(key)?;
                let sum = self.sum + self.compensation;
                finite_result(key, sum)
            }
            "avg" => {
                self.require_numeric(key)?;
                finite_result(
                    key,
                    (self.sum + self.compensation) / self.numeric_count as f64,
                )
            }
            "min" => {
                self.require_numeric(key)?;
                Ok(fmt_num(self.min))
            }
            "max" => {
                self.require_numeric(key)?;
                Ok(fmt_num(self.max))
            }
            _ => Err(QueryError::Invalid(format!(
                "unknown aggregate '{aggregate}' (count|sum|avg|min|max)"
            ))),
        }
    }

    fn require_numeric(self, key: &str) -> Result<(), QueryError> {
        if self.numeric_count == 0 {
            Err(QueryError::Invalid(format!(
                "aggregate requires a finite numeric value in group '{key}'"
            )))
        } else {
            Ok(())
        }
    }
}

fn finite_result(group: &str, value: f64) -> Result<String, QueryError> {
    if value.is_finite() {
        Ok(fmt_num(value))
    } else {
        Err(QueryError::NumericOverflow {
            group: group.to_string(),
        })
    }
}

#[derive(Clone, Debug)]
struct SelectedRow {
    index: u64,
    value: f64,
    row: Vec<String>,
}

impl PartialEq for SelectedRow {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value && self.index == other.index
    }
}

impl Eq for SelectedRow {}

impl PartialOrd for SelectedRow {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SelectedRow {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse value order: the heap root is the worst retained row.
        // Later rows lose ties. The naturally ascending heap output is also
        // the desired descending-value, ascending-input-index output.
        other
            .value
            .partial_cmp(&self.value)
            .unwrap_or(Ordering::Equal)
            .then_with(|| self.index.cmp(&other.index))
    }
}

struct TopRows {
    count: usize,
    column: usize,
    rows: BinaryHeap<SelectedRow>,
    next_index: u64,
}

impl TopRows {
    fn new(count: usize, column: usize) -> Self {
        Self {
            count,
            column,
            rows: BinaryHeap::new(),
            next_index: 0,
        }
    }

    fn push(&mut self, row: Vec<String>) -> Result<(), QueryError> {
        if self.count == 0 {
            return Ok(());
        }
        let value = parse_number(&row[self.column])
            .ok_or_else(|| invalid_number(&row[self.column], "top"))?;
        let candidate = SelectedRow {
            index: self.next_index,
            value,
            row,
        };
        self.next_index = self
            .next_index
            .checked_add(1)
            .ok_or_else(|| QueryError::Invalid("row index overflow".to_string()))?;

        if self.rows.len() < self.count {
            self.rows.push(candidate);
        } else if self
            .rows
            .peek()
            .is_some_and(|worst| candidate.cmp(worst) == Ordering::Less)
        {
            self.rows.pop();
            self.rows.push(candidate);
        }
        Ok(())
    }

    fn into_sorted_rows(self) -> impl Iterator<Item = Vec<String>> {
        self.rows
            .into_sorted_vec()
            .into_iter()
            .map(|selected| selected.row)
    }
}

enum Stage {
    Filter {
        column: usize,
        op: String,
        value: String,
    },
    Select {
        columns: Vec<usize>,
    },
    Group {
        column: usize,
        aggregate: String,
        groups: BTreeMap<String, GroupState>,
    },
    Top(TopRows),
    Count {
        count: u64,
    },
}

/// Runs a composable query pipeline over a streaming reader. Before a group,
/// top, or count stage, at most one record is held. Groups hold distinct keys;
/// top holds at most N rows. No stage ever retains the complete input.
pub fn run_query<R: BufRead, W: Write>(
    reader: &mut CsvReader<R>,
    operations: &[Operation],
    markdown: bool,
    out: &mut W,
) -> Result<(), QueryError> {
    let mut header = match reader.next_record()? {
        Some(header) if !header.is_empty() => header,
        _ => {
            return Err(QueryError::Invalid(
                "empty input or empty CSV header".to_string(),
            ))
        }
    };
    let width = header.len();
    let mut stages = Vec::new();

    for (position, operation) in operations.iter().enumerate() {
        if matches!(operation, Operation::Count) && position + 1 != operations.len() {
            return Err(QueryError::Invalid(
                "count must be the final command".to_string(),
            ));
        }
        match operation {
            Operation::Filter { column, op, value } => {
                if !matches!(
                    op.as_str(),
                    "=" | "!=" | ">" | ">=" | "<" | "<=" | "contains"
                ) {
                    return Err(QueryError::Invalid(format!(
                        "unknown filter operator '{op}' (=|!=|>|>=|<|<=|contains)"
                    )));
                }
                if matches!(op.as_str(), ">" | ">=" | "<" | "<=") {
                    parse_number(value).ok_or_else(|| invalid_number(value, "ordered filter"))?;
                }
                stages.push(Stage::Filter {
                    column: col_idx(&header, column)?,
                    op: op.clone(),
                    value: value.clone(),
                });
            }
            Operation::Select { columns } => {
                let indices = columns
                    .iter()
                    .map(|column| col_idx(&header, column))
                    .collect::<Result<Vec<_>, _>>()?;
                header = indices.iter().map(|&index| header[index].clone()).collect();
                stages.push(Stage::Select { columns: indices });
            }
            Operation::GroupBy { column, aggregate } => {
                if !matches!(aggregate.as_str(), "count" | "sum" | "avg" | "min" | "max") {
                    return Err(QueryError::Invalid(format!(
                        "unknown aggregate '{aggregate}' (count|sum|avg|min|max)"
                    )));
                }
                let column = col_idx(&header, column)?;
                header = vec![header[column].clone(), aggregate.clone()];
                stages.push(Stage::Group {
                    column,
                    aggregate: aggregate.clone(),
                    groups: BTreeMap::new(),
                });
            }
            Operation::Top { count, column } => {
                let column = col_idx(&header, column)?;
                stages.push(Stage::Top(TopRows::new(*count, column)));
            }
            Operation::Count => {
                if counting(&stages) {
                    return Err(QueryError::Invalid(
                        "count may appear only once".to_string(),
                    ));
                }
                stages.push(Stage::Count { count: 0 });
            }
        }
    }

    if !counting(&stages) {
        emit_row(out, &header, markdown)?;
        if markdown {
            write_markdown_row(out, &vec!["---".to_string(); header.len()])?;
        }
    }

    while let Some(row) = reader.next_record()? {
        if row.is_empty() {
            continue;
        }
        if row.len() != width {
            return Err(QueryError::Invalid(format!(
                "record has {} fields; header has {width}",
                row.len()
            )));
        }
        process_row(&mut stages, 0, row, markdown, out)?;
    }

    finish_stages(&mut stages, markdown, out)?;

    if let Some(Stage::Count { count }) = stages.last() {
        writeln!(out, "{count}")?;
    }
    out.flush()?;
    Ok(())
}

fn counting(stages: &[Stage]) -> bool {
    stages
        .iter()
        .any(|stage| matches!(stage, Stage::Count { .. }))
}

fn process_row<W: Write>(
    stages: &mut [Stage],
    start: usize,
    mut row: Vec<String>,
    markdown: bool,
    out: &mut W,
) -> Result<(), QueryError> {
    for stage in &mut stages[start..] {
        match stage {
            Stage::Filter { column, op, value } => {
                if !cmp_val(&row[*column], op, value)? {
                    return Ok(());
                }
            }
            Stage::Select { columns } => {
                row = columns.iter().map(|&index| row[index].clone()).collect();
            }
            Stage::Group {
                column,
                aggregate,
                groups,
            } => {
                let key = row[*column].clone();
                let value = if aggregate == "count" {
                    None
                } else {
                    Some(parse_number(&row[*column]).ok_or_else(|| {
                        invalid_number(&row[*column], &format!("aggregate for group '{key}'"))
                    })?)
                };
                groups
                    .entry(key.clone())
                    .or_insert_with(GroupState::new)
                    .add(&key, value)?;
                return Ok(());
            }
            Stage::Top(top) => {
                top.push(row)?;
                return Ok(());
            }
            Stage::Count { count } => {
                *count = count
                    .checked_add(1)
                    .ok_or_else(|| QueryError::Invalid("row count overflow".to_string()))?;
                return Ok(());
            }
        }
    }
    emit_row(out, &row, markdown)?;
    Ok(())
}

enum BufferedRows {
    Groups {
        rows: std::collections::btree_map::IntoIter<String, GroupState>,
        aggregate: String,
    },
    Top(std::vec::IntoIter<Vec<String>>),
    None,
}

impl Iterator for BufferedRows {
    type Item = Result<Vec<String>, QueryError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Groups { rows, aggregate } => rows
                .next()
                .map(|(key, state)| Ok(vec![key.clone(), state.value(&key, aggregate)?])),
            Self::Top(rows) => rows.next().map(Ok),
            Self::None => None,
        }
    }
}

fn finish_stages<W: Write>(
    stages: &mut [Stage],
    markdown: bool,
    out: &mut W,
) -> Result<(), QueryError> {
    for index in 0..stages.len() {
        let (source, downstream) = stages.split_at_mut(index + 1);
        let rows = match &mut source[index] {
            Stage::Group {
                aggregate, groups, ..
            } => BufferedRows::Groups {
                rows: std::mem::take(groups).into_iter(),
                aggregate: aggregate.clone(),
            },
            Stage::Top(top) => {
                let top = std::mem::replace(top, TopRows::new(0, 0));
                BufferedRows::Top(top.into_sorted_rows().collect::<Vec<_>>().into_iter())
            }
            _ => BufferedRows::None,
        };
        for row in rows {
            process_row(downstream, 0, row?, markdown, out)?;
        }
    }
    Ok(())
}

fn emit_row(out: &mut impl Write, row: &[String], markdown: bool) -> Result<(), QueryError> {
    if markdown {
        write_markdown_row(out, row)?;
    } else {
        write_csv_row(out, row)?;
    }
    Ok(())
}

pub fn fmt_num(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{value:.0}")
    } else {
        format!("{value}")
    }
}

fn write_markdown_row(out: &mut impl Write, cells: &[String]) -> io::Result<()> {
    out.write_all(b"|")?;
    for cell in cells {
        out.write_all(b" ")?;
        let escaped = cell.replace('|', "\\|").replace(['\r', '\n'], "<br>");
        out.write_all(escaped.as_bytes())?;
        out.write_all(b" |")?;
    }
    out.write_all(b"\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compensated_aggregation_cancels_large_offsets() {
        let mut state = GroupState::new();
        for value in [1e16, 1.0, -1e16] {
            state.add("key", Some(value)).unwrap();
        }
        assert_eq!(state.value("key", "sum").unwrap(), "1");
        assert_eq!(state.value("key", "avg").unwrap(), "0.3333333333333333");
    }
}
