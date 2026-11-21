use crate::api::SecretMeta;
use crate::app::{ACCENT, App, Detail, Editor};
use crate::worker::Job;
use eframe::egui::{self, Color32, RichText};
use std::collections::BTreeMap;

const SECRET_TYPES: &[&str] = &["generic", "password", "api_key", "certificate", "private_key", "ssh_key", "env"];

fn no_vault(ctx: &egui::Context, app: &App) -> bool {
    if app.selected_vault.is_some() {
        return false;
    }
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.centered_and_justified(|ui| {
            ui.label(RichText::new("Pick a vault on the left to get started").weak().size(16.0));
        });
    });
    true
}

// "2026-11-21T10:02:33.123Z" -> "2026-11-21 10:02"
fn short_date(s: &str) -> String {
    if s.len() >= 16 { s[..16].replace('T', " ") } else { s.to_string() }
}

fn type_icon(kind: &str) -> &'static str {
    match kind {
        "password" => "🔑",
        "api_key" => "🗝",
        "certificate" => "📜",
        "private_key" | "ssh_key" => "🔏",
        "env" => "📄",
        _ => "🔒",
    }
}

fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.18))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().color(color));
        });
}

// --- secret tree ---

#[derive(Default)]
struct Node<'a> {
    dirs: BTreeMap<&'a str, Node<'a>>,
    leaves: Vec<(&'a str, &'a SecretMeta)>,
}

fn build_tree<'a>(list: &'a [SecretMeta], filter: &str) -> Node<'a> {
    let mut root = Node::default();
    for s in list.iter().filter(|s| filter.is_empty() || s.path.to_lowercase().contains(filter)) {
        let mut parts: Vec<&str> = s.path.split('/').filter(|p| !p.is_empty()).collect();
        let name = parts.pop().unwrap_or(&s.path);
        let mut node = &mut root;
        for p in parts {
            node = node.dirs.entry(p).or_default();
        }
        node.leaves.push((name, s));
    }
    root
}

fn draw_tree(ui: &mut egui::Ui, node: &Node, prefix: &str, open_all: bool, selected: Option<&str>, out: &mut Option<String>) {
    for (dir, child) in &node.dirs {
        let id = format!("{prefix}{dir}/");
        egui::CollapsingHeader::new(format!("📁 {dir}"))
            .id_salt(&id)
            .default_open(true)
            .open(open_all.then_some(true))
            .show(ui, |ui| draw_tree(ui, child, &id, open_all, selected, out));
    }
    for (name, s) in &node.leaves {
        let sel = selected == Some(s.path.as_str());
        let r = ui.selectable_label(sel, format!("{} {}", type_icon(&s.kind), name));
        if r.clicked() {
            *out = Some(s.path.clone());
        }
        r.on_hover_text(format!("{}\n{} · v{}", s.path, s.kind, s.version));
    }
}

pub fn secrets(app: &mut App, ctx: &egui::Context) {
    if no_vault(ctx, app) {
        return;
    }
    let vault = app.selected_vault.clone().unwrap();

    egui::SidePanel::left("tree").resizable(true).default_width(280.0).show(ctx, |ui| {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let name = app.current_vault().map(|v| v.name.clone()).unwrap_or_default();
            ui.heading(name);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("＋ Secret").clicked() {
                    app.editor = Editor { open: true, is_new: true, kind: "generic".into(), ..Default::default() };
                }
            });
        });
        ui.add(egui::TextEdit::singleline(&mut app.secret_filter).hint_text("🔍 filter by path").desired_width(f32::INFINITY));
        ui.add_space(4.0);

        let filter = app.secret_filter.to_lowercase();
        let mut picked = None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| match &app.secrets {
            None => {
                ui.spinner();
            }
            Some(list) if list.is_empty() => {
                ui.label(RichText::new("This vault is empty.").weak());
            }
            Some(list) => {
                let tree = build_tree(list, &filter);
                if tree.dirs.is_empty() && tree.leaves.is_empty() {
                    ui.label(RichText::new("Nothing matches.").weak());
                }
                let sel = app.detail.as_ref().map(|d| d.path.as_str());
                draw_tree(ui, &tree, "", !filter.is_empty(), sel, &mut picked);
            }
        });
        if let Some(p) = picked {
            if app.detail.as_ref().map(|d| &d.path) != Some(&p) {
                app.open_secret(p);
            }
        }
    });

    egui::CentralPanel::default().show(ctx, |ui| {
        let meta = app
            .detail
            .as_ref()
            .and_then(|d| app.secrets.as_ref()?.iter().find(|s| s.path == d.path).cloned());
        match (app.detail.is_some(), meta) {
            (true, Some(meta)) => detail(app, ui, &vault, &meta),
            _ => {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Select a secret to see its details").weak().size(16.0));
                });
            }
        }
    });

    editor_window(app, ctx, &vault);
}

fn detail(app: &mut App, ui: &mut egui::Ui, vault: &str, meta: &SecretMeta) {
    let ctx = ui.ctx().clone();
    let mut jobs = vec![];
    let mut copy_now = None;
    let mut edit = false;
    let d: &mut Detail = app.detail.as_mut().unwrap();

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(type_icon(&meta.kind)).size(22.0));
            ui.heading(RichText::new(&meta.path).monospace().strong());
            if ui.small_button("⎘").on_hover_text("Copy path").clicked() {
                ctx.copy_text(meta.path.clone());
            }
        });
        ui.horizontal(|ui| {
            badge(ui, &meta.kind, ACCENT);
            badge(ui, &format!("v{}", meta.version), Color32::from_rgb(0x4A, 0x90, 0xE2));
            if let Some(e) = &meta.expires_at {
                badge(ui, &format!("expires {}", short_date(e)), Color32::from_rgb(0xE0, 0xA0, 0x30));
            }
            if meta.is_disabled == Some(true) {
                badge(ui, "disabled", Color32::GRAY);
            }
        });
        ui.add_space(10.0);

        egui::Grid::new("meta").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
            ui.label(RichText::new("Created").weak());
            ui.label(short_date(&meta.created_at));
            ui.end_row();
            if let Some(by) = d.loaded.as_ref().and_then(|s| s.created_by.as_deref()) {
                ui.label(RichText::new("Created by").weak());
                ui.label(by);
                ui.end_row();
            }
            ui.label(RichText::new("Secret ID").weak());
            ui.label(RichText::new(&meta.id).monospace().small());
            ui.end_row();
        });

        ui.add_space(12.0);
        ui.label(RichText::new("Value").strong());
        egui::Frame::group(ui.style()).corner_radius(8.0).inner_margin(10.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            match (&d.loaded, d.revealed) {
                (Some(s), true) => {
                    if s.version != meta.version {
                        ui.label(RichText::new(format!("showing old version v{}", s.version)).small().color(Color32::from_rgb(0xE0, 0xA0, 0x30)));
                    }
                    let mut v = s.value.as_str();
                    ui.add(
                        egui::TextEdit::multiline(&mut v)
                            .code_editor()
                            .desired_rows(1)
                            .desired_width(f32::INFINITY),
                    );
                }
                _ => {
                    ui.label(RichText::new("••••••••••••••••").monospace().size(16.0));
                }
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let label = if d.revealed { "🙈 Hide" } else { "👁 Reveal" };
                if ui.button(label).clicked() {
                    if d.revealed {
                        d.revealed = false;
                    } else if d.loaded.is_some() {
                        d.revealed = true;
                    } else {
                        jobs.push(Job::Secret { vault: vault.into(), path: meta.path.clone(), version: None });
                    }
                }
                if ui.button("⎘ Copy value").clicked() {
                    match &d.loaded {
                        Some(s) => copy_now = Some(s.value.clone()),
                        None => {
                            d.copy_when_loaded = true;
                            jobs.push(Job::Secret { vault: vault.into(), path: meta.path.clone(), version: None });
                        }
                    }
                }
                if ui.button("✏ New version").clicked() {
                    edit = true;
                }
            });
            ui.label(RichText::new("Each reveal or copy shows up in the vault's audit log.").weak().small());
        });

        if let Some(m) = meta.metadata.as_ref().filter(|m| m.as_object().is_some_and(|o| !o.is_empty())) {
            ui.add_space(10.0);
            ui.label(RichText::new("Metadata").strong());
            let mut txt = serde_json::to_string_pretty(m).unwrap_or_default();
            ui.add(egui::TextEdit::multiline(&mut txt).code_editor().desired_width(f32::INFINITY).desired_rows(2));
        }

        ui.add_space(10.0);
        let hist = egui::CollapsingHeader::new(RichText::new("History").strong()).id_salt(("hist", &meta.path)).show(ui, |ui| {
            match &d.versions {
                None => {
                    ui.spinner();
                }
                Some(list) if list.is_empty() => {
                    ui.label(RichText::new("No version history.").weak());
                }
                Some(list) => {
                    egui::Grid::new("versions").striped(true).num_columns(4).spacing([18.0, 4.0]).show(ui, |ui| {
                        for v in list {
                            ui.label(RichText::new(format!("v{}", v.version)).monospace());
                            ui.label(short_date(&v.created_at));
                            if v.is_disabled == Some(true) {
                                ui.label(RichText::new("disabled").weak());
                            } else {
                                ui.label("");
                            }
                            let viewing = d.revealed && d.loaded.as_ref().is_some_and(|s| s.version == v.version);
                            if ui.add_enabled(v.is_disabled != Some(true) && !viewing, egui::Button::new("View").small()).clicked() {
                                jobs.push(Job::Secret { vault: vault.into(), path: meta.path.clone(), version: Some(v.version) });
                            }
                            ui.end_row();
                        }
                    });
                }
            }
        });
        // lazy load history the first time the section gets expanded
        if hist.openness > 0.0 && !d.versions_requested {
            d.versions_requested = true;
            jobs.push(Job::Versions { vault: vault.into(), path: meta.path.clone() });
        }

        ui.add_space(20.0);
        ui.separator();
        if d.confirm_delete {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Delete this secret and all its versions?").color(Color32::from_rgb(0xE0, 0x4F, 0x4F)));
                if ui.button("Cancel").clicked() {
                    d.confirm_delete = false;
                }
                if ui.add(egui::Button::new(RichText::new("Delete").color(Color32::WHITE)).fill(Color32::from_rgb(0xC0, 0x39, 0x2B))).clicked() {
                    jobs.push(Job::DeleteSecret { vault: vault.into(), path: meta.path.clone() });
                    d.confirm_delete = false;
                }
            });
        } else if ui.button(RichText::new("🗑 Delete secret").color(Color32::from_rgb(0xE0, 0x4F, 0x4F))).clicked() {
            d.confirm_delete = true;
        }
    });

    if edit {
        app.editor = Editor {
            open: true,
            is_new: false,
            path: meta.path.clone(),
            kind: meta.kind.clone(),
            expires_at: meta.expires_at.clone().unwrap_or_default(),
            ..Default::default()
        };
    }
    if let Some(v) = copy_now {
        app.copy(&ctx, v);
    }
    for j in jobs {
        app.job(j);
    }
}

fn editor_window(app: &mut App, ctx: &egui::Context, vault: &str) {
    if !app.editor.open {
        return;
    }
    let mut open = true;
    let mut save = false;
    let e = &mut app.editor;
    let title = if e.is_new { "New secret" } else { "New version" };
    egui::Window::new(title)
        .collapsible(false)
        .open(&mut open)
        .default_width(440.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label("Path");
            ui.add_enabled(
                e.is_new,
                egui::TextEdit::singleline(&mut e.path).hint_text("e.g. db/prod/password").desired_width(f32::INFINITY),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Type");
                egui::ComboBox::from_id_salt("kind").selected_text(e.kind.as_str()).show_ui(ui, |ui| {
                    for t in SECRET_TYPES {
                        ui.selectable_value(&mut e.kind, t.to_string(), *t);
                    }
                });
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Value");
                ui.checkbox(&mut e.show_value, "show");
            });
            let multiline = matches!(e.kind.as_str(), "certificate" | "private_key" | "ssh_key" | "env");
            if multiline && e.show_value {
                ui.add(egui::TextEdit::multiline(&mut e.value).code_editor().desired_rows(6).desired_width(f32::INFINITY));
            } else {
                ui.add(egui::TextEdit::singleline(&mut e.value).password(!e.show_value).desired_width(f32::INFINITY));
            }
            ui.add_space(4.0);
            ui.label("Expires at (optional, RFC 3339)");
            ui.add(egui::TextEdit::singleline(&mut e.expires_at).hint_text("2027-01-01T00:00:00Z").desired_width(f32::INFINITY));
            ui.add_space(8.0);

            let path_ok = !e.path.trim().trim_matches('/').is_empty() && !e.path.contains("//");
            let ok = path_ok && !e.value.is_empty();
            if !path_ok && !e.path.is_empty() {
                ui.colored_label(Color32::from_rgb(0xE0, 0x4F, 0x4F), "path looks off");
            }
            let btn = egui::Button::new(RichText::new("Save").strong().color(Color32::WHITE)).fill(ACCENT);
            save = ui.add_enabled(ok, btn).clicked();
        });

    if save {
        let e = &app.editor;
        let exp = e.expires_at.trim();
        app.job(Job::PutSecret {
            vault: vault.into(),
            path: e.path.trim().trim_matches('/').to_string(),
            kind: e.kind.clone(),
            value: e.value.clone(),
            expires_at: (!exp.is_empty()).then(|| exp.to_string()),
        });
    }
    if !open {
        app.editor = Editor::default();
    }
}

// --- policies ---

pub fn policies(app: &mut App, ctx: &egui::Context) {
    if no_vault(ctx, app) {
        return;
    }
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.add_space(6.0);
        ui.heading("Access policies");
        ui.label(RichText::new("Who can touch which paths in this vault").weak());
        ui.add_space(8.0);
        let Some(list) = &app.policies else {
            ui.spinner();
            return;
        };
        if list.is_empty() {
            ui.label(RichText::new("No policies - only the vault owner has access.").weak());
            return;
        }
        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            egui::Grid::new("pol").striped(true).num_columns(5).spacing([20.0, 8.0]).show(ui, |ui| {
                for h in ["Path pattern", "Principal", "Permissions", "Effect", "Expires"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for p in list {
                    ui.label(RichText::new(&p.secret_path_pattern).monospace());
                    ui.label(format!("{} · {}", p.principal_type, short_id(&p.principal_id)))
                        .on_hover_text(&p.principal_id);
                    ui.horizontal(|ui| {
                        for perm in &p.permissions {
                            badge(ui, perm, Color32::from_rgb(0x4A, 0x90, 0xE2));
                        }
                    });
                    let eff = p.effect.as_deref().unwrap_or("allow");
                    let c = if eff == "deny" { Color32::from_rgb(0xE0, 0x4F, 0x4F) } else { Color32::from_rgb(0x3F, 0xB9, 0x50) };
                    badge(ui, eff, c);
                    ui.label(p.expires_at.as_deref().map(short_date).unwrap_or_else(|| "never".into()));
                    ui.end_row();
                }
            });
        });
    });
}

fn short_id(s: &str) -> &str {
    if s.len() > 13 { &s[..8] } else { s }
}

// --- audit log ---

pub fn activity(app: &mut App, ctx: &egui::Context) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.heading("Activity");
            ui.add_space(12.0);
            let before = app.audit_vault_only;
            ui.add_enabled_ui(app.selected_vault.is_some(), |ui| {
                ui.selectable_value(&mut app.audit_vault_only, true, "This vault");
            });
            ui.selectable_value(&mut app.audit_vault_only, false, "Everything");
            if before != app.audit_vault_only {
                app.audit = None;
                app.load_tab();
            }
        });
        ui.add(egui::TextEdit::singleline(&mut app.audit_filter).hint_text("🔍 filter by action or actor").desired_width(320.0));
        ui.add_space(6.0);

        let Some(list) = &app.audit else {
            ui.spinner();
            return;
        };
        let q = app.audit_filter.to_lowercase();
        let rows: Vec<_> = list
            .iter()
            .filter(|e| q.is_empty() || e.action.to_lowercase().contains(&q) || e.actor_id.to_lowercase().contains(&q))
            .collect();
        if rows.is_empty() {
            ui.label(RichText::new("No events.").weak());
            return;
        }
        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            egui::Grid::new("audit").striped(true).num_columns(5).spacing([20.0, 6.0]).show(ui, |ui| {
                for h in ["When", "Action", "Actor", "Resource", "IP"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for e in rows {
                    ui.label(short_date(&e.created_at));
                    let c = if e.action.contains("delete") {
                        Color32::from_rgb(0xE0, 0x4F, 0x4F)
                    } else if e.action.contains("read") {
                        Color32::from_rgb(0x4A, 0x90, 0xE2)
                    } else {
                        ACCENT
                    };
                    let r = ui.label(RichText::new(&e.action).monospace().color(c));
                    if let Some(det) = e.details.as_ref().filter(|d| !d.is_null()) {
                        r.on_hover_text(serde_json::to_string_pretty(det).unwrap_or_default());
                    }
                    ui.label(format!("{} {}", e.actor_type, short_id(&e.actor_id))).on_hover_text(&e.actor_id);
                    ui.label(format!("{} {}", e.resource_type, short_id(&e.resource_id))).on_hover_text(&e.resource_id);
                    ui.label(e.ip_address.as_deref().unwrap_or("-"));
                    ui.end_row();
                }
            });
        });
    });
}
