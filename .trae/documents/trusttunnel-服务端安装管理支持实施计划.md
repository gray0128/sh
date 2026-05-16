# TrustTunnel 服务端安装管理支持实施计划

## Summary

- 目标：为 `vps-cli` 增加独立的 `trusttunnel` 命令域，覆盖 TrustTunnel 服务端的安装、更新、服务生命周期、配置向导、客户端配置导出，以及卸载入口。
- 范围：首版包含 `install`、`status`、`start`、`stop`、`restart`、`logs`、`setup-wizard`、`export-config`、`uninstall`、`purge`；并在 `reclaim` 中补充对应的高风险清理实现或转发入口。
- 关键决策：
  - 安装实现优先包装官方 `install.sh`，不在首版重写 release 解析与下载逻辑。
  - `trusttunnel` 为独立命令域，不混入 `singbox`、`mieru` 现有命令组。
  - 卸载/清理两边都支持：`trusttunnel` 提供用户友好入口，底层危险删除逻辑统一收口到 `reclaim`。
  - 首版不仅做安装管理，还覆盖官方推荐的 `setup_wizard` 和 endpoint 客户端配置导出能力。

## Current State Analysis

### 代码现状

- 顶层入口 `vps_cli_optimized/src/main.rs` 当前已注册 `version`、`upgrade`、`setup-ssh`、`singbox`、`mieru`、`reclaim`，命令域扩展方式清晰，适合继续增加 `trusttunnel`。
- `vps_cli_optimized/src/singbox.rs` 已实现一套“独立命令域 + 安装 + systemd + 配置/日志/状态”的完整样板，适合作为 `trusttunnel` 的结构参考。
- `vps_cli_optimized/src/mieru.rs` 已实现一套“独立命令域 + 交互/非交互 + 敏感输出门控”的样板，适合作为 `setup-wizard` 与 `export-config` 输出控制参考。
- `vps_cli_optimized/src/reclaim.rs` 当前集中承载高风险卸载、清理、审计动作，且已有统一的备份/回滚/确认流程，适合扩展 TrustTunnel 的危险删除逻辑。
- `vps_cli_optimized/tests/` 当前已有：
  - `cli_help.rs` 顶层帮助测试
  - `singbox_parse.rs`、`mieru_parse.rs` 子命令帮助测试
  - `output_contract.rs` JSON 输出契约测试
  现有测试模式已经覆盖“命令面”和“输出契约”两个方向。
- `vps_cli_optimized/README.md` 与 `vps_cli_optimized/SKILL.md` 当前都没有任何 `TrustTunnel` 内容，需要同步更新。

### 外部集成事实

- 官方仓库为 `https://github.com/TrustTunnel/TrustTunnel`。
- 官方 README 明确给出服务端安装方式：
  - `curl -fsSL https://raw.githubusercontent.com/TrustTunnel/TrustTunnel/refs/heads/master/scripts/install.sh | sh -s -`
  - 默认安装目录为 `/opt/trusttunnel`
  - 支持 `-o DIR` 覆盖输出目录
  - 支持 `-V <version>` 安装指定版本
- 官方 `scripts/install.sh` 还支持：
  - `-u` 卸载
  - `-a y|n` 自动回答确认
  - 仅支持 Linux
  - 当前脚本内置默认版本 `1.0.33`
  - 安装后会落地：
    - `trusttunnel_endpoint`
    - `setup_wizard`
    - `trusttunnel.service.template`
- 官方 README 明确建议：
  - 用 `sudo ./setup_wizard` 生成 `vpn.toml`、`hosts.toml`、`credentials.toml`、`rules.toml`
  - 用 `/opt/trusttunnel/trusttunnel.service.template` 复制为 `/etc/systemd/system/trusttunnel.service`
  - 然后 `systemctl daemon-reload` 和 `systemctl enable --now trusttunnel`
- 官方 `CONFIGURATION.md` 明确：
  - 服务端主进程为 `trusttunnel_endpoint`
  - 启动形式为 `./trusttunnel_endpoint vpn.toml hosts.toml`
  - 导出客户端配置通过：
    - `./trusttunnel_endpoint vpn.toml hosts.toml -c <client_name> -a <address>`
    - `--format deeplink`
    - `--format toml`
    - 可选 `--name`
    - 可选多次 `--dns-upstream`
    - 可选 `--generate-client-random-prefix`
    - 可选 `--client-random-prefix`
    - 可选 `--prefix-length`、`--prefix-percent`、`--prefix-mask`

### 与目标能力的缺口

- 当前 CLI 不存在 `trusttunnel` 模块，也没有任何 TrustTunnel 相关命令注册。
- 当前没有对 `/opt/trusttunnel`、`trusttunnel.service`、`trusttunnel_endpoint`、`setup_wizard` 的安装管理能力。
- 当前没有复用官方 `install.sh` 的包装层，因此也没有：
  - 指定版本安装
  - 指定安装目录
  - 利用官方 `-u` 的卸载入口
  - 安装后步骤提示
- 当前没有对 TrustTunnel 配置向导的封装，也没有对 endpoint 客户端配置导出命令的封装。
- 当前 `reclaim` 没有 TrustTunnel 的卸载/清理实现，因此无法满足“两边都支持”的要求。

## Proposed Changes

### 1. 新增独立命令域骨架

#### 修改 `vps_cli_optimized/src/main.rs`

- 做什么：
  - 新增 `mod trusttunnel;`
  - 顶层 `build_cli()` 注册 `trusttunnel::cli()`
  - 顶层 `main()` 分发到 `handle_trusttunnel()`
- 为什么：
  - 用户明确要求使用独立命令域。
  - 现有架构已经按命令域拆模块，加入新域成本最低。
- 怎么做：
  - 保持全局参数 `--json`、`--plain`、`--no-input` 的现有行为不变。
  - 顶层帮助文案补充 `trusttunnel`。

#### 新增 `vps_cli_optimized/src/trusttunnel.rs`

- 做什么：
  - 实现 `trusttunnel` 命令定义与处理逻辑。
- 为什么：
  - `singbox.rs` 已经比较大，继续塞入 TrustTunnel 逻辑会破坏命令边界。
- 怎么做：
  - 首版命令面定为：
    - `install`
    - `status`
    - `start`
    - `stop`
    - `restart`
    - `logs`
    - `setup-wizard`
    - `export-config`
    - `uninstall`
    - `purge`

### 2. 安装与更新优先包装官方脚本

#### 在 `vps_cli_optimized/src/trusttunnel.rs` 实现 `install`

- 做什么：
  - 新增 `trusttunnel install`，包装官方 `scripts/install.sh`。
- 为什么：
  - 用户已选择“包装官方脚本”方案。
  - 这样可以直接继承官方支持的版本选择、输出目录、卸载行为、后续步骤提示，减少维护成本。
- 怎么做：
  - 命令参数建议：
    - `--version <VERSION>`：映射到官方 `-V`
    - `--output-dir <DIR>`：映射到官方 `-o`
    - `--auto-yes`：映射到官方 `-a y`
    - `--dry-run`：仅展示将要执行的 curl/sh 调用与目标目录，不真正执行
    - `--confirm`：非交互模式下执行安装必须显式传入
  - 默认安装目录按官方保持 `/opt/trusttunnel`。
  - 实现方式不直接 shell pipe，而是在 Rust 中：
    - 下载官方安装脚本到临时目录
    - 用 `sh` 执行脚本并传参
    - 这样更容易统一错误处理、JSON 输出和后续测试。
  - 在成功输出中返回：
    - `installed: true`
    - `version`（若可确定）
    - `output_dir`
    - `installer: "official-script"`
    - `managed_files` / `detected_binaries`
  - Breadcrumbs 指向：
    - `vps-cli trusttunnel setup-wizard`
    - `vps-cli trusttunnel status`

#### 兼容“更新即再次安装”

- 做什么：
  - 不单独增加 `upgrade` 子命令，首版以 `install` 复跑官方安装脚本完成更新。
- 为什么：
  - 官方 README 明确“再次运行安装脚本就是更新”。
  - 这能避免与现有顶层 `upgrade`（CLI 自身升级）语义冲突。
- 怎么做：
  - `trusttunnel install` 在输出中根据现有目录是否存在，提示“首次安装”或“执行更新”。
  - 如果检测到 `trusttunnel_endpoint` 已存在，交互模式下提醒先停止服务；非交互模式沿用 `--confirm` 门控。

### 3. 服务生命周期管理

#### 在 `vps_cli_optimized/src/trusttunnel.rs` 实现 `status/start/stop/restart/logs`

- 做什么：
  - 复用现有 `singbox` / `reclaim` 的 systemd 操作模式，为 `trusttunnel.service` 提供服务管理。
- 为什么：
  - 这是“安装管理”最核心的运行期能力。
- 怎么做：
  - `status`
    - 调用 `systemctl is-active trusttunnel`
    - 同时检查 `/etc/systemd/system/trusttunnel.service` 是否存在
    - 同时检查安装目录中的 `trusttunnel_endpoint` 是否存在
  - `start` / `stop` / `restart`
    - 调用 `systemctl start|stop|restart trusttunnel`
    - 失败时输出 stderr 到 warning/error
  - `logs`
    - 调用 `journalctl -u trusttunnel -n <lines> --no-pager`
    - 支持 `--lines`
    - 支持 `--follow`，仅在交互模式可用

### 4. 配置向导封装

#### 在 `vps_cli_optimized/src/trusttunnel.rs` 实现 `setup-wizard`

- 做什么：
  - 新增 `trusttunnel setup-wizard`，作为官方 `setup_wizard` 的包装层。
- 为什么：
  - 用户已确认首版需要包含配置向导。
  - 这是 TrustTunnel 官方推荐的配置入口，比 CLI 自行重写配置生成风险更低。
- 怎么做：
  - 命令参数建议：
    - `--install-dir <DIR>`：默认 `/opt/trusttunnel`
    - `--args <...>`：透传额外参数给官方 `setup_wizard`
    - `--print-paths`：只打印默认配置文件位置和说明，不执行
  - 首版只做“包装执行”和“前置检查”，不重写 wizard 交互流程。
  - 前置检查：
    - `setup_wizard` 二进制是否存在
    - 当前是否在交互环境；若 `--no-input`，则要求显式透传非交互参数，否则报错
  - 执行后在成功输出中返回常见配置文件路径约定：
    - `vpn.toml`
    - `hosts.toml`
    - `credentials.toml`
    - `rules.toml`
  - Breadcrumbs 指向：
    - `vps-cli trusttunnel start`
    - `vps-cli trusttunnel export-config --help`

### 5. 客户端配置导出

#### 在 `vps_cli_optimized/src/trusttunnel.rs` 实现 `export-config`

- 做什么：
  - 封装 `trusttunnel_endpoint vpn.toml hosts.toml -c <client> -a <address>` 的配置导出能力。
- 为什么：
  - 用户已确认首版需要覆盖客户端导出。
  - 官方文档明确把它视为 endpoint 安装完成后的标准步骤。
- 怎么做：
  - 命令参数建议：
    - `--settings <FILE>`：默认 `/opt/trusttunnel/vpn.toml`
    - `--hosts <FILE>`：默认 `/opt/trusttunnel/hosts.toml`
    - `--client <NAME>`：必填，映射 `-c`
    - `--address <ADDR>`：必填，映射 `-a`
    - `--format <deeplink|toml>`：默认 `deeplink`
    - `--name <DISPLAY_NAME>`
    - `--dns-upstream <VALUE>`：允许多次
    - `--generate-client-random-prefix`
    - `--client-random-prefix <VALUE>`
    - `--prefix-length <N>`
    - `--prefix-percent <N>`
    - `--prefix-mask <HEX>`
    - `--show-secrets`：显式认定输出为敏感内容
  - 输出控制：
    - 未传 `--show-secrets` 时，只返回导出命令摘要和提示，不直接回显完整 deeplink / TOML。
    - 传 `--show-secrets` 时，回显导出结果，并将 `report.sensitive = true`。
  - 首版直接执行二进制并捕获 stdout，不把 TOML/deeplink 另存文件，保持与现有 CLI 输出模型一致。

### 6. 卸载与清理双入口设计

#### 在 `vps_cli_optimized/src/trusttunnel.rs` 实现 `uninstall` 与 `purge`

- 做什么：
  - 在 `trusttunnel` 命令域内提供友好入口。
- 为什么：
  - 用户要求“两边都支持”。
  - 从使用者心智看，独立命令域应有自洽的生命周期闭环。
- 怎么做：
  - `trusttunnel uninstall`
    - 默认转调内部共享逻辑，执行“停止服务 + 官方脚本 `-u` 卸载安装目录内容 + 保留或仅提示 systemd 文件状态”
    - 若检测到 `/etc/systemd/system/trusttunnel.service` 仍存在，输出 warning，提示继续执行 `purge`
  - `trusttunnel purge`
    - 作为高风险动作，底层直接复用或调用 `reclaim` 的 TrustTunnel 清理函数
    - 删除：
      - `/opt/trusttunnel` 或指定安装目录
      - `/etc/systemd/system/trusttunnel.service`
    - 视存在情况补充删除默认配置文件和日志文件

#### 修改 `vps_cli_optimized/src/reclaim.rs`

- 做什么：
  - 新增 TrustTunnel 对应的高风险清理实现。
- 为什么：
  - 现有 `reclaim` 已是危险动作集中域，需要与 `trusttunnel` 双入口保持一致。
- 怎么做：
  - 新增子命令：
    - `trusttunnel-uninstall`
    - `trusttunnel-purge`
  - 共享路径常量建议放到 `trusttunnel.rs` 或新公共模块中：
    - 安装目录默认 `/opt/trusttunnel`
    - unit 文件 `/etc/systemd/system/trusttunnel.service`
  - `trusttunnel-uninstall`
    - 停止并禁用服务
    - 尝试调用官方安装脚本 `-u`
    - 保留 systemd 文件或配置提示，作为“轻卸载”
  - `trusttunnel-purge`
    - 删除安装目录
    - 删除 systemd unit
    - `daemon-reload`
    - 走 `reclaim` 现有备份/回滚/确认模型

### 7. 输出与错误契约对齐

#### 修改 `vps_cli_optimized/src/utils.rs` 与 `vps_cli_optimized/src/trusttunnel.rs`

- 做什么：
  - 让 TrustTunnel 新命令完全遵守现有 `OutputEnvelope`、`OperationReport`、`CliError` 契约。
- 为什么：
  - 当前 CLI 已对 JSON 输出稳定性有测试，TrustTunnel 不应成为特殊分支。
- 怎么做：
  - 所有成功命令继续输出：
    - `ok`
    - `data`
    - 可选 `warnings`
    - 可选 `changed_files`
    - 可选 `backups`
    - 可选 `rolled_back`
    - 可选 `sensitive`
  - `export-config --show-secrets`、可能输出凭据或 deeplink/TOML 的命令必须打 `sensitive=true`。
  - 非交互模式下的危险命令继续要求显式 `--confirm`。

### 8. 文档更新

#### 修改 `vps_cli_optimized/README.md`

- 做什么：
  - 补充 `trusttunnel` 命令域说明、安装方式、配置向导、导出配置、卸载/清理入口。
- 为什么：
  - 当前 README 仅覆盖 `singbox`、`mieru`、`setup-ssh`、`reclaim`。
- 怎么做：
  - 在“快速开始”“命令域”“常用示例”中加入：
    - `vps-cli trusttunnel install --confirm`
    - `vps-cli trusttunnel setup-wizard`
    - `vps-cli trusttunnel export-config --client ... --address ...`
    - `vps-cli trusttunnel status`
    - `vps-cli trusttunnel uninstall --confirm`
    - `vps-cli reclaim trusttunnel-purge --confirm`
  - 按仓库文档约定，在文末追加变更时间和本次变更概要。

#### 修改 `vps_cli_optimized/SKILL.md`

- 做什么：
  - 更新身份描述、命令组表格、决策树和 gotchas。
- 为什么：
  - Agent 文档当前不包含 TrustTunnel。
- 怎么做：
  - 新增 `trusttunnel` 命令组说明。
  - 补充官方安装目录、安装脚本包装、`setup_wizard` 与 `export-config` 的使用建议。
  - 按仓库文档约定，在文末追加变更时间和本次变更概要。

### 9. 测试补齐

#### 修改现有测试并新增 TrustTunnel 测试文件

- 做什么：
  - 将 TrustTunnel 纳入命令面和 JSON 契约测试。
- 为什么：
  - 这是新增独立命令域，最容易在帮助、参数和输出结构上回归。
- 怎么做：
  - 修改 `vps_cli_optimized/tests/cli_help.rs`
    - 断言顶层帮助包含 `trusttunnel`
  - 新增 `vps_cli_optimized/tests/trusttunnel_parse.rs`
    - 断言 `trusttunnel --help` 包含：
      - `install`
      - `status`
      - `start`
      - `stop`
      - `restart`
      - `logs`
      - `setup-wizard`
      - `export-config`
      - `uninstall`
      - `purge`
  - 修改 `vps_cli_optimized/tests/output_contract.rs`
    - 增加 `trusttunnel install --dry-run` 的 JSON 成功契约
    - 增加 `trusttunnel purge` 在 `--json --no-input` 下缺少 `--confirm` 必须失败
    - 增加 `trusttunnel export-config` 在不带 `--show-secrets` 时不直接返回敏感数据的契约

## Assumptions & Decisions

- TrustTunnel 首版仅面向 Linux 服务端，与官方安装脚本支持范围保持一致。
- 默认安装目录使用官方约定 `/opt/trusttunnel`，但命令需要允许覆盖目录。
- `trusttunnel install` 负责安装与更新，不额外引入 `upgrade` 子命令，以避免与 CLI 自升级冲突。
- `setup-wizard` 首版仅包装官方二进制，不在 CLI 内重写配置问答逻辑。
- `export-config` 首版直接调用 `trusttunnel_endpoint` 导出，不负责生成二维码或写入文件。
- `trusttunnel uninstall` 提供友好入口，但真正的危险删除和回滚语义以 `reclaim` 共享逻辑为准。
- 文案保持中文，命令关键字保持英文。

## Verification Steps

### 静态与编译验证

- 运行 `cargo fmt`
- 运行 `cargo build`
- 运行 `cargo test`
- 对新增或修改文件执行诊断检查，确保无新增 linter/编译错误

### 命令面验证

- 校验帮助：
  - `vps-cli --help`
  - `vps-cli trusttunnel --help`
- 校验干跑：
  - `vps-cli --json trusttunnel install --dry-run`
- 校验服务管理命令帮助：
  - `vps-cli trusttunnel logs --help`
  - `vps-cli trusttunnel export-config --help`

### 输出契约验证

- `trusttunnel install --dry-run --json`
  - 返回 `ok: true`
  - `data.dry_run = true`
  - 包含 `output_dir`
- `trusttunnel purge --json --no-input`
  - 未传 `--confirm` 时返回 `ok: false`
- `trusttunnel export-config --json`
  - 未传 `--show-secrets` 时不直接回显完整 deeplink / TOML
- `trusttunnel export-config --json --show-secrets`
  - 返回 `sensitive: true`

### 手工功能验收

- 安装：
  - 默认目录安装
  - 指定版本安装
  - 指定输出目录安装
- 向导：
  - 已安装后可正确启动 `setup_wizard`
  - 非交互环境缺少必要透传参数时拒绝执行
- 服务：
  - `status/start/stop/restart/logs` 可正确访问 `trusttunnel.service`
- 导出：
  - deeplink 导出
  - TOML 导出
  - `--dns-upstream` 多次传参
  - `--generate-client-random-prefix` 透传
- 卸载/清理：
  - `trusttunnel uninstall --confirm`
  - `trusttunnel purge --confirm`
  - `reclaim trusttunnel-uninstall --confirm`
  - `reclaim trusttunnel-purge --confirm`

## Implementation Order

1. 新增 `src/trusttunnel.rs` 并在 `src/main.rs` 注册独立命令域。
2. 在 `trusttunnel.rs` 先实现 `install`、`status/start/stop/restart/logs` 基础安装管理闭环。
3. 实现 `setup-wizard` 包装层和 `export-config` 导出逻辑。
4. 扩展 `reclaim.rs`，补上 TrustTunnel 的卸载/清理共享实现。
5. 回填 `trusttunnel uninstall/purge` 友好入口，接到共享危险逻辑。
6. 更新 `README.md`、`SKILL.md`。
7. 补齐 `cli_help.rs`、`output_contract.rs`、新增 `trusttunnel_parse.rs`，再统一跑格式化、编译和测试。
