#!/usr/bin/env bash
set -euo pipefail

SSHD_CONFIG="/etc/ssh/sshd_config"
SSHD_CONFIG_DIR="/etc/ssh/sshd_config.d"
MANAGED_CONFIG="$SSHD_CONFIG_DIR/00-vps-ssh-setup.conf"
BACKUP_DIR="/root/ssh-setup-backups"
SUPPORTS_KBD_INTERACTIVE="n"
SUPPORTS_CHALLENGE_RESPONSE="n"
LAST_BACKUP_FILE=""
LAST_BACKUP_HAD_FILE=""
MAIN_CONFIG_BACKUP=""
MANAGED_CONFIG_BACKUP=""

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
    die "请使用 root 运行，例如：sudo bash setup-ssh.sh"
  fi
}

show_intro() {
  cat <<'EOF'
该脚本用于当前 Ubuntu VPS 的 SSH 配置维护，偏向自用密钥轮换场景。

重要提示：
1. 请先确认你仍保留一个已登录的 SSH 会话，不要在验证新登录前关闭它。
2. 如果选择添加新公钥，脚本会把目标用户 authorized_keys 清理为“仅保留这一个新公钥”。
3. 公钥建议使用 ssh-ed25519；脚本默认拒绝旧式 ssh-rsa 公钥。
4. 如果选择跳过添加公钥，脚本不会创建、修改或清理 authorized_keys。
5. 修改 SSH 端口只会写入受控配置，不主动删除系统其他位置已有的 Port 配置或防火墙旧规则。
6. 默认“不修改”的 SSH 配置项会保留本脚本此前写入的受控配置。
7. 如果云厂商安全组存在，仍需你在控制台确认对应 SSH 端口已放行。

EOF
}

os_release_value() {
  local key="$1"
  awk -F= -v key="$key" '
    $1 == key {
      value = $2
      gsub(/^"/, "", value)
      gsub(/"$/, "", value)
      print value
      exit
    }
  ' /etc/os-release
}

require_ubuntu() {
  local os_id
  local version_id

  [ -r /etc/os-release ] || die "无法读取 /etc/os-release，暂不继续。"
  os_id="$(os_release_value ID)"
  version_id="$(os_release_value VERSION_ID)"

  [ "$os_id" = "ubuntu" ] || die "该脚本面向 Ubuntu。当前系统 ID=${os_id:-unknown}"
  log "检测到 Ubuntu ${version_id:-unknown}"
}

prompt() {
  local message="$1"
  local default_value="${2:-}"
  local input

  if [ -n "$default_value" ]; then
    if ! read -r -p "$message [$default_value]: " input; then
      die "读取输入失败，已退出。"
    fi
    printf '%s' "${input:-$default_value}"
  else
    if ! read -r -p "$message: " input; then
      die "读取输入失败，已退出。"
    fi
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
    if ! read -r -p "$message [$suffix]: " input; then
      die "读取输入失败，已退出。"
    fi
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

  [[ "$port" =~ ^[0-9]+$ ]] || return 1
  [ "$port" -ge 1 ] && [ "$port" -le 65535 ]
}

validate_ssh_port() {
  local port="$1"

  validate_port "$port" || return 1
  [ "$port" -ge 1024 ] && [ "$port" -le 65535 ]
}

validate_username() {
  local username="$1"

  [[ "$username" =~ ^[a-z_][a-z0-9_-]*[$]?$ ]]
}

validate_public_key() {
  local public_key="$1"
  local tmp

  [[ "$public_key" =~ ^(ssh-ed25519|ecdsa-sha2-nistp256|ecdsa-sha2-nistp384|ecdsa-sha2-nistp521)[[:space:]][A-Za-z0-9+/=]+([[:space:]].*)?$ ]] || return 1

  if command -v ssh-keygen >/dev/null 2>&1; then
    tmp="$(mktemp)"
    printf '%s\n' "$public_key" > "$tmp"
    if ssh-keygen -l -f "$tmp" >/dev/null 2>&1; then
      rm -f "$tmp"
      return 0
    fi
    rm -f "$tmp"
    return 1
  fi

  warn "未找到 ssh-keygen，仅完成公钥格式基础校验。"
  return 0
}

find_sshd_binary() {
  if command -v sshd >/dev/null 2>&1; then
    command -v sshd
    return
  fi

  if [ -x /usr/sbin/sshd ]; then
    printf '%s\n' "/usr/sbin/sshd"
    return
  fi

  die "未找到 sshd。请先安装 openssh-server。"
}

detect_ssh_service() {
  if systemctl cat ssh.service >/dev/null 2>&1; then
    printf '%s' "ssh"
    return
  fi

  if systemctl cat sshd.service >/dev/null 2>&1; then
    printf '%s' "sshd"
    return
  fi

  die "未找到 ssh 或 sshd systemd 服务。"
}

ensure_run_dir() {
  mkdir -p /run/sshd
  chown root:root /run/sshd
  chmod 755 /run/sshd
}

current_ports() {
  local sshd_bin="$1"
  local config_dump
  local ports

  if ! config_dump="$("$sshd_bin" -T 2>/dev/null)"; then
    die "无法读取 sshd 最终配置，请先检查当前 SSH 配置。"
  fi

  ports="$(printf '%s\n' "$config_dump" | awk '
    $1 == "port" && $2 ~ /^[0-9]+$/ && !seen[$2]++ {
      print $2
    }
  ' | paste -sd ',' -)"

  if [ -n "$ports" ]; then
    printf '%s' "$ports"
  else
    printf '%s' "22"
  fi
}

detect_sshd_keyword_support() {
  local sshd_bin="$1"
  local tmp_config

  tmp_config="$(mktemp)"
  printf 'KbdInteractiveAuthentication no\n' > "$tmp_config"
  if "$sshd_bin" -t -f "$tmp_config" >/dev/null 2>&1; then
    SUPPORTS_KBD_INTERACTIVE="y"
  fi
  rm -f "$tmp_config"

  tmp_config="$(mktemp)"
  printf 'ChallengeResponseAuthentication no\n' > "$tmp_config"
  if "$sshd_bin" -t -f "$tmp_config" >/dev/null 2>&1; then
    SUPPORTS_CHALLENGE_RESPONSE="y"
  fi
  rm -f "$tmp_config"
}

ensure_backup() {
  local stamp

  stamp="$(date +%Y%m%d%H%M%S).$$"
  mkdir -p "$BACKUP_DIR"
  MAIN_CONFIG_BACKUP="$BACKUP_DIR/sshd_config.$stamp"
  MANAGED_CONFIG_BACKUP=""
  cp -a "$SSHD_CONFIG" "$MAIN_CONFIG_BACKUP"

  if [ -f "$MANAGED_CONFIG" ]; then
    MANAGED_CONFIG_BACKUP="$BACKUP_DIR/00-vps-ssh-setup.conf.$stamp"
    cp -a "$MANAGED_CONFIG" "$MANAGED_CONFIG_BACKUP"
  fi

  log "已备份 SSH 配置到 $BACKUP_DIR"
}

backup_file_if_exists() {
  local file="$1"
  local label="$2"
  local stamp

  LAST_BACKUP_FILE=""
  LAST_BACKUP_HAD_FILE=""
  if [ ! -e "$file" ]; then
    LAST_BACKUP_HAD_FILE="n"
    return 0
  fi

  stamp="$(date +%Y%m%d%H%M%S).$$"
  mkdir -p "$BACKUP_DIR"
  cp -a "$file" "$BACKUP_DIR/${label}.${stamp}" || return 1
  LAST_BACKUP_FILE="$BACKUP_DIR/${label}.${stamp}"
  LAST_BACKUP_HAD_FILE="y"
  log "已备份 $file 到 $BACKUP_DIR/${label}.${stamp}"
}

write_file_atomic() {
  local target="$1"
  local mode="$2"
  local tmp

  tmp="$(mktemp "${target}.tmp.XXXXXX")"
  cat > "$tmp"
  chmod "$mode" "$tmp"
  mv -f "$tmp" "$target"
}

ensure_include_enabled() {
  local tmp

  mkdir -p "$SSHD_CONFIG_DIR"

  if awk '
    /^[[:space:]]*Match([[:space:]]+|$)/ {
      exit 1
    }
    /^[[:space:]]*Include[[:space:]]+\/etc\/ssh\/sshd_config\.d\/\*\.conf([[:space:]]+|$)/ {
      found = 1
      exit 0
    }
    END { exit found ? 0 : 1 }
  ' "$SSHD_CONFIG"; then
    return
  fi

  if awk '
    /^[[:space:]]*Include[[:space:]]+\/etc\/ssh\/sshd_config\.d\/\*\.conf([[:space:]]+|$)/ {
      found = 1
    }
    END { exit found ? 0 : 1 }
  ' "$SSHD_CONFIG"; then
    die "$SSHD_CONFIG 已包含 $SSHD_CONFIG_DIR/*.conf，但位置可能在 Match 块内或其后。请先手动整理 Include 位置。"
  fi

  warn "$SSHD_CONFIG 未启用 $SSHD_CONFIG_DIR/*.conf，将自动插入 Include。"
  tmp="$(mktemp "${SSHD_CONFIG}.tmp.XXXXXX")"

  awk '
    BEGIN {
      print "Include /etc/ssh/sshd_config.d/*.conf"
    }
    { print }
  ' "$SSHD_CONFIG" > "$tmp"

  chmod 644 "$tmp"
  mv -f "$tmp" "$SSHD_CONFIG"
}

managed_option_value() {
  local key="$1"

  [ -f "$MANAGED_CONFIG" ] || return 0

  awk -v key="$key" '
    $1 == key {
      $1 = ""
      sub(/^[[:space:]]+/, "")
      print
      exit
    }
  ' "$MANAGED_CONFIG"
}

effective_sshd_option() {
  local sshd_bin="$1"
  local key="$2"

  "$sshd_bin" -T -f "$SSHD_CONFIG" 2>/dev/null | awk -v key="$key" '
    $1 == key {
      $1 = ""
      sub(/^[[:space:]]+/, "")
      print
      exit
    }
  '
}

verify_effective_config() {
  local sshd_bin="$1"
  local rotate_key="$2"
  local modify_port="$3"
  local ssh_port="$4"
  local disable_root_password="$5"
  local disable_root_key="$6"
  local disable_password_auth="$7"
  local restrict_users="$8"
  local allow_users="$9"
  local effective
  local expected
  local user

  if [ "$rotate_key" = "y" ]; then
    effective="$(effective_sshd_option "$sshd_bin" "pubkeyauthentication")"
    if [ "$effective" != "yes" ]; then
      warn "PubkeyAuthentication 未按预期生效，当前有效值：${effective:-unknown}"
      return 1
    fi
  fi

  if [ "$modify_port" = "y" ]; then
    if ! "$sshd_bin" -T -f "$SSHD_CONFIG" 2>/dev/null | awk -v port="$ssh_port" '$1 == "port" && $2 == port { found = 1 } END { exit found ? 0 : 1 }'; then
      warn "新 SSH 端口 $ssh_port 未出现在 sshd 最终有效配置中。"
      return 1
    fi
  fi

  if [ "$disable_root_key" = "y" ]; then
    effective="$(effective_sshd_option "$sshd_bin" "permitrootlogin")"
    if [ "$effective" != "no" ]; then
      warn "PermitRootLogin 未按预期生效，当前有效值：${effective:-unknown}"
      return 1
    fi
  elif [ "$disable_root_password" = "y" ]; then
    effective="$(effective_sshd_option "$sshd_bin" "permitrootlogin")"
    if [ "$effective" != "prohibit-password" ]; then
      warn "PermitRootLogin 未按预期生效，当前有效值：${effective:-unknown}"
      return 1
    fi
  fi

  if [ "$disable_password_auth" = "y" ]; then
    effective="$(effective_sshd_option "$sshd_bin" "passwordauthentication")"
    if [ "$effective" != "no" ]; then
      warn "PasswordAuthentication 未按预期生效，当前有效值：${effective:-unknown}"
      return 1
    fi

    if [ "$SUPPORTS_KBD_INTERACTIVE" = "y" ]; then
      effective="$(effective_sshd_option "$sshd_bin" "kbdinteractiveauthentication")"
      if [ "$effective" != "no" ]; then
        warn "KbdInteractiveAuthentication 未按预期生效，当前有效值：${effective:-unknown}"
        return 1
      fi
    fi

    if [ "$SUPPORTS_CHALLENGE_RESPONSE" = "y" ]; then
      effective="$(effective_sshd_option "$sshd_bin" "challengeresponseauthentication")"
      if [ "$effective" != "no" ]; then
        warn "ChallengeResponseAuthentication 未按预期生效，当前有效值：${effective:-unknown}"
        return 1
      fi
    fi
  fi

  if [ "$restrict_users" = "y" ]; then
    effective="$(effective_sshd_option "$sshd_bin" "allowusers")"
    expected="$(normalize_user_list "$allow_users")"
    effective="$(normalize_user_list "$effective")"
    if [ -z "$effective" ]; then
      warn "AllowUsers 未按预期生效，当前未检测到有效值。"
      return 1
    fi

    if [ "$effective" != "$expected" ]; then
      warn "AllowUsers 与预期不一致，预期：$expected，当前有效值：$effective"
      return 1
    fi

    for user in $allow_users; do
      if [[ " $effective " != *" $user "* ]]; then
        warn "AllowUsers 未包含预期用户 $user，当前有效值：$effective"
        return 1
      fi
    done
  fi
}

validate_user_list() {
  local users="$1"
  local user

  [ -n "$users" ] || return 1
  for user in $users; do
    validate_username "$user" || return 1
    id "$user" >/dev/null 2>&1 || return 1
  done
}

normalize_user_list() {
  local users="$1"
  local user
  local normalized=""

  for user in $users; do
    normalized="${normalized:+$normalized }$user"
  done
  printf '%s' "$normalized"
}

validate_path_owner_and_mode() {
  local path="$1"
  local expected_uid="$2"
  local label="$3"
  local owner_uid
  local mode

  owner_uid="$(stat -c '%u' "$path")" || return 1
  mode="$(stat -c '%a' "$path")" || return 1

  if [ "$owner_uid" != "$expected_uid" ] && [ "$owner_uid" != "0" ]; then
    warn "$label 的所有者不是目标用户或 root：$path"
    return 1
  fi

  if (( (8#$mode & 022) != 0 )); then
    warn "$label 存在 group/world 可写权限：$path"
    return 1
  fi
}

verify_authorized_keys_file_support() {
  local sshd_bin="$1"
  local configured
  local item

  configured="$(effective_sshd_option "$sshd_bin" "authorizedkeysfile")"
  [ -n "$configured" ] || configured=".ssh/authorized_keys"

  for item in $configured; do
    case "$item" in
      .ssh/authorized_keys|%h/.ssh/authorized_keys)
        return 0
        ;;
    esac
  done

  warn "当前 sshd AuthorizedKeysFile 为：$configured"
  warn "脚本只会安全替换默认路径 .ssh/authorized_keys。"
  return 1
}

port_is_listening() {
  local port="$1"

  command -v ss >/dev/null 2>&1 || return 1
  ss -H -ltn 2>/dev/null | awk -v port="$port" '
    {
      split($4, addr, ":")
      if (addr[length(addr)] == port) {
        found = 1
      }
    }
    END { exit found ? 0 : 1 }
  '
}

ensure_port_available() {
  local port="$1"

  if ! command -v ss >/dev/null 2>&1; then
    warn "未找到 ss，无法自动检查端口占用。"
    prompt_yes_no "是否确认继续使用端口 $port？" "n"
    return
  fi

  if port_is_listening "$port"; then
    warn "端口 $port 当前已有进程监听。"
    return 1
  fi
}

write_managed_config() {
  local rotate_key="$1"
  local modify_port="$2"
  local ssh_port="$3"
  local disable_root_password="$4"
  local disable_root_key="$5"
  local disable_password_auth="$6"
  local restrict_users="$7"
  local allow_users="$8"
  local enable_hardening="$9"
  local existing_pubkey_auth
  local existing_port
  local existing_root_login
  local existing_password_auth
  local existing_kbd_auth
  local existing_challenge_auth
  local existing_allow_users
  local existing_permit_empty_passwords
  local existing_x11_forwarding
  local existing_max_auth_tries
  local existing_login_grace_time
  local existing_use_dns
  local final_pubkey_auth=""
  local final_port=""
  local final_root_login=""
  local final_password_auth=""
  local final_kbd_auth=""
  local final_challenge_auth=""
  local final_allow_users=""
  local final_permit_empty_passwords=""
  local final_x11_forwarding=""
  local final_max_auth_tries=""
  local final_login_grace_time=""
  local final_use_dns=""

  existing_pubkey_auth="$(managed_option_value "PubkeyAuthentication")"
  existing_port="$(managed_option_value "Port")"
  existing_root_login="$(managed_option_value "PermitRootLogin")"
  existing_password_auth="$(managed_option_value "PasswordAuthentication")"
  existing_kbd_auth="$(managed_option_value "KbdInteractiveAuthentication")"
  existing_challenge_auth="$(managed_option_value "ChallengeResponseAuthentication")"
  existing_allow_users="$(managed_option_value "AllowUsers")"
  existing_permit_empty_passwords="$(managed_option_value "PermitEmptyPasswords")"
  existing_x11_forwarding="$(managed_option_value "X11Forwarding")"
  existing_max_auth_tries="$(managed_option_value "MaxAuthTries")"
  existing_login_grace_time="$(managed_option_value "LoginGraceTime")"
  existing_use_dns="$(managed_option_value "UseDNS")"

  final_pubkey_auth="$existing_pubkey_auth"
  final_port="$existing_port"
  final_root_login="$existing_root_login"
  final_password_auth="$existing_password_auth"
  final_kbd_auth="$existing_kbd_auth"
  final_challenge_auth="$existing_challenge_auth"
  final_allow_users="$existing_allow_users"
  final_permit_empty_passwords="$existing_permit_empty_passwords"
  final_x11_forwarding="$existing_x11_forwarding"
  final_max_auth_tries="$existing_max_auth_tries"
  final_login_grace_time="$existing_login_grace_time"
  final_use_dns="$existing_use_dns"

  if [ "$rotate_key" = "y" ]; then
    final_pubkey_auth="yes"
  fi

  if [ "$modify_port" = "y" ]; then
    final_port="$ssh_port"
  fi

  if [ "$disable_root_key" = "y" ]; then
    final_root_login="no"
  elif [ "$disable_root_password" = "y" ]; then
    final_root_login="prohibit-password"
  fi

  if [ "$disable_password_auth" = "y" ]; then
    final_password_auth="no"
    if [ "$SUPPORTS_KBD_INTERACTIVE" = "y" ]; then
      final_kbd_auth="no"
    fi
    if [ "$SUPPORTS_CHALLENGE_RESPONSE" = "y" ]; then
      final_challenge_auth="no"
    fi
  fi

  if [ "$restrict_users" = "y" ]; then
    final_allow_users="$allow_users"
  fi

  if [ "$enable_hardening" = "y" ]; then
    final_permit_empty_passwords="no"
    final_x11_forwarding="no"
    final_max_auth_tries="3"
    final_login_grace_time="30"
    final_use_dns="no"
  fi

  {
    printf '# Managed by setup-ssh.sh. Edit with care.\n'

    if [ -n "$final_pubkey_auth" ]; then
      printf 'PubkeyAuthentication %s\n' "$final_pubkey_auth"
    fi

    if [ -n "$final_port" ]; then
      printf 'Port %s\n' "$final_port"
    fi

    if [ -n "$final_root_login" ]; then
      printf 'PermitRootLogin %s\n' "$final_root_login"
    fi

    if [ -n "$final_password_auth" ]; then
      printf 'PasswordAuthentication %s\n' "$final_password_auth"
    fi

    if [ -n "$final_kbd_auth" ]; then
      printf 'KbdInteractiveAuthentication %s\n' "$final_kbd_auth"
    fi

    if [ -n "$final_challenge_auth" ]; then
      printf 'ChallengeResponseAuthentication %s\n' "$final_challenge_auth"
    fi

    if [ -n "$final_allow_users" ]; then
      printf 'AllowUsers %s\n' "$final_allow_users"
    fi

    if [ -n "$final_permit_empty_passwords" ]; then
      printf 'PermitEmptyPasswords %s\n' "$final_permit_empty_passwords"
    fi

    if [ -n "$final_x11_forwarding" ]; then
      printf 'X11Forwarding %s\n' "$final_x11_forwarding"
    fi

    if [ -n "$final_max_auth_tries" ]; then
      printf 'MaxAuthTries %s\n' "$final_max_auth_tries"
    fi

    if [ -n "$final_login_grace_time" ]; then
      printf 'LoginGraceTime %s\n' "$final_login_grace_time"
    fi

    if [ -n "$final_use_dns" ]; then
      printf 'UseDNS %s\n' "$final_use_dns"
    fi
  } | write_file_atomic "$MANAGED_CONFIG" 644

  log "已写入受控 SSH 配置：$MANAGED_CONFIG"
}

restore_config_files() {
  local main_backup="$1"
  local managed_backup="$2"

  if [ -f "$main_backup" ]; then
    cp -a "$main_backup" "$SSHD_CONFIG"
    warn "已恢复主配置：$SSHD_CONFIG"
  fi

  restore_managed_config "$managed_backup"
}

restore_managed_config() {
  local backup_file="$1"

  if [ -n "$backup_file" ] && [ -f "$backup_file" ]; then
    cp -a "$backup_file" "$MANAGED_CONFIG"
    warn "已恢复原受控配置：$MANAGED_CONFIG"
  else
    rm -f "$MANAGED_CONFIG"
    warn "已移除新生成的受控配置：$MANAGED_CONFIG"
  fi
}

replace_authorized_keys() {
  local target_user="$1"
  local public_key="$2"
  local sshd_bin="$3"
  local home_dir
  local ssh_dir
  local auth_file
  local target_uid

  LAST_BACKUP_FILE=""
  LAST_BACKUP_HAD_FILE=""

  verify_authorized_keys_file_support "$sshd_bin" || return 1

  home_dir="$(getent passwd "$target_user" | cut -d: -f6)" || return 1
  [ -n "$home_dir" ] || {
    warn "无法获取用户 $target_user 的 home 目录。"
    return 1
  }
  target_uid="$(id -u "$target_user")" || return 1

  if [ -L "$home_dir" ] || [ ! -d "$home_dir" ]; then
    warn "$target_user 的 home 目录不是普通目录，或是符号链接：$home_dir"
    return 1
  fi
  validate_path_owner_and_mode "$home_dir" "$target_uid" "home 目录" || return 1

  ssh_dir="$home_dir/.ssh"
  auth_file="$ssh_dir/authorized_keys"

  if [ -L "$ssh_dir" ]; then
    warn "$ssh_dir 是符号链接，拒绝以 root 身份写入。"
    return 1
  fi
  if [ -e "$ssh_dir" ] && [ ! -d "$ssh_dir" ]; then
    warn "$ssh_dir 已存在但不是目录。"
    return 1
  fi
  if [ -L "$auth_file" ]; then
    warn "$auth_file 是符号链接，拒绝写入。"
    return 1
  fi
  if [ -e "$auth_file" ] && [ ! -f "$auth_file" ]; then
    warn "$auth_file 已存在但不是普通文件。"
    return 1
  fi

  mkdir -p "$ssh_dir" || return 1
  chown "$target_user:" "$ssh_dir" || return 1
  chmod 700 "$ssh_dir" || return 1
  validate_path_owner_and_mode "$ssh_dir" "$target_uid" ".ssh 目录" || return 1

  backup_file_if_exists "$auth_file" "authorized_keys.${target_user}" || return 1

  printf '%s\n' "$public_key" | write_file_atomic "$auth_file" 600 || return 1

  chown "$target_user:" "$ssh_dir" "$auth_file" || return 1
  log "已替换 $auth_file：仅保留本次输入的新公钥。"
}

restore_authorized_keys() {
  local target_user="$1"
  local backup_file="$2"
  local had_file="$3"
  local home_dir
  local ssh_dir
  local auth_file

  [ -n "$target_user" ] || return 0

  home_dir="$(getent passwd "$target_user" | cut -d: -f6)"
  [ -n "$home_dir" ] || return 0

  ssh_dir="$home_dir/.ssh"
  auth_file="$ssh_dir/authorized_keys"

  if [ "$had_file" = "y" ] && [ -n "$backup_file" ] && [ -f "$backup_file" ]; then
    cp -a "$backup_file" "$auth_file"
    chown "$target_user:" "$ssh_dir" "$auth_file"
    chmod 700 "$ssh_dir"
    chmod 600 "$auth_file"
    warn "已恢复 $auth_file"
  elif [ "$had_file" = "n" ]; then
    rm -f "$auth_file"
    warn "已移除新生成的 $auth_file"
  else
    warn "未找到本次 authorized_keys 备份，无法自动恢复：$auth_file"
  fi
}

ufw_is_active() {
  command -v ufw >/dev/null 2>&1 || return 1
  ufw status 2>/dev/null | awk 'BEGIN { active = 1 } /^[[:space:]]*Status:[[:space:]]*active/ { active = 0 } END { exit active }'
}

configure_firewall_port() {
  local port="$1"
  local changed="n"

  if ufw_is_active; then
    ufw allow "${port}/tcp" || return 1
    log "UFW 已启用，已放行 TCP ${port}。"
    changed="y"
  elif command -v ufw >/dev/null 2>&1; then
    warn "UFW 未启用，按要求不修改 UFW 状态。"
  fi

  if command -v firewall-cmd >/dev/null 2>&1; then
    if systemctl is-active --quiet firewalld; then
      firewall-cmd --permanent --add-port="${port}/tcp" || return 1
      firewall-cmd --reload || return 1
      log "firewalld 已启用，已放行 TCP ${port}。"
      changed="y"
    else
      warn "firewalld 未启用，按要求不修改 firewalld 状态。"
    fi
  fi

  if [ "$changed" = "n" ] && ! command -v ufw >/dev/null 2>&1 && ! command -v firewall-cmd >/dev/null 2>&1; then
    warn "未检测到 UFW 或 firewalld，跳过防火墙配置。"
  fi
}

configure_firewall() {
  local ports_csv="$1"
  local old_ifs="$IFS"
  local port
  local seen_ports=""

  IFS=','
  for port in $ports_csv; do
    IFS="$old_ifs"
    if validate_port "$port" && [[ ",$seen_ports," != *",$port,"* ]]; then
      configure_firewall_port "$port" || return 1
      seen_ports="${seen_ports:+$seen_ports,}$port"
    fi
    IFS=','
  done
  IFS="$old_ifs"
}

test_sshd_config() {
  local sshd_bin="$1"

  ensure_run_dir
  if "$sshd_bin" -t -f "$SSHD_CONFIG"; then
    log "sshd 配置语法检查通过。"
  else
    return 1
  fi
}

reload_ssh_service() {
  local service="$1"

  systemctl reload "$service" || return 1
  log "已重载 ${service} 服务。"
}

restore_config_and_reload() {
  local sshd_bin="$1"
  local service="$2"
  local main_backup="$3"
  local managed_backup="$4"

  restore_config_files "$main_backup" "$managed_backup"
  if test_sshd_config "$sshd_bin" && reload_ssh_service "$service"; then
    warn "已重新加载恢复后的 SSH 配置。"
    return 0
  fi

  warn "已恢复配置文件，但重新加载恢复配置失败；请保持当前会话并人工检查 SSH 服务。"
  return 1
}

main() {
  local ssh_service
  local sshd_bin
  local target_user=""
  local public_key=""
  local rotate_key="n"
  local modify_port="n"
  local ssh_port=""
  local existing_ports
  local firewall_ports
  local disable_root_password="n"
  local disable_root_key="n"
  local disable_password_auth="n"
  local restrict_users="n"
  local allow_users=""
  local enable_hardening="n"
  local managed_backup=""
  local main_backup=""
  local auth_backup=""
  local auth_had_file="n"

  need_root
  show_intro
  require_ubuntu

  [ -f "$SSHD_CONFIG" ] || die "未找到 $SSHD_CONFIG"
  sshd_bin="$(find_sshd_binary)"
  ensure_run_dir
  detect_sshd_keyword_support "$sshd_bin"
  ssh_service="$(detect_ssh_service)"
  existing_ports="$(current_ports "$sshd_bin")"

  printf '\n当前检测到的 SSH 端口：%s\n' "$existing_ports"
  printf '当前 SSH 服务名：%s\n\n' "$ssh_service"

  if prompt_yes_no "是否添加新公钥并清理该用户的其他公钥？选择 n 将完全跳过密钥文件修改" "y"; then
    rotate_key="y"
    target_user="$(prompt "请输入要替换 authorized_keys 的 Linux 用户" "root")"
    validate_username "$target_user" || die "用户名格式不合法：$target_user"
    id "$target_user" >/dev/null 2>&1 || die "用户不存在：$target_user"

    printf '\n请粘贴确认无误的新 SSH 公钥。完成后，该用户旧 authorized_keys 会被备份，并替换为仅包含此公钥。\n'
    printf '建议使用 ssh-ed25519 公钥；脚本默认拒绝旧式 ssh-rsa 公钥。\n'
    while true; do
      public_key="$(prompt "新 SSH 公钥")"
      if validate_public_key "$public_key"; then
        break
      fi
      warn "公钥格式看起来不正确，请重新输入。"
    done

    warn "即将清理 $target_user 的其他 SSH 公钥，只保留本次输入的新公钥。"
    prompt_yes_no "确认执行密钥替换？" "n" || die "用户取消。"
  else
    log "已选择跳过公钥添加；不会修改或清理任何 authorized_keys。"
  fi

  if prompt_yes_no "是否修改 SSH 端口？默认不修改当前设置" "n"; then
    modify_port="y"
    while true; do
      ssh_port="$(prompt "请输入新的 SSH 端口" "22222")"
      if validate_ssh_port "$ssh_port" && ensure_port_available "$ssh_port"; then
        break
      fi
      warn "端口必须是 1024-65535 的数字，并建议避开已被其他服务占用的端口。"
    done
    firewall_ports="$ssh_port"
  else
    ssh_port="${existing_ports%%,*}"
    firewall_ports="$existing_ports"
  fi

  if prompt_yes_no "是否禁止 root 密码登录？默认不修改当前设置" "n"; then
    disable_root_password="y"
  fi

  if prompt_yes_no "是否禁止 root SSH 登录？这会同时禁止 root 密码和密钥登录，默认不修改当前设置" "n"; then
    disable_root_key="y"
    if [ "$rotate_key" = "y" ] && [ "$target_user" = "root" ]; then
      warn "你选择给 root 添加公钥，同时禁止 root SSH 登录。该公钥将不能用于 root SSH 登录。"
      prompt_yes_no "确认继续？" "n" || die "用户取消。"
    fi
  fi

  if prompt_yes_no "是否禁用全局 SSH 密码认证？建议确认密钥可用后再启用，默认不修改当前设置" "n"; then
    disable_password_auth="y"
  fi

  if prompt_yes_no "是否限制允许 SSH 登录的用户 AllowUsers？默认不修改当前设置" "n"; then
    restrict_users="y"
    while true; do
      allow_users="$(prompt "请输入允许登录的用户列表，空格分隔")"
      if validate_user_list "$allow_users"; then
        if [ "$rotate_key" = "y" ] && [[ " $allow_users " != *" $target_user "* ]]; then
          warn "AllowUsers 必须包含本次替换密钥的用户 $target_user，请重新输入。"
          continue
        fi
        break
      fi
      warn "用户列表不合法，或包含不存在的用户，请重新输入。"
    done
  fi

  if prompt_yes_no "是否写入基础 SSH 加固项？包含空密码禁止、X11 禁用、尝试次数和登录超时限制" "n"; then
    enable_hardening="y"
  fi

  ensure_backup || die "备份 SSH 配置失败，已退出。"
  main_backup="$MAIN_CONFIG_BACKUP"
  managed_backup="$MANAGED_CONFIG_BACKUP"

  if ! ensure_include_enabled; then
    restore_config_and_reload "$sshd_bin" "$ssh_service" "$main_backup" "$managed_backup" || true
    die "启用 sshd_config.d Include 失败，已尝试恢复 SSH 配置。"
  fi

  if ! write_managed_config "$rotate_key" "$modify_port" "$ssh_port" "$disable_root_password" "$disable_root_key" "$disable_password_auth" "$restrict_users" "$allow_users" "$enable_hardening"; then
    restore_config_and_reload "$sshd_bin" "$ssh_service" "$main_backup" "$managed_backup" || true
    die "写入受控 SSH 配置失败，已尝试恢复 SSH 配置。"
  fi

  if ! test_sshd_config "$sshd_bin"; then
    restore_config_and_reload "$sshd_bin" "$ssh_service" "$main_backup" "$managed_backup" || true
    die "sshd 配置语法检查失败，已恢复 SSH 配置。请检查 $BACKUP_DIR 中的备份。"
  fi

  if ! verify_effective_config "$sshd_bin" "$rotate_key" "$modify_port" "$ssh_port" "$disable_root_password" "$disable_root_key" "$disable_password_auth" "$restrict_users" "$allow_users"; then
    restore_config_and_reload "$sshd_bin" "$ssh_service" "$main_backup" "$managed_backup" || true
    die "sshd 最终有效配置未通过校验，已恢复 SSH 配置。请检查 Include 顺序和已有全局配置。"
  fi

  if ! configure_firewall "$firewall_ports"; then
    restore_config_and_reload "$sshd_bin" "$ssh_service" "$main_backup" "$managed_backup" || true
    die "防火墙配置失败，已恢复 SSH 配置。请检查防火墙状态。"
  fi

  if [ "$rotate_key" = "y" ]; then
    if ! replace_authorized_keys "$target_user" "$public_key" "$sshd_bin"; then
      auth_backup="$LAST_BACKUP_FILE"
      auth_had_file="$LAST_BACKUP_HAD_FILE"
      restore_config_and_reload "$sshd_bin" "$ssh_service" "$main_backup" "$managed_backup" || true
      [ -n "$auth_had_file" ] && restore_authorized_keys "$target_user" "$auth_backup" "$auth_had_file"
      die "替换 authorized_keys 失败，已尝试恢复 SSH 配置和密钥文件。"
    fi
    auth_backup="$LAST_BACKUP_FILE"
    auth_had_file="$LAST_BACKUP_HAD_FILE"
  fi

  if ! reload_ssh_service "$ssh_service"; then
    restore_config_and_reload "$sshd_bin" "$ssh_service" "$main_backup" "$managed_backup" || true
    if [ "$rotate_key" = "y" ]; then
      restore_authorized_keys "$target_user" "$auth_backup" "$auth_had_file"
    fi
    die "SSH 服务重载失败，已恢复 SSH 配置。请检查服务状态。"
  fi

  if [ "$modify_port" = "y" ] && command -v ss >/dev/null 2>&1 && ! port_is_listening "$ssh_port"; then
    restore_config_and_reload "$sshd_bin" "$ssh_service" "$main_backup" "$managed_backup" || true
    if [ "$rotate_key" = "y" ]; then
      restore_authorized_keys "$target_user" "$auth_backup" "$auth_had_file"
    fi
    die "未检测到 SSH 新端口 $ssh_port 正在监听，已恢复 SSH 配置。"
  fi

  printf '\n完成。\n'
  printf '请新开一个终端验证 SSH 登录，确认无误前不要关闭当前会话。\n'
  if [ "$rotate_key" = "y" ]; then
    printf '测试命令示例：ssh -p %s %s@<VPS_IP>\n' "$ssh_port" "$target_user"
  else
    printf '未修改密钥文件；请使用你现有可用用户测试：ssh -p %s <USER>@<VPS_IP>\n' "$ssh_port"
  fi
  if [ "$modify_port" = "y" ]; then
    printf '如果云厂商安全组存在，请确认已放行 TCP %s。\n' "$ssh_port"
    printf '确认新端口登录成功后，再手动清理旧端口的防火墙规则。旧端口：%s\n' "$existing_ports"
  fi
}

main "$@"
