//! مخزن الإعدادات والقوالب (T1).
//!
//! SQLite كمصدر وحيد للحقيقة، ونسخة في الذاكرة تُحدَّث بعد نجاح كل معاملة.
//! لا تُخزَّن أي أسرار: `secret_env` أسماء متغيرات فقط، ولا يُقرأ أي متغير بيئة هنا.

use crate::config::validate::{validate_all, validate_app_name, validate_template, Issue};
use crate::config::{AppConfig, ConfigDocument, Template};
use serde_json::Value;
use sqlx::{Row, SqlitePool};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::{Arc, RwLock};

/// الأقسام القابلة للحفظ.
pub const SECTIONS: [&str; 6] = [
    "identity",
    "data_source",
    "devices",
    "web_subscribe",
    "campaigns",
    "app_check",
];

/// أخطاء المخزن.
#[derive(Debug)]
pub enum ConfigError {
    /// التحقق فشل: كل الأخطاء مرفقة (fail-closed، لا يُكتب شيء).
    Invalid(Vec<Issue>),
    /// النسخة تغيّرت عند غيرنا.
    Conflict { current_version: i64 },
    /// التطبيق أو القالب غير موجود.
    NotFound,
    /// خطأ قاعدة البيانات.
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

/// خطأ JSON يُعبَّر عنه كـ`Invalid` (المتغير الوحيد المتاح لغير أخطاء القاعدة).
fn json_issue(path: impl Into<String>, message: String) -> ConfigError {
    ConfigError::Invalid(vec![Issue::new(path, format!("[S1] {}", message))])
}

/// لقطة إعدادات تطبيق في الذاكرة.
#[derive(Clone, Debug)]
pub struct AppSnapshot {
    pub config: AppConfig,
    pub templates: BTreeMap<String, Template>,
    pub version: i64,
}

/// صف من سجل الإصدارات.
#[derive(Clone, Debug)]
pub struct VersionRow {
    pub id: i64,
    pub app: String,
    pub section: String,
    pub changed_by: String,
    pub created_at: String,
    pub content: String,
}

/// مخزن الإعدادات: قاعدة البيانات + نسخة في الذاكرة.
pub struct ConfigStore {
    pool: SqlitePool,
    cache: RwLock<HashMap<String, Arc<AppSnapshot>>>,
}

impl ConfigStore {
    /// يحمّل كل التطبيقات. أي سجل تالف أو غير صالح يُتجاهل ويُسجَّل، ولا يُفشل الإقلاع.
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
        })
    }

    /// لقطة التطبيق من الذاكرة.
    pub fn get(&self, app: &str) -> Option<Arc<AppSnapshot>> {
        let cache = self
            .cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.get(app).cloned()
    }

    /// عدد التطبيقات المحمَّلة.
    pub fn app_count(&self) -> usize {
        let cache = self
            .cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.len()
    }

    /// ينشئ تطبيقاً بالإعدادات الافتراضية. التكرار ⇒ `Conflict`.
    pub async fn create_app(&self, app: &str, actor: &str) -> Result<(), ConfigError> {
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

    /// يحفظ قسماً واحداً. `expected_version` غير المطابق ⇒ `Conflict`.
    pub async fn save_section(
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

    /// يحفظ قالباً. رقم الإصدار مستقل لكل قالب ويبدأ من 1.
    pub async fn save_template(
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
        sqlx::query(
            "INSERT OR REPLACE INTO templates (app, key, definition, enabled, version, updated_by) VALUES (?, ?, ?, ?, ?, ?)",
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

    /// يفعّل أو يعطّل قالباً بلا حذفه.
    pub async fn set_template_enabled(
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
            "UPDATE templates SET definition = ?, enabled = ?, version = ?, updated_by = ? WHERE app = ? AND key = ?",
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

    /// يحذف قالباً (المحتوى المحذوف يُحفظ في سجل الإصدارات ليُستعاد).
    pub async fn delete_template(
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

    /// يستعيد محتوى نسخة سابقة كحفظ **جديد** (لا يعدّل التاريخ).
    pub async fn restore(
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
            return self.save_template(app, key, value, actor, None).await;
        }

        let snapshot = match self.get(app) {
            Some(snapshot) => snapshot,
            None => return Err(ConfigError::NotFound),
        };

        if section == "full" {
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
        self.save_section(app, &section, value, actor, snapshot.version)
            .await
    }

    /// يصدّر الإعدادات كـJSON (نفس صيغة الاستيراد).
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

    /// يستورد مستنداً كاملاً: يستبدل كل شيء في معاملة واحدة وينشئ التطبيق إن لم يوجد.
    pub async fn import(&self, app: &str, document: Value, actor: &str) -> Result<(), ConfigError> {
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
        sqlx::query("DELETE FROM templates WHERE app = ?")
            .bind(app)
            .execute(&mut *transaction)
            .await?;
        for (key, template) in &templates {
            let definition = serde_json::to_string(template).map_err(|error| {
                json_issue(
                    format!("templates.{}", key),
                    format!("the template could not be serialized: {}", error),
                )
            })?;
            sqlx::query(
                "INSERT INTO templates (app, key, definition, enabled, version, updated_by) VALUES (?, ?, ?, ?, 1, ?)",
            )
            .bind(app)
            .bind(key)
            .bind(&definition)
            .bind(if template.enabled { 1_i64 } else { 0_i64 })
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

    /// سجل الإصدارات (الأحدث أولاً).
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

    // ---------- مساعدات داخلية ----------

    /// كتابة إعداد: تحديث الجدول الأساسي + صف إصدار + صف سجل، في معاملة واحدة.
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
            "UPDATE app_configs SET config = ?, version = ?, updated_by = ? WHERE app = ? AND version = ?",
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

/// يحلّل قيمة قسم إلى نوعه.
fn parse_section<T: serde::de::DeserializeOwned>(
    section: &str,
    value: Value,
) -> Result<T, ConfigError> {
    serde_json::from_value(value)
        .map_err(|error| json_issue(section, format!("the section could not be read: {}", error)))
}

/// يقرأ قوالب التطبيق. العلم الثاني يدلّ على وجود صف تالف.
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
            "description": "تنبيه",
            "enabled": true,
            "senders": ["admin"],
            "to": { "role": "admin" },
            "title": "عنوان",
            "body": "متن"
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

        // قالب لغير أدمن بلا حدّ سرعة ⇒ [G3]
        let template = serde_json::json!({
            "description": "تنبيه",
            "enabled": true,
            "senders": ["user"],
            "to": { "role": "admin" },
            "title": "عنوان",
            "body": "متن"
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

        // القالب يستعمل {p.order_id} بلا تعريف ⇒ يُرفض.
        let template = serde_json::json!({
            "description": "تنبيه",
            "enabled": true,
            "senders": ["admin"],
            "to": { "role": "admin" },
            "title": "طلب {p.order_id}",
            "body": "متن"
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

        // النسخة 2 هي النسخة الأولى من القسم.
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
}
