mod client;
mod config;
mod display;
mod spec_help;
mod uninstall;
mod upgrade;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use serde_json::Value;

use client::{Api, FieldValue, FormField};
use config::{ConfigFile, DEFAULT_BASE_URL, DEFAULT_PROFILE};

#[derive(Parser)]
#[command(
    name = "ypdf",
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
    /// 额度类命令输出接口原 JSON
    #[arg(long, global = true)]
    json: bool,
    /// 不向 stderr 打印上传 / 轮询进度
    #[arg(long, global = true)]
    quiet: bool,
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
    /// 检查或安装 GitHub 上的新版本
    Upgrade {
        /// 只检查，不下载、不覆盖
        #[arg(long)]
        check: bool,
    },
    /// 删除当前安装的 ypdf
    Uninstall {
        /// 同时删除本地配置（含已保存的 API Key）
        #[arg(long)]
        purge: bool,
    },
    /// 查看某个命令的 JSON --spec 字段（不连网）
    SpecHelp {
        command: Option<String>,
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
        /// 模板，默认 ${p}/${n}
        #[arg(long)]
        style: Option<String>,
        /// footer-center / footer-left / footer-right / header-*
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
        spec: Option<String>,
        /// 90 | 180 | 270；省略 --spec 时必填
        #[arg(long)]
        rotate: Option<i32>,
        /// 1-based，如 1,3-4；省略则全书
        #[arg(long)]
        pages: Option<String>,
    },
    /// 按边距裁剪
    Crop {
        file: PathBuf,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        spec: Option<String>,
        /// 四边相同的裁剪比例 0–0.45
        #[arg(long)]
        inset: Option<f32>,
        #[arg(long)]
        top: Option<f32>,
        #[arg(long)]
        right: Option<f32>,
        #[arg(long)]
        bottom: Option<f32>,
        #[arg(long)]
        left: Option<f32>,
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
    /// 删除本地 profile（不吊销站点 Key）
    Logout {
        /// 要删除的 profile，默认当前
        profile: Option<String>,
        /// 清空全部本地 profile
        #[arg(long)]
        all: bool,
    },
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
    json: bool,
    quiet: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let Cli {
        profile,
        api_key,
        base_url,
        out,
        timeout,
        json,
        quiet,
        command,
    } = Cli::parse();
    let ctx = Ctx {
        profile,
        api_key,
        base_url,
        out,
        timeout,
        json,
        quiet,
    };
    match command {
        Command::Auth { action } => run_auth(action, &ctx).await,
        Command::Upgrade { check } => upgrade::run(check).await,
        Command::Uninstall { purge } => uninstall::run(purge),
        Command::SpecHelp { command } => spec_help::run(command),
        other => {
            let (api, file) = connect(&ctx)?;
            let result = dispatch(other, &ctx, &api).await;
            persist_guest(file, &api);
            result
        }
    }
}

fn connect(ctx: &Ctx) -> Result<(Api, ConfigFile)> {
    let file = ConfigFile::load()?;
    let resolved = config::resolve(
        &file,
        ctx.profile.as_deref(),
        ctx.api_key.clone(),
        ctx.base_url.clone(),
    )?;
    let stored = (!file.guest_id.is_empty()).then(|| file.guest_id.clone());
    let api = Api::new(&resolved, ctx.quiet, stored)?;
    Ok((api, file))
}

fn persist_guest(mut file: ConfigFile, api: &Api) {
    if let Some(id) = api.guest_id() {
        let _ = file.persist_guest_id(&id);
    }
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
            let api = Api::new(&pending, ctx.quiet, None)?;
            let quota = api
                .get_json("/quota")
                .await
                .with_context(|| format!("校验失败，未写入配置 ({name})"))?;
            file.upsert(&name, Some(key), Some(url));
            let path = file.save()?;
            println!("saved {} ({})", path.display(), name);
            println!("baseUrl {}", api.base_url());
            println!("apiKey  {}", api.key_hint());
            emit_quota(&quota, ctx.json)?;
        }
        AuthCmd::Show => {
            let resolved = config::resolve(
                &file,
                ctx.profile.as_deref(),
                ctx.api_key.clone(),
                ctx.base_url.clone(),
            );
            println!("config  {}", config::config_path()?.display());
            if config::valid_guest_id(&file.guest_id) {
                println!("guest   {}", file.guest_id);
            }
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
                if resolved.api_key.is_empty() {
                    println!("active  {}  {}  guest", resolved.profile, resolved.base_url);
                } else {
                    println!("active  {}  {}", resolved.profile, resolved.base_url);
                }
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
        AuthCmd::Logout { profile, all } => {
            if all {
                file.clear_profiles();
                let path = file.save()?;
                println!("cleared {}", path.display());
                return Ok(());
            }
            let name = profile.or(ctx.profile.clone()).unwrap_or_else(|| {
                if file.default_profile.is_empty() {
                    DEFAULT_PROFILE.into()
                } else {
                    file.default_profile.clone()
                }
            });
            file.remove_profile(&name)?;
            let path = file.save()?;
            println!("removed {name} ({})", path.display());
            if file.profiles.is_empty() {
                println!("profiles (empty)");
            } else {
                println!("default {}", file.default_profile);
            }
        }
    }
    Ok(())
}

async fn dispatch(command: Command, ctx: &Ctx, api: &Api) -> Result<()> {
    let timeout = Duration::from_secs(ctx.timeout);
    let out = ctx.out.as_path();
    match command {
        Command::Quota => emit_quota(&api.get_json("/quota").await?, ctx.json)?,
        Command::Entitlements => {
            emit_entitlements(&api.get_json("/entitlements").await?, ctx.json)?
        }
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
            rotate,
            pages,
        } => {
            let mut fields = pdf_file(file, password);
            push_text(&mut fields, "flipSpec", Some(flip_spec(spec, rotate, pages)?));
            api.run_job("/pdf/flip", &fields, out, timeout).await?;
        }
        Command::Crop {
            file,
            password,
            spec,
            inset,
            top,
            right,
            bottom,
            left,
        } => {
            let mut fields = pdf_file(file, password);
            push_text(
                &mut fields,
                "cropSpec",
                Some(crop_spec(spec, inset, top, right, bottom, left)?),
            );
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
        Command::Auth { .. }
        | Command::Upgrade { .. }
        | Command::Uninstall { .. }
        | Command::SpecHelp { .. } => {
            unreachable!()
        }
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

fn flip_spec(
    spec: Option<String>,
    rotate: Option<i32>,
    pages: Option<String>,
) -> Result<String> {
    if let Some(spec) = spec.filter(|value| !value.trim().is_empty()) {
        return Ok(spec);
    }
    let rotate = rotate.unwrap_or(0);
    if ![90, 180, 270, -90, -180, -270].contains(&rotate) {
        bail!("请使用 --rotate 90|180|270，或改用 --spec（ypdf spec-help flip）");
    }
    if let Some(pages) = pages.filter(|value| !value.trim().is_empty()) {
        let pages: Vec<Value> = parse_page_list(&pages)?
            .into_iter()
            .map(|page| serde_json::json!({ "page": page, "rotate": rotate }))
            .collect();
        return Ok(serde_json::to_string(&serde_json::json!({ "pages": pages }))?);
    }
    Ok(serde_json::to_string(&serde_json::json!({ "rotate": rotate }))?)
}

fn crop_spec(
    spec: Option<String>,
    inset: Option<f32>,
    top: Option<f32>,
    right: Option<f32>,
    bottom: Option<f32>,
    left: Option<f32>,
) -> Result<String> {
    if let Some(spec) = spec.filter(|value| !value.trim().is_empty()) {
        return Ok(spec);
    }
    let uniform = inset.unwrap_or(0.0);
    let top = top.unwrap_or(uniform);
    let right = right.unwrap_or(uniform);
    let bottom = bottom.unwrap_or(uniform);
    let left = left.unwrap_or(uniform);
    if top + right + bottom + left <= 0.0 {
        bail!("请使用 --inset 或 --top/--right/--bottom/--left，或改用 --spec（ypdf spec-help crop）");
    }
    Ok(serde_json::to_string(&serde_json::json!({
        "top": top,
        "right": right,
        "bottom": bottom,
        "left": left
    }))?)
}

fn parse_page_list(raw: &str) -> Result<Vec<usize>> {
    let mut pages = Vec::new();
    for token in raw.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        if let Some((from, to)) = token.split_once('-') {
            let from: usize = from.trim().parse().context("页码无效")?;
            let to: usize = to.trim().parse().context("页码无效")?;
            if from == 0 || to < from {
                bail!("页码无效：{token}");
            }
            pages.extend(from..=to);
        } else {
            let page: usize = token.parse().context("页码无效")?;
            if page == 0 {
                bail!("页码无效：{token}");
            }
            pages.push(page);
        }
    }
    if pages.is_empty() {
        bail!("请指定页码");
    }
    Ok(pages)
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

fn emit_quota(value: &Value, json: bool) -> Result<()> {
    if json {
        print_json(value)
    } else {
        print!("{}", display::format_quota(value));
        Ok(())
    }
}

fn emit_entitlements(value: &Value, json: bool) -> Result<()> {
    if json {
        print_json(value)
    } else {
        print!("{}", display::format_entitlements(value));
        Ok(())
    }
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

#[cfg(test)]
mod spec_flag_tests {
    use super::*;

    #[test]
    fn flip_simple_rotate_is_compact_json() {
        let spec: Value = serde_json::from_str(&flip_spec(None, Some(90), None).unwrap()).unwrap();
        assert_eq!(spec["rotate"], 90);
        assert!(spec.get("pages").is_none());
    }

    #[test]
    fn flip_pages_expand_without_inspect() {
        let spec: Value =
            serde_json::from_str(&flip_spec(None, Some(180), Some("1,3-4".into())).unwrap())
                .unwrap();
        assert_eq!(spec["pages"].as_array().unwrap().len(), 3);
        assert_eq!(spec["pages"][2]["page"], 4);
        assert_eq!(spec["pages"][2]["rotate"], 180);
    }

    #[test]
    fn crop_inset_sets_all_sides() {
        let spec: Value =
            serde_json::from_str(&crop_spec(None, Some(0.1), None, None, None, None).unwrap())
                .unwrap();
        assert!((spec["top"].as_f64().unwrap() - 0.1).abs() < 1e-6);
        assert!((spec["left"].as_f64().unwrap() - 0.1).abs() < 1e-6);
    }

    #[test]
    fn explicit_spec_wins() {
        assert_eq!(
            flip_spec(Some(r#"{"pages":[{"page":1,"rotate":90}]}"#.into()), Some(180), None)
                .unwrap(),
            r#"{"pages":[{"page":1,"rotate":90}]}"#
        );
    }
}
