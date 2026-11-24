//! Janela de configuracoes: uma janela nativa propria (separada da principal), com a
//! navegacao a esquerda e "cartoes" de linhas de ajuste, no estilo das configuracoes
//! do Windows 11.

use super::update::UpdateState;
use super::*;

/// Mesma fonte do atualizador: o campo `repository` do Cargo.toml.
const REPO_URL: &str = env!("CARGO_PKG_REPOSITORY");
const WINDOW_BG: Color32 = Color32::from_rgb(0x1c, 0x1c, 0x1c);
const CARD: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
const DIVIDER: Color32 = Color32::from_rgb(0x3d, 0x3d, 0x3d);

/// Paginas da janela, na ordem da navegacao.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Page {
    Account,
    Playback,
    Updates,
    General,
}

const PAGES: [(Page, &str); 4] = [
    (Page::Account, "Conta"),
    (Page::Playback, "Reprodução"),
    (Page::Updates, "Atualizações"),
    (Page::General, "Geral"),
];

impl App {
    /// Abre/atualiza a janela de configuracoes enquanto `show_settings` estiver ligado.
    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut builder = egui::ViewportBuilder::default()
            .with_title("Configurações do YT Music")
            .with_inner_size([780.0, 520.0])
            .with_min_inner_size([560.0, 360.0]);
        if let Some(icon) = crate::app_icon() {
            builder = builder.with_icon(icon);
        }
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("settings"), builder, |ui, _class| {
            self.settings_ui(ui);
            let close = ui.ctx().input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape));
            if close {
                self.show_settings = false;
            }
        });
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("settings_nav")
            .resizable(false)
            .exact_size(210.0)
            .frame(egui::Frame::new().fill(WINDOW_BG).inner_margin(Margin::symmetric(10, 14)))
            .show(ui, |ui| {
                for (page, label) in PAGES {
                    if nav_item(ui, label, page, self.settings_page == page) {
                        self.settings_page = page;
                    }
                    ui.add_space(2.0);
                }
            });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(WINDOW_BG).inner_margin(Margin::symmetric(28, 18))).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let title = PAGES.iter().find(|(p, _)| *p == self.settings_page).map_or("", |(_, t)| *t);
                ui.label(RichText::new(title).size(26.0).strong());
                ui.add_space(14.0);
                match self.settings_page {
                    Page::Account => self.account_page(ui),
                    Page::Playback => self.playback_page(ui),
                    Page::Updates => self.updates_page(ui),
                    Page::General => self.general_page(ui),
                }
            });
        });
    }

    fn account_page(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            if self.client.is_logged_in() {
                let who = self.account_name.clone().unwrap_or_else(|| "Conta conectada".into());
                row(ui, &who, Some("Conectado ao YouTube Music"), |ui| {
                    if ui.button("Desconectar").clicked() {
                        Auth::clear();
                        self.apply_auth(None);
                        self.notice = Some("Conta desconectada.".into());
                    }
                });
            } else {
                row(ui, "Conta do YouTube", Some("Conecte para ver suas curtidas e playlists"), |ui| {
                    if pill_button(ui, "Conectar", true).clicked() {
                        self.connect_account(&ui.ctx().clone());
                        self.show_settings = false;
                    }
                });
            }
        });

        section_title(ui, "Avançado");
        card(ui, |ui| {
            ui.add_space(6.0);
            ui.collapsing("Importar cookie manualmente", |ui| {
                ui.label(RichText::new("Cole o cabeçalho Cookie de music.youtube.com (DevTools > Rede).").color(DIM).small());
                ui.add_space(4.0);
                ui.add(egui::TextEdit::multiline(&mut self.cookie_input).desired_rows(3).desired_width(f32::INFINITY));
                ui.add_space(4.0);
                if ui.button("Importar").clicked() {
                    match Auth::from_cookie_header(&self.cookie_input) {
                        Some(auth) => {
                            let _ = auth.save();
                            self.apply_auth(Some(auth));
                            self.cookie_input.clear();
                            self.notice = Some("Cookie importado.".into());
                            self.show_settings = false;
                        }
                        None => self.notice = Some("Cookie invalido: falta o SAPISID.".into()),
                    }
                }
                ui.add_space(6.0);
            });
            ui.add_space(6.0);
        });
    }

    fn playback_page(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            let mut on = self.automix;
            row(ui, "Automix", Some("Mescla o fim de uma música com o início da próxima"), |ui| {
                if toggle(ui, &mut on).changed() {
                    self.automix = on;
                    self.apply_automix(true);
                }
            });
            if self.automix {
                divider(ui);
                row(ui, "Duração da mescla", Some("Por quanto tempo as duas músicas tocam juntas"), |ui| {
                    let mut secs = self.automix_secs;
                    ui.spacing_mut().slider_width = 150.0;
                    let slider = egui::Slider::new(&mut secs, crate::player::AUTOMIX_MIN_SECS..=crate::player::AUTOMIX_MAX_SECS).suffix(" s");
                    if ui.add(slider).changed() {
                        self.automix_secs = secs;
                        self.apply_automix(true);
                    }
                });
            }
        });
        ui.add_space(8.0);
        ui.label(RichText::new("A mescla só acontece quando a próxima música já foi baixada, e não vale com repetir faixa.").color(DIM).small());
    }

    /// Atualizacoes: tudo a pedido. Nada consulta a rede sem o clique em "Verificar".
    fn updates_page(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let busy = self.update_busy();
        let installed = format!("Versão instalada: {}", updater::CURRENT);
        card(ui, |ui| {
            row(ui, "YT Music", Some(&installed), |ui| {
                let clicked = ui.add_enabled_ui(!busy, |ui| pill_button(ui, "Verificar atualizações", true).clicked()).inner;
                if clicked {
                    self.check_for_update(&ctx);
                }
            });
        });
        ui.add_space(14.0);

        match self.update_state.clone() {
            UpdateState::Idle => {
                ui.label(RichText::new("O app não verifica atualizações sozinho. Clique em “Verificar atualizações” para consultar o GitHub.").color(DIM).small());
            }
            UpdateState::Checking => message_card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Verificando...");
                });
            }),
            UpdateState::UpToDate => message_card(ui, |ui| {
                ui.label(RichText::new("Você já está na versão mais recente.").size(15.0).color(Color32::from_gray(235)));
            }),
            UpdateState::Available(rel) => {
                let title = format!("Nova versão {} disponível", rel.version);
                card(ui, |ui| {
                    row(ui, &title, Some("O download é conferido por SHA-256 antes de instalar"), |ui| {
                        if pill_button(ui, "Baixar e instalar", true).clicked() {
                            self.start_update(rel.clone(), &ctx);
                        }
                    });
                    if !rel.notes.trim().is_empty() {
                        divider(ui);
                        ui.add_space(10.0);
                        ui.label(RichText::new("Novidades").strong());
                        egui::ScrollArea::vertical().max_height(150.0).auto_shrink([false, true]).show(ui, |ui| {
                            ui.label(RichText::new(rel.notes.trim()).color(DIM).small());
                        });
                        ui.add_space(12.0);
                    }
                });
            }
            UpdateState::Downloading { done, total } => message_card(ui, |ui| {
                ui.label("Baixando a atualização...");
                ui.add_space(6.0);
                let fraction = if total > 0 { (done as f32 / total as f32).clamp(0.0, 1.0) } else { 0.0 };
                ui.add(egui::ProgressBar::new(fraction).fill(ACCENT).show_percentage());
            }),
            UpdateState::Installing => message_card(ui, |ui| {
                ui.label("Instalando... o app vai fechar e abrir de novo.");
            }),
            UpdateState::Failed(error) => message_card(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(error).color(ACCENT)).wrap());
            }),
        }
    }

    fn general_page(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Componentes");
        card(ui, |ui| {
            let desc = if self.ytdlp_ok { "Baixa o áudio das músicas" } else { "Instale com: winget install yt-dlp.yt-dlp" };
            row(ui, "yt-dlp", Some(desc), |ui| {
                if self.ytdlp_ok {
                    ui.label(RichText::new("Encontrado").color(DIM));
                } else {
                    ui.colored_label(ACCENT, "Não encontrado");
                }
            });
        });

        section_title(ui, "Sobre");
        card(ui, |ui| {
            row(ui, "YT Music", None, |ui| {
                ui.label(RichText::new(format!("Versão {}", env!("CARGO_PKG_VERSION"))).color(DIM));
            });
            divider(ui);
            row(ui, "Repositório", None, |ui| {
                if ui.link(REPO_URL.trim_start_matches("https://")).clicked() {
                    // Abre no navegador padrao. So a URL fixa acima chega aqui.
                    let _ = std::process::Command::new("explorer").arg(REPO_URL).spawn();
                }
            });
        });
    }
}

// ----------------------------------------------------------------- componentes

/// Cartao arredondado que agrupa linhas de ajuste.
fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(CARD)
        .corner_radius(8.0)
        .inner_margin(Margin::symmetric(16, 0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Linha de ajuste: titulo (e descricao opcional) a esquerda, controle a direita.
fn row<R>(ui: &mut egui::Ui, title: &str, desc: Option<&str>, right: impl FnOnce(&mut egui::Ui) -> R) -> R {
    // Reserva a altura fixa da linha (sem isso ela esticaria ou ficaria colada na borda do
    // cartao). O controle da direita e posto primeiro; o texto ocupa o que sobra de largura.
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
    let mut right_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::right_to_left(Align::Center)));
    let result = right(&mut right_ui);

    let text_right = (right_ui.min_rect().left() - 16.0).max(rect.left() + 80.0);
    let text_rect = Rect::from_min_max(rect.min, pos2(text_right, rect.max.y));
    let mut text_ui = ui.new_child(egui::UiBuilder::new().max_rect(text_rect).layout(Layout::top_down(Align::Min)));
    // Centraliza o bloco de texto na altura da linha (titulo + descricao, ou so o titulo).
    text_ui.add_space(if desc.is_some() { 13.0 } else { 20.0 });
    text_ui.label(RichText::new(title).size(15.0).color(Color32::from_gray(235)));
    if let Some(desc) = desc {
        text_ui.label(RichText::new(desc).color(DIM).small());
    }
    result
}

/// Altura de cada linha de ajuste.
const ROW_HEIGHT: f32 = 60.0;

/// Cartao com uma mensagem/estado (sem a linha de titulo e controle).
fn message_card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    card(ui, |ui| {
        ui.add_space(14.0);
        let result = add(ui);
        ui.add_space(14.0);
        result
    })
}

fn divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, DIVIDER);
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.add_space(18.0);
    ui.label(RichText::new(text).size(14.0).strong());
    ui.add_space(6.0);
}

/// Interruptor ligado/desligado (com animacao). `changed()` indica que foi clicado.
fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(42.0, 22.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let radius = rect.height() / 2.0;
    let p = ui.painter();
    p.rect_filled(rect, radius, ACCENT.gamma_multiply(t));
    p.rect_stroke(rect, radius, Stroke::new(1.5, Color32::from_gray(150).gamma_multiply(1.0 - t)), egui::StrokeKind::Inside);
    let knob_r = 5.5 + 1.5 * t;
    let knob_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    let knob = Color32::from_gray(200).lerp_to_gamma(Color32::WHITE, t);
    p.circle_filled(pos2(knob_x, rect.center().y), knob_r, knob);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Item da navegacao lateral: fundo claro no selecionado e barrinha na cor de destaque.
fn nav_item(ui: &mut egui::Ui, label: &str, page: Page, selected: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 8.0, Color32::from_white_alpha(22));
        let bar = Rect::from_center_size(pos2(rect.min.x + 2.0, rect.center().y), vec2(3.0, 16.0));
        p.rect_filled(bar, 1.5, ACCENT);
    } else if resp.hovered() {
        p.rect_filled(rect, 8.0, Color32::from_white_alpha(10));
    }
    let color = if selected || resp.hovered() { Color32::WHITE } else { Color32::from_gray(210) };
    page_icon(p, page, pos2(rect.min.x + 26.0, rect.center().y), color);
    p.text(pos2(rect.min.x + 50.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(15.0), color);
    resp.clicked()
}

fn page_icon(p: &egui::Painter, page: Page, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    match page {
        Page::Account => {
            p.circle_stroke(c + vec2(0.0, -4.0), 3.8, stroke);
            let shoulders: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI + std::f32::consts::PI * i as f32 / 12.0;
                    c + vec2(a.cos() * 7.5, a.sin() * 5.5 + 9.0)
                })
                .collect();
            p.add(egui::Shape::line(shoulders, stroke));
        }
        Page::Playback => {
            p.circle_stroke(c, 8.5, stroke);
            let tri = vec![c + vec2(-2.5, -4.0), c + vec2(-2.5, 4.0), c + vec2(4.0, 0.0)];
            p.add(egui::Shape::convex_polygon(tri, color, Stroke::NONE));
        }
        Page::Updates => {
            // Seta circular: arco de ~290 graus com uma ponta no fim.
            let start = -std::f32::consts::FRAC_PI_2 + 0.45;
            let end = start + 5.0;
            let at = |a: f32| c + vec2(a.cos(), a.sin()) * 7.5;
            let arc: Vec<Pos2> = (0..=20).map(|i| at(start + (end - start) * i as f32 / 20.0)).collect();
            p.add(egui::Shape::line(arc, stroke));
            let tip = at(end);
            let tangent = vec2(-end.sin(), end.cos());
            let normal = vec2(end.cos(), end.sin());
            let head = vec![tip + tangent * 5.0, tip + normal * 3.8, tip - normal * 3.8];
            p.add(egui::Shape::convex_polygon(head, color, Stroke::NONE));
        }
        Page::General => {
            p.circle_stroke(c, 4.2, stroke);
            for i in 0..8 {
                let a = std::f32::consts::FRAC_PI_4 * i as f32;
                let dir = vec2(a.cos(), a.sin());
                p.line_segment([c + dir * 6.6, c + dir * 9.0], Stroke::new(2.2, color));
            }
        }
    }
}
