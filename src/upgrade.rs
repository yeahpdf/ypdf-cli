use std::env;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use reqwest::redirect::Policy;

use crate::display::user_agent;

const DEFAULT_REPO: &str = "yeahpdf/ypdf-cli";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Semver {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Semver {
    pub fn parse(raw: &str) -> Result<Self> {
        let raw = raw.trim();
        let mut parts = raw.split('.');
        let major = parse_part(parts.next(), raw)?;
        let minor = parse_part(parts.next(), raw)?;
        let patch = parse_part(parts.next(), raw)?;
        if parts.next().is_some() {
            bail!("版本必须是 X.Y.Z: {raw}");
        }
        Ok(Self {
            major,
            minor,
            patch,
        })
    }

    pub fn display(self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }
}

fn parse_part(part: Option<&str>, raw: &str) -> Result<u64> {
    let part = part.context(format!("版本必须是 X.Y.Z: {raw}"))?;
    part.parse()
        .with_context(|| format!("版本必须是 X.Y.Z: {raw}"))
}

pub fn parse_release_tag(tag: &str) -> Result<Semver> {
    let tag = tag.trim().trim_end_matches('/');
    let name = tag.rsplit('/').next().unwrap_or(tag);
    let ver = name
        .strip_prefix("cli-v")
        .or_else(|| name.strip_prefix('v'))
        .unwrap_or(name);
    Semver::parse(ver)
}

pub fn latest_from_location(location: &str) -> Result<String> {
    let location = location.trim();
    if location.is_empty() {
        bail!("GitHub latest 没有 Location");
    }
    Ok(location.rsplit('/').next().unwrap_or(location).to_string())
}

pub fn target_triple(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("Darwin", "arm64" | "aarch64") => Some("aarch64-apple-darwin"),
        ("Darwin", "x86_64") => Some("x86_64-apple-darwin"),
        ("Linux", "x86_64" | "amd64") => Some("x86_64-unknown-linux-gnu"),
        ("Linux", "aarch64" | "arm64") => Some("aarch64-unknown-linux-gnu"),
        _ => None,
    }
}

fn host_os() -> &'static str {
    match env::consts::OS {
        "macos" => "Darwin",
        "linux" => "Linux",
        other => other,
    }
}

fn host_arch() -> &'static str {
    match env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        other => other,
    }
}

fn host_uname_pair() -> (&'static str, &'static str) {
    (host_os(), host_arch())
}

pub fn repo() -> String {
    env::var("YPDF_CLI_REPO").unwrap_or_else(|_| DEFAULT_REPO.into())
}

pub async fn resolve_latest_tag(repo: &str) -> Result<String> {
    let url = format!("https://github.com/{repo}/releases/latest");
    let client = reqwest::Client::builder()
        .redirect(Policy::none())
        .user_agent(user_agent())
        .build()
        .context("创建 HTTP 客户端失败")?;
    let location = match header_location(&client, reqwest::Method::HEAD, &url).await {
        Ok(value) => value,
        Err(_) => header_location(&client, reqwest::Method::GET, &url).await?,
    };
    latest_from_location(&location)
}

async fn header_location(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
) -> Result<String> {
    let response = client
        .request(method, url)
        .send()
        .await
        .with_context(|| format!("无法访问 {url}"))?;
    response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .context("GitHub latest 没有 Location")
}

pub async fn run(check_only: bool) -> Result<()> {
    let current = Semver::parse(env!("CARGO_PKG_VERSION"))?;
    let repo = repo();
    let (os, arch) = host_uname_pair();
    let target = target_triple(os, arch);
    println!("current={}", current.display());
    println!("repo={repo}");

    let Some(target) = target else {
        println!("status=unsupported_platform");
        println!("releases=https://github.com/{repo}/releases/latest");
        bail!("此平台没有预编译 ypdf-cli");
    };
    println!("target={target}");

    let tag = match resolve_latest_tag(&repo).await {
        Ok(tag) => tag,
        Err(err) => {
            println!("status=latest_unresolved");
            println!("releases=https://github.com/{repo}/releases/latest");
            return Err(err);
        }
    };
    let latest = parse_release_tag(&tag)?;
    println!("tag={tag}");
    println!("latest={}", latest.display());

    if latest <= current {
        println!("status=up_to_date");
        return Ok(());
    }

    println!("status=update_available");
    let asset = format!("ypdf-cli-{}-{target}.tar.gz", latest.display());
    let url = format!("https://github.com/{repo}/releases/download/{tag}/{asset}");
    println!("asset={asset}");
    println!("url={url}");
    if check_only {
        return Ok(());
    }

    let dest = env::current_exe().context("找不到当前 ypdf-cli 路径")?;
    if dest_not_writable(&dest) {
        println!("status=need_write");
        println!("binary={}", dest.display());
        println!("hint=bash scripts/install.sh");
        bail!("当前二进制不可写: {}", dest.display());
    }

    install_over(&url, &asset, &dest).await?;
    println!("binary={}", dest.display());
    println!("status=upgraded");
    Ok(())
}

fn dest_not_writable(dest: &Path) -> bool {
    let Some(dir) = dest.parent() else {
        return true;
    };
    let probe = dir.join(".ypdf-cli-write-probe");
    let ok = std::fs::write(&probe, b"ok").is_ok();
    let _ = std::fs::remove_file(&probe);
    !ok
}

async fn install_over(url: &str, asset: &str, dest: &Path) -> Result<()> {
    let bytes = reqwest::Client::builder()
        .user_agent(user_agent())
        .build()?
        .get(url)
        .send()
        .await
        .with_context(|| format!("下载失败: {url}"))?
        .error_for_status()
        .with_context(|| format!("下载失败: {url}"))?
        .bytes()
        .await
        .context("读取安装包失败")?;

    let tmp = tempfile_dir()?;
    let archive = tmp.join(asset);
    std::fs::write(&archive, &bytes).context("写入安装包失败")?;
    let status = std::process::Command::new("tar")
        .args(["-xzf", archive.to_str().context("安装包路径无效")?, "-C"])
        .arg(&tmp)
        .status()
        .context("需要 tar 解压安装包")?;
    if !status.success() {
        bail!("解压失败: {asset}");
    }
    let extracted = tmp.join("ypdf-cli");
    if !extracted.is_file() {
        bail!("安装包里没有 ypdf-cli: {asset}");
    }
    replace_binary(&extracted, dest)?;
    Ok(())
}

fn replace_binary(src: &Path, dest: &Path) -> Result<()> {
    let staged = dest.with_file_name(format!(
        ".{}.new",
        dest.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("ypdf-cli")
    ));
    std::fs::copy(src, &staged).with_context(|| format!("无法写入 {}", staged.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&staged, dest).with_context(|| format!("无法覆盖 {}", dest.display()))?;
    Ok(())
}

fn tempfile_dir() -> Result<PathBuf> {
    let dir = env::temp_dir().join(format!(
        "ypdf-cli-upgrade-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .context("时钟异常")?
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cli_tag() {
        assert_eq!(
            parse_release_tag("cli-v0.1.2").unwrap(),
            Semver {
                major: 0,
                minor: 1,
                patch: 2
            }
        );
        assert_eq!(
            parse_release_tag("https://github.com/yeahpdf/ypdf-cli/releases/tag/cli-v0.2.0")
                .unwrap()
                .display(),
            "0.2.0"
        );
        assert_eq!(parse_release_tag("v1.0.0").unwrap().display(), "1.0.0");
    }

    #[test]
    fn compares_semver() {
        let older = Semver::parse("0.1.1").unwrap();
        let newer = Semver::parse("0.1.2").unwrap();
        assert!(newer > older);
        assert!(older <= older);
        assert!(Semver::parse("0.2.0").unwrap() > Semver::parse("0.1.9").unwrap());
    }

    #[test]
    fn maps_targets() {
        assert_eq!(
            target_triple("Darwin", "arm64"),
            Some("aarch64-apple-darwin")
        );
        assert_eq!(
            target_triple("Linux", "x86_64"),
            Some("x86_64-unknown-linux-gnu")
        );
        assert_eq!(target_triple("Windows_NT", "x86_64"), None);
    }

    #[test]
    fn location_uses_last_segment() {
        assert_eq!(
            latest_from_location("https://github.com/yeahpdf/ypdf-cli/releases/tag/cli-v0.1.2")
                .unwrap(),
            "cli-v0.1.2"
        );
    }
}
