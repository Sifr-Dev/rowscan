"""Seeded differential oracle; invoked by tests/differential.rs with binary argv[1]."""
import csv
import io
import math
import random
import subprocess
import sys

BINARY = sys.argv[1]
SEED = 0x5F17C5
rng = random.Random(SEED)


def run(data, *args):
    return subprocess.run(
        [BINARY, *args], input=data, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, timeout=5, check=False,
    )


def encode(rows):
    stream = io.StringIO(newline="")
    csv.writer(stream, lineterminator="\n").writerows(rows)
    return stream.getvalue().encode()


def parse(data):
    text = data.decode("utf-8")
    return list(csv.reader(io.StringIO(text, newline=""), strict=True))


def compare(data, label):
    try:
        rows = parse(data)
    except (csv.Error, UnicodeError):
        expected_error = True
    else:
        expected_error = not rows or not rows[0]
        width = len(rows[0]) if rows else 0
        expected_error = expected_error or any(len(row) not in (0, width) for row in rows[1:])
    result = run(data)
    if expected_error:
        assert result.returncode == 2, (
            label, data, result.returncode, result.stdout, result.stderr,
        )
        assert b"panicked" not in result.stderr, (label, result.stderr)
        return
    assert result.returncode == 0, (label, data, result.stderr)
    expected = encode([rows[0], *(row for row in rows[1:] if row)])
    assert result.stdout == expected, (
        label, data, result.stdout, expected, result.stderr,
    )


alphabet = "ab01 ,\"\r\n"
for index in range(350):
    data = "".join(rng.choice(alphabet) for _ in range(rng.randrange(0, 90))).encode()
    compare(data, f"arbitrary-{index}")

fields = ["", "a", "a,b", '"', 'a"b', "line\nfeed", "carriage\rreturn",
          "both\r\n", "  spaced  ", "κόσμε", "🙂", "1e3", "-0", "null"]
for index in range(250):
    width = rng.randrange(1, 7)
    rows = [[rng.choice(fields) for _ in range(width)]
            for _ in range(rng.randrange(1, 18))]
    stream = io.StringIO(newline="")
    csv.writer(stream, lineterminator=rng.choice(["\n", "\r\n", "\r"])).writerows(rows)
    compare(stream.getvalue().encode("utf-8"), f"structured-{index}")

# The Python oracle also covers filtering and stable top-N over the same fixtures.
for index in range(100):
    rows = [["id", "group", "value"]]
    source = []
    for row_id in range(rng.randrange(0, 45)):
        value = rng.randrange(-1000, 1001)
        group = rng.choice(["a", "b", "with,comma", "line\nbreak"])
        source.append((str(row_id), group, value))
        rows.append([str(row_id), group, str(value)])
    data = encode(rows)
    limit = rng.randrange(0, 15)
    result = run(data, "filter", "value", ">=", "-500", "top", str(limit), "value")
    expected_rows = [row for row in source if row[2] >= -500]
    expected_rows.sort(key=lambda row: -row[2])
    expected_rows = [[row_id, group, str(value)] for row_id, group, value in expected_rows[:limit]]
    assert result.returncode == 0, (index, result.stderr)
    assert result.stdout == encode([rows[0], *expected_rows]), (
        index, result.stdout, encode([rows[0], *expected_rows]),
    )

    result = run(data, "group-by", "value", "sum")
    sums = {}
    for _, _, value in source:
        sums[value] = sums.get(value, 0) + value
    expected = [["value", "sum"]]
    # Group ordering is lexicographic by the original CSV key, not numeric.
    expected += [[str(key), str(sums[key])] for key in sorted(sums, key=str)]
    assert result.returncode == 0, (index, result.stderr)
    assert result.stdout == encode(expected), (index, result.stdout, encode(expected))

    result = run(data, "group-by", "group", "count")
    counts = {}
    for _, group, _ in source:
        counts[group] = counts.get(group, 0) + 1
    expected = [["group", "count"]]
    expected += [[key, str(counts[key])] for key in sorted(counts)]
    assert result.returncode == 0, (index, result.stderr)
    assert result.stdout == encode(expected), (index, result.stdout, encode(expected))


# Markdown is presentation output, so it is checked by decoding each emitted
# table row back into cells with a CommonMark-faithful decoder rather than by
# byte comparison. This is what catches a field that leaks a structural '|'.
def decode_markdown_row(line):
    assert line.startswith("|") and line.endswith("|"), line
    body = line[1:-1]
    cells, cell, index = [], [], 0
    while index < len(body):
        character = body[index]
        if character == "\\" and index + 1 < len(body):
            # A backslash escapes the following character (CommonMark).
            cell.append(body[index + 1])
            index += 2
            continue
        if character == "|":
            cells.append("".join(cell))
            cell = []
            index += 1
            continue
        cell.append(character)
        index += 1
    cells.append("".join(cell))

    # Every emitted cell is wrapped in exactly one leading and one trailing
    # space; strip those two, never the field's own padding.
    def unwrap(text):
        assert text.startswith(" ") and text.endswith(" "), repr(text)
        return text[1:-1].replace("<br>", "\n")

    return [unwrap(text) for text in cells]


def encode_markdown(rows):
    # Backslash is escaped first: escaping it afterwards would double the
    # backslash introduced for a pipe and break the field boundary.
    def escape(field):
        return (field.replace("\\", "\\\\").replace("|", "\\|")
                .replace("\r", "<br>").replace("\n", "<br>"))

    lines = []
    for position, row in enumerate(rows):
        lines.append("| " + " | ".join(escape(field) for field in row) + " |")
        if position == 0:
            lines.append("| " + " | ".join(["---"] * len(row)) + " |")
    return ("\n".join(lines) + "\n").encode()


markdown_fields = ["", "a|b", "c\\|d", "back\\slash", "trail\\", "\\", "|",
                   "line\nfeed", "carriage\rreturn", "both\r\n", "a\\*b",
                   "κόσμε", "🙂", "  spaced  ", "0", "-0"]
for index in range(150):
    width = rng.randrange(1, 6)
    rows = [[rng.choice(markdown_fields) for _ in range(width)]
            for _ in range(rng.randrange(1, 9))]
    data = encode(rows)
    result = run(data, "--markdown")
    assert result.returncode == 0, (index, result.stderr)
    assert result.stdout == encode_markdown(rows), (
        index, rows, result.stdout, encode_markdown(rows),
    )
    decoded = [decode_markdown_row(line)
               for line in result.stdout.decode().splitlines() if line.strip()]
    assert all(len(row) == width for row in decoded), (
        "markdown row shape changed", index, rows, decoded,
    )
    # CR and LF are both rendered as <br>, which decodes back to '\n', so the
    # comparison normalizes CR the same way before asserting values.
    expected = [[field.replace("\r", "\n") for field in row] for row in rows]
    assert decoded[0] == expected[0], (index, decoded[0], expected[0])
    assert decoded[2:] == expected[1:], (index, decoded[2:], expected[1:])

print(f"Python csv differential: 700 parser fixtures, 100 filter/top/group "
      f"fixtures, and 150 markdown fixtures passed (seed {SEED:#x})")
