# vps-cli — Agent Skill Guide

## Identity

vps-cli 是一个用 Rust 编写的命令行工具，用于在 Linux VPS 上安装和管理 sing-box 代理及加固 SSH 服务。它旨在既适合人工终端用户，也能被 AI 代理稳定使用。工具提供结构化的 JSON 输出、幂等和安全的操作以及清晰的帮助信息。

## Authentication

大部分命令需要在目标服务器上以 root 身份执行，但不涉及远程 API 鉴权。通过 SSH 登录服务器后，直接调用本工具即可。无需外部 token。

## Output Contract

- 所有命令返回零 exit code 表示成功，非零表示失败。
- 默认情况下，成功信息以简短的中文输出到 `stdout`；错误信息输出到 `stderr`。
- 传入 `--json` 时，工具将输出 JSON 对象：

  ```json
  {
    "ok": true,
    "data": { ... },
    "breadcrumbs": [ { "action": "string", "cmd": "string" }, ... ]
  }
  ```

  失败时 `ok` 为 false，并提供 `error` 字段。
- `breadcrumbs` 字段提供建议的下一步命令，代理可直接执行。

## Command Groups

| 组别 | 描述 |
|---|---|
| `setup-ssh` | 加固和配置 SSH 服务。修改端口、用户、授权公钥等。 |
| `singbox install` | 下载并安装/升级 sing-box 二进制。 |
| `singbox add-node` | 添加不同类型的代理节点。 |
| `singbox list-nodes` | 列出已配置的节点。支持 `--limit` 分页。 |
| `singbox remove-node` | 删除节点。|
| `singbox status/start/stop/restart` | 管理 sing-box systemd 服务。 |

## Decision Trees

### 安装 sing-box 并添加节点

1. `vps-cli singbox install --json --confirm` — 安装最新版本并返回 JSON 输出。 
2. 从输出中的 `breadcrumbs` 获取下一步建议，例如 `add-node`。  
3. `vps-cli singbox add-node --type vless-reality --config-file nodes/vless.json --json --confirm` — 添加节点。  
4. 检查列表：`vps-cli singbox list-nodes --json`。  
5. 启动服务：`vps-cli singbox start --json`。

### 加固 SSH

1. `vps-cli setup-ssh --port 2222 --user alice --pubkey-file ~/.ssh/id_ed25519.pub --json --confirm` — 将 SSH 端口改为 2222，允许 `alice` 登陆并设置公钥。 
2. 若只想预览：加入 `--dry-run`。 

## Invariants

- CLI 在非交互模式（无 TTY 或传入 `--no-input`）时不会出现任何提示；所有必需参数均必须通过标志传入，否则立即返回错误。 
- 所有数据输出都可通过 `--json` 获得结构化格式；诊断信息不会混入其中。 
- 如果命令涉及修改系统状态，则默认需要确认；可通过 `--dry-run` 预览。 

## Gotchas

- 安装 sing-box 需要联网下载二进制，确保服务器能访问 GitHub。 
- 执行加固 SSH 时会修改 `sshd_config.d` 并重启 sshd 服务，请在测试环境验证成功后再在生产环境使用。 
