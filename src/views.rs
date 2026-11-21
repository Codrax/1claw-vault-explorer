use crate::api::SecretMeta;
use crate::app::{App, Detail, Editor, dialog, field_label, search_bar, segmented};
use crate::theme::{self, Role, Scheme, chip};
use crate::worker::Job;
use eframe::egui::{self, CornerRadius, Margin, RichText};
use std::collections::BTreeMap;

const SECRET_TYPES: &[&str] = &["generic", "password", "api_key", "certificate", "private_key", "ssh_key", "env"];

// Panes sit as rounded "sheets" on top of the container colored background,
// like the M3 list-detail layout.
fn pane_panel(sc: &Scheme, left: i8, right: i8) -> egui::Frame {
    egui::Frame::new()
        .fill(sc.surface_container_low)
        .inner_margin(Margin { left, right, top: 0, bottom: 12 })
}

fn sheet(ui: &mut egui::Ui, sc: &Scheme, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(sc.surface)
        .corner_radius(CornerRadius::same(24))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            add(ui)
        });
}

fn empty_state(ui: &mut egui::Ui, sc: &Scheme, icon: &str, text: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.3).max(10.0));
        ui.label(RichText::new(icon).size(40.0).color(sc.outline));
        ui.add_space(6.0);
        ui.label(RichText::new(text).size(15.0).color(sc.on_surface_variant));
    });
}

fn no_vault(ctx: &egui::Context, app: &App) -> bool {
    if app.selected_vault.is_some() {
        return false;
    }
    let sc = theme::scheme(ctx);
    egui::CentralPanel::default().frame(pane_panel(&sc, 0, 12)).show(ctx, |ui| {
        sheet(ui, &sc, |ui| empty_state(ui, &sc, "🗄", "Pick a vault on the left to get started"));
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

fn draw_tree(
    ui: &mut egui::Ui,
    sc: &Scheme,
    node: &Node,
    prefix: &str,
    open_all: bool,
    selected: Option<&str>,
    out: &mut Option<String>,
) {
    for (dir, child) in &node.dirs {
        let id = format!("{prefix}{dir}/");
        egui::CollapsingHeader::new(RichText::new(format!("🗁  {dir}")).color(sc.on_surface_variant))
            .id_salt(&id)
            .default_open(true)
            .open(open_all.then_some(true))
            .show(ui, |ui| draw_tree(ui, sc, child, &id, open_all, selected, out));
    }
    for (name, s) in &node.leaves {
        let sel = selected == Some(s.path.as_str());
        let r = theme::nav_item(ui, sc, &format!("{}   {}", type_icon(&s.kind), name), sel, 36.0);
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
    let sc = theme::scheme(ctx);
    let vault = app.selected_vault.clone().unwrap();

    egui::SidePanel::left("tree")
        .resizable(true)
        .show_separator_line(false)
        .default_width(300.0)
        .frame(pane_panel(&sc, 0, 6))
        .show(ctx, |ui| {
            sheet(ui, &sc, |ui| {
                let (name, desc) = app
                    .current_vault()
                    .map(|v| (v.name.clone(), v.description.clone().unwrap_or_default()))
                    .unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(name).size(20.0).color(sc.on_surface));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(sc.tonal("+ Secret")).clicked() {
                            app.editor = Editor { open: true, is_new: true, kind: "generic".into(), ..Default::default() };
                        }
                    });
                });
                if !desc.is_empty() {
                    ui.label(RichText::new(desc).small().color(sc.on_surface_variant));
                }
                ui.add_space(6.0);
                search_bar(ui, &mut app.secret_filter, "Filter by path");
                ui.add_space(6.0);

                let filter = app.secret_filter.to_lowercase();
                let mut picked = None;
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| match &app.secrets {
                    None => {
                        ui.add(egui::Spinner::new().color(sc.primary));
                    }
                    Some(list) if list.is_empty() => empty_state(ui, &sc, "🫙", "This vault is empty"),
                    Some(list) => {
                        let tree = build_tree(list, &filter);
                        if tree.dirs.is_empty() && tree.leaves.is_empty() {
                            empty_state(ui, &sc, "🔍", "Nothing matches");
                        }
                        let sel = app.detail.as_ref().map(|d| d.path.as_str());
                        draw_tree(ui, &sc, &tree, "", !filter.is_empty(), sel, &mut picked);
                    }
                });
                if let Some(p) = picked {
                    if app.detail.as_ref().map(|d| &d.path) != Some(&p) {
                        app.open_secret(p);
                    }
                }
            });
        });

    egui::CentralPanel::default().frame(pane_panel(&sc, 6, 12)).show(ctx, |ui| {
        let meta = app
            .detail
            .as_ref()
            .and_then(|d| app.secrets.as_ref()?.iter().find(|s| s.path == d.path).cloned());
        sheet(ui, &sc, |ui| match (app.detail.is_some(), meta) {
            (true, Some(meta)) => detail(app, ui, &sc, &vault, &meta),
            _ => empty_state(ui, &sc, "🔐", "Select a secret to see its details"),
        });
    });

    editor_window(app, ctx, &vault);
}

fn detail(app: &mut App, ui: &mut egui::Ui, sc: &Scheme, vault: &str, meta: &SecretMeta) {
    let ctx = ui.ctx().clone();
    let mut jobs = vec![];
    let mut copy_now = None;
    let mut edit = false;
    let d: &mut Detail = app.detail.as_mut().unwrap();

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        // header
        ui.horizontal(|ui| {
            egui::Frame::new()
                .fill(sc.primary_container)
                .corner_radius(CornerRadius::same(16))
                .inner_margin(Margin::symmetric(12, 8))
                .show(ui, |ui| ui.label(RichText::new(type_icon(&meta.kind)).size(22.0).color(sc.on_primary_container)));
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&meta.path).monospace().size(18.0).color(sc.on_surface));
                    if ui.add(sc.text_button("🗐", sc.on_surface_variant)).on_hover_text("Copy path").clicked() {
                        ctx.copy_text(meta.path.clone());
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    chip(ui, sc, &meta.kind, Role::Primary);
                    chip(ui, sc, &format!("v{}", meta.version), Role::Secondary);
                    if let Some(e) = &meta.expires_at {
                        chip(ui, sc, &format!("⏰ expires {}", short_date(e)), Role::Tertiary);
                    }
                    if meta.is_disabled == Some(true) {
                        chip(ui, sc, "disabled", Role::Neutral);
                    }
                });
            });
        });
        ui.add_space(16.0);

        // value card
        sc.card(1).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Value").strong().color(sc.on_surface));
                if let Some(s) = d.loaded.as_ref().filter(|s| d.revealed && s.version != meta.version) {
                    chip(ui, sc, &format!("viewing old v{}", s.version), Role::Tertiary);
                }
            });
            ui.add_space(4.0);
            match (&d.loaded, d.revealed) {
                (Some(s), true) => {
                    egui::Frame::new()
                        .fill(sc.surface_container_highest)
                        .corner_radius(CornerRadius::same(12))
                        .inner_margin(Margin::same(6))
                        .show(ui, |ui| {
                            let mut v = s.value.as_str();
                            ui.add(
                                egui::TextEdit::multiline(&mut v)
                                    .code_editor()
                                    .frame(false)
                                    .margin(Margin::same(6))
                                    .desired_rows(1)
                                    .desired_width(f32::INFINITY),
                            );
                        });
                }
                _ => {
                    egui::Frame::new()
                        .fill(sc.surface_container_highest)
                        .corner_radius(CornerRadius::same(12))
                        .inner_margin(Margin::same(12))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new("• • • • • • • • • • • •").monospace().size(16.0).color(sc.on_surface_variant));
                        });
                }
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                let label = if d.revealed { "🙈  Hide" } else { "👁  Reveal" };
                if ui.add(sc.tonal(label)).clicked() {
                    if d.revealed {
                        d.revealed = false;
                    } else if d.loaded.is_some() {
                        d.revealed = true;
                    } else {
                        jobs.push(Job::Secret { vault: vault.into(), path: meta.path.clone(), version: None });
                    }
                }
                if ui.add(sc.tonal("🗐  Copy")).clicked() {
                    match &d.loaded {
                        Some(s) => copy_now = Some(s.value.clone()),
                        None => {
                            d.copy_when_loaded = true;
                            jobs.push(Job::Secret { vault: vault.into(), path: meta.path.clone(), version: None });
                        }
                    }
                }
                if ui.add(sc.filled("✏  New version").min_size(egui::vec2(0.0, 36.0))).clicked() {
                    edit = true;
                }
            });
            ui.add_space(2.0);
            ui.label(RichText::new("Every reveal or copy shows up in the vault's audit log.").small().color(sc.on_surface_variant));
        });
        ui.add_space(12.0);

        // info card
        sc.card(1).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Details").strong().color(sc.on_surface));
            ui.add_space(4.0);
            egui::Grid::new("meta").num_columns(2).spacing([24.0, 8.0]).show(ui, |ui| {
                let row = |ui: &mut egui::Ui, k: &str, v: RichText| {
                    ui.label(RichText::new(k).color(sc.on_surface_variant));
                    ui.label(v);
                    ui.end_row();
                };
                row(ui, "Created", RichText::new(short_date(&meta.created_at)));
                if let Some(by) = d.loaded.as_ref().and_then(|s| s.created_by.as_deref()) {
                    row(ui, "Created by", RichText::new(by));
                }
                row(ui, "Secret ID", RichText::new(&meta.id).monospace().small());
            });
            if let Some(m) = meta.metadata.as_ref().filter(|m| m.as_object().is_some_and(|o| !o.is_empty())) {
                ui.add_space(8.0);
                field_label(ui, sc, "Metadata");
                let mut txt = serde_json::to_string_pretty(m).unwrap_or_default();
                ui.add(
                    egui::TextEdit::multiline(&mut txt)
                        .code_editor()
                        .margin(Margin::same(10))
                        .desired_width(f32::INFINITY)
                        .desired_rows(2),
                );
            }
        });
        ui.add_space(12.0);

        // history card
        sc.card(1).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let hist = egui::CollapsingHeader::new(RichText::new("History").strong().color(sc.on_surface))
                .id_salt(("hist", &meta.path))
                .show(ui, |ui| match &d.versions {
                    None => {
                        ui.add(egui::Spinner::new().color(sc.primary));
                    }
                    Some(list) if list.is_empty() => {
                        ui.label(RichText::new("No version history.").color(sc.on_surface_variant));
                    }
                    Some(list) => {
                        for v in list {
                            ui.horizontal(|ui| {
                                chip(ui, sc, &format!("v{}", v.version), if v.version == meta.version { Role::Primary } else { Role::Neutral });
                                ui.label(RichText::new(short_date(&v.created_at)).color(sc.on_surface_variant));
                                if v.is_disabled == Some(true) {
                                    chip(ui, sc, "disabled", Role::Neutral);
                                }
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let viewing = d.revealed && d.loaded.as_ref().is_some_and(|s| s.version == v.version);
                                    let label = if viewing { "Viewing" } else { "View" };
                                    let b = sc.text_button(label, sc.primary);
                                    if ui.add_enabled(v.is_disabled != Some(true) && !viewing, b).clicked() {
                                        jobs.push(Job::Secret { vault: vault.into(), path: meta.path.clone(), version: Some(v.version) });
                                    }
                                });
                            });
                        }
                    }
                });
            // lazy load history the first time the section gets expanded
            if hist.openness > 0.0 && !d.versions_requested {
                d.versions_requested = true;
                jobs.push(Job::Versions { vault: vault.into(), path: meta.path.clone() });
            }
        });

        ui.add_space(16.0);
        if d.confirm_delete {
            egui::Frame::new()
                .fill(sc.error_container)
                .corner_radius(CornerRadius::same(16))
                .inner_margin(Margin::same(14))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Delete this secret and all its versions? This can't be undone.").color(sc.on_error_container));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.add(sc.danger("Delete")).clicked() {
                            jobs.push(Job::DeleteSecret { vault: vault.into(), path: meta.path.clone() });
                            d.confirm_delete = false;
                        }
                        if ui.add(sc.text_button("Cancel", sc.on_error_container)).clicked() {
                            d.confirm_delete = false;
                        }
                    });
                });
        } else if ui.add(sc.text_button("🗑  Delete secret", sc.error)).clicked() {
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
    let sc = theme::scheme(ctx);
    let mut cancel = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let mut save = false;
    let e = &mut app.editor;
    let title = if e.is_new { "New secret" } else { "New version" };
    dialog(ctx, title, |ui| {
        ui.set_width(460.0);
        field_label(ui, &sc, "Path");
        ui.add_enabled(
            e.is_new,
            egui::TextEdit::singleline(&mut e.path)
                .hint_text("e.g. db/prod/password")
                .margin(Margin::symmetric(14, 10))
                .desired_width(f32::INFINITY),
        );
        ui.add_space(6.0);
        field_label(ui, &sc, "Type");
        // filter chips instead of a dropdown
        ui.horizontal_wrapped(|ui| {
            for t in SECRET_TYPES {
                let sel = e.kind == *t;
                let (fill, fg) = if sel { (sc.secondary_container, sc.on_secondary_container) } else { (sc.surface_container_high, sc.on_surface_variant) };
                let text = if sel { format!("✔ {t}") } else { t.to_string() };
                let b = egui::Button::new(RichText::new(text).color(fg))
                    .fill(fill)
                    .stroke(egui::Stroke::new(1.0f32, if sel { fill } else { sc.outline_variant }))
                    .corner_radius(CornerRadius::same(8))
                    .min_size(egui::vec2(0.0, 30.0));
                if ui.add(b).clicked() {
                    e.kind = t.to_string();
                }
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            field_label(ui, &sc, "Value");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.checkbox(&mut e.show_value, "show");
            });
        });
        let multiline = matches!(e.kind.as_str(), "certificate" | "private_key" | "ssh_key" | "env");
        if multiline && e.show_value {
            ui.add(
                egui::TextEdit::multiline(&mut e.value)
                    .code_editor()
                    .margin(Margin::same(12))
                    .desired_rows(6)
                    .desired_width(f32::INFINITY),
            );
        } else {
            ui.add(
                egui::TextEdit::singleline(&mut e.value)
                    .password(!e.show_value)
                    .margin(Margin::symmetric(14, 10))
                    .desired_width(f32::INFINITY),
            );
        }
        ui.add_space(6.0);
        field_label(ui, &sc, "Expires at (optional, RFC 3339)");
        ui.add(
            egui::TextEdit::singleline(&mut e.expires_at)
                .hint_text("2027-01-01T00:00:00Z")
                .margin(Margin::symmetric(14, 10))
                .desired_width(f32::INFINITY),
        );

        let path_ok = !e.path.trim().trim_matches('/').is_empty() && !e.path.contains("//");
        if !path_ok && !e.path.is_empty() {
            ui.label(RichText::new("That path looks off").small().color(sc.error));
        }
        ui.add_space(14.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            save = ui.add_enabled(path_ok && !e.value.is_empty(), sc.filled("Save")).clicked();
            if ui.add(sc.text_button("Cancel", sc.primary)).clicked() {
                cancel = true;
            }
        });
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
    } else if cancel {
        app.editor = Editor::default();
    }
}

// --- policies ---

pub fn policies(app: &mut App, ctx: &egui::Context) {
    if no_vault(ctx, app) {
        return;
    }
    let sc = theme::scheme(ctx);
    egui::CentralPanel::default().frame(pane_panel(&sc, 0, 12)).show(ctx, |ui| {
        sheet(ui, &sc, |ui| {
            ui.label(RichText::new("Access policies").size(22.0).color(sc.on_surface));
            ui.label(RichText::new("Who can touch which paths in this vault").color(sc.on_surface_variant));
            ui.add_space(12.0);
            let Some(list) = &app.policies else {
                ui.add(egui::Spinner::new().color(sc.primary));
                return;
            };
            if list.is_empty() {
                empty_state(ui, &sc, "🛡", "No policies - only the vault owner has access");
                return;
            }
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                for p in list {
                    sc.card(1).corner_radius(CornerRadius::same(16)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&p.secret_path_pattern).monospace().size(15.0).color(sc.on_surface));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let eff = p.effect.as_deref().unwrap_or("allow");
                                chip(ui, &sc, eff, if eff == "deny" { Role::Error } else { Role::Tertiary });
                            });
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(format!("👤 {} · {}", p.principal_type, short_id(&p.principal_id))).color(sc.on_surface_variant))
                                .on_hover_text(&p.principal_id);
                            ui.label(RichText::new("·").color(sc.outline));
                            for perm in &p.permissions {
                                chip(ui, &sc, perm, Role::Secondary);
                            }
                            ui.label(RichText::new("·").color(sc.outline));
                            let exp = p.expires_at.as_deref().map(short_date).unwrap_or_else(|| "never expires".into());
                            ui.label(RichText::new(exp).small().color(sc.on_surface_variant));
                        });
                    });
                    ui.add_space(6.0);
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
    let sc = theme::scheme(ctx);
    egui::CentralPanel::default().frame(pane_panel(&sc, 0, 12)).show(ctx, |ui| {
        sheet(ui, &sc, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Activity").size(22.0).color(sc.on_surface));
                ui.add_space(12.0);
                let before = app.audit_vault_only;
                if app.selected_vault.is_some() {
                    segmented(ui, &sc, &mut app.audit_vault_only, &[(true, "This vault"), (false, "Everything")]);
                } else {
                    app.audit_vault_only = false;
                }
                if before != app.audit_vault_only {
                    app.audit = None;
                    app.load_tab();
                }
            });
            ui.add_space(8.0);
            ui.scope(|ui| {
                ui.set_max_width(380.0);
                search_bar(ui, &mut app.audit_filter, "Filter by action or actor");
            });
            ui.add_space(8.0);

            let Some(list) = &app.audit else {
                ui.add(egui::Spinner::new().color(sc.primary));
                return;
            };
            let q = app.audit_filter.to_lowercase();
            let rows: Vec<_> = list
                .iter()
                .filter(|e| q.is_empty() || e.action.to_lowercase().contains(&q) || e.actor_id.to_lowercase().contains(&q))
                .collect();
            if rows.is_empty() {
                empty_state(ui, &sc, "📜", "No events");
                return;
            }
            egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                egui::Grid::new("audit").striped(true).num_columns(5).spacing([24.0, 10.0]).show(ui, |ui| {
                    for h in ["When", "Action", "Actor", "Resource", "IP"] {
                        ui.label(RichText::new(h).small().strong().color(sc.on_surface_variant));
                    }
                    ui.end_row();
                    for e in rows {
                        ui.label(RichText::new(short_date(&e.created_at)).color(sc.on_surface_variant));
                        let role = if e.action.contains("delete") {
                            Role::Error
                        } else if e.action.contains("read") {
                            Role::Secondary
                        } else if e.action.contains("create") {
                            Role::Primary
                        } else {
                            Role::Tertiary
                        };
                        let r = ui.scope(|ui| chip(ui, &sc, &e.action, role)).response;
                        if let Some(det) = e.details.as_ref().filter(|d| !d.is_null()) {
                            r.on_hover_text(serde_json::to_string_pretty(det).unwrap_or_default());
                        }
                        ui.label(format!("{} {}", e.actor_type, short_id(&e.actor_id))).on_hover_text(&e.actor_id);
                        ui.label(format!("{} {}", e.resource_type, short_id(&e.resource_id))).on_hover_text(&e.resource_id);
                        ui.label(RichText::new(e.ip_address.as_deref().unwrap_or("-")).monospace().color(sc.on_surface_variant));
                        ui.end_row();
                    }
                });
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(path: &str) -> SecretMeta {
        serde_json::from_value(serde_json::json!({ "id": path, "path": path, "type": "generic", "version": 1 })).unwrap()
    }

    #[test]
    fn tree_groups_by_folder() {
        let list = vec![meta("db/prod/pw"), meta("db/staging/pw"), meta("stripe_key")];
        let t = build_tree(&list, "");
        assert_eq!(t.leaves.len(), 1);
        assert_eq!(t.dirs["db"].dirs.len(), 2);
    }

    #[test]
    fn tree_filter() {
        let list = vec![meta("db/prod/pw"), meta("stripe_key")];
        let t = build_tree(&list, "stripe");
        assert!(t.dirs.is_empty());
        assert_eq!(t.leaves.len(), 1);
    }
}
