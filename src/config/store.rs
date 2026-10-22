//! Application configuration and template store (T1).
//!
//! SQLite is the single source of truth; the in-memory copy is refreshed only
//! after a transaction commits. No secrets live here: `secret_env` holds
//! variable names only and no environment variable is read by this module.
//!
//! Single-instance assumption: `write_lock` serializes writers inside this
//! process only. Two processes sharing one database file would need a
//! database-level lock, which this tool does not support.

use crate::config::validate::{validate_all, validate_app_name, validate_template, Issue};
use crate::config::{AppConfig, ConfigDocument, Template};
use serde_json::Value;
use sqlx::{Row, SqlitePool};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::{Arc, RwLock};

/// The sections that can be saved.
pub const SECTIONS: [&str; 6] = [
    "identity",
    "data_source",
    "devices",
    "web_subscribe",
    "campaigns",
    "app_check",
];

/// Store errors.
#[derive(Debug)]
pub enum ConfigError {
    /// Validation failed: all errors are attached (fail-closed, nothing is written).
    Invalid(Vec<Issue>),
    /// Someone else changed the version.
    Conflict { current_version: i64 },
    /// The app or the template does not exist.
    NotFound,
    /// Database error.
    Db(sqlx::Error),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Invalid(issues) => {
                write!(
                    formatter,
                    "invalid configuration ({} issue(s))",
                    issues.len()
                )?;
                for issue in issues {
                    write!(formatter, "\n{}: {}", issue.path, issue.message)?;
                }
                Ok(())
            }
            ConfigError::Conflict { current_version } => write!(
                formatter,
                "the configuration was modified by someone else (current version {})",
                current_version
            ),
            ConfigError::NotFound => write!(formatter, "not found"),
            ConfigError::Db(error) => write!(formatter, "database error: {}", error),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<sqlx::Error> for ConfigError {
    fn from(error: sqlx::Error) -> Self {
        ConfigError::Db(error)
    }
}

/// A JSON problem reported as `Invalid`, the only variant available for
/// non-database failures.
fn json_issue(path: impl Into<String>, message: String) -> ConfigError {
    ConfigError::Invalid(vec![Issue::new(path, format!("[S1] {}", message))])
}

/// One app configuration as held in memory.
#[derive(Clone, Debug)]
pub struct AppSnapshot {
    pub config: AppConfig,
    pub templates: BTreeMap<String, Template>,
    /// Version of the general configuration only.
    ///
    /// Saving a template does not bump it: template conflicts are detected with
    /// the per-template `expected_version` instead.
    pub version: i64,
}

/// One row of the version history.
#[derive(Clone, Debug)]
pub struct VersionRow {
    pub id: i64,
    pub app: String,
    pub section: String,
    pub changed_by: String,
    pub created_at: String,
    pub content: String,
}

/// The configuration store: SQLite plus the in-memory copy.
pub struct ConfigStore {
    pool: SqlitePool,
    cache: RwLock<HashMap<String, Arc<AppSnapshot>>>,
    /// Held across every write (read snapshot, validate, transaction, cache
    /// refresh) so no writer can commit state validated against a stale
    /// snapshot or overwrite a newer in-memory copy.
    write_lock: tokio::sync::Mutex<()>,
}

impl ConfigStore {
    /// Loads every app. A corrupt or invalid row is skipped and logged; startup continues.
    pub async fn new(pool: SqlitePool) -> Result<Self, ConfigError> {
        let mut cache: HashMap<String, Arc<AppSnapshot>> = HashMap::new();

        let rows = sqlx::query("SELECT app, config, version FROM app_configs ORDER BY app")
            .fetch_all(&pool)
            .await?;

        for row in rows {
            let app: String = row.get("app");
            let raw: String = row.get("config");
            let version: i64 = row.get("version");

            let config = match serde_json::from_str::<AppConfig>(&raw) {
                Ok(config) => config,
                Err(error) => {
                    tracing::error!(
                        "skipping app '{}' with a corrupt config row: {}",
                        app,
                        error
                    );
                    continue;
                }
            };

            let (templates, corrupt_template) = match load_templates(&pool, &app).await {
                Ok(loaded) => loaded,
                Err(error) => {
                    tracing::error!("skipping app '{}': cannot read templates: {}", app, error);
                    continue;
                }
            };
            if corrupt_template {
                tracing::error!(
                    "skipping app '{}' because one of its templates is corrupt",
                    app
                );
                continue;
            }

            let issues = validate_all(&config, &templates);
            if !issues.is_empty() {
                tracing::error!(
                    "skipping app '{}' because its stored configuration is invalid: {:?}",
                    app,
                    issues
                );
                continue;
            }

            cache.insert(
                app.clone(),
                Arc::new(AppSnapshot {
                    config,
                    templates,
                    version,
                }),
            );
        }

        Ok(ConfigStore {
            pool,
            cache: RwLock::new(cache),
            write_lock: tokio::sync::Mutex::new(()),
        })
    }

    /// The in-memory snapshot of an app.
    pub fn get(&self, app: &str) -> Option<Arc<AppSnapshot>> {
        let cache = self
            .cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.get(app).cloned()
    }

    /// Number of loaded apps.
    pub fn app_count(&self) -> usize {
        let cache = self
            .cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.len()
    }

    /// The names of the configured apps, sorted.
    ///
    /// The dashboard needs the configured apps to list them next to the apps
    /// that only have credentials.
    pub fn app_names(&self) -> Vec<String> {
        let cache = self
            .cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut names: Vec<String> = cache.keys().cloned().collect();
        names.sort();
        names
    }

    /// Creates an app with the default settings. A duplicate is `Conflict`.
    pub async fn create_app(&self, app: &str, actor: &str) -> Result<(), ConfigError> {
        let _guard = self.write_lock.lock().await;
        self.create_app_locked(app, actor).await
    }

    async fn create_app_locked(&self, app: &str, actor: &str) -> Result<(), ConfigError> {
        let issues = validate_app_name(app);
        if !issues.is_empty() {
            return Err(ConfigError::Invalid(issues));
        }

        let config = AppConfig::default_firebase();
        let templates = BTreeMap::new();
        let issues = validate_all(&config, &templates);
        if !issues.is_empty() {
            return Err(ConfigError::Invalid(issues));
        }

        let existing: Option<i64> =
            sqlx::query_scalar("SELECT version FROM app_configs WHERE app = ?")
                .bind(app)
                .fetch_optional(&self.pool)
                .await?;
        if let Some(version) = existing {
            return Err(ConfigError::Conflict {
                current_version: version,
            });
        }

        let content = serde_json::to_string(&config)
            .map_err(|error| json_issue("config", format!("cannot be serialized: {}", error)))?;

        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO app_configs (app, config, version, updated_by) VALUES (?, ?, 1, ?)",
        )
        .bind(app)
        .bind(&content)
        .bind(actor)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO config_versions (app, section, content, changed_by) VALUES (?, 'full', ?, ?)",
        )
        .bind(app)
        .bind(&content)
        .bind(actor)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("INSERT INTO audit_log (actor, app, action, detail) VALUES (?, ?, ?, ?)")
            .bind(actor)
            .bind(app)
            .bind("config.create_app")
            .bind("the app was created with the default settings")
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;

        self.update_cache(app, config, templates, 1);
        Ok(())
    }

    /// Saves one section. A mismatched `expected_version` is `Conflict`.
    pub async fn save_section(
        &self,
        app: &str,
        section: &str,
        value: Value,
        actor: &str,
        expected_version: i64,
    ) -> Result<i64, ConfigError> {
        let _guard = self.write_lock.lock().await;
        self.save_section_locked(app, section, value, actor, expected_version)
            .await
    }

    async fn save_section_locked(
        &self,
        app: &str,
        section: &str,
        value: Value,
        actor: &str,
        expected_version: i64,
    ) -> Result<i64, ConfigError> {
        let issues = validate_app_name(app);
        if !issues.is_empty() {
            return Err(ConfigError::Invalid(issues));
        }
        if !SECTIONS.contains(&section) {
            return Err(ConfigError::Invalid(vec![Issue::new(
                section.to_string(),
                format!("[S2] unknown section '{}'", section),
            )]));
        }

        let snapshot = match self.get(app) {
            Some(snapshot) => snapshot,
            None => return Err(ConfigError::NotFound),
        };
        if snapshot.version != expected_version {
            return Err(ConfigError::Conflict {
                current_version: snapshot.version,
            });
        }

        let mut config = snapshot.config.clone();
        match section {
            "identity" => {
                config.identity = parse_section("identity", value.clone())?;
            }
            "data_source" => {
                config.data_source = parse_section("data_source", value.clone())?;
            }
            "devices" => {
                config.devices = parse_section("devices", value.clone())?;
            }
            "web_subscribe" => {
                config.web_subscribe = parse_section("web_subscribe", value.clone())?;
            }
            "campaigns" => {
                config.campaigns = parse_section("campaigns", value.clone())?;
            }
            "app_check" => {
                config.app_check = parse_section("app_check", value.clone())?;
            }
            _ => {
                return Err(ConfigError::Invalid(vec![Issue::new(
                    section.to_string(),
                    format!("[S2] unknown section '{}'", section),
                )]))
            }
        }

        let issues = validate_all(&config, &snapshot.templates);
        if !issues.is_empty() {
            return Err(ConfigError::Invalid(issues));
        }

        let content = serde_json::to_string(&value)
            .map_err(|error| json_issue(section, format!("cannot be serialized: {}", error)))?;

        self.write_config(
            app,
            &config,
            &snapshot.templates,
            section,
            &content,
            actor,
            expected_version,
        )
        .await
    }

    /// Saves one template. Its version number is independent and starts at 1.
    pub async fn save_template(
        &self,
        app: &str,
        key: &str,
        definition: Value,
        actor: &str,
        expected_version: Option<i64>,
    ) -> Result<i64, ConfigError> {
        let _guard = self.write_lock.lock().await;
        self.save_template_locked(app, key, definition, actor, expected_version)
            .await
    }

    async fn save_template_locked(
        &self,
        app: &str,
        key: &str,
        definition: Value,
        actor: &str,
        expected_version: Option<i64>,
    ) -> Result<i64, ConfigError> {
        let issues = validate_app_name(app);
        if !issues.is_empty() {
            return Err(ConfigError::Invalid(issues));
        }

        let snapshot = match self.get(app) {
            Some(snapshot) => snapshot,
            None => return Err(ConfigError::NotFound),
        };

        let template: Template = serde_json::from_value(definition.clone()).map_err(|error| {
            json_issue(
                format!("templates.{}", key),
                format!("the template could not be read: {}", error),
            )
        })?;

        let current_version = self.template_version(app, key).await?;
        if let Some(expected) = expected_version {
            if current_version != expected {
                return Err(ConfigError::Conflict { current_version });
            }
        }

        let mut issues = validate_template(key, &template, &snapshot.config);
        let mut templates = snapshot.templates.clone();
        templates.insert(key.to_string(), template.clone());
        issues.extend(validate_all(&snapshot.config, &templates));
        if !issues.is_empty() {
            return Err(ConfigError::Invalid(issues));
        }

        let content = serde_json::to_string(&template).map_err(|error| {
            json_issue(
                format!("templates.{}", key),
                format!("cannot be serialized: {}", error),
            )
        })?;
        let new_version = current_version + 1;

        let mut transaction = self.pool.begin().await?;
        // Upsert instead of INSERT OR REPLACE: a replace deletes the row, which
        // would reset `last_used_at` (needed to protect used templates later).
        sqlx::query(
            "INSERT INTO templates (app, key, definition, enabled, version, updated_by, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP) \
             ON CONFLICT(app, key) DO UPDATE SET \
             definition = excluded.definition, enabled = excluded.enabled, \
             version = excluded.version, updated_by = excluded.updated_by, \
             updated_at = CURRENT_TIMESTAMP",
        )
        .bind(app)
        .bind(key)
        .bind(&content)
        .bind(if template.enabled { 1_i64 } else { 0_i64 })
        .bind(new_version)
        .bind(actor)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO config_versions (app, section, content, changed_by) VALUES (?, ?, ?, ?)",
        )
        .bind(app)
        .bind(format!("template:{}", key))
        .bind(&content)
        .bind(actor)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("INSERT INTO audit_log (actor, app, action, detail) VALUES (?, ?, ?, ?)")
            .bind(actor)
            .bind(app)
            .bind("config.save_template")
            .bind(format!(
                "template '{}' saved as version {}",
                key, new_version
            ))
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;

        self.update_cache(app, snapshot.config.clone(), templates, snapshot.version);
        Ok(new_version)
    }

    /// Enables or disables a template without deleting it.
    pub async fn set_template_enabled(
        &self,
        app: &str,
        key: &str,
        enabled: bool,
        actor: &str,
    ) -> Result<(), ConfigError> {
        let _guard = self.write_lock.lock().await;
        self.set_template_enabled_locked(app, key, enabled, actor)
            .await
    }

    async fn set_template_enabled_locked(
        &self,
        app: &str,
        key: &str,
        enabled: bool,
        actor: &str,
    ) -> Result<(), ConfigError> {
        let snapshot = match self.get(app) {
            Some(snapshot) => snapshot,
            None => return Err(ConfigError::NotFound),
        };
        let mut templates = snapshot.templates.clone();
        let template = match templates.get(key) {
            Some(template) => template.clone(),
            None => return Err(ConfigError::NotFound),
        };

        let current_version = self.template_version(app, key).await?;
        let new_version = current_version + 1;
        let mut updated = template.clone();
        updated.enabled = enabled;
        let content = serde_json::to_string(&updated).map_err(|error| {
            json_issue(
                format!("templates.{}", key),
                format!("cannot be serialized: {}", error),
            )
        })?;

        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "UPDATE templates SET definition = ?, enabled = ?, version = ?, updated_by = ?, updated_at = CURRENT_TIMESTAMP WHERE app = ? AND key = ?",
        )
        .bind(&content)
        .bind(if enabled { 1_i64 } else { 0_i64 })
        .bind(new_version)
        .bind(actor)
        .bind(app)
        .bind(key)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO config_versions (app, section, content, changed_by) VALUES (?, ?, ?, ?)",
        )
        .bind(app)
        .bind(format!("template:{}", key))
        .bind(&content)
        .bind(actor)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("INSERT INTO audit_log (actor, app, action, detail) VALUES (?, ?, ?, ?)")
            .bind(actor)
            .bind(app)
            .bind("config.set_template_enabled")
            .bind(format!(
                "template '{}' was {}",
                key,
                if enabled { "enabled" } else { "disabled" }
            ))
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;

        templates.insert(key.to_string(), updated);
        self.update_cache(app, snapshot.config.clone(), templates, snapshot.version);
        Ok(())
    }

    /// Deletes a template (its content stays in the version log for restore).
    pub async fn delete_template(
        &self,
        app: &str,
        key: &str,
        actor: &str,
    ) -> Result<(), ConfigError> {
        let _guard = self.write_lock.lock().await;
        self.delete_template_locked(app, key, actor).await
    }

    async fn delete_template_locked(
        &self,
        app: &str,
        key: &str,
        actor: &str,
    ) -> Result<(), ConfigError> {
        let snapshot = match self.get(app) {
            Some(snapshot) => snapshot,
            None => return Err(ConfigError::NotFound),
        };
        let mut templates = snapshot.templates.clone();
        let template = match templates.remove(key) {
            Some(template) => template,
            None => return Err(ConfigError::NotFound),
        };
        let content = serde_json::to_string(&template).map_err(|error| {
            json_issue(
                format!("templates.{}", key),
                format!("cannot be serialized: {}", error),
            )
        })?;

        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM templates WHERE app = ? AND key = ?")
            .bind(app)
            .bind(key)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "INSERT INTO config_versions (app, section, content, changed_by) VALUES (?, ?, ?, ?)",
        )
        .bind(app)
        .bind(format!("template:{}", key))
        .bind(&content)
        .bind(actor)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("INSERT INTO audit_log (actor, app, action, detail) VALUES (?, ?, ?, ?)")
            .bind(actor)
            .bind(app)
            .bind("config.delete_template")
            .bind(format!("template '{}' was deleted", key))
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;

        self.update_cache(app, snapshot.config.clone(), templates, snapshot.version);
        Ok(())
    }

    /// Restores an old version's content as a **new** save (history stays).
    pub async fn restore(
        &self,
        app: &str,
        version_id: i64,
        actor: &str,
    ) -> Result<i64, ConfigError> {
        let _guard = self.write_lock.lock().await;
        self.restore_locked(app, version_id, actor).await
    }

    async fn restore_locked(
        &self,
        app: &str,
        version_id: i64,
        actor: &str,
    ) -> Result<i64, ConfigError> {
        let row =
            sqlx::query("SELECT section, content FROM config_versions WHERE id = ? AND app = ?")
                .bind(version_id)
                .bind(app)
                .fetch_optional(&self.pool)
                .await?;

        let (section, content) = match row {
            Some(row) => (
                row.get::<String, _>("section"),
                row.get::<String, _>("content"),
            ),
            None => return Err(ConfigError::NotFound),
        };

        if let Some(key) = section.strip_prefix("template:") {
            let value: Value = serde_json::from_str(&content).map_err(|error| {
                json_issue(
                    format!("templates.{}", key),
                    format!("the stored version could not be read: {}", error),
                )
            })?;
            return self
                .save_template_locked(app, key, value, actor, None)
                .await;
        }

        let snapshot = match self.get(app) {
            Some(snapshot) => snapshot,
            None => return Err(ConfigError::NotFound),
        };

        if section == "full" {
            // Two shapes of a full version exist: the creation of the app
            // stores the configuration alone, and an import stores the whole
            // document, with the templates. Both are read, so a row written by
            // an older version of the tool still restores.
            if serde_json::from_str::<ConfigDocument>(&content).is_ok() {
                let document: Value = serde_json::from_str(&content).map_err(|error| {
                    json_issue(
                        "config",
                        format!("the stored version could not be read: {}", error),
                    )
                })?;
                self.import_locked(app, document, actor).await?;
                let version = self.get(app).map(|snapshot| snapshot.version).unwrap_or(0);
                return Ok(version);
            }

            let config: AppConfig = serde_json::from_str(&content).map_err(|error| {
                json_issue(
                    "config",
                    format!("the stored version could not be read: {}", error),
                )
            })?;
            let issues = validate_all(&config, &snapshot.templates);
            if !issues.is_empty() {
                return Err(ConfigError::Invalid(issues));
            }
            return self
                .write_config(
                    app,
                    &config,
                    &snapshot.templates,
                    "full",
                    &content,
                    actor,
                    snapshot.version,
                )
                .await;
        }

        if !SECTIONS.contains(&section.as_str()) {
            return Err(ConfigError::Invalid(vec![Issue::new(
                section.clone(),
                format!("[S2] unknown section '{}'", section),
            )]));
        }

        let value: Value = serde_json::from_str(&content).map_err(|error| {
            json_issue(
                section.clone(),
                format!("the stored version could not be read: {}", error),
            )
        })?;
        self.save_section_locked(app, &section, value, actor, snapshot.version)
            .await
    }

    /// Exports the configuration as JSON (the same format that import accepts).
    pub async fn export(&self, app: &str) -> Result<Value, ConfigError> {
        let snapshot = match self.get(app) {
            Some(snapshot) => snapshot,
            None => return Err(ConfigError::NotFound),
        };
        let document = snapshot.config.to_document(&snapshot.templates);
        serde_json::to_value(&document).map_err(|error| {
            json_issue(
                "config",
                format!("the configuration could not be exported: {}", error),
            )
        })
    }

    /// Deletes an app and its templates in one transaction.
    ///
    /// The deleted configuration is kept as a version, so the history of the
    /// app stays readable. An app that does not exist is `NotFound`.
    pub async fn delete_app(&self, app: &str, actor: &str) -> Result<(), ConfigError> {
        let _guard = self.write_lock.lock().await;
        self.delete_app_locked(app, actor).await
    }

    async fn delete_app_locked(&self, app: &str, actor: &str) -> Result<(), ConfigError> {
        let issues = validate_app_name(app);
        if !issues.is_empty() {
            return Err(ConfigError::Invalid(issues));
        }
        let snapshot = match self.get(app) {
            Some(snapshot) => snapshot,
            None => return Err(ConfigError::NotFound),
        };

        // The deleted document is the last version of the app.
        let document = snapshot.config.to_document(&snapshot.templates);
        let content = serde_json::to_string(&document).map_err(|error| {
            json_issue(
                "config",
                format!("the configuration could not be serialized: {}", error),
            )
        })?;

        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM templates WHERE app = ?")
            .bind(app)
            .execute(&mut *transaction)
            .await?;
        let deleted = sqlx::query("DELETE FROM app_configs WHERE app = ?")
            .bind(app)
            .execute(&mut *transaction)
            .await?;
        if deleted.rows_affected() == 0 {
            transaction.rollback().await?;
            return Err(ConfigError::NotFound);
        }
        sqlx::query(
            "INSERT INTO config_versions (app, section, content, changed_by) \
             VALUES (?, 'deleted', ?, ?)",
        )
        .bind(app)
        .bind(&content)
        .bind(actor)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("INSERT INTO audit_log (actor, app, action, detail) VALUES (?, ?, ?, ?)")
            .bind(actor)
            .bind(app)
            .bind("config.delete_app")
            .bind(format!(
                "the app was deleted at version {}",
                snapshot.version
            ))
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;

        // The cache forgets the app.
        let mut cache = self
            .cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.remove(app);
        Ok(())
    }

    /// Imports a whole document: replaces everything in one transaction and
    /// creates the app when it does not exist yet.
    pub async fn import(&self, app: &str, document: Value, actor: &str) -> Result<(), ConfigError> {
        let _guard = self.write_lock.lock().await;
        self.import_locked(app, document, actor).await
    }

    async fn import_locked(
        &self,
        app: &str,
        document: Value,
        actor: &str,
    ) -> Result<(), ConfigError> {
        let issues = validate_app_name(app);
        if !issues.is_empty() {
            return Err(ConfigError::Invalid(issues));
        }

        let parsed: ConfigDocument = serde_json::from_value(document.clone()).map_err(|error| {
            json_issue(
                "config",
                format!("the document could not be read: {}", error),
            )
        })?;
        if parsed.version != 1 {
            return Err(ConfigError::Invalid(vec![Issue::new(
                "version",
                format!("[S3] unsupported document version {}", parsed.version),
            )]));
        }

        let (config, templates) = parsed.into_parts();
        let issues = validate_all(&config, &templates);
        if !issues.is_empty() {
            return Err(ConfigError::Invalid(issues));
        }

        let document_text = serde_json::to_string(&document).map_err(|error| {
            json_issue(
                "config",
                format!("the document could not be serialized: {}", error),
            )
        })?;
        let config_text = serde_json::to_string(&config).map_err(|error| {
            json_issue(
                "config",
                format!("the configuration could not be serialized: {}", error),
            )
        })?;

        let existing: Option<i64> =
            sqlx::query_scalar("SELECT version FROM app_configs WHERE app = ?")
                .bind(app)
                .fetch_optional(&self.pool)
                .await?;
        let version = existing.unwrap_or(0) + 1;

        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT OR REPLACE INTO app_configs (app, config, version, updated_by) VALUES (?, ?, ?, ?)",
        )
        .bind(app)
        .bind(&config_text)
        .bind(version)
        .bind(actor)
        .execute(&mut *transaction)
        .await?;
        // Delete only the templates that vanished from the document and upsert
        // the rest, so an import keeps each surviving template's `last_used_at`.
        let existing: Vec<String> = sqlx::query_scalar("SELECT key FROM templates WHERE app = ?")
            .bind(app)
            .fetch_all(&mut *transaction)
            .await?;
        for key in &existing {
            if !templates.contains_key(key) {
                sqlx::query("DELETE FROM templates WHERE app = ? AND key = ?")
                    .bind(app)
                    .bind(key)
                    .execute(&mut *transaction)
                    .await?;
            }
        }
        for (key, template) in &templates {
            let definition = serde_json::to_string(template).map_err(|error| {
                json_issue(
                    format!("templates.{}", key),
                    format!("the template could not be serialized: {}", error),
                )
            })?;
            // A new template starts at 1; an existing one moves to its previous
            // version + 1, so an import never rewinds a template version. The
            // history of a template removed by the import stays in
            // `config_versions`.
            let current: Option<i64> =
                sqlx::query_scalar("SELECT version FROM templates WHERE app = ? AND key = ?")
                    .bind(app)
                    .bind(key)
                    .fetch_optional(&mut *transaction)
                    .await?;
            let new_version = current.unwrap_or(0) + 1;
            sqlx::query(
                "INSERT INTO templates (app, key, definition, enabled, version, updated_by, updated_at) \
                 VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP) \
                 ON CONFLICT(app, key) DO UPDATE SET \
                 definition = excluded.definition, enabled = excluded.enabled, \
                 version = excluded.version, updated_by = excluded.updated_by, \
                 updated_at = CURRENT_TIMESTAMP",
            )
            .bind(app)
            .bind(key)
            .bind(&definition)
            .bind(if template.enabled { 1_i64 } else { 0_i64 })
            .bind(new_version)
            .bind(actor)
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query(
            "INSERT INTO config_versions (app, section, content, changed_by) VALUES (?, 'full', ?, ?)",
        )
        .bind(app)
        .bind(&document_text)
        .bind(actor)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("INSERT INTO audit_log (actor, app, action, detail) VALUES (?, ?, ?, ?)")
            .bind(actor)
            .bind(app)
            .bind("config.import")
            .bind(format!(
                "the whole configuration was imported as version {}",
                version
            ))
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;

        self.update_cache(app, config, templates, version);
        Ok(())
    }

    /// The version history (newest first).
    pub async fn versions(
        &self,
        app: &str,
        section: Option<&str>,
        limit: i64,
    ) -> Result<Vec<VersionRow>, ConfigError> {
        let limit = limit.clamp(1, 500);
        let rows = match section {
            Some(section) => {
                sqlx::query(
                    "SELECT id, app, section, changed_by, created_at, content FROM config_versions WHERE app = ? AND section = ? ORDER BY id DESC LIMIT ?",
                )
                .bind(app)
                .bind(section)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query(
                    "SELECT id, app, section, changed_by, created_at, content FROM config_versions WHERE app = ? ORDER BY id DESC LIMIT ?",
                )
                .bind(app)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
        };

        Ok(rows
            .into_iter()
            .map(|row| VersionRow {
                id: row.get("id"),
                app: row.get("app"),
                section: row.get("section"),
                changed_by: row.get("changed_by"),
                created_at: row.try_get("created_at").unwrap_or_default(),
                content: row.get("content"),
            })
            .collect())
    }

    // ---------- internal helpers ----------

    /// Writes a configuration: main table + version row + audit row in one transaction.
    #[allow(clippy::too_many_arguments)]
    async fn write_config(
        &self,
        app: &str,
        config: &AppConfig,
        templates: &BTreeMap<String, Template>,
        section: &str,
        content: &str,
        actor: &str,
        expected_version: i64,
    ) -> Result<i64, ConfigError> {
        let config_text = serde_json::to_string(config).map_err(|error| {
            json_issue(
                "config",
                format!("the configuration could not be serialized: {}", error),
            )
        })?;
        let new_version = expected_version + 1;

        let mut transaction = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE app_configs SET config = ?, version = ?, updated_by = ?, updated_at = CURRENT_TIMESTAMP WHERE app = ? AND version = ?",
        )
        .bind(&config_text)
        .bind(new_version)
        .bind(actor)
        .bind(app)
        .bind(expected_version)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() == 0 {
            transaction.rollback().await?;
            let current = self.get(app).map(|snapshot| snapshot.version);
            return Err(ConfigError::Conflict {
                current_version: current.unwrap_or(expected_version),
            });
        }
        sqlx::query(
            "INSERT INTO config_versions (app, section, content, changed_by) VALUES (?, ?, ?, ?)",
        )
        .bind(app)
        .bind(section)
        .bind(content)
        .bind(actor)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("INSERT INTO audit_log (actor, app, action, detail) VALUES (?, ?, ?, ?)")
            .bind(actor)
            .bind(app)
            .bind(format!("config.save_section.{}", section))
            .bind(format!(
                "section '{}' saved as version {}",
                section, new_version
            ))
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;

        self.update_cache(app, config.clone(), templates.clone(), new_version);
        Ok(new_version)
    }

    async fn template_version(&self, app: &str, key: &str) -> Result<i64, ConfigError> {
        let version: Option<i64> =
            sqlx::query_scalar("SELECT version FROM templates WHERE app = ? AND key = ?")
                .bind(app)
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        Ok(version.unwrap_or(0))
    }

    fn update_cache(
        &self,
        app: &str,
        config: AppConfig,
        templates: BTreeMap<String, Template>,
        version: i64,
    ) {
        let mut cache = self
            .cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.insert(
            app.to_string(),
            Arc::new(AppSnapshot {
                config,
                templates,
                version,
            }),
        );
    }
}

/// Parses a section value into its type.
fn parse_section<T: serde::de::DeserializeOwned>(
    section: &str,
    value: Value,
) -> Result<T, ConfigError> {
    serde_json::from_value(value)
        .map_err(|error| json_issue(section, format!("the section could not be read: {}", error)))
}

/// Reads the app's templates. The second flag means a row could not be parsed.
async fn load_templates(
    pool: &SqlitePool,
    app: &str,
) -> Result<(BTreeMap<String, Template>, bool), ConfigError> {
    let rows =
        sqlx::query("SELECT key, definition, enabled FROM templates WHERE app = ? ORDER BY key")
            .bind(app)
            .fetch_all(pool)
            .await?;

    let mut templates = BTreeMap::new();
    let mut corrupt = false;
    for row in rows {
        let key: String = row.get("key");
        let definition: String = row.get("definition");
        let enabled: i64 = row.get("enabled");
        match serde_json::from_str::<Template>(&definition) {
            Ok(mut template) => {
                template.enabled = enabled != 0;
                templates.insert(key, template);
            }
            Err(error) => {
                corrupt = true;
                tracing::error!(
                    "skipping corrupt template '{}' of app '{}': {}",
                    key,
                    app,
                    error
                );
            }
        }
    }
    Ok((templates, corrupt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrate;
    use sqlx::sqlite::SqlitePoolOptions;

    const EXAMPLE: &str = include_str!("../../policies/watt.example.json");

    async fn memory_store() -> (ConfigStore, SqlitePool) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("the in-memory database must open");
        migrate(&pool).await.expect("the migration must run");
        let store = ConfigStore::new(pool.clone())
            .await
            .expect("the store must open");
        (store, pool)
    }

    fn example_document() -> Value {
        serde_json::from_str(EXAMPLE).expect("the example policy must parse")
    }

    fn section_value() -> Value {
        serde_json::json!({
            "provider": { "type": "firebase" },
            "role": { "source": "data_source", "doc": "users/{uid}", "field": "role", "cache_seconds": 60 },
            "admin_roles": ["admin"],
            "role_members": { "collection": "users", "field": "role", "cache_seconds": 300, "max": 50 }
        })
    }

    #[tokio::test]
    async fn create_app_default_is_valid() {
        let (store, pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("creation must work");

        let snapshot = store.get("watt").expect("the app must be in the cache");
        assert_eq!(snapshot.version, 1);
        assert!(snapshot.templates.is_empty());
        let issues = validate_all(&snapshot.config, &snapshot.templates);
        assert!(issues.is_empty(), "issues: {:?}", issues);

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM app_configs WHERE app = 'watt'")
            .fetch_one(&pool)
            .await
            .expect("the row must exist");
        assert_eq!(count, 1);
    }

    /// The id of the newest full version of an app.
    async fn newest_full_version(store: &ConfigStore, app: &str) -> i64 {
        let rows = store
            .versions(app, Some("full"), 50)
            .await
            .expect("the versions must be readable");
        rows.first().expect("a full version must exist").id
    }

    #[tokio::test]
    async fn cli_import_then_restore_full_version_works() {
        let (store, _pool) = memory_store().await;
        let document = example_document();
        store
            .import("watt", document.clone(), "cli")
            .await
            .expect("the import must work");
        let version = newest_full_version(&store, "watt").await;
        let before = store.export("watt").await.expect("the export must work");

        // The configuration changes after the import.
        let snapshot = store.get("watt").expect("the app must exist");
        store
            .save_section(
                "watt",
                "identity",
                section_value(),
                "tester",
                snapshot.version,
            )
            .await
            .expect("the section must be saved");

        // The restore of the version of the import works and brings the
        // document back.
        store
            .restore("watt", version, "tester")
            .await
            .expect("the restore must work");
        let exported = store.export("watt").await.expect("the export must work");
        assert_eq!(exported, before);
    }

    #[tokio::test]
    async fn restore_reads_legacy_full_row_written_by_old_import() {
        let (store, pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("the app must be created");

        // The row of an older version of the tool: the whole document, with
        // the templates, in the full section.
        let document = example_document();
        let content = serde_json::to_string(&document).expect("the document must serialize");
        sqlx::query(
            "INSERT INTO config_versions (app, section, content, changed_by) \
             VALUES (?, 'full', ?, 'cli')",
        )
        .bind("watt")
        .bind(&content)
        .bind("cli")
        .execute(&pool)
        .await
        .expect("the version row must be inserted");
        let id: i64 = sqlx::query_scalar(
            "SELECT id FROM config_versions WHERE app = ? AND section = 'full' ORDER BY id DESC LIMIT 1",
        )
        .bind("watt")
        .fetch_one(&pool)
        .await
        .expect("the version row must exist");

        store
            .restore("watt", id, "tester")
            .await
            .expect("the old row must be readable");

        // The same document, imported the usual way, gives the same result.
        store
            .import("alpha", document.clone(), "cli")
            .await
            .expect("the import must work");
        let exported = store.export("watt").await.expect("the export must work");
        let imported = store.export("alpha").await.expect("the export must work");
        assert_eq!(exported, imported);
    }

    #[tokio::test]
    async fn restore_still_rejects_unknown_fields_in_config_sections() {
        let (store, pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("the app must be created");

        let content = serde_json::json!({
            "provider": { "type": "firebase" },
            "unknown_field": 1
        })
        .to_string();
        sqlx::query(
            "INSERT INTO config_versions (app, section, content, changed_by) \
             VALUES (?, 'identity', ?, 'tester')",
        )
        .bind("watt")
        .bind(&content)
        .bind("tester")
        .execute(&pool)
        .await
        .expect("the version row must be inserted");
        let id: i64 = sqlx::query_scalar(
            "SELECT id FROM config_versions WHERE app = ? AND section = 'identity' ORDER BY id DESC LIMIT 1",
        )
        .bind("watt")
        .fetch_one(&pool)
        .await
        .expect("the version row must exist");

        let error = store
            .restore("watt", id, "tester")
            .await
            .expect_err("an unknown field must be refused");
        match error {
            ConfigError::Invalid(issues) => {
                assert!(
                    issues
                        .iter()
                        .any(|issue| issue.message.contains("unknown field")),
                    "the issue must name the unknown field: {:?}",
                    issues
                );
            }
            other => panic!("the restore must refuse the row, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn export_import_restore_roundtrip_is_identical() {
        let (store, _pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("the app must be created");
        store
            .save_template(
                "watt",
                "news",
                serde_json::json!({
                    "enabled": true,
                    "senders": ["admin"],
                    "to": { "topic": "news" },
                    "title": "News",
                    "body": "Body"
                }),
                "tester",
                None,
            )
            .await
            .expect("the template must be saved");
        let document = store.export("watt").await.expect("the export must work");

        store
            .import("alpha", document.clone(), "cli")
            .await
            .expect("the import must work");
        let version = newest_full_version(&store, "alpha").await;
        let before = store.export("alpha").await.expect("the export must work");

        // The imported app changes after the import.
        store
            .save_template(
                "alpha",
                "news",
                serde_json::json!({
                    "enabled": false,
                    "senders": ["admin"],
                    "to": { "topic": "news" },
                    "title": "Changed",
                    "body": "Body"
                }),
                "tester",
                None,
            )
            .await
            .expect("the template must be saved");

        store
            .restore("alpha", version, "tester")
            .await
            .expect("the restore must work");
        let exported = store.export("alpha").await.expect("the export must work");
        assert_eq!(exported, before);
    }

    #[tokio::test]
    async fn app_names_sorted_and_includes_configured() {
        let (store, _pool) = memory_store().await;
        assert!(store.app_names().is_empty(), "a fresh store has no apps");

        store
            .create_app("watt", "tester")
            .await
            .expect("creation must work");
        store
            .create_app("alpha", "tester")
            .await
            .expect("creation must work");

        assert_eq!(
            store.app_names(),
            vec!["alpha".to_string(), "watt".to_string()]
        );
    }

    #[tokio::test]
    async fn delete_app_removes_config_and_templates() {
        let (store, pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("the app must be created");
        store
            .save_template(
                "watt",
                "news",
                serde_json::json!({
                    "enabled": true,
                    "senders": ["admin"],
                    "to": { "topic": "news" },
                    "title": "News",
                    "body": "Body"
                }),
                "tester",
                None,
            )
            .await
            .expect("the template must be saved");

        store
            .delete_app("watt", "tester")
            .await
            .expect("the app must be deleted");

        assert!(store.get("watt").is_none(), "the app must be gone");
        let configs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM app_configs WHERE app = ?")
            .bind("watt")
            .fetch_one(&pool)
            .await
            .expect("the count must exist");
        assert_eq!(configs, 0);
        let templates: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM templates WHERE app = ?")
            .bind("watt")
            .fetch_one(&pool)
            .await
            .expect("the count must exist");
        assert_eq!(templates, 0);

        // The deletion is a version of the app.
        let versions: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM config_versions WHERE app = ? AND section = 'deleted'",
        )
        .bind("watt")
        .fetch_one(&pool)
        .await
        .expect("the count must exist");
        assert_eq!(versions, 1);
    }

    #[tokio::test]
    async fn delete_app_unknown_is_not_found() {
        let (store, _pool) = memory_store().await;
        let error = store
            .delete_app("ghost", "tester")
            .await
            .expect_err("the app must not be found");
        assert!(matches!(error, ConfigError::NotFound));
    }

    #[tokio::test]
    async fn delete_app_is_atomic() {
        let (store, pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("the app must be created");
        store
            .save_template(
                "watt",
                "news",
                serde_json::json!({
                    "enabled": true,
                    "senders": ["admin"],
                    "to": { "topic": "news" },
                    "title": "News",
                    "body": "Body"
                }),
                "tester",
                None,
            )
            .await
            .expect("the template must be saved");

        store
            .delete_app("watt", "tester")
            .await
            .expect("the app must be deleted");

        // Nothing of the app is left: the templates and the configuration went
        // in one transaction.
        let left: i64 = sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM app_configs WHERE app = ?) \
             + (SELECT COUNT(*) FROM templates WHERE app = ?)",
        )
        .bind("watt")
        .bind("watt")
        .fetch_one(&pool)
        .await
        .expect("the count must exist");
        assert_eq!(left, 0, "the app must be gone completely");
    }

    #[tokio::test]
    async fn create_app_twice_conflicts() {
        let (store, _pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("creation must work");
        let error = store
            .create_app("watt", "tester")
            .await
            .expect_err("the second creation must fail");
        assert!(matches!(error, ConfigError::Conflict { .. }));
    }

    #[tokio::test]
    async fn save_section_bumps_version_and_cache() {
        let (store, pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("creation must work");

        let version = store
            .save_section("watt", "identity", section_value(), "tester", 1)
            .await
            .expect("the section must be saved");
        assert_eq!(version, 2);

        let snapshot = store.get("watt").expect("the app must be cached");
        assert_eq!(snapshot.version, 2);
        assert_eq!(
            snapshot.config.identity.admin_roles,
            vec!["admin".to_string()]
        );

        let stored: i64 = sqlx::query_scalar("SELECT version FROM app_configs WHERE app = 'watt'")
            .fetch_one(&pool)
            .await
            .expect("the row must exist");
        assert_eq!(stored, 2);
    }

    #[tokio::test]
    async fn save_section_stale_version_conflict() {
        let (store, _pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("creation must work");
        store
            .save_section("watt", "identity", section_value(), "tester", 1)
            .await
            .expect("the section must be saved");

        let error = store
            .save_section("watt", "identity", section_value(), "tester", 1)
            .await
            .expect_err("the stale version must fail");
        match error {
            ConfigError::Conflict { current_version } => assert_eq!(current_version, 2),
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[tokio::test]
    async fn save_invalid_writes_nothing() {
        let (store, pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("creation must work");

        let broken = serde_json::json!({
            "provider": { "type": "jwt", "algorithms": ["none"] }
        });
        let error = store
            .save_section("watt", "identity", broken, "tester", 1)
            .await
            .expect_err("the broken section must fail");
        assert!(matches!(error, ConfigError::Invalid(_)));

        let stored: i64 = sqlx::query_scalar("SELECT version FROM app_configs WHERE app = 'watt'")
            .fetch_one(&pool)
            .await
            .expect("the row must exist");
        assert_eq!(stored, 1, "nothing must be written");

        let versions: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM config_versions WHERE app = 'watt'")
                .fetch_one(&pool)
                .await
                .expect("the count must exist");
        assert_eq!(versions, 1, "only the creation version exists");
    }

    #[tokio::test]
    async fn save_template_valid_and_listed() {
        let (store, pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("creation must work");

        let template = serde_json::json!({
            "description": "alert",
            "enabled": true,
            "senders": ["admin"],
            "to": { "role": "admin" },
            "title": "Title",
            "body": "Body"
        });
        let version = store
            .save_template("watt", "admin_alert", template, "tester", None)
            .await
            .expect("the template must be saved");
        assert_eq!(version, 1);

        let snapshot = store.get("watt").expect("the app must be cached");
        assert!(snapshot.templates.contains_key("admin_alert"));

        let listed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM templates WHERE app = 'watt'")
            .fetch_one(&pool)
            .await
            .expect("the count must exist");
        assert_eq!(listed, 1);
    }

    #[tokio::test]
    async fn save_invalid_template_writes_nothing() {
        let (store, pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("creation must work");

        // A non-admin template without a rate limit => [G3]
        let template = serde_json::json!({
            "description": "alert",
            "enabled": true,
            "senders": ["user"],
            "to": { "role": "admin" },
            "title": "Title",
            "body": "Body"
        });
        let error = store
            .save_template("watt", "broken", template, "tester", None)
            .await
            .expect_err("the broken template must fail");
        assert!(matches!(error, ConfigError::Invalid(_)));

        let listed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM templates WHERE app = 'watt'")
            .fetch_one(&pool)
            .await
            .expect("the count must exist");
        assert_eq!(listed, 0, "nothing must be written");
    }

    #[tokio::test]
    async fn template_change_cannot_break_whole_config() {
        let (store, pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("creation must work");

        // The template uses {p.order_id} without declaring it => rejected.
        let template = serde_json::json!({
            "description": "alert",
            "enabled": true,
            "senders": ["admin"],
            "to": { "role": "admin" },
            "title": "Order {p.order_id}",
            "body": "Body"
        });
        let error = store
            .save_template("watt", "broken", template, "tester", None)
            .await
            .expect_err("the broken template must fail");
        match error {
            ConfigError::Invalid(issues) => {
                assert!(
                    issues.iter().any(|issue| issue.message.contains("[T3]")),
                    "issues: {:?}",
                    issues
                );
            }
            other => panic!("unexpected error: {:?}", other),
        }

        let listed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM templates WHERE app = 'watt'")
            .fetch_one(&pool)
            .await
            .expect("the count must exist");
        assert_eq!(listed, 0);
    }

    #[tokio::test]
    async fn restore_returns_exact_content_as_new_version() {
        let (store, _pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("creation must work");

        let version = store
            .save_section("watt", "identity", section_value(), "tester", 1)
            .await
            .expect("the section must be saved");
        assert_eq!(version, 2);

        let other = serde_json::json!({
            "provider": { "type": "firebase" },
            "role": { "source": "data_source", "doc": "accounts/{uid}", "field": "role" },
            "admin_roles": ["admin"]
        });
        store
            .save_section("watt", "identity", other, "tester", 2)
            .await
            .expect("the section must be saved");

        // Version 2 is the first version of the section.
        let restored = store
            .restore("watt", 2, "tester")
            .await
            .expect("the restore must work");
        assert_eq!(restored, 4, "the restore creates a new version");

        let snapshot = store.get("watt").expect("the app must be cached");
        assert_eq!(
            snapshot.config.identity.role,
            crate::config::RoleSource::DataSource {
                doc: "users/{uid}".to_string(),
                field: "role".to_string(),
                cache_seconds: 60
            }
        );
    }

    #[tokio::test]
    async fn restore_other_app_version_not_found() {
        let (store, _pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("creation must work");
        store
            .create_app("other", "tester")
            .await
            .expect("creation must work");

        let error = store
            .restore("watt", 2, "tester")
            .await
            .expect_err("the version of another app must not be found");
        assert!(matches!(error, ConfigError::NotFound));
    }

    #[tokio::test]
    async fn versions_and_audit_rows_written_in_same_transaction() {
        let (store, pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("creation must work");
        store
            .save_section("watt", "identity", section_value(), "tester", 1)
            .await
            .expect("the section must be saved");

        let versions = store
            .versions("watt", None, 10)
            .await
            .expect("the versions must be listed");
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].section, "identity");
        assert_eq!(versions[0].changed_by, "tester");
        assert_eq!(versions[1].section, "full");

        let audits: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_log WHERE app = 'watt'")
            .fetch_one(&pool)
            .await
            .expect("the count must exist");
        assert_eq!(audits, 2, "one audit row per write");

        let filtered = store
            .versions("watt", Some("identity"), 10)
            .await
            .expect("the versions must be listed");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].section, "identity");
    }

    #[tokio::test]
    async fn export_import_round_trip_is_lossless() {
        let (store, _pool) = memory_store().await;
        store
            .import("watt", example_document(), "tester")
            .await
            .expect("the import must work");

        let exported = store.export("watt").await.expect("the export must work");
        let document: ConfigDocument =
            serde_json::from_value(exported.clone()).expect("the export must be a document");

        let (other_store, _pool) = memory_store().await;
        other_store
            .import("copy", exported, "tester")
            .await
            .expect("the import must work");
        let re_exported = other_store
            .export("copy")
            .await
            .expect("the export must work");

        assert_eq!(
            document,
            serde_json::from_value::<ConfigDocument>(re_exported).expect("document")
        );
        assert_eq!(
            document.templates.len(),
            3,
            "the example has three templates"
        );
    }

    #[tokio::test]
    async fn import_example_creates_app() {
        let (store, pool) = memory_store().await;
        store
            .import("watt", example_document(), "tester")
            .await
            .expect("the import must work");

        let snapshot = store.get("watt").expect("the app must be cached");
        assert_eq!(snapshot.templates.len(), 3);
        assert!(snapshot.templates.contains_key("admin_alert"));
        assert!(snapshot.templates.contains_key("order_new"));
        assert!(snapshot.templates.contains_key("order_status"));

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM app_configs WHERE app = 'watt'")
            .fetch_one(&pool)
            .await
            .expect("the count must exist");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn import_invalid_leaves_previous_intact() {
        let (store, pool) = memory_store().await;
        store
            .import("watt", example_document(), "tester")
            .await
            .expect("the import must work");

        let broken = serde_json::json!({
            "version": 1,
            "identity": {
                "provider": { "type": "jwt", "algorithms": ["none"] }
            }
        });
        let error = store
            .import("watt", broken, "tester")
            .await
            .expect_err("the broken import must fail");
        assert!(matches!(error, ConfigError::Invalid(_)));

        let snapshot = store.get("watt").expect("the app must be cached");
        assert_eq!(snapshot.templates.len(), 3, "the templates must stay");
        assert_eq!(snapshot.version, 1, "the version must not change");

        let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM templates WHERE app = 'watt'")
            .fetch_one(&pool)
            .await
            .expect("the count must exist");
        assert_eq!(stored, 3, "the stored templates must stay");
    }

    #[tokio::test]
    async fn store_loads_existing_rows_on_new() {
        let (store, pool) = memory_store().await;
        store
            .import("watt", example_document(), "tester")
            .await
            .expect("the import must work");
        drop(store);

        let reopened = ConfigStore::new(pool.clone())
            .await
            .expect("the store must open");
        let snapshot = reopened.get("watt").expect("the app must be loaded");
        assert_eq!(snapshot.templates.len(), 3);
        assert_eq!(snapshot.version, 1);
        assert_eq!(reopened.app_count(), 1);
    }

    #[tokio::test]
    async fn corrupt_row_is_skipped_not_fatal() {
        let (store, pool) = memory_store().await;
        store
            .import("watt", example_document(), "tester")
            .await
            .expect("the import must work");
        store
            .import("healthy", example_document(), "tester")
            .await
            .expect("the import must work");
        drop(store);

        sqlx::query("UPDATE app_configs SET config = 'not json' WHERE app = 'watt'")
            .execute(&pool)
            .await
            .expect("the row must be broken");
        sqlx::query(
            "INSERT INTO templates (app, key, definition, enabled, version, updated_by) VALUES ('healthy', 'broken', 'not json', 1, 1, 'tester')",
        )
        .execute(&pool)
        .await
        .expect("the row must be inserted");

        let reopened = ConfigStore::new(pool.clone())
            .await
            .expect("a corrupt row must not be fatal");
        assert!(reopened.get("watt").is_none(), "the corrupt app is skipped");
        assert!(
            reopened.get("healthy").is_none(),
            "an app with a corrupt template is skipped"
        );
        assert_eq!(reopened.app_count(), 0);
    }

    fn admin_template(description: &str) -> Value {
        serde_json::json!({
            "description": description,
            "enabled": true,
            "senders": ["admin"],
            "to": { "role": "admin" },
            "title": "Order update",
            "body": "Your order status changed"
        })
    }

    fn other_section_value() -> Value {
        serde_json::json!({
            "provider": { "type": "firebase" },
            "role": { "source": "data_source", "doc": "accounts/{uid}", "field": "role", "cache_seconds": 120 },
            "admin_roles": ["admin"],
            "role_members": { "collection": "accounts", "field": "role", "cache_seconds": 60, "max": 100 }
        })
    }

    #[tokio::test]
    async fn concurrent_template_saves_keep_all_templates_in_cache() {
        let (store, pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("creation must work");
        let store = Arc::new(store);

        let mut handles = Vec::new();
        for index in 0..8 {
            let store = store.clone();
            handles.push(tokio::spawn(async move {
                let key = format!("template_{index}");
                store
                    .save_template("watt", &key, admin_template(&key), "tester", None)
                    .await
                    .expect("the template must be saved");
            }));
        }
        for handle in handles {
            handle.await.expect("the task must finish");
        }

        let snapshot = store.get("watt").expect("the app must be cached");
        assert_eq!(
            snapshot.templates.len(),
            8,
            "no concurrent save may be lost from the cache"
        );
        let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM templates WHERE app = 'watt'")
            .fetch_one(&pool)
            .await
            .expect("the count must exist");
        assert_eq!(stored, 8);
    }

    #[tokio::test]
    async fn concurrent_section_and_template_saves_leave_cache_equal_to_db() {
        let (store, pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("creation must work");
        let store = Arc::new(store);

        let mut handles = Vec::new();
        {
            let store = store.clone();
            handles.push(tokio::spawn(async move {
                let mut version = 1_i64;
                for round in 0..8 {
                    let identity = if round % 2 == 0 {
                        section_value()
                    } else {
                        other_section_value()
                    };
                    version = store
                        .save_section("watt", "identity", identity, "tester", version)
                        .await
                        .expect("the section must be saved");
                }
            }));
        }
        for index in 0..8 {
            let store = store.clone();
            handles.push(tokio::spawn(async move {
                let key = format!("template_{index}");
                store
                    .save_template("watt", &key, admin_template(&key), "tester", None)
                    .await
                    .expect("the template must be saved");
            }));
        }
        for handle in handles {
            handle.await.expect("the task must finish");
        }

        let cached = store.get("watt").expect("the app must be cached");
        let reloaded = ConfigStore::new(pool).await.expect("the store must reopen");
        let persisted = reloaded.get("watt").expect("the app must be reloaded");

        assert_eq!(cached.version, persisted.version);
        assert_eq!(cached.config, persisted.config);
        assert_eq!(cached.templates, persisted.templates);
        assert_eq!(cached.templates.len(), 8);
    }

    #[tokio::test]
    async fn template_save_preserves_last_used_at() {
        let (store, pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("creation must work");
        store
            .save_template("watt", "order_new", admin_template("first"), "tester", None)
            .await
            .expect("the template must be saved");
        sqlx::query(
            "UPDATE templates SET last_used_at = '2026-01-02 03:04:05' WHERE app = 'watt' AND key = 'order_new'",
        )
        .execute(&pool)
        .await
        .expect("the timestamp must be set");

        store
            .save_template(
                "watt",
                "order_new",
                admin_template("second"),
                "tester",
                None,
            )
            .await
            .expect("the template must be saved again");

        let last_used: Option<String> = sqlx::query_scalar(
            "SELECT last_used_at FROM templates WHERE app = 'watt' AND key = 'order_new'",
        )
        .fetch_one(&pool)
        .await
        .expect("the row must exist");
        assert_eq!(
            last_used.as_deref(),
            Some("2026-01-02 03:04:05"),
            "a save must not reset last_used_at"
        );
    }

    #[tokio::test]
    async fn save_section_refreshes_updated_at() {
        let (store, pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("creation must work");
        sqlx::query("UPDATE app_configs SET updated_at = '2000-01-01 00:00:00' WHERE app = 'watt'")
            .execute(&pool)
            .await
            .expect("the timestamp must be set");

        store
            .save_section("watt", "identity", section_value(), "tester", 1)
            .await
            .expect("the section must be saved");

        let updated_at: String =
            sqlx::query_scalar("SELECT updated_at FROM app_configs WHERE app = 'watt'")
                .fetch_one(&pool)
                .await
                .expect("the row must exist");
        assert_ne!(
            updated_at, "2000-01-01 00:00:00",
            "a save must refresh updated_at"
        );
    }

    #[tokio::test]
    async fn import_removes_templates_missing_from_document() {
        let (store, pool) = memory_store().await;
        store
            .import("watt", example_document(), "tester")
            .await
            .expect("the import must work");
        sqlx::query(
            "UPDATE templates SET last_used_at = '2026-01-02 03:04:05' WHERE app = 'watt' AND key = 'order_new'",
        )
        .execute(&pool)
        .await
        .expect("the timestamp must be set");

        let mut document = example_document();
        let templates = document["templates"]
            .as_object_mut()
            .expect("the example must define templates");
        let removed: Vec<String> = templates
            .keys()
            .filter(|key| key.as_str() != "order_new")
            .cloned()
            .collect();
        assert!(
            !removed.is_empty(),
            "the example must define more than one template"
        );
        for key in removed {
            templates.remove(&key);
        }

        store
            .import("watt", document, "tester")
            .await
            .expect("the import must work");

        let keys: Vec<String> =
            sqlx::query_scalar("SELECT key FROM templates WHERE app = 'watt' ORDER BY key")
                .fetch_all(&pool)
                .await
                .expect("the rows must be listed");
        assert_eq!(keys, vec!["order_new".to_string()]);
        let last_used: Option<String> = sqlx::query_scalar(
            "SELECT last_used_at FROM templates WHERE app = 'watt' AND key = 'order_new'",
        )
        .fetch_one(&pool)
        .await
        .expect("the row must exist");
        assert_eq!(
            last_used.as_deref(),
            Some("2026-01-02 03:04:05"),
            "a template kept by the import must keep last_used_at"
        );
    }

    #[tokio::test]
    async fn import_keeps_template_versions_monotonic() {
        let (store, pool) = memory_store().await;
        store
            .import("watt", example_document(), "tester")
            .await
            .expect("the first import must work");

        let version: i64 = sqlx::query_scalar(
            "SELECT version FROM templates WHERE app = 'watt' AND key = 'order_new'",
        )
        .fetch_one(&pool)
        .await
        .expect("the template must exist");
        assert_eq!(version, 1, "a new template starts at version 1");

        store
            .import("watt", example_document(), "tester")
            .await
            .expect("the second import must work");

        let version: i64 = sqlx::query_scalar(
            "SELECT version FROM templates WHERE app = 'watt' AND key = 'order_new'",
        )
        .fetch_one(&pool)
        .await
        .expect("the template must exist");
        assert_eq!(
            version, 2,
            "an import must bump an existing template version"
        );

        let snapshot = store.get("watt").expect("the app must be cached");
        let definition = serde_json::to_value(&snapshot.templates["order_new"])
            .expect("the template must serialize");
        let error = store
            .save_template("watt", "order_new", definition, "tester", Some(1))
            .await
            .expect_err("a stale expected_version must conflict");
        match error {
            ConfigError::Conflict { current_version } => assert_eq!(current_version, 2),
            other => panic!("expected Conflict, got {other:?}"),
        }
    }
}
