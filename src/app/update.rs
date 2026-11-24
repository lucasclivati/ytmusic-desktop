//! Atualizacao do app, sempre a pedido do usuario (aba Atualizacoes das configuracoes).
//! Nao ha verificacao automatica: a rede so e consultada ao clicar em "Verificar", e o
//! download so comeca ao clicar em "Baixar e instalar".

use super::*;

#[derive(Clone, PartialEq)]
pub(super) enum UpdateState {
    /// Nada foi verificado nesta sessao.
    Idle,
    Checking,
    UpToDate,
    /// Ha uma versao nova; o usuario decide se instala.
    Available(updater::Release),
    Downloading { done: u64, total: u64 },
    Installing,
    Failed(String),
}

impl App {
    pub(super) fn update_busy(&self) -> bool {
        matches!(self.update_state, UpdateState::Checking | UpdateState::Downloading { .. } | UpdateState::Installing)
    }

    /// Consulta o ultimo Release no GitHub (em segundo plano). So e chamada pelo botao.
    pub(super) fn check_for_update(&mut self, ctx: &egui::Context) {
        if self.update_busy() {
            return;
        }
        self.update_state = UpdateState::Checking;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::UpdateChecked(updater::fetch_latest()));
            ctx.request_repaint();
        });
    }

    pub(super) fn update_checked(&mut self, result: Result<Option<updater::Release>, String>) {
        self.update_state = match result {
            Ok(Some(release)) => UpdateState::Available(release),
            Ok(None) => UpdateState::UpToDate,
            Err(e) => UpdateState::Failed(e),
        };
    }

    /// Baixa o instalador (conferindo o SHA-256) e, ao terminar, executa-o e fecha o app.
    pub(super) fn start_update(&mut self, rel: updater::Release, ctx: &egui::Context) {
        if self.update_busy() {
            return;
        }
        self.update_state = UpdateState::Downloading { done: 0, total: rel.size };
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let (progress_tx, progress_ctx) = (tx.clone(), ctx.clone());
            let result = updater::download(&rel, move |done, total| {
                let _ = progress_tx.send(Msg::UpdateProgress(done, total));
                progress_ctx.request_repaint();
            });
            let _ = tx.send(Msg::UpdateDownloaded(result));
            ctx.request_repaint();
        });
    }

    pub(super) fn update_progress(&mut self, done: u64, total: u64) {
        if let UpdateState::Downloading { done: d, total: t } = &mut self.update_state {
            (*d, *t) = (done, total);
        }
    }

    pub(super) fn update_downloaded(&mut self, result: Result<std::path::PathBuf, String>) {
        match result {
            Ok(path) => {
                self.update_state = UpdateState::Installing;
                match updater::launch_installer(&path) {
                    // O instalador precisa substituir os arquivos: o app fecha e ele o reabre.
                    Ok(()) => std::process::exit(0),
                    Err(e) => self.update_state = UpdateState::Failed(e),
                }
            }
            Err(e) => self.update_state = UpdateState::Failed(e),
        }
    }
}
