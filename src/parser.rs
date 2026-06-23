//! Hand-written Simai parser. Replaces MaiConverter's Lark grammar with a
//! straight-forward lexer/parser tuned for the chart fragments seen in real
//! community charts. Sufficient for round-tripping notes, holds, slides
//! (including `V<reflect>` and chained `*` slides), touch notes, BPM
//! changes `(NUM)` and divisor changes `{N}`.

use crate::model::{Bpm, SimaiChart, SimaiFile, SimaiNote, SlidePattern};

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    /// Approximate offset within the input chart text, in chars, when known.
    pub offset: Option<usize>,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.offset {
            Some(o) => write!(f, "Simai parse error at offset {}: {}", o, self.message),
            None => write!(f, "Simai parse error: {}", self.message),
        }
    }
}

impl std::error::Error for ParseError {}

fn err<T>(message: impl Into<String>) -> Result<T, ParseError> {
    Err(ParseError { message: message.into(), offset: None })
}

/// Parse the top-level chart body of a Simai chart (the value of `&inote_N=`).
///
/// The text may contain whitespace, `\r\n`, `||...` block comments (skipped),
/// and the standard Simai chart syntax. Comma `,` separates beats. Returns a
/// `SimaiChart` whose notes have measures relative to measure `1.0` (the
/// canonical Simai start).
pub fn parse_chart_text(text: &str) -> Result<SimaiChart, ParseError> {
    // Strip whitespace and comments. MaiConverter's reference impl skips
    // any line containing `||` (used for chart maker comments), then joins
    // the rest after removing all whitespace.
    let mut cleaned = String::with_capacity(text.len());
    for line in text.lines() {
        if line.contains("||") {
            continue;
        }
        for c in line.chars() {
            if !c.is_whitespace() {
                cleaned.push(c);
            }
        }
    }

    let mut chart = SimaiChart::default();
    // Default starting state.
    let mut measure: f32 = 1.0;
    let mut divisor: f32 = 4.0; // Default 4 if not yet specified.

    // Iterate over `,`-separated fragments.
    for frag in cleaned.split(',') {
        if frag.is_empty() || frag == "E" {
            // Empty fragments still advance time by 1/divisor.
            measure += 1.0 / divisor;
            continue;
        }
        parse_fragment(frag, measure, &mut divisor, &mut chart)?;
        measure += 1.0 / divisor;
    }

    // Round measures to the same precision as MaiConverter to keep round-trips
    // stable.
    for n in chart.notes.iter_mut() {
        round_note_measure(n);
    }
    for b in chart.bpms.iter_mut() {
        b.measure = round5(b.measure);
    }
    Ok(chart)
}

fn round5(v: f32) -> f32 {
    (v * 100_000.0).round() / 100_000.0
}

fn round_note_measure(n: &mut SimaiNote) {
    match n {
        SimaiNote::Tap { measure, .. }
        | SimaiNote::Hold { measure, .. }
        | SimaiNote::Slide { measure, .. }
        | SimaiNote::TouchTap { measure, .. }
        | SimaiNote::TouchHold { measure, .. } => {
            *measure = round5(*measure);
        }
    }
}

/// Parse a single `,`-separated fragment, applying any BPM/divisor side
/// effects and pushing notes into `chart`.
fn parse_fragment(
    frag: &str,
    base_measure: f32,
    divisor: &mut f32,
    chart: &mut SimaiChart,
) -> Result<(), ParseError> {
    let bytes = frag.as_bytes();
    let mut i = 0usize;
    // `pseudo_each` offset: each `` ` `` adds one ma2-tick at 384 res.
    // MaiConverter uses 0.0027 as the increment.
    let mut pseudo_offset: f32 = 0.0;
    let mut pending_pseudo: bool = false;
    // Track buttons that already produced a star tap so we don't add a
    // duplicate when a chained slide reuses the same start.
    let mut star_buttons: Vec<u8> = Vec::new();
    let mut last_button: Option<u8> = None;

    while i < bytes.len() {
        let c = bytes[i] as char;
        match c {
            '/' => {
                i += 1;
                pseudo_offset = 0.0;
                pending_pseudo = false;
            }
            '`' => {
                pending_pseudo = true;
                i += 1;
            }
            '(' => {
                let end = find_close(bytes, i, b')')?;
                let inner = std::str::from_utf8(&bytes[i + 1..end]).unwrap_or("");
                let bpm: f32 = inner
                    .parse()
                    .map_err(|_| ParseError { message: format!("invalid bpm: {inner}"), offset: Some(i) })?;
                if bpm <= 0.0 {
                    return err(format!("non-positive bpm: {bpm}"));
                }
                chart.bpms.retain(|b| (b.measure - base_measure).abs() > 0.0001);
                chart.bpms.push(Bpm { measure: base_measure, bpm });
                i = end + 1;
            }
            '{' => {
                let end = find_close(bytes, i, b'}')?;
                let inner = std::str::from_utf8(&bytes[i + 1..end]).unwrap_or("");
                let main = inner.split('#').next().unwrap_or(inner);
                let v: f32 = main.parse().map_err(|_| ParseError {
                    message: format!("invalid divisor: {inner}"),
                    offset: Some(i),
                })?;
                if v <= 0.0 {
                    return err(format!("non-positive divisor: {v}"));
                }
                *divisor = v;
                i = end + 1;
            }
            '0'..='8' => {
                let m_offset = if pending_pseudo {
                    pseudo_offset += 0.0027;
                    pseudo_offset
                } else {
                    pseudo_offset = 0.0;
                    0.0
                };
                pending_pseudo = false;
                if bytes[i] != b'0' {
                    let digit = bytes[i] as u8 - b'1';
                    last_button = Some(digit & 0x07);
                }
                parse_button_note(bytes, &mut i, base_measure + m_offset, chart, &mut star_buttons)?;
            }
            '*' => {
                i += 1;
                let mods = read_modifiers(bytes, &mut i);
                let is_break = mods.contains('b');
                let is_ex = mods.contains('x');
                let is_tapless = mods.contains('?') || mods.contains('!');
                if let Some(btn) = last_button {
                    let m_offset = if pending_pseudo {
                        pseudo_offset += 0.0027;
                        pseudo_offset
                    } else {
                        0.0
                    };
                    pending_pseudo = false;
                    parse_slide_body(bytes, &mut i, btn, base_measure + m_offset, chart, &mut star_buttons, is_break, is_ex, is_tapless)?;
                } else {
                    return Err(ParseError {
                        message: "* without prior button".to_string(),
                        offset: Some(i),
                    });
                }
            }
            '`' => {
                pending_pseudo = true;
                i += 1;
            }
            '(' => {
                let end = find_close(bytes, i, b')')?;
                let inner = std::str::from_utf8(&bytes[i + 1..end]).unwrap_or("");
                let bpm: f32 = inner
                    .parse()
                    .map_err(|_| ParseError { message: format!("invalid bpm: {inner}"), offset: Some(i) })?;
                if bpm <= 0.0 {
                    return err(format!("non-positive bpm: {bpm}"));
                }
                // Replace any existing BPM at this measure.
                chart.bpms.retain(|b| (b.measure - base_measure).abs() > 0.0001);
                chart.bpms.push(Bpm { measure: base_measure, bpm });
                i = end + 1;
            }
            '{' => {
                let end = find_close(bytes, i, b'}')?;
                let inner = std::str::from_utf8(&bytes[i + 1..end]).unwrap_or("");
                // Accept `{N}` and `{N#M}` (rare; treat M as ignored).
                let main = inner.split('#').next().unwrap_or(inner);
                let v: f32 = main.parse().map_err(|_| ParseError {
                    message: format!("invalid divisor: {inner}"),
                    offset: Some(i),
                })?;
                if v <= 0.0 {
                    return err(format!("non-positive divisor: {v}"));
                }
                *divisor = v;
                i = end + 1;
            }
            '0'..='8' => {
                let m_offset = if pending_pseudo {
                    pseudo_offset += 0.0027;
                    pseudo_offset
                } else {
                    pseudo_offset = 0.0;
                    0.0
                };
                pending_pseudo = false;
                parse_button_note(bytes, &mut i, base_measure + m_offset, chart, &mut star_buttons)?;
            }
            'A' | 'B' | 'C' | 'D' | 'E' => {
                let m_offset = if pending_pseudo {
                    pseudo_offset += 0.0027;
                    pseudo_offset
                } else {
                    pseudo_offset = 0.0;
                    0.0
                };
                pending_pseudo = false;
                parse_touch_note(bytes, &mut i, base_measure + m_offset, chart)?;
            }
            ' ' | '\t' | '\n' | '\r' => {
                i += 1;
            }
            _ => {
                return Err(ParseError {
                    message: format!("unexpected character `{c}` in fragment `{frag}`"),
                    offset: Some(i),
                });
            }
        }
    }
    Ok(())
}

fn find_close(bytes: &[u8], open_idx: usize, close: u8) -> Result<usize, ParseError> {
    let mut j = open_idx + 1;
    while j < bytes.len() {
        if bytes[j] == close {
            return Ok(j);
        }
        j += 1;
    }
    Err(ParseError {
        message: format!("unmatched `{}`", bytes[open_idx] as char),
        offset: Some(open_idx),
    })
}

/// Read modifier characters (`bxe$h?!@*`f) starting at `i` and return them as
/// a string. Stops at the first character that is not a modifier or at EOL.
fn read_modifiers(bytes: &[u8], i: &mut usize) -> String {
    let mut s = String::new();
    while *i < bytes.len() {
        let c = bytes[*i] as char;
        if matches!(c, 'b' | 'x' | 'e' | '$' | 'h' | '?' | '!' | '@' | 'f') {
            s.push(c);
            *i += 1;
        } else {
            break;
        }
    }
    s
}

/// Parse a `[D:N]` or `[bpm#D:N]` duration block. Returns `(equivalent_bpm,
/// duration_measures)`. On a malformed block returns an error.
fn parse_duration(bytes: &[u8], i: &mut usize) -> Result<(Option<f32>, f32, Option<f32>), ParseError> {
    debug_assert_eq!(bytes[*i], b'[');
    let end = find_close(bytes, *i, b']')?;
    let inner = std::str::from_utf8(&bytes[*i + 1..end]).unwrap_or("");
    *i = end + 1;

    // Handle `##` format: delay##duration
    if let Some(idx) = inner.find("##") {
        let delay_str = &inner[..idx];
        let dur_str = &inner[idx + 2..];
        let delay: f32 = delay_str.parse().unwrap_or(0.0);
        let dur: f32 = dur_str.parse().map_err(|_| ParseError {
            message: format!("invalid ## duration: {dur_str}"),
            offset: None,
        })?;
        return Ok((None, dur, Some(delay)));
    }

    let (eq_bpm, body) = if let Some(idx) = inner.find('#') {
        let head = &inner[..idx];
        let body = &inner[idx + 1..];
        if head.is_empty() {
            // Format: `#seconds`
            let dur: f32 = body.parse().map_err(|_| ParseError {
                message: format!("invalid duration: {body}"),
                offset: None,
            })?;
            return Ok((None, dur, None));
        }
        let eq: f32 = head.parse().map_err(|_| ParseError {
            message: format!("invalid equivalent bpm: {head}"),
            offset: None,
        })?;
        (Some(eq), body)
    } else {
        (None, inner)
    };

    let mut parts = body.split(':');
    let den_str = parts.next().unwrap_or("");
    let num_str = parts.next().ok_or_else(|| ParseError {
        message: format!("duration missing `:` in `{inner}`"),
        offset: None,
    })?;
    let den: f32 = den_str
        .parse()
        .map_err(|_| ParseError { message: format!("invalid denominator: {den_str}"), offset: None })?;
    let num: f32 = num_str
        .parse()
        .map_err(|_| ParseError { message: format!("invalid numerator: {num_str}"), offset: None })?;
    if den <= 0.0 {
        return Ok((eq_bpm, 0.0, None));
    }
    Ok((eq_bpm, num / den, None))
}

fn parse_button_note(
    bytes: &[u8],
    i: &mut usize,
    measure: f32,
    chart: &mut SimaiChart,
    star_buttons: &mut Vec<u8>,
) -> Result<(), ParseError> {
    // Read leading button digit. `0` is illegal in Simai; MaiConverter
    // silently drops it. We do the same.
    let digit = bytes[*i] as char;
    *i += 1;
    if digit == '0' {
        // Skip any modifiers/duration on this malformed note.
        let _ = read_modifiers(bytes, i);
        if *i < bytes.len() && bytes[*i] == b'[' {
            let _ = parse_duration(bytes, i)?;
        }
        return Ok(());
    }
    let button: u8 = (digit as u8 - b'1') & 0x07;

    let mods = read_modifiers(bytes, i);
    let mut is_break = mods.contains('b');
    let mut is_ex = mods.contains('x');
    let is_hold = mods.contains('h');
    let is_dollar_star = mods.contains('$');
    let is_tapless = mods.contains('?') || mods.contains('!') || mods.contains('$');

    // Slide pattern char comes AFTER modifiers.
    let next = if *i < bytes.len() { bytes[*i] as char } else { '\0' };
    let is_slide_start = is_slide_pattern_char(next);

    if is_hold {
        let mut duration = 0.0;
        if *i < bytes.len() && bytes[*i] == b'[' {
            let (_eq, d, _) = parse_duration(bytes, i)?;
            duration = d;
        }
        chart.notes.push(SimaiNote::Hold { measure, button, duration, is_ex });
        return Ok(());
    }

    if is_slide_start {
        parse_slide_body(bytes, i, button, measure, chart, star_buttons, is_break, is_ex, is_tapless)?;
        return Ok(());
    }

    // Plain tap (or star without slide via `$`).
    chart.notes.push(SimaiNote::Tap {
        measure,
        button,
        is_break,
        is_ex,
        is_star: is_dollar_star,
    });
    Ok(())
}

fn parse_slide_body(
    bytes: &[u8],
    i: &mut usize,
    button: u8,
    measure: f32,
    chart: &mut SimaiChart,
    star_buttons: &mut Vec<u8>,
    mut is_break: bool,
    mut is_ex: bool,
    is_tapless: bool,
) -> Result<(), ParseError> {
    if !is_tapless && !star_buttons.contains(&button) {
        chart.notes.push(SimaiNote::Tap {
            measure,
            button,
            is_break,
            is_ex,
            is_star: true,
        });
        star_buttons.push(button);
    }

    let (first_arc, end_idx) = read_slide_arc(bytes, *i)?;
    *i = end_idx;
    let first_mods = read_modifiers(bytes, i);
    is_break = is_break || first_mods.contains('b');
    is_ex = is_ex || first_mods.contains('x');

    let mut chain: Vec<(SlidePattern, u8, Option<u8>, bool)> = Vec::new();
    loop {
        if *i >= bytes.len() { break; }
        if bytes[*i] == b'[' {
            let saved_i = *i;
            let _ = parse_duration(bytes, i)?;
            let has_chain = *i < bytes.len()
                && (bytes[*i] == b'*' || is_slide_pattern_char(bytes[*i] as char));
            if !has_chain {
                *i = saved_i;
                break;
            }
        }
        if *i >= bytes.len() { break; }
        let is_star = bytes[*i] == b'*';
        if is_star {
            *i += 1;
            let _ = read_modifiers(bytes, i);
        } else if is_slide_pattern_char(bytes[*i] as char) {
        } else {
            break;
        }
        let (arc, end_idx) = read_slide_arc(bytes, *i)?;
        *i = end_idx;
        let arc_mods = read_modifiers(bytes, i);
        is_break = is_break || arc_mods.contains('b');
        is_ex = is_ex || arc_mods.contains('x');
        chain.push((arc.pattern, arc.end, arc.reflect, is_star));
    }

    let trail_mods = read_modifiers(bytes, i);
    is_break = is_break || trail_mods.contains('b');
    is_ex = is_ex || trail_mods.contains('x');

    let mut shared_dur: Option<(Option<f32>, f32, Option<f32>)> = None;
    if *i < bytes.len() && bytes[*i] == b'[' {
        shared_dur = Some(parse_duration(bytes, i)?);
    }
    let (eq_bpm, base_dur, explicit_delay) = shared_dur.unwrap_or((None, 0.0, None));

    let cur_bpm = current_bpm_at(chart, measure);
    let (delay, dur) = if let Some(d) = explicit_delay {
        // `##` format: X and Y are in seconds, convert to measures.
        let mult = cur_bpm / 240.0;
        (d * mult, base_dur * mult)
    } else {
        scale_with_eq_bpm(eq_bpm, base_dur, cur_bpm)
    };

    chart.notes.push(SimaiNote::Slide {
        measure,
        start: button,
        end: first_arc.end,
        pattern: first_arc.pattern,
        reflect: first_arc.reflect,
        duration: dur,
        delay,
        is_break,
        is_ex,
        is_tapless,
        chain,
    });

    Ok(())
}

fn scale_with_eq_bpm(eq_bpm: Option<f32>, duration: f32, cur_bpm: f32) -> (f32, f32) {
    let default_delay = 0.25_f32;
    if let Some(eq) = eq_bpm {
        if eq > 0.0 {
            let mult = cur_bpm / eq;
            return (default_delay * mult, duration * mult);
        }
    }
    (default_delay, duration)
}

fn current_bpm_at(chart: &SimaiChart, measure: f32) -> f32 {
    let mut last = 0.0_f32;
    let mut sorted: Vec<_> = chart.bpms.iter().collect();
    sorted.sort_by(|a, b| a.measure.partial_cmp(&b.measure).unwrap_or(std::cmp::Ordering::Equal));
    for b in sorted {
        if b.measure > measure + 1e-4 {
            break;
        }
        last = b.bpm;
    }
    if last <= 0.0 { 120.0 } else { last }
}

/// Returns true if `c` is a valid slide pattern start character.
fn is_slide_pattern_char(c: char) -> bool {
    matches!(c, '-' | '^' | '<' | '>' | 's' | 'z' | 'v' | 'w' | 'p' | 'q' | 'V')
}

#[derive(Debug, Clone)]
struct SlideArc {
    pattern: SlidePattern,
    end: u8,
    reflect: Option<u8>,
}

/// Read a single slide arc starting at the pattern character. Returns the arc
/// and the index after the end button digit.
fn read_slide_arc(bytes: &[u8], mut i: usize) -> Result<(SlideArc, usize), ParseError> {
    if i >= bytes.len() {
        return err("slide arc: unexpected EOL");
    }
    let c = bytes[i] as char;
    let (pattern, advance) = match c {
        '-' => (SlidePattern::Line, 1),
        '^' => (SlidePattern::Caret, 1),
        '<' => (SlidePattern::Left, 1),
        '>' => (SlidePattern::Right, 1),
        's' => (SlidePattern::S, 1),
        'z' => (SlidePattern::Z, 1),
        'v' => (SlidePattern::LowerV, 1),
        'w' => (SlidePattern::Wifi, 1),
        'p' => {
            if i + 1 < bytes.len() && bytes[i + 1] == b'p' {
                (SlidePattern::PP, 2)
            } else {
                (SlidePattern::P, 1)
            }
        }
        'q' => {
            if i + 1 < bytes.len() && bytes[i + 1] == b'q' {
                (SlidePattern::QQ, 2)
            } else {
                (SlidePattern::Q, 1)
            }
        }
        'V' => (SlidePattern::BigV, 1),
        other => return Err(ParseError { message: format!("unknown slide pattern `{other}`"), offset: Some(i) }),
    };
    i += advance;

    let reflect = if matches!(pattern, SlidePattern::BigV) {
        if i >= bytes.len() || !(bytes[i] as char).is_ascii_digit() {
            return err("slide `V` missing reflect button");
        }
        let r = (bytes[i] as u8 - b'1') & 0x07;
        i += 1;
        Some(r)
    } else {
        None
    };

    if i >= bytes.len() || !(bytes[i] as char).is_ascii_digit() {
        return err("slide arc missing end button");
    }
    let end = (bytes[i] as u8 - b'1') & 0x07;
    i += 1;
    Ok((SlideArc { pattern, end, reflect }, i))
}


fn parse_touch_note(
    bytes: &[u8],
    i: &mut usize,
    measure: f32,
    chart: &mut SimaiChart,
) -> Result<(), ParseError> {
    let region = bytes[*i] as char;
    *i += 1;
    let position = if *i < bytes.len() && (bytes[*i] as char).is_ascii_digit() {
        let p = (bytes[*i] as u8).saturating_sub(b'1') & 0x07;
        *i += 1;
        p
    } else {
        0
    };
    let mods = read_modifiers(bytes, i);
    let is_hold = mods.contains('h');
    let is_firework = mods.contains('f');

    if is_hold {
        let mut duration = 0.0;
        if *i < bytes.len() && bytes[*i] == b'[' {
            let (_eq, d, _) = parse_duration(bytes, i)?;
            duration = d;
        }
        chart.notes.push(SimaiNote::TouchHold { measure, region, position, duration, is_firework });
    } else {
        chart.notes.push(SimaiNote::TouchTap { measure, region, position, is_firework });
    }
    Ok(())
}

// ────────────────────────── Top-level Simai file ──────────────────────────

/// Parse a Simai file (typically a `maidata.txt`) extracting metadata and
/// every `&inote_N=` chart found.
pub fn parse_file(text: &str) -> Result<SimaiFile, ParseError> {
    let mut file = SimaiFile::default();

    // Walk the source and split it into `&KEY=VALUE` blocks. A block runs
    // until the next `&` that begins a new key (newline + `&` is the safest
    // delimiter; some metadata values are multi-line).
    let mut blocks: Vec<(String, String)> = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((idx, c)) = chars.next() {
        if c == '&' {
            // Read key up to `=`.
            let mut key = String::new();
            let mut value_start = None;
            while let Some(&(j, cj)) = chars.peek() {
                if cj == '=' {
                    chars.next();
                    value_start = Some(j + cj.len_utf8());
                    break;
                } else if cj == '\n' || cj == '\r' {
                    break;
                } else {
                    key.push(cj);
                    chars.next();
                }
            }
            let Some(vs) = value_start else { continue };
            // Read value until next `\n&` boundary or EOF.
            let mut end = text.len();
            let bytes = text.as_bytes();
            let mut k = vs;
            while k < bytes.len() {
                if bytes[k] == b'\n' {
                    let mut m = k + 1;
                    // Skip blank lines & whitespace before next `&`.
                    while m < bytes.len() && (bytes[m] == b'\r' || bytes[m] == b'\n' || bytes[m] == b' ' || bytes[m] == b'\t') {
                        m += 1;
                    }
                    if m < bytes.len() && bytes[m] == b'&' {
                        end = k;
                        break;
                    }
                }
                k += 1;
            }
            let value = text[vs..end].trim_end_matches(['\r', '\n', ' ', '\t']).to_string();
            blocks.push((key, value));
            // Advance past the value.
            while let Some(&(j, _)) = chars.peek() {
                if j >= end {
                    break;
                }
                chars.next();
            }
            let _ = idx;
        }
    }

    for (key, value) in blocks {
        match key.as_str() {
            "title" => file.title = value,
            "artist" => file.artist = value,
            "first" => {
                file.first = value.parse().unwrap_or(0.0);
            }
            "wholebpm" => {
                file.wholebpm = value.parse().ok();
            }
            k if k.starts_with("lv_") => {
                if let Ok(n) = k[3..].parse::<u32>() {
                    file.levels.push((n, value));
                }
            }
            k if k.starts_with("inote_") => {
                if let Ok(n) = k[6..].parse::<u32>() {
                    let chart = parse_chart_text(&value)?;
                    file.charts.push((n, chart));
                }
            }
            _ => {}
        }
    }

    Ok(file)
}
