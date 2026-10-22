//! The end-to-end tests of the whole router (T13-A-2).
//!
//! The tests build the same router the server runs (`build_router`) and call
//! it with `tower::ServiceExt::oneshot`, so a route is exercised through the
//! real stack: the layers, the authentication and the handlers.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

use crate::admin::password;
use crate::admin::session;
use crate::appkeys::AppKeyStore;
use crate::auth::{AuthConfig, AuthService};
use crate::config::store::ConfigStore;
use crate::credentials::{AuthMap, CredentialManager, ProjectIdMap};
use crate::datasource::registry::DataSourceRegistry;
use crate::datasource::test_support::{json as json_response, TestServer};
use crate::db::TemplateRateLimiter;
use crate::devices::{DeviceStore, RoleDirectory};
use crate::identity::jwt::EnvSource;
use crate::identity::registry::IdentityRegistry;
use crate::ratelimit::RateLimiter;
use crate::send::test_support::FakeTransport;
use crate::send::FcmTransport;
use crate::{build_router, AppState};

const APP: &str = "watt";
const OTHER_APP: &str = "other";
const SERVICE_KEY: &str = "e2e-service-key";
const LEGACY_KEY: &str = "e2e-legacy-key";
const PASSWORD: &str = "e2e dashboard password 2026";
const SECRET_ENV: &str = "E2E_RESOLVER_SECRET";
const SECRET: &str = "e2e-resolver-secret-0123456789abcdef";
const CSRF: &str = "fcm-dashboard";

/// An environment with the secret of the resolver and nothing else.
struct FakeEnv;

impl EnvSource for FakeEnv {
    fn get(&self, name: &str) -> Option<String> {
        match name {
            SECRET_ENV => Some(SECRET.to_string()),
            _ => None,
        }
    }
}

/// The state of the tests: the whole server, without a real network.
struct Harness {
    state: Arc<AppState>,
    router: Router,
    pool: SqlitePool,
    /// The resolver of the entities, when the test needs one. It stays in the
    /// harness so the server keeps running while the test uses it.
    _resolver: Option<TestServer>,
}

impl Harness {
    /// The session cookie of the owner of the dashboard.
    async fn owner_cookie(&self) -> String {
        let response = self
            .router
            .clone()
            .oneshot(request(
                Method::POST,
                "/api/v1/admin/auth/login",
                Some(json!({ "email": "owner@example.com", "password": PASSWORD })),
                None,
                true,
            ))
            .await
            .expect("the login must run");
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        session::read_cookie([cookie.as_str()], session::SECURE_COOKIE_NAME)
            .expect("the cookie must carry the token")
    }
}

/// Builds a request with the headers the routes expect.
fn request(
    method: Method,
    uri: &str,
    body: Option<Value>,
    bearer: Option<&str>,
    csrf: bool,
) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if csrf {
        builder = builder.header("x-requested-with", CSRF);
    }
    if let Some(key) = bearer {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {key}"));
    }
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    let mut request = builder.body(body).expect("the request must build");
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40_000))));
    request
}

/// A request with the cookie of a dashboard session.
fn request_with_cookie(
    method: Method,
    uri: &str,
    body: Option<Value>,
    cookie: &str,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(
            header::COOKIE,
            format!("{}={}", session::SECURE_COOKIE_NAME, cookie),
        )
        .header("x-requested-with", CSRF);
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    let mut request = builder.body(body).expect("the request must build");
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40_000))));
    request
}

/// Sends a request and returns the status and the body.
async fn call(harness: &Harness, request: Request<Body>) -> (StatusCode, Value) {
    let response = harness
        .router
        .clone()
        .oneshot(request)
        .await
        .expect("the router must answer");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("the body must be readable")
        .to_bytes();
    let text = String::from_utf8_lossy(&bytes).to_string();
    let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
    (status, body)
}

/// The in-memory database of the tests.
async fn memory_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("the in-memory database must open");
    crate::db::migrate(&pool)
        .await
        .expect("the migrations must run");
    pool
}

/// Builds the state of the tests.
///
/// The resolver of the entities is a local server, so no test reaches the
/// network.
async fn harness(resolver: Option<TestServer>) -> Harness {
    let pool = memory_pool().await;
    let fcm: Arc<dyn FcmTransport> = FakeTransport::accepting();

    let project_map: ProjectIdMap = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
    let auth_map: AuthMap = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
    let config = Arc::new(
        ConfigStore::new(pool.clone())
            .await
            .expect("the config store must open"),
    );
    config
        .create_app(APP, "tester")
        .await
        .expect("the app must be created");
    config
        .create_app(OTHER_APP, "tester")
        .await
        .expect("the second app must be created");

    let identity = Arc::new(IdentityRegistry::new(
        config.clone(),
        project_map.clone(),
        reqwest::Client::new(),
        Arc::new(FakeEnv),
    ));
    let datasource = Arc::new(DataSourceRegistry::new(
        config.clone(),
        project_map.clone(),
        auth_map.clone(),
        reqwest::Client::new(),
        Arc::new(FakeEnv),
    ));

    let app_keys = Arc::new(AppKeyStore::new(pool.clone()));
    let auth = AuthService::new(
        project_map.clone(),
        identity.clone(),
        datasource.clone(),
        config.clone(),
        AuthConfig {
            service_api_key: Some(SERVICE_KEY.to_string()),
            legacy_api_key: Some(LEGACY_KEY.to_string()),
            legacy_enabled: true,
        },
    )
    .with_app_keys(app_keys.clone());

    let tool_auth = Arc::new(crate::admin::ToolAuth {
        pool: pool.clone(),
        cookie: session::CookieConfig::with_hours(true, 12),
        login_limiter: RateLimiter::new(5, Duration::from_secs(900)),
        dummy_hash: "unused in the end-to-end tests".to_string(),
        allowed_origins: Vec::new(),
    });

    let credentials = Arc::new(CredentialManager::new(
        std::env::temp_dir().join(format!("fcm-e2e-credentials-{}", std::process::id())),
        auth_map.clone(),
        project_map.clone(),
        Arc::new(tokio::sync::RwLock::new(std::collections::HashSet::new())),
        Arc::new(crate::credentials::GoogleVerifier),
        true,
    ));

    let state = Arc::new(AppState {
        db: pool.clone(),
        auth_map: auth_map.clone(),
        project_map: project_map.clone(),
        client: reqwest::Client::new(),
        auth,
        trust_proxy: false,
        topics_limiter: RateLimiter::new(30, Duration::from_secs(60)),
        auth_fail_limiter: RateLimiter::new(10, Duration::from_secs(60)),
        devices: Arc::new(DeviceStore::new(pool.clone())),
        devices_limiter: RateLimiter::new(20, Duration::from_secs(60)),
        config: config.clone(),
        tool_auth,
        identity,
        datasource,
        fcm,
        role_directory: Arc::new(RoleDirectory::new()),
        template_rate: Arc::new(TemplateRateLimiter::new()),
        credentials,
        legacy: Arc::new(crate::legacy::LegacyGuard::new(
            crate::legacy::DEFAULT_LEGACY_RATE_LIMIT_PER_MINUTE,
        )),
        admin_limiters: crate::AdminLimiters::new(),
    });

    // The apps of the tests are known to the server.
    state
        .project_map
        .write()
        .await
        .insert(APP.to_string(), "project-watt".to_string());
    state
        .project_map
        .write()
        .await
        .insert(OTHER_APP.to_string(), "project-other".to_string());

    // The owner of the dashboard.
    let hash = password::hash_password(PASSWORD)
        .await
        .expect("the password must hash");
    sqlx::query(
        "INSERT INTO tool_users (email, password_hash, role, must_change_password) \
         VALUES (?, ?, 'owner', 0)",
    )
    .bind("owner@example.com")
    .bind(&hash)
    .execute(&pool)
    .await
    .expect("the owner must be inserted");

    // The resolver of the entities, when the test needs one.
    if let Some(server) = resolver.as_ref() {
        let value = json!({
            "type": "http",
            "base_url": server.base_url,
            "secret_env": SECRET_ENV,
            "timeout_ms": 2000,
        });
        let document = json!({
            "version": 1,
            "identity": {
                "provider": { "type": "firebase" },
                "role": { "source": "claim", "claim": "role" },
                "admin_roles": ["admin"]
            },
            "data_source": value,
            "campaigns": {
                "enabled": true,
                "senders": ["company"],
                "entity": {
                    "param": "company_id",
                    "doc": "companies/{p.company_id}",
                    "require": { "user_id": "{sender.uid}", "status": "approved" },
                    "name_field": "name"
                },
                "audience": {
                    "allowed_topics": ["company_{p.company_id}"],
                    "required_topic": "promos",
                    "max_topics": 5
                },
                "content": {
                    "title_prefix": "{entity.name}: ",
                    "title_max": 60,
                    "body_max": 240,
                    "forbid_links": true,
                    "actions": {
                        "open_company": {
                            "data": { "action": "open_company" }
                        }
                    }
                },
                "quota": { "per_entity_per_day": 1, "per_entity_per_week": 5 },
                "review": { "mode": "always" }
            }
        });
        config
            .import(APP, document, "tester")
            .await
            .expect("the app configuration must be imported");
    }

    let router = build_router(state.clone());
    Harness {
        state,
        router,
        pool,
        _resolver: resolver,
    }
}

/// A template of the app, ready to be sent.
async fn save_template(harness: &Harness, key: &str) {
    let definition = json!({
        "description": "a greeting",
        "enabled": true,
        "senders": ["admin"],
        "params": { "name": { "type": "text", "max": 40 } },
        "to": { "topic": "news" },
        "title": "Hello {p.name}",
        "body": "Welcome {p.name}",
        "data": {}
    });
    harness
        .state
        .config
        .save_template(APP, key, definition, "tester", None)
        .await
        .expect("the template must be saved");
}

#[tokio::test]
async fn e2e_app_key_send_template_happy_path() {
    let harness = harness(None).await;
    save_template(&harness, "greet").await;

    let key = harness
        .state
        .auth
        .app_keys()
        .expect("the store of the application keys must be there")
        .create(APP, "tester", "tester@example.com")
        .await
        .expect("the application key must be created");

    let (status, body) = call(
        &harness,
        request(
            Method::POST,
            "/api/v1/send/template",
            Some(json!({
                "app": APP,
                "template": "greet",
                "params": { "name": "Sara" }
            })),
            Some(&key.key),
            false,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "the body was {body}");
}

#[tokio::test]
async fn e2e_revoked_app_key_gets_401_on_send_template() {
    let harness = harness(None).await;
    save_template(&harness, "greet").await;

    let key = harness
        .state
        .auth
        .app_keys()
        .expect("the store of the application keys must be there")
        .create(APP, "tester", "tester@example.com")
        .await
        .expect("the application key must be created");
    harness
        .state
        .auth
        .app_keys()
        .expect("the store of the application keys must be there")
        .revoke(APP, key.id)
        .await
        .expect("the application key must be revoked");

    let (status, _) = call(
        &harness,
        request(
            Method::POST,
            "/api/v1/send/template",
            Some(json!({
                "app": APP,
                "template": "greet",
                "params": { "name": "Sara" }
            })),
            Some(&key.key),
            false,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn e2e_app_key_wrong_app_403() {
    let harness = harness(None).await;
    save_template(&harness, "greet").await;

    let key = harness
        .state
        .auth
        .app_keys()
        .expect("the store of the application keys must be there")
        .create(OTHER_APP, "tester", "tester@example.com")
        .await
        .expect("the application key must be created");

    let (status, _) = call(
        &harness,
        request(
            Method::POST,
            "/api/v1/send/template",
            Some(json!({
                "app": APP,
                "template": "greet",
                "params": { "name": "Sara" }
            })),
            Some(&key.key),
            false,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn e2e_legacy_key_only_send() {
    let harness = harness(None).await;
    save_template(&harness, "greet").await;

    // The legacy key is accepted by `/send`.
    let (status, _) = call(
        &harness,
        request(
            Method::POST,
            "/api/v1/send",
            Some(json!({ "app": APP, "topic": "news", "title": "Hi" })),
            Some(LEGACY_KEY),
            false,
        ),
    )
    .await;
    assert_ne!(status, StatusCode::UNAUTHORIZED);

    // It is refused by the template route.
    let (status, _) = call(
        &harness,
        request(
            Method::POST,
            "/api/v1/send/template",
            Some(json!({
                "app": APP,
                "template": "greet",
                "params": { "name": "Sara" }
            })),
            Some(LEGACY_KEY),
            false,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn e2e_admin_route_rejects_app_key() {
    let harness = harness(None).await;

    let key = harness
        .state
        .auth
        .app_keys()
        .expect("the store of the application keys must be there")
        .create(APP, "tester", "tester@example.com")
        .await
        .expect("the application key must be created");

    let (status, _) = call(
        &harness,
        request(
            Method::GET,
            "/api/v1/admin/apps",
            None,
            Some(&key.key),
            true,
        ),
    )
    .await;

    assert!(
        status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN,
        "the admin route must refuse the application key, got {status}"
    );
}

#[tokio::test]
async fn e2e_devices_route_rejects_app_key() {
    let harness = harness(None).await;

    let key = harness
        .state
        .auth
        .app_keys()
        .expect("the store of the application keys must be there")
        .create(APP, "tester", "tester@example.com")
        .await
        .expect("the application key must be created");

    let (status, _) = call(
        &harness,
        request(
            Method::POST,
            "/api/v1/devices/register",
            Some(json!({ "app": APP, "token": "device-token", "platform": "android" })),
            Some(&key.key),
            false,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn e2e_dashboard_login_then_apps_list_with_csrf() {
    let harness = harness(None).await;

    // Without the CSRF header the login is refused.
    let (status, _) = call(
        &harness,
        request(
            Method::POST,
            "/api/v1/admin/auth/login",
            Some(json!({ "email": "owner@example.com", "password": PASSWORD })),
            None,
            false,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // With it, the session works and the list of the apps answers.
    let cookie = harness.owner_cookie().await;
    let (status, body) = call(
        &harness,
        request_with_cookie(Method::GET, "/api/v1/admin/apps", None, &cookie),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "the body was {body}");
    assert!(body.is_array(), "the list of the apps must be an array");
}

#[tokio::test]
async fn e2e_disabled_app_cannot_send() {
    let harness = harness(None).await;
    harness
        .state
        .credentials
        .disable(APP)
        .await
        .expect("the app must be disabled");

    let (status, _) = call(
        &harness,
        request(
            Method::POST,
            "/api/v1/send",
            Some(json!({ "app": APP, "topic": "news", "title": "Hi" })),
            Some(SERVICE_KEY),
            false,
        ),
    )
    .await;

    assert_ne!(status, StatusCode::OK, "a disabled app must not send");
}

#[tokio::test]
async fn e2e_blocked_company_cannot_send_via_campaigns_route() {
    let resolver = TestServer::start(vec![json_response(
        200,
        "OK",
        r#"{"name":"Acme","user_id":"company-1","status":"approved"}"#,
    )])
    .await;
    let harness = harness(Some(resolver)).await;

    // The company is blocked.
    crate::db::block_entity(
        &harness.pool,
        APP,
        "company-1",
        Some("abuse"),
        "tester@example.com",
    )
    .await
    .expect("the entity must be blocked");

    let (status, body) = call(
        &harness,
        request(
            Method::POST,
            "/api/v1/campaigns",
            Some(json!({
                "app": APP,
                "entity_id": "company-1",
                "title": "Sale",
                "body": "Body",
                "action": "open_company",
                "audience": { "groups": [["promos"]] }
            })),
            Some(SERVICE_KEY),
            false,
        ),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a blocked company must not create a campaign: {body}"
    );
}

/// Every route of the router must be in the documented table.
///
/// `build_router` is built from `public_routes()` and `admin_routes()`, so
/// comparing those two tables with the static list compares the router. A new
/// route without a line in `docs/plans/reports/T13A-routes.md` fails the test.
#[test]
fn route_table_is_documented() {
    const DOC: &str = include_str!("../docs/plans/reports/T13A-routes.md");
    let block = DOC
        .split("```routes\n")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .expect("the documented route block");

    let mut documented: Vec<(String, String, String)> = Vec::new();
    for line in block.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let mut fields = line.split('|').map(str::trim);
        let (Some(method), Some(path), Some(auth)) = (fields.next(), fields.next(), fields.next())
        else {
            panic!("the documented line has fewer than three columns: {line}");
        };
        assert!(
            !method.is_empty() && !path.is_empty() && !auth.is_empty(),
            "the documented line has an empty column: {line}"
        );
        documented.push((method.to_string(), path.to_string(), auth.to_string()));
    }

    let mut actual: Vec<(String, String, String)> = crate::public_route_documentation()
        .into_iter()
        .map(|(method, path, auth)| (method, format!("/api/v1{path}"), auth.to_string()))
        .collect();
    actual.extend(
        crate::admin::admin_routes()
            .into_iter()
            .map(|(method, path, session, _)| {
                let auth = if session { "dashboard session" } else { "none" };
                (
                    method.to_string(),
                    format!("/admin{path}"),
                    auth.to_string(),
                )
            }),
    );

    documented.sort();
    actual.sort();

    let undocumented: Vec<_> = actual
        .iter()
        .filter(|route| !documented.contains(route))
        .collect();
    let stale: Vec<_> = documented
        .iter()
        .filter(|route| !actual.contains(route))
        .collect();
    assert!(
        undocumented.is_empty() && stale.is_empty(),
        "undocumented routes: {undocumented:?}; stale document lines: {stale:?}"
    );
    assert_eq!(
        documented, actual,
        "the documented route table differs from the router"
    );
}
