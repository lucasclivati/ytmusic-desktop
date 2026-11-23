//! Credenciais da conta: o cabecalho `Cookie` do YouTube Music, guardado no
//! disco protegido pelo DPAPI do Windows (so o seu usuario consegue ler).

use crate::util;
use sha1::{Digest, Sha1};
use std::path::PathBuf;

pub const ORIGIN: &str = "https://music.youtube.com";

#[derive(Clone)]
pub struct Auth {
    cookie_header: String,
    sapisid: String,
}

impl Auth {
    /// Aceita o cabecalho `Cookie` completo (`a=b; c=d; ...`). Falha se nao
    /// houver o cookie SAPISID, que e o que prova que a sessao esta logada.
    pub fn from_cookie_header(header: &str) -> Option<Auth> {
        let header = header.trim().trim_start_matches("Cookie:").trim();
        let find = |name: &str| {
            header.split(';').find_map(|part| {
                let (k, v) = part.trim().split_once('=')?;
                (k == name).then(|| v.to_string())
            })
        };
        let sapisid = find("__Secure-3PAPISID").or_else(|| find("SAPISID"))?;
        if sapisid.is_empty() {
            return None;
        }
        Some(Auth {
            cookie_header: header.to_string(),
            sapisid,
        })
    }

    pub fn cookie_header(&self) -> String {
        // SOCS evita a tela de consentimento em algumas regioes.
        if self.cookie_header.contains("SOCS=") {
            self.cookie_header.clone()
        } else {
            format!("{}; SOCS=CAI", self.cookie_header)
        }
    }

    /// `Authorization: SAPISIDHASH <ts>_<sha1(ts sapisid origin)>`.
    pub fn authorization(&self) -> String {
        sapisid_hash(&self.sapisid, util::unix_secs())
    }

    fn file() -> PathBuf {
        util::data_dir().join("auth.bin")
    }

    pub fn load() -> Option<Auth> {
        let blob = std::fs::read(Self::file()).ok()?;
        let plain = dpapi::unprotect(&blob)?;
        Auth::from_cookie_header(std::str::from_utf8(&plain).ok()?)
    }

    pub fn save(&self) -> Result<(), String> {
        let blob = dpapi::protect(self.cookie_header.as_bytes())
            .ok_or("nao foi possivel proteger as credenciais")?;
        std::fs::write(Self::file(), blob).map_err(|e| e.to_string())
    }

    pub fn clear() {
        let _ = std::fs::remove_file(Self::file());
    }
}

pub fn sapisid_hash(sapisid: &str, ts: u64) -> String {
    let mut hasher = Sha1::new();
    hasher.update(format!("{ts} {sapisid} {ORIGIN}").as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("SAPISIDHASH {ts}_{hex}")
}

#[cfg(windows)]
mod dpapi {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };

    fn run(data: &[u8], protect: bool) -> Option<Vec<u8>> {
        unsafe {
            let input = CRYPT_INTEGER_BLOB {
                cbData: data.len() as u32,
                pbData: data.as_ptr() as *mut u8,
            };
            let mut out = CRYPT_INTEGER_BLOB::default();
            let res = if protect {
                CryptProtectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            } else {
                CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            };
            res.ok()?;
            let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            let _ = LocalFree(Some(HLOCAL(out.pbData as *mut _)));
            Some(bytes)
        }
    }

    pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
        run(data, true)
    }

    pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
        run(data, false)
    }
}

#[cfg(not(windows))]
mod dpapi {
    pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
        Some(data.to_vec())
    }
    pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
        Some(data.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cookie_header() {
        let a = Auth::from_cookie_header("Cookie: SID=1; __Secure-3PAPISID=abc; X=y").unwrap();
        assert_eq!(a.sapisid, "abc");
        assert!(a.cookie_header().contains("SOCS=CAI"));
        assert!(Auth::from_cookie_header("SID=1; X=y").is_none());
    }

    #[test]
    fn hash_has_expected_shape() {
        let h = sapisid_hash("abc", 1_700_000_000);
        assert!(h.starts_with("SAPISIDHASH 1700000000_"));
        assert_eq!(h.len(), "SAPISIDHASH 1700000000_".len() + 40);
    }

    #[test]
    fn dpapi_roundtrip() {
        let blob = dpapi::protect(b"segredo").unwrap();
        assert_eq!(dpapi::unprotect(&blob).unwrap(), b"segredo");
    }
}
