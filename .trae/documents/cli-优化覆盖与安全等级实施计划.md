# CLI 优化覆盖与安全等级实施计划

## Summary

- 目标：把 `vps_cli_optimized` 从“基础命令集合”升级为“尽量覆盖现有脚本能力的可编排 CLI”，同时让 `SSH` 与 `sing-box/mieru` 相关危险操作达到与脚本等效的安全控制等级，并将 `sing-box` 卸载/清理能力抽离为独立命令域。
- 范围：覆盖 `sing-box` 脚本中的安装、协议节点管理、配置检查、完整配置查看、日志，以及独立的卸载、清理、审计命令域；覆盖 `mieru/mita` 独立命令域；补齐 `setup-ssh` 在备份、校验、回滚、生效验证上的安全缺口；统一帮助、提示、错误和确认文案为中文。
- 关键决策：
  - 本次尽量覆盖全部脚本能力，而不是仅做高频功能。
  - `mieru` 从 `singbox` 命令域中彻底拆出，不保留旧入口兼容。
  - `sing-box` 脚本中的卸载、清理、审计相关能力从 `singbox` 命令组中拆出，单独形成运维收口命令域。
  - 交互模式采用“双模式并重”：保留可自动化的子命令/参数方式，同时为高频与高风险操作补充交互式向导。
  - 安全目标采用“等效而非逐项复刻”：允许实现方式不同，但必须实现同等级别的风险控制、确认、回滚与校验。

## Current State Analysis

### 代码现状

- 当前 CLI 入口仅注册两个命令组：`setup-ssh` 与 `singbox`，见 `vps_cli_optimized/src/main.rs`。
- 当前不存在独立的“卸载/清理/审计”命令域，`sing-box` 未来若继续直接堆叠此类能力，会让命令边界混杂“日常管理”和“危险收口操作”。
- 当前 `singbox` 能力仅覆盖：
  - `install`
  - `add-node`
  - `list-nodes`
  - `remove-node`
  - `status/start/stop/restart`
  见 `vps_cli_optimized/src/singbox.rs`。
- 当前 `setup-ssh` 能力仅覆盖：
  - 端口修改
  - 登录用户限制
  - 公钥写入
  - snippet 写入
  - `sshd -t` 校验
  - `systemctl reload`
  - 防火墙放行
  见 `vps_cli_optimized/src/setup_ssh.rs`。
- 当前输出基础设施较薄，仅有：
  - `OutputFormat`
  - `OutputEnvelope`
  - `Breadcrumb`
  - `CliError`
  见 `vps_cli_optimized/src/utils.rs`。

### 与脚本/文档对比出的主要缺口

- `sing-box` 缺少脚本中的协议级交互向导与完整运维能力：
  - `VLESS Reality`
  - `Trojan TLS`
  - `Hysteria2 TLS`
  - `TUIC TLS`
  - `Shadowsocks`
  - 配置检查
  - 完整配置显示
  - 日志查看
  - 卸载保留配置
  - 托管文件彻底清理
  - 代理服务审计与清理
  - `nginx/caddy` 审计与清理
  - `mieru/mita` 独立管理
  依据 `sing-box/README_singbox_deploy_hardened_v17_zh.md`。
- `sing-box` 相关危险操作当前没有独立命令域承载，若全部塞入 `singbox` 下，会让常规节点管理与危险清理动作并列，增加误用风险。
- `setup-ssh` 缺少脚本中的安全兜底：
  - `authorized_keys` 备份/恢复
  - 公钥有效性校验
  - `sshd -T` 生效校验
  - 白名单与公钥用户一致性校验
  - 更细粒度的可选安全项
  - 更严格的失败回滚策略
  依据 `setup-sshh/README.md` 与 `setup-sshh/setup-ssh.sh`。
- 当前 CLI 文案虽然大多为中文，但仍不完整：
  - 命令帮助与错误提示中文程度不一致
  - 危险操作尚未形成一致的中文确认话术
  - 缺少对“敏感输出”“危险操作”“回滚已执行”等状态的统一表达

## Proposed Changes

### 1. 重构命令域与模块边界

#### 修改 `vps_cli_optimized/src/main.rs`

- 做什么：
  - 将顶层命令组调整为：
    - `setup-ssh`
    - `singbox`
    - `mieru`
    - `reclaim`
  - 为后续新增命令组保留统一的命令注册方式。
- 为什么：
  - 当前 `main.rs` 直接耦合两个命令组，不利于继续扩展。
  - `mieru` 需要从 `singbox` 域中彻底拆出。
  - `sing-box` 的卸载/清理/审计类动作需要从日常管理域中分离，降低危险操作误触概率。
- 怎么做：
  - 新增 `mod mieru;`
  - 新增 `mod reclaim;`
  - 顶层 `build_cli()` 注册 `mieru::cli()`
  - 顶层 `build_cli()` 注册 `reclaim::cli()`
  - 顶层 `main()` 分发到 `handle_mieru()`
  - 顶层 `main()` 分发到 `handle_reclaim()`
  - 保持全局选项 `--json`、`--plain`、`--no-input`
  - 保持所有帮助文案为中文

#### 新增 `vps_cli_optimized/src/mieru.rs`

- 做什么：
  - 新建独立 `mieru` 命令域，覆盖脚本中的 mita/mieru 生命周期与节点管理能力。
- 为什么：
  - 用户明确要求从 `singbox` 命令域中独立出来。
  - 脚本文档本身已将其视为独立服务，而不是普通 sing-box 节点。
- 怎么做：
  - 设计子命令：
    - `install`
    - `add-node`
    - `list-nodes`
    - `show-links`
    - `status`
    - `start`
    - `stop`
    - `show-config`
  - 按脚本文档中的托管路径组织数据：
    - `/etc/mieru-managed/nodes.json`
    - `/etc/mieru-managed/links.txt`
    - `/etc/mieru-managed/mita_config.json`
  - 将 `mita` systemd 服务管理与 `mieru` 节点数据管理放在同一命令组内
  - 所有输出分为：
    - 安全视图
    - 敏感视图
  - 对敏感输出增加中文风险提示与显式确认

#### 新增 `vps_cli_optimized/src/reclaim.rs`

- 做什么：
  - 新建独立 `reclaim` 命令域，承载 `sing-box` 脚本中的卸载、清理、审计与环境收口能力。
- 为什么：
  - 用户要求把 `sing-box` 脚本中的卸载功能拆成独立命令域。
  - 卸载、清理、审计本质上属于危险运维动作，与 `singbox` 下的安装、节点、状态等日常管理动作分离更安全。
- 怎么做：
  - 设计子命令：
    - `singbox-uninstall`
    - `singbox-purge`
    - `audit-proxies`
    - `cleanup-proxies`
    - `audit-nginx`
    - `cleanup-nginx`
    - `audit-caddy`
    - `cleanup-caddy`
    - `mieru-uninstall`
  - 所有清理类命令默认先输出候选结果，再要求显式确认。
  - 所有删除类命令复用统一备份、回滚、危险确认和中文风险提示。

### 2. 拆分 `singbox` 为更清晰的能力层

#### 修改 `vps_cli_optimized/src/singbox.rs`

- 做什么：
  - 从“单文件塞满全部逻辑”改成“命令定义 + 协议操作 + 服务管理 + 配置安全”的结构。
- 为什么：
  - 当前文件同时承担命令注册、下载、配置读写、服务控制，扩展到脚本同等级功能后会难以维护。
- 怎么做：
  - 第一阶段在同文件内先完成能力扩展与内部私有函数分层。
  - 第二阶段若代码量过大，再拆成目录模块：
    - `src/singbox/mod.rs`
    - `src/singbox/install.rs`
    - `src/singbox/node_builders.rs`
    - `src/singbox/service.rs`
    - `src/singbox/config_ops.rs`
    - `src/singbox/audit.rs`
  - 优先保证命令接口稳定，再做内部拆分。

#### 在 `singbox` 命令组内新增/补齐子命令

- 做什么：
  - 将 `singbox` 扩展为覆盖脚本主要菜单项的子命令集合。
- 为什么：
  - 当前只能装、增删节点、控服务，覆盖面远不足以替代脚本。
- 怎么做：
  - 保留并增强现有命令：
    - `install`
    - `add-node`
    - `list-nodes`
    - `remove-node`
    - `status/start/stop/restart`
  - 新增命令：
    - `check-config`
    - `show-config`
    - `show-links`
    - `logs`
  - 明确不再放入 `singbox` 命令组的能力：
    - 卸载
    - 彻底清理
    - 代理审计与清理
    - `nginx/caddy` 审计与清理
  - 这些危险运维动作统一迁移到独立 `reclaim` 命令域。

#### 增强 `add-node`，同时新增协议向导型子命令

- 做什么：
  - 兼容“通用 JSON 导入”和“协议级向导生成”两种方式。
- 为什么：
  - 用户要求双模式并重，既要自动化友好，也要接近脚本使用体验。
- 怎么做：
  - 保留现有 `add-node --type --config-file`
  - 新增协议明确的向导子命令：
    - `add-vless-reality`
    - `add-trojan-tls`
    - `add-hysteria2-tls`
    - `add-tuic-tls`
    - `add-shadowsocks`
  - 每个向导命令支持：
    - 纯参数模式
    - 缺参时交互式补全
    - `--dry-run`
    - `--confirm`
  - 输出结果分三类：
    - 安全视图中的节点摘要
    - 敏感视图中的 URI / client JSON
    - 写回后的配置路径与后续建议命令
  - 对 TLS 类协议支持：
    - 使用已有证书路径
    - 或显式要求生成自签名证书
  - 对 Shadowsocks 提供脚本中支持的 method 白名单

### 3. 建立统一的安全控制与回滚基础设施

#### 新增 `vps_cli_optimized/src/safety.rs`

- 做什么：
  - 提供所有高风险操作共享的备份、确认、回滚、危险标识、敏感输出控制工具。
- 为什么：
  - 目前 `setup_ssh.rs` 和 `singbox.rs` 各自直接操作文件，缺少统一的安全边界。
  - 要达到与脚本等效的安全等级，必须有统一的事务式封装。
- 怎么做：
  - 提供抽象能力：
    - `BackupManager`
    - `RollbackGuard`
    - `DangerConfirm`
    - `SensitiveOutputGate`
    - `OperationReport`
  - 统一备份目录规范：
    - `SSH` 备份放 `/root/ssh-setup-backups`
    - `sing-box` / `mieru` 危险修改使用各自托管备份目录
  - 统一确认策略：
    - 交互模式下使用中文确认
    - 非交互模式必须显式 `--confirm`
    - 对 `purge`/`cleanup-*`/敏感输出引入更强确认条件
  - 统一回滚策略：
    - 文件写入后校验失败则自动回滚
    - 服务重载/启动失败则自动回滚
    - 回滚结果写入清晰中文输出和 JSON 报告

#### 修改 `vps_cli_optimized/src/utils.rs`

- 做什么：
  - 扩展输出与错误模型，支撑中文提示、安全事件与多层级结果。
- 为什么：
  - 当前 `OutputEnvelope` 与 `CliError` 无法承载“已备份、已回滚、存在敏感信息、建议下一步”等状态。
- 怎么做：
  - 扩展响应结构，增加可选字段：
    - `warnings`
    - `sensitive`
    - `changed_files`
    - `backups`
    - `rolled_back`
  - 为中文终端输出增加统一打印辅助方法
  - 统一危险操作错误与建议文案

### 4. 将 `setup-ssh` 升级到与脚本等效的安全等级

#### 修改 `vps_cli_optimized/src/setup_ssh.rs`

- 做什么：
  - 让 `setup-ssh` 从“简化实现”升级为“具备脚本级校验、回滚、生效验证、粒度开关”的正式命令。
- 为什么：
  - 当前 `setup-ssh` 的风险集中在：
    - `authorized_keys` 直接覆盖
    - 无公钥内容校验
    - 仅做 `sshd -t`，不做 `sshd -T` 生效验证
    - 配置项写法较硬，缺少细粒度控制
- 怎么做：
  - 增加显式参数，将脚本里的关键开关 CLI 化：
    - `--port`
    - `--user`
    - `--pubkey-file`
    - `--set-allow-users`
    - `--disable-root-login`
    - `--disable-password-auth`
    - `--write-hardening`
    - `--rotate-authorized-key`
    - `--skip-pubkey`
  - 默认行为遵循保守原则：
    - 未显式指定的项不主动改变
    - 但交互模式可引导用户选择
  - 新增安全流程：
    - 备份 `/etc/ssh/sshd_config`
    - 备份 drop-in 配置
    - 备份目标用户 `authorized_keys`
    - 先用 `ssh-keygen` 校验公钥
    - 再写入文件
    - 再运行 `sshd -t`
    - 再运行 `sshd -T`
    - 验证关键项确已生效
    - 验证失败自动回滚
  - 对 `AllowUsers` 增加一致性校验：
    - 若轮换的是某个用户的公钥，则白名单必须包含该用户
  - 保留现有 drop-in 模式，不直接大改主配置
  - 帮助、交互、错误、回滚提示全部中文化

### 5. 为 `singbox` 与 `mieru` 补齐运维能力

#### `singbox install` 增强

- 做什么：
  - 补齐脚本中对版本、校验、依赖、服务文件、基础目录、初始配置的更完整控制。
- 为什么：
  - 当前安装流程较基础，无法支撑完整运维生命周期。
- 怎么做：
  - 保留 `--version`、`--sha256`
  - 增加：
    - 依赖检测结果输出
    - 版本/架构探测结果输出
    - 已安装版本检查
    - 升级与首次安装的差异化提示
  - 对默认配置文件生成引入更清晰的模板初始化逻辑

#### `singbox check-config/show-config/show-links/logs`

- 做什么：
  - 把脚本里的“检查配置、显示完整配置、查看节点链接、查看日志”能力落到 CLI。
- 为什么：
  - 这是把 CLI 从“配置写入器”提升为“可运维工具”的关键。
- 怎么做：
  - `check-config`
    - 调用 `sing-box check -c /etc/sing-box/config.json`
    - 输出人类可读摘要或 JSON
  - `show-config`
    - 默认要求显式 `--sensitive` 才输出完整配置
    - 默认模式只给摘要
  - `show-links`
    - 只展示节点链接或客户端片段
    - 视图区分安全/敏感
  - `logs`
    - 读取 `journalctl -u sing-box`
    - 支持 `--lines`
    - 支持 `--follow` 仅在交互环境中使用

#### `reclaim` 命令域承载 `sing-box` 卸载/清理/审计

- 做什么：
  - 对齐脚本中的卸载、彻底清理、代理审计、可选 `nginx/caddy` 审计清理，并以独立命令域承载。
- 为什么：
  - 这是脚本覆盖差距最大的部分，也是危险操作最集中的部分。
  - 与 `singbox` 日常管理动作拆分后，命令边界更清晰，也更符合“高危动作集中管理”的安全设计。
- 怎么做：
  - `reclaim singbox-uninstall`
    - 停止并禁用服务
    - 删除二进制与 unit
    - 保留配置目录
  - `reclaim singbox-purge`
    - 清理 CLI 托管的 sing-box 文件
    - 需要显式危险确认
  - `reclaim audit-proxies`
    - 收集常见 `233boy/sing-box/xray/v2ray` unit、配置、日志、数据和二进制路径
    - 仅展示候选，不删除
  - `reclaim cleanup-proxies`
    - 依赖 `audit-proxies` 的候选结果
    - 展示后确认删除
    - 清理后可选继续处理 `nginx/caddy`
  - `reclaim audit-nginx` / `reclaim audit-caddy`
    - 只做候选发现
  - `reclaim cleanup-nginx` / `reclaim cleanup-caddy`
    - 显式危险确认后删除
  - 统一遵循“先审计，再清理”的模型

#### `mieru` 生命周期与节点能力

- 做什么：
  - 覆盖脚本中的 mita 安装/更新、节点生成、状态查看、敏感链接输出、卸载。
- 为什么：
  - 当前 CLI 完全没有该能力，而用户要求独立命令域。
- 怎么做：
  - `mieru install`
    - 支持 `--version`
    - 支持 `--sha256`
    - 管理 mita 服务安装与更新
  - `mieru add-node`
    - 生成 mita 配置
    - 维护托管节点文件
    - 输出 `mierus://` link 与 client JSON
  - `mieru list-nodes`
    - 默认安全视图
  - `mieru show-links`
    - 默认危险提示
  - `mieru status/show-config/start/stop`
    - 对齐脚本的 mita 运维能力
  - `mieru` 命令组内不再承载卸载功能
  - `reclaim mieru-uninstall`
    - 卸载 mita 服务与托管状态
    - 危险操作必须确认

### 6. 补齐中文化体验与命令帮助

#### 修改 `vps_cli_optimized/src/main.rs`、`src/singbox.rs`、`src/mieru.rs`、`src/setup_ssh.rs`、`src/utils.rs`

- 做什么：
  - 统一所有帮助、提示、确认、危险说明、错误信息为中文。
- 为什么：
  - 用户明确要求 CLI 提示等信息尽量使用中文。
- 怎么做：
  - 中文化 `about`、`help`、交互文案、危险提示、错误建议
  - 建立统一中文术语：
    - “安全视图”
    - “敏感视图”
    - “危险操作”
    - “已备份”
    - “已自动回滚”
    - “建议下一步”
  - 对敏感输出统一附带提醒：
    - 含密码、UUID、私钥、链接等内容，不应贴入公开日志

### 7. 增加测试与验收覆盖

#### 新增 `vps_cli_optimized/tests/` 集成测试目录

- 做什么：
  - 为命令解析、输出格式和关键安全策略增加自动化测试。
- 为什么：
  - 这是大规模扩展 CLI 后控制回归风险的最低保障。
- 怎么做：
  - 新增测试文件：
    - `tests/cli_help.rs`
    - `tests/output_contract.rs`
    - `tests/singbox_parse.rs`
    - `tests/mieru_parse.rs`
    - `tests/setup_ssh_parse.rs`
  - 测试重点：
    - 子命令和参数解析
    - 中文帮助文本存在性
    - `--json` 输出结构
    - 非交互模式下危险命令必须显式 `--confirm`
    - 敏感命令必须要求附加标志或确认
  - 对需要 root / systemd / 实机文件系统的流程：
    - 单元测试不直接操作真实系统
    - 将命令构建、参数验证、配置渲染、风险门控抽成可测试纯函数

### 8. 文档同步

#### 修改 `vps_cli_optimized/SKILL.md`

- 做什么：
  - 更新工具身份说明、命令组、决策树、输出约定、危险操作语义。
- 为什么：
  - 当前 `SKILL.md` 仍只有 `setup-ssh` 与 `singbox` 基础能力，已经不符合目标状态。
- 怎么做：
  - 补充 `mieru` 命令组
  - 补充 `reclaim` 命令组
  - 更新 `singbox` 新增子命令
  - 写明安全视图/敏感视图
  - 写明危险操作确认和回滚行为

#### 视实现情况新增 `vps_cli_optimized/README.md`

- 做什么：
  - 若仓库中暂无该文档，则新增一个面向人类用户的 CLI 使用说明。
- 为什么：
  - 当前仓库没有独立的 CLI 用户说明，后续命令面会显著扩张。
- 怎么做：
  - 提供：
    - 安装与构建
    - 顶层命令概览
    - `singbox` 常用流程
    - `mieru` 常用流程
    - `setup-ssh` 安全提示
    - 敏感输出与危险操作说明
  - 若新增此文档，在文末追加变更时间与摘要

## Assumptions & Decisions

- 顶层命令名称保持英文，命令内部文案与帮助尽量中文化；不把命令关键字改成中文。
- `mieru` 不再作为 `singbox` 的任何别名或兼容子命令保留。
- `sing-box` 的卸载、清理、审计动作不再挂在 `singbox` 命令组下，而是统一收口到 `reclaim` 命令域。
- “尽量覆盖脚本功能”按命令能力覆盖理解，不要求在第一次实现里逐字复刻脚本菜单编号。
- “同等安全等级”按风险控制能力等效理解，允许用更结构化的 Rust 封装替代脚本中的实现方式。
- 本次优先保证：
  - 命令域结构正确
  - 危险操作有统一门控
  - 高风险写操作有备份、校验、回滚
  - 常用功能具备非交互与交互双模式
- 审计/清理功能以“CLI 托管路径 + 常见第三方路径候选扫描”为边界，不尝试做无穷尽全盘扫描。

## Verification Steps

### 静态与编译验证

- 运行 `cargo fmt`
- 运行 `cargo test`
- 运行 `cargo build`
- 检查新增/修改文件的诊断信息是否为零

### 命令面验证

- 校验 `--help` 中文化输出：
  - `vps-cli --help`
  - `vps-cli singbox --help`
  - `vps-cli mieru --help`
  - `vps-cli reclaim --help`
  - `vps-cli setup-ssh --help`
- 校验 JSON 输出结构：
  - 成功场景包含 `ok/data`
  - 失败场景包含 `ok/error`
  - 危险操作可附带 `warnings/backups/rolled_back`

### `singbox` 功能验收

- 安装/升级：
  - 固定版本安装
  - 可选 SHA256 校验
- 节点：
  - 五类协议向导创建
  - 通用 JSON 导入
  - 安全视图列表
  - 敏感链接展示
- 运维：
  - 配置检查
  - 完整配置显示门控
  - 日志查看
  - 状态与启停重启
- 卸载与清理：
  - `reclaim singbox-uninstall` 保留配置
  - `reclaim singbox-purge` 危险确认
  - 审计先于清理

### `mieru` 功能验收

- 安装/升级 mita
- 节点添加与列出
- `mierus://` 链接显示
- 状态、启动、停止、配置显示
- 通过 `reclaim mieru-uninstall` 卸载托管状态

### `setup-ssh` 安全验收

- 非交互模式缺少 `--confirm` 时拒绝执行危险修改
- 公钥内容损坏时拒绝写入
- 写入前创建备份
- `sshd -t` 或 `sshd -T` 校验失败时自动回滚
- `AllowUsers` 与目标用户冲突时拒绝执行
- 人类可读输出与 JSON 输出都能明确说明：
  - 改了什么
  - 备份在哪
  - 是否已回滚

## Implementation Order

1. 调整顶层命令注册，建立 `mieru` 与 `reclaim` 独立命令域骨架。
2. 抽出 `utils` 与 `safety` 的公共能力，先统一输出、确认、备份、回滚模型。
3. 重做 `setup-ssh` 的参数、验证、备份、回滚与生效校验。
4. 扩展 `singbox` 的配置检查、显示、日志与节点向导能力，并移除其中的卸载/清理职责。
5. 实现 `reclaim` 命令域，承载 `sing-box` 与 `mieru` 的卸载、清理、审计能力。
6. 实现 `mieru` 安装、节点、状态、配置能力。
7. 补齐测试、更新 `SKILL.md`，必要时新增 `README.md`。

---

变更时间：2026-05-16
本次变更概要：根据补充要求，将 `sing-box` 脚本中的卸载、清理、审计能力从 `singbox` 命令组中拆出，规划为独立 `reclaim` 命令域，并同步更新目标、模块设计、验收项与实施顺序。
