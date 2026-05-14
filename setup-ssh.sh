#!/usr/bin/env bash
set -euo pipefail

SSHD_CONFIG="/etc/ssh/sshd_config"
BACKUP_DIR="/root/ssh-setup-backups"

log() {
  printf '\033[1;32m[INFO]\033[0m %s\n' "$*"
}

warn() {
  printf '\033[1;33m[WARN]\033[0m %s\n' "$*"
}

die() {
  printf '\033[1;31m[ERROR]\033[0m %s\n' "$*" >&2
  exit 1
}

need_root() {
  if [ "$(id -u)" -ne 0 ]; then
    die "请使用 root 运行：sudo bash $0"
  fi
}

require_ubuntu() {
  if [ ! -r /etc/os-release ]; then
    die "无法读取 /etc/os-release，暂不继续。"
  fi

  . /etc/os-release
  if [ "${ID:-}" != "ubuntu" ]; then
    die "该脚本面向 Ubuntu。当前系统 ID=${ID:-unknown}"
  fi

  log "检测到 Ubuntu ${VERSION_ID:-unknown}"
}

prompt() {
  local message="$1"
  local default_value="${2:-}"
  local input

  if [ -n "$default_value" ]; then
    read -r -p "$message [$default_value]: " input
    printf '%s' "${input:-$default_value}"
  else
    read -r -p "$message: " input
    printf '%s' "$input"
  fi
}

prompt_yes_no() {
  local message="$1"
  local default_value="$2"
  local input
  local suffix

  case "$default_value" in
    y|Y) suffix="Y/n" ;;
    n|N) suffix="y/N" ;;
    *) suffix="y/n" ;;
  esac

  while true; do
    read -r -p "$message [$suffix]: " input
    input="${input:-$default_value}"
    case "$input" in
      y|Y|yes|YES|Yes) return 0 ;;
      n|N|no|NO|No) return 1 ;;
      *) warn "请输入 y 或 n。" ;;
    esac
  done
}

validate_port() {
  local port="$1"

  if ! [[ "$port" =~ ^[0-9]+$ ]]; then
    return 1
  fi

  if [ "$port" -lt 1 ] || [ "$port" -gt 65535 ]; then
    return 1
  fi

  return 0
}

detect_ssh_service() {
  if systemctl list-unit-files ssh.service >/dev/null 2>&1; then
    printf '%s' "ssh"
    return
  fi

  if systemctl list-unit-files sshd.service >/dev/null 2>&1; then
    printf '%s' "sshd"
    return
  fi

  die "未找到 ssh 或 sshd systemd 服务。"
}

current_ports() {
  awk '
    /^[[:space:]]*Port[[:space:]]+[0-9]+/ {
      print $2
    }
  ' "$SSHD_CONFIG" | paste -sd ',' -
}

ensure_backup() {
  mkdir -p "$BACKUP_DIR"
  cp -a "$SSHD_CONFIG" "$BACKUP_DIR/sshd_config.$(date +%Y%m%d%H%M%S)"
  log "已备份 $SSHD_CONFIG 到 $BACKUP_DIR"
}

set_sshd_option() {
  local key="$1"
  local value="$2"
  local file="$SSHD_CONFIG"
  local tmp

  tmp="$(mktemp)"

  awk -v key="$key" -v value="$value" '
    BEGIN { done = 0 }
    {
      line = $0
      trimmed = line
      sub(/^[[:space:]]+/, "", trimmed)
      if (trimmed ~ "^#?[[:space:]]*" key "([[:space:]]+|$)") {
        if (!done) {
          print key " " value
          done = 1
        }
        next
      }
      print line
    }
    END {
      if (!done) {
        print key " " value
      }
    }
  ' "$file" > "$tmp"

  cat "$tmp" > "$file"
  rm -f "$tmp"
}

add_public_key() {
  local target_user="$1"
  local public_key="$2"
  local home_dir
  local ssh_dir
  local auth_file

  home_dir="$(getent passwd "$target_user" | cut -d: -f6)"
  [ -n "$home_dir" ] || die "无法获取用户 $target_user 的 home 目录。"

  ssh_dir="$home_dir/.ssh"
  auth_file="$ssh_dir/authorized_keys"

  mkdir -p "$ssh_dir"
  chmod 700 "$ssh_dir"
  touch "$auth_file"
  chmod 600 "$auth_file"

  if grep -qxF "$public_key" "$auth_file"; then
    log "公钥已存在于 $auth_file，跳过重复写入。"
  else
    printf '%s\n' "$public_key" >> "$auth_file"
    log "已写入公钥到 $auth_file"
  fi

  chown -R "$target_user:$target_user" "$ssh_dir"
}

configure_firewall_port() {
  local port="$1"

  if command -v ufw >/dev/null 2>&1; then
    if ufw status | grep -qi '^Status: active'; then
      ufw allow "${port}/tcp"
      log "UFW 已启用，已放行 TCP ${port}。"
      return
    fi
    warn "UFW 未启用，按要求不修改防火墙状态。"
    return
  fi

  if command -v firewall-cmd >/dev/null 2>&1; then
    if systemctl is-active --quiet firewalld; then
      firewall-cmd --permanent --add-port="${port}/tcp"
      firewall-cmd --reload
      log "firewalld 已启用，已放行 TCP ${port}。"
      return
    fi
    warn "firewalld 未启用，按要求不修改防火墙状态。"
    return
  fi

  warn "未检测到 UFW 或 firewalld，跳过防火墙配置。"
}

configure_firewall() {
  local ports_csv="$1"
  local old_ifs="$IFS"
  local port

  IFS=','
  for port in $ports_csv; do
    IFS="$old_ifs"
    [ -n "$port" ] || continue
    configure_firewall_port "$port"
    IFS=','
  done
  IFS="$old_ifs"
}

test_sshd_config() {
  if sshd -t; then
    log "sshd 配置语法检查通过。"
  else
    die "sshd 配置语法检查失败。已保留备份：$BACKUP_DIR"
  fi
}

reload_ssh_service() {
  local service="$1"

  systemctl reload "$service"
  log "已重载 ${service} 服务。"
}

main() {
  local ssh_service
  local target_user
  local public_key
  local modify_port="n"
  local ssh_port=""
  local existing_ports
  local firewall_ports

  need_root
  require_ubuntu

  [ -f "$SSHD_CONFIG" ] || die "未找到 $SSHD_CONFIG"
  ssh_service="$(detect_ssh_service)"
  existing_ports="$(current_ports)"
  [ -n "$existing_ports" ] || existing_ports="22"

  printf '\n当前检测到的 SSH 端口：%s\n' "$existing_ports"
  printf '当前 SSH 服务名：%s\n\n' "$ssh_service"

  target_user="$(prompt "请输入要添加公钥的 Linux 用户" "root")"
  id "$target_user" >/dev/null 2>&1 || die "用户不存在：$target_user"

  while true; do
    public_key="$(prompt "请粘贴 SSH 公钥")"
    if [[ "$public_key" =~ ^(ssh-ed25519|ssh-rsa|ecdsa-sha2-nistp256|ecdsa-sha2-nistp384|ecdsa-sha2-nistp521)[[:space:]]+ ]]; then
      break
    fi
    warn "公钥格式看起来不正确，请重新输入。"
  done

  if prompt_yes_no "是否修改 SSH 端口？默认不修改当前设置" "n"; then
    modify_port="y"
    while true; do
      ssh_port="$(prompt "请输入新的 SSH 端口" "22222")"
      if validate_port "$ssh_port"; then
        break
      fi
      warn "端口必须是 1-65535 的数字。"
    done
    firewall_ports="$ssh_port"
  else
    ssh_port="${existing_ports%%,*}"
    firewall_ports="$existing_ports"
  fi

  ensure_backup
  add_public_key "$target_user" "$public_key"
  set_sshd_option "PubkeyAuthentication" "yes"
  set_sshd_option "AuthorizedKeysFile" ".ssh/authorized_keys"

  if [ "$modify_port" = "y" ]; then
    set_sshd_option "Port" "$ssh_port"
    log "已设置 SSH 端口为 ${ssh_port}。"
  else
    log "未修改 SSH 端口配置。"
  fi

  if prompt_yes_no "是否禁止 root 密码登录？默认不修改当前设置" "n"; then
    set_sshd_option "PermitRootLogin" "prohibit-password"
    log "已禁止 root 密码登录。"
  else
    log "未修改 root 密码登录相关配置。"
  fi

  if prompt_yes_no "是否禁止 root 密钥登录？默认不修改当前设置" "n"; then
    set_sshd_option "PermitRootLogin" "no"
    log "已禁止 root 通过 SSH 登录，包括密钥登录。"
  else
    log "未修改 root 密钥登录相关配置。"
  fi

  test_sshd_config
  configure_firewall "$firewall_ports"
  reload_ssh_service "$ssh_service"

  printf '\n完成。\n'
  printf '请新开一个终端验证 SSH 登录，确认无误前不要关闭当前会话。\n'
  printf '测试命令示例：ssh -p %s %s@<VPS_IP>\n' "$ssh_port" "$target_user"
}

main "$@"
