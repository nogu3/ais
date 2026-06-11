//! フェッチ層。HTTP + Digest 認証（ureq）のみを担い、レスポンスの中身は解釈しない。

pub mod digest;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tracing::debug;

use crate::error::{AisError, ErrorKind, Result};

pub struct Client {
    agent: ureq::Agent,
    host: String,
    user: String,
    pass: String,
}

struct RawResponse {
    status: u16,
    www_authenticate: Option<String>,
    location: Option<String>,
    body: String,
}

/// リダイレクト追従の上限（ループ防止の安全弁）。
const MAX_REDIRECTS: u32 = 5;

impl Client {
    pub fn new(host: &str, user: &str, pass: &str, timeout_secs: u64) -> Self {
        // リダイレクトは自前で追従する。ureq に任せると Digest の uri が
        // リダイレクト先とずれて 401 になる（例: トップページ `/` → `/index.cgi`）。
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(timeout_secs)))
            .build();
        Self {
            agent: config.into(),
            host: host.to_string(),
            user: user.to_string(),
            pass: pass.to_string(),
        }
    }

    /// パス（クエリ込み可）を GET し、ボディ文字列を返す。
    pub fn get(&self, path_query: &str) -> Result<String> {
        self.request("GET", path_query, None)
    }

    /// `application/x-www-form-urlencoded` ボディを POST する。
    pub fn post_form(&self, path_query: &str, body: &str) -> Result<String> {
        self.request("POST", path_query, Some(body))
    }

    fn request(&self, method: &str, path_query: &str, body: Option<&str>) -> Result<String> {
        let mut path = path_query.to_string();
        for _ in 0..MAX_REDIRECTS {
            let res = self.request_once(method, &path, body)?;
            // GET のみリダイレクトを追従し、リダイレクト先で Digest をやり直す
            if method == "GET" && matches!(res.status, 301 | 302 | 303 | 307 | 308) {
                if let Some(next) = res.location.as_deref().map(normalize_location) {
                    debug!(from = %path, to = %next, "following redirect");
                    path = next;
                    continue;
                }
            }
            return ok_or_status(res, &path);
        }
        Err(AisError::new(
            ErrorKind::HttpStatus,
            format!("too many redirects for {path_query}"),
        ))
    }

    fn request_once(&self, method: &str, path_query: &str, body: Option<&str>) -> Result<RawResponse> {
        let first = self.send(method, path_query, body, None)?;
        if first.status != 401 {
            return Ok(first);
        }

        // 401 → Digest チャレンジに応答して 1 回だけ再試行
        let header = first.www_authenticate.ok_or_else(|| {
            AisError::new(
                ErrorKind::AuthFailed,
                format!("401 without WWW-Authenticate for {path_query}"),
            )
        })?;
        let challenge = digest::parse_challenge(&header).ok_or_else(|| {
            AisError::new(
                ErrorKind::AuthFailed,
                format!("unsupported auth challenge: {header}"),
            )
        })?;
        let auth = digest::authorization(
            &self.user,
            &self.pass,
            method,
            path_query,
            &challenge,
            &generate_cnonce(),
            "00000001",
        );

        let second = self.send(method, path_query, body, Some(&auth))?;
        if second.status == 401 {
            return Err(AisError::new(
                ErrorKind::AuthFailed,
                format!("digest authentication rejected for {path_query}"),
            ));
        }
        Ok(second)
    }

    fn send(
        &self,
        method: &str,
        path_query: &str,
        body: Option<&str>,
        auth: Option<&str>,
    ) -> Result<RawResponse> {
        let url = format!("http://{}{}", self.host, path_query);
        debug!(method, url, "sending request");

        let result = match method {
            "POST" => {
                let mut req = self
                    .agent
                    .post(&url)
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .header("X-Requested-With", "XMLHttpRequest");
                if let Some(a) = auth {
                    req = req.header("Authorization", a);
                }
                req.send(body.unwrap_or(""))
            }
            _ => {
                let mut req = self.agent.get(&url);
                if let Some(a) = auth {
                    req = req.header("Authorization", a);
                }
                req.call()
            }
        };

        let mut response = result.map_err(|e| map_ureq_error(e, &url))?;
        let status = response.status().as_u16();
        let www_authenticate = response
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let location = response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|e| map_ureq_error(e, &url))?;
        debug!(status, bytes = body.len(), "received response");

        Ok(RawResponse {
            status,
            www_authenticate,
            location,
            body,
        })
    }
}

fn ok_or_status(res: RawResponse, path_query: &str) -> Result<String> {
    if (200..300).contains(&res.status) {
        Ok(res.body)
    } else {
        Err(AisError::new(
            ErrorKind::HttpStatus,
            format!("unexpected HTTP {} for {path_query}", res.status),
        ))
    }
}

/// Location ヘッダーをホスト相対パスに正規化する（絶対 URL はパス以降を取り出す）。
fn normalize_location(loc: &str) -> String {
    if let Some(rest) = loc
        .strip_prefix("http://")
        .or_else(|| loc.strip_prefix("https://"))
    {
        match rest.find('/') {
            Some(i) => rest[i..].to_string(),
            None => "/".to_string(),
        }
    } else {
        loc.to_string()
    }
}

fn map_ureq_error(e: ureq::Error, url: &str) -> AisError {
    let kind = match &e {
        ureq::Error::Timeout(_) => ErrorKind::Timeout,
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => ErrorKind::Timeout,
        _ => ErrorKind::Network,
    };
    AisError::new(kind, format!("{e} ({url})"))
}

fn generate_cnonce() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("{:016x}", nanos ^ ((std::process::id() as u64) << 32))
}
