# vps-cli

面向 VPS 场景的 Rust CLI，用于管理 `sing-box`、`mita/mieru` 和 SSH 加固。

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
- 将二进制放到目标主机，例如：

```bash
sudo install -m 0755 vps-cli /usr/local/bin/vps-cli
```

## 快速开始

```bash
vps-cli --help
vps-cli singbox --help
vps-cli mieru --help
vps-cli reclaim --help
vps-cli setup-ssh --help
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

## 常用示例

### 安装 sing-box

```bash
vps-cli singbox install --confirm
```

### 添加 VLESS + Reality 节点

```bash
vps-cli singbox add-vless-reality \
  --server example.com \
  --port 443 \
  --show-secrets \
  --confirm
```

### 查看节点列表

```bash
vps-cli singbox list-nodes
```

### 安装 mita 并添加 mieru 节点

```bash
vps-cli mieru install --confirm
vps-cli mieru add-node --host example.com --port 8443 --protocol TCP --confirm
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

### 危险收口动作

```bash
vps-cli reclaim audit-proxies
vps-cli reclaim singbox-uninstall --confirm
```

## 输出约定

- 默认输出尽量使用中文。
- 传入 `--json` 后返回结构化 JSON。
- 敏感输出需要显式标志，例如：
  - `singbox show-links --show-secrets`
  - `singbox show-config --sensitive`
  - `mieru show-links --show-secrets`
  - `mieru show-config --sensitive`

## 安全说明

- `setup-ssh` 会在写入前备份主配置、托管 drop-in 和 `authorized_keys`，并执行 `sshd -t` 与 `sshd -T` 双重验证。
- `reclaim` 下的命令属于高风险动作，建议先执行对应 `audit-*` 命令。
- 自签名证书只适合测试环境。
