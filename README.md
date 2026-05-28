# maisimai

[中文文档](README.zh-CN.md)

Pure-Rust **Simai (maimai)** chart parser and exporter, ported from the `simai` module of [MaiConverter](https://github.com/donmai-me/MaiConverter).

## Features

- Parse `maidata.txt` files (`&title`, `&artist`, `&first`, `&lv_N`, `&inote_N` metadata)
- Parse chart body: Tap, Hold, Slide, TouchTap, TouchHold
- BPM changes `(NUM)` and beat divisor `{N}` directives
- All common slide shapes: `-`, `^`, `<`, `>`, `s`, `z`, `v`, `p`, `q`, `pp`, `qq`, `V`, `w`
- Slide chaining (`*`)
- `measure ↔ seconds` timing conversion utilities
- Export internal data structures back to Simai format text

## Quick Start

```toml
# Cargo.toml
[dependencies]
maisimai = { path = "../maisimai" }
```

### Parse Chart Text

```rust
use maisimai::{parse_chart_text, SimaiChart};

let text = "(160){4}1,3,5h[4:1],7,E";
let chart: SimaiChart = parse_chart_text(text).unwrap();
println!("notes: {}", chart.notes.len());
```

### Parse a Full maidata.txt

```rust
use maisimai::parse_file;

let content = std::fs::read_to_string("maidata.txt").unwrap();
let file = parse_file(&content).unwrap();
println!("title: {}", file.title);
for (diff, chart) in &file.charts {
    println!("  diff {} — {} notes", diff, chart.notes.len());
}
```

### Export

```rust
use maisimai::{export_chart, export_file};

// Export a single chart
let simai_text = export_chart(&chart);

// Export a full file
let maidata = export_file(&file);
std::fs::write("maidata_out.txt", maidata).unwrap();
```

### Timing Conversion

```rust
use maisimai::{measure_to_seconds, seconds_to_measure, Bpm};

let bpms = vec![Bpm { measure: 1.0, bpm: 150.0 }];
let secs = measure_to_seconds(3.0, &bpms); // measure 3 → seconds
let meas = seconds_to_measure(secs, &bpms); // seconds → measure
```

## Data Model

| Type | Description |
|------|-------------|
| `SimaiFile` | Full maidata file (title, artist, offset, multi-difficulty charts) |
| `SimaiChart` | Single difficulty chart (BPM list + note list) |
| `SimaiNote` | Note enum: `Tap`, `Hold`, `Slide`, `TouchTap`, `TouchHold` |
| `SlidePattern` | Slide shape enum (Line, Caret, Left, Right, LowerV, BigV, S, Z, P, Q, Pp, Qq, Wi) |
| `Bpm` | BPM change point (measure position + BPM value) |

## Supported Simai Syntax

```
(BPM)       — BPM change
{N}         — Beat divisor (N-th notes per measure)
1-8         — Tap (button 1–8)
1h[4:1]     — Hold (duration = 1 quarter note)
1-5[8:3]    — Slide (button 1 to 5, duration = 3 eighth notes)
C / B3 / E2 — Touch (zone C / B3 / E2)
Cf          — Touch Hold (firework modifier)
1b          — Break Tap
/           — Each (simultaneous notes)
,           — Separator (advance one subdivision)
E           — End-of-chart marker
```

## License

MIT
