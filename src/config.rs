use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

pub const DEFAULT_BASE_URL: &str = "https://www.yeahpdf.com/api/v1";
pub const DEFAULT_PROFILE: &str = "default";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFile {
    #[serde(default = "default_profile_name")]
    pub default_profile: String,
    #[serde(default)]
    pub guest_id: String,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self {
            default_profile: DEFAULT_PROFILE.into(),
            guest_id: String::new(),
            profiles: BTreeMap::new(),
        }
    }
}

fn default_profile_name() -> String {
    DEFAULT_PROFILE.into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub base_url: String,
}

#[derive(Debug, Clone)]
pub struct Resolved {
    pub profile: String,
    pub api_key: String,
    pub base_url: String,
}

impl ConfigFile {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("读取配置失败: {}", path.display()))?;
        let parsed: Self =
            toml::from_str(&text).with_context(|| format!("解析配置失败: {}", path.display()))?;
        Ok(parsed)
    }

    pub fn save(&self) -> Result<PathBuf> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("创建配置目录失败: {}", parent.display()))?;
        }
        let text = toml::to_string_pretty(self).context("序列化配置失败")?;
        fs::write(&path, text).with_context(|| format!("写入配置失败: {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
        Ok(path)
    }

    pub fn upsert(&mut self, name: &str, api_key: Option<String>, base_url: Option<String>) {
        let profile = self.profiles.entry(name.to_string()).or_default();
        if let Some(key) = api_key.filter(|value| !value.is_empty()) {
            profile.api_key = key;
        }
        if let Some(url) = base_url.filter(|value| !value.is_empty()) {
            profile.base_url = normalize_base_url(&url);
        }
        if profile.base_url.is_empty() {
            profile.base_url = DEFAULT_BASE_URL.into();
        }
        self.default_profile = name.to_string();
    }

    pub fn remove_profile(&mut self, name: &str) -> Result<()> {
        if self.profiles.remove(name).is_none() {
            bail!("profile 不存在: {name}");
        }
        if self.default_profile == name {
            self.default_profile = self
                .profiles
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| DEFAULT_PROFILE.into());
        }
        Ok(())
    }

    pub fn clear_profiles(&mut self) {
        self.profiles.clear();
        self.default_profile = DEFAULT_PROFILE.into();
    }

    pub fn persist_guest_id(&mut self, guest_id: &str) -> Result<Option<PathBuf>> {
        if !valid_guest_id(guest_id) || self.guest_id == guest_id {
            return Ok(None);
        }
        self.guest_id = guest_id.to_string();
        Ok(Some(self.save()?))
    }
}

pub fn config_path() -> Result<PathBuf> {
    if let Some(explicit) = std::env::var_os("YPDF_CONFIG") {
        return Ok(PathBuf::from(explicit));
    }
    let dir = dirs::config_dir().context("无法确定用户配置目录")?;
    Ok(dir.join("ypdf").join("config.toml"))
}

pub fn normalize_base_url(raw: &str) -> String {
    let trimmed = prefer_www_yeahpdf(raw.trim().trim_end_matches('/'));
    if trimmed.is_empty() {
        return DEFAULT_BASE_URL.into();
    }
    if trimmed.contains("/api") {
        trimmed
    } else {
        format!("{trimmed}/api/v1")
    }
}

/// Apex `yeahpdf.com` 301 到 `www`；reqwest 跨域跳转会丢掉 Authorization，表现为 1101。
fn prefer_www_yeahpdf(raw: &str) -> String {
    for prefix in ["https://yeahpdf.com", "http://yeahpdf.com"] {
        if raw == prefix
            || raw.starts_with(&format!("{prefix}/"))
            || raw.starts_with(&format!("{prefix}?"))
        {
            return format!("https://www.yeahpdf.com{}", &raw[prefix.len()..]);
        }
    }
    raw.to_string()
}

pub fn mask_key(key: &str) -> String {
    if key.chars().count() <= 10 {
        return "ypdf_…".into();
    }
    let prefix: String = key.chars().take(10).collect();
    format!("{prefix}…")
}

pub fn resolve(
    file: &ConfigFile,
    profile: Option<&str>,
    api_key: Option<String>,
    base_url: Option<String>,
) -> Result<Resolved> {
    let name = profile
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            if file.default_profile.is_empty() {
                DEFAULT_PROFILE
            } else {
                file.default_profile.as_str()
            }
        })
        .to_string();
    let stored = file.profiles.get(&name);
    let key = first_nonempty([
        api_key,
        std::env::var("YPDF_API_KEY").ok(),
        stored.map(|item| item.api_key.clone()),
    ]);
    let url = first_nonempty([
        base_url,
        std::env::var("YPDF_BASE_URL").ok(),
        stored.map(|item| item.base_url.clone()),
        Some(DEFAULT_BASE_URL.into()),
    ])
    .unwrap_or_else(|| DEFAULT_BASE_URL.into());
    let key = key.unwrap_or_default();
    if !key.is_empty() && !key.starts_with("ypdf_") {
        bail!("API Key 应以 ypdf_ 开头");
    }
    Ok(Resolved {
        profile: name,
        api_key: key,
        base_url: normalize_base_url(&url),
    })
}

pub fn origin_from_base_url(base: &str) -> Option<String> {
    let raw = base.trim();
    let (scheme, rest) = raw.split_once("://")?;
    let host = rest.split(['/', '?']).next()?.trim();
    if host.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{host}"))
}

pub fn valid_guest_id(value: &str) -> bool {
    uuid::Uuid::parse_str(value.trim()).is_ok()
}

pub fn new_guest_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn parse_guest_set_cookie(value: &str) -> Option<String> {
    let first = value.split(';').next()?.trim();
    let (name, id) = first.split_once('=')?;
    let id = id.trim();
    if name.trim() != "ypdf_guest" || !valid_guest_id(id) {
        return None;
    }
    Some(id.to_string())
}

fn first_nonempty(values: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_api_prefix_when_missing() {
        assert_eq!(
            normalize_base_url("https://yeahpdf.com"),
            "https://www.yeahpdf.com/api/v1"
        );
        assert_eq!(
            normalize_base_url("https://yeahpdf.com/api/v1/"),
            "https://www.yeahpdf.com/api/v1"
        );
        assert_eq!(
            normalize_base_url("https://www.yeahpdf.com"),
            "https://www.yeahpdf.com/api/v1"
        );
        assert_eq!(
            normalize_base_url("https://staging.yeahpdf.com/api/v2"),
            "https://staging.yeahpdf.com/api/v2"
        );
    }

    #[test]
    fn accepts_ypdf_keys_only() {
        let file = ConfigFile::default();
        let ypdf = resolve(
            &file,
            None,
            Some("ypdf_abc123".into()),
            Some("https://yeahpdf.com".into()),
        )
        .unwrap();
        assert_eq!(ypdf.api_key, "ypdf_abc123");
        assert!(resolve(
            &file,
            None,
            Some("hp_oldkey".into()),
            Some("https://yeahpdf.com".into()),
        )
        .is_err());
        assert!(resolve(
            &file,
            None,
            Some("sk_wrong".into()),
            Some("https://yeahpdf.com".into()),
        )
        .is_err());
        let guest = resolve(&file, None, None, Some("https://www.yeahpdf.com".into())).unwrap();
        assert!(guest.api_key.is_empty());
        assert_eq!(guest.base_url, "https://www.yeahpdf.com/api/v1");
    }

    #[test]
    fn origin_follows_base_host() {
        assert_eq!(
            origin_from_base_url("https://www.yeahpdf.com/api/v1"),
            Some("https://www.yeahpdf.com".into())
        );
        assert_eq!(
            origin_from_base_url("http://127.0.0.1:8080/api/v1"),
            Some("http://127.0.0.1:8080".into())
        );
    }

    #[test]
    fn parses_guest_set_cookie() {
        assert_eq!(
            parse_guest_set_cookie(
                "ypdf_guest=aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee; Path=/; HttpOnly"
            )
            .as_deref(),
            Some("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee")
        );
        assert!(parse_guest_set_cookie("ypdf_guest=not-a-uuid").is_none());
    }

    #[test]
    fn logout_switches_default_to_remaining_profile() {
        let mut file = ConfigFile::default();
        file.upsert("default", Some("ypdf_aaa".into()), None);
        file.upsert("work", Some("ypdf_bbb".into()), None);
        file.default_profile = "default".into();
        file.remove_profile("default").unwrap();
        assert!(!file.profiles.contains_key("default"));
        assert_eq!(file.default_profile, "work");
        file.guest_id = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into();
        file.clear_profiles();
        assert!(file.profiles.is_empty());
        assert_eq!(file.default_profile, DEFAULT_PROFILE);
        assert_eq!(file.guest_id, "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee");
        assert!(file.remove_profile("missing").is_err());
    }
}

pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent().filter(|item| !item.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建目录失败: {}", parent.display()))?;
    }
    Ok(())
}
