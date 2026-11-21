#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod app;
mod config;
mod views;
mod worker;

fn main() -> eframe::Result {
    let opts = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("1Claw Vault Explorer")
            .with_app_id("vault-explorer")
            .with_inner_size([1180.0, 740.0])
            .with_min_inner_size([760.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native("1Claw Vault Explorer", opts, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
