use super::*;
use crate::load_auth_dot_json;
use crate::save_auth;
use codex_app_server_protocol::AuthMode;
use codex_config::CONFIG_TOML_FILE;
use codex_config::types::AuthCredentialsStoreMode;
use pretty_assertions::assert_eq;
use serde::Serialize;
use tempfile::tempdir;

fn sample_auth() -> AuthDotJson {
    AuthDotJson {
        auth_mode: Some(AuthMode::ApiKey),
        openai_api_key: Some("sk-test".to_string()),
        tokens: None,
        last_refresh: None,
    }
}

fn sample_account(alias: &str) -> AccountPoolAccount {
    AccountPoolAccount {
        alias: alias.to_string(),
        auth_snapshot: sample_auth(),
        config_snapshot: Some("model = \"gpt-5\"".to_string()),
        source: AccountPoolSource::Native,
        account_identity: AccountIdentity {
            account_id: Some(format!("acct-{alias}")),
            user_id: Some(format!("user-{alias}")),
            email: Some(format!("{alias}@example.com")),
            plan_type: Some("plus".to_string()),
        },
        token_health: TokenHealth {
            last_refresh_at: Some(Utc::now()),
            expires_at: None,
            refresh_status: Some("ok".to_string()),
            needs_relogin: false,
        },
        usage_health: UsageHealth {
            five_hour_remaining_percent: Some(80),
            five_hour_resets_at: Some(1_744_204_860),
            weekly_remaining_percent: Some(70),
            weekly_resets_at: Some(1_744_808_860),
            last_checked_at: Some(Utc::now()),
            quota_exhausted: false,
        },
        switch_policy_state: SwitchPolicyState {
            priority: Some(10),
            cooldown_until: None,
            last_selected_at: None,
            last_failure_reason: None,
        },
    }
}

fn sample_chatgpt_auth() -> anyhow::Result<AuthDotJson> {
    Ok(AuthDotJson {
        auth_mode: Some(AuthMode::Chatgpt),
        openai_api_key: None,
        tokens: Some(crate::token_data::TokenData {
            id_token: crate::token_data::IdTokenInfo {
                email: Some("native@example.com".to_string()),
                chatgpt_plan_type: Some(codex_protocol::auth::PlanType::Known(
                    codex_protocol::auth::KnownPlan::Pro,
                )),
                chatgpt_user_id: Some("user-native".to_string()),
                chatgpt_account_id: Some("org-native".to_string()),
                raw_jwt: fake_jwt("native@example.com", "pro", "user-native", "org-native")?,
            },
            access_token: fake_jwt("native@example.com", "pro", "user-native", "org-native")?,
            refresh_token: "refresh-token".to_string(),
            account_id: Some("org-native".to_string()),
        }),
        last_refresh: Some(Utc::now()),
    })
}

fn fake_jwt(
    email: &str,
    plan_type: &str,
    user_id: &str,
    account_id: &str,
) -> anyhow::Result<String> {
    #[derive(Serialize)]
    struct Header {
        alg: &'static str,
        typ: &'static str,
    }

    let header = Header {
        alg: "none",
        typ: "JWT",
    };
    let payload = serde_json::json!({
        "exp": 4_102_444_800_i64,
        "email": email,
        "https://api.openai.com/auth": {
            "chatgpt_plan_type": plan_type,
            "chatgpt_user_id": user_id,
            "user_id": user_id,
            "chatgpt_account_id": account_id,
        },
    });
    let encode = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let header_b64 = encode(&serde_json::to_vec(&header)?);
    let payload_b64 = encode(&serde_json::to_vec(&payload)?);
    let signature_b64 = encode(b"sig");
    Ok(format!("{header_b64}.{payload_b64}.{signature_b64}"))
}

#[test]
fn file_account_pool_storage_load_returns_default_when_file_missing() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let storage = FileAccountPoolStorage::new(codex_home.path().to_path_buf());

    let loaded = storage.load()?;

    assert_eq!(loaded, AccountPoolFile::default());
    Ok(())
}

#[test]
fn file_account_pool_storage_save_and_load_round_trip() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let storage = FileAccountPoolStorage::new(codex_home.path().to_path_buf());
    let account_pool = AccountPoolFile {
        current_alias: Some("primary".to_string()),
        accounts: vec![sample_account("primary"), sample_account("backup")],
    };

    storage.save(&account_pool)?;
    let loaded = storage.load()?;

    assert_eq!(loaded, account_pool);
    Ok(())
}

#[test]
fn file_account_pool_storage_save_persists_expected_json_shape() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let storage = FileAccountPoolStorage::new(codex_home.path().to_path_buf());
    let account_pool = AccountPoolFile {
        current_alias: Some("primary".to_string()),
        accounts: vec![sample_account("primary")],
    };

    storage.save(&account_pool)?;
    let persisted = storage.try_read_account_pool(&get_account_pool_file(codex_home.path()))?;

    assert_eq!(persisted, account_pool);
    Ok(())
}

#[test]
fn account_pool_manager_upsert_replaces_existing_alias() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let manager = AccountPoolManager::new(
        codex_home.path().to_path_buf(),
        AuthCredentialsStoreMode::File,
    );

    manager.upsert_account(sample_account("primary"), /*make_current*/ false)?;

    let mut updated = sample_account("primary");
    updated.account_identity.plan_type = Some("pro".to_string());
    manager.upsert_account(updated.clone(), /*make_current*/ false)?;

    let account_pool = manager.load()?;
    assert_eq!(
        account_pool,
        AccountPoolFile {
            current_alias: None,
            accounts: vec![updated],
        }
    );
    Ok(())
}

#[test]
fn account_pool_manager_activate_writes_active_auth_and_config() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let manager = AccountPoolManager::new(
        codex_home.path().to_path_buf(),
        AuthCredentialsStoreMode::File,
    );
    let primary = sample_account("primary");
    manager.upsert_account(primary.clone(), /*make_current*/ false)?;

    manager.activate_account("primary")?;

    let account_pool = manager.load()?;
    let auth = load_auth_dot_json(codex_home.path(), AuthCredentialsStoreMode::File)?
        .expect("active auth should be written");
    let config = std::fs::read_to_string(codex_home.path().join(CONFIG_TOML_FILE))?;

    assert_eq!(account_pool.current_alias, Some("primary".to_string()));
    assert_eq!(auth, primary.auth_snapshot);
    assert_eq!(config, "model = \"gpt-5\"");
    Ok(())
}

#[test]
fn account_pool_manager_activate_removes_config_when_snapshot_is_missing() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let manager = AccountPoolManager::new(
        codex_home.path().to_path_buf(),
        AuthCredentialsStoreMode::File,
    );
    let mut primary = sample_account("primary");
    primary.config_snapshot = None;
    manager.upsert_account(primary, /*make_current*/ false)?;
    std::fs::write(
        codex_home.path().join(CONFIG_TOML_FILE),
        "model = \"stale\"",
    )?;

    manager.activate_account("primary")?;

    assert!(!codex_home.path().join(CONFIG_TOML_FILE).exists());
    Ok(())
}

#[test]
fn account_pool_manager_activate_errors_when_alias_is_missing() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let manager = AccountPoolManager::new(
        codex_home.path().to_path_buf(),
        AuthCredentialsStoreMode::File,
    );

    let err = manager
        .activate_account("missing")
        .expect_err("missing alias should fail");

    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    Ok(())
}

#[test]
fn account_pool_manager_syncs_active_auth_into_native_entry() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let manager = AccountPoolManager::new(
        codex_home.path().to_path_buf(),
        AuthCredentialsStoreMode::File,
    );
    save_auth(
        codex_home.path(),
        &sample_chatgpt_auth()?,
        AuthCredentialsStoreMode::File,
    )?;
    std::fs::write(
        codex_home.path().join(CONFIG_TOML_FILE),
        "model = \"gpt-5\"",
    )?;

    let account_pool = manager
        .sync_active_auth(None)?
        .expect("sync should create an account");

    assert_eq!(
        account_pool.current_alias,
        Some("native@example.com".to_string())
    );
    assert_eq!(account_pool.accounts.len(), 1);
    assert_eq!(account_pool.accounts[0].source, AccountPoolSource::Native);
    assert_eq!(
        account_pool.accounts[0].account_identity,
        AccountIdentity {
            account_id: Some("org-native".to_string()),
            user_id: Some("user-native".to_string()),
            email: Some("native@example.com".to_string()),
            plan_type: Some("pro".to_string()),
        }
    );
    assert_eq!(
        account_pool.accounts[0].config_snapshot,
        Some("model = \"gpt-5\"".to_string())
    );
    assert_eq!(
        account_pool.accounts[0]
            .token_health
            .refresh_status
            .as_deref(),
        Some("ok")
    );
    assert_eq!(account_pool.accounts[0].token_health.needs_relogin, false);
    assert!(account_pool.accounts[0].token_health.expires_at.is_some());
    Ok(())
}

#[test]
fn account_pool_manager_clear_current_alias_resets_active_pointer() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let manager = AccountPoolManager::new(
        codex_home.path().to_path_buf(),
        AuthCredentialsStoreMode::File,
    );
    manager.upsert_account(sample_account("primary"), /*make_current*/ true)?;

    let account_pool = manager.clear_current_alias()?;

    assert_eq!(account_pool.current_alias, None);
    assert_eq!(account_pool.accounts.len(), 1);
    Ok(())
}

#[test]
fn account_pool_manager_prefers_most_recent_quota_refresh_when_switching() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let manager = AccountPoolManager::new(
        codex_home.path().to_path_buf(),
        AuthCredentialsStoreMode::File,
    );
    let mut exhausted = sample_account("exhausted");
    exhausted.usage_health.quota_exhausted = true;
    exhausted.switch_policy_state.priority = Some(100);
    let mut relogin = sample_account("relogin");
    relogin.token_health.needs_relogin = true;
    relogin.switch_policy_state.priority = Some(90);
    let mut stale_but_higher_priority = sample_account("stale-but-higher-priority");
    stale_but_higher_priority.switch_policy_state.priority = Some(100);
    stale_but_higher_priority.usage_health.last_checked_at =
        Some(Utc::now() - chrono::Duration::minutes(10));
    let mut fresh = sample_account("fresh");
    fresh.switch_policy_state.priority = Some(10);
    fresh.usage_health.last_checked_at = Some(Utc::now());
    manager.upsert_account(exhausted, /*make_current*/ false)?;
    manager.upsert_account(relogin, /*make_current*/ false)?;
    manager.upsert_account(stale_but_higher_priority, /*make_current*/ false)?;
    manager.upsert_account(fresh, /*make_current*/ false)?;

    let selected = manager.select_best_switch_target(Some("stale-but-higher-priority"))?;

    assert_eq!(selected.as_deref(), Some("fresh"));
    Ok(())
}

#[test]
fn account_pool_manager_updates_account_usage_health() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let manager = AccountPoolManager::new(
        codex_home.path().to_path_buf(),
        AuthCredentialsStoreMode::File,
    );
    manager.upsert_account(sample_account("primary"), /*make_current*/ true)?;

    manager.update_account_usage_health(
        "primary",
        Some(0),
        Some(1_744_204_860),
        Some(10),
        Some(1_744_808_860),
    )?;

    let account_pool = manager.load()?;
    let account = account_pool
        .accounts
        .into_iter()
        .find(|account| account.alias == "primary")
        .expect("primary account should exist");
    assert_eq!(account.usage_health.five_hour_remaining_percent, Some(0));
    assert_eq!(account.usage_health.weekly_remaining_percent, Some(10));
    assert!(account.usage_health.quota_exhausted);
    Ok(())
}

#[test]
fn account_pool_manager_imports_codex_acc_store() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let manager = AccountPoolManager::new(
        codex_home.path().to_path_buf(),
        AuthCredentialsStoreMode::File,
    );
    let codex_acc_store = serde_json::json!({
        "current": "primary",
        "profiles": {
            "primary": {
                "alias": "primary",
                "auth": sample_auth(),
                "config": "model = \"gpt-5\"",
                "account": {
                    "accountId": "acct-primary",
                    "userId": "user-primary",
                    "email": "primary@example.com",
                    "planType": "plus"
                },
                "usage": {
                    "fiveHourLimit": "80% 剩余",
                    "weeklyLimit": "70% 剩余"
                }
            },
            "backup": {
                "alias": "backup",
                "auth": sample_auth(),
                "config": "model = \"gpt-5.1\"",
                "account": {
                    "accountId": "acct-backup",
                    "userId": "user-backup",
                    "email": "backup@example.com",
                    "planType": "pro"
                },
                "usage": {
                    "fiveHourLimit": "40% 剩余",
                    "weeklyLimit": "20% 剩余"
                }
            }
        }
    });
    let store_path = codex_home.path().join("codex-cc.json");
    std::fs::write(&store_path, serde_json::to_vec_pretty(&codex_acc_store)?)?;

    let result = manager.import_codex_acc_store(&store_path)?;
    let account_pool = manager.load()?;

    assert_eq!(
        result.imported_aliases,
        vec!["backup".to_string(), "primary".to_string()]
    );
    assert_eq!(result.skipped_aliases, Vec::<String>::new());
    assert_eq!(account_pool.current_alias, Some("primary".to_string()));
    assert_eq!(account_pool.accounts.len(), 2);
    assert_eq!(
        account_pool.accounts[0].source,
        AccountPoolSource::CodexAccImport
    );
    assert_eq!(
        account_pool.accounts[0]
            .usage_health
            .five_hour_remaining_percent,
        Some(40)
    );
    assert_eq!(
        account_pool.accounts[0]
            .usage_health
            .weekly_remaining_percent,
        Some(20)
    );
    Ok(())
}

#[test]
fn account_pool_manager_imports_codex_acc_store_dedupes_by_email() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let manager = AccountPoolManager::new(
        codex_home.path().to_path_buf(),
        AuthCredentialsStoreMode::File,
    );
    manager.upsert_account(sample_account("primary"), /*make_current*/ false)?;
    let codex_acc_store = serde_json::json!({
        "profiles": {
            "other-alias": {
                "auth": sample_auth(),
                "config": "model = \"gpt-5.1\"",
                "account": {
                    "accountId": "acct-primary-new",
                    "userId": "user-primary-new",
                    "email": "primary@example.com",
                    "planType": "pro"
                },
                "usage": {
                    "fiveHourLimit": "40% 剩余",
                    "weeklyLimit": "20% 剩余"
                }
            }
        }
    });
    let store_path = codex_home.path().join("codex-cc.json");
    std::fs::write(&store_path, serde_json::to_vec_pretty(&codex_acc_store)?)?;

    let _ = manager.import_codex_acc_store(&store_path)?;
    let account_pool = manager.load()?;

    assert_eq!(account_pool.accounts.len(), 1);
    assert_eq!(account_pool.accounts[0].alias, "primary".to_string());
    assert_eq!(
        account_pool.accounts[0].account_identity.plan_type,
        Some("pro".to_string())
    );
    assert_eq!(
        account_pool.accounts[0]
            .usage_health
            .five_hour_remaining_percent,
        Some(40)
    );
    Ok(())
}

#[tokio::test]
async fn account_pool_manager_imports_cc_switch_db() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let manager = AccountPoolManager::new(
        codex_home.path().to_path_buf(),
        AuthCredentialsStoreMode::File,
    );
    let db_path = codex_home.path().join("cc-switch.db");
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true);
    let pool = sqlx::SqlitePool::connect_with(options).await?;
    sqlx::query(
        "CREATE TABLE providers (
            id TEXT PRIMARY KEY,
            app_type TEXT NOT NULL,
            name TEXT NOT NULL,
            settings_config TEXT NOT NULL,
            sort_index INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(&pool)
    .await?;
    let settings_config = serde_json::json!({
        "auth": sample_auth(),
        "config": "model = \"gpt-5.4\""
    });
    sqlx::query(
        "INSERT INTO providers (id, app_type, name, settings_config, sort_index, created_at)
         VALUES (?1, 'codex', ?2, ?3, 0, 1)",
    )
    .bind("provider-1")
    .bind("Imported")
    .bind(settings_config.to_string())
    .execute(&pool)
    .await?;
    pool.close().await;

    let result = manager.import_cc_switch_db(&db_path).await?;
    let account_pool = manager.load()?;

    assert_eq!(result.imported_aliases, vec!["Imported".to_string()]);
    assert_eq!(account_pool.current_alias, None);
    assert_eq!(account_pool.accounts.len(), 1);
    assert_eq!(
        account_pool.accounts[0].source,
        AccountPoolSource::CcSwitchImport
    );
    assert_eq!(
        account_pool.accounts[0].config_snapshot,
        Some("model = \"gpt-5.4\"".to_string())
    );
    Ok(())
}
