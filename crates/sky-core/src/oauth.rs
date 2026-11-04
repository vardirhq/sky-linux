//! Browser sign-in, run by Sky Core itself so the page the user lands on
//! after signing in is Sky Linux's own (translated, no mention of rclone).
//!
//! This is the standard OAuth 2.0 authorization-code flow with PKCE, using
//! exactly the parameters rclone uses for Jottacloud white-labels (see
//! `SERVICES`, copied from rclone v1.75.2 `backend/jottacloud`). The token
//! is then handed to rclone, which refreshes it from then on; Sky Core never
//! stores it anywhere else.

use std::time::Duration;

use base64::Engine;
use chrono::Utc;
use rand::distr::{Alphanumeric, SampleString};
use reqwest::Url;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::error::{CoreError, Result};
use crate::provider::Provider;

/// The redirect URI registered for these OAuth clients (rclone's
/// `oauthutil.RedirectLocalhostURL`). It must match exactly.
pub const REDIRECT_PORT: u16 = 53682;
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// OAuth settings for one Jottacloud white-label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthService {
    /// Base URL of the identity server, e.g. `https://sky-auth.telia.no`.
    pub base_url: String,
    pub realm: &'static str,
    pub client_id: &'static str,
    pub scopes: &'static [&'static str],
}

const JOTTA_SCOPES: &[&str] = &["openid", "jotta-default", "offline_access"];
const BASIC_SCOPES: &[&str] = &["openid", "offline_access"];

impl OAuthService {
    pub fn for_provider(p: Provider) -> Self {
        // (domain, realm, client id, scopes) — from rclone v1.75.2.
        let (domain, realm, client_id, scopes) = match p {
            Provider::TeliaSkyNo => ("sky-auth.telia.no", "get", "desktop", JOTTA_SCOPES),
            Provider::TeliaCloudSe => ("cloud-auth.telia.se", "telia_se", "desktop", JOTTA_SCOPES),
            Provider::Jottacloud => ("id.jottacloud.com", "jottacloud", "desktop", JOTTA_SCOPES),
            Provider::ElkjopNo => ("cloud.elkjop.no", "elkjop", "desktop", JOTTA_SCOPES),
            Provider::Tele2Se => ("mittcloud-auth.tele2.se", "comhem", "desktop", JOTTA_SCOPES),
            Provider::Onlime => ("cloud-auth.onlime.dk", "onlime_wl", "desktop", JOTTA_SCOPES),
            Provider::PhoneroSky => ("sky.phonero.no", "phonero", "desktop-win", BASIC_SCOPES),
        };
        Self {
            base_url: format!("https://{domain}"),
            realm,
            client_id,
            scopes,
        }
    }

    fn well_known(&self) -> String {
        format!(
            "{}/auth/realms/{}/.well-known/openid-configuration",
            self.base_url, self.realm
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Endpoints {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

/// A token in the JSON shape rclone stores (`golang.org/x/oauth2.Token`).
pub fn rclone_token_json(
    access: &str,
    refresh: Option<&str>,
    token_type: Option<&str>,
    expires_in: Option<i64>,
) -> String {
    let expiry = Utc::now() + chrono::Duration::seconds(expires_in.unwrap_or(3600));
    serde_json::json!({
        "access_token": access,
        "token_type": token_type.unwrap_or("Bearer"),
        "refresh_token": refresh.unwrap_or_default(),
        "expiry": expiry.to_rfc3339(),
    })
    .to_string()
}

/// Strings for the page shown in the browser after signing in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageLanguage {
    Norwegian,
    English,
}

impl PageLanguage {
    pub fn from_setting(s: Option<&str>) -> Self {
        match s {
            Some("en") => Self::English,
            _ => Self::Norwegian, // the default language of Sky Linux
        }
    }
}

const PAGE: &str = include_str!("oauth_page.html");

/// Render the page shown after the redirect.
pub fn render_page(lang: PageLanguage, provider: &str, ok: bool) -> String {
    let (lang_code, title, heading, body) = match (lang, ok) {
        (PageLanguage::Norwegian, true) => (
            "nb",
            "Logget inn",
            "Du er logget inn",
            format!("Sky Linux er koblet til {provider}. Du kan lukke denne fanen og gå tilbake til Sky Linux."),
        ),
        (PageLanguage::Norwegian, false) => (
            "nb",
            "Innloggingen ble ikke fullført",
            "Innloggingen ble ikke fullført",
            "Ingenting er endret. Gå tilbake til Sky Linux og prøv igjen.".to_string(),
        ),
        (PageLanguage::English, true) => (
            "en",
            "Signed in",
            "You're signed in",
            format!("Sky Linux is connected to {provider}. You can close this tab and return to Sky Linux."),
        ),
        (PageLanguage::English, false) => (
            "en",
            "Sign-in wasn't completed",
            "Sign-in wasn't completed",
            "Nothing was changed. Go back to Sky Linux and try again.".to_string(),
        ),
    };
    PAGE.replace("{{lang}}", lang_code)
        .replace("{{title}}", title)
        .replace("{{state}}", if ok { "ok" } else { "error" })
        .replace("{{heading}}", &html_escape(heading))
        .replace("{{body}}", &html_escape(&body))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// PKCE verifier and its S256 challenge.
pub fn pkce_pair() -> (String, String) {
    let verifier = Alphanumeric.sample_string(&mut rand::rng(), 64);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

/// What the browser sent back to the redirect URI.
#[derive(Debug, PartialEq, Eq)]
pub enum Callback {
    Code {
        code: String,
        state: String,
    },
    Denied {
        error: String,
        state: Option<String>,
    },
    /// Anything else (favicon requests, probes): answer 404 and keep waiting.
    Other,
}

/// Parse the request line of an HTTP request to the redirect URI.
pub fn parse_callback(request_line: &str) -> Callback {
    let mut parts = request_line.split_whitespace();
    let (Some("GET"), Some(target)) = (parts.next(), parts.next()) else {
        return Callback::Other;
    };
    let Ok(url) = Url::parse(&format!("http://localhost{target}")) else {
        return Callback::Other;
    };
    if url.path() != "/" {
        return Callback::Other;
    }
    let get = |k: &str| {
        url.query_pairs()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.into_owned())
    };
    match (get("code"), get("state"), get("error")) {
        (Some(code), Some(state), _) => Callback::Code { code, state },
        (_, state, Some(error)) => Callback::Denied { error, state },
        _ => Callback::Other,
    }
}

/// Runs one sign-in: owns the loopback listener until the browser returns.
pub struct SignIn {
    service: OAuthService,
    endpoints: Endpoints,
    listener: TcpListener,
    redirect_uri: String,
    state: String,
    verifier: String,
    pub auth_url: String,
    http: reqwest::Client,
}

fn http_client(base_url: &str) -> Result<reqwest::Client> {
    let mut b = reqwest::Client::builder()
        .user_agent(concat!("sky-linux/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30));
    if base_url.starts_with("http://127.0.0.1") || base_url.starts_with("http://localhost") {
        b = b.no_proxy(); // tests run a local identity server
    }
    b.build().map_err(|e| CoreError::Invalid(e.to_string()))
}

impl SignIn {
    /// Discover the endpoints and start listening on the redirect port.
    pub async fn start(service: OAuthService, port: u16) -> Result<Self> {
        let http = http_client(&service.base_url)?;
        let endpoints: Endpoints = http
            .get(service.well_known())
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| CoreError::Rclone {
                path: "oauth/discovery".into(),
                status: e.status().map(|s| s.as_u16()).unwrap_or(0),
                message: e.to_string(),
            })?
            .json()
            .await
            .map_err(|e| CoreError::Invalid(format!("unexpected sign-in server response: {e}")))?;

        let listener = TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| {
            CoreError::rule(
                "sign_in_port_busy",
                format!(
                    "Port {port} is in use, so the sign-in page can't return to Sky Linux ({e})."
                ),
            )
            .with("port", port)
        })?;
        let redirect_uri = format!("http://localhost:{port}/");
        let state = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let (verifier, challenge) = pkce_pair();
        let mut url = Url::parse(&endpoints.authorization_endpoint)
            .map_err(|e| CoreError::Invalid(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("client_id", service.client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", &service.scopes.join(" "))
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("access_type", "offline");
        Ok(Self {
            auth_url: url.to_string(),
            service,
            endpoints,
            listener,
            redirect_uri,
            state,
            verifier,
            http,
        })
    }

    pub fn token_endpoint(&self) -> &str {
        &self.endpoints.token_endpoint
    }

    /// Wait for the browser, show our page, exchange the code. Returns the
    /// token as rclone-compatible JSON.
    pub async fn finish(self, lang: PageLanguage, provider_name: &str) -> Result<String> {
        let deadline = tokio::time::Instant::now() + SIGN_IN_TIMEOUT;
        loop {
            let (mut stream, _) = tokio::time::timeout_at(deadline, self.listener.accept())
                .await
                .map_err(|_| {
                    CoreError::rule(
                        "sign_in_timeout",
                        "Sign-in took too long. Please try again.",
                    )
                })??;
            let line = read_request_line(&mut stream).await.unwrap_or_default();
            match parse_callback(&line) {
                Callback::Other => {
                    let _ = respond(&mut stream, "404 Not Found", "").await;
                }
                Callback::Denied { state, .. } if state.as_deref() != Some(&self.state) => {
                    let _ = respond(&mut stream, "400 Bad Request", "").await;
                }
                Callback::Code { state, .. } if state != self.state => {
                    // Not our sign-in (stale tab, or a forged request).
                    let _ = respond(&mut stream, "400 Bad Request", "").await;
                }
                Callback::Denied { error, .. } => {
                    let page = render_page(lang, provider_name, false);
                    let _ = respond(&mut stream, "200 OK", &page).await;
                    return Err(CoreError::rule(
                        "sign_in_cancelled",
                        format!("Sign-in was cancelled ({error})."),
                    ));
                }
                Callback::Code { code, .. } => {
                    let result = self.exchange(&code).await;
                    let page = render_page(lang, provider_name, result.is_ok());
                    let _ = respond(&mut stream, "200 OK", &page).await;
                    return result;
                }
            }
        }
    }

    async fn exchange(&self, code: &str) -> Result<String> {
        let resp = self
            .http
            .post(&self.endpoints.token_endpoint)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", self.redirect_uri.as_str()),
                ("client_id", self.service.client_id),
                ("code_verifier", self.verifier.as_str()),
            ])
            .send()
            .await
            .map_err(|e| CoreError::Rclone {
                path: "oauth/token".into(),
                status: 0,
                message: e.to_string(),
            })?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(CoreError::Rclone {
                path: "oauth/token".into(),
                status: status.as_u16(),
                message: format!("token exchange failed: {body}"),
            });
        }
        let t: TokenResponse = resp
            .json()
            .await
            .map_err(|e| CoreError::Invalid(format!("unexpected token response: {e}")))?;
        if t.refresh_token.as_deref().unwrap_or_default().is_empty() {
            // Without a refresh token rclone can't stay signed in.
            return Err(CoreError::Invalid(
                "The sign-in server didn't grant offline access.".into(),
            ));
        }
        Ok(rclone_token_json(
            &t.access_token,
            t.refresh_token.as_deref(),
            t.token_type.as_deref(),
            t.expires_in,
        ))
    }
}

async fn read_request_line(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut buf = vec![0u8; 8192];
    let mut len = 0;
    loop {
        let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buf[len..]))
            .await
            .map_err(|_| std::io::Error::other("timeout"))??;
        if n == 0 {
            break;
        }
        len += n;
        if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") || len == buf.len() {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buf[..len]);
    Ok(text.lines().next().unwrap_or_default().to_string())
}

async fn respond(stream: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nReferrer-Policy: no-referrer\r\n\
         Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; img-src data:\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    stream.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telia_parameters_match_rclone() {
        let s = OAuthService::for_provider(Provider::TeliaSkyNo);
        assert_eq!(
            s.well_known(),
            "https://sky-auth.telia.no/auth/realms/get/.well-known/openid-configuration"
        );
        assert_eq!(s.client_id, "desktop");
        assert_eq!(s.scopes, ["openid", "jotta-default", "offline_access"]);
    }

    #[test]
    fn parses_callbacks() {
        assert_eq!(
            parse_callback("GET /?state=abc&code=xyz%2B1 HTTP/1.1"),
            Callback::Code {
                code: "xyz+1".into(),
                state: "abc".into()
            }
        );
        assert_eq!(
            parse_callback("GET /?error=access_denied&state=abc HTTP/1.1"),
            Callback::Denied {
                error: "access_denied".into(),
                state: Some("abc".into())
            }
        );
        assert_eq!(parse_callback("GET /favicon.ico HTTP/1.1"), Callback::Other);
        assert_eq!(
            parse_callback("POST /?code=1&state=2 HTTP/1.1"),
            Callback::Other
        );
        assert_eq!(parse_callback(""), Callback::Other);
    }

    #[test]
    fn pkce_challenge_is_s256_of_verifier() {
        let (v, c) = pkce_pair();
        assert_eq!(v.len(), 64);
        let expected =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(v.as_bytes()));
        assert_eq!(c, expected);
        assert!(!c.contains('=') && !c.contains('+') && !c.contains('/'));
    }

    #[test]
    fn token_json_has_rclone_shape() {
        let j: serde_json::Value =
            serde_json::from_str(&rclone_token_json("a", Some("r"), None, Some(300))).unwrap();
        assert_eq!(j["access_token"], "a");
        assert_eq!(j["refresh_token"], "r");
        assert_eq!(j["token_type"], "Bearer");
        let expiry = chrono::DateTime::parse_from_rfc3339(j["expiry"].as_str().unwrap()).unwrap();
        assert!(expiry > Utc::now() + chrono::Duration::seconds(250));
    }

    #[test]
    fn page_is_translated_and_never_mentions_rclone() {
        let nb = render_page(PageLanguage::Norwegian, "Telia Sky", true);
        assert!(nb.contains("lang=\"nb\"") && nb.contains("Du er logget inn"));
        assert!(nb.contains("koblet til Telia Sky"));
        let en = render_page(PageLanguage::English, "Telia Sky", false);
        assert!(en.contains("Sign-in wasn't completed"));
        for p in [nb, en] {
            assert!(!p.to_lowercase().contains("rclone"));
            assert!(!p.contains("{{"));
        }
        assert_eq!(PageLanguage::from_setting(None), PageLanguage::Norwegian);
    }

    #[test]
    fn escapes_html() {
        assert_eq!(html_escape("<a & \"b\">"), "&lt;a &amp; &quot;b&quot;&gt;");
    }
}
