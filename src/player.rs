//! Reproducao de audio: `yt-dlp` baixa o audio da faixa para o cache e o rodio
//! (symphonia, em Rust puro) decodifica e toca. Um thread dono do dispositivo
//! de audio recebe comandos; a UI so le o estado compartilhado.

use crate::util;
use crate::ytm::Track;
use rodio::{Decoder, DeviceSinkBuilder, Source};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Idle,
    Loading,
    Playing,
    Paused,
    Error(String),
}

#[derive(Clone, Debug)]
pub struct PlayerState {
    pub status: Status,
    pub current: Option<Track>,
    pub position: Duration,
    pub duration: Duration,
    pub volume: f32,
    /// A faixa acabou (ou falhou) e a UI deve avancar a fila. A UI zera.
    pub finished: bool,
    /// Em `Loading`: a faixa ainda nao esta no cache e o yt-dlp esta baixando.
    pub downloading: bool,
}

enum Cmd {
    Play(Track),
    Toggle,
    Seek(Duration),
    Volume(f32),
    Ready { token: u64, path: PathBuf },
    Failed { token: u64, message: String },
}

pub struct Player {
    tx: Sender<Cmd>,
    pub state: Arc<Mutex<PlayerState>>,
    ctx: eframe::egui::Context,
}

impl Player {
    pub fn new(ctx: eframe::egui::Context) -> Player {
        let state = Arc::new(Mutex::new(PlayerState {
            status: Status::Idle,
            current: None,
            position: Duration::ZERO,
            duration: Duration::ZERO,
            volume: load_volume(),
            finished: false,
            downloading: false,
        }));
        let (tx, rx) = channel();
        let (worker_state, worker_tx, worker_ctx) = (state.clone(), tx.clone(), ctx.clone());
        std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || worker(rx, worker_tx, worker_state, worker_ctx))
            .expect("thread de audio");
        std::thread::spawn(trim_cache);
        Player { tx, state, ctx }
    }

    pub fn snapshot(&self) -> PlayerState {
        self.state.lock().unwrap().clone()
    }

    pub fn play(&self, track: Track) {
        let _ = self.tx.send(Cmd::Play(track));
    }

    pub fn toggle(&self) {
        let _ = self.tx.send(Cmd::Toggle);
    }

    pub fn seek(&self, to: Duration) {
        let _ = self.tx.send(Cmd::Seek(to));
    }

    pub fn set_volume(&self, v: f32) {
        let _ = self.tx.send(Cmd::Volume(v.clamp(0.0, 1.0)));
    }

    /// Baixa a proxima faixa em segundo plano para a troca ser instantanea.
    pub fn prefetch(&self, video_id: &str) {
        let id = video_id.to_string();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = ensure_downloaded(&id);
            ctx.request_repaint();
        });
    }
}

fn worker(rx: Receiver<Cmd>, tx: Sender<Cmd>, state: Arc<Mutex<PlayerState>>, ctx: eframe::egui::Context) {
    let mut sink: Option<rodio::MixerDeviceSink> = None;
    let mut player: Option<rodio::Player> = None;
    let mut token: u64 = 0;
    let mut retried: Vec<String> = Vec::new();

    loop {
        match rx.recv_timeout(Duration::from_millis(150)) {
            Ok(Cmd::Play(track)) => {
                token += 1;
                if let Some(p) = player.take() {
                    p.stop();
                }
                {
                    let mut s = state.lock().unwrap();
                    s.status = Status::Loading;
                    s.position = Duration::ZERO;
                    s.duration = Duration::from_secs(track.duration_secs as u64);
                    s.current = Some(track.clone());
                    s.finished = false;
                    s.downloading = cached_file(&track.video_id).is_none();
                }
                let (my_token, tx2, ctx2) = (token, tx.clone(), ctx.clone());
                std::thread::spawn(move || {
                    let cmd = match ensure_downloaded(&track.video_id) {
                        Ok(path) => Cmd::Ready { token: my_token, path },
                        Err(message) => Cmd::Failed { token: my_token, message },
                    };
                    let _ = tx2.send(cmd);
                    ctx2.request_repaint();
                });
            }
            Ok(Cmd::Ready { token: t, path }) if t == token => {
                match start(&path, &mut sink, &state) {
                    Ok(p) => player = Some(p),
                    Err(message) => {
                        // Arquivo do cache corrompido (ex.: download antigo interrompido):
                        // apaga e baixa de novo, uma vez so.
                        let id = state.lock().unwrap().current.as_ref().map(|t| t.video_id.clone());
                        let _ = std::fs::remove_file(&path);
                        match id {
                            Some(id) if !retried.contains(&id) => {
                                retried.push(id.clone());
                                let (my_token, tx2, ctx2) = (token, tx.clone(), ctx.clone());
                                std::thread::spawn(move || {
                                    let cmd = match ensure_downloaded(&id) {
                                        Ok(path) => Cmd::Ready { token: my_token, path },
                                        Err(message) => Cmd::Failed { token: my_token, message },
                                    };
                                    let _ = tx2.send(cmd);
                                    ctx2.request_repaint();
                                });
                            }
                            _ => fail(&state, message),
                        }
                    }
                }
            }
            Ok(Cmd::Failed { token: t, message }) if t == token => fail(&state, message),
            Ok(Cmd::Ready { .. }) | Ok(Cmd::Failed { .. }) => {} // resposta de uma faixa antiga
            Ok(Cmd::Toggle) => {
                if let Some(p) = &player {
                    let mut s = state.lock().unwrap();
                    if p.is_paused() {
                        p.play();
                        s.status = Status::Playing;
                    } else {
                        p.pause();
                        s.status = Status::Paused;
                    }
                }
            }
            Ok(Cmd::Seek(to)) => {
                if let Some(p) = &player {
                    let _ = p.try_seek(to);
                }
            }
            Ok(Cmd::Volume(v)) => {
                state.lock().unwrap().volume = v;
                save_volume(v);
                if let Some(p) = &player {
                    p.set_volume(v * v);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if let Some(p) = &player {
            let mut s = state.lock().unwrap();
            s.position = p.get_pos();
            if p.empty() && s.status == Status::Playing {
                s.status = Status::Idle;
                s.finished = true;
                drop(s);
                player = None;
                ctx.request_repaint();
            }
        }
    }
}

fn fail(state: &Arc<Mutex<PlayerState>>, message: String) {
    let mut s = state.lock().unwrap();
    s.status = Status::Error(message);
    s.finished = true;
}

fn start(
    path: &Path,
    sink: &mut Option<rodio::MixerDeviceSink>,
    state: &Arc<Mutex<PlayerState>>,
) -> Result<rodio::Player, String> {
    // Reabre o dispositivo a cada faixa: sobrevive a troca de fone/saida.
    let mut device = DeviceSinkBuilder::open_default_sink().map_err(|e| format!("audio: {e}"))?;
    device.log_on_drop(false);
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let decoder = Decoder::try_from(file).map_err(|e| format!("formato nao suportado: {e}"))?;
    let total = decoder.total_duration();
    let player = rodio::Player::connect_new(device.mixer());
    let volume = state.lock().unwrap().volume;
    player.set_volume(volume * volume);
    player.append(decoder);
    *sink = Some(device);
    let mut s = state.lock().unwrap();
    if let Some(t) = total {
        s.duration = t;
    }
    s.status = Status::Playing;
    Ok(player)
}

// ------------------------------------------------------------------ yt-dlp

fn locks() -> &'static Mutex<HashMap<String, Arc<Mutex<()>>>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(Default::default)
}

fn cached_file(id: &str) -> Option<PathBuf> {
    std::fs::read_dir(util::cache_dir())
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| is_final_audio(p, id))
}

fn which(name: &str) -> Option<PathBuf> {
    let exe = format!("{name}.exe");
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(&exe))
            .find(|p| p.is_file())
    })
}

/// Procura `exe` na pasta de pacotes do winget (`<package_prefix>_*`). O PATH
/// do processo pode estar desatualizado logo apos instalar, entao olhamos direto la.
fn winget_package_exe(package_prefix: &str, exe: &str) -> Option<PathBuf> {
    let packages = dirs::data_local_dir()?.join("Microsoft").join("WinGet").join("Packages");
    std::fs::read_dir(packages)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(package_prefix))
        .map(|e| e.path().join(exe))
        .find(|p| p.is_file())
}

pub fn find_ytdlp() -> Option<PathBuf> {
    let mut candidates = vec![util::data_dir().join("yt-dlp.exe")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("yt-dlp.exe"));
        }
    }
    candidates.extend(winget_package_exe("yt-dlp.yt-dlp_", "yt-dlp.exe"));
    if let Some(found) = candidates.into_iter().find(|p| p.is_file()) {
        return Some(found);
    }
    which("yt-dlp")
}

/// Argumento `--js-runtimes` para o yt-dlp: Deno (inclusive o do winget) ou Node.
fn js_runtime_arg() -> Option<String> {
    if let Some(p) = which("deno").or_else(|| winget_package_exe("DenoLand.Deno_", "deno.exe")) {
        return Some(format!("deno:{}", p.display()));
    }
    which("node").map(|p| format!("node:{}", p.display()))
}

fn ensure_downloaded(id: &str) -> Result<PathBuf, String> {
    if let Some(p) = cached_file(id) {
        return Ok(p);
    }
    let lock = locks().lock().unwrap().entry(id.to_string()).or_default().clone();
    let _guard = lock.lock().unwrap();
    if let Some(p) = cached_file(id) {
        return Ok(p); // outro thread terminou enquanto esperavamos
    }

    let ytdlp = find_ytdlp().ok_or_else(|| crate::tr!("player.ytdlp_missing").to_string())?;
    // Baixa numa pasta temporaria e so move para o cache quando o yt-dlp termina
    // (inclusive o conserto do container pelo ffmpeg). Antes o arquivo parcial ja
    // aparecia no cache com o nome final, e tocar a faixa durante o download dava
    // "formato nao suportado".
    let staging = staging_dir();
    clear_staging(id);
    let mut cmd = Command::new(ytdlp);
    cmd.args(["-f", "bestaudio[ext=m4a]/bestaudio[acodec^=mp4a]/140"])
        .args(["--no-playlist", "--no-warnings", "--no-progress", "-q"])
        .arg("-o")
        .arg(staging.join("%(id)s.%(ext)s"));
    // Versoes recentes do yt-dlp precisam de um runtime JS para o YouTube.
    if let Some(rt) = js_runtime_arg() {
        cmd.args(["--js-runtimes", &rt]);
    }
    cmd.arg(format!("https://music.youtube.com/watch?v={id}"));
    hide_console(&mut cmd);
    let out = cmd.output().map_err(|e| format!("falha ao executar yt-dlp: {e}"))?;
    if !out.status.success() {
        clear_staging(id);
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("erro desconhecido");
        return Err(format!("yt-dlp: {}", last.trim()));
    }
    let done = std::fs::read_dir(&staging)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .find(|p| is_final_audio(p, id))
        .ok_or("o yt-dlp nao gerou arquivo de audio")?;
    let dest = util::cache_dir().join(done.file_name().unwrap_or_default());
    std::fs::rename(&done, &dest).map_err(|e| format!("cache: {e}"))?;
    clear_staging(id);
    Ok(dest)
}

/// `<cache>/tmp`: downloads em andamento.
fn staging_dir() -> PathBuf {
    let dir = util::cache_dir().join("tmp");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// `<id>.<ext>` final (nao `.part`, `.ytdl`, `.temp.m4a` etc.).
fn is_final_audio(p: &Path, id: &str) -> bool {
    p.is_file()
        && p.file_stem().and_then(|s| s.to_str()) == Some(id)
        && p.extension().is_some_and(|e| e != "part" && e != "ytdl")
}

/// Apaga restos de um download anterior desta faixa.
fn clear_staging(id: &str) {
    let Ok(rd) = std::fs::read_dir(staging_dir()) else { return };
    for e in rd.flatten() {
        if e.file_name().to_string_lossy().starts_with(&format!("{id}.")) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[cfg(windows)]
fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}

#[cfg(not(windows))]
fn hide_console(_: &mut Command) {}

/// Mantem so as 80 faixas mais recentes no cache (~4 MB cada).
fn trim_cache() {
    // Downloads interrompidos de sessoes anteriores.
    let _ = std::fs::remove_dir_all(staging_dir());
    let Ok(rd) = std::fs::read_dir(util::cache_dir()) else { return };
    let mut files: Vec<_> = rd
        .flatten()
        .filter(|e| e.path().is_file())
        .filter(|e| {
            // Restos do formato antigo (arquivo parcial gravado direto no cache).
            let name = e.file_name().to_string_lossy().to_string();
            let junk = name.contains(".temp.") || name.ends_with(".part") || name.ends_with(".ytdl");
            if junk {
                let _ = std::fs::remove_file(e.path());
            }
            !junk
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in files.into_iter().skip(80) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Baixa uma faixa de verdade e confere que o rodio decodifica e busca.
    /// Rode com `cargo test download_and_decode -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn download_and_decode() {
        let path = ensure_downloaded("Jb6gcoR266U").expect("download");
        println!("arquivo: {} ({} bytes)", path.display(), std::fs::metadata(&path).unwrap().len());
        let file = std::fs::File::open(&path).unwrap();
        let dec = Decoder::try_from(file).expect("decoder");
        println!("duracao: {:?}, canais: {:?}, taxa: {:?}", dec.total_duration(), dec.channels(), dec.sample_rate());
        let secs = dec.take_duration(Duration::from_secs(2)).count();
        assert!(secs > 0, "nenhuma amostra decodificada");
        println!("amostras em 2s: {secs}");
    }

    /// Toca 3s no dispositivo de audio padrao e confere que a posicao avanca e
    /// que o seek funciona. Precisa de uma saida de audio.
    #[test]
    #[ignore]
    fn plays_and_seeks() {
        let path = ensure_downloaded("Jb6gcoR266U").expect("download");
        let mut device = DeviceSinkBuilder::open_default_sink().expect("saida de audio");
        device.log_on_drop(false);
        let player = rodio::Player::connect_new(device.mixer());
        player.set_volume(0.05);
        player.append(Decoder::try_from(std::fs::File::open(path).unwrap()).unwrap());
        std::thread::sleep(Duration::from_secs(3));
        let p1 = player.get_pos();
        println!("pos apos 3s: {p1:?}");
        assert!(p1 > Duration::from_secs(2), "posicao nao avancou");
        player.try_seek(Duration::from_secs(120)).expect("seek");
        std::thread::sleep(Duration::from_millis(500));
        let p2 = player.get_pos();
        println!("pos apos seek: {p2:?}");
        assert!(p2 > Duration::from_secs(119) && p2 < Duration::from_secs(123), "seek falhou: {p2:?}");
        assert!(!player.empty());
    }
}

fn volume_file() -> PathBuf {
    util::data_dir().join("volume")
}

fn load_volume() -> f32 {
    std::fs::read_to_string(volume_file())
        .ok()
        .and_then(|s| s.trim().parse::<f32>().ok())
        .map(|v| v.clamp(0.0, 1.0))
        .unwrap_or(0.7)
}

fn save_volume(v: f32) {
    let _ = std::fs::write(volume_file(), v.to_string());
}
