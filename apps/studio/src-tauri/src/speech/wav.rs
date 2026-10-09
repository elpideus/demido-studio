//! WAV files: how long a sound is, and a recording cut into parts a model takes one at a time.
//!
//! The composer records 16 kHz mono 16-bit PCM (see `src/voice/wav.ts`), which is what the speech
//! model reads and the smallest format every model that hears takes.

/// What the header of a WAV file says, and where its sound is.
#[derive(Clone, Debug, PartialEq)]
pub struct Wav {
    /// 1 for integer PCM.
    pub format: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub bits: u16,
    pub byte_rate: u32,
    /// Byte range of the `data` chunk, cut to what the file holds.
    pub data: std::ops::Range<usize>,
}

impl Wav {
    /// Walks a RIFF file's chunks for its format (`fmt `) and its sound (`data`). `None` when it
    /// is not a WAV file or says nothing of its format.
    pub fn parse(b: &[u8]) -> Option<Wav> {
        if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
            return None;
        }
        let u16_at = |i: usize| b.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]]));
        let u32_at = |i: usize| b.get(i..i + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
        let (mut at, mut fmt, mut data) = (12usize, None, None);
        while at + 8 <= b.len() {
            let size = u32_at(at + 4)? as usize;
            match &b[at..at + 4] {
                b"fmt " => {
                    fmt = Some((
                        u16_at(at + 8)?,
                        u16_at(at + 10)?,
                        u32_at(at + 12)?,
                        u32_at(at + 16)?,
                        u16_at(at + 22)?,
                    ))
                }
                // A recorder that never went back to write the size leaves 0 or 0xFFFFFFFF.
                b"data" => data = Some(at + 8..(at + 8).saturating_add(size).min(b.len())),
                _ => {}
            }
            at = at.saturating_add(8 + size + (size & 1));
        }
        let (format, channels, sample_rate, byte_rate, bits) = fmt?;
        Some(Wav {
            format,
            channels,
            sample_rate,
            bits,
            byte_rate,
            data: data.unwrap_or(b.len()..b.len()),
        })
    }

    /// Seconds of sound.
    pub fn seconds(&self) -> Option<f64> {
        (self.byte_rate > 0).then(|| self.data.len() as f64 / f64::from(self.byte_rate))
    }

    fn is_pcm16(&self) -> bool {
        self.format == 1 && self.bits == 16 && self.channels > 0 && self.sample_rate > 0
    }
}

/// Seconds of a WAV file's sound, from its header.
pub fn seconds(b: &[u8]) -> Option<f64> {
    Wav::parse(b)?.seconds()
}

/// A WAV file of 16-bit PCM samples (interleaved when there are several channels).
pub fn encode(samples: &[i16], sample_rate: u32, channels: u16) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * u32::from(channels) * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// A recording cut into parts of at most `max_seconds`, each a WAV file of its own, for a model
/// that hears that much at a time (Gemma 4 takes 30 seconds a clip). Each cut is in the quietest
/// 20 ms of the last fifth of a part, between words rather than through one. A recording that
/// fits, or that is not 16-bit PCM, comes back whole.
pub fn split(b: &[u8], max_seconds: u32) -> Vec<Vec<u8>> {
    let Some(wav) = Wav::parse(b) else {
        return vec![b.to_vec()];
    };
    if !wav.is_pcm16() || wav.seconds().is_none_or(|s| s <= f64::from(max_seconds)) {
        return vec![b.to_vec()];
    }
    let channels = usize::from(wav.channels);
    let samples: Vec<i16> = b[wav.data.clone()]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&s| i16::from_le_bytes(s))
        .collect();
    let frames = samples.len() / channels;
    let rate = wav.sample_rate as usize;
    let max = rate * max_seconds as usize;
    let window = (rate / 50).max(1);
    let mut parts = Vec::new();
    let mut start = 0usize;
    while frames - start > max {
        let cut = quietest(&samples, channels, start + max - max / 5, start + max, window);
        parts.push(encode(
            &samples[start * channels..cut * channels],
            wav.sample_rate,
            wav.channels,
        ));
        start = cut;
    }
    parts.push(encode(&samples[start * channels..], wav.sample_rate, wav.channels));
    parts
}

/// The middle of the quietest `window` frames between frames `from` and `to`.
fn quietest(samples: &[i16], channels: usize, from: usize, to: usize, window: usize) -> usize {
    let energy = |at: usize| -> u64 {
        samples[at * channels..(at + window) * channels]
            .iter()
            .map(|&s| (i64::from(s) * i64::from(s)) as u64)
            .sum()
    };
    let mut best = (u64::MAX, to);
    let mut at = from;
    while at + window <= to {
        let e = energy(at);
        if e < best.0 {
            best = (e, at + window / 2);
        }
        at += window;
    }
    best.1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `seconds` of a 440 Hz tone at 16 kHz, with silence over `quiet` (in seconds).
    fn tone(seconds: f64, quiet: std::ops::Range<f64>) -> Vec<i16> {
        let n = (seconds * 16_000.0) as usize;
        (0..n)
            .map(|i| {
                let t = i as f64 / 16_000.0;
                if quiet.contains(&t) {
                    0
                } else {
                    ((t * 440.0 * std::f64::consts::TAU).sin() * 8_000.0) as i16
                }
            })
            .collect()
    }

    #[test]
    fn the_length_comes_from_the_header() {
        let wav = encode(&vec![0i16; 16_000 * 3 / 2], 16_000, 1);
        assert_eq!(seconds(&wav), Some(1.5));
        let parsed = Wav::parse(&wav).unwrap();
        assert_eq!(
            (parsed.format, parsed.channels, parsed.sample_rate, parsed.bits),
            (1, 1, 16_000, 16)
        );
        assert_eq!(parsed.data, 44..wav.len());

        // A chunk before `fmt `, and a stereo file at 44.1 kHz.
        let mut odd = b"RIFF\0\0\0\0WAVELIST\x03\0\0\0abc\0".to_vec();
        let stereo = encode(&vec![0i16; 44_100 * 2], 44_100, 2);
        odd.extend_from_slice(&stereo[12..]);
        assert_eq!(seconds(&odd), Some(1.0));
        assert_eq!(seconds(b"not a wav file"), None);
        assert_eq!(seconds(b"RIFF\0\0\0\0WAVE"), None, "no format");
    }

    #[test]
    fn a_data_size_never_written_counts_to_the_end() {
        let mut wav = encode(&vec![0i16; 8_000], 16_000, 1);
        wav[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(seconds(&wav), Some(0.5));
    }

    #[test]
    fn long_recordings_are_cut_where_it_is_quiet() {
        // 70 s, quiet from 26.0 to 26.3 s and from 51.5 to 51.8 s.
        let mut samples = tone(70.0, 26.0..26.3);
        let quiet = tone(70.0, 51.5..51.8);
        samples[(51.0 * 16_000.0) as usize..].copy_from_slice(&quiet[(51.0 * 16_000.0) as usize..]);
        let wav = encode(&samples, 16_000, 1);
        let parts = split(&wav, 30);
        let lengths: Vec<f64> = parts.iter().map(|p| seconds(p).unwrap()).collect();
        assert_eq!(lengths.len(), 3, "{lengths:?}");
        assert!(lengths.iter().all(|&s| s <= 30.0), "{lengths:?}");
        assert!((26.0..26.3).contains(&lengths[0]), "{lengths:?}");
        assert!((51.5..51.8).contains(&(lengths[0] + lengths[1])), "{lengths:?}");
        assert!((lengths.iter().sum::<f64>() - 70.0).abs() < 1e-9, "nothing is lost");

        // Without a pause, at the most a part can take.
        let parts = split(&encode(&tone(45.0, 0.0..0.0), 16_000, 1), 30);
        assert_eq!(parts.len(), 2);
        assert!(seconds(&parts[0]).unwrap() <= 30.0);
    }

    #[test]
    fn what_fits_or_cannot_be_cut_stays_whole() {
        let short = encode(&tone(29.0, 0.0..0.0), 16_000, 1);
        assert_eq!(split(&short, 30), vec![short.clone()]);
        let mut float = encode(&tone(40.0, 0.0..0.0), 16_000, 1);
        float[20..22].copy_from_slice(&3u16.to_le_bytes());
        assert_eq!(split(&float, 30).len(), 1);
        assert_eq!(split(b"mp3 bytes", 30), vec![b"mp3 bytes".to_vec()]);
    }
}
