use std::io::Write;
use std::process::{Command, Stdio};

fn run(args: &[&str], input: &[u8]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sifr-query"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn assert_failure(args: &[&str], input: &[u8], message: &str) {
    let output = run(args, input);
    assert_eq!(
        output.status.code(),
        Some(2),
        "args={args:?}, output={output:?}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(message), "args={args:?}, stderr={stderr}");
    assert!(!stderr.to_lowercase().contains("panicked"), "{stderr}");
}

#[test]
fn malformed_empty_ragged_and_numeric_inputs_exit_two() {
    assert_failure(&[], b"a,b\n\"unterminated,1\n", "unterminated quoted field");
    assert_failure(&[], b"a,b\n\"closed\"junk,1\n", "expected a comma");
    assert_failure(&[], b"", "empty input");
    assert_failure(&[], b"\n", "empty CSV header");
    assert_failure(
        &["filter", "b", "=", "1"],
        b"a,b\n1\n",
        "record has 1 fields",
    );
    assert_failure(&[], b"a,b\n\xff,1\n", "valid UTF-8");
    let oversized = [
        b"a\n".as_slice(),
        vec![b'x'; 1024 * 1024 + 1].as_slice(),
        b"\n",
    ]
    .concat();
    assert_failure(&[], &oversized, "field exceeds 1048576 bytes");
    assert_failure(&["top", "1", "b"], b"a,b\n1,oops\n", "finite number");
    assert_failure(&["top", "1", "b"], b"a,b\n1,NaN\n", "finite number");
    assert_failure(
        &["group-by", "b", "sum"],
        b"a,b\n1,not-a-number\n",
        "finite number",
    );
    assert_failure(
        &["group-by", "b", "sum"],
        b"a,b\n1,1e999\n",
        "finite number",
    );
    assert_failure(
        &["group-by", "b", "sum"],
        b"a,b\n1,1e308\n1,1e308\n",
        "numeric overflow",
    );
}

#[test]
fn bad_arguments_and_io_failures_do_not_panic() {
    assert_failure(&["top", "-1", "a"], b"a\n1\n", "non-negative integer");
    assert_failure(
        &["top", "184467440737095516160", "a"],
        b"a\n1\n",
        "non-negative integer",
    );
    assert_failure(
        &["filter", "a", "bogus", "1"],
        b"a\n1\n",
        "unknown filter operator",
    );
    assert_failure(&["filter", "a", ">", "word"], b"a\n1\n", "finite number");
    assert_failure(&["group-by", "a", "median"], b"a\n1\n", "unknown aggregate");
    assert_failure(&["count", "select", "a"], b"a\n1\n", "final command");
    assert_failure(&["select", "a,,b"], b"a,b\n1,2\n", "non-empty");
    assert_failure(&["select", "missing"], b"a,b\n1,2\n", "unknown column");
    assert_failure(&["definitely-does-not-exist.csv"], b"", "cannot read");
    assert_failure(&["input.csv", "filter", "a"], b"", "missing operator");
}

#[test]
fn valid_cli_operations_preserve_csv_and_order() {
    let input = b"id,note,value\nfirst,\"a,b\",3\nsecond,\"x\r\ny\",1\nthird,\"a\"\"b\",3\n";
    let output = run(&["filter", "value", ">=", "1", "top", "2", "value"], input);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        output.stdout,
        b"id,note,value\nfirst,\"a,b\",3\nthird,\"a\"\"b\",3\n"
    );
    let output = run(&["count"], input);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"3\n");
}

#[cfg(unix)]
#[test]
fn closed_stdout_is_a_clean_success_not_a_panic() {
    use std::os::unix::io::FromRawFd;

    let mut descriptors = [0; 2];
    assert_eq!(unsafe { libc_pipe(descriptors.as_mut_ptr()) }, 0);
    let (read_fd, write_fd) = (descriptors[0], descriptors[1]);
    unsafe { libc_close(read_fd) };
    let write = unsafe { Stdio::from_raw_fd(write_fd) };
    let mut child = Command::new(env!("CARGO_BIN_EXE_sifr-query"))
        .stdin(Stdio::piped())
        .stdout(write)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input = b"a\n1\n2\n".repeat(4096);
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
}

#[cfg(unix)]
#[test]
fn streaming_stays_bounded_under_a_hard_address_space_limit() {
    let script = r#"
import os, resource, subprocess, sys, threading
resource.setrlimit(resource.RLIMIT_AS, (64 * 1024 * 1024,) * 2)
read_fd, write_fd = os.pipe()
rows = 250_000

def feed():
    with os.fdopen(write_fd, "wb") as stream:
        stream.write(b"id,note,value\n")
        for index in range(rows):
            stream.write(f'{index},"{index},x",{index % 1000}\n'.encode())

thread = threading.Thread(target=feed)
thread.start()
result = subprocess.run(
    [sys.argv[1], "filter", "value", ">=", "500", "top", "8", "value"],
    stdin=read_fd, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
)
os.close(read_fd)
thread.join()
assert result.returncode == 0, result.stderr
assert result.stdout.startswith(b"id,note,value\n999,")
assert len(result.stdout.splitlines()) == 9
print(f"bounded streaming: {rows} rows, top 8, 64 MiB address-space limit passed")
"#;
    let output = Command::new("python3")
        .args(["-c", script, env!("CARGO_BIN_EXE_sifr-query")])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
}

#[cfg(target_os = "linux")]
#[test]
fn non_pipe_stdout_failures_exit_two_without_panicking() {
    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sifr-query"))
        .stdin(Stdio::piped())
        .stdout(full)
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child.stdin.take().unwrap().write_all(b"a\n1\n")?;
            child.wait_with_output()
        })
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("output write failed"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

#[cfg(unix)]
extern "C" {
    #[link_name = "pipe"]
    fn libc_pipe(descriptors: *mut i32) -> i32;
    #[link_name = "close"]
    fn libc_close(descriptor: i32) -> i32;
}
