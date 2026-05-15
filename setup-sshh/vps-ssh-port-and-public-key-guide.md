# VPS 修改 SSH 端口与添加公钥手顺

适用场景：Linux VPS 上将 SSH 默认端口 `22` 修改为自定义端口，并为普通用户添加 SSH 公钥登录。

> 建议先在 VPS 控制台确认有 Web Console / VNC / Serial Console 等紧急登录方式。修改 SSH 配置前，不要关闭当前已登录的 SSH 会话。

## 1. 准备信息

示例变量如下，请按实际情况替换：

```bash
VPS_IP="203.0.113.10"
SSH_USER="deploy"
NEW_SSH_PORT="22222"
PUBLIC_KEY='ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA... your-name@local'
```

端口建议：

- 使用 `1024-65535` 之间的端口。
- 避免使用已被服务占用的端口，例如 `80`、`443`、`3306`、`5432`。
- 确认云厂商安全组、防火墙规则允许新端口入站。

## 2. 登录 VPS

使用当前可用方式登录 VPS：

```bash
ssh root@VPS_IP
```

如果已有普通用户：

```bash
ssh SSH_USER@VPS_IP
```

登录后切换到 root 或使用 `sudo`：

```bash
sudo -i
```

## 3. 创建或确认登录用户

如果已经有目标用户，可跳过创建步骤。

```bash
id deploy
```

如用户不存在，创建用户并加入 sudo 组：

Debian / Ubuntu：

```bash
adduser deploy
usermod -aG sudo deploy
```

CentOS / Rocky Linux / AlmaLinux：

```bash
adduser deploy
passwd deploy
usermod -aG wheel deploy
```

## 4. 添加 SSH 公钥

切换到目标用户的 home 目录并写入公钥：

```bash
mkdir -p /home/deploy/.ssh
chmod 700 /home/deploy/.ssh
```

编辑 `authorized_keys`：

```bash
nano /home/deploy/.ssh/authorized_keys
```

粘贴你的公钥，例如：

```text
ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA... your-name@local
```

保存后设置权限：

```bash
chmod 600 /home/deploy/.ssh/authorized_keys
chown -R deploy:deploy /home/deploy/.ssh
```

如果系统启用了 SELinux，执行：

```bash
restorecon -Rv /home/deploy/.ssh
```

## 5. 修改 SSH 端口

编辑 SSH 服务配置：

```bash
nano /etc/ssh/sshd_config
```

找到或新增以下配置：

```sshconfig
Port 22222
PubkeyAuthentication yes
AuthorizedKeysFile .ssh/authorized_keys
PermitRootLogin prohibit-password
PasswordAuthentication no
```

说明：

- `Port 22222`：将 SSH 监听端口改为 `22222`。
- `PubkeyAuthentication yes`：启用公钥登录。
- `PermitRootLogin prohibit-password`：禁止 root 密码登录，但保留 root 公钥登录能力。如不需要 root SSH 登录，可改为 `PermitRootLogin no`。
- `PasswordAuthentication no`：关闭密码登录。确认公钥登录成功前，可以先临时保持为 `yes`。

检查配置语法：

```bash
sshd -t
```

如果没有输出，表示语法检查通过。

## 6. 放行新 SSH 端口

先放行新端口，再重载 SSH 服务。

### 使用 UFW

```bash
ufw allow 22222/tcp
ufw status
```

如果 UFW 未启用，谨慎启用：

```bash
ufw allow 22222/tcp
ufw enable
```

### 使用 firewalld

```bash
firewall-cmd --permanent --add-port=22222/tcp
firewall-cmd --reload
firewall-cmd --list-ports
```

### 使用 iptables

```bash
iptables -I INPUT -p tcp --dport 22222 -j ACCEPT
```

持久化规则根据系统不同而不同。Debian / Ubuntu 可安装并保存：

```bash
apt update
apt install -y iptables-persistent
netfilter-persistent save
```

CentOS / Rocky Linux / AlmaLinux 可使用：

```bash
service iptables save
```

### 云厂商安全组

在 VPS 控制台中放行入站 TCP 端口：

```text
协议：TCP
端口：22222
来源：你的公网 IP/32
```

如不确定本机公网 IP，可在本地执行：

```bash
curl ifconfig.me
```

## 7. 重载 SSH 服务

优先使用 reload，避免直接断开当前连接：

```bash
systemctl reload ssh
```

如果服务名不是 `ssh`，使用：

```bash
systemctl reload sshd
```

查看服务状态：

```bash
systemctl status ssh --no-pager
```

或：

```bash
systemctl status sshd --no-pager
```

确认新端口正在监听：

```bash
ss -tlnp | grep 22222
```

## 8. 新开终端验证登录

不要关闭原来的 SSH 会话。新开一个本地终端，使用新端口测试：

```bash
ssh -p 22222 deploy@VPS_IP
```

如使用指定私钥：

```bash
ssh -i ~/.ssh/id_ed25519 -p 22222 deploy@VPS_IP
```

登录成功后，确认 sudo 权限：

```bash
sudo whoami
```

输出应为：

```text
root
```

## 9. 收口旧端口

确认新端口、公钥登录、sudo 都正常后，再移除旧端口 `22` 的防火墙放行规则。

UFW：

```bash
ufw delete allow 22/tcp
ufw status
```

firewalld：

```bash
firewall-cmd --permanent --remove-service=ssh
firewall-cmd --reload
```

如云厂商安全组仍放行 `22`，也应删除或限制来源 IP。

## 10. 常见故障处理

### 无法通过新端口连接

在旧 SSH 会话或 VPS 控制台中检查：

```bash
sshd -t
ss -tlnp | grep 22222
systemctl status ssh --no-pager
systemctl status sshd --no-pager
```

重点确认：

- `/etc/ssh/sshd_config` 中 `Port` 是否正确。
- 防火墙是否放行新端口。
- 云厂商安全组是否放行新端口。
- SSH 服务是否已 reload。

### 公钥登录失败

检查权限：

```bash
ls -ld /home/deploy /home/deploy/.ssh
ls -l /home/deploy/.ssh/authorized_keys
```

推荐权限：

```text
/home/deploy                  755 或更严格
/home/deploy/.ssh             700
/home/deploy/.ssh/authorized_keys 600
```

检查日志：

Debian / Ubuntu：

```bash
journalctl -u ssh -n 100 --no-pager
```

CentOS / Rocky Linux / AlmaLinux：

```bash
journalctl -u sshd -n 100 --no-pager
```

### 被锁在服务器外

使用 VPS 控制台登录，恢复 `/etc/ssh/sshd_config`：

```sshconfig
Port 22
PasswordAuthentication yes
```

然后放行 22 端口并重载 SSH：

```bash
systemctl reload ssh
```

或：

```bash
systemctl reload sshd
```

## 11. 建议加固项

可选但推荐：

```sshconfig
PermitEmptyPasswords no
X11Forwarding no
MaxAuthTries 3
ClientAliveInterval 300
ClientAliveCountMax 2
AllowUsers deploy
```

如果服务器只允许固定 IP 登录，优先在云厂商安全组和系统防火墙中限制来源 IP。

## 12. 最终检查清单

- [ ] 当前 SSH 会话未关闭。
- [ ] 已添加目标用户公钥。
- [ ] `/etc/ssh/sshd_config` 已配置新端口。
- [ ] `sshd -t` 语法检查通过。
- [ ] 系统防火墙已放行新端口。
- [ ] 云厂商安全组已放行新端口。
- [ ] 新终端可通过 `ssh -p 22222 deploy@VPS_IP` 登录。
- [ ] 目标用户 sudo 权限正常。
- [ ] 确认无误后，旧端口 `22` 已删除或限制来源。
