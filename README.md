# ypdf

YeahPDF 命令行客户端（crate 名 `ypdf-cli`）。用 API Key 调用 [yeahpdf.com](https://yeahpdf.com) 处理 PDF（合并、拆分、转换、加密、压缩等）。

源码在站点仓库的 `cli/` 里维护；本仓库只承接对外发布。

## 安装

从 [Releases](https://github.com/yeahpdf/ypdf-cli/releases) 下载对应平台的包，解压后放到 `PATH`。Unix 是 `ypdf-{ver}-{target}.tar.gz`，Windows x64 是 `ypdf-{ver}-x86_64-pc-windows-msvc.zip`。

```bash
# Apple Silicon 示例
ver=0.1.4
curl -fsSL -o ypdf.tgz \
  "https://github.com/yeahpdf/ypdf-cli/releases/download/cli-v${ver}/ypdf-${ver}-aarch64-apple-darwin.tar.gz"
tar -xzf ypdf.tgz
sudo mv ypdf /usr/local/bin/
ypdf --version
```

Windows 也可跑 Skill 里的 `scripts/install.ps1`，默认装到 `%LOCALAPPDATA%\ypdf\bin`。

## 升级

不需要登录。对照 GitHub 最新 Release（`cli-vX.Y.Z`）：

```bash
ypdf upgrade --check    # 只看有没有新版本
ypdf upgrade            # 有更新则覆盖当前可写的二进制
```

仓库可用 `YPDF_CLI_REPO` 覆盖，默认 `yeahpdf/ypdf-cli`。当前文件不可写时用 Skill 的 `scripts/install.sh`（Unix）或 `scripts/install.ps1`（Windows）。

## 卸载

不需要登录。删掉当前这条 `ypdf`：

```bash
ypdf uninstall              # 只删二进制，保留 ~/.config/ypdf
ypdf uninstall --purge      # 同时删除本地配置（含已保存的 API Key）
```

不改 PATH。Windows 上若正在运行，可能留下 `ypdf.exe.old`，关掉终端后可再删。

## 未登录与登录

没有 API Key 时按网站游客额度工作（与浏览器未登录共用出口 IP 日配额）。本地会记住 `ypdf_guest`，`auth logout` 不会删它。额度用尽或功能需要登录时，再：

```bash
ypdf auth login --base-url https://www.yeahpdf.com
```

在 [控制台](https://yeahpdf.com/console) 创建 `ypdf_` 开头的 API Key。登录后走独立的 API 套餐，不再消耗游客额度。

```bash
ypdf quota                 # 默认人可读；游客会标明身份
ypdf --json quota          # 脚本用原 JSON（remaining 为 -1 表示不限）
ypdf auth logout           # 删除当前本地 profile（保留 guestId）
ypdf auth logout --all     # 清空 profile（保留 guestId）
```

不要把 Key 写进仓库或对话记录。配置默认在 `~/.config/ypdf/config.toml`。`logout` 只改本机文件，不吊销站点 Key。判断额度用 `--json` 的数字字段，不要用摘要里的「不限」。

站点返回 `1401` 时，CLI 会等约 10 秒再重试该次请求一次；`13xx` 额度错误不会重试。大文件按流上传，进度打在 stderr；脚本可加 `--quiet`。

## 发布（维护者）

在站点仓库里：

```bash
make cli-release    # 本机测一遍并打当前架构包
make cli-publish    # 同步到本仓库并打 cli-v*，GitHub Actions 编全平台（含 Windows x64）
```
