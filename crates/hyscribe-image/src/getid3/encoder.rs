// SPDX-License-Identifier: AGPL-3.0-or-later
// Ported from getID3() by James Heinrich (https://www.getid3.org), which offers its
// code under the GNU GPL version 3 among other licences; taken under that one.

//! The LAME tag of an MP3's first frame and the encoder options getID3
//! guesses from it (`GuessEncoderOptions()`).

use super::mp3::Num;

/// What a LAME tag of version 3.90 or later says.
pub(super) struct Full {
    pub vbr_quality: i64,
    /// `raw[vbr_method]`.
    pub method: i64,
    pub lowpass: i64,
    pub ath_type: i64,
    pub nogap: bool,
    pub min_bitrate: i64,
    pub bitrate_abr: i64,
    pub bitrate_min: i64,
    pub noise_shaping: i64,
    pub stereo_mode: i64,
    pub source_frequency: i64,
    pub preset: String,
    pub preset_id: i64,
    pub audio_bytes: i64,
}

impl Full {
    /// The first three characters of `vbr_method`'s name, which is the
    /// bitrate mode.
    pub(super) fn method_mode(&self) -> &'static str {
        match self.method {
            0 => "unk",
            1 | 8 => "cbr",
            2 | 9 => "abr",
            3..=6 => "vbr",
            15 => "res",
            _ => "",
        }
    }
}

/// The LAME tag.
pub(super) struct Lame {
    /// The encoder name getID3 gives: `long_version` for versions before
    /// 3.90, else the tag's `short_version`.
    pub version: Vec<u8>,
    pub full: Option<Full>,
}

/// `intval()` of the digits a component starts with.
fn intval(part: &[u8]) -> i64 {
    part.iter()
        .take_while(|b| b.is_ascii_digit())
        .fold(0, |n, &b| {
            n.saturating_mul(10).saturating_add(i64::from(b - b'0'))
        })
}

/// `LAMEpresetUsedLookup()`.
fn preset_name(id: i64, method: i64) -> String {
    let fast = if method == 4 { "fast " } else { "" };
    match id {
        0 => String::new(),
        320 => "--preset insane".into(),
        8..=319 if method == 1 => format!("--alt-preset cbr {id}"),
        8..=319 => format!("--alt-preset {id}"),
        410 => "-V9".into(),
        420 => "-V8".into(),
        440 => "-V6".into(),
        430 => "--preset radio".into(),
        450 => format!("--preset {fast}portable"),
        460 => format!("--preset {fast}medium"),
        470 | 1000 => "--r3mix".into(),
        480 => format!("--preset {fast}standard"),
        490 => "-V1".into(),
        500 => format!("--preset {fast}extreme"),
        1001 => "--alt-preset standard".into(),
        1002 => "--alt-preset extreme".into(),
        1003 => "--alt-preset insane".into(),
        1004 => "--alt-preset fast standard".into(),
        1005 => "--alt-preset fast extreme".into(),
        1006 => "--alt-preset medium".into(),
        1007 => "--alt-preset fast medium".into(),
        1010 => "--preset portable".into(),
        1015 => "--preset radio".into(),
        _ => format!("new/unknown preset: {id} - report to info@getid3.org"),
    }
}

impl Lame {
    /// The tag in the first frame's 226 bytes `window`, whose Xing or Info
    /// header sits at `id`; the window holds every field read.
    pub(super) fn parse(window: &[u8], id: usize) -> Self {
        let long = window.get(id + 120..id + 140).unwrap_or_default();
        let tail = long.get(4..).unwrap_or_default();
        let numeric_end = tail
            .iter()
            .position(|b| !(b.is_ascii_digit() || *b == b'.' || b.is_ascii_lowercase()))
            .unwrap_or(tail.len());
        let numeric = tail.get(..numeric_end).unwrap_or_default();
        let mut lame = Self {
            version: long.to_vec(),
            full: None,
        };
        let mut parts = numeric.split(|&b| b == b'.').map(intval);
        let (major, minor) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
        if !numeric.is_empty() && major.saturating_mul(1000).saturating_add(minor) >= 3090 {
            lame.read(window, id);
        }
        lame
    }

    /// The fields of a tag of version 3.90 or later, at the offsets of a
    /// Xing header at 0x24.
    fn read(&mut self, window: &[u8], id: usize) {
        let field = |offset: usize, len: usize| {
            let from = id + offset - 0x24;
            let bytes = window.get(from..from + len).unwrap_or_default();
            bytes.iter().fold(0, |n, &b| n * 256 + i64::from(b))
        };
        let from = id + 0x9c - 0x24;
        let mut short = window.get(from..from + 9).unwrap_or_default().to_vec();
        let method = field(0xa5, 1) & 0x0f;
        let flags = field(0xaf, 1);
        let min_bitrate = field(0xb0, 1);
        let misc = field(0xb4, 1);
        let preset_id = field(0xb6, 2) & 0x07ff;
        if short == b"LAME3.90." && preset_id != 0 {
            short = b"LAME3.90.3".to_vec();
        }
        self.version = short;
        self.full = Some(Full {
            vbr_quality: field(0x9b, 1),
            method,
            lowpass: field(0xa6, 1) * 100,
            ath_type: flags & 0x0f,
            nogap: flags & 0xc0 != 0,
            min_bitrate,
            bitrate_abr: if method == 2 { min_bitrate } else { 0 },
            bitrate_min: if method != 2 && method != 1 {
                min_bitrate
            } else {
                0
            },
            noise_shaping: misc & 3,
            stereo_mode: (misc >> 2) & 7,
            source_frequency: misc >> 6,
            preset: preset_name(preset_id, method),
            preset_id,
            audio_bytes: field(0xb8, 4),
        });
    }
}

/// Known LAME settings: the lowest bitrate (`None` for any), the quality,
/// the method, the noise shaping, the stereo mode, the ATH type, the
/// lowpass and the name.
type Known = (Option<i64>, i64, i64, i64, i64, i64, i64, &'static str);

const KNOWN: &[Known] = &[
    (Some(255), 58, 1, 1, 3, 2, 20500, "--alt-preset insane"),
    (Some(255), 58, 1, 1, 3, 2, 20600, "--alt-preset insane"),
    (Some(255), 57, 1, 1, 3, 4, 20500, "--alt-preset insane"),
    (None, 78, 3, 2, 3, 2, 19500, "--alt-preset extreme"),
    (None, 78, 3, 2, 3, 2, 19600, "--alt-preset extreme"),
    (None, 78, 3, 1, 3, 2, 19600, "--alt-preset extreme"),
    (None, 78, 4, 2, 3, 2, 19500, "--alt-preset fast extreme"),
    (None, 78, 4, 2, 3, 2, 19600, "--alt-preset fast extreme"),
    (None, 78, 3, 2, 3, 4, 19000, "--alt-preset standard"),
    (None, 78, 3, 1, 3, 4, 19000, "--alt-preset standard"),
    (None, 78, 4, 2, 3, 4, 19000, "--alt-preset fast standard"),
    (None, 78, 4, 1, 3, 4, 19000, "--alt-preset fast standard"),
    (None, 88, 4, 1, 3, 3, 19500, "--r3mix"),
    (None, 88, 4, 1, 3, 3, 19600, "--r3mix"),
    (None, 67, 4, 1, 3, 4, 18000, "--r3mix"),
    (None, 68, 3, 2, 3, 4, 18000, "--alt-preset medium"),
    (None, 68, 4, 2, 3, 4, 18000, "--alt-preset fast medium"),
    (Some(255), 99, 1, 1, 1, 2, 0, "--preset studio"),
    (Some(255), 58, 2, 1, 3, 2, 20600, "--preset studio"),
    (Some(255), 58, 2, 1, 3, 2, 20500, "--preset studio"),
    (Some(255), 57, 2, 1, 3, 4, 20500, "--preset studio"),
    (Some(192), 88, 1, 1, 1, 2, 0, "--preset cd"),
    (Some(192), 58, 2, 2, 3, 2, 19600, "--preset cd"),
    (Some(192), 58, 2, 2, 3, 2, 19500, "--preset cd"),
    (Some(192), 57, 2, 1, 3, 4, 19500, "--preset cd"),
    (Some(160), 78, 1, 1, 3, 2, 18000, "--preset hifi"),
    (Some(160), 58, 2, 2, 3, 2, 18000, "--preset hifi"),
    (Some(160), 57, 2, 1, 3, 4, 18000, "--preset hifi"),
    (Some(128), 67, 1, 1, 3, 2, 18000, "--preset tape"),
    (Some(128), 67, 1, 1, 3, 2, 15000, "--preset radio"),
    (Some(112), 67, 1, 1, 3, 2, 15000, "--preset fm"),
    (Some(112), 58, 2, 2, 3, 2, 16000, "--preset tape/radio/fm"),
    (Some(112), 57, 2, 1, 3, 4, 16000, "--preset tape/radio/fm"),
    (Some(56), 58, 2, 2, 0, 2, 10000, "--preset voice"),
    (Some(56), 57, 2, 1, 0, 4, 15000, "--preset voice"),
    (Some(56), 57, 2, 1, 0, 4, 16000, "--preset voice"),
    (Some(40), 65, 1, 1, 0, 2, 7500, "--preset mw-us"),
    (Some(40), 65, 1, 1, 0, 2, 7600, "--preset mw-us"),
    (Some(40), 58, 2, 2, 0, 2, 7000, "--preset mw-us"),
    (Some(40), 57, 2, 1, 0, 4, 10500, "--preset mw-us"),
    (Some(40), 57, 2, 1, 0, 4, 11200, "--preset mw-us"),
    (Some(40), 57, 2, 1, 0, 4, 8800, "--preset mw-us"),
    (Some(24), 58, 2, 2, 0, 2, 4000, "--preset phon+/lw/mw-eu/sw"),
    (Some(24), 58, 2, 2, 0, 2, 3900, "--preset phon+/lw/mw-eu/sw"),
    (Some(24), 57, 2, 1, 0, 4, 5900, "--preset phon+/lw/mw-eu/sw"),
    (Some(24), 57, 2, 1, 0, 4, 6200, "--preset phon+/lw/mw-eu/sw"),
    (Some(24), 57, 2, 1, 0, 4, 3200, "--preset phon+/lw/mw-eu/sw"),
    (Some(16), 58, 2, 2, 0, 2, 3800, "--preset phone"),
    (Some(16), 58, 2, 2, 0, 2, 3700, "--preset phone"),
    (Some(16), 57, 2, 1, 0, 4, 5600, "--preset phone"),
];

/// Lowpass frequencies the presets are known to set, by name.
const EXPECTED_LOWPASS: &[(&str, i64)] = &[
    ("insane", 20500),
    ("insane", 20600),
    ("medium", 18000),
    ("fast medium", 18000),
    ("extreme", 19500),
    ("extreme", 19600),
    ("fast extreme", 19500),
    ("fast extreme", 19600),
    ("standard", 19000),
    ("fast standard", 19000),
    ("r3mix", 19500),
    ("r3mix", 19600),
    ("r3mix", 18000),
];

/// Sample rates the other presets resample to, by name.
const EXPECTED_RESAMPLED: &[(&str, i64)] = &[
    ("phon+/lw/mw-eu/sw", 16000),
    ("mw-us", 24000),
    ("mw-us", 32000),
    ("mw-us", 16000),
    ("phone", 16000),
    ("phone", 11025),
    ("radio", 32000),
    ("fm/radio", 32000),
    ("fm", 32000),
    ("voice", 32000),
];

/// The audio fields `GuessEncoderOptions()` reads.
pub(super) struct Stream<'a> {
    /// `[mpeg][audio][VBR_method] == Fraunhofer`, and `VBR_quality`.
    pub fraunhofer: Option<i64>,
    pub lame: Option<&'a Lame>,
    /// `[audio][bitrate_mode]`.
    pub mode: &'a str,
    pub bitrate: Num,
    pub sample_rate: i64,
}

/// The first two parts of `$text` split at spaces into at most four, as
/// the preset branches read them: a missing part is empty, and a second
/// part `fast` takes the third after it.
fn preset_parts(text: &str) -> (&str, String) {
    let mut parts = text.splitn(4, ' ');
    let first = parts.next().unwrap_or_default();
    let mut second = parts.next().unwrap_or_default().to_string();
    if second == "fast" {
        second.push(' ');
        second.push_str(parts.next().unwrap_or_default());
    }
    (first, second)
}

/// `GuessEncoderOptions()`.
pub(super) fn encoder_options(stream: &Stream<'_>) -> String {
    let full = stream.lame.and_then(|lame| lame.full.as_ref());
    let rounded = |divisor: f64| (stream.bitrate.float() / divisor).round() as i64;
    let mut options: String;
    if let Some(quality) = stream.fraunhofer.filter(|&quality| quality != 0) {
        options = format!("VBR q{quality}");
    } else if let Some(full) = full.filter(|full| {
        !full.preset.is_empty()
            && !matches!(
                full.preset_id,
                16 | 24 | 40 | 56 | 112 | 128 | 160 | 192 | 256
            )
    }) {
        options = full.preset.clone();
    } else if let Some(full) = full.filter(|full| full.vbr_quality != 0) {
        let exact = |wildcard: bool| {
            KNOWN.iter().find_map(|known| {
                let minimum = match wildcard {
                    true => known.0.is_none(),
                    false => known.0 == Some(full.min_bitrate),
                };
                let same = (known.1, known.2, known.3, known.4, known.5, known.6)
                    == (
                        full.vbr_quality,
                        full.method,
                        full.noise_shaping,
                        full.stereo_mode,
                        full.ath_type,
                        full.lowpass,
                    );
                (minimum && same).then_some(known.7)
            })
        };
        options = if let Some(name) = exact(false).or_else(|| exact(true)) {
            name.to_string()
        } else if stream.mode == "vbr" {
            let version = 10.0 - (full.vbr_quality as f64 / 10.0).ceil();
            let quality = 100.0 - full.vbr_quality as f64 - version * 10.0;
            format!("-V{version} -q{quality}")
        } else if stream.mode == "cbr" {
            format!("CBR{}", (stream.bitrate.float() / 1000.0).ceil())
        } else {
            stream.mode.to_uppercase()
        };
    } else if let Some(full) = full.filter(|full| full.bitrate_abr != 0) {
        options = format!("ABR{}", full.bitrate_abr);
    } else {
        options = match stream.mode {
            "cbr" => format!("CBR{}", rounded(1000.0)),
            mode => mode.to_uppercase(),
        };
    }
    if let Some(full) = full {
        if full.bitrate_min != 0 {
            options.push_str(&format!(" -b{}", full.bitrate_min));
        }
        if full.nogap {
            options.push_str(" --nogap");
        }
        if full.lowpass != 0 {
            lowpass(&mut options, full.lowpass, stream.sample_rate);
        }
        if stream.sample_rate == 44100 && full.source_frequency != 1 {
            options.push_str(" --resample 44100");
        } else if stream.sample_rate == 48000 && full.source_frequency != 2 {
            options.push_str(" --resample 48000");
        } else if stream.sample_rate < 44100 && full.source_frequency != 0 {
            resample(&mut options, stream.sample_rate);
        }
    }
    options
}

/// The lowpass branch of `GuessEncoderOptions()`.
fn lowpass(options: &mut String, lowpass: i64, sample_rate: i64) {
    let (first, mut second) = preset_parts(options);
    if first == "--r3mix" {
        second = "r3mix".into();
    }
    if !matches!(first, "--preset" | "--alt-preset" | "--r3mix") {
        return;
    }
    let named = [
        "portable",
        "medium",
        "standard",
        "extreme",
        "insane",
        "fast portable",
        "fast medium",
        "fast standard",
        "fast extreme",
        "fast insane",
        "r3mix",
    ];
    if !named.contains(&second.as_str()) {
        return;
    }
    let expected = EXPECTED_LOWPASS
        .iter()
        .any(|(name, rate)| *name == second && *rate == lowpass);
    let low = (lowpass as f64 / 1000.0).round() < (sample_rate as f64 / 2000.0).round();
    // The lowpass is a multiple of 100: below 22050 is at most 22000.
    if !expected && lowpass <= 22000 && low {
        options.push_str(&format!(" --lowpass {lowpass}"));
    }
}

/// The resampling branch for sample rates below 44100.
fn resample(options: &mut String, sample_rate: i64) {
    let mut parts = options.splitn(4, ' ');
    let (first, second) = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    );
    // The named presets are none of the expected rates' names.
    let add = match first {
        "--preset" | "--alt-preset" => !EXPECTED_RESAMPLED
            .iter()
            .any(|(name, rate)| *name == second && *rate == sample_rate),
        _ => true,
    };
    if add {
        options.push_str(&format!(" --resample {sample_rate}"));
    }
}
