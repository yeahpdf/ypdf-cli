use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::multipart::Form;
use reqwest::{Client as Http, StatusCode};
use serde::Deserialize;
use serde_json::Value;
use tokio::time::sleep;

use crate::config::{ensure_parent_dir, Resolved};

#[derive(Debug, Clone)]
pub struct FormField {
    pub name: String,
    pub value: FieldValue,
}

#[derive(Debug, Clone)]
pub enum FieldValue {
    Text(String),
    File(PathBuf),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub job_id: String,
    pub kind: String,
    pub status: String,
    pub error: Option<String>,
    pub data: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    code: Option<Value>,
    message: Option<String>,
}

pub struct Api {
    http: Http,
    base_url: String,
    api_key: String,
}

impl Api {
    pub fn new(resolved: &Resolved) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", resolved.api_key))
                .context("API Key 含非法字符")?,
        );
        let http = Http::builder()
            .default_headers(headers)
            .user_agent("ypdf-cli/0.1.0")
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(1800))
            .build()
            .context("创建 HTTP 客户端失败")?;
        Ok(Self {
            http,
            base_url: resolved.base_url.trim_end_matches('/').to_string(),
            api_key: resolved.api_key.clone(),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn key_hint(&self) -> String {
        crate::config::mask_key(&self.api_key)
    }

    pub async fn get_json(&self, path: &str) -> Result<Value> {
        let url = self.url(path);
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        read_json(response).await
    }

    pub async fn post_form(&self, path: &str, fields: &[FormField]) -> Result<Value> {
        let url = self.url(path);
        let mut form = Form::new();
        for field in fields {
            form = match &field.value {
                FieldValue::Text(value) => form.text(field.name.clone(), value.clone()),
                FieldValue::File(path) => {
                    let (key, name) = self.upload_direct(path).await?;
                    if field.name == "watermarkImage" {
                        form = form.text("watermarkImageKey", key);
                        form.text("watermarkImageName", name)
                    } else {
                        form = form.text("fileKey", key);
                        form.text("fileName", name)
                    }
                }
            };
        }
        let response = self
            .http
            .post(&url)
            .multipart(form)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        read_json(response).await
    }

    async fn upload_direct(&self, path: &Path) -> Result<(String, String)> {
        let bytes = tokio::fs::read(path)
            .await
            .with_context(|| format!("读取文件失败: {}", path.display()))?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("upload.bin")
            .to_string();
        let presign_url = self.url("/uploads/presign");
        let body = serde_json::json!({
            "fileName": name,
            "contentType": content_type_for(&name),
            "bytes": bytes.len(),
        });
        let response = self
            .http
            .post(&presign_url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {presign_url}"))?;
        let value = read_json(response).await?;
        let upload_url = value
            .get("uploadUrl")
            .and_then(Value::as_str)
            .context("预签名响应缺少 uploadUrl")?;
        let key = value
            .get("key")
            .and_then(Value::as_str)
            .context("预签名响应缺少 key")?
            .to_string();
        let mut put = self.http.put(upload_url).body(bytes);
        if let Some(headers) = value.get("headers").and_then(Value::as_object) {
            for (name, header) in headers {
                if name.eq_ignore_ascii_case("content-length") {
                    continue;
                }
                if let Some(header) = header.as_str() {
                    put = put.header(name, header);
                }
            }
        }
        let uploaded = put
            .send()
            .await
            .with_context(|| format!("PUT {upload_url}"))?;
        if !uploaded.status().is_success() {
            bail!(
                "直传对象存储失败 ({}): {}",
                uploaded.status(),
                uploaded.text().await.unwrap_or_default()
            );
        }
        Ok((key, name))
    }

    pub async fn run_job(
        &self,
        path: &str,
        fields: &[FormField],
        out: &Path,
        timeout: Duration,
    ) -> Result<PathBuf> {
        let value = self.post_form(path, fields).await?;
        let job: JobView = serde_json::from_value(value).context("入队响应无法解析")?;
        eprintln!("queued {} ({})", job.job_id, job.kind);
        let finished = self.wait_job(&job.job_id, timeout).await?;
        if finished.status != "succeeded" {
            bail!(
                "任务失败 ({}): {}",
                finished.status,
                finished.error.unwrap_or_else(|| "未知错误".into())
            );
        }
        self.download_result(&finished.job_id, out).await
    }

    pub async fn wait_job(&self, job_id: &str, timeout: Duration) -> Result<JobView> {
        let started = Instant::now();
        let mut delay = Duration::from_secs(2);
        loop {
            let job = self.get_job(job_id).await?;
            match job.status.as_str() {
                "succeeded" | "failed" | "expired" => return Ok(job),
                status => eprintln!("  {job_id} {status}"),
            }
            if started.elapsed() >= timeout {
                bail!("等待任务超时: {job_id}");
            }
            sleep(delay).await;
            if delay < Duration::from_secs(8) {
                delay *= 2;
            }
        }
    }

    pub async fn get_job(&self, job_id: &str) -> Result<JobView> {
        let value = self.get_json(&format!("/jobs/{job_id}")).await?;
        serde_json::from_value(value).context("任务响应无法解析")
    }

    pub async fn download_result(&self, job_id: &str, out: &Path) -> Result<PathBuf> {
        let meta = self
            .get_json(&format!("/jobs/{job_id}/result?redirect=json"))
            .await?;
        let download_url = meta
            .get("url")
            .and_then(Value::as_str)
            .context("结果响应缺少预签名 url")?;
        let hinted = meta
            .get("fileName")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let response = Http::builder()
            .user_agent("ypdf-cli/0.1.0")
            .timeout(Duration::from_secs(300))
            .build()
            .context("创建下载客户端失败")?
            .get(download_url)
            .send()
            .await
            .with_context(|| format!("下载 {download_url}"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(api_error(status, response.text().await.unwrap_or_default()));
        }
        let bytes = response.bytes().await.context("读取结果失败")?;
        let path = resolve_out_path(out, hinted.as_deref().unwrap_or("result.bin"))?;
        ensure_parent_dir(&path)?;
        tokio::fs::write(&path, &bytes)
            .await
            .with_context(|| format!("写入失败: {}", path.display()))?;
        eprintln!("saved {}", path.display());
        Ok(path)
    }

    fn url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            return path.to_string();
        }
        let path = path.trim_start_matches('/');
        format!("{}/{}", self.base_url.trim_end_matches('/'), path)
    }
}

fn content_type_for(name: &str) -> &'static str {
    let ext = Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "xlsx" | "xlsm" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xls" => "application/vnd.ms-excel",
        "xlsb" => "application/vnd.ms-excel.sheet.binary.workbook",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        _ => "application/octet-stream",
    }
}

fn resolve_out_path(out: &Path, filename: &str) -> Result<PathBuf> {
    if out.as_os_str().is_empty() || out == Path::new(".") {
        return Ok(PathBuf::from(filename));
    }
    if out.is_dir()
        || out
            .to_string_lossy()
            .ends_with(['/', std::path::MAIN_SEPARATOR])
        || out.extension().is_none()
    {
        if !out.exists() {
            std::fs::create_dir_all(out)
                .with_context(|| format!("创建输出目录失败: {}", out.display()))?;
        }
        return Ok(out.join(filename));
    }
    Ok(out.to_path_buf())
}

async fn read_json(response: reqwest::Response) -> Result<Value> {
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(api_error(status, text));
    }
    serde_json::from_str(&text).with_context(|| format!("响应不是 JSON: {text}"))
}

fn api_error(status: StatusCode, text: String) -> anyhow::Error {
    if let Ok(body) = serde_json::from_str::<ErrorBody>(&text) {
        let code = body
            .code
            .map(|value| match value {
                Value::String(text) => text,
                Value::Number(number) => number.to_string(),
                other => other.to_string(),
            })
            .unwrap_or_else(|| status.as_u16().to_string());
        let message = body.message.unwrap_or(text);
        anyhow::anyhow!("{code} {message}")
    } else if text.is_empty() {
        anyhow::anyhow!("HTTP {status}")
    } else {
        anyhow::anyhow!("HTTP {status}: {text}")
    }
}
