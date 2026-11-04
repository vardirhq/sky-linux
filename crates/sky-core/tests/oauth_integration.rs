//! Sky Core's own browser sign-in, end to end against a stand-in identity
//! server (Keycloak-shaped: well-known discovery, PKCE, token endpoint),
//! then handing the token to a real rclone. The rclone part is skipped
//! when rclone isn't installed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use base64::Engine;
use sha2::{Digest, Sha256};
use sky_core::oauth::{OAuthService, PageLanguage, SignIn};
use sky_core::paths::AppPaths;
use sky_core::provider::Provider;
use sky_core::rclone::SupervisorConfig;
use sky_core::secrets::MemorySecretStore;
use sky_core::service::{CoreOptions, SkyCore};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Default)]
struct IdpState {
    /// code_challenge from the authorization request (what the browser saw).
    challenge: Option<String>,
    token_requests: Vec<HashMap<String, String>>,
}

/// A minimal identity server on 127.0.0.1.
async fn identity_server(state: Arc<Mutex<IdpState>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let base2 = base.clone();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = listener.accept().await.unwrap();
            let state = state.clone();
            let base = base2.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16384];
                let mut n = 0;
                loop {
                    let r = s.read(&mut buf[n..]).await.unwrap();
                    n += r;
                    let text = String::from_utf8_lossy(&buf[..n]).to_string();
                    if let Some(i) = text.find("\r\n\r\n") {
                        let len = text
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if n >= i + 4 + len || r == 0 {
                            break;
                        }
                    }
                    if r == 0 {
                        break;
                    }
                }
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let line = req.lines().next().unwrap_or_default().to_string();
                let body = req.split("\r\n\r\n").nth(1).unwrap_or_default().to_string();
                let (status, json) = if line
                    .starts_with("GET /auth/realms/get/.well-known/openid-configuration")
                {
                    (
                        "200 OK",
                        serde_json::json!({
                            "authorization_endpoint": format!("{base}/auth/realms/get/protocol/openid-connect/auth"),
                            "token_endpoint": format!("{base}/auth/realms/get/protocol/openid-connect/token"),
                        }),
                    )
                } else if line.starts_with("POST /auth/realms/get/protocol/openid-connect/token") {
                    let form: HashMap<String, String> =
                        url::form_urlencoded::parse(body.as_bytes())
                            .into_owned()
                            .collect();
                    let mut st = state.lock().unwrap();
                    st.token_requests.push(form.clone());
                    let verifier_ok = st.challenge.as_deref()
                        == Some(
                            &base64::engine::general_purpose::URL_SAFE_NO_PAD
                                .encode(Sha256::digest(form["code_verifier"].as_bytes())),
                        );
                    if form["code"] == "good-code"
                        && verifier_ok
                        && form["grant_type"] == "authorization_code"
                    {
                        (
                            "200 OK",
                            serde_json::json!({
                                "access_token": "AT-123", "refresh_token": "RT-456",
                                "token_type": "Bearer", "expires_in": 300
                            }),
                        )
                    } else {
                        (
                            "400 Bad Request",
                            serde_json::json!({ "error": "invalid_grant" }),
                        )
                    }
                } else {
                    ("404 Not Found", serde_json::json!({}))
                };
                let body = json.to_string();
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    base
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn browser() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

fn service(base: &str) -> OAuthService {
    OAuthService {
        base_url: base.to_string(),
        ..OAuthService::for_provider(Provider::TeliaSkyNo)
    }
}

fn query(url: &str) -> HashMap<String, String> {
    reqwest::Url::parse(url)
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn sign_in_shows_our_page_and_returns_a_token() {
    let idp = Arc::new(Mutex::new(IdpState::default()));
    let base = identity_server(idp.clone()).await;
    let port = free_port();

    let sign_in = SignIn::start(service(&base), port).await.unwrap();
    let q = query(&sign_in.auth_url);
    assert!(sign_in.auth_url.starts_with(&format!(
        "{base}/auth/realms/get/protocol/openid-connect/auth?"
    )));
    assert_eq!(q["client_id"], "desktop");
    assert_eq!(q["response_type"], "code");
    assert_eq!(q["scope"], "openid jotta-default offline_access");
    assert_eq!(q["redirect_uri"], format!("http://localhost:{port}/"));
    assert_eq!(q["code_challenge_method"], "S256");
    idp.lock().unwrap().challenge = Some(q["code_challenge"].clone());
    let state = q["state"].clone();

    let finish = tokio::spawn(sign_in.finish(PageLanguage::Norwegian, "Telia Sky"));
    let b = browser();
    let cb = format!("http://127.0.0.1:{port}");
    // Browsers ask for a favicon; a stale tab or forged request has the wrong state.
    assert_eq!(
        b.get(format!("{cb}/favicon.ico"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        b.get(format!("{cb}/?code=good-code&state=wrong"))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    // The real redirect from the identity server:
    let page = b
        .get(format!("{cb}/?code=good-code&state={state}"))
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), 200);
    assert!(page.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .contains("default-src 'none'"));
    let html = page.text().await.unwrap();
    assert!(html.contains("Du er logget inn"), "{html}");
    assert!(!html.to_lowercase().contains("rclone"));

    let token: serde_json::Value = serde_json::from_str(&finish.await.unwrap().unwrap()).unwrap();
    assert_eq!(token["access_token"], "AT-123");
    assert_eq!(token["refresh_token"], "RT-456");
    let req = idp.lock().unwrap().token_requests[0].clone();
    assert_eq!(req["redirect_uri"], format!("http://localhost:{port}/"));
    assert_eq!(req["client_id"], "desktop");
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelled_or_failed_sign_in_shows_the_error_page() {
    let idp = Arc::new(Mutex::new(IdpState::default()));
    let base = identity_server(idp.clone()).await;
    let b = browser();

    // User pressed "cancel" on the sign-in page.
    let port = free_port();
    let sign_in = SignIn::start(service(&base), port).await.unwrap();
    let state = query(&sign_in.auth_url)["state"].clone();
    let finish = tokio::spawn(sign_in.finish(PageLanguage::English, "Telia Sky"));
    let html = b
        .get(format!(
            "http://127.0.0.1:{port}/?error=access_denied&state={state}"
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("Sign-in wasn't completed"));
    let err = finish.await.unwrap().unwrap_err().to_user();
    assert_eq!(err.rule.as_deref(), Some("sign_in_cancelled"));

    // The identity server rejects the code (e.g. a wrong PKCE verifier).
    let port = free_port();
    let sign_in = SignIn::start(service(&base), port).await.unwrap();
    let state = query(&sign_in.auth_url)["state"].clone();
    idp.lock().unwrap().challenge = Some("not-the-right-challenge".into());
    let finish = tokio::spawn(sign_in.finish(PageLanguage::Norwegian, "Telia Sky"));
    let html = b
        .get(format!(
            "http://127.0.0.1:{port}/?code=good-code&state={state}"
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("Innloggingen ble ikke fullført"));
    assert!(finish.await.unwrap().is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn busy_redirect_port_is_reported_clearly() {
    let idp = Arc::new(Mutex::new(IdpState::default()));
    let base = identity_server(idp).await;
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let err = SignIn::start(service(&base), port)
        .await
        .err()
        .unwrap()
        .to_user();
    assert_eq!(err.rule.as_deref(), Some("sign_in_port_busy"));
    assert_eq!(err.params["port"], port.to_string());
}

#[tokio::test(flavor = "multi_thread")]
async fn rclone_accepts_our_token_without_its_own_sign_in() {
    let Some(rclone) = SupervisorConfig::find_binary() else {
        eprintln!("rclone not found; skipping");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let core = SkyCore::start(CoreOptions {
        paths: AppPaths::rooted(dir.path().join("s")),
        rclone_binary: rclone,
        secrets: Arc::new(MemorySecretStore::default()),
        encrypt_rclone_config: false,
    })
    .await
    .unwrap();
    let token = sky_core::oauth::rclone_token_json("AT-123", Some("RT-456"), None, Some(300));
    core.accounts
        .create_remote_from_token(Provider::TeliaSkyNo, "sky-telia-no", &token)
        .await
        .expect("rclone must not ask to open its own browser page");

    let cfg = core
        .client()
        .call("config/get", serde_json::json!({ "name": "sky-telia-no" }))
        .await
        .unwrap();
    assert_eq!(cfg["type"], "jottacloud");
    assert_eq!(cfg["client_id"], "desktop");
    assert_eq!(
        cfg["tokenURL"],
        "https://sky-auth.telia.no/auth/realms/get/protocol/openid-connect/token"
    );
    let stored: serde_json::Value = serde_json::from_str(cfg["token"].as_str().unwrap()).unwrap();
    assert_eq!(stored["refresh_token"], "RT-456");
    core.shutdown().await;
}
