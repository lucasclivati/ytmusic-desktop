#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod auth;
mod player;
mod util;
mod ytm;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();

    // Modo de depuracao: `ytmusic --search "termo"` imprime os resultados.
    if args.get(1).map(String::as_str) == Some("--search") {
        let client = ytm::Client::new(auth::Auth::load());
        match client.search(&args[2..].join(" ")) {
            Ok(tracks) => tracks.iter().for_each(|t| {
                println!("{} | {} | {} | {}", t.video_id, t.title, t.artists, util::fmt_time(t.duration_secs as u64));
            }),
            Err(e) => eprintln!("erro: {e}"),
        }
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("YT Music")
            .with_inner_size([1000.0, 680.0])
            .with_min_inner_size([360.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native("YT Music", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
