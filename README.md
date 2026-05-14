# sh

Ubuntu VPS SSH 初始化脚本集合。

## setup-ssh.sh

`setup-ssh.sh` 是一个交互式 SSH 安全配置脚本，适配 Ubuntu 多个版本，适合 VPS 初始加固时使用。

功能：

- 手动输入 SSH 公钥，并写入指定用户的 `authorized_keys`。
- 手动选择是否修改 SSH 端口，默认不修改当前设置。
- 手动选择是否禁止 root 密码登录，默认不修改当前设置。
- 手动选择是否禁止 root 密钥登录，默认不修改当前设置。
- 自动检测当前 SSH 服务名并重载服务。
- 自动检测当前 SSH 端口。
- 如果 UFW 或 firewalld 已启用，自动放行 SSH 端口。
- 如果防火墙未启用，不主动启用或修改防火墙状态。
- 修改前自动备份 `/etc/ssh/sshd_config`。

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
