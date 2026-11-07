//! Sons do app, gerados em código (sem arquivos de áudio) e tocados pela API
//! nativa do Windows (`PlaySound`), que roda fora da WebView: funciona com a
//! janela oculta e não pesa no processo.
//!
//! - `ding`: aviso curto quando a mensagem é enviada.
//! - `alarm`: alarme em loop (modo alarme) até `stop()`.

use std::sync::OnceLock;

const RATE: u32 = 22_050;

/// WAV PCM 16-bit mono a partir de amostras em [-1, 1].
fn wav(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // mono
    v.extend_from_slice(&RATE.to_le_bytes());
    v.extend_from_slice(&(RATE * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        v.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    v
}

/// Tom senoidal com ataque curto e decaimento exponencial.
fn tone(out: &mut Vec<f32>, freq: f32, secs: f32, decay: f32, gain: f32) {
    let n = (secs * RATE as f32) as usize;
    let attack = (0.005 * RATE as f32) as usize;
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let env = (i.min(attack) as f32 / attack as f32) * (-t * decay).exp();
        // fundamental + harmônico leve para soar como "sino"
        let s = (2.0 * std::f32::consts::PI * freq * t).sin() + 0.3 * (4.0 * std::f32::consts::PI * freq * t).sin();
        out.push(s * env * gain / 1.3);
    }
}

fn silence(out: &mut Vec<f32>, secs: f32) {
    out.extend(std::iter::repeat(0.0).take((secs * RATE as f32) as usize));
}

fn ding_wav() -> &'static [u8] {
    static W: OnceLock<Vec<u8>> = OnceLock::new();
    W.get_or_init(|| {
        let mut s = Vec::new();
        tone(&mut s, 880.0, 0.14, 9.0, 0.55);
        tone(&mut s, 1318.5, 0.55, 6.0, 0.6);
        wav(&s)
    })
}

fn alarm_wav() -> &'static [u8] {
    static W: OnceLock<Vec<u8>> = OnceLock::new();
    W.get_or_init(|| {
        // bip-bip-bip-bip ... pausa (1,4s por ciclo)
        let mut s = Vec::new();
        for _ in 0..4 {
            tone(&mut s, 1046.5, 0.12, 4.0, 0.8);
            silence(&mut s, 0.06);
        }
        silence(&mut s, 0.68);
        wav(&s)
    })
}

#[cfg(windows)]
mod sys {
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_LOOP, SND_MEMORY, SND_NODEFAULT};

    pub fn play(bytes: &'static [u8], looped: bool) {
        let mut flags = SND_MEMORY | SND_ASYNC | SND_NODEFAULT;
        if looped {
            flags |= SND_LOOP;
        }
        // SAFETY: o buffer é 'static (OnceLock), válido durante toda a reprodução assíncrona.
        unsafe {
            PlaySoundW(bytes.as_ptr() as *const u16, std::ptr::null_mut(), flags);
        }
    }

    pub fn stop() {
        // SAFETY: ponteiro nulo encerra qualquer som em reprodução.
        unsafe {
            PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0);
        }
    }
}

#[cfg(not(windows))]
mod sys {
    pub fn play(_: &'static [u8], _: bool) {}
    pub fn stop() {}
}

/// Aviso curto de mensagem enviada.
pub fn ding() {
    sys::play(ding_wav(), false);
}

/// Alarme contínuo até `stop()`.
pub fn alarm() {
    sys::play(alarm_wav(), true);
}

pub fn stop() {
    sys::stop();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_headers_are_valid() {
        for w in [ding_wav(), alarm_wav()] {
            assert_eq!(&w[0..4], b"RIFF");
            assert_eq!(&w[8..12], b"WAVE");
            let riff_len = u32::from_le_bytes(w[4..8].try_into().unwrap()) as usize;
            assert_eq!(riff_len + 8, w.len());
        }
    }
}
