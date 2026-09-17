# ypdf-cli

YeahPDF 命令行客户端。用 API Key 调用 [yeahpdf.com](https://yeahpdf.com) 处理 PDF（合并、拆分、转换、加密、压缩等）。

源码在站点仓库的 `cli/` 里维护；本仓库只承接对外发布。

## 安装

从 [Releases](https://github.com/yeahpdf/ypdf-cli/releases) 下载对应平台的 `tar.gz`，解压后放到 `PATH`。

```bash
# Apple Silicon 示例
ver=0.1.0
curl -fsSL -o ypdf-cli.tgz \
  "https://github.com/yeahpdf/ypdf-cli/releases/download/cli-v${ver}/ypdf-cli-${ver}-aarch64-apple-darwin.tar.gz"
tar -xzf ypdf-cli.tgz
sudo mv ypdf-cli /usr/local/bin/
ypdf-cli --version
```

## 登录

在 [控制台](https://yeahpdf.com/console) 创建 `ypdf_` 开头的 API Key，然后：

```bash
ypdf-cli auth login --base-url https://www.yeahpdf.com
ypdf-cli quota
```

不要把 Key 写进仓库或对话记录。配置默认在 `~/.config/ypdf/config.toml`。

## 发布（维护者）

在站点仓库里：

```bash
make cli-release    # 本机测一遍并打当前架构包
make cli-publish    # 同步到本仓库并打 cli-v*，GitHub Actions 编全平台
```
