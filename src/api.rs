// Thin blocking client for the 1Claw REST API.
// Only covers what the explorer actually needs - vaults, secrets, versions,
// policies and the audit log.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, Instant};

pub const DEFAULT_BASE_URL: &str = "https://api.1claw.co";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum KeyKind {
    /// personal user key (1ck_...)
    User,
    /// agent key (ocv_...)
    Agent,
}

#[derive(Clone, Debug)]
pub struct Credentials {
    pub base_url: String,
    pub kind: KeyKind,
    pub api_key: String,
    pub agent_id: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
struct TokenResponse {
    access_token: String,
    expires_in: Option<u64>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Vault {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub created_by: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub cmek_enabled: Option<bool>,
    #[serde(default)]
    pub mpc_custody: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SecretMeta {
    pub id: String,
    pub path: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub version: i64,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub is_disabled: Option<bool>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Secret {
    pub path: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    pub value: String,
    #[serde(default)]
    pub version: i64,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub created_by: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Policy {
    pub id: String,
    pub secret_path_pattern: String,
    pub principal_type: String,
    pub principal_id: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub effect: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct AuditEvent {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub actor_id: String,
    #[serde(default)]
    pub actor_type: String,
    #[serde(default)]
    pub resource_type: String,
    #[serde(default)]
    pub resource_id: String,
    #[serde(default)]
    pub details: Option<Value>,
    #[serde(default)]
    pub ip_address: Option<String>,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Serialize)]
pub struct PutSecret<'a> {
    #[serde(rename = "type")]
    pub kind: &'a str,
    pub value: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<&'a str>,
}

pub struct Client {
    http: reqwest::blocking::Client,
    creds: Credentials,
    token: Option<(String, Instant)>,
}

impl Client {
    pub fn new(creds: Credentials) -> Self {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("vault-explorer/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("http client");
        Self { http, creds, token: None }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.creds.base_url.trim_end_matches('/'), path)
    }

    /// Swap the long lived key for a short lived JWT.
    pub fn login(&mut self) -> Result<(), String> {
        let (endpoint, body) = match self.creds.kind {
            KeyKind::User => (
                "/v1/auth/api-key-token",
                serde_json::json!({ "api_key": self.creds.api_key }),
            ),
            KeyKind::Agent => {
                let mut b = serde_json::json!({ "api_key": self.creds.api_key });
                if let Some(id) = self.creds.agent_id.as_deref().filter(|s| !s.is_empty()) {
                    b["agent_id"] = Value::String(id.to_string());
                }
                ("/v1/auth/agent-token", b)
            }
        };
        let resp = self
            .http
            .post(self.url(endpoint))
            .json(&body)
            .send()
            .map_err(|e| format!("network error: {e}"))?;
        let tok: TokenResponse = read_json(resp)?;
        // refresh a bit early so we don't race the expiry
        let ttl = tok.expires_in.unwrap_or(900).saturating_sub(30).max(30);
        self.token = Some((tok.access_token, Instant::now() + Duration::from_secs(ttl)));
        Ok(())
    }

    fn bearer(&mut self) -> Result<String, String> {
        match &self.token {
            Some((t, exp)) if Instant::now() < *exp => Ok(t.clone()),
            _ => {
                self.login()?;
                Ok(self.token.as_ref().unwrap().0.clone())
            }
        }
    }

    fn send(
        &mut self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<reqwest::blocking::Response, String> {
        // one retry on 401 in case the token got revoked / expired early
        for attempt in 0..2 {
            let token = self.bearer()?;
            let mut req = self.http.request(method.clone(), self.url(path)).bearer_auth(token);
            if let Some(b) = body {
                req = req.json(b);
            }
            let resp = req.send().map_err(|e| format!("network error: {e}"))?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                self.token = None;
                continue;
            }
            return Ok(resp);
        }
        unreachable!()
    }

    fn get<T: for<'de> Deserialize<'de>>(&mut self, path: &str) -> Result<T, String> {
        let r = self.send(reqwest::Method::GET, path, None)?;
        read_json(r)
    }

    fn delete(&mut self, path: &str) -> Result<(), String> {
        let r = self.send(reqwest::Method::DELETE, path, None)?;
        check(r).map(|_| ())
    }

    // --- vaults ---

    pub fn vaults(&mut self) -> Result<Vec<Vault>, String> {
        #[derive(Deserialize)]
        struct R {
            #[serde(default)]
            vaults: Vec<Vault>,
        }
        Ok(self.get::<R>("/v1/vaults")?.vaults)
    }

    pub fn create_vault(&mut self, name: &str, description: &str) -> Result<Vault, String> {
        let body = serde_json::json!({ "name": name, "description": description });
        let r = self.send(reqwest::Method::POST, "/v1/vaults", Some(&body))?;
        read_json(r)
    }

    pub fn delete_vault(&mut self, vault_id: &str) -> Result<(), String> {
        self.delete(&format!("/v1/vaults/{}", enc(vault_id)))
    }

    // --- secrets ---

    pub fn secrets(&mut self, vault_id: &str) -> Result<Vec<SecretMeta>, String> {
        #[derive(Deserialize)]
        struct R {
            #[serde(default)]
            secrets: Vec<SecretMeta>,
        }
        Ok(self.get::<R>(&format!("/v1/vaults/{}/secrets", enc(vault_id)))?.secrets)
    }

    pub fn secret(&mut self, vault_id: &str, path: &str) -> Result<Secret, String> {
        self.get(&format!("/v1/vaults/{}/secrets/{}", enc(vault_id), enc_path(path)))
    }

    pub fn secret_version(&mut self, vault_id: &str, path: &str, version: i64) -> Result<Secret, String> {
        self.get(&format!(
            "/v1/vaults/{}/secret-version/{}/{}",
            enc(vault_id),
            enc_path(path),
            version
        ))
    }

    pub fn versions(&mut self, vault_id: &str, path: &str) -> Result<Vec<SecretMeta>, String> {
        #[derive(Deserialize)]
        struct R {
            #[serde(default)]
            versions: Vec<SecretMeta>,
        }
        let mut v = self
            .get::<R>(&format!("/v1/vaults/{}/secret-versions/{}", enc(vault_id), enc_path(path)))?
            .versions;
        v.sort_by(|a, b| b.version.cmp(&a.version));
        Ok(v)
    }

    pub fn put_secret(&mut self, vault_id: &str, path: &str, body: &PutSecret) -> Result<SecretMeta, String> {
        let body = serde_json::to_value(body).map_err(|e| e.to_string())?;
        let r = self.send(
            reqwest::Method::PUT,
            &format!("/v1/vaults/{}/secrets/{}", enc(vault_id), enc_path(path)),
            Some(&body),
        )?;
        read_json(r)
    }

    pub fn delete_secret(&mut self, vault_id: &str, path: &str) -> Result<(), String> {
        self.delete(&format!("/v1/vaults/{}/secrets/{}", enc(vault_id), enc_path(path)))
    }

    // --- policies / audit ---

    pub fn policies(&mut self, vault_id: &str) -> Result<Vec<Policy>, String> {
        #[derive(Deserialize)]
        struct R {
            #[serde(default)]
            policies: Vec<Policy>,
        }
        Ok(self.get::<R>(&format!("/v1/vaults/{}/policies", enc(vault_id)))?.policies)
    }

    pub fn audit(&mut self, resource_id: Option<&str>, limit: u32) -> Result<Vec<AuditEvent>, String> {
        #[derive(Deserialize)]
        struct R {
            #[serde(default)]
            events: Vec<AuditEvent>,
        }
        let mut q = format!("/v1/audit/events?limit={limit}");
        if let Some(id) = resource_id {
            q.push_str(&format!("&resource_id={}", enc(id)));
        }
        Ok(self.get::<R>(&q)?.events)
    }
}

fn enc(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

// secret paths look like "db/prod/password" - the API wants the slashes kept,
// so encode each segment on its own
fn enc_path(p: &str) -> String {
    p.split('/').map(enc).collect::<Vec<_>>().join("/")
}

fn check(resp: reqwest::blocking::Response) -> Result<reqwest::blocking::Response, String> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let text = resp.text().unwrap_or_default();
    // errors come back as RFC 7807 problem json, pull out something readable
    let msg = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| {
            v.get("detail")
                .or_else(|| v.get("title"))
                .or_else(|| v.get("error"))
                .or_else(|| v.get("message"))
                .and_then(|m| m.as_str().map(str::to_string))
        })
        .unwrap_or_else(|| text.chars().take(200).collect());
    Err(format!("{} {}", status.as_u16(), msg).trim().to_string())
}

fn read_json<T: for<'de> Deserialize<'de>>(resp: reqwest::blocking::Response) -> Result<T, String> {
    let resp = check(resp)?;
    resp.json::<T>().map_err(|e| format!("bad response: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_encoding_keeps_slashes() {
        assert_eq!(enc_path("db/prod pass"), "db/prod%20pass");
        assert_eq!(enc_path("a?b/c#d"), "a%3Fb/c%23d");
    }

    #[test]
    fn parses_secret_list() {
        let raw = r#"{"secrets":[{"id":"1","path":"db/pw","type":"password","version":3,"created_at":"2026-01-01T00:00:00Z"}]}"#;
        #[derive(Deserialize)]
        struct R {
            secrets: Vec<SecretMeta>,
        }
        let r: R = serde_json::from_str(raw).unwrap();
        assert_eq!(r.secrets[0].kind, "password");
        assert_eq!(r.secrets[0].version, 3);
    }
}
