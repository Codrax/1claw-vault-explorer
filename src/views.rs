use crate::app::App;
use eframe::egui;

pub fn secrets(_app: &mut App, ctx: &egui::Context) {
    egui::CentralPanel::default().show(ctx, |ui| ui.label("todo"));
}

pub fn policies(_app: &mut App, ctx: &egui::Context) {
    egui::CentralPanel::default().show(ctx, |ui| ui.label("todo"));
}

pub fn activity(_app: &mut App, ctx: &egui::Context) {
    egui::CentralPanel::default().show(ctx, |ui| ui.label("todo"));
}
