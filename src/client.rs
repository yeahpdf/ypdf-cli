use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use reqwest::header::{AUTHORIZATION, CONTENT_LENGTH, COOKIE, ORIGIN, SET_COOKIE};
use reqwest::multipart::Form;
use reqwest::{Body, Client as Http, RequestBuilder, StatusCode};
use serde::Deserialize;
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::time::sleep;
use tokio_util::io::ReaderStream;

use crate::config::{
    ensure_parent_dir, new_guest_id, origin_from_base_url, parse_guest_set_cookie, valid_guest_id,
    Resolved,
};
use crate::display::{format_bytes, is_rate_limit_error, user_agent, with_login_hint};

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

#[derive(Debug, Clone)]
enum PreparedValue {
    Text(String),
    File {
        key: String,
        name: String,
        watermark: bool,
    },
}

#[derive(Debug, Clone)]
struct PreparedField {
    name: String,
    value: PreparedValue,
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
    origin: Option<String>,
    guest_id: std::sync::Mutex<Option<String>>,
    quiet: bool,
}

impl Api {
    pub fn new(resolved: &Resolved, quiet: bool, stored_guest_id: Option<String>) -> Result<Self> {
        let http = Http::builder()
            .user_agent(user_agent())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(1800))
            .build()
            .context("创建 HTTP 客户端失败")?;
        let guest = if resolved.api_key.is_empty() {
            Some(
                stored_guest_id
                    .filter(|id| valid_guest_id(id))
                    .unwrap_or_else(new_guest_id),
            )
        } else {
            None
        };
        Ok(Self {
            http,
            base_url: resolved.base_url.trim_end_matches('/').to_string(),
            api_key: resolved.api_key.clone(),
            origin: if resolved.api_key.is_empty() {
                origin_from_base_url(&resolved.base_url)
            } else {
                None
            },
            guest_id: std::sync::Mutex::new(guest),
            quiet,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn key_hint(&self) -> String {
        if self.api_key.is_empty() {
            "游客（未登录）".into()
        } else {
            crate::config::mask_key(&self.api_key)
        }
    }

    pub fn guest_id(&self) -> Option<String> {
        self.guest_id.lock().ok().and_then(|value| value.clone())
    }

    fn authorize(&self, request: RequestBuilder) -> RequestBuilder {
        if !self.api_key.is_empty() {
            return request.header(AUTHORIZATION, format!("Bearer {}", self.api_key));
        }
        let mut request = request;
        if let Some(origin) = &self.origin {
            request = request.header(ORIGIN, origin);
        }
        if let Some(id) = self.guest_id() {
            request = request.header(COOKIE, format!("ypdf_guest={id}"));
        }
        request
    }

    fn remember_guest_cookie(&self, response: &reqwest::Response) {
        if !self.api_key.is_empty() {
            return;
        }
        for value in response.headers().get_all(SET_COOKIE) {
            if let Some(id) = parse_guest_set_cookie(value.to_str().unwrap_or_default()) {
                if let Ok(mut slot) = self.guest_id.lock() {
                    *slot = Some(id);
                }
                break;
            }
        }
    }

    pub async fn get_json(&self, path: &str) -> Result<Value> {
        match self.get_json_once(path).await {
            Ok(value) => Ok(value),
            Err(err) if is_rate_limit_error(&err) => {
                self.retry_after_rate_limit().await;
                self.get_json_once(path).await
            }
            Err(err) => Err(with_login_hint(err)),
        }
    }

    async fn get_json_once(&self, path: &str) -> Result<Value> {
        let url = self.url(path);
        let response = self
            .authorize(self.http.get(&url))
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        self.remember_guest_cookie(&response);
        read_json(response).await
    }

    pub async fn post_form(&self, path: &str, fields: &[FormField]) -> Result<Value> {
        let prepared = self.prepare_fields(fields).await?;
        match self.post_prepared_once(path, &prepared).await {
            Ok(value) => Ok(value),
            Err(err) if is_rate_limit_error(&err) => {
                self.retry_after_rate_limit().await;
                self.post_prepared_once(path, &prepared).await
            }
            Err(err) => Err(with_login_hint(err)),
        }
    }

    async fn prepare_fields(&self, fields: &[FormField]) -> Result<Vec<PreparedField>> {
        let mut prepared = Vec::with_capacity(fields.len());
        for field in fields {
            let value = match &field.value {
                FieldValue::Text(value) => PreparedValue::Text(value.clone()),
                FieldValue::File(path) => {
                    let (key, name) = self.upload_direct(path).await?;
                    PreparedValue::File {
                        key,
                        name,
                        watermark: field.name == "watermarkImage",
                    }
                }
            };
            prepared.push(PreparedField {
                name: field.name.clone(),
                value,
            });
        }
        Ok(prepared)
    }

    async fn post_prepared_once(&self, path: &str, fields: &[PreparedField]) -> Result<Value> {
        let url = self.url(path);
        let mut form = Form::new();
        for field in fields {
            form = match &field.value {
                PreparedValue::Text(value) => form.text(field.name.clone(), value.clone()),
                PreparedValue::File {
                    key,
                    name,
                    watermark,
                } => {
                    if *watermark {
                        form = form.text("watermarkImageKey", key.clone());
                        form.text("watermarkImageName", name.clone())
                    } else {
                        form = form.text("fileKey", key.clone());
                        form.text("fileName", name.clone())
                    }
                }
            };
        }
        let response = self
            .authorize(self.http.post(&url).multipart(form))
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        self.remember_guest_cookie(&response);
        read_json(response).await
    }

    async fn upload_direct(&self, path: &Path) -> Result<(String, String)> {
        let meta = tokio::fs::metadata(path)
            .await
            .with_context(|| format!("读取文件失败: {}", path.display()))?;
        let bytes = meta.len();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("upload.bin")
            .to_string();
        let value = match self.presign_once(&name, bytes).await {
            Ok(value) => value,
            Err(err) if is_rate_limit_error(&err) => {
                self.retry_after_rate_limit().await;
                self.presign_once(&name, bytes).await?
            }
            Err(err) => return Err(err),
        };
        let upload_url = value
            .get("uploadUrl")
            .and_then(Value::as_str)
            .context("预签名响应缺少 uploadUrl")?;
        let key = value
            .get("key")
            .and_then(Value::as_str)
            .context("预签名响应缺少 key")?
            .to_string();
        let file = tokio::fs::File::open(path)
            .await
            .with_context(|| format!("打开文件失败: {}", path.display()))?;
        let sent = Arc::new(AtomicU64::new(0));
        let progress = sent.clone();
        let quiet = self.quiet;
        let label = name.clone();
        let stream = ReaderStream::new(file).inspect(move |chunk| {
            if let Ok(buf) = chunk {
                let n = progress.fetch_add(buf.len() as u64, Ordering::Relaxed) + buf.len() as u64;
                report_upload_progress(&label, n, bytes, quiet);
            }
        });
        let mut put = self
            .http
            .put(upload_url)
            .header(CONTENT_LENGTH, bytes)
            .body(Body::wrap_stream(stream));
        if let Some(headers) = value.get("headers").and_then(Value::as_object) {
            for (header_name, header) in headers {
                if header_name.eq_ignore_ascii_case("content-length") {
                    continue;
                }
                if let Some(header) = header.as_str() {
                    put = put.header(header_name, header);
                }
            }
        }
        let uploaded = put
            .send()
            .await
            .with_context(|| format!("PUT {upload_url}"))?;
        finish_upload_progress(&name, bytes, self.quiet);
        if !uploaded.status().is_success() {
            bail!(
                "直传对象存储失败 ({}): {}",
                uploaded.status(),
                uploaded.text().await.unwrap_or_default()
            );
        }
        Ok((key, name))
    }

    async fn presign_once(&self, name: &str, bytes: u64) -> Result<Value> {
        let presign_url = self.url("/uploads/presign");
        let body = serde_json::json!({
            "fileName": name,
            "contentType": content_type_for(name),
            "bytes": bytes,
        });
        let response = self
            .authorize(self.http.post(&presign_url).json(&body))
            .send()
            .await
            .with_context(|| format!("POST {presign_url}"))?;
        self.remember_guest_cookie(&response);
        read_json(response).await
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
        self.note(&format!("queued {} ({})", job.job_id, job.kind));
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
                status => self.note(&format!(
                    "  {job_id} {status} ({}s)",
                    started.elapsed().as_secs()
                )),
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
            .user_agent(user_agent())
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
        let path = resolve_out_path(out, hinted.as_deref().unwrap_or("result.bin"))?;
        ensure_parent_dir(&path)?;
        let mut file = tokio::fs::File::create(&path)
            .await
            .with_context(|| format!("写入失败: {}", path.display()))?;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("读取结果失败")?;
            file.write_all(&chunk)
                .await
                .with_context(|| format!("写入失败: {}", path.display()))?;
        }
        file.flush()
            .await
            .with_context(|| format!("写入失败: {}", path.display()))?;
        self.note(&format!("saved {}", path.display()));
        Ok(path)
    }

    async fn retry_after_rate_limit(&self) {
        self.note("1401 限流，10 秒后重试一次");
        sleep(Duration::from_secs(10)).await;
    }

    fn note(&self, message: &str) {
        if !self.quiet {
            eprintln!("{message}");
        }
    }

    fn url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            return path.to_string();
        }
        let path = path.trim_start_matches('/');
        format!("{}/{}", self.base_url.trim_end_matches('/'), path)
    }
}

fn report_upload_progress(name: &str, sent: u64, total: u64, quiet: bool) {
    if quiet {
        return;
    }
    let pct = sent.saturating_mul(100).checked_div(total).unwrap_or(100);
    eprint!(
        "\r上传 {name}  {} / {} ({pct}%)",
        format_bytes(sent as i64),
        format_bytes(total as i64)
    );
    let _ = std::io::stderr().flush();
}

fn finish_upload_progress(name: &str, total: u64, quiet: bool) {
    if quiet {
        return;
    }
    report_upload_progress(name, total, total, false);
    eprintln!();
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
