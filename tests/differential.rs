use std::process::{Command, Stdio};

const SCRIPT: &str = include_str!("differential.py");

#[test]
fn python_csv_is_the_randomized_parser_and_cli_oracle() {
    let binary = env!("CARGO_BIN_EXE_sifr-query");
    let output = Command::new("python3")
        .arg("-")
        .arg(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(SCRIPT.as_bytes())?;
            child.wait_with_output()
        })
        .unwrap();
    assert!(
        output.status.success(),
        "differential test failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
}
