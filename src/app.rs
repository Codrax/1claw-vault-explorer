use crate::api::{AuditEvent, Credentials, KeyKind, Policy, Secret, SecretMeta, Vault};
use crate::config::Config;
use crate::worker::{Job, Msg, Worker};
use crate::theme;
use eframe::egui::{self, Color32, CornerRadius, Margin, RichText};
use std::time::{Duration, Instant};

pub const CLIPBOARD_CLEAR_AFTER: Duration = Duration::from_secs(30);

#[derive(PartialEq, Clone, Copy)]
pub enum Tab {
    Secrets,
    Policies,
    Activity,
}

#[derive(PartialEq)]
enum Screen {
    Login,
    Main,
}

pub struct Toast {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

/// What's open on the right hand side for the selected secret.
#[derive(Default)]
pub struct Detail {
    pub path: String,
    pub loaded: Option<Secret>,
    pub revealed: bool,
    pub versions: Option<Vec<SecretMeta>>,
    pub copy_when_loaded: bool,
    pub confirm_delete: bool,
    pub versions_requested: bool,
}

#[derive(Default)]
pub struct Editor {
    pub open: bool,
    pub is_new: bool,
    pub path: String,
    pub kind: String,
    pub value: String,
    pub expires_at: String,
    pub show_value: bool,
}

pub struct App {
    pub worker: Worker,
    pub cfg: Config,
    screen: Screen,
    api_key: String,
    login_error: Option<String>,
    pub busy: usize,

    pub vaults: Vec<Vault>,
    pub vault_filter: String,
    pub selected_vault: Option<String>,
    pub new_vault: Option<(String, String)>,
    pub confirm_vault_delete: Option<String>,

    pub tab: Tab,
    pub secrets: Option<Vec<SecretMeta>>,
    pub secret_filter: String,
    pub detail: Option<Detail>,
    pub editor: Editor,
    pub policies: Option<Vec<Policy>>,
    pub audit: Option<Vec<AuditEvent>>,
    pub audit_vault_only: bool,
    pub audit_filter: String,

    pub toast: Option<Toast>,
    pub clipboard_set_at: Option<Instant>,
    sized: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let cfg = Config::load();
        theme::apply(&cc.egui_ctx, cfg.seed, cfg.dark);
        let mut app = Self {
            worker: Worker::spawn(cc.egui_ctx.clone()),
            cfg,
            screen: Screen::Login,
            api_key: String::new(),
            login_error: None,
            busy: 0,
            vaults: vec![],
            vault_filter: String::new(),
            selected_vault: None,
            new_vault: None,
            confirm_vault_delete: None,
            tab: Tab::Secrets,
            secrets: None,
            secret_filter: String::new(),
            detail: None,
            editor: Editor::default(),
            policies: None,
            audit: None,
            audit_vault_only: true,
            audit_filter: String::new(),
            toast: None,
            clipboard_set_at: None,
            sized: false,
        };
        // handy for scripting / demos: ONECLAW_API_KEY=... vault-explorer
        if let Ok(k) = std::env::var("ONECLAW_API_KEY") {
            if !k.trim().is_empty() {
                app.api_key = k.trim().to_string();
                if app.api_key.starts_with("ocv_") {
                    app.cfg.kind = KeyKind::Agent;
                }
                app.connect(false);
                return app;
            }
        }
        if app.cfg.remember && !app.cfg.api_key.is_empty() {
            app.api_key = app.cfg.api_key.clone();
            app.connect(false);
        }
        app
    }

    pub fn job(&mut self, j: Job) {
        self.busy += 1;
        self.worker.send(j);
    }

    pub fn info(&mut self, t: impl Into<String>) {
        self.toast = Some(Toast { text: t.into(), error: false, at: Instant::now() });
    }

    pub fn error(&mut self, t: impl Into<String>) {
        self.toast = Some(Toast { text: t.into(), error: true, at: Instant::now() });
    }

    /// `persist` is false for keys that didn't come from the form (env var,
    /// already saved) so we don't overwrite what's on disk with them.
    fn connect(&mut self, persist: bool) {
        self.login_error = None;
        if persist {
            if self.cfg.remember {
                self.cfg.api_key = self.api_key.trim().to_string();
            } else {
                self.cfg.api_key.clear();
            }
        }
        let creds = Credentials {
            base_url: std::env::var("ONECLAW_BASE_URL").unwrap_or_else(|_| self.cfg.base_url.trim().to_string()),
            kind: self.cfg.kind,
            api_key: self.api_key.trim().to_string(),
            agent_id: Some(self.cfg.agent_id.trim().to_string()),
        };
        self.cfg.save();
        self.job(Job::Login(creds));
    }

    fn logout(&mut self) {
        // wipe everything that came from the server
        self.screen = Screen::Login;
        self.api_key.clear();
        // signing out on purpose means forget the key, the checkbox stays ticked though
        if !self.cfg.api_key.is_empty() {
            self.cfg.api_key.clear();
            self.cfg.save();
        }
        self.worker.send(Job::Logout);
        self.vaults.clear();
        self.selected_vault = None;
        self.reset_vault_view();
    }

    pub fn reset_vault_view(&mut self) {
        self.secrets = None;
        self.detail = None;
        self.policies = None;
        self.audit = None;
        self.editor = Editor::default();
        self.secret_filter.clear();
    }

    pub fn select_vault(&mut self, id: String) {
        if self.selected_vault.as_deref() == Some(&id) {
            return;
        }
        self.reset_vault_view();
        self.selected_vault = Some(id);
        self.load_tab();
    }

    /// Fetch whatever the current tab needs, if we don't have it yet.
    pub fn load_tab(&mut self) {
        let v = self.selected_vault.clone();
        match (self.tab, v) {
            (Tab::Secrets, Some(v)) if self.secrets.is_none() => self.job(Job::Secrets(v)),
            (Tab::Policies, Some(v)) if self.policies.is_none() => self.job(Job::Policies(v)),
            // activity works without a vault too, it just shows the whole org
            (Tab::Activity, v) if self.audit.is_none() => {
                let rid = v.filter(|_| self.audit_vault_only);
                self.job(Job::Audit { resource_id: rid })
            }
            _ => {}
        }
    }

    pub fn refresh(&mut self) {
        self.job(Job::Vaults);
        match self.tab {
            Tab::Secrets => self.secrets = None,
            Tab::Policies => self.policies = None,
            Tab::Activity => self.audit = None,
        }
        self.load_tab();
    }

    pub fn current_vault(&self) -> Option<&Vault> {
        let id = self.selected_vault.as_deref()?;
        self.vaults.iter().find(|v| v.id == id)
    }

    pub fn copy(&mut self, ctx: &egui::Context, text: String) {
        ctx.copy_text(text);
        self.clipboard_set_at = Some(Instant::now());
        self.info(format!("Copied - clipboard clears in {}s", CLIPBOARD_CLEAR_AFTER.as_secs()));
    }

    fn handle(&mut self, ctx: &egui::Context, m: Msg) {
        let in_vault = |s: &Self, v: &str| s.selected_vault.as_deref() == Some(v);
        match m {
            Msg::Done => self.busy = self.busy.saturating_sub(1),
            Msg::LoggedIn => {
                self.screen = Screen::Main;
                self.api_key.clear(); // client holds it now, no need to keep a copy in the form
            }
            Msg::LoginFailed(e) => self.login_error = Some(e),
            Msg::Error(e) => self.error(e),
            Msg::Vaults(mut list) => {
                list.sort_by_key(|v| v.name.to_lowercase());
                self.vaults = list;
                if self.selected_vault.as_ref().is_some_and(|s| !self.vaults.iter().any(|v| &v.id == s)) {
                    self.selected_vault = None;
                    self.reset_vault_view();
                }
                if self.selected_vault.is_none() {
                    if let Some(first) = self.vaults.first().map(|v| v.id.clone()) {
                        self.select_vault(first);
                    }
                }
            }
            Msg::VaultCreated(v) => {
                self.info(format!("Vault \"{}\" created", v.name));
                let id = v.id.clone();
                self.vaults.push(v);
                self.vaults.sort_by_key(|v| v.name.to_lowercase());
                self.select_vault(id);
            }
            Msg::VaultDeleted(id) => {
                self.vaults.retain(|v| v.id != id);
                if self.selected_vault.as_deref() == Some(&id) {
                    self.selected_vault = None;
                    self.reset_vault_view();
                }
                self.info("Vault deleted");
            }
            Msg::Secrets { vault, mut list } => {
                if in_vault(self, &vault) {
                    list.sort_by(|a, b| a.path.cmp(&b.path));
                    self.secrets = Some(list);
                }
            }
            Msg::Secret { vault, secret } => {
                if !in_vault(self, &vault) {
                    return;
                }
                if let Some(d) = self.detail.as_mut().filter(|d| d.path == secret.path) {
                    if d.copy_when_loaded {
                        d.copy_when_loaded = false;
                        let val = secret.value.clone();
                        d.loaded = Some(secret);
                        self.copy(ctx, val);
                    } else {
                        d.loaded = Some(secret);
                        d.revealed = true;
                    }
                }
            }
            Msg::Versions { vault, path, list } => {
                if in_vault(self, &vault) {
                    if let Some(d) = self.detail.as_mut().filter(|d| d.path == path) {
                        d.versions = Some(list);
                    }
                }
            }
            Msg::SecretSaved { vault, meta } => {
                self.info(format!("Saved {} (v{})", meta.path, meta.version));
                if in_vault(self, &vault) {
                    self.editor.open = false;
                    self.editor.value.clear();
                    self.open_secret(meta.path.clone());
                    self.job(Job::Secrets(vault));
                }
            }
            Msg::SecretDeleted { vault, path } => {
                self.info(format!("Deleted {path}"));
                if in_vault(self, &vault) {
                    if let Some(list) = self.secrets.as_mut() {
                        list.retain(|s| s.path != path);
                    }
                    if self.detail.as_ref().is_some_and(|d| d.path == path) {
                        self.detail = None;
                    }
                }
            }
            Msg::Policies { vault, list } => {
                if in_vault(self, &vault) {
                    self.policies = Some(list);
                }
            }
            Msg::Audit(list) => self.audit = Some(list),
        }
    }

    pub fn open_secret(&mut self, path: String) {
        self.detail = Some(Detail { path, ..Default::default() });
    }

    fn login_screen(&mut self, ctx: &egui::Context) {
        let sc = theme::scheme(ctx);
        egui::CentralPanel::default().frame(sc.panel(sc.surface)).show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() * 0.12).max(16.0));

                theme::icon_tile(ui, &sc, "🔐", 72.0);
                ui.add_space(12.0);
                ui.label(RichText::new("1Claw Vault Explorer").size(30.0).color(sc.on_surface));
                ui.label(RichText::new("Browse your vaults, secrets and audit trail").color(sc.on_surface_variant));
                ui.add_space(24.0);

                sc.card(1).corner_radius(CornerRadius::same(28)).inner_margin(Margin::same(24)).show(ui, |ui| {
                    ui.set_width(400.0);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        segmented(ui, &sc, &mut self.cfg.kind, &[(KeyKind::User, "Personal key"), (KeyKind::Agent, "Agent key")]);
                        ui.add_space(14.0);

                        field_label(ui, &sc, "API key");
                        let hint = if self.cfg.kind == KeyKind::User { "1ck_..." } else { "ocv_..." };
                        let key = ui.add(
                            egui::TextEdit::singleline(&mut self.api_key)
                                .password(true)
                                .hint_text(hint)
                                .margin(Margin::symmetric(14, 10))
                                .desired_width(f32::INFINITY),
                        );

                        if self.cfg.kind == KeyKind::Agent {
                            ui.add_space(6.0);
                            field_label(ui, &sc, "Agent ID (optional)");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.cfg.agent_id)
                                    .hint_text("only needed for older keys")
                                    .margin(Margin::symmetric(14, 10))
                                    .desired_width(f32::INFINITY),
                            );
                        }

                        ui.add_space(6.0);
                        ui.checkbox(&mut self.cfg.remember, "Remember me").on_hover_text(
                            "Keeps the key in your config folder so you're signed in next time.\n\
                             It's stored as plain text (readable only by your user).",
                        );

                        egui::CollapsingHeader::new(RichText::new("Advanced").color(sc.on_surface_variant)).show(ui, |ui| {
                            field_label(ui, &sc, "API base URL");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.cfg.base_url)
                                    .margin(Margin::symmetric(14, 10))
                                    .desired_width(f32::INFINITY),
                            );
                        });

                        ui.add_space(12.0);
                        let can_go = !self.api_key.trim().is_empty() && self.busy == 0;
                        let enter = key.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let label = if self.busy > 0 { "Signing in..." } else { "Connect" };
                        let btn = ui.add_enabled(can_go, sc.filled(label).min_size(egui::vec2(ui.available_width(), 44.0)));
                        if (btn.clicked() || enter) && can_go {
                            self.connect(true);
                        }
                        if let Some(e) = &self.login_error {
                            ui.add_space(8.0);
                            sc.card(0)
                                .fill(sc.error_container)
                                .corner_radius(CornerRadius::same(12))
                                .inner_margin(Margin::symmetric(12, 8))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(RichText::new(format!("⚠ {e}")).color(sc.on_error_container));
                                });
                        }
                    });
                });

                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    // poor man's centering for a horizontal row
                    ui.add_space((ui.available_width() - 300.0).max(0.0) / 2.0);
                    ui.hyperlink_to(RichText::new("Get a key at 1claw.co").small(), "https://1claw.co");
                    ui.label(RichText::new("·").small().color(sc.outline));
                    self.palette_inline(ui, ctx);
                });
            });
        });
        self.snackbar(ctx);
    }

    /// Theme toggle + seed swatches, shared by the login screen and the menu.
    fn palette_inline(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let sc = theme::scheme(ctx);
        let icon = if self.cfg.dark { "☀" } else { "🌙" };
        if ui.add(sc.text_button(icon, sc.on_surface_variant)).on_hover_text("Light / dark").clicked() {
            self.cfg.dark = !self.cfg.dark;
            self.retheme(ctx);
        }
        ui.menu_button(RichText::new("🎨").color(sc.on_surface_variant), |ui| self.palette_menu(ui, ctx));
    }

    fn palette_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let sc = theme::scheme(ctx);
        ui.label(RichText::new("Color").small().color(sc.on_surface_variant));
        ui.horizontal(|ui| {
            for (seed, name) in theme::SEEDS {
                let c = Color32::from_rgb(seed[0], seed[1], seed[2]);
                if theme::swatch(ui, c, *seed == self.cfg.seed, sc.on_surface).on_hover_text(*name).clicked() {
                    self.cfg.seed = *seed;
                    self.retheme(ctx);
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.selectable_label(!self.cfg.dark, "☀ Light").clicked() {
                self.cfg.dark = false;
                self.retheme(ctx);
            }
            if ui.selectable_label(self.cfg.dark, "🌙 Dark").clicked() {
                self.cfg.dark = true;
                self.retheme(ctx);
            }
        });
    }

    fn retheme(&mut self, ctx: &egui::Context) {
        theme::apply(ctx, self.cfg.seed, self.cfg.dark);
        self.cfg.save();
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        let sc = theme::scheme(ctx);
        egui::TopBottomPanel::top("top")
            .show_separator_line(false)
            .exact_height(60.0)
            .frame(sc.panel(sc.surface_container_low).inner_margin(Margin::symmetric(16, 0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    theme::icon_tile(ui, &sc, "🔐", 36.0);
                    ui.label(RichText::new("Vault Explorer").size(19.0).color(sc.on_surface));
                    ui.add_space(18.0);

                    let before = self.tab;
                    segmented(
                        ui,
                        &sc,
                        &mut self.tab,
                        &[(Tab::Secrets, "🗝  Secrets"), (Tab::Policies, "🛡  Policies"), (Tab::Activity, "📜  Activity")],
                    );
                    if before != self.tab {
                        self.load_tab();
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(sc.tonal("Sign out")).clicked() {
                            self.logout();
                        }
                        ui.menu_button(RichText::new("🎨").size(16.0).color(sc.on_surface_variant), |ui| {
                            self.palette_menu(ui, ctx)
                        })
                        .response
                        .on_hover_text("Theme");
                        if ui.add(sc.text_button("⟳", sc.on_surface_variant)).on_hover_text("Refresh (F5)").clicked() {
                            self.refresh();
                        }
                        if self.busy > 0 {
                            ui.add(egui::Spinner::new().color(sc.primary));
                        }
                    });
                });
            });
    }

    /// M3 snackbar - floats at the bottom center, inverse colors.
    fn snackbar(&mut self, ctx: &egui::Context) {
        let Some(t) = &self.toast else { return };
        let ttl = Duration::from_secs(if t.error { 8 } else { 4 });
        if t.at.elapsed() >= ttl {
            self.toast = None;
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        let sc = theme::scheme(ctx);
        let (bg, fg) = if t.error { (sc.error_container, sc.on_error_container) } else { (sc.on_surface, sc.surface) };
        let mut dismiss = false;
        egui::Area::new(egui::Id::new("snackbar"))
            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -20.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(bg)
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::symmetric(16, 12))
                    .shadow(ctx.style().visuals.popup_shadow)
                    .show(ui, |ui| {
                        ui.set_max_width(520.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&t.text).color(fg));
                            ui.add_space(12.0);
                            let accent = if t.error { sc.error } else { sc.primary_container };
                            if ui.add(sc.text_button("Dismiss", accent)).clicked() {
                                dismiss = true;
                            }
                        });
                    });
            });
        if dismiss {
            self.toast = None;
        }
    }

    fn vault_panel(&mut self, ctx: &egui::Context) {
        let sc = theme::scheme(ctx);
        egui::SidePanel::left("vaults")
            .resizable(true)
            .show_separator_line(false)
            .default_width(240.0)
            .frame(sc.panel(sc.surface_container_low).inner_margin(Margin::symmetric(12, 12)))
            .show(ctx, |ui| {
                if ui.add(sc.filled("+   New vault").min_size(egui::vec2(0.0, 48.0)).corner_radius(CornerRadius::same(16))).clicked() {
                    self.new_vault = Some(Default::default());
                }
                ui.add_space(10.0);
                search_bar(ui, &mut self.vault_filter, "Search vaults");
                ui.add_space(8.0);
                ui.label(RichText::new(format!("VAULTS · {}", self.vaults.len())).small().strong().color(sc.on_surface_variant));

                let q = self.vault_filter.to_lowercase();
                let mut clicked = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.vaults.is_empty() && self.busy == 0 {
                        ui.label(RichText::new("No vaults yet.").color(sc.on_surface_variant));
                    }
                    for v in self.vaults.iter().filter(|v| q.is_empty() || v.name.to_lowercase().contains(&q)) {
                        let sel = self.selected_vault.as_deref() == Some(&v.id);
                        let mut label = format!("🗄   {}", v.name);
                        if v.cmek_enabled == Some(true) {
                            label.push_str("  🔑");
                        }
                        let r = theme::nav_item(ui, &sc, &label, sel, 44.0);
                        let r = match v.description.as_deref().filter(|d| !d.is_empty()) {
                            Some(d) => r.on_hover_text(d),
                            None => r,
                        };
                        if r.clicked() {
                            clicked = Some(v.id.clone());
                        }
                        r.context_menu(|ui| {
                            if ui.button("🗐  Copy vault ID").clicked() {
                                ctx.copy_text(v.id.clone());
                                ui.close_menu();
                            }
                            if ui.button(RichText::new("🗑  Delete vault...").color(sc.error)).clicked() {
                                self.confirm_vault_delete = Some(v.id.clone());
                                ui.close_menu();
                            }
                        });
                    }
                });
                if let Some(id) = clicked {
                    self.select_vault(id);
                }
            });
    }

    fn vault_dialogs(&mut self, ctx: &egui::Context) {
        let sc = theme::scheme(ctx);
        if let Some((name, desc)) = self.new_vault.as_mut() {
            let mut cancel = false;
            let mut submit = false;
            dialog(ctx, "New vault", |ui| {
                ui.label(RichText::new("Vaults group secrets and have their own access policies.").color(sc.on_surface_variant));
                ui.add_space(8.0);
                field_label(ui, &sc, "Name");
                ui.add(egui::TextEdit::singleline(name).margin(Margin::symmetric(14, 10)).desired_width(f32::INFINITY));
                field_label(ui, &sc, "Description");
                ui.add(egui::TextEdit::singleline(desc).margin(Margin::symmetric(14, 10)).desired_width(f32::INFINITY));
                ui.add_space(12.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ok = !name.trim().is_empty();
                    submit = ui.add_enabled(ok, sc.filled("Create")).clicked()
                        || (ok && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    cancel = ui.add(sc.text_button("Cancel", sc.primary)).clicked();
                });
            });
            if submit {
                let (n, d) = self.new_vault.take().unwrap();
                self.job(Job::CreateVault { name: n.trim().to_string(), description: d.trim().to_string() });
            } else if cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.new_vault = None;
            }
        }

        if let Some(id) = self.confirm_vault_delete.clone() {
            let name = self.vaults.iter().find(|v| v.id == id).map(|v| v.name.clone()).unwrap_or_default();
            let mut keep = !ctx.input(|i| i.key_pressed(egui::Key::Escape));
            dialog(ctx, "Delete vault?", |ui| {
                ui.label(RichText::new(format!("\"{name}\" and every secret in it will be gone for good.")).color(sc.on_surface_variant));
                ui.add_space(14.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(sc.danger("Delete")).clicked() {
                        self.job(Job::DeleteVault(id.clone()));
                        keep = false;
                    }
                    if ui.add(sc.text_button("Cancel", sc.primary)).clicked() {
                        keep = false;
                    }
                });
            });
            if !keep {
                self.confirm_vault_delete = None;
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(m) = self.worker.rx.try_recv() {
            self.handle(ctx, m);
        }

        // don't leave secrets sitting in the clipboard forever
        if let Some(t) = self.clipboard_set_at {
            if t.elapsed() >= CLIPBOARD_CLEAR_AFTER {
                ctx.copy_text(String::new());
                self.clipboard_set_at = None;
            } else {
                ctx.request_repaint_after(CLIPBOARD_CLEAR_AFTER - t.elapsed());
            }
        }

        // some compositors (KWin) ignore the initial size hint and open us tiny,
        // so ask again once we're actually on screen
        if !self.sized {
            self.sized = true;
            let r = ctx.screen_rect();
            if r.width() < 760.0 || r.height() < 480.0 {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1180.0, 740.0)));
            }
        }

        if self.screen == Screen::Login {
            self.login_screen(ctx);
            return;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
            self.refresh();
        }

        self.top_bar(ctx);
        self.vault_panel(ctx);
        self.vault_dialogs(ctx);

        match self.tab {
            Tab::Secrets => crate::views::secrets(self, ctx),
            Tab::Policies => crate::views::policies(self, ctx),
            Tab::Activity => crate::views::activity(self, ctx),
        }
        self.snackbar(ctx);
    }
}

// --- small shared widgets ---

/// M3 segmented button. Works for any Copy + PartialEq value.
pub fn segmented<T: PartialEq + Copy>(ui: &mut egui::Ui, sc: &theme::Scheme, value: &mut T, options: &[(T, &str)]) {
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0f32, sc.outline_variant))
        .corner_radius(CornerRadius::same(20))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| {
                for (v, label) in options {
                    let sel = *value == *v;
                    let (fill, fg) = if sel { (sc.secondary_container, sc.on_secondary_container) } else { (Color32::TRANSPARENT, sc.on_surface) };
                    let text = if sel { format!("✔ {label}") } else { label.to_string() };
                    let b = egui::Button::new(RichText::new(text).color(fg))
                        .fill(fill)
                        .corner_radius(CornerRadius::same(17))
                        .min_size(egui::vec2(0.0, 32.0));
                    if ui.add(b).clicked() {
                        *value = *v;
                    }
                }
            });
        });
}

pub fn search_bar(ui: &mut egui::Ui, text: &mut String, hint: &str) -> egui::Response {
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(format!("🔍  {hint}"))
            .margin(Margin::symmetric(16, 10))
            .desired_width(f32::INFINITY),
    )
}

pub fn field_label(ui: &mut egui::Ui, sc: &theme::Scheme, text: &str) {
    ui.label(RichText::new(text).small().strong().color(sc.on_surface_variant));
}

/// Centered modal-ish window with M3 dialog styling + a scrim behind it.
pub fn dialog(ctx: &egui::Context, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    let sc = theme::scheme(ctx);
    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new(("scrim", title)))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .interactable(true)
        .show(ctx, |ui| {
            let (_, r) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
            ui.painter().rect_filled(r.rect, 0.0, Color32::from_black_alpha(if sc.dark { 120 } else { 70 }));
        });
    egui::Window::new(RichText::new(title).size(22.0).color(sc.on_surface))
        .id(egui::Id::new(("dlg", title)))
        .collapsible(false)
        .resizable(false)
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(egui::Frame::window(&ctx.style()).fill(sc.surface_container_high).inner_margin(Margin::same(24)))
        .show(ctx, |ui| {
            ui.set_min_width(360.0);
            body(ui)
        });
}
