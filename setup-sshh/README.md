# sh

Ubuntu VPS SSH 配置脚本集合。

## setup-ssh.sh

`setup-ssh.sh` 是一个交互式 SSH 配置脚本，适配 Ubuntu 多个版本，偏向自用 VPS 的密钥轮换和基础 SSH 配置维护。

脚本设计目标：

- 给当前运行中的 VPS 替换 SSH 登录公钥。
- 可重复执行。
- 每次执行前保留当前 SSH 会话，执行后新开终端验证。
- 对 SSH 配置使用受控 drop-in 文件，尽量减少直接改写主配置。

功能：

- 可选择添加新 SSH 公钥。
- 添加新 SSH 公钥时，会备份目标用户原 `authorized_keys`，然后清理其他公钥，只保留本次输入的新公钥。
- 新公钥默认接受 `ssh-ed25519` 和 ECDSA，拒绝旧式 `ssh-rsa`。
- 可选择跳过公钥添加；跳过时不会创建、修改或清理任何 `authorized_keys`。
- 手动选择是否修改 SSH 端口，默认不修改当前设置。
- 手动选择是否禁止 root 密码登录，默认不修改当前设置。
- 手动选择是否禁止 root SSH 登录，默认不修改当前设置。
- 手动选择是否禁用全局 SSH 密码认证，默认不修改当前设置。
- 手动选择是否设置 `AllowUsers` 登录白名单，默认不修改当前设置。
- 手动选择是否写入基础 SSH 加固项，默认不修改当前设置。
- 修改 SSH 端口时，只放行新端口；旧端口规则需在新登录验证成功后手动清理。
- 自动检测当前 SSH 服务名并重载服务。
- 自动检测当前 SSH 端口。
- 如果 UFW 或 firewalld 已启用，自动放行 SSH 端口。
- 如果防火墙未启用，不主动启用或修改防火墙状态。
- 修改前自动备份 `/etc/ssh/sshd_config`、受控 SSH drop-in 配置和被替换的 `authorized_keys`。

## 密钥轮换行为

如果选择添加新公钥，脚本会对目标用户执行“替换”而不是“追加”：

```text
旧 authorized_keys -> 自动备份
新 authorized_keys -> 只包含本次输入的新公钥
```

这意味着该用户原先的其他 SSH 公钥会被清理。该行为适合自用 VPS 密钥轮换，但不适合多人共用账号。脚本会在替换前进行二次确认。

如果选择跳过添加公钥，脚本不会触碰任何用户的 `authorized_keys`，也不会清理旧密钥。

## 使用方式

在 VPS 上执行：

```bash
curl -fsSL https://raw.githubusercontent.com/gray0128/sh/main/setup-ssh.sh -o setup-ssh.sh
sudo bash setup-ssh.sh
```

或手动上传后执行：

```bash
chmod +x setup-ssh.sh
sudo ./setup-ssh.sh
```

## 安全提示

运行前建议确认 VPS 控制台可用。修改 SSH 配置时，不要关闭当前 SSH 会话；脚本完成后请新开一个终端验证登录。

如果修改了 SSH 端口，还需要确认云厂商安全组已放行对应端口。

## 备份位置

脚本会将备份保存到：

```text
/root/ssh-setup-backups
```

常见备份包括：

- `sshd_config.<timestamp>`
- `00-vps-ssh-setup.conf.<timestamp>`
- `authorized_keys.<user>.<timestamp>`

## 注意事项

- 修改 SSH 端口时，脚本会写入受控配置文件，但不会主动删除系统其他位置已有的 `Port` 配置。
- 脚本不会自动清理旧端口规则；请在新端口登录成功后，再手动收口本机防火墙和云安全组。
- 新 SSH 端口交互输入限制为 `1024-65535`。
- 默认“不修改”的 SSH 配置项会保留脚本此前写入的受控配置。
- 新公钥会优先通过 `ssh-keygen` 校验，避免把明显损坏的公钥写入 `authorized_keys`。
- OpenSSH 8.8 起默认禁用 `ssh-rsa` SHA-1 签名；脚本默认拒绝 `ssh-rsa` 公钥，建议使用 `ssh-ed25519`。
- 禁止 root SSH 登录会写入 `PermitRootLogin no`，这会同时禁止 root 密码和密钥登录。
- 禁用全局 SSH 密码认证会写入 `PasswordAuthentication no`，并按当前 OpenSSH 支持情况写入 `KbdInteractiveAuthentication no` 或 `ChallengeResponseAuthentication no`。确认密钥登录可用前请谨慎启用。
- 基础 SSH 加固项包括：`PermitEmptyPasswords no`、`X11Forwarding no`、`MaxAuthTries 3`、`LoginGraceTime 30`、`UseDNS no`。
- `AllowUsers` 白名单只适合明确知道哪些账号需要 SSH 登录的场景。
- 如果同时轮换密钥并设置 `AllowUsers`，脚本会检查白名单是否包含本次替换密钥的用户。
- 脚本会用 `sshd -T` 校验本次明确选择的关键配置是否实际生效；若被主配置中更早的全局指令覆盖，会恢复并退出。
- 如果系统已有复杂的 `Include` 或 `Match` 配置，请先人工检查 `/etc/ssh/sshd_config`。
- 如果本机防火墙未启用，脚本不会主动启用防火墙。
- 云厂商安全组不属于 VPS 内部防火墙，仍需在云控制台确认。
