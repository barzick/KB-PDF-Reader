//! AI assistant: models the user plugs in themselves (no bundled provider).
//!
//! Two API families cover nearly every service: the OpenAI-compatible
//! `/chat/completions` (OpenAI, Mistral, Gemini, OpenRouter, Groq, Ollama,
//! LM Studio…) and Anthropic's `/v1/messages` (Claude). Requests are made
//! here, not in the web view, so keys never reach page scripts and there is
//! no CORS limit. Answers stream back as `ai-delta` events.

use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{Emitter, Manager, State};

use crate::commands::Library;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ApiKind {
  Openai,
  Anthropic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProfile {
  pub id: String,
  pub name: String,
  pub kind: ApiKind,
  /// Base address: "https://api.openai.com/v1", "http://localhost:11434/v1", "https://api.anthropic.com"…
  pub endpoint: String,
  pub model: String,
  /// Most document text sent with a question, in characters.
  #[serde(default = "default_context")]
  pub context_chars: usize,
}

fn default_context() -> usize {
  60_000
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Settings {
  profiles: Vec<AiProfile>,
  active: Option<String>,
}

#[derive(Serialize)]
pub struct ProfileView {
  #[serde(flatten)]
  profile: AiProfile,
  has_key: bool,
}

#[derive(Serialize)]
pub struct SettingsView {
  profiles: Vec<ProfileView>,
  active: Option<String>,
}

#[derive(Default)]
pub struct AiState {
  lock: Mutex<()>,
  cancelled: Mutex<HashSet<String>>,
}

fn dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
  let d = app.path().app_config_dir().map_err(|e| e.to_string())?;
  std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
  Ok(d)
}

fn read_json<T: Default + for<'a> Deserialize<'a>>(path: &PathBuf) -> T {
  std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Writes a file only the current user can read (keys).
fn write_private(path: &PathBuf, data: &[u8]) -> Result<(), String> {
  let tmp = path.with_extension("tmp");
  std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
  }
  std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn load(app: &tauri::AppHandle) -> Result<(Settings, std::collections::HashMap<String, String>), String> {
  let d = dir(app)?;
  Ok((read_json(&d.join("ai.json")), read_json(&d.join("ai-keys.json"))))
}

fn store(app: &tauri::AppHandle, s: &Settings, keys: &std::collections::HashMap<String, String>) -> Result<(), String> {
  let d = dir(app)?;
  write_private(&d.join("ai.json"), &serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?)?;
  write_private(&d.join("ai-keys.json"), &serde_json::to_vec(keys).map_err(|e| e.to_string())?)
}

fn view(s: Settings, keys: &std::collections::HashMap<String, String>) -> SettingsView {
  SettingsView {
    profiles: s
      .profiles
      .into_iter()
      .map(|p| ProfileView { has_key: keys.get(&p.id).is_some_and(|k| !k.is_empty()), profile: p })
      .collect(),
    active: s.active,
  }
}

#[tauri::command]
pub fn ai_settings(app: tauri::AppHandle) -> Result<SettingsView, String> {
  let (s, keys) = load(&app)?;
  Ok(view(s, &keys))
}

/// Adds or updates a profile. `api_key`: None keeps the stored key, "" removes it.
#[tauri::command]
pub fn ai_save_profile(app: tauri::AppHandle, st: State<'_, AiState>, profile: AiProfile, api_key: Option<String>) -> Result<SettingsView, String> {
  let _g = st.lock.lock();
  let (mut s, mut keys) = load(&app)?;
  if let Some(k) = api_key {
    if k.trim().is_empty() {
      keys.remove(&profile.id);
    } else {
      keys.insert(profile.id.clone(), k.trim().to_string());
    }
  }
  match s.profiles.iter_mut().find(|p| p.id == profile.id) {
    Some(p) => *p = profile.clone(),
    None => s.profiles.push(profile.clone()),
  }
  if s.active.is_none() {
    s.active = Some(profile.id);
  }
  store(&app, &s, &keys)?;
  Ok(view(s, &keys))
}

#[tauri::command]
pub fn ai_delete_profile(app: tauri::AppHandle, st: State<'_, AiState>, id: String) -> Result<SettingsView, String> {
  let _g = st.lock.lock();
  let (mut s, mut keys) = load(&app)?;
  s.profiles.retain(|p| p.id != id);
  keys.remove(&id);
  if s.active.as_deref() == Some(&id) {
    s.active = s.profiles.first().map(|p| p.id.clone());
  }
  store(&app, &s, &keys)?;
  Ok(view(s, &keys))
}

#[tauri::command]
pub fn ai_set_active(app: tauri::AppHandle, st: State<'_, AiState>, id: String) -> Result<SettingsView, String> {
  let _g = st.lock.lock();
  let (mut s, keys) = load(&app)?;
  if s.profiles.iter().any(|p| p.id == id) {
    s.active = Some(id);
  }
  store(&app, &s, &keys)?;
  Ok(view(s, &keys))
}

fn profile(app: &tauri::AppHandle, id: Option<&str>) -> Result<(AiProfile, String), String> {
  let (s, keys) = load(app)?;
  let id = id.map(str::to_string).or(s.active.clone()).ok_or("aucun modèle configuré : ouvrez les réglages de l'assistant")?;
  let p = s.profiles.into_iter().find(|p| p.id == id).ok_or("modèle introuvable dans les réglages")?;
  let key = keys.get(&p.id).cloned().unwrap_or_default();
  Ok((p, key))
}

fn agent() -> ureq::Agent {
  ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout_read(Duration::from_secs(300)).build()
}

fn base(p: &AiProfile) -> String {
  p.endpoint.trim().trim_end_matches('/').to_string()
}

/// A readable message for an HTTP failure, with the service's own explanation.
fn http_error(e: ureq::Error) -> String {
  match e {
    ureq::Error::Status(code, resp) => {
      let body = resp.into_string().unwrap_or_default();
      let msg = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().or(v["error"].as_str()).or(v["message"].as_str()).map(str::to_string))
        .unwrap_or_else(|| body.chars().take(300).collect());
      let hint = match code {
        401 | 403 => " (clé API refusée)",
        404 => " (adresse ou nom de modèle incorrect)",
        429 => " (trop de demandes ou crédit épuisé)",
        _ => "",
      };
      format!("erreur {code}{hint} : {msg}")
    }
    ureq::Error::Transport(t) => format!("service injoignable : {t}"),
  }
}

fn anthropic_headers(req: ureq::Request, key: &str) -> ureq::Request {
  req.set("x-api-key", key).set("anthropic-version", "2026-10-21")
}

/// The models the service offers.
#[tauri::command]
pub async fn ai_list_models(app: tauri::AppHandle, id: String) -> Result<Vec<String>, String> {
  let (p, key) = profile(&app, Some(&id))?;
  tauri::async_runtime::spawn_blocking(move || {
    let a = agent();
    let resp = match p.kind {
      ApiKind::Openai => {
        let mut r = a.get(&format!("{}/models", base(&p)));
        if !key.is_empty() {
          r = r.set("Authorization", &format!("Bearer {key}"));
        }
        r.call()
      }
      ApiKind::Anthropic => anthropic_headers(a.get(&format!("{}/v1/models?limit=100", base(&p))), &key).call(),
    }
    .map_err(http_error)?;
    let v: Value = resp.into_json().map_err(|e| e.to_string())?;
    let mut ids: Vec<String> = v["data"]
      .as_array()
      .or(v["models"].as_array())
      .map(|a| a.iter().filter_map(|m| m["id"].as_str().or(m["name"].as_str()).map(str::to_string)).collect())
      .unwrap_or_default();
    ids.sort();
    Ok(ids)
  })
  .await
  .map_err(|e| e.to_string())?
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
  pub role: String,
  pub content: String,
}

#[derive(Serialize, Clone)]
struct Delta {
  request_id: String,
  text: String,
}

/// Claude models that take the server-side fallback on a policy decline.
fn takes_fallbacks(model: &str) -> bool {
  ["claude-opus-5", "claude-fable-5", "claude-sonnet-5-5", "claude-mythos-5"].iter().any(|m| model.starts_with(m))
}

/// Sends the conversation and streams the answer (`ai-delta` events); returns the whole answer.
#[tauri::command]
pub async fn ai_chat(
  app: tauri::AppHandle,
  request_id: String,
  profile_id: Option<String>,
  system: String,
  messages: Vec<ChatMessage>,
) -> Result<String, String> {
  let (p, key) = profile(&app, profile_id.as_deref())?;
  if p.model.trim().is_empty() {
    return Err("aucun modèle choisi dans les réglages".into());
  }
  let emitter = app.clone();
  tauri::async_runtime::spawn_blocking(move || {
    let st = emitter.state::<AiState>();
    st.cancelled.lock().remove(&request_id);
    let a = agent();
    let resp = match p.kind {
      ApiKind::Openai => {
        let mut msgs = vec![json!({"role": "system", "content": system})];
        msgs.extend(messages.iter().map(|m| json!({"role": m.role, "content": m.content})));
        let mut r = a.post(&format!("{}/chat/completions", base(&p))).set("Content-Type", "application/json");
        if !key.is_empty() {
          r = r.set("Authorization", &format!("Bearer {key}"));
        }
        r.send_json(json!({"model": p.model, "messages": msgs, "stream": true}))
      }
      ApiKind::Anthropic => {
        let mut body = json!({
          "model": p.model,
          "max_tokens": 64000,
          "stream": true,
          "system": system,
          "messages": messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
        });
        let mut r = anthropic_headers(a.post(&format!("{}/v1/messages", base(&p))), &key).set("Content-Type", "application/json");
        if takes_fallbacks(&p.model) {
          // On a policy decline, the API answers with another model instead of stopping.
          body["fallbacks"] = json!("default");
          r = r.set("anthropic-beta", "server-side-fallback-2026-07-01");
        }
        r.send_json(body)
      }
    }
    .map_err(http_error)?;

    let mut answer = String::new();
    let reader = BufReader::new(resp.into_reader());
    for line in reader.lines() {
      if st.cancelled.lock().remove(&request_id) {
        break;
      }
      let line = line.map_err(|e| format!("réponse interrompue : {e}"))?;
      let Some(data) = line.strip_prefix("data:").map(str::trim) else { continue };
      if data == "[DONE]" {
        break;
      }
      let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
      if let Some(msg) = v["error"]["message"].as_str() {
        return Err(msg.to_string());
      }
      let text = match p.kind {
        ApiKind::Openai => v["choices"][0]["delta"]["content"].as_str().map(str::to_string),
        ApiKind::Anthropic => match v["type"].as_str() {
          Some("content_block_delta") if v["delta"]["type"] == "text_delta" => v["delta"]["text"].as_str().map(str::to_string),
          Some("message_delta") if v["delta"]["stop_reason"] == "refusal" => {
            Some("\n\n_(Le modèle a refusé de répondre à cette demande.)_".to_string())
          }
          Some("message_delta") if v["delta"]["stop_reason"] == "max_tokens" => Some("\n\n_(Réponse coupée : limite de longueur atteinte.)_".to_string()),
          _ => None,
        },
      };
      if let Some(t) = text.filter(|t| !t.is_empty()) {
        answer.push_str(&t);
        let _ = emitter.emit("ai-delta", Delta { request_id: request_id.clone(), text: t });
      }
    }
    Ok(answer)
  })
  .await
  .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn ai_cancel(st: State<'_, AiState>, request_id: String) {
  st.cancelled.lock().insert(request_id);
}

#[derive(Serialize)]
pub struct DocContext {
  text: String,
  /// Pages included (1-based), in order.
  pages: Vec<u32>,
  /// Some pages were left out to fit the budget.
  partial: bool,
}

fn words(s: &str) -> Vec<String> {
  s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 4).map(str::to_string).collect()
}

/// Document text for a question, within `budget` characters: everything when
/// it fits, else the current page and the pages that share the most words
/// with the question.
#[tauri::command]
pub async fn ai_doc_context(lib: State<'_, Library>, id: String, question: String, current: u32, budget: usize) -> Result<DocContext, String> {
  let bytes = lib.bytes(&id)?;
  tauri::async_runtime::spawn_blocking(move || {
    let n = kb_core::info(&bytes).map_err(|e| e.to_string())?.page_count;
    let pages: Vec<String> = (0..n).map(|p| kb_core::to_text(&bytes, &[p]).unwrap_or_default().trim().to_string()).collect();
    let total: usize = pages.iter().map(|t| t.chars().count() + 20).sum();
    let mut chosen: Vec<u32> = if total <= budget {
      (0..n).collect()
    } else {
      let q = words(&question);
      let mut order: Vec<(usize, u32)> = (0..n)
        .map(|p| {
          let w = words(&pages[p as usize]);
          (q.iter().map(|x| w.iter().filter(|y| *y == x).count()).sum::<usize>(), p)
        })
        .collect();
      // Current page first, then by relevance, then reading order.
      order.sort_by(|a, b| (b.1 == current).cmp(&(a.1 == current)).then(b.0.cmp(&a.0)).then(a.1.cmp(&b.1)));
      let mut used = 0;
      let mut out = vec![];
      for (_, p) in order {
        let len = pages[p as usize].chars().count() + 20;
        if used + len > budget {
          continue;
        }
        used += len;
        out.push(p);
      }
      out
    };
    chosen.sort();
    let mut text = String::new();
    for &p in &chosen {
      text.push_str(&format!("=== Page {} ===\n{}\n\n", p + 1, pages[p as usize]));
    }
    // A single page bigger than the budget: send its beginning.
    if chosen.is_empty() {
      let p = current.min(n.saturating_sub(1));
      let t: String = pages.get(p as usize).map(|t| t.chars().take(budget).collect()).unwrap_or_default();
      text = format!("=== Page {} (début) ===\n{t}\n", p + 1);
      chosen.push(p);
    }
    Ok(DocContext { partial: chosen.len() < n as usize, pages: chosen.into_iter().map(|p| p + 1).collect(), text })
  })
  .await
  .map_err(|e| e.to_string())?
}
