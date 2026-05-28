//! Data model for Simai charts.

#[derive(Debug, Clone, PartialEq)]
pub struct Bpm {
    /// 1-indexed measure where this BPM takes effect.
    pub measure: f32,
    pub bpm: f32,
}

/// Slide pattern. Mirrors the canonical Simai pattern characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlidePattern {
    /// `-` Straight line.
    Line,
    /// `^` Short arc (shortest direction).
    Caret,
    /// `<` Clockwise arc.
    Left,
    /// `>` Counter-clockwise arc.
    Right,
    /// `v` V-shape passing through the center.
    LowerV,
    /// `V<reflect>` Reflected V passing through `reflect`.
    BigV,
    /// `s` Lateral S-shape.
    S,
    /// `z` Mirrored S.
    Z,
    /// `p` Lower-half loop (counter-clockwise typically).
    P,
    /// `q` Lower-half loop (other direction).
    Q,
    /// `pp` Wider P.
    PP,
    /// `qq` Wider Q.
    QQ,
    /// `w` Wifi (3-prong fan).
    Wifi,
}

impl SlidePattern {
    pub fn as_str(&self) -> &'static str {
        match self {
            SlidePattern::Line => "-",
            SlidePattern::Caret => "^",
            SlidePattern::Left => "<",
            SlidePattern::Right => ">",
            SlidePattern::LowerV => "v",
            SlidePattern::BigV => "V",
            SlidePattern::S => "s",
            SlidePattern::Z => "z",
            SlidePattern::P => "p",
            SlidePattern::Q => "q",
            SlidePattern::PP => "pp",
            SlidePattern::QQ => "qq",
            SlidePattern::Wifi => "w",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SimaiNote {
    Tap {
        /// 1-indexed measure.
        measure: f32,
        /// Button 0..=7 (Simai's internal indexing).
        button: u8,
        is_break: bool,
        is_ex: bool,
        is_star: bool,
    },
    Hold {
        measure: f32,
        button: u8,
        /// Duration in measures.
        duration: f32,
        is_ex: bool,
    },
    Slide {
        measure: f32,
        /// Start button 0..=7.
        start: u8,
        /// End button 0..=7.
        end: u8,
        pattern: SlidePattern,
        /// For `BigV` only: 0..=7. None for other patterns.
        reflect: Option<u8>,
        /// Travel time in measures (does NOT include `delay`).
        duration: f32,
        /// Pre-roll delay in measures (default 0.25).
        delay: f32,
        is_break: bool,
        is_ex: bool,
        /// Slide had no companion star (modifier `?`/`!`/`$`).
        is_tapless: bool,
        /// Chained arcs after the first: `(pattern, end_button, reflect)`.
        /// e.g. `4<6-2` has chain = `[(Line, 1, None)]` (end=1 is 0-indexed for button 2).
        chain: Vec<(SlidePattern, u8, Option<u8>)>,
    },
    TouchTap {
        measure: f32,
        /// `'A'`, `'B'`, `'C'`, `'D'`, or `'E'`.
        region: char,
        /// 0..=7 (or 0 for `'C'`).
        position: u8,
        is_firework: bool,
    },
    TouchHold {
        measure: f32,
        region: char,
        position: u8,
        duration: f32,
        is_firework: bool,
    },
}

impl SimaiNote {
    pub fn measure(&self) -> f32 {
        match self {
            SimaiNote::Tap { measure, .. }
            | SimaiNote::Hold { measure, .. }
            | SimaiNote::Slide { measure, .. }
            | SimaiNote::TouchTap { measure, .. }
            | SimaiNote::TouchHold { measure, .. } => *measure,
        }
    }
}

/// A single Simai chart (notes + bpm changes).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SimaiChart {
    pub notes: Vec<SimaiNote>,
    pub bpms: Vec<Bpm>,
}

/// Top-level Simai file (a maidata.txt). Holds metadata and one or more charts
/// keyed by difficulty number (1..=7 for Basic..Re:Master in convention).
#[derive(Debug, Clone, Default)]
pub struct SimaiFile {
    pub title: String,
    pub artist: String,
    /// Audio offset in seconds (`&first=`).
    pub first: f32,
    /// `&lv_N=` entries (level slot, displayed level).
    pub levels: Vec<(u32, String)>,
    /// `&inote_N=` parsed charts.
    pub charts: Vec<(u32, SimaiChart)>,
    /// `&wholebpm=` if present.
    pub wholebpm: Option<f32>,
}
