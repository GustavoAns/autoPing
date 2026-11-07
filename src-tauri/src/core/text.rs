//! Normalização de texto para comparações tolerantes:
//! `PinG` = `ping` = `PÍNG` = `  ping  `.
//!
//! - Decomposição NFKD + remoção de acentos (marcas combinantes)
//! - minúsculas
//! - espaços (inclusive quebras de linha) colapsados em um só, sem bordas

use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for c in s.nfkd().filter(|c| !is_combining_mark(*c)) {
        if c.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.extend(c.to_lowercase());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn case_and_accents() {
        for s in ["ping", "PING", "PinG", "PÍNG", "pïng", "  ping  "] {
            assert_eq!(normalize(s), "ping", "{s}");
        }
    }

    #[test]
    fn whitespace_collapses() {
        assert_eq!(normalize("  Vaga \n\t ABERTA "), "vaga aberta");
    }

    #[test]
    fn portuguese_text() {
        assert_eq!(normalize("Ação Já"), "acao ja");
    }

    #[test]
    fn fullwidth_and_compat_chars() {
        assert_eq!(normalize("ＰＩＮＧ"), "ping");
    }
}
