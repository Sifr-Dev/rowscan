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

print(f"Python csv differential: 700 parser fixtures and 100 filter/top/group fixtures passed (seed {SEED:#x})")
