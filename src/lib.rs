//! Pure-Rust Simai (maimai) chart parser and exporter.
//!
//! This is a focused port of the parsing/exporting parts of MaiConverter
//! (`maiconverter/simai/*.py`) to native Rust. It supports the most common
//! features of the Simai format used by community charts:
//!
//!   * `&title=`, `&artist=`, `&first=`, `&lv_N=`, `&inote_N=` metadata
//!   * Tap, Hold, Slide, TouchTap, TouchHold notes
//!   * BPM `(NUM)` and divisor `{N}` directives inside the chart body
//!   * Slide patterns `-`, `^`, `<`, `>`, `s`, `z`, `v`, `p`, `q`, `pp`,
//!     `qq`, `V<reflect>`, `w`
//!
//! It deliberately ignores `&smsg`, `&des`, `&freemsg`, `&PVStart`, `&amsg_*`
//! and similar metadata that are irrelevant to playback.
//!
//! See `MaiConverter/maiconverter/simai/{simai.py,simai_parser.py,tools.py}`
//! for the original implementation.

mod model;
mod parser;
mod exporter;

pub use model::{Bpm, SimaiChart, SimaiFile, SimaiNote, SlidePattern};
pub use parser::{parse_chart_text, parse_file, ParseError};
pub use exporter::{export_chart, export_file};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_chart() {
        // (160) sets BPM; {8} sets 1/8 divisor; mix of taps, hold, slide,
        // touch, BigV reflect, and chained slide.
        let text = "(160){4}1,3,5h[4:1],7,(160){8}1-5[8:3]/3v8[4:2],,2V64[8:5]*-7[8:3],C,B3f,";
        let c = parse_chart_text(text).expect("parse");
        assert!(!c.notes.is_empty(), "expected notes");
        // Round-trip: just make sure exporter doesn't panic and produces
        // something non-empty ending in `E`.
        let out = export_chart(&c);
        assert!(out.contains("E"));
    }

    #[test]
    fn parses_minimal_file() {
        let f = parse_file(
            "&title=Demo\n&artist=Me\n&first=0\n&lv_5=10\n&inote_5=(120){4}1,2,3,4,E\n",
        )
        .expect("file");
        assert_eq!(f.title, "Demo");
        assert_eq!(f.charts.len(), 1);
        assert_eq!(f.charts[0].0, 5);
        assert!(f.charts[0].1.notes.len() >= 4);
    }
}

/// Convert a measure (1-indexed, fractional) to seconds given a list of BPM
/// changes. BPMs must be sorted ascending by `measure`. The first BPM is used
/// for any time before its anchor measure.
///
/// One measure = 4 beats = 240/bpm seconds.
pub fn measure_to_seconds(measure: f32, bpms: &[Bpm]) -> f32 {
    if bpms.is_empty() {
        return 0.0;
    }
    let mut t = 0.0_f64;
    let mut cur_m = 1.0_f64;
    let mut cur_bpm = bpms[0].bpm as f64;
    for b in bpms {
        let bm = b.measure as f64;
        if bm > measure as f64 {
            break;
        }
        if bm > cur_m {
            t += (bm - cur_m) * 240.0 / cur_bpm;
            cur_m = bm;
        }
        cur_bpm = b.bpm as f64;
    }
    t += (measure as f64 - cur_m).max(0.0) * 240.0 / cur_bpm;
    t as f32
}

/// Inverse of [`measure_to_seconds`].
pub fn seconds_to_measure(seconds: f32, bpms: &[Bpm]) -> f32 {
    if bpms.is_empty() {
        return 1.0;
    }
    let mut remain = seconds as f64;
    let mut cur_m = 1.0_f64;
    let mut cur_bpm = bpms[0].bpm as f64;
    for b in bpms.iter().skip(1) {
        let span_m = b.measure as f64 - cur_m;
        let span_s = span_m * 240.0 / cur_bpm;
        if remain <= span_s {
            return (cur_m + remain * cur_bpm / 240.0) as f32;
        }
        remain -= span_s;
        cur_m = b.measure as f64;
        cur_bpm = b.bpm as f64;
    }
    (cur_m + remain * cur_bpm / 240.0) as f32
}
