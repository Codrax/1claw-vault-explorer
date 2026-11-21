// All network calls happen on one background thread so the UI never blocks.
// The UI pushes Jobs in, results come back as Msgs.

use crate::api::{self, AuditEvent, Client, Credentials, Policy, Secret, SecretMeta, Vault};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

pub enum Job {
    Login(Credentials),
    Vaults,
    CreateVault { name: String, description: String },
    DeleteVault(String),
    Secrets(String),
    Secret { vault: String, path: String, version: Option<i64> },
    Versions { vault: String, path: String },
    PutSecret { vault: String, path: String, kind: String, value: String, expires_at: Option<String> },
    DeleteSecret { vault: String, path: String },
    Policies(String),
    Audit { resource_id: Option<String> },
}

pub enum Msg {
    LoggedIn,
    LoginFailed(String),
    Vaults(Vec<Vault>),
    VaultCreated(Vault),
    VaultDeleted(String),
    Secrets { vault: String, list: Vec<SecretMeta> },
    Secret { vault: String, secret: Secret },
    Versions { vault: String, path: String, list: Vec<SecretMeta> },
    SecretSaved { vault: String, meta: SecretMeta },
    SecretDeleted { vault: String, path: String },
    Policies { vault: String, list: Vec<Policy> },
    Audit(Vec<AuditEvent>),
    Error(String),
    Done,
}

pub struct Worker {
    tx: Sender<Job>,
    pub rx: Receiver<Msg>,
}

impl Worker {
    pub fn spawn(ctx: eframe::egui::Context) -> Self {
        let (job_tx, job_rx) = channel::<Job>();
        let (msg_tx, msg_rx) = channel::<Msg>();

        thread::spawn(move || {
            let mut client: Option<Client> = None;
            for job in job_rx {
                let out = run(&mut client, job);
                for m in out {
                    let _ = msg_tx.send(m);
                }
                let _ = msg_tx.send(Msg::Done);
                ctx.request_repaint();
            }
        });

        Self { tx: job_tx, rx: msg_rx }
    }

    pub fn send(&self, job: Job) {
        let _ = self.tx.send(job);
    }
}

fn run(client: &mut Option<Client>, job: Job) -> Vec<Msg> {
    if let Job::Login(creds) = job {
        let mut c = Client::new(creds);
        return match c.login() {
            Ok(()) => {
                // grab the vault list right away, saves a round trip from the ui
                let v = c.vaults();
                *client = Some(c);
                match v {
                    Ok(list) => vec![Msg::LoggedIn, Msg::Vaults(list)],
                    Err(e) => vec![Msg::LoggedIn, Msg::Error(e)],
                }
            }
            Err(e) => vec![Msg::LoginFailed(e)],
        };
    }

    let Some(c) = client.as_mut() else {
        return vec![Msg::Error("not signed in".into())];
    };

    let res: Result<Msg, String> = match job {
        Job::Login(_) => unreachable!(),
        Job::Vaults => c.vaults().map(Msg::Vaults),
        Job::CreateVault { name, description } => c.create_vault(&name, &description).map(Msg::VaultCreated),
        Job::DeleteVault(id) => c.delete_vault(&id).map(|_| Msg::VaultDeleted(id)),
        Job::Secrets(vault) => c.secrets(&vault).map(|list| Msg::Secrets { vault, list }),
        Job::Secret { vault, path, version } => {
            let r = match version {
                Some(v) => c.secret_version(&vault, &path, v),
                None => c.secret(&vault, &path),
            };
            r.map(|secret| Msg::Secret { vault, secret })
        }
        Job::Versions { vault, path } => c.versions(&vault, &path).map(|list| Msg::Versions { vault, path, list }),
        Job::PutSecret { vault, path, kind, value, expires_at } => {
            let body = api::PutSecret { kind: &kind, value: &value, expires_at: expires_at.as_deref() };
            c.put_secret(&vault, &path, &body).map(|meta| Msg::SecretSaved { vault, meta })
        }
        Job::DeleteSecret { vault, path } => c.delete_secret(&vault, &path).map(|_| Msg::SecretDeleted { vault, path }),
        Job::Policies(vault) => c.policies(&vault).map(|list| Msg::Policies { vault, list }),
        Job::Audit { resource_id } => c.audit(resource_id.as_deref(), 100).map(Msg::Audit),
    };

    vec![res.unwrap_or_else(Msg::Error)]
}
