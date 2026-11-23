use std::path::PathBuf;

/// Pasta de dados do app: `%LOCALAPPDATA%\ytmusic`.
pub fn data_dir() -> PathBuf {
    let dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ytmusic");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Cache de audio ja baixado.
pub fn cache_dir() -> PathBuf {
    let dir = data_dir().join("cache");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Data UTC de hoje como `YYYYMMDD`, usada na versao do cliente InnerTube.
pub fn today_yyyymmdd() -> String {
    let days = (unix_secs() / 86_400) as i64;
    // Algoritmo de Howard Hinnant (civil_from_days).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}{m:02}{d:02}")
}

/// Formata segundos como `m:ss` (ou `h:mm:ss`).
pub fn fmt_time(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_time() {
        assert_eq!(fmt_time(5), "0:05");
        assert_eq!(fmt_time(225), "3:45");
        assert_eq!(fmt_time(3725), "1:02:05");
    }

    #[test]
    fn date_has_eight_digits() {
        let d = today_yyyymmdd();
        assert_eq!(d.len(), 8);
        assert!(d.starts_with("20"));
    }
}
