# singbox_deploy_hardened_v17_zh.sh

面向个人 VPS 的安全加固型 sing-box 部署、节点管理与清理脚本。

该版本在 v16 基础上将主要用户交互内容改为中文，包括菜单、提示、确认、警告和帮助信息。变量名、函数名、JSON 字段、协议名、命令名等技术标识仍保持英文，以避免破坏脚本逻辑。

脚本主要用于：

- 安装 / 更新 sing-box；
- 管理 VLESS Reality、Trojan TLS、Hysteria2 TLS、TUIC TLS、Shadowsocks 节点；
- 以独立 mita 服务方式管理 mieru；
- 审计并清理 233boy / sing-box / xray / v2ray 相关部署；
- 可选清理 nginx / caddy；
- 保持配置备份、校验、回滚和最小权限运行。

> 该脚本不是完整控制面板，目标是“自用、可审计、可回滚、影响范围可控”。

---

## 脚本地址

GitHub 页面：

```text
https://github.com/gray0128/sh/blob/main/sing-box/singbox_deploy_hardened_v17_zh.sh
```

Raw 下载地址：

```text
https://raw.githubusercontent.com/gray0128/sh/main/sing-box/singbox_deploy_hardened_v17_zh.sh
```

如果仓库中仍使用旧文件名，请将下面命令里的文件名替换为实际路径。

---

## 支持环境

主要目标环境：

- Debian / Ubuntu；
- RHEL / Rocky / AlmaLinux / CentOS / Fedora；
- 其他 systemd VPS 发行版。

脚本要求：

- root 权限；
- systemd；
- 可访问 GitHub；
- 服务器网络和防火墙允许对应端口入站。

脚本包含 best-effort 的 `apk` / `pacman` / `zypper` 分支，但 Alpine / OpenRC 不是主要目标环境。

---

## 快速安装

### 使用 curl

```bash
curl -fsSL -o singbox_deploy_hardened_v17_zh.sh \
  https://raw.githubusercontent.com/gray0128/sh/main/sing-box/singbox_deploy_hardened_v17_zh.sh

chmod +x singbox_deploy_hardened_v17_zh.sh

sudo ./singbox_deploy_hardened_v17_zh.sh
```

### 使用 wget

```bash
wget -O singbox_deploy_hardened_v17_zh.sh \
  https://raw.githubusercontent.com/gray0128/sh/main/sing-box/singbox_deploy_hardened_v17_zh.sh

chmod +x singbox_deploy_hardened_v17_zh.sh

sudo ./singbox_deploy_hardened_v17_zh.sh
```

### 临时下载并直接运行

不建议在正式服务器上直接管道执行远程脚本。更推荐先下载、查看，再运行：

```bash
curl -fsSL -o singbox_deploy_hardened_v17_zh.sh \
  https://raw.githubusercontent.com/gray0128/sh/main/sing-box/singbox_deploy_hardened_v17_zh.sh

less singbox_deploy_hardened_v17_zh.sh
sudo bash singbox_deploy_hardened_v17_zh.sh
```

---

## 推荐：带 SHA256 校验安装 sing-box

脚本支持固定版本下载和 SHA256 校验。正式 VPS 建议提供 sing-box release tarball 的 SHA256：

```bash
sudo ./singbox_deploy_hardened_v17_zh.sh \
  --sha256 <sing-box-release-tarball-sha256>
```

如果暂时只是测试，也可以显式允许未校验下载：

```bash
sudo ./singbox_deploy_hardened_v17_zh.sh --allow-unverified-download
```

不建议在正式服务器上长期使用 `--allow-unverified-download`。

---

## mieru / mita 安装参数

脚本将 mieru 作为独立的 mita 服务管理，不会把 mieru 塞进 sing-box 配置。

安装 mita 时可指定版本和安装包校验值：

```bash
sudo ./singbox_deploy_hardened_v17_zh.sh \
  --mieru-version 3.32.0 \
  --mieru-sha256 <mita-deb-or-rpm-sha256>
```

测试时也可以配合：

```bash
sudo ./singbox_deploy_hardened_v17_zh.sh --allow-unverified-download
```

v16 起已修复 mieru-only 场景缺少 `jq` 的问题。安装 / 添加 mieru 节点前会检查常见运行依赖，例如 `curl`、`jq`、`openssl`、`sha256sum`、`ss`。

---

## 菜单功能概览

运行脚本后会进入中文交互菜单：

```text
1) 安装 / 更新 sing-box 二进制和 systemd 服务
2) 添加 VLESS + Reality 节点
3) 添加 Trojan + TLS 节点
4) 添加 Hysteria2 + TLS 节点
5) 添加 TUIC + TLS 节点
6) 添加 Shadowsocks 节点
7) 查看节点，安全视图
8) 查看节点链接，敏感
9) 删除节点
10) 检查配置
11) 显示完整配置 JSON，敏感
12) 启动服务
13) 停止服务
14) 重启服务
15) 查看日志
16) 卸载二进制和服务，保留配置
17) 清理所有托管文件，危险
18) 安装 / 更新 mita 服务端，用于 mieru
19) 添加 mieru 节点，独立 mita 服务
20) 查看 mieru 节点，安全视图
21) 查看 mieru 链接，敏感
22) 启动 mita
23) 停止 mita
24) 查看 mita 状态 / 配置
25) 审计 233boy/sing-box/xray/v2ray 清理候选项
26) 清理代理候选项，然后可选清理 nginx/caddy
27) 审计可选 nginx 清理候选项
28) 清理 nginx，需要显式确认
29) 审计可选 caddy 清理候选项
30) 清理 caddy，需要显式确认
31) 卸载 mita / mieru 托管状态
0) 退出
```

---

## 常用流程

### 1. 安装 sing-box

选择：

```text
1) 安装 / 更新 sing-box 二进制和 systemd 服务
```

脚本会：

- 安装依赖；
- 下载固定版本 sing-box；
- 可选校验 SHA256；
- 创建 `singbox` 系统用户；
- 初始化 `/etc/sing-box`；
- 生成 systemd 服务；
- 执行基础配置检查。

---

### 2. 添加 VLESS Reality 节点

选择：

```text
2) 添加 VLESS + Reality 节点
```

脚本会提示输入：

- TCP 监听端口；
- 客户端使用的服务器公网域名 / IP；
- 是否启用 IPv6 / dual-stack；
- Reality SNI / handshake 域名。

添加成功后会输出：

- 节点 URI；
- sing-box client outbound JSON 片段。

---

### 3. 添加 TLS 类节点

支持：

```text
Trojan TLS
Hysteria2 TLS
TUIC TLS
```

添加时可以：

- 提供已有证书路径；
- 或让脚本生成自签名证书。

生产环境建议使用真实证书。自签名证书适合测试，客户端通常需要允许 insecure。

---

### 4. 添加 Shadowsocks 节点

选择：

```text
6) 添加 Shadowsocks 节点
```

脚本内置方法菜单，避免手动输入错误 method。

支持：

```text
2022-blake3-aes-128-gcm
2022-blake3-aes-256-gcm
2022-blake3-chacha20-poly1305
aes-128-gcm
aes-256-gcm
chacha20-ietf-poly1305
xchacha20-ietf-poly1305
```

---

### 5. 安装并使用 mieru / mita

mieru 不是官方 sing-box 的普通 inbound 类型。脚本以独立 mita 服务方式支持。

安装 mita：

```text
18) 安装 / 更新 mita 服务端，用于 mieru
```

添加 mieru 节点：

```text
19) 添加 mieru 节点，独立 mita 服务
```

脚本会生成：

- mita server config；
- `mierus://` simple link；
- mieru client JSON。

---

## IPv6 支持说明

脚本支持务实的 IPv6 场景：

- public host 可输入域名、IPv4 或 IPv6 literal；
- IPv6 literal 会自动在 URI 中转为 `[IPv6]` 格式；
- IPv6 public host 会自动使用 `::` 监听；
- 域名 / IPv4 默认仍使用 `0.0.0.0`，可手动选择 `::`。

注意：

- Reality SNI 仍要求域名；
- 不支持 IPv6 zone id，例如 `fe80::1%eth0`；
- 如果监听 `::`，IPv4 是否同时可用取决于系统 `net.ipv6.bindv6only`；
- 脚本不会自动修改 IPv6 防火墙、云安全组或内核参数。

---

## 文件与目录

sing-box 相关：

```text
/opt/sing-box/sing-box
/usr/local/bin/sing-box
/etc/sing-box/config.json
/etc/sing-box/nodes.json
/etc/sing-box/links.txt
/etc/sing-box/certs/
/etc/sing-box/backups/
/var/log/sing-box/
```

mieru / mita 相关：

```text
/etc/mieru-managed/nodes.json
/etc/mieru-managed/links.txt
/etc/mieru-managed/mita_config.json
```

systemd 服务：

```text
sing-box.service
mita.service
```

---

## 安全设计

脚本采用以下安全策略：

- 不执行远程 shell 脚本；
- 固定 sing-box / mita 版本；
- 支持 SHA256 校验；
- 下载时使用 HTTPS；
- 解压前检查 tar 路径穿越、symlink、hardlink；
- sing-box 以专用 `singbox` 用户运行；
- 配置变更前备份；
- 配置变更后执行 `sing-box check`；
- 校验失败自动回滚；
- 节点链接和敏感配置使用 root-only 权限；
- 不自动修改防火墙；
- 不自动删除 nginx / caddy，除非用户显式确认。

---

## 清理功能说明

### 审计代理服务

选择：

```text
25) 审计 233boy/sing-box/xray/v2ray 清理候选项
```

只展示候选项，不执行删除。

会检查：

- 233boy 风格 sing-box；
- sing-box / singbox；
- xray；
- v2ray；
- 常见 unit、配置、日志、数据、二进制路径；
- `/root/.bashrc` 中相关 alias；
- cron 中相关引用。

cron 只报告，不自动编辑。

---

### 清理代理服务

选择：

```text
26) 清理代理候选项，然后可选清理 nginx/caddy
```

会先展示审计结果，再要求确认。

清理范围包括：

```text
/etc/sing-box
/etc/xray
/etc/v2ray
/opt/sing-box
/opt/xray
/opt/v2ray
/usr/local/bin/sing-box
/usr/local/bin/xray
/usr/local/bin/v2ray
/usr/local/bin/sb
/var/log/sing-box
/var/log/xray
/var/log/v2ray
```

不会自动删除：

```text
nginx
caddy
ssh
数据库
Docker
防火墙规则
云安全组
```

代理清理完成后，会询问是否额外删除 nginx / caddy。

---

### 可选清理 nginx / caddy

单独审计：

```text
27) 审计可选 nginx 清理候选项
29) 审计可选 caddy 清理候选项
```

单独删除：

```text
28) 清理 nginx，需要显式确认
30) 清理 caddy，需要显式确认
```

删除前会备份配置 / 状态到：

```text
/root/web-cleanup-backup.nginx.<timestamp>.XXXXXX
/root/web-cleanup-backup.caddy.<timestamp>.XXXXXX
```

不会删除：

```text
/var/www
```

日志不会自动备份。如果需要保留日志，请先手动复制：

```bash
sudo cp -a /var/log/nginx /root/nginx-log-backup
sudo cp -a /var/log/caddy /root/caddy-log-backup
```

---

## 卸载

### 卸载 sing-box 二进制和服务，保留配置

选择：

```text
16) 卸载二进制和服务，保留配置
```

会删除：

```text
/opt/sing-box
/usr/local/bin/sing-box
sing-box.service
```

保留：

```text
/etc/sing-box
```

---

### 危险：彻底清理脚本管理的 sing-box 文件

选择：

```text
17) 清理所有托管文件，危险
```

会删除脚本管理的 sing-box 文件、配置、证书、节点链接和备份。

---

### 卸载 mita / mieru

选择：

```text
31) 卸载 mita / mieru 托管状态
```

会处理：

```text
mita service
mita package
/etc/mieru-managed
```

删除前会备份：

```text
/root/mieru-managed-backup.<timestamp>.XXXXXX
```

不会删除 sing-box / xray / v2ray / nginx / caddy。

---

## 防火墙与云安全组

脚本不会自动放行端口。

添加节点后，请手动确认：

- OS 防火墙；
- 云厂商安全组；
- IPv4 / IPv6 入站规则。

示例：

```bash
sudo ufw allow 443/tcp
sudo ufw allow 443/udp
```

或：

```bash
sudo firewall-cmd --permanent --add-port=443/tcp
sudo firewall-cmd --permanent --add-port=443/udp
sudo firewall-cmd --reload
```

---

## 常用排错命令

检查 sing-box 配置：

```bash
sudo /opt/sing-box/sing-box check -c /etc/sing-box/config.json
```

查看 sing-box 状态：

```bash
sudo systemctl status sing-box --no-pager -l
```

查看 sing-box 日志：

```bash
sudo journalctl -u sing-box -n 100 --no-pager
```

查看监听端口：

```bash
sudo ss -lntup | grep -E 'sing-box|mita|xray|v2ray'
```

查看 mita 状态：

```bash
sudo systemctl status mita --no-pager -l
sudo mita status
sudo mita describe config
```

---

## 注意事项

1. 建议先在新 VPS 或快照环境中测试。
2. 使用前建议阅读脚本，确认删除范围。
3. 正式使用建议提供 SHA256。
4. 节点链接和配置包含密码、UUID、密钥等敏感信息，不要贴到公开日志。
5. URI 兼容性因客户端而异；sing-box 客户端优先使用脚本输出的 JSON snippet。
6. 自签名证书适合测试，不建议作为长期生产方案。
7. 清理 nginx / caddy 前，确认 `/var/www` 之外没有其他需要保留的配置或二进制。
8. 如果系统缺少 `jq`、`openssl`、`ss` 等依赖，脚本会尝试自动安装；如果包管理器不受支持，需要手动安装。

---

## License

按仓库实际许可证为准。若仓库未声明许可证，默认保留所有权利。
