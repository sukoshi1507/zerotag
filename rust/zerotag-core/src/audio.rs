// SPDX-License-Identifier: MIT
//! Short sound effects and a looping chiptune, synthesised in code (square / triangle / noise voices) and played
//! through `aplay` (raw S16 mono on stdin): no audio library and no sound file. Everything is a no-op when sound is
//! disabled or when aplay is missing. Port of src/platform/audio.cpp.

use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::Mutex;

const RATE: u32 = 22050;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    /// "tu-du-tuuu": an amiibo was read.
    Success,
    /// Emulation started.
    Start,
    /// Something failed.
    Error,
    /// Small UI confirmation (favourite...).
    Blip,
}

#[derive(Clone, Copy)]
enum Wave {
    Square50,
    Square25,
    Triangle,
    Noise,
}

fn midi_hz(note: i32) -> f64 {
    440.0 * 2f64.powf((note - 69) as f64 / 12.0)
}

/// Adds one note to `pcm` starting at `start_ms`: linear attack of 3 ms, exponential-ish decay to silence.
fn add_note(pcm: &mut Vec<f32>, start_ms: f64, length_ms: f64, note: i32, wave: Wave, volume: f32, decay: f64) {
    let first = (start_ms * RATE as f64 / 1000.0) as usize;
    let count = (length_ms * RATE as f64 / 1000.0) as usize;
    if pcm.len() < first + count {
        pcm.resize(first + count, 0.0);
    }
    let hz = midi_hz(note);
    let mut lfsr: u32 = 0xACE1;
    let mut noise_value = 0.0f32;
    for i in 0..count {
        let t = i as f64 / RATE as f64;
        let phase = (t * hz).fract();
        let s = match wave {
            Wave::Square50 => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Square25 => {
                if phase < 0.25 {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Triangle => (if phase < 0.5 { 4.0 * phase - 1.0 } else { 3.0 - 4.0 * phase }) as f32,
            Wave::Noise => {
                if i % 6 == 0 {
                    // sample-and-hold noise, a bit crunchy
                    let bit = (lfsr ^ (lfsr >> 2) ^ (lfsr >> 3) ^ (lfsr >> 5)) & 1;
                    lfsr = (lfsr >> 1) | (bit << 15);
                    noise_value = if lfsr & 1 != 0 { 1.0 } else { -1.0 };
                }
                noise_value
            }
        };
        let attack = (t / 0.003).min(1.0);
        let env = attack * (-decay * i as f64 / count as f64).exp();
        pcm[first + i] += s * volume * env as f32;
    }
}

fn to_pcm(mix: &[f32]) -> Vec<i16> {
    mix.iter().map(|v| (v.clamp(-1.0, 1.0) * 30000.0) as i16).collect()
}

pub fn make_sound(sound: Sound) -> Vec<i16> {
    let mut m = Vec::new();
    match sound {
        Sound::Success => {
            // C5 G5 then a long C6 with a fifth under it
            add_note(&mut m, 0.0, 95.0, 72, Wave::Square25, 0.30, 2.0);
            add_note(&mut m, 100.0, 95.0, 79, Wave::Square25, 0.30, 2.0);
            add_note(&mut m, 200.0, 520.0, 84, Wave::Square25, 0.30, 2.6);
            add_note(&mut m, 200.0, 520.0, 76, Wave::Triangle, 0.30, 2.6);
        }
        Sound::Start => {
            add_note(&mut m, 0.0, 70.0, 67, Wave::Square50, 0.25, 1.5);
            add_note(&mut m, 75.0, 70.0, 72, Wave::Square50, 0.25, 1.5);
            add_note(&mut m, 150.0, 140.0, 76, Wave::Square50, 0.25, 2.5);
        }
        Sound::Error => {
            add_note(&mut m, 0.0, 120.0, 55, Wave::Square50, 0.28, 1.0);
            add_note(&mut m, 130.0, 260.0, 48, Wave::Square25, 0.28, 2.0);
        }
        Sound::Blip => add_note(&mut m, 0.0, 60.0, 88, Wave::Square25, 0.22, 3.0),
    }
    to_pcm(&m)
}

/// A 7 s loop in A minor: arpeggio, bass, hats and a small lead, tracker style (16th notes at 140 bpm).
pub fn make_music() -> Vec<i16> {
    let step = 60000.0 / 140.0 / 4.0; // ms per 16th
    // Chords (Am F C G): root note of the bass and the three notes of the arpeggio.
    const BASS: [i32; 4] = [45, 41, 48, 43];
    const CHORD: [[i32; 3]; 4] = [[57, 60, 64], [53, 57, 60], [60, 64, 67], [55, 59, 62]];
    // Lead: two bars of melody, one value per 16th (0 = rest), repeated twice with a variation.
    const LEAD: [[i32; 32]; 2] = [
        [76, 0, 0, 76, 0, 79, 0, 76, 74, 0, 0, 72, 0, 0, 0, 0, 72, 0, 0, 72, 0, 76, 0, 72, 71, 0, 0, 67, 0, 0, 0, 0],
        [81, 0, 0, 81, 0, 79, 0, 76, 77, 0, 0, 76, 0, 0, 72, 0, 76, 0, 0, 79, 0, 83, 0, 79, 74, 0, 71, 0, 67, 0, 0, 0],
    ];
    let mut m = Vec::new();
    for round in 0..2usize {
        for s in 0..32usize {
            let t = (round * 32 + s) as f64 * step;
            let c = s / 8;
            add_note(&mut m, t, step * 0.9, CHORD[c][s % 3] + 12, Wave::Square25, 0.10, 2.2); // arpeggio
            if s % 4 == 0 {
                add_note(&mut m, t, step * 3.6, BASS[c], Wave::Triangle, 0.30, 1.6); // bass
            }
            if s % 2 == 1 {
                add_note(&mut m, t, step * 0.5, 60, Wave::Noise, 0.07, 4.0); // hat
            }
            if s % 8 == 0 {
                add_note(&mut m, t, step * 1.5, 40, Wave::Noise, 0.16, 5.0); // kick-ish thump
            }
            if LEAD[round][s] != 0 {
                add_note(&mut m, t, step * 2.0, LEAD[round][s], Wave::Square50, 0.14, 2.0); // lead
            }
        }
    }
    to_pcm(&m)
}

static ENABLED: AtomicBool = AtomicBool::new(true);
static MUSIC_STOP: AtomicBool = AtomicBool::new(false);
static MUSIC_RUNNING: AtomicBool = AtomicBool::new(false);
/// The aplay child of the music loop, kept here so `stop_music` can end it.
static MUSIC_CHILD: Mutex<Option<Child>> = Mutex::new(None);

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, SeqCst);
    if !enabled {
        stop_music();
    }
}

pub fn enabled() -> bool {
    ENABLED.load(SeqCst)
}

fn spawn_aplay() -> Option<(Child, std::process::ChildStdin)> {
    let mut child = Command::new("aplay")
        .args(["-q", "-t", "raw", "-f", "S16_LE", "-r", &RATE.to_string(), "-c", "1"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdin = child.stdin.take()?;
    Some((child, stdin))
}

fn bytes(pcm: &[i16]) -> Vec<u8> {
    pcm.iter().flat_map(|s| s.to_le_bytes()).collect()
}

/// Fire and forget: returns immediately.
pub fn play(sound: Sound) {
    if !enabled() {
        return;
    }
    std::thread::spawn(move || {
        let Some((mut child, mut stdin)) = spawn_aplay() else { return };
        let _ = stdin.write_all(&bytes(&make_sound(sound)));
        drop(stdin);
        let _ = child.wait();
    });
}

/// Chiptune loop for the manual (does nothing if already playing).
pub fn start_music() {
    if !enabled() || MUSIC_RUNNING.swap(true, SeqCst) {
        return;
    }
    MUSIC_STOP.store(false, SeqCst);
    std::thread::spawn(|| {
        let pcm = bytes(&make_music());
        if let Some((child, mut stdin)) = spawn_aplay() {
            *MUSIC_CHILD.lock().unwrap() = Some(child);
            // The same buffer is written again and again into one aplay: the loop has no gap.
            while !MUSIC_STOP.load(SeqCst) && stdin.write_all(&pcm).is_ok() {}
            drop(stdin);
            if let Some(mut c) = MUSIC_CHILD.lock().unwrap().take() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
        MUSIC_RUNNING.store(false, SeqCst);
    });
}

pub fn stop_music() {
    MUSIC_STOP.store(true, SeqCst);
    if let Some(c) = MUSIC_CHILD.lock().unwrap().as_mut() {
        let _ = c.kill(); // the writer then gets EPIPE and leaves its loop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(ms: f64) -> usize {
        (ms * RATE as f64 / 1000.0) as usize
    }

    #[test]
    fn effects_have_the_expected_length_and_are_audible_and_in_range() {
        for (s, ms) in [(Sound::Success, 720.0), (Sound::Start, 290.0), (Sound::Error, 390.0), (Sound::Blip, 60.0)] {
            let pcm = make_sound(s);
            assert_eq!(pcm.len(), samples(ms), "{s:?}");
            let peak = pcm.iter().map(|v| v.unsigned_abs()).max().unwrap();
            assert!(peak > 1000 && peak <= 30000, "{s:?} peak {peak}");
        }
    }

    #[test]
    fn synthesis_is_deterministic() {
        assert_eq!(make_sound(Sound::Success), make_sound(Sound::Success));
        assert_eq!(make_music(), make_music());
    }

    #[test]
    fn music_is_a_seven_second_loop_without_clipping() {
        let pcm = make_music();
        let secs = pcm.len() as f64 / RATE as f64;
        // 64 sixteenths at 140 bpm: the last bass note (s = 60) ends at 63.6 sixteenths = 6.85 s
        assert!((6.8..6.9).contains(&secs), "{secs}");
        assert!(pcm.iter().all(|v| v.unsigned_abs() < 30001));
    }

    #[test]
    fn disabled_sound_is_a_no_op() {
        set_enabled(false);
        assert!(!enabled());
        play(Sound::Blip);
        start_music();
        assert!(!MUSIC_RUNNING.load(SeqCst));
        set_enabled(true);
    }
}
