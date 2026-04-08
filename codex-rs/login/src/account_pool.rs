use base64::Engine;
use chrono::DateTime;
use chrono::Datelike;
use chrono::Local;
use chrono::TimeZone;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use sqlx::Row;
use std::collections::HashMap;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;

use crate::AuthDotJson;
use crate::load_auth_dot_json;
use crate::save_auth;
use codex_config::CONFIG_TOML_FILE;
use codex_config::types::AuthCredentialsStoreMode;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub struct AccountPoolFile {
    pub current_alias: Option<String>,
    pub accounts: Vec<AccountPoolAccount>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountPoolAccount {
    pub alias: String,
    pub auth_snapshot: AuthDotJson,
    pub config_snapshot: Option<String>,
    pub source: AccountPoolSource,
    pub account_identity: AccountIdentity,
    pub token_health: TokenHealth,
    pub usage_health: UsageHealth,
    pub switch_policy_state: SwitchPolicyState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AccountPoolSource {
    Native,
    CodexAccImport,
    CcSwitchImport,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountIdentity {
    pub account_id: Option<String>,
    pub user_id: Option<String>,
    pub email: Option<String>,
    pub plan_type: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenHealth {
    pub last_refresh_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub refresh_status: Option<String>,
    pub needs_relogin: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageHealth {
    pub five_hour_remaining_percent: Option<u8>,
    pub five_hour_resets_at: Option<i64>,
    pub weekly_remaining_percent: Option<u8>,
    pub weekly_resets_at: Option<i64>,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub quota_exhausted: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchPolicyState {
    pub priority: Option<i32>,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub last_selected_at: Option<DateTime<Utc>>,
    pub last_failure_reason: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountPoolImportResult {
    pub imported_aliases: Vec<String>,
    pub skipped_aliases: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct FileAccountPoolStorage {
    codex_home: PathBuf,
}

impl FileAccountPoolStorage {
    pub fn new(codex_home: PathBuf) -> Self {
        Self { codex_home }
    }

    pub fn load(&self) -> std::io::Result<AccountPoolFile> {
        let account_pool_file = get_account_pool_file(&self.codex_home);
        match self.try_read_account_pool(&account_pool_file) {
            Ok(mut account_pool) => {
                normalize_account_pool(&mut account_pool);
                Ok(account_pool)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Ok(AccountPoolFile::default())
            }
            Err(err) => Err(err),
        }
    }

    pub fn save(&self, account_pool: &AccountPoolFile) -> std::io::Result<()> {
        let account_pool_file = get_account_pool_file(&self.codex_home);
        if let Some(parent) = account_pool_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json_data = serde_json::to_string_pretty(account_pool)?;
        let mut options = OpenOptions::new();
        options.truncate(true).write(true).create(true);
        #[cfg(unix)]
        {
            options.mode(0o600);
        }
        let mut file = options.open(account_pool_file)?;
        file.write_all(json_data.as_bytes())?;
        file.flush()?;
        Ok(())
    }

    fn try_read_account_pool(&self, account_pool_file: &Path) -> std::io::Result<AccountPoolFile> {
        let mut file = File::open(account_pool_file)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        serde_json::from_str(&contents).map_err(std::io::Error::other)
    }
}

#[derive(Clone, Debug)]
pub struct AccountPoolManager {
    codex_home: PathBuf,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
    storage: FileAccountPoolStorage,
}

impl AccountPoolManager {
    pub fn new(codex_home: PathBuf, auth_credentials_store_mode: AuthCredentialsStoreMode) -> Self {
        let storage = FileAccountPoolStorage::new(codex_home.clone());
        Self {
            codex_home,
            auth_credentials_store_mode,
            storage,
        }
    }

    pub fn load(&self) -> std::io::Result<AccountPoolFile> {
        self.storage.load()
    }

    pub fn clear_current_alias(&self) -> std::io::Result<AccountPoolFile> {
        let mut account_pool = match self.storage.load() {
            Ok(account_pool) => account_pool,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(AccountPoolFile::default());
            }
            Err(err) => return Err(err),
        };
        if account_pool.current_alias.is_none() {
            return Ok(account_pool);
        }
        account_pool.current_alias = None;
        self.storage.save(&account_pool)?;
        Ok(account_pool)
    }

    pub fn upsert_account(
        &self,
        account: AccountPoolAccount,
        make_current: bool,
    ) -> std::io::Result<AccountPoolFile> {
        let mut account_pool = self.storage.load()?;
        let alias = account.alias.clone();
        if let Some(existing) = account_pool
            .accounts
            .iter_mut()
            .find(|existing| existing.alias == alias)
        {
            *existing = account;
        } else {
            account_pool.accounts.push(account);
        }
        if make_current {
            account_pool.current_alias = Some(alias);
        }
        self.storage.save(&account_pool)?;
        Ok(account_pool)
    }

    pub fn activate_account(&self, alias: &str) -> std::io::Result<AccountPoolFile> {
        let mut account_pool = self.storage.load()?;
        let account = account_pool
            .accounts
            .iter()
            .find(|account| account.alias == alias)
            .cloned()
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("account alias `{alias}` was not found"),
                )
            })?;
        save_auth(
            &self.codex_home,
            &account.auth_snapshot,
            self.auth_credentials_store_mode,
        )?;
        self.persist_config_snapshot(account.config_snapshot.as_deref())?;
        account_pool.current_alias = Some(account.alias);
        self.storage.save(&account_pool)?;
        Ok(account_pool)
    }

    pub fn sync_active_auth(
        &self,
        alias: Option<&str>,
    ) -> std::io::Result<Option<AccountPoolFile>> {
        let Some(auth_snapshot) =
            load_auth_dot_json(&self.codex_home, self.auth_credentials_store_mode)?
        else {
            return Ok(None);
        };
        let mut account_pool = self.storage.load()?;
        let current_alias = account_pool.current_alias.clone();
        let alias = alias
            .map(str::to_string)
            .or(current_alias)
            .unwrap_or_else(|| derive_account_alias(&auth_snapshot));
        let config_snapshot = self.read_config_snapshot()?;
        let account_identity = account_identity_from_auth(&auth_snapshot);
        let expires_at = auth_snapshot
            .tokens
            .as_ref()
            .and_then(|tokens| parse_access_token_expiration(&tokens.access_token));
        let existing = account_pool
            .accounts
            .iter()
            .find(|account| account.alias == alias)
            .cloned();
        let source = existing
            .as_ref()
            .map(|account| account.source.clone())
            .unwrap_or(AccountPoolSource::Native);
        let usage_health = existing
            .as_ref()
            .map(|account| account.usage_health.clone())
            .unwrap_or_default();
        let switch_policy_state = existing
            .as_ref()
            .map(|account| account.switch_policy_state.clone())
            .unwrap_or_default();
        let mut token_health = existing
            .as_ref()
            .map(|account| account.token_health.clone())
            .unwrap_or_default();
        token_health.last_refresh_at = auth_snapshot.last_refresh;
        token_health.expires_at = expires_at;
        token_health.refresh_status = Some("ok".to_string());
        token_health.needs_relogin = false;
        let synced_account = AccountPoolAccount {
            alias: alias.clone(),
            auth_snapshot,
            config_snapshot,
            source,
            account_identity,
            token_health,
            usage_health,
            switch_policy_state,
        };
        if let Some(existing) = account_pool
            .accounts
            .iter_mut()
            .find(|account| account.alias == alias)
        {
            *existing = synced_account;
        } else {
            account_pool.accounts.push(synced_account);
        }
        account_pool.current_alias = Some(alias);
        account_pool
            .accounts
            .sort_by(|left, right| left.alias.cmp(&right.alias));
        self.storage.save(&account_pool)?;
        Ok(Some(account_pool))
    }

    pub fn update_account_usage_health(
        &self,
        alias: &str,
        five_hour_remaining_percent: Option<u8>,
        five_hour_resets_at: Option<i64>,
        weekly_remaining_percent: Option<u8>,
        weekly_resets_at: Option<i64>,
    ) -> std::io::Result<AccountPoolFile> {
        let mut account_pool = self.storage.load()?;
        let account = account_pool
            .accounts
            .iter_mut()
            .find(|account| account.alias == alias)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("account alias `{alias}` was not found"),
                )
            })?;
        account.usage_health.five_hour_remaining_percent = five_hour_remaining_percent;
        account.usage_health.five_hour_resets_at = five_hour_resets_at;
        account.usage_health.weekly_remaining_percent = weekly_remaining_percent;
        account.usage_health.weekly_resets_at = weekly_resets_at;
        account.usage_health.last_checked_at = Some(Utc::now());
        account.usage_health.quota_exhausted =
            usage_quota_exhausted(five_hour_remaining_percent, weekly_remaining_percent);
        self.storage.save(&account_pool)?;
        Ok(account_pool)
    }

    pub fn mark_account_needs_relogin(&self, alias: &str) -> std::io::Result<AccountPoolFile> {
        let mut account_pool = self.storage.load()?;
        let account = account_pool
            .accounts
            .iter_mut()
            .find(|account| account.alias == alias)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("account alias `{alias}` was not found"),
                )
            })?;
        account.token_health.needs_relogin = true;
        account.token_health.refresh_status = Some("needs_relogin".to_string());
        self.storage.save(&account_pool)?;
        Ok(account_pool)
    }

    pub fn select_best_switch_target(
        &self,
        exclude_alias: Option<&str>,
    ) -> std::io::Result<Option<String>> {
        let now = Utc::now();
        let account_pool = self.storage.load()?;
        let mut candidates = account_pool
            .accounts
            .into_iter()
            .filter(|account| Some(account.alias.as_str()) != exclude_alias)
            .filter(|account| !account.token_health.needs_relogin)
            .filter(|account| !account.usage_health.quota_exhausted)
            .filter(|account| {
                account
                    .switch_policy_state
                    .cooldown_until
                    .is_none_or(|cooldown_until| cooldown_until <= now)
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            plan_sort_priority(&left.account_identity.plan_type)
                .cmp(&plan_sort_priority(&right.account_identity.plan_type))
                .then_with(|| {
                    next_quota_reset_at(&left.usage_health)
                        .cmp(&next_quota_reset_at(&right.usage_health))
                })
                .then_with(|| {
                    right
                        .switch_policy_state
                        .priority
                        .unwrap_or_default()
                        .cmp(&left.switch_policy_state.priority.unwrap_or_default())
                })
                .then_with(|| {
                    right
                        .usage_health
                        .weekly_remaining_percent
                        .unwrap_or_default()
                        .cmp(
                            &left
                                .usage_health
                                .weekly_remaining_percent
                                .unwrap_or_default(),
                        )
                })
                .then_with(|| {
                    right
                        .usage_health
                        .five_hour_remaining_percent
                        .unwrap_or_default()
                        .cmp(
                            &left
                                .usage_health
                                .five_hour_remaining_percent
                                .unwrap_or_default(),
                        )
                })
                .then_with(|| left.alias.cmp(&right.alias))
        });
        Ok(candidates.into_iter().next().map(|account| account.alias))
    }

    pub fn import_codex_acc_store(
        &self,
        store_path: &Path,
    ) -> std::io::Result<AccountPoolImportResult> {
        let store = read_codex_acc_store(store_path)?;
        let mut account_pool = self.storage.load()?;
        let imported_accounts = store
            .profiles
            .into_iter()
            .filter_map(|(alias, profile)| {
                let auth_snapshot = profile.auth.clone()?;
                Some(profile.into_account(alias, auth_snapshot))
            })
            .collect::<Vec<_>>();
        let imported_aliases = imported_accounts
            .iter()
            .map(|account| account.alias.clone())
            .collect::<Vec<_>>();
        let mut imported_aliases = imported_aliases;
        imported_aliases.sort();
        merge_imported_accounts(&mut account_pool.accounts, imported_accounts);
        if let Some(current_alias) = store.current
            && account_pool
                .accounts
                .iter()
                .any(|account| account.alias == current_alias)
        {
            account_pool.current_alias = Some(current_alias);
        }
        account_pool
            .accounts
            .sort_by(|left, right| left.alias.cmp(&right.alias));
        self.storage.save(&account_pool)?;
        Ok(AccountPoolImportResult {
            imported_aliases,
            skipped_aliases: Vec::new(),
        })
    }

    pub async fn import_cc_switch_db(
        &self,
        db_path: &Path,
    ) -> std::io::Result<AccountPoolImportResult> {
        let mut account_pool = self.storage.load()?;
        let imported_accounts = load_cc_switch_accounts(db_path).await?;
        let imported_aliases = imported_accounts
            .iter()
            .map(|account| account.alias.clone())
            .collect::<Vec<_>>();
        let mut imported_aliases = imported_aliases;
        imported_aliases.sort();
        merge_imported_accounts(&mut account_pool.accounts, imported_accounts);
        account_pool
            .accounts
            .sort_by(|left, right| left.alias.cmp(&right.alias));
        self.storage.save(&account_pool)?;
        Ok(AccountPoolImportResult {
            imported_aliases,
            skipped_aliases: Vec::new(),
        })
    }

    fn persist_config_snapshot(&self, config_snapshot: Option<&str>) -> std::io::Result<()> {
        let config_file = self.codex_home.join(CONFIG_TOML_FILE);
        match config_snapshot {
            Some(config_snapshot) => {
                if let Some(parent) = config_file.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(config_file, config_snapshot)
            }
            None => match std::fs::remove_file(config_file) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(err),
            },
        }
    }

    fn read_config_snapshot(&self) -> std::io::Result<Option<String>> {
        let config_file = self.codex_home.join(CONFIG_TOML_FILE);
        match std::fs::read_to_string(config_file) {
            Ok(config_snapshot) => Ok(Some(config_snapshot)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }
}

pub fn get_account_pool_file(codex_home: &Path) -> PathBuf {
    codex_home.join("account-pool.json")
}

fn merge_imported_accounts(
    existing_accounts: &mut Vec<AccountPoolAccount>,
    imported_accounts: Vec<AccountPoolAccount>,
) {
    let mut by_alias = existing_accounts
        .drain(..)
        .map(|account| (account.alias.clone(), account))
        .collect::<HashMap<_, _>>();
    for account in imported_accounts {
        if let Some(email) = account.account_identity.email.as_deref()
            && let Some(existing_alias) = by_alias
                .values()
                .find(|existing| existing.account_identity.email.as_deref() == Some(email))
                .map(|existing| existing.alias.clone())
        {
            let mut merged = account;
            merged.alias = existing_alias.clone();
            by_alias.insert(existing_alias, merged);
            continue;
        }
        by_alias.insert(account.alias.clone(), account);
    }
    *existing_accounts = by_alias.into_values().collect();
}

fn normalize_account_pool(account_pool: &mut AccountPoolFile) {
    for account in &mut account_pool.accounts {
        account.usage_health.quota_exhausted = usage_quota_exhausted(
            account.usage_health.five_hour_remaining_percent,
            account.usage_health.weekly_remaining_percent,
        );
    }
}

fn usage_quota_exhausted(
    five_hour_remaining_percent: Option<u8>,
    weekly_remaining_percent: Option<u8>,
) -> bool {
    five_hour_remaining_percent == Some(0) || weekly_remaining_percent == Some(0)
}

fn next_quota_reset_at(usage_health: &UsageHealth) -> Option<i64> {
    [
        usage_health.five_hour_resets_at,
        usage_health.weekly_resets_at,
    ]
    .into_iter()
    .flatten()
    .min()
}

fn plan_sort_priority(plan_type: &Option<String>) -> u8 {
    match plan_type.as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("free") => 0,
        Some("team") => 1,
        Some("plus") => 2,
        _ => 3,
    }
}

#[derive(Debug, Deserialize)]
struct CodexAccStore {
    current: Option<String>,
    #[serde(default)]
    profiles: HashMap<String, CodexAccProfile>,
}

#[derive(Debug, Deserialize)]
struct CodexAccProfile {
    auth: Option<AuthDotJson>,
    config: Option<String>,
    account: Option<CodexAccAccountIdentity>,
    usage: Option<CodexAccUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexAccAccountIdentity {
    account_id: Option<String>,
    user_id: Option<String>,
    email: Option<String>,
    plan_type: Option<String>,
    plan_label: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexAccUsage {
    five_hour_limit: Option<String>,
    weekly_limit: Option<String>,
}

impl CodexAccProfile {
    fn into_account(self, alias: String, auth_snapshot: AuthDotJson) -> AccountPoolAccount {
        let account_identity = self.account.unwrap_or(CodexAccAccountIdentity {
            account_id: None,
            user_id: None,
            email: None,
            plan_type: None,
            plan_label: None,
        });
        let five_hour_remaining_percent = self
            .usage
            .as_ref()
            .and_then(|usage| parse_remaining_percent(usage.five_hour_limit.as_deref()));
        let five_hour_resets_at = self
            .usage
            .as_ref()
            .and_then(|usage| parse_reset_at(usage.five_hour_limit.as_deref()));
        let weekly_remaining_percent = self
            .usage
            .as_ref()
            .and_then(|usage| parse_remaining_percent(usage.weekly_limit.as_deref()));
        let weekly_resets_at = self
            .usage
            .as_ref()
            .and_then(|usage| parse_reset_at(usage.weekly_limit.as_deref()));
        AccountPoolAccount {
            alias,
            auth_snapshot,
            config_snapshot: self.config.filter(|config| !config.trim().is_empty()),
            source: AccountPoolSource::CodexAccImport,
            account_identity: AccountIdentity {
                account_id: account_identity.account_id,
                user_id: account_identity.user_id,
                email: account_identity.email,
                plan_type: account_identity.plan_type.or(account_identity.plan_label),
            },
            token_health: TokenHealth::default(),
            usage_health: UsageHealth {
                five_hour_remaining_percent,
                five_hour_resets_at,
                weekly_remaining_percent,
                weekly_resets_at,
                last_checked_at: None,
                quota_exhausted: usage_quota_exhausted(
                    five_hour_remaining_percent,
                    weekly_remaining_percent,
                ),
            },
            switch_policy_state: SwitchPolicyState::default(),
        }
    }
}

fn read_codex_acc_store(store_path: &Path) -> std::io::Result<CodexAccStore> {
    let contents = std::fs::read_to_string(store_path)?;
    serde_json::from_str(&contents).map_err(std::io::Error::other)
}

fn parse_remaining_percent(text: Option<&str>) -> Option<u8> {
    let text = text?.trim();
    let percent = text.split('%').next()?.trim().parse::<u8>().ok()?;
    Some(percent.min(100))
}

fn parse_reset_at(text: Option<&str>) -> Option<i64> {
    let text = text?.trim();
    let start = text.find('（')?;
    let end = text[start..].find('）')?;
    let reset_text = text[start + '（'.len_utf8()..start + end].trim();
    let reset_text = reset_text.strip_suffix("恢复").unwrap_or(reset_text).trim();
    if let Some(time_text) = reset_text.strip_prefix("今日") {
        let time_text = time_text.trim();
        let today = Local::now().date_naive();
        let local_time = chrono::NaiveTime::parse_from_str(time_text, "%H:%M").ok()?;
        let local_dt = today.and_time(local_time);
        return Local
            .from_local_datetime(&local_dt)
            .single()
            .map(|dt| dt.timestamp());
    }
    let current_year = Local::now().year();
    let local_dt = chrono::NaiveDateTime::parse_from_str(
        &format!("{current_year}-{reset_text}"),
        "%Y-%m-%d %H:%M",
    )
    .ok()?;
    Local
        .from_local_datetime(&local_dt)
        .single()
        .map(|dt| dt.timestamp())
}

fn derive_account_alias(auth_snapshot: &AuthDotJson) -> String {
    auth_snapshot
        .tokens
        .as_ref()
        .and_then(|tokens| {
            tokens
                .id_token
                .email
                .clone()
                .or_else(|| tokens.account_id.clone())
                .or_else(|| tokens.id_token.chatgpt_account_id.clone())
        })
        .or_else(|| {
            auth_snapshot.openai_api_key.as_ref().map(|api_key| {
                let suffix = api_key
                    .chars()
                    .rev()
                    .take(4)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>();
                format!("api-key-{suffix}")
            })
        })
        .unwrap_or_else(|| "account".to_string())
}

fn account_identity_from_auth(auth_snapshot: &AuthDotJson) -> AccountIdentity {
    let Some(tokens) = auth_snapshot.tokens.as_ref() else {
        return AccountIdentity::default();
    };
    AccountIdentity {
        account_id: tokens
            .account_id
            .clone()
            .or_else(|| tokens.id_token.chatgpt_account_id.clone()),
        user_id: tokens.id_token.chatgpt_user_id.clone(),
        email: tokens.id_token.email.clone(),
        plan_type: tokens.id_token.get_chatgpt_plan_type_raw(),
    }
}

fn parse_access_token_expiration(access_token: &str) -> Option<DateTime<Utc>> {
    let mut parts = access_token.split('.');
    let (_header_b64, payload_b64, _sig_b64) = match (parts.next(), parts.next(), parts.next()) {
        (Some(header), Some(payload), Some(signature))
            if !header.is_empty() && !payload.is_empty() && !signature.is_empty() =>
        {
            (header, payload, signature)
        }
        _ => return None,
    };
    let payload_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .ok()?;
    let claims = serde_json::from_slice::<JwtExpirationClaims>(&payload_bytes).ok()?;
    let expiration = claims.exp?;
    DateTime::from_timestamp(expiration, 0)
}

#[derive(Deserialize)]
struct JwtExpirationClaims {
    #[serde(default)]
    exp: Option<i64>,
}

async fn load_cc_switch_accounts(db_path: &Path) -> std::io::Result<Vec<AccountPoolAccount>> {
    if !db_path.exists() {
        return Ok(Vec::new());
    }
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(db_path)
        .read_only(true);
    let pool = sqlx::SqlitePool::connect_with(options)
        .await
        .map_err(std::io::Error::other)?;
    let rows = sqlx::query(
        "SELECT id, name, settings_config
         FROM providers
         WHERE app_type = 'codex'
         ORDER BY sort_index ASC, created_at ASC, name ASC, id ASC",
    )
    .fetch_all(&pool)
    .await
    .map_err(std::io::Error::other)?;
    pool.close().await;
    rows.into_iter()
        .filter_map(|row| parse_cc_switch_row(&row).transpose())
        .collect()
}

#[derive(Debug, Deserialize)]
struct CcSwitchSettingsConfig {
    auth: Option<AuthDotJson>,
    config: Option<String>,
}

fn parse_cc_switch_row(
    row: &sqlx::sqlite::SqliteRow,
) -> std::io::Result<Option<AccountPoolAccount>> {
    let name = row
        .try_get::<String, _>("name")
        .map_err(std::io::Error::other)?;
    let settings_config = row
        .try_get::<String, _>("settings_config")
        .map_err(std::io::Error::other)?;
    let settings = serde_json::from_str::<CcSwitchSettingsConfig>(&settings_config)
        .map_err(std::io::Error::other)?;
    let Some(auth_snapshot) = settings.auth else {
        return Ok(None);
    };
    Ok(Some(AccountPoolAccount {
        alias: name,
        auth_snapshot,
        config_snapshot: settings.config.filter(|config| !config.trim().is_empty()),
        source: AccountPoolSource::CcSwitchImport,
        account_identity: AccountIdentity::default(),
        token_health: TokenHealth::default(),
        usage_health: UsageHealth::default(),
        switch_policy_state: SwitchPolicyState::default(),
    }))
}

#[cfg(test)]
#[path = "account_pool_tests.rs"]
mod tests;
