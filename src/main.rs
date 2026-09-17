mod client;
mod config;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use serde_json::Value;

use client::{Api, FieldValue, FormField};
use config::{ConfigFile, DEFAULT_BASE_URL, DEFAULT_PROFILE};

#[derive(Parser)]
#[command(
    name = "ypdf-cli",
    version,
    about = "YeahPDF CLI：用 API Key 调用 yeahpdf.com，等待任务完成后写出文件"
)]
struct Cli {
    /// 配置中的 profile 名
    #[arg(long, global = true, env = "YPDF_PROFILE")]
    profile: Option<String>,
    /// 覆盖已保存的 API Key
    #[arg(long, global = true, env = "YPDF_API_KEY")]
    api_key: Option<String>,
    /// 覆盖已保存的 Base URL，例如 https://www.yeahpdf.com 或 https://www.yeahpdf.com/api/v1
    #[arg(long, global = true, env = "YPDF_BASE_URL")]
    base_url: Option<String>,
    /// 结果文件或目录
    #[arg(short, long, global = true, default_value = ".")]
    out: PathBuf,
    /// 异步任务最长等待秒数
    #[arg(long, global = true, default_value_t = 1800)]
    timeout: u64,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 保存或切换 API Key / Base URL
    Auth {
        #[command(subcommand)]
        action: AuthCmd,
    },
    /// 查询当前 API 通道余量
    Quota,
    /// 查询当前 API 套餐权益
    Entitlements,
    /// 同步读取 PDF 元数据
    Inspect {
        file: PathBuf,
        #[arg(long)]
        password: Option<String>,
    },
    /// 任务查询与下载
    Jobs {
        #[command(subcommand)]
        action: JobsCmd,
    },
    /// 转换类接口
    Convert {
        #[command(subcommand)]
        action: ConvertCmd,
    },
    /// 合并多个 PDF
    Merge {
        files: Vec<PathBuf>,
        #[arg(long)]
        password: Option<String>,
    },
    /// 按 ranges 拆分
    Split {
        file: PathBuf,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        ranges: Option<String>,
    },
    /// 按 pages（fileIndex:page）重组
    Reorganize {
        files: Vec<PathBuf>,
        #[arg(long)]
        pages: String,
        #[arg(long)]
        password: Option<String>,
    },
    /// 设置打开密码
    Encrypt {
        file: PathBuf,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        user_password: String,
        #[arg(long)]
        owner_password: Option<String>,
    },
    /// 移除加密
    Decrypt {
        file: PathBuf,
        #[arg(long)]
        password: String,
    },
    /// 写入可见签名
    Sign {
        file: PathBuf,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        sign_name: String,
        #[arg(long)]
        sign_reason: Option<String>,
        #[arg(long)]
        sign_location: Option<String>,
        #[arg(long, default_value = "last")]
        sign_page: String,
    },
    /// 加水印
    Watermark {
        file: PathBuf,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        text: Option<String>,
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        image: Option<PathBuf>,
    },
    /// 去水印
    Unwatermark {
        file: PathBuf,
        #[arg(long)]
        password: Option<String>,
    },
    /// 添加页码
    PageNumbers {
        file: PathBuf,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        style: Option<String>,
        #[arg(long)]
        position: Option<String>,
        #[arg(long)]
        font_size: Option<String>,
    },
    /// 按页旋转
    Flip {
        file: PathBuf,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        spec: String,
    },
    /// 按边距裁剪
    Crop {
        file: PathBuf,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        spec: String,
    },
    /// 压缩 PDF
    Compress {
        file: PathBuf,
        #[arg(long)]
        password: Option<String>,
        #[arg(long, default_value = "recommended")]
        level: String,
        #[arg(long)]
        jpeg_quality: Option<String>,
        #[arg(long)]
        compress_images: Option<String>,
        #[arg(long)]
        lossless_images: Option<String>,
        #[arg(long)]
        lossless_dpi: Option<String>,
        #[arg(long)]
        flate_level: Option<String>,
    },
    /// 修复 PDF
    Repair {
        file: PathBuf,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        spec: Option<String>,
    },
    /// 图片转 PDF
    ImageToPdf {
        files: Vec<PathBuf>,
        #[arg(long)]
        format: Option<String>,
        #[arg(long)]
        dpi: Option<String>,
        #[arg(long)]
        spec: Option<String>,
    },
    /// Excel 转 PDF
    ExcelToPdf {
        file: PathBuf,
        #[arg(long)]
        format: Option<String>,
    },
}

#[derive(Subcommand)]
enum AuthCmd {
    /// 用 /quota 校验 Key 与 Base URL，通过后才写入配置
    Login {
        #[arg(long)]
        api_key: Option<String>,
        #[arg(long)]
        base_url: Option<String>,
        #[arg(long)]
        profile: Option<String>,
    },
    /// 查看当前配置（Key 仅显示前缀）
    Show,
    /// 切换默认 profile
    Use { profile: String },
}

#[derive(Subcommand)]
enum JobsCmd {
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: i64,
        #[arg(long, default_value_t = 0)]
        offset: i64,
    },
    Get {
        id: String,
    },
    Wait {
        id: String,
    },
    Result {
        id: String,
    },
}

#[derive(Subcommand)]
enum ConvertCmd {
    Pages {
        file: PathBuf,
        #[command(flatten)]
        common: PdfIn,
        #[arg(long, default_value = "144")]
        dpi: String,
        #[arg(long, default_value = "png")]
        format: String,
        #[arg(long)]
        ranges: Option<String>,
        #[arg(long)]
        name_prefix: Option<String>,
        #[arg(long)]
        name_suffix: Option<String>,
    },
    Long {
        file: PathBuf,
        #[command(flatten)]
        common: PdfIn,
        #[arg(long, default_value = "144")]
        dpi: String,
        #[arg(long, default_value = "png")]
        format: String,
        #[arg(long)]
        ranges: Option<String>,
        #[arg(long)]
        gap: Option<String>,
    },
    Word {
        file: PathBuf,
        #[command(flatten)]
        common: PdfIn,
        #[command(flatten)]
        analysis: Analysis,
    },
    Markdown {
        file: PathBuf,
        #[command(flatten)]
        common: PdfIn,
        #[command(flatten)]
        analysis: Analysis,
    },
    Html {
        file: PathBuf,
        #[command(flatten)]
        common: PdfIn,
        #[command(flatten)]
        analysis: Analysis,
    },
    Excel {
        file: PathBuf,
        #[command(flatten)]
        common: PdfIn,
        #[command(flatten)]
        analysis: Analysis,
    },
}

#[derive(Args)]
struct PdfIn {
    #[arg(long)]
    password: Option<String>,
}

#[derive(Args)]
struct Analysis {
    #[arg(long)]
    ocr: bool,
    #[arg(long)]
    layout_analysis: bool,
    #[arg(long)]
    table_analysis: bool,
    #[arg(long)]
    formula_analysis: bool,
    #[arg(long)]
    model_select: Option<String>,
}

struct Ctx {
    profile: Option<String>,
    api_key: Option<String>,
    base_url: Option<String>,
    out: PathBuf,
    timeout: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let Cli {
        profile,
        api_key,
        base_url,
        out,
        timeout,
        command,
    } = Cli::parse();
    let ctx = Ctx {
        profile,
        api_key,
        base_url,
        out,
        timeout,
    };
    match command {
        Command::Auth { action } => run_auth(action, &ctx).await,
        other => {
            let api = connect(&ctx)?;
            dispatch(other, &ctx, &api).await
        }
    }
}

fn connect(ctx: &Ctx) -> Result<Api> {
    let file = ConfigFile::load()?;
    let resolved = config::resolve(
        &file,
        ctx.profile.as_deref(),
        ctx.api_key.clone(),
        ctx.base_url.clone(),
    )?;
    Api::new(&resolved)
}

async fn run_auth(action: AuthCmd, ctx: &Ctx) -> Result<()> {
    let mut file = ConfigFile::load()?;
    match action {
        AuthCmd::Login {
            api_key,
            base_url,
            profile,
        } => {
            let name = profile
                .or(ctx.profile.clone())
                .unwrap_or_else(|| DEFAULT_PROFILE.into());
            let key = api_key
                .or(ctx.api_key.clone())
                .or_else(|| std::env::var("YPDF_API_KEY").ok())
                .filter(|value| !value.trim().is_empty());
            let key = match key {
                Some(value) => value,
                None => rpassword::prompt_password("API Key: ").context("读取 API Key 失败")?,
            };
            let key = key.trim().to_string();
            if key.is_empty() {
                bail!("未输入 API Key");
            }
            if !key.starts_with("ypdf_") {
                bail!("API Key 应以 ypdf_ 开头");
            }
            let url = config::normalize_base_url(
                base_url
                    .as_deref()
                    .or(ctx.base_url.as_deref())
                    .unwrap_or(DEFAULT_BASE_URL),
            );
            let pending = config::Resolved {
                profile: name.clone(),
                api_key: key.clone(),
                base_url: url.clone(),
            };
            let api = Api::new(&pending)?;
            let quota = api
                .get_json("/quota")
                .await
                .with_context(|| format!("校验失败，未写入配置 ({name})"))?;
            file.upsert(&name, Some(key), Some(url));
            let path = file.save()?;
            println!("saved {} ({})", path.display(), name);
            println!("baseUrl {}", api.base_url());
            println!("apiKey  {}", api.key_hint());
            print_json(&quota)?;
        }
        AuthCmd::Show => {
            let resolved = config::resolve(
                &file,
                ctx.profile.as_deref(),
                ctx.api_key.clone(),
                ctx.base_url.clone(),
            );
            println!("config  {}", config::config_path()?.display());
            println!(
                "default {}",
                if file.default_profile.is_empty() {
                    DEFAULT_PROFILE
                } else {
                    file.default_profile.as_str()
                }
            );
            if file.profiles.is_empty() {
                println!("profiles (empty)");
            } else {
                for (name, profile) in &file.profiles {
                    let mark = if name == &file.default_profile {
                        "*"
                    } else {
                        " "
                    };
                    println!(
                        "{mark} {name}: {}  {}",
                        config::mask_key(&profile.api_key),
                        if profile.base_url.is_empty() {
                            DEFAULT_BASE_URL
                        } else {
                            profile.base_url.as_str()
                        }
                    );
                }
            }
            if let Ok(resolved) = resolved {
                println!("active  {}  {}", resolved.profile, resolved.base_url);
            }
        }
        AuthCmd::Use { profile } => {
            if !file.profiles.contains_key(&profile) {
                bail!("profile 不存在: {profile}");
            }
            file.default_profile = profile.clone();
            let path = file.save()?;
            println!("default {} ({})", profile, path.display());
        }
    }
    Ok(())
}

async fn dispatch(command: Command, ctx: &Ctx, api: &Api) -> Result<()> {
    let timeout = Duration::from_secs(ctx.timeout);
    let out = ctx.out.as_path();
    match command {
        Command::Quota => print_json(&api.get_json("/quota").await?)?,
        Command::Entitlements => print_json(&api.get_json("/entitlements").await?)?,
        Command::Inspect { file, password } => {
            let mut fields = vec![file_field("file", file)];
            push_text(&mut fields, "password", password);
            print_json(&api.post_form("/pdf/inspect", &fields).await?)?;
        }
        Command::Jobs { action } => match action {
            JobsCmd::List {
                status,
                limit,
                offset,
            } => {
                let mut path = format!("/jobs?limit={limit}&offset={offset}");
                if let Some(status) = status {
                    path.push_str(&format!("&status={status}"));
                }
                print_json(&api.get_json(&path).await?)?;
            }
            JobsCmd::Get { id } => print_json(&api.get_json(&format!("/jobs/{id}")).await?)?,
            JobsCmd::Wait { id } => {
                let job = api.wait_job(&id, timeout).await?;
                print_json(&serde_json::to_value(&job_to_value(&job)?)?)?;
                if job.status == "succeeded" {
                    api.download_result(&id, out).await?;
                } else if let Some(error) = job.error {
                    bail!("{error}");
                }
            }
            JobsCmd::Result { id } => {
                api.download_result(&id, out).await?;
            }
        },
        Command::Convert { action } => match action {
            ConvertCmd::Pages {
                file,
                common,
                dpi,
                format,
                ranges,
                name_prefix,
                name_suffix,
            } => {
                let mut fields = pdf_file(file, common.password);
                push_text(&mut fields, "dpi", Some(dpi));
                push_text(&mut fields, "format", Some(format));
                push_text(&mut fields, "ranges", ranges);
                push_text(&mut fields, "namePrefix", name_prefix);
                push_text(&mut fields, "nameSuffix", name_suffix);
                api.run_job("/pdf/convert/pages", &fields, out, timeout)
                    .await?;
            }
            ConvertCmd::Long {
                file,
                common,
                dpi,
                format,
                ranges,
                gap,
            } => {
                let mut fields = pdf_file(file, common.password);
                push_text(&mut fields, "dpi", Some(dpi));
                push_text(&mut fields, "format", Some(format));
                push_text(&mut fields, "ranges", ranges);
                push_text(&mut fields, "gap", gap);
                api.run_job("/pdf/convert/long", &fields, out, timeout)
                    .await?;
            }
            ConvertCmd::Word {
                file,
                common,
                analysis,
            } => {
                api.run_job(
                    "/pdf/convert/word",
                    &analysis_fields(file, common.password, analysis),
                    out,
                    timeout,
                )
                .await?;
            }
            ConvertCmd::Markdown {
                file,
                common,
                analysis,
            } => {
                api.run_job(
                    "/pdf/convert/markdown",
                    &analysis_fields(file, common.password, analysis),
                    out,
                    timeout,
                )
                .await?;
            }
            ConvertCmd::Html {
                file,
                common,
                analysis,
            } => {
                api.run_job(
                    "/pdf/convert/html",
                    &analysis_fields(file, common.password, analysis),
                    out,
                    timeout,
                )
                .await?;
            }
            ConvertCmd::Excel {
                file,
                common,
                analysis,
            } => {
                api.run_job(
                    "/pdf/convert/excel",
                    &analysis_fields(file, common.password, analysis),
                    out,
                    timeout,
                )
                .await?;
            }
        },
        Command::Merge { files, password } => {
            if files.len() < 2 {
                bail!("merge 至少需要 2 个文件");
            }
            let mut fields = files
                .into_iter()
                .map(|file| file_field("files", file))
                .collect();
            push_text(&mut fields, "password", password);
            api.run_job("/pdf/merge", &fields, out, timeout).await?;
        }
        Command::Split {
            file,
            password,
            ranges,
        } => {
            let mut fields = pdf_file(file, password);
            push_text(&mut fields, "ranges", ranges);
            api.run_job("/pdf/split", &fields, out, timeout).await?;
        }
        Command::Reorganize {
            files,
            pages,
            password,
        } => {
            let mut fields: Vec<FormField> = files
                .into_iter()
                .map(|file| file_field("files", file))
                .collect();
            push_text(&mut fields, "pages", Some(pages));
            push_text(&mut fields, "password", password);
            api.run_job("/pdf/reorganize", &fields, out, timeout)
                .await?;
        }
        Command::Encrypt {
            file,
            password,
            user_password,
            owner_password,
        } => {
            let mut fields = pdf_file(file, password);
            push_text(&mut fields, "userPassword", Some(user_password));
            push_text(&mut fields, "ownerPassword", owner_password);
            api.run_job("/pdf/encrypt", &fields, out, timeout).await?;
        }
        Command::Decrypt { file, password } => {
            api.run_job(
                "/pdf/decrypt",
                &pdf_file(file, Some(password)),
                out,
                timeout,
            )
            .await?;
        }
        Command::Sign {
            file,
            password,
            sign_name,
            sign_reason,
            sign_location,
            sign_page,
        } => {
            let mut fields = pdf_file(file, password);
            push_text(&mut fields, "signName", Some(sign_name));
            push_text(&mut fields, "signReason", sign_reason);
            push_text(&mut fields, "signLocation", sign_location);
            push_text(&mut fields, "signPage", Some(sign_page));
            api.run_job("/pdf/sign", &fields, out, timeout).await?;
        }
        Command::Watermark {
            file,
            password,
            text,
            spec,
            image,
        } => {
            let mut fields = pdf_file(file, password);
            if let Some(image) = image {
                fields.push(file_field("watermarkImage", image));
            }
            push_text(&mut fields, "watermarkSpec", spec);
            push_text(&mut fields, "watermarkText", text);
            api.run_job("/pdf/watermark", &fields, out, timeout).await?;
        }
        Command::Unwatermark { file, password } => {
            api.run_job("/pdf/unwatermark", &pdf_file(file, password), out, timeout)
                .await?;
        }
        Command::PageNumbers {
            file,
            password,
            spec,
            style,
            position,
            font_size,
        } => {
            let mut fields = pdf_file(file, password);
            push_text(&mut fields, "pageNumberSpec", spec);
            push_text(&mut fields, "pageNumberStyle", style);
            push_text(&mut fields, "pageNumberPosition", position);
            push_text(&mut fields, "pageNumberFontSize", font_size);
            api.run_job("/pdf/pagenumbers", &fields, out, timeout)
                .await?;
        }
        Command::Flip {
            file,
            password,
            spec,
        } => {
            let mut fields = pdf_file(file, password);
            push_text(&mut fields, "flipSpec", Some(spec));
            api.run_job("/pdf/flip", &fields, out, timeout).await?;
        }
        Command::Crop {
            file,
            password,
            spec,
        } => {
            let mut fields = pdf_file(file, password);
            push_text(&mut fields, "cropSpec", Some(spec));
            api.run_job("/pdf/crop", &fields, out, timeout).await?;
        }
        Command::Compress {
            file,
            password,
            level,
            jpeg_quality,
            compress_images,
            lossless_images,
            lossless_dpi,
            flate_level,
        } => {
            let mut fields = pdf_file(file, password);
            push_text(&mut fields, "compressLevel", Some(level));
            push_text(&mut fields, "jpegQuality", jpeg_quality);
            push_text(&mut fields, "compressImages", compress_images);
            push_text(&mut fields, "losslessImages", lossless_images);
            push_text(&mut fields, "losslessDpi", lossless_dpi);
            push_text(&mut fields, "flateLevel", flate_level);
            api.run_job("/pdf/compress", &fields, out, timeout).await?;
        }
        Command::Repair {
            file,
            password,
            spec,
        } => {
            let mut fields = pdf_file(file, password);
            push_text(&mut fields, "repairSpec", spec);
            api.run_job("/pdf/repair", &fields, out, timeout).await?;
        }
        Command::ImageToPdf {
            files,
            format,
            dpi,
            spec,
        } => {
            if files.is_empty() {
                bail!("至少需要 1 张图片");
            }
            let mut fields: Vec<FormField> = files
                .into_iter()
                .map(|file| file_field("files", file))
                .collect();
            push_text(&mut fields, "format", format);
            push_text(&mut fields, "dpi", dpi);
            push_text(&mut fields, "imagePdfSpec", spec);
            api.run_job("/pdf/image-to-pdf", &fields, out, timeout)
                .await?;
        }
        Command::ExcelToPdf { file, format } => {
            let mut fields = vec![file_field("file", file)];
            push_text(&mut fields, "format", format);
            api.run_job("/pdf/excel-to-pdf", &fields, out, timeout)
                .await?;
        }
        Command::Auth { .. } => unreachable!(),
    }
    Ok(())
}

fn pdf_file(file: PathBuf, password: Option<String>) -> Vec<FormField> {
    let mut fields = vec![file_field("file", file)];
    push_text(&mut fields, "password", password);
    fields
}

fn analysis_fields(file: PathBuf, password: Option<String>, analysis: Analysis) -> Vec<FormField> {
    let mut fields = pdf_file(file, password);
    if analysis.ocr {
        push_text(&mut fields, "ocr", Some("true".into()));
    }
    if analysis.layout_analysis {
        push_text(&mut fields, "layoutAnalysis", Some("true".into()));
    }
    if analysis.table_analysis {
        push_text(&mut fields, "tableAnalysis", Some("true".into()));
    }
    if analysis.formula_analysis {
        push_text(&mut fields, "formulaAnalysis", Some("true".into()));
    }
    push_text(&mut fields, "modelSelect", analysis.model_select);
    fields
}

fn file_field(name: &str, path: PathBuf) -> FormField {
    FormField {
        name: name.into(),
        value: FieldValue::File(path),
    }
}

fn push_text(fields: &mut Vec<FormField>, name: &str, value: Option<String>) {
    if let Some(value) = value.filter(|item| !item.is_empty()) {
        fields.push(FormField {
            name: name.into(),
            value: FieldValue::Text(value),
        });
    }
}

fn print_json(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn job_to_value(job: &client::JobView) -> Result<Value> {
    Ok(serde_json::json!({
        "jobId": job.job_id,
        "kind": job.kind,
        "status": job.status,
        "error": job.error,
        "data": job.data,
    }))
}
