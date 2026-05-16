# vps-cli

面向 VPS 场景的 Rust CLI，用于管理 `sing-box`、`mita/mieru` 和 SSH 加固。

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

### 方式三：使用 GitHub Actions 构建产物

- 为仓库打上形如 `v1.2.3` 的 tag，或手动触发 `Build and Release` workflow。
- 下载工作流产物中的 `vps-cli-<target>`。
- 将其中的 `vps-cli` 二进制放到目标主机，例如：

```bash
sudo install -m 0755 vps-cli /usr/local/bin/vps-cli
```

建议：

- 本机开发调试优先使用 `cargo install --path .` 或 `cargo build --release`
- 在 CI/CD 或多台服务器分发时优先使用 GitHub Actions 产物

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
vps-cli singbox --help
vps-cli mieru --help
vps-cli reclaim --help
vps-cli setup-ssh --help
```

查看版本：

```bash
vps-cli --version
```

## 命令域

- `setup-ssh`
  - 配置 SSH 端口
  - 轮换 `authorized_keys`
  - 写入 `AllowUsers`
  - 可选禁用 root SSH 登录
  - 可选禁用全局密码认证
  - 可选写入基础 SSH 加固项
- `singbox`
  - 安装或更新 sing-box
  - 添加 `VLESS Reality`、`Trojan TLS`、`Hysteria2 TLS`、`TUIC TLS`、`Shadowsocks` 节点
  - 查看节点安全视图和敏感视图
  - 检查配置
  - 查看完整配置
  - 查看日志
  - 管理服务启停
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
- `mieru add-node`：支持交互补全，也支持一次性完整传参

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

## 常用示例

### 安装 sing-box

```bash
vps-cli singbox install --confirm
```

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
- 如需直接拿到链接或客户端 JSON，可附加 `--show-secrets`

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

### 安装 mita 并添加 mieru 节点

```bash
vps-cli mieru install --confirm
vps-cli mieru add-node --host example.com --port 8443 --protocol TCP --confirm
```

查看安全视图：

```bash
vps-cli mieru list-nodes
```

查看敏感链接：

```bash
vps-cli mieru show-links --show-secrets
```

查看状态：

```bash
vps-cli mieru status
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
  - `vps-cli mieru show-links --show-secrets`
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
- `show-links`、`show-config --sensitive` 等命令会输出链接、凭据、UUID、证书路径或完整配置，不应贴入公开日志。
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
vps-cli singbox --help
vps-cli mieru --help
vps-cli reclaim --help
vps-cli setup-ssh --help
```
