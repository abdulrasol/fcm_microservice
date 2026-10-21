#![allow(dead_code)]
// يُزال عند ربط المخزن في T7/T8

//! نماذج بيانات إعدادات التطبيقات والقوالب (T1).
//!
//! مطابقة للمثال في `docs/plans/secure-sending-plan.md` القسم 3.2 حرفياً.
//! كل النماذج `deny_unknown_fields`: أي حقل غير معروف يُرفض عند التحليل.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub mod placeholder;
pub mod store;
pub mod validate;

// ---------- قيم افتراضية ----------

fn default_true() -> bool {
    true
}

fn default_max_per_uid() -> u32 {
    5
}

fn default_cache_seconds() -> u64 {
    60
}

fn default_role_members_cache_seconds() -> u64 {
    300
}

fn default_role_members_max() -> u32 {
    50
}

fn default_uid_claim() -> String {
    "sub".to_string()
}

fn default_max_age_seconds() -> u64 {
    3600
}

fn default_timeout_ms() -> u64 {
    3000
}

fn default_max_topics() -> u32 {
    5
}

fn default_title_max() -> u32 {
    60
}

fn default_body_max() -> u32 {
    240
}

fn default_per_entity_per_day() -> u32 {
    1
}

fn default_per_entity_per_week() -> u32 {
    5
}

// ---------- إعدادات التطبيق ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub identity: Identity,
    pub data_source: DataSource,
    #[serde(default)]
    pub devices: Devices,
    #[serde(default)]
    pub web_subscribe: WebSubscribe,
    #[serde(default)]
    pub campaigns: Campaigns,
    #[serde(default)]
    pub app_check: AppCheck,
}

impl AppConfig {
    /// الإعداد الافتراضي لتطبيق Firebase: هوية من `users/{uid}.role`،
    /// مصدر بيانات Firestore، بلا قوالب ولا حملات.
    pub fn default_firebase() -> Self {
        AppConfig {
            identity: Identity {
                provider: IdentityProvider::Firebase,
                role: RoleSource::DataSource {
                    doc: "users/{uid}".to_string(),
                    field: "role".to_string(),
                    cache_seconds: default_cache_seconds(),
                },
                admin_roles: vec!["admin".to_string()],
                role_members: None,
            },
            data_source: DataSource::Firestore,
            devices: Devices {
                max_per_uid: default_max_per_uid(),
            },
            web_subscribe: WebSubscribe {
                anonymous: Vec::new(),
                signed_in: Vec::new(),
            },
            campaigns: Campaigns::default(),
            app_check: AppCheck {
                mode: AppCheckMode::Off,
                project_number: None,
            },
        }
    }

    /// يبني مستند التصدير (الصيغة نفسها المستوردة) من الإعداد والقوالب.
    pub fn to_document(&self, templates: &BTreeMap<String, Template>) -> ConfigDocument {
        ConfigDocument {
            version: 1,
            identity: self.identity.clone(),
            data_source: self.data_source.clone(),
            devices: self.devices.clone(),
            web_subscribe: self.web_subscribe.clone(),
            templates: templates.clone(),
            campaigns: self.campaigns.clone(),
            app_check: self.app_check.clone(),
        }
    }
}

// ---------- الهوية ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub provider: IdentityProvider,
    pub role: RoleSource,
    #[serde(default)]
    pub admin_roles: Vec<String>,
    #[serde(default)]
    pub role_members: Option<RoleMembers>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum IdentityProvider {
    Firebase,
    Jwt {
        algorithms: Vec<JwtAlgorithm>,
        #[serde(default)]
        jwks_url: Option<String>,
        #[serde(default)]
        hs256_secret_env: Option<String>,
        issuer: String,
        audience: String,
        #[serde(default = "default_uid_claim")]
        uid_claim: String,
        #[serde(default)]
        name_claim: Option<String>,
        #[serde(default = "default_max_age_seconds")]
        max_age_seconds: u64,
    },
}

/// خوارزميات JWT المدعومة فقط. `none` غير موجود أصلاً ⇒ يفشل التحليل.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum JwtAlgorithm {
    RS256,
    ES256,
    HS256,
}

impl JwtAlgorithm {
    pub fn is_hs(self: &JwtAlgorithm) -> bool {
        matches!(self, JwtAlgorithm::HS256)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoleSource {
    Claim {
        claim: String,
    },
    DataSource {
        doc: String,
        field: String,
        #[serde(default = "default_cache_seconds")]
        cache_seconds: u64,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RoleMembers {
    pub collection: String,
    pub field: String,
    #[serde(default = "default_role_members_cache_seconds")]
    pub cache_seconds: u64,
    #[serde(default = "default_role_members_max")]
    pub max: u32,
}

// ---------- مصدر البيانات ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DataSource {
    Firestore,
    Http {
        base_url: String,
        secret_env: String,
        #[serde(default = "default_timeout_ms")]
        timeout_ms: u64,
    },
    None,
}

// ---------- أقسام بسيطة ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Devices {
    #[serde(default = "default_max_per_uid")]
    pub max_per_uid: u32,
}

impl Default for Devices {
    fn default() -> Self {
        Devices {
            max_per_uid: default_max_per_uid(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WebSubscribe {
    #[serde(default)]
    pub anonymous: Vec<String>,
    #[serde(default)]
    pub signed_in: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AppCheck {
    pub mode: AppCheckMode,
    #[serde(default)]
    pub project_number: Option<String>,
}

impl Default for AppCheck {
    fn default() -> Self {
        AppCheck {
            mode: AppCheckMode::Off,
            project_number: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AppCheckMode {
    Off,
    Monitor,
    Enforce,
}

// ---------- القوالب ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Template {
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub senders: Vec<String>,
    #[serde(default)]
    pub params: BTreeMap<String, Param>,
    #[serde(default)]
    pub context: Option<Context>,
    #[serde(default)]
    pub vars: BTreeMap<String, Var>,
    pub to: Target,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub data: BTreeMap<String, String>,
    #[serde(default)]
    pub dedupe: Option<Dedupe>,
    #[serde(default)]
    pub rate: Option<Rate>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Param {
    Text {
        max: u32,
    },
    Pattern {
        pattern: String,
        #[serde(default)]
        max: Option<u32>,
    },
    Enum {
        values: Vec<String>,
    },
    Int {
        #[serde(default)]
        min: Option<i64>,
        #[serde(default)]
        max: Option<i64>,
    },
}

impl Param {
    pub fn is_text(&self) -> bool {
        matches!(self, Param::Text { .. })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub doc: String,
    #[serde(default)]
    pub require: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Var {
    pub from: String,
    #[serde(default)]
    pub map: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Uid(String),
    Role(String),
    Topic(String),
    Condition(String),
}

impl Target {
    /// اسم الحقل الوحيد داخل `to` (وهي مبنية كـenum فلا تحتمل حقلين).
    pub fn kind(&self) -> &'static str {
        match self {
            Target::Uid(_) => "uid",
            Target::Role(_) => "role",
            Target::Topic(_) => "topic",
            Target::Condition(_) => "condition",
        }
    }

    pub fn value(&self) -> &str {
        match self {
            Target::Uid(v) | Target::Role(v) | Target::Topic(v) | Target::Condition(v) => v,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Dedupe {
    pub key: String,
    pub seconds: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Rate {
    pub per_uid_per_hour: u32,
}

// ---------- الحملات ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Campaigns {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub senders: Vec<String>,
    #[serde(default)]
    pub entity: Entity,
    #[serde(default)]
    pub audience: Audience,
    #[serde(default)]
    pub content: Content,
    #[serde(default)]
    pub quota: Quota,
    #[serde(default)]
    pub review: Review,
}

impl Default for Campaigns {
    fn default() -> Self {
        Campaigns {
            enabled: false,
            senders: Vec::new(),
            entity: Entity {
                param: "entity_id".to_string(),
                doc: "entities/{p.entity_id}".to_string(),
                require: BTreeMap::new(),
                name_field: "name".to_string(),
            },
            audience: Audience {
                allowed_topics: Vec::new(),
                required_topic: "promos".to_string(),
                max_topics: default_max_topics(),
            },
            content: Content {
                title_prefix: String::new(),
                title_max: default_title_max(),
                body_max: default_body_max(),
                forbid_links: true,
                image_hosts: Vec::new(),
                actions: BTreeMap::new(),
            },
            quota: Quota {
                per_entity_per_day: default_per_entity_per_day(),
                per_entity_per_week: default_per_entity_per_week(),
            },
            review: Review {
                mode: ReviewMode::None,
                n: None,
            },
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    pub param: String,
    pub doc: String,
    #[serde(default)]
    pub require: BTreeMap<String, String>,
    pub name_field: String,
}

impl Default for Entity {
    fn default() -> Self {
        Entity {
            param: "entity_id".to_string(),
            doc: "entities/{p.entity_id}".to_string(),
            require: BTreeMap::new(),
            name_field: "name".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Audience {
    #[serde(default)]
    pub allowed_topics: Vec<String>,
    pub required_topic: String,
    #[serde(default = "default_max_topics")]
    pub max_topics: u32,
}

impl Default for Audience {
    fn default() -> Self {
        Audience {
            allowed_topics: Vec::new(),
            required_topic: "promos".to_string(),
            max_topics: default_max_topics(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Content {
    #[serde(default)]
    pub title_prefix: String,
    #[serde(default = "default_title_max")]
    pub title_max: u32,
    #[serde(default = "default_body_max")]
    pub body_max: u32,
    #[serde(default)]
    pub forbid_links: bool,
    #[serde(default)]
    pub image_hosts: Vec<String>,
    #[serde(default)]
    pub actions: BTreeMap<String, Action>,
}

impl Default for Content {
    fn default() -> Self {
        Content {
            title_prefix: String::new(),
            title_max: default_title_max(),
            body_max: default_body_max(),
            forbid_links: true,
            image_hosts: Vec::new(),
            actions: BTreeMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Action {
    #[serde(default)]
    pub params: BTreeMap<String, Param>,
    #[serde(default)]
    pub check: Option<Check>,
    #[serde(default)]
    pub data: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub doc: String,
    #[serde(default)]
    pub require: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Quota {
    #[serde(default = "default_per_entity_per_day")]
    pub per_entity_per_day: u32,
    #[serde(default = "default_per_entity_per_week")]
    pub per_entity_per_week: u32,
}

impl Default for Quota {
    fn default() -> Self {
        Quota {
            per_entity_per_day: default_per_entity_per_day(),
            per_entity_per_week: default_per_entity_per_week(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub mode: ReviewMode,
    #[serde(default)]
    pub n: Option<u32>,
}

impl Default for Review {
    fn default() -> Self {
        Review {
            mode: ReviewMode::None,
            n: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewMode {
    None,
    Always,
    FirstN,
}

// ---------- مستند التصدير/الاستيراد ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConfigDocument {
    pub version: u32,
    pub identity: Identity,
    pub data_source: DataSource,
    #[serde(default)]
    pub devices: Devices,
    #[serde(default)]
    pub web_subscribe: WebSubscribe,
    #[serde(default)]
    pub templates: BTreeMap<String, Template>,
    #[serde(default)]
    pub campaigns: Campaigns,
    #[serde(default)]
    pub app_check: AppCheck,
}

impl ConfigDocument {
    /// يحوّل المستند إلى إعداد + قوالب.
    pub fn into_parts(self) -> (AppConfig, BTreeMap<String, Template>) {
        let config = AppConfig {
            identity: self.identity,
            data_source: self.data_source,
            devices: self.devices,
            web_subscribe: self.web_subscribe,
            campaigns: self.campaigns,
            app_check: self.app_check,
        };
        (config, self.templates)
    }
}
