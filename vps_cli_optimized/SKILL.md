# vps-cli — Agent Skill Guide

## Identity

vps-cli 是一个用 Rust 编写的命令行工具，用于在 Linux VPS 上安装和管理 sing-box 服务端、mita/mieru 服务端，并对 SSH 服务执行更安全的加固与密钥轮换。它既适合人工终端用户，也适合 AI 代理调用。工具提供结构化 JSON 输出、危险操作确认、备份与回滚提示，以及尽量中文化的帮助信息。

## Authentication

大部分命令需要在目标服务器上以 root 身份执行，但不涉及远程 API 鉴权。通过 SSH 登录服务器后，直接调用本工具即可。无需外部 token。

## Build And Distribution

- 实际安装后的二进制名为 `vps-cli`。
- 本地安装可使用 `cargo install --path .`，或执行 `cargo build --release` 后手动安装 `target/release/vps-cli`。
- GitHub Actions 的发布工作流位于仓库根目录 `.github/workflows/release.yml`，构建目录为 `vps_cli_optimized/`。
- Release 资产名称固定为：
  - `vps-cli-linux-amd64.tar.gz`
  - `vps-cli-linux-arm64.tar.gz`
- 当前约定的最新 release 版本先按 `0.1.1` 处理。

## Output Contract

- 所有命令返回零 exit code 表示成功，非零表示失败。
- 默认情况下，成功信息以中文输出到 `stdout`；错误信息输出到 `stderr`。
- 传入 `--json` 时，工具将输出 JSON 对象：

  ```json
  {
    "ok": true,
    "data": { "...": "..." },
    "breadcrumbs": [ { "action": "string", "cmd": "string" } ],
    "warnings": [],
    "changed_files": [],
    "backups": [],
    "rolled_back": false,
    "sensitive": false
  }
  ```

- 失败时 `ok` 为 false，并提供 `error` 字段。
- `warnings` 表示风险提示或降级行为。
- `backups` 表示本次变更前保存的备份位置。
- `sensitive` 为 true 时，表示当前输出包含链接、凭据、密钥或完整配置等敏感信息。
- `breadcrumbs` 字段提供建议的下一步命令，代理可直接执行。

## Command Groups

| 组别 | 描述 |
|---|---|
| `setup-ssh` | 配置 SSH 端口、AllowUsers、公钥轮换和基础加固项，带备份、校验和自动回滚。 |
| `singbox` | 安装 sing-box、添加协议节点、查看节点、安全/敏感视图、检查配置、查看日志和管理服务。 |
| `mieru` | 安装 mita、添加 mieru 节点、查看安全视图、分别查看 simple/standard 分享链接、查看配置和管理 mita 服务。 |
| `reclaim` | 集中承载高风险收口动作，例如 sing-box 卸载、托管文件清理、代理栈清理、nginx/caddy 清理和 mieru 卸载。 |
| `version` | 查看当前版本、最新 release 版本、平台架构与下载地址。 |
| `upgrade` | 下载对应架构的 GitHub Release 资产并升级当前 `vps-cli`。 |

## Decision Trees

### 安装 sing-box 并添加节点

1. `vps-cli singbox install --json --confirm` — 安装或更新 sing-box；未指定版本时会在真正执行安装时自动解析最新稳定版 release 的真实 Linux 安装包。  
2. `vps-cli singbox add-vless-reality --server example.com --port 443 --json --confirm` — 用协议向导生成服务端入站并输出结构化结果。  
3. `vps-cli singbox list-nodes --json` — 查看安全视图。  
4. `vps-cli singbox show-links --show-secrets --json` — 在显式确认后查看敏感链接或客户端 JSON。  
5. `vps-cli singbox check-config --json` — 校验当前配置。  
6. `vps-cli singbox restart --json` — 重启服务。

### 安装 mita 并添加 mieru 节点

1. `vps-cli mieru install --version 3.32.0 --json --confirm` — 安装或更新 mita。  
2. `vps-cli mieru add-node --host example.com --port 8443 --protocol TCP --json --confirm` — 添加 mieru 节点；交互模式缺少 `--protocol` 时会询问选择 `TCP` 或 `UDP`。  
3. `vps-cli mieru list-nodes --json` — 查看安全视图。  
4. `vps-cli mieru show-simple-links --show-secrets --json` — 查看 simple 分享链接 `mierus://`。  
5. `vps-cli mieru show-standard-links --show-secrets --json` — 查看标准分享链接 `mieru://`。  
6. `vps-cli mieru status --json` — 检查 systemd 状态与 mita 状态。  
7. `vps-cli mieru add-node --show-secrets --json --confirm` — 仅返回敏感信息查看入口，不再在添加结果中直接内联分享链接。

### 加固 SSH

1. `vps-cli setup-ssh --port 2222 --user alice --pubkey-file ~/.ssh/id_ed25519.pub --rotate-authorized-key --set-allow-users alice --disable-root-login --disable-password-auth --write-hardening --json --confirm` — 将 SSH 端口改为 2222，轮换 `alice` 的公钥，写入白名单并执行基础加固。  
2. CLI 会先备份主配置、托管 drop-in 和 `authorized_keys`，再执行 `sshd -t` 与 `sshd -T` 双重验证。  
3. 若校验或重载失败，CLI 会尝试自动恢复最近一次备份。  
4. 若只想预览：加入 `--dry-run`。

### 危险收口动作

1. `vps-cli reclaim singbox-uninstall --json --confirm` — 删除 sing-box 二进制和 systemd 服务，保留配置。  
2. `vps-cli reclaim singbox-purge --json --confirm` — 删除 vps-cli 托管的 sing-box 文件。  
3. `vps-cli reclaim audit-proxies --json` — 审计常见代理栈清理候选项。  
4. `vps-cli reclaim cleanup-proxies --json --confirm` — 清理代理栈候选项。  
5. `vps-cli reclaim mieru-uninstall --json --confirm` — 卸载 mita/mieru 服务端包、服务和托管状态。

## Invariants

- CLI 在非交互模式下不会发起提问；涉及危险写操作时必须显式传入 `--confirm`，否则直接失败。
- `singbox` 节点管理面向服务端 `inbounds` 语义，不再把脚本中的节点能力错误映射为客户端 `outbounds`。
- `mieru` 作为独立命令域存在，不再挂在 `singbox` 之下。
- `reclaim` 专门承载卸载、清理、审计等高风险动作，避免与日常管理命令混放。
- 敏感视图必须由显式标志触发；默认输出优先使用安全视图。
- 所有数据输出都可通过 `--json` 获得结构化格式；诊断信息不会混入成功数据。

## Gotchas

- 安装 sing-box 或 mita 需要联网下载二进制或安装包，确保服务器能访问 GitHub。
- 使用 `--self-signed` 生成的证书仅适合测试环境，客户端通常需要允许 insecure。
- `setup-ssh` 会修改 `sshd_config.d` 并重载 SSH 服务；生产环境执行前应保持当前 SSH 会话不断开，并准备好控制台入口。
- `show-links`、`show-simple-links --show-secrets`、`show-standard-links --show-secrets`、`show-config --sensitive` 等命令会返回凭据、UUID、私钥或完整配置，不应贴入公开日志。
- `reclaim` 命令会删除系统文件或 systemd 服务，建议先执行对应的 `audit-*` 命令确认候选项。
- `version` 与 `upgrade --check` 通常不要求 root；`upgrade` 若目标安装目录不可写，则需要 `sudo` 或 `root`。
- `singbox`、`mieru`、`setup-ssh`、`reclaim` 的实际管理命令大多需要 `root` 或 `sudo`。

---

变更时间：2026-05-16
本次变更概要：同步更新 vps-cli 的命令域设计、JSON 输出约定、安全/敏感视图、SSH 回滚语义，以及新增的 `mieru` 与 `reclaim` 命令组说明。

---

变更时间：2026-05-16
本次变更概要：补充 vps-cli 的实际二进制命名、安装方式和 GitHub Actions 构建产物约定，并对齐根目录工作流位置。

---

变更时间：2026-05-16
本次变更概要：新增 `version` 与 `upgrade` 命令，同步 release 资产命名、root/sudo 使用约定，以及 Linux amd64/arm64 下载分发规则。

---

变更时间：2026-05-16
本次变更概要：为 `mieru` 增加 simple/standard 分享链接独立命令，并在交互式添加节点时显式询问 `TCP/UDP` 协议。

---

变更时间：2026-05-16
本次变更概要：将 `mieru add-node --show-secrets` 调整为只返回敏感查看入口，并修复 `singbox install` 默认最新稳定版解析官方 release 资产名导致的下载 404 问题。

---

变更时间：2026-05-16
本次变更概要：同步发布版本线到 `0.1.1`，更新默认最新 release 说明，用于发布 `v0.1.1`。
