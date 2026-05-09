//! Simai chart exporter. Mirrors `MaiConverter/maiconverter/simai/tools.py`
//! and `simai.py::SimaiChart.export` but in idiomatic Rust.
//!
//! The exporter walks every distinct measure that has events, computes the
//! best-fitting divisor for each whole measure, and emits one fragment per
//! event measure followed by `,` rests to advance the time cursor.

use crate::model::{Bpm, SimaiChart, SimaiFile, SimaiNote, SlidePattern};

/// Render a single [`SimaiChart`] to a Simai chart-body string (no `&inote_=`
/// header). Output ends with `,\nE\n` like MaiConverter.
pub fn export_chart(chart: &SimaiChart) -> String {
    export_chart_with(chart, 1000)
}

fn export_chart_with(chart: &SimaiChart, max_den: u32) -> String {
    // Collect every distinct measure that has either a note or a bpm event.
    let mut measures: Vec<f32> = Vec::new();
    for n in &chart.notes {
        measures.push(n.measure());
    }
    for b in &chart.bpms {
        measures.push(b.measure);
    }
    // Always include each whole-measure boundary plus measure 1.0.
    let mut whole_set: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
    for m in &measures {
        whole_set.insert(m.floor() as i64);
    }
    for w in whole_set {
        measures.push(w as f32);
    }
    measures.push(1.0);
    measures.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    measures.dedup_by(|a, b| (*a - *b).abs() < 1e-5);

    let last_whole = measures.iter().fold(1, |acc, m| acc.max(m.floor() as i64));
    // Per-whole-measure divisor, when one fits.
    let mut whole_divisors: Vec<Option<u32>> = Vec::with_capacity((last_whole + 1) as usize);
    for w in 0..=last_whole {
        let in_w: Vec<f32> = measures
            .iter()
            .copied()
            .filter(|m| m.floor() as i64 == w)
            .collect();
        whole_divisors.push(measure_divisor(&in_w, max_den));
    }

    let mut last_measure = 1.0_f32;
    let mut measure_tick = 1.0_f32;
    let mut prev_div: Option<u32> = None;
    let mut prev_measure_int: i64 = 0;
    let mut out = String::new();

    let n_measures = measures.len();
    for (i, &cur) in measures.iter().enumerate() {
        let bpm_here: Vec<&Bpm> = chart.bpms.iter().filter(|b| (b.measure - cur).abs() < 1e-4).collect();
        let notes_here: Vec<&SimaiNote> = chart.notes.iter().filter(|n| (n.measure() - cur).abs() < 1e-4).collect();

        // Track end-of-tail for active holds/slides so we render rests up
        // to them at the end.
        for n in &notes_here {
            match n {
                SimaiNote::Hold { duration, .. } | SimaiNote::TouchHold { duration, .. } => {
                    last_measure = last_measure.max(cur + *duration);
                }
                SimaiNote::Slide { duration, delay, .. } => {
                    last_measure = last_measure.max(cur + *delay + *duration);
                }
                _ => {}
            }
        }

        let whole_div = whole_divisors.get(cur.floor() as usize).copied().flatten();

        let (whole, cur_div, rest_amount);
        if i == n_measures - 1 {
            if last_measure > cur {
                let r = compute_rest(cur, last_measure, None, prev_div.or(whole_div), max_den);
                whole = r.0;
                cur_div = r.1;
                rest_amount = r.2;
            } else {
                whole = 0;
                cur_div = prev_div.or(whole_div).unwrap_or(4);
                rest_amount = 0;
            }
        } else {
            let next_m = measures[i + 1];
            let after = if i + 2 < n_measures { Some(measures[i + 2]) } else { None };
            let r = compute_rest(cur, next_m, after, prev_div.or(whole_div), max_den);
            whole = r.0;
            cur_div = r.1;
            rest_amount = r.2;
        }

        let bpm_at_next = bpm_value_at(chart, cur + 1.0);
        let frag = render_fragment(&notes_here, &bpm_here, bpm_at_next, max_den);

        if prev_div != Some(cur_div) || (measure_tick.floor() as i64) > prev_measure_int {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("{{{cur_div}}}"));
            out.push_str(&frag);
            prev_div = Some(cur_div);
            prev_measure_int = measure_tick.floor() as i64;
        } else {
            out.push_str(&frag);
        }
        measure_tick = cur;

        for _ in 0..rest_amount {
            out.push(',');
            measure_tick += 1.0 / cur_div as f32;
        }
        if whole > 0 {
            if cur_div != 1 {
                out.push_str("{1}");
                prev_div = Some(1);
            }
            for _ in 0..whole {
                out.push(',');
                measure_tick += 1.0;
            }
        }
        measure_tick = (measure_tick * 10000.0).round() / 10000.0;
    }

    out.push_str(",\nE\n");
    out
}

/// Render every event at a single measure into a Simai fragment. Order
/// matches MaiConverter: bpm → divisor (handled outside) → taps → holds →
/// touch taps → touch holds → slides.
fn render_fragment(notes: &[&SimaiNote], bpms: &[&Bpm], bpm_for_slides: f32, max_den: u32) -> String {
    let mut out = String::new();
    let mut counter = 0u32;
    if let Some(b) = bpms.first() {
        out.push_str(&format!("({})", trim_float(b.bpm)));
    }

    // Star buttons that already produced a tap; used when emitting a slide
    // to know if we still need to write a star marker.
    let mut star_positions: Vec<u8> = Vec::new();

    let taps: Vec<&SimaiNote> = notes
        .iter()
        .copied()
        .filter(|n| matches!(n, SimaiNote::Tap { .. }))
        .collect();
    let holds: Vec<&SimaiNote> = notes
        .iter()
        .copied()
        .filter(|n| matches!(n, SimaiNote::Hold { .. }))
        .collect();
    let touch_taps: Vec<&SimaiNote> = notes
        .iter()
        .copied()
        .filter(|n| matches!(n, SimaiNote::TouchTap { .. }))
        .collect();
    let touch_holds: Vec<&SimaiNote> = notes
        .iter()
        .copied()
        .filter(|n| matches!(n, SimaiNote::TouchHold { .. }))
        .collect();
    let slides: Vec<&SimaiNote> = notes
        .iter()
        .copied()
        .filter(|n| matches!(n, SimaiNote::Slide { .. }))
        .collect();

    for n in &taps {
        if let SimaiNote::Tap { button, is_break, is_ex, is_star, .. } = n {
            // If this is a star that produces a slide later, skip it; the
            // slide writer will emit the head.
            if *is_star && slides.iter().any(|s| matches!(s, SimaiNote::Slide { start, .. } if start == button)) {
                star_positions.push(*button);
                continue;
            }
            if counter > 0 { out.push('/'); }
            let mut mods = String::new();
            if *is_break { mods.push('b'); }
            if *is_ex { mods.push('x'); }
            if *is_star { mods.push('$'); }
            out.push_str(&format!("{}{}", button + 1, mods));
            counter += 1;
        }
    }

    for n in &holds {
        if let SimaiNote::Hold { button, duration, is_ex, .. } = n {
            if counter > 0 { out.push('/'); }
            let mods = if *is_ex { "hx" } else { "h" };
            let (den, num) = float_to_fraction(*duration, max_den * 2);
            out.push_str(&format!("{}{}[{}:{}]", button + 1, mods, den, num));
            counter += 1;
        }
    }

    for n in &touch_taps {
        if let SimaiNote::TouchTap { region, position, is_firework, .. } = n {
            if counter > 0 { out.push('/'); }
            let modf = if *is_firework { "f" } else { "" };
            // C has no positional digit in canonical Simai output.
            if *region == 'C' {
                out.push_str(&format!("C{modf}"));
            } else {
                out.push_str(&format!("{}{}{}", region, position + 1, modf));
            }
            counter += 1;
        }
    }

    for n in &touch_holds {
        if let SimaiNote::TouchHold { region, position, duration, is_firework, .. } = n {
            if counter > 0 { out.push('/'); }
            let mods = if *is_firework { "hf" } else { "h" };
            let (den, num) = float_to_fraction(*duration, max_den * 2);
            if *region == 'C' {
                out.push_str(&format!("C{mods}[{den}:{num}]"));
            } else {
                out.push_str(&format!("{}{}{}[{}:{}]", region, position + 1, mods, den, num));
            }
            counter += 1;
        }
    }

    let mut written_starts: Vec<u8> = Vec::new();
    for n in &slides {
        if let SimaiNote::Slide { start, end, pattern, reflect, duration, delay, is_break, is_ex, is_tapless, .. } = n {
            if counter > 0 && !written_starts.contains(start) { out.push('/'); }
            // Star/break/ex/tapless modifier when emitting head for the first time.
            let head = if written_starts.contains(start) {
                "*".to_string()
            } else {
                format!("{}", start + 1)
            };
            let mut mods = String::new();
            if !written_starts.contains(start) {
                if *is_tapless && !star_positions.contains(start) {
                    mods.push('?');
                } else if *is_break {
                    mods.push('b');
                } else if *is_ex {
                    mods.push('x');
                }
            }
            let pat_str = match pattern {
                SlidePattern::BigV => format!("V{}", reflect.unwrap_or(*end) + 1),
                _ => pattern.as_str().to_string(),
            };
            // Duration: when delay differs from default 0.25 measures we
            // emit `[bpm#D:N]` form using the equivalent bpm trick.
            let suffix = if (delay - 0.25).abs() > 0.0025 {
                let scale = if *delay > 0.0025 { 0.25 / *delay } else { 100.0 };
                let eq_bpm = ((bpm_for_slides * scale) * 10000.0).round() / 10000.0;
                let (den, num) = float_to_fraction(*duration * scale, max_den * 10);
                format!("[{}#{}:{}]", trim_float(eq_bpm), den, num)
            } else {
                let (den, num) = float_to_fraction(*duration, max_den * 10);
                format!("[{den}:{num}]")
            };
            out.push_str(&format!("{head}{mods}{pat_str}{}{suffix}", end + 1));
            counter += 1;
            if !written_starts.contains(start) {
                written_starts.push(*start);
            }
        }
    }

    out
}

fn bpm_value_at(chart: &SimaiChart, measure: f32) -> f32 {
    let mut last = if let Some(b) = chart.bpms.first() { b.bpm } else { 120.0 };
    let mut sorted: Vec<&Bpm> = chart.bpms.iter().collect();
    sorted.sort_by(|a, b| a.measure.partial_cmp(&b.measure).unwrap_or(std::cmp::Ordering::Equal));
    for b in sorted {
        if b.measure > measure + 1e-4 { break; }
        last = b.bpm;
    }
    last
}

fn trim_float(v: f32) -> String {
    // Print without trailing zeros / unnecessary `.0`.
    let mut s = format!("{v:.4}");
    while s.contains('.') && (s.ends_with('0') || s.ends_with('.')) {
        s.pop();
    }
    s
}

// ─── Numerical helpers (rational approx, gcd, lcm, rest finder) ───────────

fn gcd_u32(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd_u32(b, a % b) }
}

fn lcm_u32(a: u32, b: u32) -> u32 {
    if a == 0 || b == 0 { 0 } else { a / gcd_u32(a, b) * b }
}

/// Approximate `value` as `num/den` with `den <= max_den`. Stern-Brocot style.
fn float_to_fraction(value: f32, max_den: u32) -> (u32, u32) {
    if !value.is_finite() || value < 0.0 {
        return (1, 0);
    }
    if value == 0.0 {
        return (1, 0);
    }
    // Continued-fraction based best rational approximation.
    let mut h0: i64 = 0;
    let mut h1: i64 = 1;
    let mut k0: i64 = 1;
    let mut k1: i64 = 0;
    let mut x = value as f64;
    for _ in 0..32 {
        let a = x.floor() as i64;
        let h2 = a * h1 + h0;
        let k2 = a * k1 + k0;
        if k2 > max_den as i64 {
            break;
        }
        h0 = h1; h1 = h2;
        k0 = k1; k1 = k2;
        let frac = x - a as f64;
        if frac.abs() < 1e-9 { break; }
        x = 1.0 / frac;
    }
    let den = k1.max(1) as u32;
    let num = h1.max(0) as u32;
    let g = gcd_u32(den, num.max(1));
    (den / g.max(1), num / g.max(1))
}

fn measure_divisor(measures: &[f32], max_den: u32) -> Option<u32> {
    if measures.is_empty() { return None; }
    let base = measures[0].floor();
    let mut prev = base;
    let mut sorted = measures.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut current_lcm: u32 = 1;
    for m in sorted {
        let frac = (m - prev).fract().abs();
        let (d, _n) = float_to_fraction(frac, max_den);
        current_lcm = lcm_u32(current_lcm, d);
        if current_lcm > 64 { return None; }
        prev = m;
    }
    Some(current_lcm)
}

fn compute_rest(
    cur: f32,
    next: f32,
    after_next: Option<f32>,
    cur_divisor: Option<u32>,
    max_den: u32,
) -> (i32, u32, i32) {
    if next < cur { return (0, cur_divisor.unwrap_or(4), 0); }
    let diff = next - cur;
    if diff < 1e-5 {
        return (0, cur_divisor.unwrap_or(4), 0);
    }
    let whole = diff.floor() as i32;
    let frac = diff - whole as f32;
    let (frac_den, frac_num) = float_to_fraction(frac, max_den);

    if let Some(cd) = cur_divisor {
        let l = lcm_u32(cd, frac_den);
        if l == cd && diff < 1.0 {
            let amount = (frac * cd as f32).round() as i32;
            return (0, cd, amount);
        }
    }

    if let Some(an) = after_next {
        if an >= next {
            let diff2 = an - next;
            let frac2 = diff2 - diff2.floor();
            let (d2, _) = float_to_fraction(frac2, max_den);
            let l = lcm_u32(frac_den, d2);
            if l <= 64 && diff < 1.0 {
                let amount = (frac * l as f32).round() as i32;
                return (0, l, amount);
            }
        }
    }

    (whole, frac_den.max(1), frac_num as i32)
}

// ─── Top-level Simai file export ───────────────────────────────────────────

/// Render an entire [`SimaiFile`] into a maidata.txt-style string.
pub fn export_file(file: &SimaiFile) -> String {
    let mut s = String::new();
    s.push_str(&format!("&title={}\n", file.title));
    s.push_str(&format!("&artist={}\n", file.artist));
    s.push_str(&format!("&first={}\n", trim_float(file.first)));
    if let Some(b) = file.wholebpm {
        s.push_str(&format!("&wholebpm={}\n", trim_float(b)));
    }
    for (n, lv) in &file.levels {
        s.push_str(&format!("&lv_{n}={lv}\n"));
    }
    for (n, chart) in &file.charts {
        s.push_str(&format!("&inote_{n}={}\n", export_chart(chart)));
    }
    s
}
