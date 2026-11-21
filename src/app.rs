use crate::api::{AuditEvent, Credentials, KeyKind, Policy, Secret, SecretMeta, Vault};
use crate::config::Config;
use crate::worker::{Job, Msg, Worker};
use eframe::egui::{self, Color32, RichText};
use std::time::{Duration, Instant};

pub const ACCENT: Color32 = Color32::from_rgb(0xE8, 0x5D, 0x3A);
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
}

impl App {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let cfg = Config::load();
        apply_theme(&cc.egui_ctx, cfg.dark);
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
        };
        // handy for scripting / demos: ONECLAW_API_KEY=... vault-explorer
        if let Ok(k) = std::env::var("ONECLAW_API_KEY") {
            if !k.trim().is_empty() {
                app.api_key = k.trim().to_string();
                if app.api_key.starts_with("ocv_") {
                    app.cfg.kind = KeyKind::Agent;
                }
                app.connect();
            }
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

    fn connect(&mut self) {
        self.login_error = None;
        let creds = Credentials {
            base_url: self.cfg.base_url.trim().to_string(),
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
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() * 0.18).max(20.0));
                ui.label(RichText::new("🔐").size(44.0));
                ui.heading(RichText::new("1Claw Vault Explorer").strong().size(26.0));
                ui.label(RichText::new("Browse your vaults, secrets and audit trail").weak());
                ui.add_space(24.0);

                egui::Frame::group(ui.style()).inner_margin(18.0).corner_radius(10.0).show(ui, |ui| {
                    ui.set_width(380.0);
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.cfg.kind, KeyKind::User, "Personal key (1ck_)");
                        ui.selectable_value(&mut self.cfg.kind, KeyKind::Agent, "Agent key (ocv_)");
                    });
                    ui.add_space(8.0);

                    ui.label("API key");
                    let key = ui.add(
                        egui::TextEdit::singleline(&mut self.api_key)
                            .password(true)
                            .hint_text("paste your key")
                            .desired_width(f32::INFINITY),
                    );

                    if self.cfg.kind == KeyKind::Agent {
                        ui.add_space(4.0);
                        ui.label("Agent ID (optional)");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.cfg.agent_id)
                                .hint_text("only needed for older keys")
                                .desired_width(f32::INFINITY),
                        );
                    }

                    ui.add_space(4.0);
                    egui::CollapsingHeader::new("Advanced").show(ui, |ui| {
                        ui.label("API base URL");
                        ui.add(egui::TextEdit::singleline(&mut self.cfg.base_url).desired_width(f32::INFINITY));
                    });

                    ui.add_space(10.0);
                    let can_go = !self.api_key.trim().is_empty() && self.busy == 0;
                    let enter = key.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let btn = ui.add_enabled(
                        can_go,
                        egui::Button::new(RichText::new("Connect").strong().color(Color32::WHITE))
                            .fill(ACCENT)
                            .min_size(egui::vec2(ui.available_width(), 34.0)),
                    );
                    if (btn.clicked() || enter) && can_go {
                        self.connect();
                    }
                    if self.busy > 0 {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Signing in...");
                        });
                    }
                    if let Some(e) = &self.login_error {
                        ui.colored_label(Color32::from_rgb(0xE0, 0x4F, 0x4F), e);
                    }
                });

                ui.add_space(10.0);
                ui.hyperlink_to(RichText::new("Get a key at 1claw.co").small(), "https://1claw.co");
            });
        });
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("🔐 1Claw Vault Explorer").strong());
                ui.separator();
                let before = self.tab;
                ui.selectable_value(&mut self.tab, Tab::Secrets, "🗝 Secrets");
                ui.selectable_value(&mut self.tab, Tab::Policies, "🛡 Policies");
                ui.selectable_value(&mut self.tab, Tab::Activity, "📜 Activity");
                if before != self.tab {
                    self.load_tab();
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Sign out").clicked() {
                        self.logout();
                    }
                    let icon = if self.cfg.dark { "☀" } else { "🌙" };
                    if ui.button(icon).on_hover_text("Toggle theme").clicked() {
                        self.cfg.dark = !self.cfg.dark;
                        apply_theme(ctx, self.cfg.dark);
                        self.cfg.save();
                    }
                    if ui.button("⟳").on_hover_text("Refresh (F5)").clicked() {
                        self.refresh();
                    }
                    if self.busy > 0 {
                        ui.spinner();
                    }
                });
            });
            ui.add_space(2.0);
        });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                match &self.toast {
                    Some(t) if t.at.elapsed() < Duration::from_secs(if t.error { 8 } else { 4 }) => {
                        let c = if t.error { Color32::from_rgb(0xE0, 0x4F, 0x4F) } else { ui.visuals().text_color() };
                        ui.colored_label(c, &t.text);
                        ctx.request_repaint_after(Duration::from_millis(500));
                    }
                    _ => {
                        ui.label(RichText::new(format!("{} vaults", self.vaults.len())).weak());
                        if let Some(s) = &self.secrets {
                            ui.label(RichText::new(format!("· {} secrets in this vault", s.len())).weak());
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(self.cfg.base_url.trim_start_matches("https://")).weak().small());
                });
            });
        });
    }

    fn vault_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("vaults").resizable(true).default_width(220.0).show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("Vaults");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("＋").on_hover_text("New vault").clicked() {
                        self.new_vault = Some(Default::default());
                    }
                });
            });
            ui.add(egui::TextEdit::singleline(&mut self.vault_filter).hint_text("🔍 filter").desired_width(f32::INFINITY));
            ui.add_space(4.0);

            let q = self.vault_filter.to_lowercase();
            let mut clicked = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                if self.vaults.is_empty() && self.busy == 0 {
                    ui.label(RichText::new("No vaults yet. Hit ＋ to make one.").weak());
                }
                for v in self.vaults.iter().filter(|v| q.is_empty() || v.name.to_lowercase().contains(&q)) {
                    let sel = self.selected_vault.as_deref() == Some(&v.id);
                    let mut label = format!("🗄 {}", v.name);
                    if v.cmek_enabled == Some(true) {
                        label.push_str("  🔑");
                    }
                    let r = ui.add_sized(
                        [ui.available_width(), 26.0],
                        egui::SelectableLabel::new(sel, label),
                    );
                    let r = match v.description.as_deref().filter(|d| !d.is_empty()) {
                        Some(d) => r.on_hover_text(d),
                        None => r,
                    };
                    if r.clicked() {
                        clicked = Some(v.id.clone());
                    }
                    r.context_menu(|ui| {
                        if ui.button("Copy vault ID").clicked() {
                            ctx.copy_text(v.id.clone());
                            ui.close_menu();
                        }
                        if ui.button("🗑 Delete vault...").clicked() {
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
        if let Some((name, desc)) = self.new_vault.as_mut() {
            let mut open = true;
            let mut submit = false;
            egui::Window::new("New vault")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(name);
                    ui.label("Description");
                    ui.text_edit_singleline(desc);
                    ui.add_space(6.0);
                    submit = ui.add_enabled(!name.trim().is_empty(), egui::Button::new("Create")).clicked()
                        || (ui.input(|i| i.key_pressed(egui::Key::Enter)) && !name.trim().is_empty());
                });
            if submit {
                let (n, d) = self.new_vault.take().unwrap();
                self.job(Job::CreateVault { name: n.trim().to_string(), description: d.trim().to_string() });
            } else if !open {
                self.new_vault = None;
            }
        }

        if let Some(id) = self.confirm_vault_delete.clone() {
            let name = self.vaults.iter().find(|v| v.id == id).map(|v| v.name.clone()).unwrap_or_default();
            let mut keep = true;
            egui::Window::new("Delete vault?")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!("\"{name}\" and every secret in it will be gone for good."));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                        let del = egui::Button::new(RichText::new("Delete").color(Color32::WHITE))
                            .fill(Color32::from_rgb(0xC0, 0x39, 0x2B));
                        if ui.add(del).clicked() {
                            self.job(Job::DeleteVault(id.clone()));
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

        if self.screen == Screen::Login {
            self.login_screen(ctx);
            return;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
            self.refresh();
        }

        self.top_bar(ctx);
        self.status_bar(ctx);
        self.vault_panel(ctx);
        self.vault_dialogs(ctx);

        match self.tab {
            Tab::Secrets => crate::views::secrets(self, ctx),
            Tab::Policies => crate::views::policies(self, ctx),
            Tab::Activity => crate::views::activity(self, ctx),
        }
    }
}

fn apply_theme(ctx: &egui::Context, dark: bool) {
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.selection.bg_fill = ACCENT.gamma_multiply(if dark { 0.55 } else { 0.35 });
    v.hyperlink_color = ACCENT;
    v.window_corner_radius = 10.0.into();
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(8.0, 4.0);
    });
}
