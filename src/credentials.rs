//! The service account files of the tool (T16).
//!
//! The files live in one directory and are loaded into the maps of the running
//! tool. The dashboard may install a new file or replace one, but only the
//! owner may, the file is written atomically with the private permissions of
//! the process, and nothing ever reads the file back to the dashboard.

use async_trait::async_trait;
use hyper::client::HttpConnector;
use hyper_rustls::HttpsConnector;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;
use yup_oauth2::{authenticator::Authenticator, ServiceAccountAuthenticator, ServiceAccountKey};

/// The scope the live check asks for.
const MESSAGING_SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";

/// The largest service account file the tool accepts.
pub const MAX_CREDENTIAL_BYTES: usize = 8192;

/// The largest request the dashboard sends for one file.
pub const MAX_UPLOAD_BODY_BYTES: usize = 16 * 1024;

/// The timeout of the live check.
const LIVE_CHECK_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Deserialize)]
struct ServiceAccount {
    project_id: String,
}

pub type AuthMap = Arc<RwLock<HashMap<String, Authenticator<HttpsConnector<HttpConnector>>>>>;
pub type ProjectIdMap = Arc<RwLock<HashMap<String, String>>>;

/// The apps that are disabled in this process.
pub type DisabledApps = Arc<RwLock<HashSet<String>>>;

/// Why a credential operation failed.
///
/// The variants never carry a value of the file: the messages are fixed and
/// the details of a live check are logged, not answered.
#[derive(Debug)]
pub enum CredentialError {
    /// The file is not a usable service account.
    Invalid(&'static str),
    /// The replacement belongs to another project.
    ProjectMismatch,
    /// The live check failed. The reason is logged, never answered.
    LiveCheckFailed,
    /// The app already has a credential file.
    Exists,
    /// The app has no credential file.
    NotFound,
    /// The upload of credentials is disabled.
    Disabled,
    /// The file system refused the operation.
    Io,
}

impl std::fmt::Display for CredentialError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CredentialError::Invalid(message) => write!(formatter, "{}", message),
            CredentialError::ProjectMismatch => {
                write!(formatter, "the replacement belongs to another project")
            }
            CredentialError::LiveCheckFailed => {
                write!(formatter, "the service account could not be verified")
            }
            CredentialError::Exists => write!(formatter, "the app already has a credential file"),
            CredentialError::NotFound => write!(formatter, "the app has no credential file"),
            CredentialError::Disabled => write!(formatter, "the app is disabled"),
            CredentialError::Io => write!(formatter, "the credential file could not be written"),
        }
    }
}

/// A service account file that passed every check.
pub struct ValidatedKey {
    pub app_project_id: String,
    pub client_email: String,
    pub private_key_id: String,
    key: ServiceAccountKey,
    raw: Vec<u8>,
}

impl std::fmt::Debug for ValidatedKey {
    /// The private key is never printed.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "ValidatedKey {{ app_project_id: {}, client_email: {}, private_key_id: {} }}",
            self.app_project_id, self.client_email, self.private_key_id
        )
    }
}

/// The public information about an installed credential.
#[derive(Clone, Debug)]
pub struct CredentialMeta {
    pub project_id: String,
    pub client_email: String,
    pub private_key_id: String,
}

/// What an install did. It never holds a key.
#[derive(Clone, Debug)]
pub struct InstallOutcome {
    pub client_email: String,
    pub private_key_id: String,
    pub project_id: String,
    /// The identity of the replaced key, when the app had one.
    pub replaced: Option<(String, String)>,
}

/// The live check of a service account.
#[async_trait]
pub trait CredentialVerifier: Send + Sync {
    /// Checks the key against the live service.
    async fn verify(&self, key: &ServiceAccountKey) -> Result<(), String>;
}

/// The live check: an access token with the messaging scope.
pub struct GoogleVerifier;

#[async_trait]
impl CredentialVerifier for GoogleVerifier {
    async fn verify(&self, key: &ServiceAccountKey) -> Result<(), String> {
        let authenticator = ServiceAccountAuthenticator::builder(key.clone())
            .build()
            .await
            .map_err(|error| format!("the service account could not be used: {}", error))?;
        let token =
            tokio::time::timeout(LIVE_CHECK_TIMEOUT, authenticator.token(&[MESSAGING_SCOPE]))
                .await
                .map_err(|_| "the service account check timed out".to_string())?
                .map_err(|error| format!("the service account was refused: {}", error))?;
        if token.token().is_none() {
            return Err("the service account returned no token".to_string());
        }
        Ok(())
    }
}

/// `^[a-z0-9_-]{1,40}$`: the name of an app, and the name of its file.
///
/// The name never holds a path separator, so the file of an app is always
/// `dir.join(format!("{app}.json"))`.
pub fn app_name_ok(app: &str) -> bool {
    !app.is_empty()
        && app.len() <= 40
        && app.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
}

/// Reads and checks a service account file.
///
/// Every message is fixed: no value of the file ever reaches an error.
pub async fn parse_and_validate(bytes: &[u8]) -> Result<ValidatedKey, CredentialError> {
    if bytes.len() > MAX_CREDENTIAL_BYTES {
        return Err(CredentialError::Invalid(
            "the service account file is larger than 8KB",
        ));
    }

    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| CredentialError::Invalid("the service account file is not valid JSON"))?;
    let Some(object) = value.as_object() else {
        return Err(CredentialError::Invalid(
            "the service account file is not a JSON object",
        ));
    };
    if object.get("type").and_then(Value::as_str) != Some("service_account") {
        return Err(CredentialError::Invalid(
            "the file is not a service account",
        ));
    }

    let field = |name: &str| -> Result<String, CredentialError> {
        object
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|value| !value.trim().is_empty())
            .ok_or(CredentialError::Invalid(
                "the service account file is missing a field",
            ))
    };
    let project_id = field("project_id")?;
    let client_email = field("client_email")?;
    let private_key = field("private_key")?;
    let private_key_id = field("private_key_id")?;

    if project_id.len() > 100
        || !project_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(CredentialError::Invalid("the project id is not valid"));
    }
    if !client_email.ends_with(".gserviceaccount.com") {
        return Err(CredentialError::Invalid("the client email is not valid"));
    }
    if !private_key.starts_with("-----BEGIN PRIVATE KEY-----") {
        return Err(CredentialError::Invalid("the private key is not a PEM key"));
    }

    let key = ServiceAccountKey {
        key_type: Some("service_account".to_string()),
        project_id: Some(project_id.clone()),
        private_key_id: Some(private_key_id.clone()),
        private_key,
        client_email: client_email.clone(),
        client_id: object
            .get("client_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        auth_uri: object
            .get("auth_uri")
            .and_then(Value::as_str)
            .map(str::to_string),
        token_uri: object
            .get("token_uri")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| "https://oauth2.googleapis.com/token".to_string()),
        auth_provider_x509_cert_url: object
            .get("auth_provider_x509_cert_url")
            .and_then(Value::as_str)
            .map(str::to_string),
        client_x509_cert_url: object
            .get("client_x509_cert_url")
            .and_then(Value::as_str)
            .map(str::to_string),
    };

    // The private key must be a real key: the authenticator parses it.
    ServiceAccountAuthenticator::builder(key.clone())
        .build()
        .await
        .map_err(|_| CredentialError::Invalid("the private key could not be parsed"))?;

    Ok(ValidatedKey {
        app_project_id: project_id,
        client_email,
        private_key_id,
        key,
        raw: bytes.to_vec(),
    })
}

/// Loads one credential file into the maps of the tool.
pub async fn load_credential(
    path: &Path,
    auth_map: AuthMap,
    project_map: ProjectIdMap,
    disabled: &DisabledApps,
) {
    let file_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();

    if file_name.is_empty() {
        return;
    }
    // A dot file is a temporary file of an upload, never a credential.
    if file_name.starts_with('.') {
        return;
    }
    if !app_name_ok(&file_name) {
        tracing::warn!(
            "Ignoring a credential file with an invalid app name: {}",
            file_name
        );
        return;
    }
    // A disabled app stays disabled, even when the watcher sees the file.
    if disabled.read().await.contains(&file_name) {
        return;
    }

    // Read the project_id from the JSON file
    let content = match tokio::fs::read_to_string(path).await {
        Ok(c) => c,
        Err(_) => return,
    };
    let sa: ServiceAccount = match serde_json::from_str(&content) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Create Authenticator
    match yup_oauth2::read_service_account_key(path).await {
        Ok(secret) => match ServiceAccountAuthenticator::builder(secret).build().await {
            Ok(auth) => {
                tracing::info!("Loaded credentials for app: {}", file_name);
                auth_map.write().await.insert(file_name.clone(), auth);
                project_map.write().await.insert(file_name, sa.project_id);
            }
            Err(e) => tracing::error!("Failed to build auth for {}: {}", file_name, e),
        },
        Err(e) => tracing::error!("Failed to read SA key for {}: {}", file_name, e),
    }
}

/// Loads every credential file of the directory.
pub async fn scan_credentials_dir(
    dir: &Path,
    auth_map: AuthMap,
    project_map: ProjectIdMap,
    disabled: &DisabledApps,
) {
    if !dir.exists() {
        let _ = tokio::fs::create_dir_all(dir).await;
    }

    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            load_credential(&path, auth_map.clone(), project_map.clone(), disabled).await;
        }
    }
}

/// Watches the directory for new credential files.
pub fn watch_credentials_dir(
    dir: PathBuf,
    auth_map: AuthMap,
    project_map: ProjectIdMap,
    disabled: DisabledApps,
) {
    tokio::task::spawn_blocking(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = match RecommendedWatcher::new(tx, Config::default()) {
            Ok(w) => w,
            Err(e) => {
                tracing::error!("Failed to initialize watcher: {}", e);
                return;
            }
        };

        if let Err(e) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
            tracing::error!("Failed to watch {}: {}", dir.display(), e);
            return;
        }

        tracing::info!("Watching {} for new credentials", dir.display());

        for res in rx {
            match res {
                Ok(Event { kind: _, paths, .. }) => {
                    for path in paths {
                        if path.extension().and_then(|e| e.to_str()) == Some("json") {
                            // Run the async load_credential in a new tokio task
                            let am = auth_map.clone();
                            let pm = project_map.clone();
                            let d = disabled.clone();
                            let p = path.clone();
                            tokio::spawn(async move {
                                // Add a small delay to ensure the file is fully written before reading
                                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                load_credential(&p, am, pm, &d).await;
                            });
                        }
                    }
                }
                Err(e) => tracing::error!("Watch error: {}", e),
            }
        }
    });
}

/// The manager of the credential files of the tool.
///
/// One operation at a time: the lock covers the whole install, so two
/// uploads never write the same file at the same time.
pub struct CredentialManager {
    dir: PathBuf,
    auth_map: AuthMap,
    project_map: ProjectIdMap,
    disabled: DisabledApps,
    verifier: Arc<dyn CredentialVerifier>,
    lock: tokio::sync::Mutex<()>,
    enabled: bool,
}

impl CredentialManager {
    /// Opens the manager on a directory.
    pub fn new(
        dir: PathBuf,
        auth_map: AuthMap,
        project_map: ProjectIdMap,
        disabled: DisabledApps,
        verifier: Arc<dyn CredentialVerifier>,
        enabled: bool,
    ) -> Self {
        // The directory of the files is created when the tool starts.
        let _ = std::fs::create_dir_all(&dir);
        Self {
            dir,
            auth_map,
            project_map,
            disabled,
            verifier,
            lock: tokio::sync::Mutex::new(()),
            enabled,
        }
    }

    /// Is the upload of credentials allowed?
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// The directory of the credential files.
    pub fn directory(&self) -> &Path {
        &self.dir
    }

    /// The path of the file of an app.
    fn path_of(&self, app: &str) -> PathBuf {
        self.dir.join(format!("{}.json", app))
    }

    /// Installs a credential file for an app, or replaces the one it has.
    ///
    /// The order is fixed: the file is checked, then verified against the
    /// live service, then written to a temporary file with the private
    /// permissions, then renamed, then loaded.
    pub async fn install(
        &self,
        app: &str,
        bytes: &[u8],
        replace: bool,
    ) -> Result<InstallOutcome, CredentialError> {
        if !self.enabled {
            return Err(CredentialError::Disabled);
        }
        if !app_name_ok(app) {
            return Err(CredentialError::Invalid("the app name is not valid"));
        }
        let _guard = self.lock.lock().await;

        let key = parse_and_validate(bytes).await?;

        let target = self.path_of(app);
        let exists = target.exists() || self.project_map.read().await.contains_key(app);
        if !replace {
            if exists {
                return Err(CredentialError::Exists);
            }
        } else if !exists {
            return Err(CredentialError::NotFound);
        }

        // A replacement belongs to the same project.
        let previous = if replace {
            let old_project = match self.project_map.read().await.get(app).cloned() {
                Some(project) => Some(project),
                None => self.project_id_of_file(&target).await,
            };
            match old_project {
                Some(old_project) if old_project == key.app_project_id => old_project,
                _ => return Err(CredentialError::ProjectMismatch),
            }
        } else {
            String::new()
        };

        // The live check comes before anything is written.
        self.verifier.verify(&key.key).await.map_err(|reason| {
            tracing::warn!(app = %app, reason = %reason, "the live credential check failed");
            CredentialError::LiveCheckFailed
        })?;

        // The old file is kept in memory, so a failed load can undo the write.
        let old_bytes = tokio::fs::read(&target).await.ok();
        // The identity of the replaced key is read before the new file lands.
        let replaced = if replace {
            self.metadata(app)
                .await
                .map(|meta| (meta.client_email, meta.private_key_id))
        } else {
            None
        };

        self.write_atomic(app, &key.raw).await?;

        // The new file is loaded right away: the watcher is not waited for.
        load_credential(
            &target,
            self.auth_map.clone(),
            self.project_map.clone(),
            &self.disabled,
        )
        .await;
        let loaded = self
            .project_map
            .read()
            .await
            .get(app)
            .cloned()
            .unwrap_or_default();
        if loaded != key.app_project_id {
            // The new file does not work: the old one comes back.
            match old_bytes {
                Some(old) => {
                    let _ = self.write_atomic(app, &old).await;
                    load_credential(
                        &target,
                        self.auth_map.clone(),
                        self.project_map.clone(),
                        &self.disabled,
                    )
                    .await;
                }
                None => {
                    let _ = tokio::fs::remove_file(&target).await;
                    self.auth_map.write().await.remove(app);
                    self.project_map.write().await.remove(app);
                }
            }
            return Err(CredentialError::Io);
        }

        let _ = previous;

        Ok(InstallOutcome {
            client_email: key.client_email,
            private_key_id: key.private_key_id,
            project_id: key.app_project_id,
            replaced,
        })
    }

    /// Writes the file of an app atomically, with the private permissions.
    ///
    /// The temporary file lives in the same directory, does not end with
    /// `.json` and never reaches the watcher.
    async fn write_atomic(&self, app: &str, bytes: &[u8]) -> Result<(), CredentialError> {
        let target = self.path_of(app);
        let temp = self
            .dir
            .join(format!(".{}.upload-{}", app, random_suffix()));

        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            // The private mode is the same `open(2)` flag the standard
            // library sets with `OpenOptionsExt::mode`.
            options.mode(0o600);
        }

        let mut file = match options.open(&temp).await {
            Ok(file) => file,
            Err(error) => {
                tracing::error!(app = %app, error = %error, "the temporary file could not be created");
                return Err(CredentialError::Io);
            }
        };
        if let Err(error) = file.write_all(bytes).await {
            let _ = tokio::fs::remove_file(&temp).await;
            tracing::error!(app = %app, error = %error, "the credential file could not be written");
            return Err(CredentialError::Io);
        }
        if let Err(error) = file.sync_all().await {
            let _ = tokio::fs::remove_file(&temp).await;
            tracing::error!(app = %app, error = %error, "the credential file could not be synced");
            return Err(CredentialError::Io);
        }
        drop(file);

        if let Err(error) = tokio::fs::rename(&temp, &target).await {
            let _ = tokio::fs::remove_file(&temp).await;
            tracing::error!(app = %app, error = %error, "the credential file could not be renamed");
            return Err(CredentialError::Io);
        }
        // The directory entry is durable too.
        if let Ok(directory) = tokio::fs::File::open(&self.dir).await {
            let _ = directory.sync_all().await;
        }
        Ok(())
    }

    /// Stops an app at once: the file stays, the app sends nothing.
    pub async fn disable(&self, app: &str) -> Result<(), CredentialError> {
        if !app_name_ok(app) {
            return Err(CredentialError::Invalid("the app name is not valid"));
        }
        let _guard = self.lock.lock().await;
        self.disabled.write().await.insert(app.to_string());
        self.auth_map.write().await.remove(app);
        self.project_map.write().await.remove(app);
        Ok(())
    }

    /// Starts a disabled app again.
    pub async fn enable(&self, app: &str) -> Result<(), CredentialError> {
        if !app_name_ok(app) {
            return Err(CredentialError::Invalid("the app name is not valid"));
        }
        let _guard = self.lock.lock().await;
        let target = self.path_of(app);
        if !target.exists() {
            return Err(CredentialError::NotFound);
        }
        self.disabled.write().await.remove(app);
        load_credential(
            &target,
            self.auth_map.clone(),
            self.project_map.clone(),
            &self.disabled,
        )
        .await;
        Ok(())
    }

    /// Removes the credential file of an app.
    pub async fn remove(&self, app: &str) -> Result<(), CredentialError> {
        if !app_name_ok(app) {
            return Err(CredentialError::Invalid("the app name is not valid"));
        }
        let _guard = self.lock.lock().await;
        let target = self.path_of(app);
        if target.exists() {
            if let Err(error) = tokio::fs::remove_file(&target).await {
                tracing::error!(app = %app, error = %error, "the credential file could not be removed");
                return Err(CredentialError::Io);
            }
        }
        self.disabled.write().await.remove(app);
        self.auth_map.write().await.remove(app);
        self.project_map.write().await.remove(app);
        Ok(())
    }

    /// The public information about the credential of an app.
    pub async fn metadata(&self, app: &str) -> Option<CredentialMeta> {
        if !app_name_ok(app) {
            return None;
        }
        let target = self.path_of(app);
        let content = tokio::fs::read_to_string(&target).await.ok()?;
        let value: Value = serde_json::from_str(&content).ok()?;
        let object = value.as_object()?;
        let project_id = object.get("project_id")?.as_str()?.to_string();
        let client_email = object.get("client_email")?.as_str()?.to_string();
        let private_key_id = object.get("private_key_id")?.as_str()?.to_string();
        Some(CredentialMeta {
            project_id,
            client_email,
            private_key_id,
        })
    }

    /// Is the app disabled in this process?
    pub async fn is_disabled(&self, app: &str) -> bool {
        self.disabled.read().await.contains(app)
    }

    /// The project id of a credential file, read from the file itself.
    async fn project_id_of_file(&self, path: &Path) -> Option<String> {
        let content = tokio::fs::read_to_string(path).await.ok()?;
        let value: Value = serde_json::from_str(&content).ok()?;
        value
            .as_object()?
            .get("project_id")?
            .as_str()
            .map(str::to_string)
    }
}

/// A random suffix for the temporary file of an upload.
fn random_suffix() -> String {
    use rand::RngCore;

    let mut bytes = [0_u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let mut hex = String::with_capacity(16);
    for byte in bytes {
        let _ = std::fmt::Write::write_fmt(&mut hex, format_args!("{:02x}", byte));
    }
    hex
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    /// A service account file for the dashboard tests.
    pub(crate) fn test_service_account(project_id: &str, key_id: &str) -> Value {
        json!({
            "type": "service_account",
            "project_id": project_id,
            "private_key_id": key_id,
            "private_key": test_pem(),
            "client_email": "service@demo.iam.gserviceaccount.com",
            "client_id": "1234567890",
            "auth_uri": "https://accounts.google.com/o/oauth2/auth",
            "token_uri": "https://oauth2.googleapis.com/token",
            "auth_provider_x509_cert_url": "https://www.googleapis.com/oauth2/v1/certs",
            "client_x509_cert_url": "https://www.googleapis.com/robot/v1/metadata/x509/service"
        })
    }

    /// A verifier that accepts every key.
    struct AcceptingVerifier;

    #[async_trait]
    impl CredentialVerifier for AcceptingVerifier {
        async fn verify(&self, _key: &ServiceAccountKey) -> Result<(), String> {
            Ok(())
        }
    }

    /// A verifier that refuses every key.
    struct RefusingVerifier;

    #[async_trait]
    impl CredentialVerifier for RefusingVerifier {
        async fn verify(&self, _key: &ServiceAccountKey) -> Result<(), String> {
            Err("the service account was refused".to_string())
        }
    }

    /// A private key for the tests, generated once.
    fn test_pem() -> &'static str {
        use std::sync::OnceLock;

        static PEM: OnceLock<String> = OnceLock::new();
        PEM.get_or_init(|| {
            use rsa::pkcs8::{EncodePrivateKey, LineEnding};
            use rsa::RsaPrivateKey;

            let key = RsaPrivateKey::new(&mut rand::thread_rng(), 2048)
                .expect("the test key must generate");
            key.to_pkcs8_pem(LineEnding::LF)
                .expect("the test key must encode")
                .to_string()
        })
    }

    /// A service account file for the tests.
    fn service_account(project_id: &str, key_id: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "type": "service_account",
            "project_id": project_id,
            "private_key_id": key_id,
            "private_key": test_pem(),
            "client_email": "service@demo.iam.gserviceaccount.com",
            "client_id": "1234567890",
            "auth_uri": "https://accounts.google.com/o/oauth2/auth",
            "token_uri": "https://oauth2.googleapis.com/token",
            "auth_provider_x509_cert_url": "https://www.googleapis.com/oauth2/v1/certs",
            "client_x509_cert_url": "https://www.googleapis.com/robot/v1/metadata/x509/service"
        }))
        .expect("the file must serialize")
    }

    /// A fresh directory for one test.
    fn fresh_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let dir = std::env::temp_dir().join(format!(
            "fcm-credentials-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the directory must be created");
        dir
    }

    /// A manager on a fresh directory.
    fn manager(dir: &Path, accepting: bool) -> CredentialManager {
        let verifier: Arc<dyn CredentialVerifier> = if accepting {
            Arc::new(AcceptingVerifier)
        } else {
            Arc::new(RefusingVerifier)
        };
        CredentialManager::new(
            dir.to_path_buf(),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashSet::new())),
            verifier,
            true,
        )
    }

    // ---------- the validation ----------

    #[tokio::test]
    async fn validate_accepts_minimal_valid_key() {
        let bytes = service_account("demo-project", "key-1");
        let key = parse_and_validate(&bytes)
            .await
            .expect("the file must be accepted");
        assert_eq!(key.app_project_id, "demo-project");
        assert_eq!(key.private_key_id, "key-1");
        assert_eq!(key.client_email, "service@demo.iam.gserviceaccount.com");
    }

    #[tokio::test]
    async fn validate_rejects_wrong_type() {
        let bytes = serde_json::to_vec(&json!({
            "type": "authorized_user",
            "project_id": "demo-project",
            "private_key_id": "key-1",
            "private_key": test_pem(),
            "client_email": "service@demo.iam.gserviceaccount.com"
        }))
        .expect("the file must serialize");
        let error = parse_and_validate(&bytes)
            .await
            .expect_err("the type must be refused");
        assert!(matches!(error, CredentialError::Invalid(_)));
    }

    #[tokio::test]
    async fn validate_rejects_missing_fields() {
        for field in [
            "project_id",
            "client_email",
            "private_key",
            "private_key_id",
        ] {
            let mut value =
                serde_json::from_slice::<Value>(&service_account("demo-project", "key-1"))
                    .expect("the file must parse");
            value.as_object_mut().expect("an object").remove(field);
            let bytes = serde_json::to_vec(&value).expect("the file must serialize");
            let error = parse_and_validate(&bytes)
                .await
                .expect_err("the missing field must be refused");
            assert!(
                matches!(error, CredentialError::Invalid(_)),
                "field {}",
                field
            );
        }
    }

    #[tokio::test]
    async fn validate_rejects_corrupt_pem() {
        let mut value = serde_json::from_slice::<Value>(&service_account("demo-project", "key-1"))
            .expect("the file must parse");
        value.as_object_mut().expect("an object").insert(
            "private_key".to_string(),
            json!("-----BEGIN PRIVATE KEY-----\nnot a key\n-----END PRIVATE KEY-----\n"),
        );
        let bytes = serde_json::to_vec(&value).expect("the file must serialize");
        let error = parse_and_validate(&bytes)
            .await
            .expect_err("the broken key must be refused");
        match error {
            CredentialError::Invalid(message) => {
                assert_eq!(message, "the private key could not be parsed")
            }
            other => panic!("expected Invalid, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn validate_rejects_oversized() {
        let mut bytes = service_account("demo-project", "key-1");
        bytes.extend(std::iter::repeat_n(b'x', MAX_CREDENTIAL_BYTES));
        let error = parse_and_validate(&bytes)
            .await
            .expect_err("the big file must be refused");
        assert!(matches!(error, CredentialError::Invalid(_)));
    }

    #[tokio::test]
    async fn validate_rejects_bad_project_id_and_email() {
        let mut value = serde_json::from_slice::<Value>(&service_account("demo-project", "key-1"))
            .expect("the file must parse");
        value
            .as_object_mut()
            .expect("an object")
            .insert("project_id".to_string(), json!("Demo Project"));
        let bytes = serde_json::to_vec(&value).expect("the file must serialize");
        assert!(parse_and_validate(&bytes).await.is_err());

        let mut value = serde_json::from_slice::<Value>(&service_account("demo-project", "key-1"))
            .expect("the file must parse");
        value
            .as_object_mut()
            .expect("an object")
            .insert("client_email".to_string(), json!("service@example.com"));
        let bytes = serde_json::to_vec(&value).expect("the file must serialize");
        assert!(parse_and_validate(&bytes).await.is_err());
    }

    #[tokio::test]
    async fn errors_never_contain_key_material() {
        let mut value = serde_json::from_slice::<Value>(&service_account("demo-project", "key-1"))
            .expect("the file must parse");
        value.as_object_mut().expect("an object").insert(
            "private_key".to_string(),
            json!("-----BEGIN PRIVATE KEY-----\nsecret-material\n-----END PRIVATE KEY-----\n"),
        );
        let bytes = serde_json::to_vec(&value).expect("the file must serialize");
        let error = parse_and_validate(&bytes)
            .await
            .expect_err("the broken key must be refused");
        let text = error.to_string();
        assert!(!text.contains("secret-material"), "the error holds no key");
        assert!(
            !text.contains("BEGIN PRIVATE KEY"),
            "the error holds no key"
        );
    }

    #[test]
    fn app_name_whitelist_rejects_traversal_and_oddities() {
        for app in [
            "../x",
            "a/b",
            ".hidden",
            "CON",
            "with space",
            "",
            &"n".repeat(41),
        ] {
            assert!(!app_name_ok(app), "the name {:?} must be refused", app);
        }
        for app in ["watt", "my_app_1", "a", &"n".repeat(40)] {
            assert!(app_name_ok(app), "the name {:?} must be accepted", app);
        }
    }

    // ---------- the installation ----------

    #[tokio::test]
    async fn install_writes_0600_and_loads_immediately() {
        let dir = fresh_dir();
        let manager = manager(&dir, true);

        let outcome = manager
            .install("watt", &service_account("demo-project", "key-1"), false)
            .await
            .expect("the file must be installed");
        assert_eq!(outcome.project_id, "demo-project");

        let path = dir.join("watt.json");
        assert!(path.exists(), "the file must be there");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("the file must exist")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "the file is private");
        }

        // The file is loaded at once: the tool knows the app.
        let project = manager
            .project_map
            .read()
            .await
            .get("watt")
            .cloned()
            .unwrap_or_default();
        assert_eq!(project, "demo-project");
    }

    #[tokio::test]
    async fn install_temp_file_never_ends_with_json() {
        // The temporary file of an upload is a dot file with a random suffix,
        // so neither the scan nor the watcher can take it.
        // The name of the temporary file is a format string in the source.
        let source = include_str!("credentials.rs");
        let suffix = source
            .find(".upload-")
            .expect("the temporary file name must be in the source");
        let head = &source[..suffix];
        let marker = "format!(\"";
        let start = head
            .rfind(marker)
            .expect("the temporary file name must be a format string");
        let rest = &source[start + marker.len()..];
        let end = rest.find('"').expect("the format must end");
        let format = &rest[..end];
        assert!(
            format.contains(".upload-"),
            "the temporary file has a suffix: {}",
            format
        );
        assert!(
            !format.ends_with(".json"),
            "the temporary file must not be a json file: {}",
            format
        );
        assert!(format.starts_with('.'), "the temporary file is a dot file");
    }

    #[tokio::test]
    async fn install_existing_app_is_exists() {
        let dir = fresh_dir();
        let manager = manager(&dir, true);
        manager
            .install("watt", &service_account("demo-project", "key-1"), false)
            .await
            .expect("the file must be installed");

        let error = manager
            .install("watt", &service_account("demo-project", "key-2"), false)
            .await
            .expect_err("the second file must be refused");
        assert!(matches!(error, CredentialError::Exists));
    }

    #[tokio::test]
    async fn install_live_check_failure_writes_nothing() {
        let dir = fresh_dir();
        let manager = manager(&dir, false);

        let error = manager
            .install("watt", &service_account("demo-project", "key-1"), false)
            .await
            .expect_err("the live check must refuse");
        assert!(matches!(error, CredentialError::LiveCheckFailed));
        assert!(!dir.join("watt.json").exists(), "nothing must be written");
    }

    #[tokio::test]
    async fn replace_requires_same_project_id() {
        let dir = fresh_dir();
        let manager = manager(&dir, true);
        manager
            .install("watt", &service_account("demo-project", "key-1"), false)
            .await
            .expect("the file must be installed");

        let error = manager
            .install("watt", &service_account("other-project", "key-2"), true)
            .await
            .expect_err("another project must be refused");
        assert!(matches!(error, CredentialError::ProjectMismatch));
    }

    #[tokio::test]
    async fn replace_returns_old_and_new_ids_only() {
        let dir = fresh_dir();
        let manager = manager(&dir, true);
        manager
            .install("watt", &service_account("demo-project", "key-1"), false)
            .await
            .expect("the file must be installed");

        let outcome = manager
            .install("watt", &service_account("demo-project", "key-2"), true)
            .await
            .expect("the file must be replaced");
        assert_eq!(outcome.private_key_id, "key-2");
        let (old_email, old_key) = outcome.replaced.expect("the old key must be named");
        assert_eq!(old_key, "key-1");
        assert!(old_email.ends_with(".gserviceaccount.com"));
    }

    #[tokio::test]
    async fn replace_swaps_atomically_and_keeps_old_on_load_failure() {
        let dir = fresh_dir();
        let manager = manager(&dir, true);
        manager
            .install("watt", &service_account("demo-project", "key-1"), false)
            .await
            .expect("the file must be installed");

        // A disabled app is never loaded, so the new file cannot be used: the
        // old one must come back.
        manager
            .disable("watt")
            .await
            .expect("the app must be disabled");
        let error = manager
            .install("watt", &service_account("demo-project", "key-2"), true)
            .await
            .expect_err("the file that cannot be loaded must be refused");
        assert!(matches!(error, CredentialError::Io));

        let content = std::fs::read_to_string(dir.join("watt.json")).expect("the file must exist");
        assert!(
            content.contains("key-1"),
            "the old file must come back: {}",
            content
        );
    }

    #[tokio::test]
    async fn no_temp_files_left_after_failure() {
        let dir = fresh_dir();
        let manager = manager(&dir, false);
        let _ = manager
            .install("watt", &service_account("demo-project", "key-1"), false)
            .await;

        let entries = std::fs::read_dir(&dir).expect("the directory must open");
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.expect("the entry must be readable");
            names.push(entry.file_name().to_string_lossy().to_string());
        }
        assert!(names.is_empty(), "no file must stay: {:?}", names);
    }

    #[tokio::test]
    async fn concurrent_installs_are_serialized() {
        let dir = fresh_dir();
        let manager = Arc::new(manager(&dir, true));

        let first = {
            let manager = manager.clone();
            tokio::spawn(async move {
                manager
                    .install("watt", &service_account("demo-project", "key-1"), false)
                    .await
            })
        };
        let second = {
            let manager = manager.clone();
            tokio::spawn(async move {
                manager
                    .install("watt", &service_account("demo-project", "key-2"), false)
                    .await
            })
        };

        let (first, second) = (
            first.await.expect("the first must run"),
            second.await.expect("the second must run"),
        );
        let results = [first, second];
        let installed = results.iter().filter(|result| result.is_ok()).count();
        let refused = results
            .iter()
            .filter(|result| matches!(result, Err(CredentialError::Exists)))
            .count();
        assert_eq!(installed, 1, "one install must win: {:?}", results);
        assert_eq!(refused, 1, "the other must be refused: {:?}", results);
    }

    #[tokio::test]
    async fn watcher_ignores_dotfiles_and_invalid_names() {
        let dir = fresh_dir();
        let auth_map: AuthMap = Arc::new(RwLock::new(HashMap::new()));
        let project_map: ProjectIdMap = Arc::new(RwLock::new(HashMap::new()));
        let disabled: DisabledApps = Arc::new(RwLock::new(HashSet::new()));

        // A dot file is never a credential.
        let dot = dir.join(".watt.upload-1234.json");
        std::fs::write(&dot, service_account("demo-project", "key-1"))
            .expect("the file must be written");
        // An invalid app name is refused with a warning.
        let odd = dir.join("Bad Name.json");
        std::fs::write(&odd, service_account("demo-project", "key-1"))
            .expect("the file must be written");

        load_credential(&dot, auth_map.clone(), project_map.clone(), &disabled).await;
        load_credential(&odd, auth_map.clone(), project_map.clone(), &disabled).await;

        assert!(
            project_map.read().await.is_empty(),
            "no file must be loaded"
        );
    }

    #[tokio::test]
    async fn scan_still_loads_existing_valid_files() {
        let dir = fresh_dir();
        let auth_map: AuthMap = Arc::new(RwLock::new(HashMap::new()));
        let project_map: ProjectIdMap = Arc::new(RwLock::new(HashMap::new()));
        let disabled: DisabledApps = Arc::new(RwLock::new(HashSet::new()));

        std::fs::write(
            dir.join("watt.json"),
            service_account("demo-project", "key-1"),
        )
        .expect("the file must be written");
        scan_credentials_dir(&dir, auth_map.clone(), project_map.clone(), &disabled).await;

        assert_eq!(
            project_map.read().await.get("watt").cloned(),
            Some("demo-project".to_string())
        );
    }

    // ---------- the disable and the removal ----------

    #[tokio::test]
    async fn disable_stops_app_immediately() {
        let dir = fresh_dir();
        let manager = manager(&dir, true);
        manager
            .install("watt", &service_account("demo-project", "key-1"), false)
            .await
            .expect("the file must be installed");

        manager
            .disable("watt")
            .await
            .expect("the app must be disabled");

        assert!(manager.is_disabled("watt").await);
        assert!(
            !manager.project_map.read().await.contains_key("watt"),
            "the app must be unknown now"
        );
        assert!(dir.join("watt.json").exists(), "the file stays");
    }

    #[tokio::test]
    async fn disabled_app_not_reloaded_by_scan_or_watcher() {
        let dir = fresh_dir();
        let manager = manager(&dir, true);
        manager
            .install("watt", &service_account("demo-project", "key-1"), false)
            .await
            .expect("the file must be installed");
        manager
            .disable("watt")
            .await
            .expect("the app must be disabled");

        let auth_map: AuthMap = Arc::new(RwLock::new(HashMap::new()));
        let project_map: ProjectIdMap = Arc::new(RwLock::new(HashMap::new()));
        let disabled = manager.disabled.clone();
        scan_credentials_dir(&dir, auth_map.clone(), project_map.clone(), &disabled).await;
        load_credential(
            &dir.join("watt.json"),
            auth_map.clone(),
            project_map.clone(),
            &disabled,
        )
        .await;

        assert!(
            project_map.read().await.is_empty(),
            "a disabled app stays disabled"
        );
    }

    #[tokio::test]
    async fn enable_reloads_file() {
        let dir = fresh_dir();
        let manager = manager(&dir, true);
        manager
            .install("watt", &service_account("demo-project", "key-1"), false)
            .await
            .expect("the file must be installed");
        manager
            .disable("watt")
            .await
            .expect("the app must be disabled");

        manager
            .enable("watt")
            .await
            .expect("the app must be enabled");
        assert!(!manager.is_disabled("watt").await);
        assert_eq!(
            manager.project_map.read().await.get("watt").cloned(),
            Some("demo-project".to_string())
        );
    }

    #[tokio::test]
    async fn disabled_state_survives_restart() {
        // The disabled apps live in the database, so a new process knows them.
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("the database must open");
        crate::db::migrate(&pool)
            .await
            .expect("the migration must run");
        crate::db::set_app_disabled(&pool, "watt", "owner@example.com")
            .await
            .expect("the app must be disabled");
        let disabled = crate::db::list_disabled_apps(&pool)
            .await
            .expect("the list must run");
        assert_eq!(disabled, vec!["watt".to_string()]);
    }

    #[tokio::test]
    async fn remove_deletes_file_and_maps() {
        let dir = fresh_dir();
        let manager = manager(&dir, true);
        manager
            .install("watt", &service_account("demo-project", "key-1"), false)
            .await
            .expect("the file must be installed");

        manager
            .remove("watt")
            .await
            .expect("the app must be removed");
        assert!(!dir.join("watt.json").exists(), "the file must be gone");
        assert!(!manager.project_map.read().await.contains_key("watt"));
    }
}
