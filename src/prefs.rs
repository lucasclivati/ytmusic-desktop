//! Preferencias do usuario, em `%LOCALAPPDATA%\ytmusic\prefs.json`.

use crate::util;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Ao minimizar a janela principal, mostra o mini-player flutuante.
    pub mini_on_minimize: bool,
    /// Mini-player travado no lugar (nao arrasta).
    pub mini_pinned: bool,
    /// Formato compacto do mini-player (so controles e progresso).
    pub mini_compact: bool,
    /// Ultima posicao do mini-player na tela (pontos), se o usuario o moveu.
    pub mini_pos: Option<[f32; 2]>,
    /// Painel "A seguir / Relacionadas" a direita.
    pub show_queue: bool,
    /// Idioma da interface (`pt-BR`, `en`). Vazio: segue o idioma do Windows.
    pub lang: Option<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { mini_on_minimize: true, mini_pinned: false, mini_compact: false, mini_pos: None, show_queue: true, lang: None }
    }
}

impl Prefs {
    fn file() -> std::path::PathBuf {
        util::data_dir().join("prefs.json")
    }

    pub fn load() -> Prefs {
        std::fs::read_to_string(Self::file())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(Self::file(), s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_use_defaults() {
        let p: Prefs = serde_json::from_str(r#"{"mini_pinned": true}"#).unwrap();
        assert!(p.mini_pinned);
        assert!(p.mini_on_minimize);
        assert!(p.show_queue);
    }
}
