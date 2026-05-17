# vps-cli

面向 VPS 场景的 Rust CLI，用于管理 `sing-box`、`mita/mieru`、防火墙查询和 SSH 加固。

适用场景：

- 在 Linux VPS 上安装和维护 `sing-box`
- 以独立命令域管理 `mita/mieru`
- 更安全地执行 SSH 端口、白名单、公钥轮换与基础加固
- 通过结构化 JSON 输出让脚本或 AI 代理调用

核心特点：

- 顶层命令名固定为 `vps-cli`
- 同时支持交互式和非交互式使用
- 高风险写操作默认要求确认
- 敏感信息输出显式门控
- 关键写操作带备份、校验与回滚提示
- 支持查看 release 版本并自升级

## 权限要求

- `--help`、`--version`、`vps-cli version`、`vps-cli upgrade --check` 通常不需要 `root`
- `vps-cli upgrade` 是否需要 `sudo/root` 取决于当前二进制安装位置：
  - 如果安装在当前用户可写目录，一般不需要
  - 如果安装在 `/usr/local/bin`、`/usr/bin` 等系统目录，通常需要 `sudo` 或 `root`
- `singbox`、`mieru`、`setup-ssh`、`reclaim` 的大多数实际管理命令都会写系统文件、systemd 或防火墙，建议直接以 `root` 身份运行，或在命令前加 `sudo`
- `firewall ports` 当前会读取 Ubuntu 上的防火墙状态，也建议以 `root` 身份运行，避免权限或规则可见性差异
- 对生产 VPS 的推荐做法：
  - 先 `sudo -i`
  - 再使用 `vps-cli ...`

## 安装

### 方式一：使用 Cargo 安装

```bash
cargo install --path .
```

安装完成后可直接执行：

```bash
vps-cli --help
```

### 方式二：本地构建后手动安装

```bash
cargo build --release
sudo install -m 0755 target/release/vps-cli /usr/local/bin/vps-cli
```

### 方式三：使用 GitHub Release 二进制

- 直接下载 GitHub Release 中的压缩包。
- 当前最新 release 为 `0.3.3`。
- 发布资产命名规则：
  - `vps-cli-linux-amd64.tar.gz`
  - `vps-cli-linux-arm64.tar.gz`
- 解压后将其中的 `vps-cli` 二进制放到目标主机，例如：

```bash
sudo install -m 0755 vps-cli /usr/local/bin/vps-cli
```

### 使用 curl 下载

`amd64 / x86_64`：

```bash
curl -fL https://github.com/gray0128/sh/releases/download/v0.3.3/vps-cli-linux-amd64.tar.gz -o vps-cli-linux-amd64.tar.gz
tar -xzf vps-cli-linux-amd64.tar.gz
sudo install -m 0755 vps-cli /usr/local/bin/vps-cli
```

`arm64 / aarch64`：

```bash
curl -fL https://github.com/gray0128/sh/releases/download/v0.3.3/vps-cli-linux-arm64.tar.gz -o vps-cli-linux-arm64.tar.gz
tar -xzf vps-cli-linux-arm64.tar.gz
sudo install -m 0755 vps-cli /usr/local/bin/vps-cli
```

### 使用 wget 下载

`amd64 / x86_64`：

```bash
wget https://github.com/gray0128/sh/releases/download/v0.3.3/vps-cli-linux-amd64.tar.gz
tar -xzf vps-cli-linux-amd64.tar.gz
sudo install -m 0755 vps-cli /usr/local/bin/vps-cli
```

`arm64 / aarch64`：

```bash
wget https://github.com/gray0128/sh/releases/download/v0.3.3/vps-cli-linux-arm64.tar.gz
tar -xzf vps-cli-linux-arm64.tar.gz
sudo install -m 0755 vps-cli /usr/local/bin/vps-cli
```

### 不同 VPS 架构下载哪个链接

- `uname -m` 输出 `x86_64`：使用 `amd64` 链接
- `uname -m` 输出 `aarch64`：使用 `arm64` 链接
- 可先执行：

```bash
uname -m
```

对应下载地址：

- `amd64`：
  - [v0.3.3 / vps-cli-linux-amd64.tar.gz](https://github.com/gray0128/sh/releases/download/v0.3.3/vps-cli-linux-amd64.tar.gz)
- `arm64`：
  - [v0.3.3 / vps-cli-linux-arm64.tar.gz](https://github.com/gray0128/sh/releases/download/v0.3.3/vps-cli-linux-arm64.tar.gz)

建议：

- 本机开发调试优先使用 `cargo install --path .` 或 `cargo build --release`
- 在 CI/CD 或多台服务器分发时优先使用 GitHub Release 二进制

## 基本约定

- 顶层全局参数：
  - `--json`：输出结构化 JSON
  - `--plain`：输出纯文本
  - `--no-input`：禁止交互式补问，缺少参数时直接报错
- 常见确认参数：
  - `--confirm`：跳过交互确认，直接执行危险写操作
  - `--dry-run`：仅预览计划动作，不实际写入
- 敏感输出参数：
  - `--show-secrets`
  - `--sensitive`

推荐使用方式：

- 人工手工操作：可以省略部分参数，让 CLI 继续交互补问
- 自动化脚本或 AI 代理：传全参数，并显式加 `--no-input --confirm`

## 快速开始

```bash
vps-cli --help
vps-cli firewall --help
vps-cli singbox --help
vps-cli mieru --help
vps-cli reclaim --help
vps-cli setup-ssh --help
```

查看版本：

```bash
vps-cli --version
vps-cli version
```

检查是否有新版：

```bash
vps-cli upgrade --check
```

升级到最新 release：

```bash
vps-cli upgrade
```

升级到指定版本：

```bash
vps-cli upgrade --version 0.3.3
```

如果当前安装路径在系统目录，通常需要：

```bash
sudo vps-cli upgrade
```

## 命令域

- `setup-ssh`
  - 配置 SSH 端口
  - 轮换 `authorized_keys`
  - 写入 `AllowUsers`
  - 可选禁用 root SSH 登录
  - 可选禁用全局密码认证
  - 可选写入基础 SSH 加固项
- `firewall`
  - 查看 Ubuntu 防火墙开放端口与协议
  - 支持查看全部规则
  - 支持按端口过滤
  - 兼容 `ufw`、`nftables`、`iptables`
- `singbox`
  - 安装或更新 sing-box
  - 添加 `VLESS Reality`、`Trojan TLS`、`Hysteria2 TLS`、`TUIC TLS`、`Shadowsocks` 节点
  - 查看节点安全视图和敏感视图
  - 检查配置
  - 查看完整配置
  - 查看日志
  - 管理服务启停
- `trusttunnel`
  - 使用官方安装脚本安装或更新 TrustTunnel 服务端
  - 执行官方 `setup_wizard` 配置向导
  - 导出 deeplink / TOML 客户端配置
  - 查看 `trusttunnel` systemd 服务状态和日志
  - 提供卸载与彻底清理入口
- `mieru`
  - 安装或更新 mita
  - 添加 mieru 节点
  - 查看节点安全视图和敏感视图
  - 查看配置
  - 管理 mita 服务启停
- `reclaim`
  - 卸载 sing-box 二进制和服务
  - 清理 vps-cli 托管的 sing-box 文件
  - 审计和清理代理候选项
  - 审计和清理 nginx / caddy
  - 卸载 mita / mieru 托管状态

## 交互与参数模式

### 交互式模式

- 默认允许交互。
- 当协议向导缺少必要参数时，CLI 会提示继续输入。
- 适合人工在 VPS 上逐步完成配置。

示例：

```bash
vps-cli singbox add-vless-reality
```

这类命令通常会继续提示输入：

- 服务器域名或 IP
- 监听端口
- 证书或自签名证书处理方式
- 最终是否确认写入

### 非交互模式

- 传入 `--no-input` 后，CLI 不会再提问。
- 如果缺少必要参数，会直接报错。
- 适合脚本、自动化平台和 AI 代理。

示例：

```bash
vps-cli --no-input singbox add-vless-reality \
  --server example.com \
  --port 443 \
  --server-name www.cloudflare.com \
  --self-signed \
  --confirm
```

### `add-node` 的区别

- `singbox add-node`：通用 JSON 导入模式，必须自己准备好 JSON 文件
- `singbox add-vless-reality` / `add-trojan-tls` / `add-hysteria2-tls` / `add-tuic-tls` / `add-shadowsocks`：协议向导模式，支持缺参时交互补全
- `mieru add-node`：支持交互补全，也支持一次性完整传参；交互模式下如果未指定 `--protocol`，会明确询问使用 `TCP` 还是 `UDP`

通用 JSON 导入示例：

```bash
vps-cli singbox add-node --type vless --config-file ./inbound.json --confirm
```

`mieru` 非交互示例：

```bash
vps-cli --no-input mieru add-node \
  --host example.com \
  --port 8443 \
  --protocol TCP \
  --confirm
```

`mieru` 交互示例：

```bash
vps-cli mieru add-node
```

交互模式下通常会继续提示：

- 输入服务器公网域名或 IP
- 输入监听端口
- 选择 `TCP` 或 `UDP`
- 确认是否写入节点

## 常用示例

### 安装 sing-box

```bash
vps-cli singbox install --confirm
```

说明：

- 留空版本时会自动查询 GitHub 上的最新稳定版 release，再选择与你当前架构匹配的真实安装包
- 指定版本时支持 `1.13.12` 和 `v1.13.12` 两种写法
- 当前会自动匹配官方资产名，例如 `sing-box-1.13.12-linux-amd64.tar.gz`

指定版本并校验 SHA256：

```bash
vps-cli singbox install --version 1.10.3 --sha256 <checksum> --confirm
```

### 添加 VLESS + Reality 节点

```bash
vps-cli singbox add-vless-reality \
  --server example.com \
  --port 443 \
  --show-secrets \
  --confirm
```

说明：

- 如果未显式提供全部参数，交互模式下会继续询问
- `mieru add-node --show-secrets` 现在不会在添加结果里直接内联敏感链接；只会返回后续查看命令，敏感内容统一改为通过独立命令查看

### 查看节点列表

```bash
vps-cli singbox list-nodes
```

只查看敏感链接：

```bash
vps-cli singbox show-links --show-secrets
```

检查配置：

```bash
vps-cli singbox check-config
```

查看完整配置：

```bash
vps-cli singbox show-config --sensitive
```

查看最近日志：

```bash
vps-cli singbox logs --lines 100
```

### 查看防火墙开放端口与协议

查看全部开放规则：

```bash
sudo vps-cli firewall ports
```

仅查看指定端口：

```bash
sudo vps-cli firewall ports --port 443
```

输出说明：

- 默认输出：中文摘要 + 对齐表格
- `--plain`：输出 TSV，字段为 `port protocol family action backend`
- `--json`：输出结构化 JSON
- `--plain` 下的 warning 会输出到 `stderr`，不会污染 `stdout`

### 安装 mita 并添加 mieru 节点

```bash
vps-cli mieru install --confirm
vps-cli mieru add-node --host example.com --port 8443 --protocol TCP --confirm
```

查看安全视图：

```bash
vps-cli mieru list-nodes
```

查看 simple 分享链接：

```bash
vps-cli mieru show-simple-links --show-secrets
```

查看标准分享链接：

```bash
vps-cli mieru show-standard-links --show-secrets
```

如果添加节点时带了 `--show-secrets`，返回结果只会提示下一步怎么查看：

```bash
vps-cli mieru add-node --host example.com --port 8443 --protocol TCP --show-secrets --confirm
```

随后按需执行：

```bash
vps-cli mieru show-simple-links --show-secrets
vps-cli mieru show-standard-links --show-secrets
```

兼容入口：

```bash
vps-cli mieru show-links --show-secrets
```

查看状态：

```bash
vps-cli mieru status
```

### 安装并管理 TrustTunnel 服务端

安装或更新到默认目录：

```bash
vps-cli trusttunnel install --confirm
```

指定版本和安装目录：

```bash
vps-cli trusttunnel install --version 1.0.33 --output-dir /opt/trusttunnel --confirm
```

仅预览将执行的官方安装动作：

```bash
vps-cli --json trusttunnel install --dry-run
```

执行官方配置向导：

```bash
vps-cli trusttunnel setup-wizard
```

只查看默认配置文件路径：

```bash
vps-cli trusttunnel setup-wizard --print-paths
```

导出客户端 deeplink：

```bash
vps-cli trusttunnel export-config \
  --client alice \
  --address vpn.example.com:443 \
  --show-secrets
```

导出 TOML 配置并附带 DNS upstream：

```bash
vps-cli trusttunnel export-config \
  --client alice \
  --address 203.0.113.10:443 \
  --format toml \
  --dns-upstream tls://1.1.1.1 \
  --dns-upstream https://dns.google/dns-query \
  --show-secrets
```

查看状态与日志：

```bash
vps-cli trusttunnel status
vps-cli trusttunnel logs --lines 100
```

卸载安装目录内容：

```bash
vps-cli trusttunnel uninstall --confirm
```

### 执行 SSH 加固

```bash
vps-cli setup-ssh \
  --port 2222 \
  --user alice \
  --pubkey-file ~/.ssh/id_ed25519.pub \
  --rotate-authorized-key \
  --set-allow-users "alice" \
  --disable-root-login \
  --disable-password-auth \
  --write-hardening \
  --confirm
```

仅预览：

```bash
vps-cli setup-ssh \
  --port 2222 \
  --user alice \
  --pubkey-file ~/.ssh/id_ed25519.pub \
  --rotate-authorized-key \
  --set-allow-users "alice" \
  --disable-root-login \
  --disable-password-auth \
  --write-hardening \
  --dry-run
```

### 危险收口动作

```bash
vps-cli reclaim audit-proxies
vps-cli reclaim singbox-uninstall --confirm
```

彻底清理 vps-cli 托管的 sing-box 文件：

```bash
vps-cli reclaim singbox-purge --confirm
```

审计并清理 nginx：

```bash
vps-cli reclaim audit-nginx
vps-cli reclaim cleanup-nginx --confirm
```

卸载或彻底清理 TrustTunnel：

```bash
vps-cli reclaim trusttunnel-uninstall --confirm
vps-cli reclaim trusttunnel-purge --confirm
```

## 版本与升级

- 当前 release 最新版本为 `0.3.3`
- 顶层已有内建版本输出：
  - `vps-cli --version`
  - `vps-cli version`
- `vps-cli version --remote` 会尝试查询 GitHub Release 最新版本；如果远端不可用，会回退到当前二进制内建版本

- `vps-cli upgrade --check` 只检查，不替换当前二进制
- `vps-cli upgrade` 会：
  - 判断当前平台架构
  - 下载对应 release 压缩包
  - 校验 `.sha256`
  - 替换当前可执行文件
- 目前自动升级仅支持 Linux release 包：
  - `amd64`
  - `arm64`

## 输出约定

- 默认输出尽量使用中文。
- 传入 `--json` 后返回结构化 JSON。
- 成功时通常包含：
  - `ok`
  - `data`
  - `warnings`
  - `changed_files`
  - `backups`
  - `rolled_back`
- 失败时包含：
  - `ok: false`
  - `error`
  - 可能附带 `warnings/backups/changed_files/rolled_back`
- 敏感输出需要显式标志，例如：
  - `vps-cli singbox show-links --show-secrets`
  - `vps-cli singbox show-config --sensitive`
  - `vps-cli trusttunnel export-config --show-secrets`
  - `vps-cli mieru show-simple-links --show-secrets`
  - `vps-cli mieru show-standard-links --show-secrets`
  - `vps-cli mieru show-config --sensitive`

JSON 示例：

```json
{
  "ok": true,
  "data": {
    "added": "vless-01",
    "type": "vless-reality",
    "port": 443
  },
  "warnings": [],
  "changed_files": [
    "/etc/sing-box/config.json"
  ],
  "backups": [
    "/root/sing-box-backups/config.20260516-120000.bak"
  ],
  "rolled_back": false
}
```

## 安全说明

- `setup-ssh` 会在写入前备份主配置、托管 drop-in 和 `authorized_keys`，并执行 `sshd -t` 与 `sshd -T` 双重验证。
- `reclaim` 下的命令属于高风险动作，建议先执行对应 `audit-*` 命令。
- 自签名证书只适合测试环境。
- `trusttunnel export-config --show-secrets` 会直接输出 deeplink 或 TOML 配置，包含敏感连接信息，不应贴入公开日志。
- `trusttunnel setup-wizard` 和官方安装脚本默认围绕 `/opt/trusttunnel` 约定工作；如果自定义安装目录，请同步调整 service template 和配置路径。
- `mieru show-simple-links --show-secrets` 输出的是 simple 分享链接 `mierus://...`，更适合快速分享单节点参数。
- `mieru show-standard-links --show-secrets` 输出的是标准分享链接 `mieru://...`，更适合完整客户端配置导入。
- `show-links`、`show-simple-links --show-secrets`、`show-standard-links --show-secrets`、`show-config --sensitive` 等命令会输出链接、凭据、UUID、证书路径或完整配置，不应贴入公开日志。
- 在生产环境执行 SSH 加固前，建议保留当前会话不断开，并预留控制台入口。

## 开发与验证

在项目目录中执行：

```bash
cargo fmt
cargo build
cargo test
```

如果只想看帮助与命令面：

```bash
vps-cli --help
vps-cli firewall --help
vps-cli singbox --help
vps-cli trusttunnel --help
vps-cli mieru --help
vps-cli reclaim --help
vps-cli setup-ssh --help
```
