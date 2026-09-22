use serde_json::Value;

pub fn user_agent() -> String {
    format!("ypdf/{}", env!("CARGO_PKG_VERSION"))
}

pub fn is_rate_limit_error(err: &anyhow::Error) -> bool {
    error_code(&err.to_string()) == Some("1401")
}

pub fn is_pending_upload_error(err: &anyhow::Error) -> bool {
    matches!(
        error_code(&err.to_string()),
        Some("1508" | "1509" | "1510" | "1511")
    )
}

pub fn needs_login_hint(err: &anyhow::Error) -> bool {
    let text = err.to_string();
    error_code(&text).is_some_and(|code| code.starts_with("13")) || text.contains("需要登录")
}

pub fn with_login_hint(err: anyhow::Error) -> anyhow::Error {
    if needs_login_hint(&err) {
        err.context("游客额度已用完或该功能需要登录。运行 `ypdf auth login` 使用 API 套餐")
    } else {
        err
    }
}

pub fn with_pending_hint(err: anyhow::Error) -> anyhow::Error {
    if is_pending_upload_error(&err) {
        err.context(
            "这是未提交的上传凭证占满，不是 QPS 限流。不要立刻再传新文件；入队失败后站点会释放本次凭证，更早的凭证需等待过期",
        )
    } else {
        err
    }
}

pub fn decorate_api_error(err: anyhow::Error) -> anyhow::Error {
    with_pending_hint(with_login_hint(err))
}

pub fn error_code(text: &str) -> Option<&str> {
    let code = text.split_whitespace().next()?;
    if code.len() == 4 && code.chars().all(|ch| ch.is_ascii_digit()) {
        Some(code)
    } else {
        None
    }
}

pub fn format_bytes(bytes: i64) -> String {
    if bytes < 0 {
        return "不限".into();
    }
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else if value >= 100.0 {
        format!("{:.0} {}", value, UNITS[unit])
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

pub fn format_quota(value: &Value) -> String {
    let mut lines = Vec::new();
    let plan_name = text(value, "planName");
    let plan_code = text(value, "planCode");
    if !plan_name.is_empty() || !plan_code.is_empty() {
        lines.push(format!(
            "套餐    {} ({})",
            or_dash(&plan_name),
            or_dash(&plan_code)
        ));
    }
    if plan_code == "guest" {
        lines.push("身份    未登录游客（与网站未登录共用出口 IP 额度）".into());
    }
    let credits = int(value, "credits");
    let consumed = int(value, "creditsConsumed");
    if credits.is_some() || consumed.is_some() {
        lines.push(format!(
            "积分    {}（今日已用 {}）",
            credits.unwrap_or(0),
            consumed.unwrap_or(0)
        ));
    }
    push_bucket(&mut lines, "任务", value.get("jobs"), CountUnit::Count);
    push_bucket(&mut lines, "页数", value.get("pages"), CountUnit::Count);
    push_bucket(&mut lines, "文件", value.get("files"), CountUnit::Count);
    push_bucket(
        &mut lines,
        "上传",
        value.get("uploadBytes"),
        CountUnit::Bytes,
    );
    push_bucket(
        &mut lines,
        "进行中",
        value.get("activeJobs"),
        CountUnit::Count,
    );
    if let Some(qps) = value.get("qps") {
        let limit = int(qps, "limit").unwrap_or(0);
        let burst = int(qps, "burst").unwrap_or(0);
        lines.push(format!("QPS     {limit}（突发 {burst}）"));
    }
    if let Some(ms) = int(value, "resetsAt") {
        lines.push(format!("重置    {}", format_utc_millis(ms)));
    }
    if lines.is_empty() {
        lines.push("额度    （响应缺少常用字段）".into());
    }
    lines.push(String::new());
    lines.join("\n")
}

pub fn format_entitlements(value: &Value) -> String {
    let mut lines = Vec::new();
    let plan = value.get("plan").unwrap_or(value);
    let name = text(plan, "name");
    let code = text(plan, "code");
    if !name.is_empty() || !code.is_empty() {
        lines.push(format!("套餐    {} ({})", or_dash(&name), or_dash(&code)));
    }
    if let Some(membership) = value.get("membership") {
        if !membership.is_null() {
            let plan_code = text(membership, "planCode");
            let ends = int(membership, "endsAt")
                .map(format_utc_millis)
                .unwrap_or_else(|| "—".into());
            lines.push(format!("会员    {}，到期 {}", or_dash(&plan_code), ends));
        }
    }
    let limits = value.get("limits").unwrap_or(plan);
    lines.push(format!(
        "日限额  任务 {} / 页 {} / 文件 {} / 上传 {}",
        limit_label(int(limits, "dailyJobLimit").unwrap_or(-1)),
        limit_label(int(limits, "dailyPageLimit").unwrap_or(-1)),
        limit_label(int(limits, "dailyFileLimit").unwrap_or(-1)),
        mb_label(int(limits, "dailyUploadMb").unwrap_or(-1)),
    ));
    if let Some(queue) = text_opt(limits, "queue") {
        lines.push(format!("队列    {queue}"));
    }
    let enabled = enabled_kinds(value.get("entitlements"));
    if enabled.is_empty() {
        lines.push("开通    （无）".into());
    } else {
        lines.push(format!("开通    {}", enabled.join(", ")));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn enabled_kinds(entitlements: Option<&Value>) -> Vec<String> {
    let Some(map) = entitlements.and_then(Value::as_object) else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(kind, item)| {
            if item.get("enabled").and_then(Value::as_bool) == Some(true) {
                Some(kind.clone())
            } else {
                None
            }
        })
        .collect()
}

enum CountUnit {
    Count,
    Bytes,
}

fn push_bucket(lines: &mut Vec<String>, label: &str, bucket: Option<&Value>, unit: CountUnit) {
    let Some(bucket) = bucket else {
        return;
    };
    let used = int(bucket, "used").unwrap_or(0);
    let limit = int(bucket, "limit").unwrap_or(0);
    let remaining = int(bucket, "remaining");
    let unlimited = remaining == Some(-1) || limit < 0;
    let (used_s, limit_s, remain_s) = match unit {
        CountUnit::Count => (
            used.to_string(),
            if unlimited {
                "不限".into()
            } else {
                limit.to_string()
            },
            remaining
                .filter(|value| *value >= 0)
                .map(|value| value.to_string()),
        ),
        CountUnit::Bytes => (
            format_bytes(used),
            if unlimited {
                "不限".into()
            } else {
                format_bytes(limit)
            },
            remaining.filter(|value| *value >= 0).map(format_bytes),
        ),
    };
    if let Some(remain_s) = remain_s {
        lines.push(format!(
            "{label:<6} {used_s} / {limit_s}（剩余 {remain_s}）"
        ));
    } else {
        lines.push(format!("{label:<6} {used_s} / {limit_s}"));
    }
}

fn limit_label(limit: i64) -> String {
    if limit <= 0 {
        "不限".into()
    } else {
        limit.to_string()
    }
}

fn mb_label(mb: i64) -> String {
    if mb <= 0 {
        "不限".into()
    } else {
        format!("{mb} MB")
    }
}

fn text(value: &Value, key: &str) -> String {
    text_opt(value, key).unwrap_or_default()
}

fn text_opt(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
}

fn int(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn or_dash(value: &str) -> &str {
    if value.is_empty() {
        "—"
    } else {
        value
    }
}

fn format_utc_millis(ms: i64) -> String {
    if ms <= 0 {
        return "未知".into();
    }
    let days = ms.div_euclid(86_400_000);
    let rem = ms.rem_euclid(86_400_000);
    let hour = rem / 3_600_000;
    let min = (rem % 3_600_000) / 60_000;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{min:02} UTC")
}

/// Howard Hinnant, days since Unix epoch → civil date.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn user_agent_uses_crate_version() {
        assert_eq!(user_agent(), format!("ypdf/{}", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn rate_limit_is_only_1401() {
        assert!(is_rate_limit_error(&anyhow::anyhow!(
            "1401 too many requests"
        )));
        assert!(!is_rate_limit_error(&anyhow::anyhow!(
            "1301 今日任务已用完"
        )));
        assert!(!is_rate_limit_error(&anyhow::anyhow!(
            "1508 未完成的上传过多：本账号已有 8 个未提交文件（上限 8）"
        )));
        assert!(!is_rate_limit_error(&anyhow::anyhow!("1101 unauthorized")));
        assert!(is_pending_upload_error(&anyhow::anyhow!(
            "1508 未完成的上传过多：本账号已有 8 个未提交文件（上限 8）"
        )));
        assert!(is_pending_upload_error(&anyhow::anyhow!(
            "1510 当前网络未完成的上传过多"
        )));
        assert!(!is_pending_upload_error(&anyhow::anyhow!(
            "1401 too many requests"
        )));
        let hinted = with_pending_hint(anyhow::anyhow!(
            "1508 未完成的上传过多：本账号已有 8 个未提交文件（上限 8）"
        ));
        assert!(hinted.to_string().contains("不是 QPS 限流"));
        assert!(format!("{hinted:#}").contains("1508"));
        assert_eq!(error_code("1401 too many"), Some("1401"));
        assert_eq!(error_code("HTTP 429"), None);
    }

    #[test]
    fn login_hint_uses_codes_not_unlimited_copy() {
        assert!(needs_login_hint(&anyhow::anyhow!("1306 今日页数不足")));
        assert!(needs_login_hint(&anyhow::anyhow!("「ocr」需要登录后使用")));
        assert!(!needs_login_hint(&anyhow::anyhow!(
            "1401 too many requests"
        )));
        assert!(!needs_login_hint(&anyhow::anyhow!("页数 不限")));
        let wrapped = with_login_hint(anyhow::anyhow!("1306 今日页数不足"));
        assert!(wrapped.to_string().contains("auth login"));
    }

    #[test]
    fn quota_summary_is_human() {
        let text = format_quota(&json!({
            "planCode": "pro",
            "planName": "API 专业版",
            "credits": 120,
            "creditsConsumed": 3,
            "jobs": { "used": 2, "limit": 50, "remaining": 48 },
            "pages": { "used": 10, "limit": 0, "remaining": -1 },
            "files": { "used": 1, "limit": 20, "remaining": 19 },
            "uploadBytes": { "used": 1_572_864, "limit": 524_288_000, "remaining": 522_715_136 },
            "activeJobs": { "used": 0, "limit": 3, "remaining": 3 },
            "qps": { "limit": 2, "burst": 4 },
            "resetsAt": 1_790_000_000_000_i64
        }));
        assert!(!text.contains("未登录游客"));
        assert!(text.contains("API 专业版"));
        assert!(text.contains("pro"));
        assert!(text.contains("剩余 48"));
        assert!(text.contains("不限"));
        assert!(text.contains("QPS"));
        assert!(text.contains("UTC"));
        assert!(!text.trim_start().starts_with('{'));
        let guest = format_quota(&json!({
            "planCode": "guest",
            "planName": "游客",
            "jobs": { "used": 0, "limit": 0, "remaining": -1 }
        }));
        assert!(guest.contains("未登录游客"));
        assert!(guest.contains("不限"));
    }

    #[test]
    fn entitlements_lists_enabled_kinds() {
        let text = format_entitlements(&json!({
            "plan": { "code": "free", "name": "API 免费版" },
            "limits": {
                "dailyJobLimit": 10,
                "dailyPageLimit": 0,
                "dailyFileLimit": 5,
                "dailyUploadMb": 50,
                "queue": "standard"
            },
            "entitlements": {
                "merge": { "enabled": true },
                "pdfCreate": { "enabled": false },
                "split": { "enabled": true }
            }
        }));
        assert!(text.contains("API 免费版"));
        assert!(text.contains("merge"));
        assert!(text.contains("split"));
        assert!(!text.contains("pdfCreate"));
        assert!(text.contains("不限"));
        assert!(text.contains("50 MB"));
    }

    #[test]
    fn formats_byte_sizes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(-1), "不限");
    }
}
