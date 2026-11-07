//! Log simples em arquivo (%APPDATA%/AutoPing/logs/autoping.log) com rotação
//! por tamanho. Nunca registrar o token.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 1024 * 1024;

struct LogFile {
    path: PathBuf,
    file: Option<File>,
}

static LOG: OnceLock<Mutex<LogFile>> = OnceLock::new();

pub fn init(dir: PathBuf) {
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("autoping.log");
    let file = OpenOptions::new().create(true).append(true).open(&path).ok();
    let _ = LOG.set(Mutex::new(LogFile { path, file }));
}

pub fn log(msg: impl AsRef<str>) {
    let line = format!("[{}] {}\n", timestamp(), msg.as_ref());
    #[cfg(debug_assertions)]
    eprint!("{line}");
    let Some(lock) = LOG.get() else { return };
    let Ok(mut lf) = lock.lock() else { return };
    rotate_if_needed(&mut lf);
    if let Some(f) = lf.file.as_mut() {
        let _ = f.write_all(line.as_bytes());
    }
}

fn rotate_if_needed(lf: &mut LogFile) {
    let size = fs::metadata(&lf.path).map(|m| m.len()).unwrap_or(0);
    if size < MAX_BYTES {
        return;
    }
    lf.file = None;
    let _ = fs::rename(&lf.path, lf.path.with_extension("log.1"));
    lf.file = OpenOptions::new().create(true).append(true).open(&lf.path).ok();
}

/// Data/hora UTC "AAAA-MM-DD HH:MM:SS" sem dependências externas.
fn timestamp() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Algoritmo civil_from_days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if mo <= 2 { 1 } else { 0 };
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}Z")
}

#[macro_export]
macro_rules! logf {
    ($($arg:tt)*) => { $crate::logger::log(format!($($arg)*)) };
}
