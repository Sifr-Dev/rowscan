//! sifr-query — zero-dependency streaming CSV query tool.
//!
//!   sifr-query [file.csv] [COMMAND]...
//!   commands: filter <col> <op> <val>   (ops: =, !=, >, >=, <, <=, contains)
//!             select <col>[,col]...
//!             group-by <col> <agg>      (count, sum, avg, min, max; col may be numeric)
//!             top <n> <col>             (n largest rows by col, numeric)
//!             count
//!   output: CSV to stdout (group-by emits CSV too); --markdown for a table.
use std::fs;
use std::io::{self, Read};

// ---------- CSV parsing ----------

fn parse_csv(text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut cur_field = String::new();
    let mut cur_row: Vec<String> = Vec::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                if in_quotes && chars.peek() == Some(&'"') {
                    cur_field.push('"');
                    chars.next();
                } else {
                    in_quotes = !in_quotes;
                }
            }
            ',' if !in_quotes => {
                cur_row.push(std::mem::take(&mut cur_field));
            }
            '\n' if !in_quotes => {
                cur_row.push(std::mem::take(&mut cur_field));
                rows.push(std::mem::take(&mut cur_row));
            }
            '\r' => {
                // skip; \n ends the row (handles CRLF)
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                cur_row.push(std::mem::take(&mut cur_field));
                rows.push(std::mem::take(&mut cur_row));
            }
            _ => cur_field.push(c),
        }
    }
    if !cur_field.is_empty() || !cur_row.is_empty() {
        cur_row.push(cur_field);
        rows.push(cur_row);
    }
    if in_quotes {
        return Err("unterminated quoted field".to_string());
    }
    Ok(rows)
}

fn write_csv(out: &mut impl io::Write, rows: &[Vec<String>]) -> io::Result<()> {
    for row in rows {
        let line: Vec<String> = row.iter().map(|f| csv_escape(f)).collect();
        writeln!(out, "{}", line.join(","))?;
    }
    Ok(())
}

fn csv_escape(f: &str) -> String {
    if f.contains([',', '"', '\n']) {
        format!("\"{}\"", f.replace('"', "\"\""))
    } else {
        f.to_string()
    }
}

fn col_idx(header: &[String], name: &str) -> Result<usize, String> {
    header.iter().position(|h| h.eq_ignore_ascii_case(name)).ok_or_else(|| format!("unknown column '{name}'"))
}

fn num(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok()
}

fn cmp_val(a: &str, op: &str, b: &str) -> bool {
    let (an, bn) = (num(a), num(b));
    if let (Some(x), Some(y)) = (an, bn) {
        return match op {
            "=" => x == y, "!=" => x != y,
            ">" => x > y, ">=" => x >= y,
            "<" => x < y, "<=" => x <= y,
            _ => false,
        };
    }
    match op {
        "=" => a == b,
        "!=" => a != b,
        "contains" => a.contains(b),
        _ => false,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // one pass: a token is the file unless it is consumed as an operand of a command
    const CMD_OPERANDS: [(&str, usize); 5] = [("filter", 3), ("select", 1), ("group-by", 2), ("top", 2), ("count", 0)];
    let mut consumed = vec![false; args.len()];
    let mut i = 1;
    while i < args.len() {
        if let Some((_, n)) = CMD_OPERANDS.iter().find(|(c, _)| *c == args[i]) {
            consumed[i] = true;
            for k in 1..=*n {
                if i + k < args.len() {
                    consumed[i + k] = true;
                }
            }
            i += n + 1;
        } else {
            i += 1;
        }
    }
    let mut file: Option<String> = None;
    for k in 1..args.len() {
        if consumed[k] {
            continue;
        }
        let a = args[k].as_str();
        if a.starts_with('-') {
            continue;
        }
        file = Some(a.to_string());
        break;
    }
    let text = match &file {
        Some(f) => fs::read_to_string(f).unwrap_or_else(|e| {
            eprintln!("sifr-query: cannot read {f}: {e}");
            std::process::exit(2);
        }),
        None => {
            let mut s = String::new();
            io::stdin().read_to_string(&mut s).expect("stdin read failed");
            s
        }
    };
    let rows = parse_csv(&text).unwrap_or_else(|e| {
        eprintln!("sifr-query: {e}");
        std::process::exit(2);
    });
    if rows.is_empty() {
        eprintln!("sifr-query: empty input");
        std::process::exit(2);
    }
    let mut header = rows[0].clone();
    let mut data: Vec<Vec<String>> = rows[1..].to_vec();

    // walk commands after the file arg was consumed
    let markdown = args.iter().any(|a| a == "--markdown");
    let mut j = 1;
    while j < args.len() {
        let cmd = args[j].as_str();
        match cmd {
            "--markdown" => { j += 1; }
            "filter" => {
                let col = col_idx(&header, args.get(j + 1).unwrap_or_else(|| usage())).unwrap_or_else(|e| die(&e));
                let op = args.get(j + 2).map(|s| s.as_str()).unwrap_or_else(|| usage());
                let val = args.get(j + 3).map(|s| s.as_str()).unwrap_or_else(|| usage());
                data.retain(|r| cmp_val(&r[col], op, val));
                j += 4;
            }
            "select" => {
                let list = args.get(j + 1).unwrap_or_else(|| usage());
                let cols: Result<Vec<usize>, String> = list.split(',').map(|c| col_idx(&header, c.trim())).collect();
                let cols = cols.unwrap_or_else(|e| die(&e));
                data = data.iter().map(|r| cols.iter().map(|&c| r.get(c).cloned().unwrap_or_default()).collect()).collect();
                header = cols.iter().map(|&c| header[c].clone()).collect();
                j += 2;
            }
            "group-by" => {
                let col = col_idx(&header, args.get(j + 1).unwrap_or_else(|| usage())).unwrap_or_else(|e| die(&e));
                let agg = args.get(j + 2).map(|s| s.as_str()).unwrap_or_else(|| usage()).to_string();
                // numeric column for sum/avg/min/max: use the same column
                let mut acc: std::collections::BTreeMap<String, (u64, f64, f64, f64)> = std::collections::BTreeMap::new();
                // (count, sum, min, max)
                for r in &data {
                    let key = r.get(col).cloned().unwrap_or_default();
                    let v = num(&r.get(col).cloned().unwrap_or_default());
                    let e = acc.entry(key).or_insert((0, 0.0, f64::INFINITY, f64::NEG_INFINITY));
                    e.0 += 1;
                    if let Some(x) = v {
                        e.1 += x;
                        if x < e.2 { e.2 = x; }
                        if x > e.3 { e.3 = x; }
                    }
                }
                let mut out: Vec<Vec<String>> = Vec::new();
                for (k, (count, sum, mn, mx)) in acc {
                    let val = match agg.as_str() {
                        "count" => count as f64,
                        "sum" => sum,
                        "avg" => if count > 0 { sum / count as f64 } else { 0.0 },
                        "min" => if mn.is_finite() { mn } else { 0.0 },
                        "max" => if mx.is_finite() { mx } else { 0.0 },
                        _ => die(&format!("unknown aggregate '{agg}' (count|sum|avg|min|max)")),
                    };
                    out.push(vec![k, fmt_num(val)]);
                }
                data = out;
                header = vec![header[col].clone(), agg];
                j += 3;
            }
            "top" => {
                let n: usize = args.get(j + 1).and_then(|s| s.parse().ok()).unwrap_or_else(|| usage());
                let col = col_idx(&header, args.get(j + 2).unwrap_or_else(|| usage())).unwrap_or_else(|e| die(&e));
                data.sort_by(|a, b| num(&b[col]).partial_cmp(&num(&a[col])).unwrap_or(std::cmp::Ordering::Equal));
                data.truncate(n);
                j += 3;
            }
            "count" => {
                println!("{}", data.len());
                return;
            }
            _ => { j += 1; }
        }
    }

    let mut out = io::stdout().lock();
    if markdown {
        print_md(&mut out, &header, &data).expect("write failed");
    } else {
        let mut rows = vec![header];
        rows.extend(data);
        write_csv(&mut out, &rows).expect("write failed");
    }
}

fn fmt_num(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 { format!("{:.0}", v) } else { format!("{v}") }
}

fn print_md(out: &mut impl io::Write, header: &[String], data: &[Vec<String>]) -> io::Result<()> {
    let widths: Vec<usize> = header.iter().enumerate().map(|(i, h)| {
        data.iter().map(|r| r.get(i).map(|c| c.len()).unwrap_or(0)).max().unwrap_or(0).max(h.len())
    }).collect();
    let fmt_row = |cells: &[String]| -> String {
        cells.iter().enumerate().map(|(i, c)| format!("{:width$}", c, width = widths[i])).collect::<Vec<_>>().join(" | ")
    };
    writeln!(out, "| {} |", fmt_row(header))?;
    writeln!(out, "| {} |", widths.iter().map(|w| "-".repeat(*w)).collect::<Vec<_>>().join(" | "))?;
    for r in data {
        writeln!(out, "| {} |", fmt_row(r))?;
    }
    Ok(())
}

fn usage() -> ! {
    eprintln!("usage: sifr-query [file.csv] [filter col op val] [select cols] [group-by col agg] [top n col] [count] [--markdown]");
    std::process::exit(2)
}

fn die(msg: &str) -> ! {
    eprintln!("sifr-query: {msg}");
    std::process::exit(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_crlf_and_quoted_commas() {
        let rows = parse_csv("a,b\r\n\"x,y\",2\r\n\"say \"\"hi\"\"\",3\r\n").unwrap();
        assert_eq!(rows, vec![
            vec!["a".to_string(), "b".to_string()],
            vec!["x,y".to_string(), "2".to_string()],
            vec!["say \"hi\"".to_string(), "3".to_string()],
        ]);
    }

    #[test]
    fn cmp_ops() {
        assert!(cmp_val("10", ">", "5"));
        assert!(cmp_val("10", "<=", "10"));
        assert!(cmp_val("abc", "=", "abc"));
        assert!(cmp_val("abc", "contains", "b"));
        assert!(!cmp_val("abc", ">", "a")); // non-numeric, string ops not ordered
    }

    #[test]
    fn csv_escape_roundtrip() {
        let rows = parse_csv("name\n\"a,\"\"b\"\"\"").unwrap();
        let mut buf = Vec::new();
        write_csv(&mut buf, &rows).unwrap();
        let back = String::from_utf8(buf).unwrap();
        let rows2 = parse_csv(&back).unwrap();
        assert_eq!(rows, rows2);
    }

    #[test]
    fn col_idx_case_insensitive() {
        let h = vec!["Country".to_string(), "Age".to_string()];
        assert_eq!(col_idx(&h, "country").unwrap(), 0);
        assert_eq!(col_idx(&h, "AGE").unwrap(), 1);
        assert!(col_idx(&h, "nope").is_err());
    }
}
