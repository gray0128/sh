#!/usr/bin/env bash
#
# singbox_deploy_hardened_v15.sh
#
# Practical hardened sing-box deployment script for personal use.
# Target: Linux servers with systemd.
#
# Design choices:
# - No remote script execution.
# - Pinned sing-box version.
# - SHA256 verification supported; explicit confirmation required if omitted.
# - Simple interactive menu.
# - Minimal but useful safeguards: config backup, sing-box check, rollback.
# - Supported nodes: VLESS Reality, Trojan TLS, Hysteria2 TLS, TUIC TLS, Shadowsocks.
#
# This script is intentionally not a full control panel.
#
# v15 practical fixes:
# - Keeps all v14 sing-box, IPv6, mieru/mita, proxy cleanup, and optional nginx/caddy cleanup behavior.
# - check_port_free now also checks mieru-managed nodes when present.
# - Adds explicit uninstall for mita / mieru managed state.
# - No broad feature expansion; this is a targeted reliability and cleanup update.

set -Eeuo pipefail
IFS=$'\n\t'
umask 077

SCRIPT_NAME="$(basename "$0")"
SINGBOX_VERSION="${SINGBOX_VERSION:-1.13.11}"
EXPECTED_SHA256="${EXPECTED_SHA256:-}"

# Independent mieru/mita support. This is not a sing-box inbound; mita is
# installed and managed as a separate service.
MIERU_VERSION="${MIERU_VERSION:-3.32.0}"
MIERU_EXPECTED_SHA256="${MIERU_EXPECTED_SHA256:-}"

ALLOW_UNVERIFIED_DOWNLOAD=0
NONINTERACTIVE=0

INSTALL_ROOT="/opt/sing-box"
BIN_PATH="${INSTALL_ROOT}/sing-box"
BIN_LINK="/usr/local/bin/sing-box"

CONFIG_DIR="/etc/sing-box"
CONFIG_FILE="${CONFIG_DIR}/config.json"
NODES_FILE="${CONFIG_DIR}/nodes.json"
LINKS_FILE="${CONFIG_DIR}/links.txt"
CERT_DIR="${CONFIG_DIR}/certs"
BACKUP_DIR="${CONFIG_DIR}/backups"
LOG_DIR="/var/log/sing-box"

SERVICE_FILE="/etc/systemd/system/sing-box.service"
SINGBOX_USER="singbox"
SINGBOX_GROUP="singbox"

MIERU_MANAGED_DIR="/etc/mieru-managed"
MIERU_NODES_FILE="${MIERU_MANAGED_DIR}/nodes.json"
MIERU_LINKS_FILE="${MIERU_MANAGED_DIR}/links.txt"
MIERU_CONFIG_FILE="${MIERU_MANAGED_DIR}/mita_config.json"

tmpdir=""
backup_file=""

die() { printf '[!] %s\n' "$*" >&2; exit 1; }
warn() { printf '[!] %s\n' "$*" >&2; }
info() { printf '[*] %s\n' "$*" >&2; }

cleanup() {
  [[ -n "${tmpdir}" && -d "${tmpdir}" ]] && rm -rf "${tmpdir}"
}
trap cleanup EXIT
trap 'die "error at line ${LINENO}"' ERR

usage() {
  cat <<EOF
Usage:
  sudo bash ${SCRIPT_NAME} [options]

Options:
  --version <ver>                 sing-box version, default: ${SINGBOX_VERSION}
  --sha256 <hash>                 expected SHA256 of sing-box release tarball
  --mieru-version <ver>           mieru/mita version, default: ${MIERU_VERSION}
  --mieru-sha256 <hash>           expected SHA256 of mita deb/rpm package
  --allow-unverified-download     allow install without SHA256 verification for sing-box or mita
  --noninteractive                fail instead of prompting for risky actions
  -h, --help                      show help

Example:
  sudo bash ${SCRIPT_NAME} --sha256 <release-tarball-sha256>

Supported target:
  Debian/Ubuntu/RHEL-family systemd VPS are the primary target.
  Alpine/OpenRC is not a primary target even though dependency installation has a best-effort apk branch.

IPv6:
  IPv4 remains the default listen mode.
  IPv6 literal public hosts are supported and URI links will use [IPv6] format.
  For domain public hosts, the menu can enable IPv6/dual-stack listen with ::.
EOF
}

validate_version() {
  [[ "${SINGBOX_VERSION}" =~ ^[0-9]+(\.[0-9]+){2}(-[A-Za-z0-9._-]+)?$ ]] || die "invalid SINGBOX_VERSION: ${SINGBOX_VERSION}"
  [[ "${MIERU_VERSION}" =~ ^[0-9]+(\.[0-9]+){2}(-[A-Za-z0-9._-]+)?$ ]] || die "invalid MIERU_VERSION: ${MIERU_VERSION}"
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --version)
        [[ $# -ge 2 ]] || die "--version requires a value"
        SINGBOX_VERSION="$2"
        shift 2
        ;;
      --sha256)
        [[ $# -ge 2 ]] || die "--sha256 requires a value"
        EXPECTED_SHA256="$2"
        shift 2
        ;;
      --mieru-version)
        [[ $# -ge 2 ]] || die "--mieru-version requires a value"
        MIERU_VERSION="$2"
        shift 2
        ;;
      --mieru-sha256)
        [[ $# -ge 2 ]] || die "--mieru-sha256 requires a value"
        MIERU_EXPECTED_SHA256="$2"
        shift 2
        ;;
      --allow-unverified-download)
        ALLOW_UNVERIFIED_DOWNLOAD=1
        shift
        ;;
      --noninteractive)
        NONINTERACTIVE=1
        shift
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        die "unknown option: $1"
        ;;
    esac
  done
  validate_version
}

confirm() {
  local prompt="${1:-Continue?}" ans
  [[ "${NONINTERACTIVE}" -eq 1 ]] && return 1
  read -r -p "${prompt} [y/N]: " ans
  [[ "${ans}" == "y" || "${ans}" == "Y" || "${ans}" == "yes" || "${ans}" == "YES" ]]
}

require_root() {
  [[ "$(id -u)" -eq 0 ]] || die "run as root"
}

require_systemd() {
  command -v systemctl >/dev/null 2>&1 || die "systemd/systemctl is required"
}

require_installed() {
  [[ -x "${BIN_PATH}" || -x "${BIN_LINK}" ]] || die "sing-box is not installed; choose install first"
  [[ -f "${CONFIG_FILE}" ]] || die "config file not found; choose install first"
  getent passwd "${SINGBOX_USER}" >/dev/null || die "system user ${SINGBOX_USER} not found; choose install first"
}

detect_arch() {
  case "$(uname -m)" in
    x86_64|amd64) echo "amd64" ;;
    aarch64|arm64) echo "arm64" ;;
    armv7l|armv7) echo "armv7" ;;
    i386|i686) echo "386" ;;
    *) die "unsupported architecture: $(uname -m)" ;;
  esac
}

install_deps() {
  info "Installing dependencies"
  if command -v apt-get >/dev/null 2>&1; then
    DEBIAN_FRONTEND=noninteractive apt-get update -y
    DEBIAN_FRONTEND=noninteractive apt-get install -y ca-certificates curl jq openssl tar coreutils util-linux uuid-runtime iproute2
  elif command -v dnf >/dev/null 2>&1; then
    dnf install -y ca-certificates curl jq openssl tar coreutils util-linux iproute
  elif command -v yum >/dev/null 2>&1; then
    yum install -y ca-certificates curl jq openssl tar coreutils util-linux iproute
  elif command -v apk >/dev/null 2>&1; then
    apk add --no-cache ca-certificates curl jq openssl tar coreutils util-linux iproute2
    update-ca-certificates || true
  elif command -v pacman >/dev/null 2>&1; then
    pacman -Sy --noconfirm ca-certificates curl jq openssl tar coreutils util-linux iproute2
  elif command -v zypper >/dev/null 2>&1; then
    zypper --non-interactive install ca-certificates curl jq openssl tar coreutils util-linux iproute2 || \
      zypper --non-interactive install ca-certificates curl jq openssl tar coreutils util-linux
  else
    die "unsupported package manager; install ca-certificates curl jq openssl tar coreutils util-linux manually"
  fi
}

create_user_and_dirs() {
  if ! getent group "${SINGBOX_GROUP}" >/dev/null; then
    groupadd --system "${SINGBOX_GROUP}"
  fi

  if ! getent passwd "${SINGBOX_USER}" >/dev/null; then
    useradd --system --gid "${SINGBOX_GROUP}" --no-create-home --shell /usr/sbin/nologin "${SINGBOX_USER}" 2>/dev/null \
      || useradd --system --gid "${SINGBOX_GROUP}" --no-create-home --shell /sbin/nologin "${SINGBOX_USER}"
  fi

  mkdir -p "${INSTALL_ROOT}" "${CONFIG_DIR}" "${CERT_DIR}" "${BACKUP_DIR}" "${LOG_DIR}"

  # sing-box runs as User=singbox Group=singbox. It must be able to traverse
  # CONFIG_DIR and CERT_DIR to read config.json and certificate/key files.
  chown root:root "${INSTALL_ROOT}"
  chown root:"${SINGBOX_GROUP}" "${CONFIG_DIR}" "${CERT_DIR}"
  chown root:root "${BACKUP_DIR}"
  chown "${SINGBOX_USER}:${SINGBOX_GROUP}" "${LOG_DIR}"

  chmod 755 "${INSTALL_ROOT}"
  chmod 750 "${CONFIG_DIR}" "${CERT_DIR}"
  chmod 700 "${BACKUP_DIR}"
  chmod 750 "${LOG_DIR}"
}

assert_safe_tar() {
  local tarball="$1" member mode
  while IFS= read -r member; do
    case "${member}" in
      ""|/*|*"/../"*|*"../"*|*".."|*"/..")
        die "unsafe tar member path: ${member}"
        ;;
    esac
  done < <(tar -tf "${tarball}")

  # Reject symlink/hardlink members. The official release tarball should not need them.
  while IFS= read -r line; do
    mode="${line%% *}"
    case "${mode}" in
      l*|h*) die "unsafe tar member type: ${line}" ;;
    esac
  done < <(tar -tvf "${tarball}")
}

download_singbox() {
  local arch file url tarball actual extracted_dir

  arch="$(detect_arch)"
  file="sing-box-${SINGBOX_VERSION}-linux-${arch}.tar.gz"
  url="https://github.com/SagerNet/sing-box/releases/download/v${SINGBOX_VERSION}/${file}"

  if [[ -z "${EXPECTED_SHA256}" && "${ALLOW_UNVERIFIED_DOWNLOAD}" -ne 1 ]]; then
    warn "EXPECTED_SHA256 is not set. This weakens supply-chain protection."
    confirm "Proceed without SHA256 verification" || die "aborted; set --sha256 or --allow-unverified-download"
  fi

  tmpdir="$(mktemp -d)"
  tarball="${tmpdir}/${file}"

  info "Downloading ${url}"
  curl --fail --location --proto '=https' --tlsv1.2 --retry 3 --output "${tarball}" "${url}"

  if [[ -n "${EXPECTED_SHA256}" ]]; then
    actual="$(sha256sum "${tarball}" | awk '{print $1}')"
    [[ "${actual}" == "${EXPECTED_SHA256}" ]] || die "SHA256 mismatch: expected ${EXPECTED_SHA256}, got ${actual}"
    info "SHA256 verified"
  else
    warn "Skipped SHA256 verification by explicit opt-in"
  fi

  assert_safe_tar "${tarball}"
  tar --no-same-owner --no-same-permissions -xf "${tarball}" -C "${tmpdir}"

  extracted_dir="${tmpdir}/sing-box-${SINGBOX_VERSION}-linux-${arch}"
  [[ -f "${extracted_dir}/sing-box" && ! -L "${extracted_dir}/sing-box" && -x "${extracted_dir}/sing-box" ]] \
    || die "sing-box binary is not a regular executable file"

  if [[ -x "${BIN_PATH}" ]]; then
    cp -a "${BIN_PATH}" "${BIN_PATH}.bak.$(date +%Y%m%d%H%M%S)" || true
  fi

  install -m 0755 "${extracted_dir}/sing-box" "${BIN_PATH}"
  ln -sf "${BIN_PATH}" "${BIN_LINK}"
  info "Installed ${BIN_PATH}"
}

init_config() {
  mkdir -p "${CONFIG_DIR}" "${BACKUP_DIR}" "${CERT_DIR}" "${LOG_DIR}"

  chown root:"${SINGBOX_GROUP}" "${CONFIG_DIR}" "${CERT_DIR}"
  chmod 750 "${CONFIG_DIR}" "${CERT_DIR}"
  chown root:root "${BACKUP_DIR}"
  chmod 700 "${BACKUP_DIR}"
  chown "${SINGBOX_USER}:${SINGBOX_GROUP}" "${LOG_DIR}"
  chmod 750 "${LOG_DIR}"

  if [[ ! -f "${CONFIG_FILE}" ]]; then
    cat > "${CONFIG_FILE}" <<'JSON'
{
  "log": {
    "level": "info",
    "timestamp": true
  },
  "inbounds": [],
  "outbounds": [
    {
      "type": "direct",
      "tag": "direct"
    },
    {
      "type": "block",
      "tag": "block"
    }
  ],
  "route": {
    "rules": [],
    "final": "direct"
  }
}
JSON
  fi

  if [[ ! -f "${NODES_FILE}" ]]; then
    printf '[]\n' > "${NODES_FILE}"
  fi

  if [[ ! -f "${LINKS_FILE}" ]]; then
    : > "${LINKS_FILE}"
  fi

  chown root:"${SINGBOX_GROUP}" "${CONFIG_FILE}"
  chmod 640 "${CONFIG_FILE}"

  chown root:root "${NODES_FILE}" "${LINKS_FILE}"
  chmod 600 "${NODES_FILE}" "${LINKS_FILE}"

  run_as_singbox test -r "${CONFIG_FILE}" || die "singbox user cannot read ${CONFIG_FILE}; check directory permissions"
}


write_service() {
  cat > "${SERVICE_FILE}" <<EOF
[Unit]
Description=sing-box service
Documentation=https://sing-box.sagernet.org/
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=${SINGBOX_USER}
Group=${SINGBOX_GROUP}
ExecStart=${BIN_PATH} run -c ${CONFIG_FILE}
ExecReload=/bin/kill -HUP \$MAINPID
Restart=on-failure
RestartSec=3s
LimitNOFILE=1048576

CapabilityBoundingSet=CAP_NET_BIND_SERVICE
AmbientCapabilities=CAP_NET_BIND_SERVICE
NoNewPrivileges=true
PrivateTmp=true
PrivateDevices=true
ProtectHome=true
ProtectSystem=strict
ReadOnlyPaths=${CONFIG_DIR}
ReadWritePaths=${LOG_DIR}
RestrictSUIDSGID=true
LockPersonality=true
MemoryDenyWriteExecute=false

[Install]
WantedBy=multi-user.target
EOF

  chmod 644 "${SERVICE_FILE}"
  systemctl daemon-reload
  systemctl enable sing-box.service
  info "systemd service installed"
}

install_all() {
  require_systemd
  install_deps
  create_user_and_dirs
  download_singbox
  init_config
  write_service
  "${BIN_PATH}" version || true
  if [[ -s "${CONFIG_FILE}" ]]; then
    if "${BIN_PATH}" check -c "${CONFIG_FILE}" >/dev/null 2>&1; then
      info "Current config is compatible with the installed sing-box binary."
    else
      warn "Current config did not pass sing-box check with the installed binary."
      warn "Review ${CONFIG_FILE} before restarting the service."
    fi
  fi
  info "Install complete. Add nodes from the menu, then start/restart service."
}

backup_config() {
  mkdir -p "${BACKUP_DIR}"
  chown root:root "${BACKUP_DIR}"
  chmod 700 "${BACKUP_DIR}"
  backup_file="$(mktemp "${BACKUP_DIR}/config.json.$(date +%Y%m%d%H%M%S).XXXXXX.bak")"
  cp -a "${CONFIG_FILE}" "${backup_file}"
  chmod 600 "${backup_file}" || true
  info "Config backup: ${backup_file}"
}

restore_config() {
  if [[ -n "${backup_file}" && -f "${backup_file}" ]]; then
    cp -a "${backup_file}" "${CONFIG_FILE}"
    chown root:"${SINGBOX_GROUP}" "${CONFIG_FILE}"
    chmod 640 "${CONFIG_FILE}"
    warn "Config restored from ${backup_file}"
  fi
}

validate_config_or_rollback() {
  local check_log
  check_log="$(mktemp "${BACKUP_DIR}/check.XXXXXX.log")"

  if "${BIN_PATH}" check -c "${CONFIG_FILE}" >"${check_log}" 2>&1; then
    rm -f "${check_log}"
    info "sing-box config check passed"
    return 0
  fi

  warn "sing-box config check failed:"
  cat "${check_log}" >&2 || true
  rm -f "${check_log}"
  restore_config
  return 1
}

rebuild_links_file() {
  jq -r '.[].link' "${NODES_FILE}" > "${LINKS_FILE}.tmp"
  mv "${LINKS_FILE}.tmp" "${LINKS_FILE}"
  chown root:root "${LINKS_FILE}"
  chmod 600 "${LINKS_FILE}"
}

commit_node_transaction() {
  local inbound_json="$1" type="$2" tag="$3" link="$4" client_json="${5:-null}"
  local nodes_backup new_nodes

  backup_config
  nodes_backup="$(mktemp "${BACKUP_DIR}/nodes.$(date +%Y%m%d%H%M%S).XXXXXX.bak")"
  cp -a "${NODES_FILE}" "${nodes_backup}"

  jq --argjson inbound "${inbound_json}" '.inbounds += [$inbound]' "${CONFIG_FILE}" > "${CONFIG_FILE}.tmp"
  mv "${CONFIG_FILE}.tmp" "${CONFIG_FILE}"
  chown root:"${SINGBOX_GROUP}" "${CONFIG_FILE}"
  chmod 640 "${CONFIG_FILE}"

  if ! validate_config_or_rollback; then
    return 1
  fi

  new_nodes="$(mktemp "${BACKUP_DIR}/nodes.XXXXXX.tmp")"
  if ! jq --arg type "${type}" --arg tag "${tag}" --arg link "${link}" --argjson client "${client_json}" \
    '. += [{"type":$type,"tag":$tag,"link":$link,"client":$client,"created_at":(now|todateiso8601)}]' \
    "${NODES_FILE}" > "${new_nodes}"; then
    warn "Failed to write node metadata; rolling back config."
    cp -a "${nodes_backup}" "${NODES_FILE}"
    restore_config
    return 1
  fi

  mv "${new_nodes}" "${NODES_FILE}"
  chown root:root "${NODES_FILE}"
  chmod 600 "${NODES_FILE}"
  if ! rebuild_links_file; then
    warn "links.txt rebuild failed; nodes.json remains the source of truth."
  fi
}

delete_node_transaction() {
  local index="$1" tag="$2"
  local nodes_backup links_backup

  backup_config
  nodes_backup="$(mktemp "${BACKUP_DIR}/nodes.$(date +%Y%m%d%H%M%S).XXXXXX.bak")"
  links_backup="$(mktemp "${BACKUP_DIR}/links.$(date +%Y%m%d%H%M%S).XXXXXX.bak")"
  cp -a "${NODES_FILE}" "${nodes_backup}"
  cp -a "${LINKS_FILE}" "${links_backup}"

  jq --arg tag "${tag}" '(.inbounds) |= map(select(.tag != $tag))' "${CONFIG_FILE}" > "${CONFIG_FILE}.tmp"
  mv "${CONFIG_FILE}.tmp" "${CONFIG_FILE}"
  chown root:"${SINGBOX_GROUP}" "${CONFIG_FILE}"
  chmod 640 "${CONFIG_FILE}"

  if ! validate_config_or_rollback; then
    cp -a "${nodes_backup}" "${NODES_FILE}"
    cp -a "${links_backup}" "${LINKS_FILE}"
    return 1
  fi

  if ! jq --argjson i "${index}" 'del(.[$i])' "${NODES_FILE}" > "${NODES_FILE}.tmp"; then
    warn "Failed to update node metadata; rolling back."
    restore_config
    cp -a "${nodes_backup}" "${NODES_FILE}"
    cp -a "${links_backup}" "${LINKS_FILE}"
    return 1
  fi

  mv "${NODES_FILE}.tmp" "${NODES_FILE}"
  chown root:root "${NODES_FILE}"
  chmod 600 "${NODES_FILE}"
  if ! rebuild_links_file; then
    warn "links.txt rebuild failed; nodes.json remains the source of truth."
  fi
}

reload_or_restart() {
  if systemctl is-active --quiet sing-box.service; then
    systemctl restart sing-box.service
    systemctl status sing-box.service --no-pager -l || true
  fi
}

random_hex() {
  openssl rand -hex "${1:-16}"
}

random_b64url() {
  openssl rand -base64 "${1:-24}" | tr '+/' '-_' | tr -d '=\n'
}

new_uuid() {
  if command -v uuidgen >/dev/null 2>&1; then
    uuidgen | tr '[:upper:]' '[:lower:]'
  elif [[ -r /proc/sys/kernel/random/uuid ]]; then
    tr '[:upper:]' '[:lower:]' < /proc/sys/kernel/random/uuid
  else
    python3 - <<'PY'
import uuid
print(uuid.uuid4())
PY
  fi
}

urlencode() {
  jq -rn --arg v "$1" '$v|@uri'
}

safe_basename() {
  local raw="$1" safe
  safe="$(printf '%s' "${raw}" | sed 's/[^A-Za-z0-9_.-]/_/g')"
  safe="${safe#.}"
  safe="${safe:-cert}"
  printf '%s\n' "${safe}"
}

is_ipv4() {
  local ip="$1" IFS=. a b c d
  [[ "${ip}" =~ ^([0-9]{1,3}\.){3}[0-9]{1,3}$ ]] || return 1
  read -r a b c d <<< "${ip}"
  for octet in "${a}" "${b}" "${c}" "${d}"; do
    [[ "${octet}" =~ ^[0-9]+$ ]] || return 1
    (( octet >= 0 && octet <= 255 )) || return 1
  done
}

is_ipv6() {
  local v="$1"
  [[ -n "${v}" && "${v}" == *:* ]] || return 1
  [[ "${v}" != *"://"* && "${v}" != *"/"* && "${v}" != *" "* && "${v}" != *"["* && "${v}" != *"]"* ]] || return 1

  if command -v python3 >/dev/null 2>&1; then
    python3 - "$v" <<'PY' >/dev/null 2>&1
import ipaddress, sys
try:
    ipaddress.IPv6Address(sys.argv[1])
except Exception:
    sys.exit(1)
PY
    return $?
  fi

  # Conservative fallback when python3 is unavailable. This accepts normal IPv6
  # characters but does not fully validate every IPv6 edge case.
  [[ "${v}" =~ ^[0-9A-Fa-f:.]+$ ]]
}

is_hostname() {
  local v="$1"
  [[ "${v}" =~ ^[A-Za-z0-9]([A-Za-z0-9.-]{0,251}[A-Za-z0-9])?$ && "${v}" == *.* ]] || return 1
  [[ "${v}" != *".."* ]] || return 1
}

is_valid_public_host() {
  local v="$1"
  [[ -n "${v}" ]] || return 1
  [[ "${v}" != *"://"* && "${v}" != *"/"* && "${v}" != *" "* ]] || return 1

  # Numeric dotted values must be valid IPv4, not treated as hostnames.
  if [[ "${v}" =~ ^[0-9.]+$ ]]; then
    is_ipv4 "${v}"
    return
  fi

  is_ipv4 "${v}" || is_ipv6 "${v}" || is_hostname "${v}"
}

is_valid_sni_domain() {
  local v="$1"
  [[ -n "${v}" ]] || return 1
  [[ "${v}" != *"://"* && "${v}" != *"/"* && "${v}" != *" "* && "${v}" != *":"* ]] || return 1
  is_hostname "${v}"
}

validate_public_host() {
  is_valid_public_host "$1" || die "invalid host/domain/IP: $1"
}

validate_sni_domain() {
  is_valid_sni_domain "$1" || die "invalid SNI domain: $1"
}

format_uri_host() {
  local host="$1"
  if is_ipv6 "${host}"; then
    printf '[%s]\n' "${host}"
  else
    printf '%s\n' "${host}"
  fi
}

choose_listen_address() {
  local public_host="$1" ans
  if is_ipv6 "${public_host}"; then
    echo "::"
    warn "IPv6 public host detected; this node will listen on ::."
    warn "On some systems, :: may or may not also accept IPv4 depending on net.ipv6.bindv6only."
    return
  fi

  read -r -p "Enable IPv6/dual-stack listen with ::? Default is stable IPv4-only 0.0.0.0 [y/N]: " ans
  if [[ "${ans}" == "y" || "${ans}" == "Y" || "${ans}" == "yes" || "${ans}" == "YES" ]]; then
    echo "::"
    warn "Selected :: listen. If net.ipv6.bindv6only=1, IPv4 clients will not reach this inbound."
  else
    echo "0.0.0.0"
  fi
}

prompt_nonempty() {
  local prompt="$1" value
  while true; do
    read -r -p "${prompt}: " value
    [[ -n "${value}" ]] && { printf '%s\n' "${value}"; return; }
    echo "Value cannot be empty."
  done
}

prompt_public_host() {
  local value
  while true; do
    value="$(prompt_nonempty "$1")"
    if is_valid_public_host "${value}"; then
      printf '%s\n' "${value}"
      return
    fi
    echo "Invalid host. Use a domain, IPv4 address, or IPv6 literal, without scheme/path/port."
  done
}

prompt_sni() {
  local default="${1:-www.microsoft.com}" value
  while true; do
    read -r -p "Reality handshake/SNI domain [${default}]: " value
    value="${value:-$default}"
    if is_valid_sni_domain "${value}"; then
      printf '%s\n' "${value}"
      return
    fi
    echo "Invalid SNI. Use a normal domain, e.g. www.microsoft.com."
  done
}

prompt_port() {
  local prompt="${1:-Listen port}" port
  while true; do
    read -r -p "${prompt} [1-65535]: " port
    if [[ "${port}" =~ ^[0-9]+$ ]] && (( port >= 1 && port <= 65535 )); then
      printf '%s\n' "${port}"
      return
    fi
    echo "Invalid port."
  done
}

check_port_free() {
  local port="$1" proto="${2:-tcp}"
  if command -v ss >/dev/null 2>&1; then
    if [[ "${proto}" == "udp" ]]; then
      ss -H -lun 2>/dev/null | awk '{print $4}' | grep -Eq "[:.]${port}$" && die "UDP port already in use: ${port}"
    else
      ss -H -ltn 2>/dev/null | awk '{print $4}' | grep -Eq "[:.]${port}$" && die "TCP port already in use: ${port}"
    fi
  fi

  if [[ -f "${CONFIG_FILE}" ]]; then
    if jq -e --argjson p "${port}" '.inbounds[]? | select(.listen_port == $p)' "${CONFIG_FILE}" >/dev/null; then
      die "port already exists in sing-box config: ${port}"
    fi
  fi

  # Avoid configuration-level conflicts with managed mieru/mita nodes even when
  # the mita service is not currently running.
  if [[ -f "${MIERU_NODES_FILE:-}" ]]; then
    local proto_upper
    proto_upper="$(printf '%s' "${proto}" | tr '[:lower:]' '[:upper:]')"
    if jq -e --argjson p "${port}" --arg proto "${proto_upper}"       '.[]? | select(.port == $p and (.protocol == $proto or .protocol == "TCP" or .protocol == "UDP"))'       "${MIERU_NODES_FILE}" >/dev/null; then
      die "port already exists in managed mieru nodes: ${proto_upper}/${port}"
    fi
  fi
}

firewall_hint() {
  local port="$1" proto="$2"
  echo
  echo "Firewall/security-group reminder:"
  echo "  Open ${proto^^} port ${port} in the OS firewall and cloud security group if needed."
  if command -v ufw >/dev/null 2>&1; then
    echo "  ufw example: ufw allow ${port}/${proto}"
  elif command -v firewall-cmd >/dev/null 2>&1; then
    echo "  firewalld example: firewall-cmd --permanent --add-port=${port}/${proto} && firewall-cmd --reload"
  fi
}

run_as_singbox() {
  if command -v runuser >/dev/null 2>&1; then
    runuser -u "${SINGBOX_USER}" -- "$@"
  elif command -v su >/dev/null 2>&1; then
    su -s /bin/sh -c "$(printf '%q ' "$@")" "${SINGBOX_USER}"
  else
    die "neither runuser nor su is available"
  fi
}

copy_cert_to_managed_dir() {
  local src="$1" suffix="$2" name="$3" dst
  [[ -f "${src}" ]] || die "file not found: ${src}"
  name="$(safe_basename "${name}")"
  dst="${CERT_DIR}/${name}.${suffix}"
  if [[ -e "${dst}" ]]; then
    warn "Managed certificate file already exists and will be overwritten: ${dst}"
  fi
  install -m 0640 -o root -g "${SINGBOX_GROUP}" "${src}" "${dst}"
  printf '%s\n' "${dst}"
}

prompt_cert_paths_or_generate() {
  local domain="$1" safe cert_path key_path user_cert user_key san
  local tls_insecure="false"

  read -r -p "Certificate path (leave blank to generate self-signed certificate): " user_cert
  safe="$(safe_basename "${domain}")"

  if [[ -n "${user_cert}" ]]; then
    read -r -p "Private key path: " user_key
    cert_path="$(copy_cert_to_managed_dir "${user_cert}" "crt" "${safe}")"
    key_path="$(copy_cert_to_managed_dir "${user_key}" "key" "${safe}")"
    run_as_singbox test -r "${cert_path}" || die "singbox user cannot read ${cert_path}"
    run_as_singbox test -r "${key_path}" || die "singbox user cannot read ${key_path}"
    printf '%s\n%s\n%s\n' "${cert_path}" "${key_path}" "${tls_insecure}"
    return
  fi

  validate_public_host "${domain}"
  if is_ipv4 "${domain}" || is_ipv6 "${domain}"; then
    san="IP:${domain}"
  else
    san="DNS:${domain}"
  fi

  cert_path="${CERT_DIR}/${safe}.crt"
  key_path="${CERT_DIR}/${safe}.key"
  tls_insecure="true"

  if [[ -e "${cert_path}" || -e "${key_path}" ]]; then
    warn "Managed self-signed certificate/key may be overwritten for ${domain}:"
    warn "  ${cert_path}"
    warn "  ${key_path}"
  fi
  warn "Generating self-signed certificate for ${domain}; production use should prefer a real certificate."
  openssl req -x509 -nodes -newkey rsa:2048 -days 3650 \
    -subj "/CN=${domain}" \
    -addext "subjectAltName=${san}" \
    -keyout "${key_path}" \
    -out "${cert_path}" >/dev/null 2>&1

  chown root:"${SINGBOX_GROUP}" "${cert_path}" "${key_path}"
  chmod 640 "${cert_path}" "${key_path}"

  run_as_singbox test -r "${cert_path}" || die "singbox user cannot read ${cert_path}"
  run_as_singbox test -r "${key_path}" || die "singbox user cannot read ${key_path}"

  printf '%s\n%s\n%s\n' "${cert_path}" "${key_path}" "${tls_insecure}"
}

parse_reality_keypair() {
  local output="$1" priv pub
  priv="$(printf '%s\n' "${output}" | jq -r '.private_key // .PrivateKey // empty' 2>/dev/null || true)"
  pub="$(printf '%s\n' "${output}" | jq -r '.public_key // .PublicKey // empty' 2>/dev/null || true)"

  [[ -z "${priv}" ]] && priv="$(printf '%s\n' "${output}" | awk -F': *' '/PrivateKey/ {print $2; exit}')"
  [[ -z "${pub}" ]] && pub="$(printf '%s\n' "${output}" | awk -F': *' '/PublicKey/ {print $2; exit}')"

  [[ -n "${priv}" && -n "${pub}" ]] || die "failed to parse reality keypair output"
  printf '%s\n%s\n' "${priv}" "${pub}"
}

print_node_output() {
  local link="$1" client_json="${2:-null}"
  echo
  echo "Sensitive output. Do not paste this into public logs."
  echo
  echo "Share URI:"
  echo "${link}"
  echo
  echo "URI compatibility varies by client. For sing-box clients, prefer the JSON snippet below when available."
  if [[ "${client_json}" != "null" && -n "${client_json}" ]]; then
    echo
    echo "sing-box client outbound snippet / structured parameters:"
    jq . <<< "${client_json}" || printf '%s\n' "${client_json}"
  fi
}

add_vless_reality() {
  require_installed
  local port public_host uri_host listen_addr sni uuid tag keys priv pub short_id inbound link client_json
  local -a key_lines

  echo
  echo "Add VLESS + Reality"
  port="$(prompt_port "TCP listen port")"
  check_port_free "${port}" tcp
  public_host="$(prompt_public_host "Public server domain/IP used by clients")"
  uri_host="$(format_uri_host "${public_host}")"
  listen_addr="$(choose_listen_address "${public_host}")"
  sni="$(prompt_sni "www.microsoft.com")"

  uuid="$(new_uuid)"
  tag="vless-reality-$(random_hex 4)"
  short_id="$(random_hex 4)"

  keys="$("${BIN_PATH}" generate reality-keypair)"
  mapfile -t key_lines < <(parse_reality_keypair "${keys}")
  priv="${key_lines[0]}"
  pub="${key_lines[1]}"

  inbound="$(jq -n \
    --arg tag "${tag}" --argjson port "${port}" --arg uuid "${uuid}" \
    --arg sni "${sni}" --arg private_key "${priv}" --arg short_id "${short_id}" --arg listen "${listen_addr}" \
    '{
      type: "vless",
      tag: $tag,
      listen: $listen,
      listen_port: $port,
      users: [{name: "default", uuid: $uuid, flow: "xtls-rprx-vision"}],
      tls: {
        enabled: true,
        server_name: $sni,
        reality: {
          enabled: true,
          handshake: {server: $sni, server_port: 443},
          private_key: $private_key,
          short_id: [$short_id]
        }
      }
    }')"

  link="vless://${uuid}@${uri_host}:${port}?encryption=none&flow=xtls-rprx-vision&security=reality&sni=$(urlencode "${sni}")&fp=chrome&pbk=$(urlencode "${pub}")&sid=${short_id}&type=tcp#${tag}"

  client_json="$(jq -n \
    --arg tag "${tag}" --arg server "${public_host}" --argjson port "${port}" \
    --arg uuid "${uuid}" --arg sni "${sni}" --arg pub "${pub}" --arg sid "${short_id}" \
    '{
      type: "vless",
      tag: $tag,
      server: $server,
      server_port: $port,
      uuid: $uuid,
      flow: "xtls-rprx-vision",
      tls: {
        enabled: true,
        server_name: $sni,
        utls: {enabled: true, fingerprint: "chrome"},
        reality: {enabled: true, public_key: $pub, short_id: $sid}
      }
    }')"

  commit_node_transaction "${inbound}" "vless-reality" "${tag}" "${link}" "${client_json}"
  print_node_output "${link}" "${client_json}"
  firewall_hint "${port}" tcp
  reload_or_restart
}

add_trojan_tls() {
  require_installed
  local port public_host uri_host listen_addr password tag cert_path key_path tls_insecure inbound link client_json insecure_param
  local -a cert_data

  echo
  echo "Add Trojan + TLS"
  port="$(prompt_port "TCP listen port")"
  check_port_free "${port}" tcp
  public_host="$(prompt_public_host "Public server domain used by clients")"
  uri_host="$(format_uri_host "${public_host}")"
  listen_addr="$(choose_listen_address "${public_host}")"

  password="$(random_b64url 24)"
  tag="trojan-$(random_hex 4)"
  mapfile -t cert_data < <(prompt_cert_paths_or_generate "${public_host}")
  cert_path="${cert_data[0]}"
  key_path="${cert_data[1]}"
  tls_insecure="${cert_data[2]}"

  inbound="$(jq -n \
    --arg tag "${tag}" --argjson port "${port}" --arg password "${password}" \
    --arg cert "${cert_path}" --arg key "${key_path}" --arg server_name "${public_host}" --arg listen "${listen_addr}" \
    '{
      type: "trojan",
      tag: $tag,
      listen: $listen,
      listen_port: $port,
      users: [{name: "default", password: $password}],
      tls: {enabled: true, server_name: $server_name, certificate_path: $cert, key_path: $key}
    }')"

  insecure_param=""
  [[ "${tls_insecure}" == "true" ]] && insecure_param="&allowInsecure=1"
  link="trojan://${password}@${uri_host}:${port}?security=tls&sni=$(urlencode "${public_host}")${insecure_param}#${tag}"

  client_json="$(jq -n \
    --arg tag "${tag}" --arg server "${public_host}" --argjson port "${port}" \
    --arg password "${password}" --argjson insecure "${tls_insecure}" \
    '{type:"trojan",tag:$tag,server:$server,server_port:$port,password:$password,tls:{enabled:true,server_name:$server,insecure:$insecure}}')"

  commit_node_transaction "${inbound}" "trojan" "${tag}" "${link}" "${client_json}"
  print_node_output "${link}" "${client_json}"
  firewall_hint "${port}" tcp
  reload_or_restart
}

add_hysteria2_tls() {
  require_installed
  local port public_host uri_host listen_addr password obfs tag cert_path key_path tls_insecure inbound link client_json insecure_param
  local -a cert_data

  echo
  echo "Add Hysteria2 + TLS"
  port="$(prompt_port "UDP listen port")"
  check_port_free "${port}" udp
  public_host="$(prompt_public_host "Public server domain used by clients")"
  uri_host="$(format_uri_host "${public_host}")"
  listen_addr="$(choose_listen_address "${public_host}")"

  password="$(random_b64url 24)"
  obfs="$(random_b64url 18)"
  tag="hy2-$(random_hex 4)"

  mapfile -t cert_data < <(prompt_cert_paths_or_generate "${public_host}")
  cert_path="${cert_data[0]}"
  key_path="${cert_data[1]}"
  tls_insecure="${cert_data[2]}"

  inbound="$(jq -n \
    --arg tag "${tag}" --argjson port "${port}" --arg password "${password}" --arg obfs "${obfs}" \
    --arg cert "${cert_path}" --arg key "${key_path}" --arg server_name "${public_host}" --arg listen "${listen_addr}" \
    '{
      type: "hysteria2",
      tag: $tag,
      listen: $listen,
      listen_port: $port,
      users: [{name: "default", password: $password}],
      obfs: {type: "salamander", password: $obfs},
      ignore_client_bandwidth: false,
      tls: {enabled: true, server_name: $server_name, certificate_path: $cert, key_path: $key}
    }')"

  insecure_param=""
  [[ "${tls_insecure}" == "true" ]] && insecure_param="&insecure=1"
  link="hysteria2://${password}@${uri_host}:${port}?sni=$(urlencode "${public_host}")&obfs=salamander&obfs-password=$(urlencode "${obfs}")${insecure_param}#${tag}"

  client_json="$(jq -n \
    --arg tag "${tag}" --arg server "${public_host}" --argjson port "${port}" \
    --arg password "${password}" --arg obfs "${obfs}" --argjson insecure "${tls_insecure}" \
    '{type:"hysteria2",tag:$tag,server:$server,server_port:$port,password:$password,obfs:{type:"salamander",password:$obfs},tls:{enabled:true,server_name:$server,insecure:$insecure}}')"

  commit_node_transaction "${inbound}" "hysteria2" "${tag}" "${link}" "${client_json}"
  print_node_output "${link}" "${client_json}"
  firewall_hint "${port}" udp
  reload_or_restart
}

add_tuic_tls() {
  require_installed
  local port public_host uri_host listen_addr uuid password tag cert_path key_path tls_insecure inbound link client_json insecure_param
  local -a cert_data

  echo
  echo "Add TUIC + TLS"
  port="$(prompt_port "UDP listen port")"
  check_port_free "${port}" udp
  public_host="$(prompt_public_host "Public server domain used by clients")"
  uri_host="$(format_uri_host "${public_host}")"
  listen_addr="$(choose_listen_address "${public_host}")"

  uuid="$(new_uuid)"
  password="$(random_b64url 20)"
  tag="tuic-$(random_hex 4)"

  mapfile -t cert_data < <(prompt_cert_paths_or_generate "${public_host}")
  cert_path="${cert_data[0]}"
  key_path="${cert_data[1]}"
  tls_insecure="${cert_data[2]}"

  inbound="$(jq -n \
    --arg tag "${tag}" --argjson port "${port}" --arg uuid "${uuid}" --arg password "${password}" \
    --arg cert "${cert_path}" --arg key "${key_path}" --arg server_name "${public_host}" --arg listen "${listen_addr}" \
    '{
      type: "tuic",
      tag: $tag,
      listen: $listen,
      listen_port: $port,
      users: [{name: "default", uuid: $uuid, password: $password}],
      congestion_control: "bbr",
      zero_rtt_handshake: false,
      heartbeat: "10s",
      tls: {enabled: true, server_name: $server_name, certificate_path: $cert, key_path: $key}
    }')"

  insecure_param=""
  [[ "${tls_insecure}" == "true" ]] && insecure_param="&allow_insecure=1"
  link="tuic://${uuid}:$(urlencode "${password}")@${uri_host}:${port}?congestion_control=bbr&sni=$(urlencode "${public_host}")${insecure_param}#${tag}"

  client_json="$(jq -n \
    --arg tag "${tag}" --arg server "${public_host}" --argjson port "${port}" \
    --arg uuid "${uuid}" --arg password "${password}" --argjson insecure "${tls_insecure}" \
    '{type:"tuic",tag:$tag,server:$server,server_port:$port,uuid:$uuid,password:$password,congestion_control:"bbr",tls:{enabled:true,server_name:$server,insecure:$insecure}}')"

  commit_node_transaction "${inbound}" "tuic" "${tag}" "${link}" "${client_json}"
  print_node_output "${link}" "${client_json}"
  firewall_hint "${port}" udp
  reload_or_restart
}

select_ss_method() {
  local choice
  cat <<EOF
Select Shadowsocks method:
1) 2022-blake3-aes-128-gcm
2) 2022-blake3-aes-256-gcm
3) 2022-blake3-chacha20-poly1305
4) aes-128-gcm
5) aes-256-gcm
6) chacha20-ietf-poly1305
7) xchacha20-ietf-poly1305
EOF
  read -r -p "Select [1-7, default 1]: " choice
  case "${choice:-1}" in
    1) echo "2022-blake3-aes-128-gcm" ;;
    2) echo "2022-blake3-aes-256-gcm" ;;
    3) echo "2022-blake3-chacha20-poly1305" ;;
    4) echo "aes-128-gcm" ;;
    5) echo "aes-256-gcm" ;;
    6) echo "chacha20-ietf-poly1305" ;;
    7) echo "xchacha20-ietf-poly1305" ;;
    *) die "invalid Shadowsocks method selection" ;;
  esac
}

ss_password_for_method() {
  local method="$1"
  case "${method}" in
    2022-blake3-aes-128-gcm) openssl rand -base64 16 ;;
    2022-blake3-aes-256-gcm|2022-blake3-chacha20-poly1305) openssl rand -base64 32 ;;
    aes-128-gcm|aes-256-gcm|chacha20-ietf-poly1305|xchacha20-ietf-poly1305) random_b64url 24 ;;
    *) die "unsupported Shadowsocks method: ${method}" ;;
  esac
}

add_shadowsocks() {
  require_installed
  local port public_host uri_host listen_addr method password tag inbound link client_json ss_userinfo

  echo
  echo "Add Shadowsocks"
  port="$(prompt_port "TCP/UDP listen port")"
  check_port_free "${port}" tcp
  check_port_free "${port}" udp
  public_host="$(prompt_public_host "Public server domain/IP used by clients")"
  uri_host="$(format_uri_host "${public_host}")"
  listen_addr="$(choose_listen_address "${public_host}")"

  method="$(select_ss_method)"
  password="$(ss_password_for_method "${method}")"
  tag="ss-$(random_hex 4)"

  inbound="$(jq -n \
    --arg tag "${tag}" --argjson port "${port}" --arg method "${method}" --arg password "${password}" --arg listen "${listen_addr}" \
    '{
      type: "shadowsocks",
      tag: $tag,
      listen: $listen,
      listen_port: $port,
      method: $method,
      password: $password
    }')"

  ss_userinfo="$(printf '%s:%s' "${method}" "${password}" | base64 | tr '+/' '-_' | tr -d '=\n')"
  link="ss://${ss_userinfo}@${uri_host}:${port}#${tag}"

  client_json="$(jq -n \
    --arg tag "${tag}" --arg server "${public_host}" --argjson port "${port}" --arg method "${method}" --arg password "${password}" \
    '{type:"shadowsocks",tag:$tag,server:$server,server_port:$port,method:$method,password:$password}')"

  commit_node_transaction "${inbound}" "shadowsocks" "${tag}" "${link}" "${client_json}"
  print_node_output "${link}" "${client_json}"
  firewall_hint "${port}" tcp
  firewall_hint "${port}" udp
  reload_or_restart
}

list_nodes() {
  require_installed
  echo
  echo "Sensitive output: node links contain credentials."
  if [[ ! -s "${NODES_FILE}" ]] || [[ "$(jq 'length' "${NODES_FILE}")" -eq 0 ]]; then
    echo "No nodes."
    return
  fi
  jq -r 'to_entries[] | "\(.key+1). [\(.value.type)] \(.value.tag)\n   \(.value.link)"' "${NODES_FILE}"
}

list_nodes_safe() {
  require_installed
  echo
  if [[ ! -s "${NODES_FILE}" ]] || [[ "$(jq 'length' "${NODES_FILE}")" -eq 0 ]]; then
    echo "No nodes."
    return
  fi
  jq -r 'to_entries[] | "\(.key+1). [\(.value.type)] \(.value.tag)"' "${NODES_FILE}"
}

delete_node() {
  require_installed
  local idx count tag

  count="$(jq 'length' "${NODES_FILE}")"
  [[ "${count}" -gt 0 ]] || { echo "No nodes."; return; }

  list_nodes_safe
  echo
  idx="$(prompt_nonempty "Node number to delete")"
  [[ "${idx}" =~ ^[0-9]+$ ]] || die "invalid number"
  (( idx >= 1 && idx <= count )) || die "number out of range"

  tag="$(jq -r --argjson i "$((idx-1))" '.[$i].tag' "${NODES_FILE}")"
  delete_node_transaction "$((idx-1))" "${tag}"
  echo "Deleted node: ${tag}"
  reload_or_restart
}

show_config() {
  require_installed
  warn "Full config contains secrets."
  jq . "${CONFIG_FILE}"
}

check_config() {
  require_installed
  "${BIN_PATH}" check -c "${CONFIG_FILE}"
}

service_start() {
  require_installed
  systemctl start sing-box.service
  systemctl status sing-box.service --no-pager -l
}

service_stop() {
  require_installed
  systemctl stop sing-box.service
  systemctl status sing-box.service --no-pager -l || true
}

service_restart() {
  require_installed
  systemctl restart sing-box.service
  systemctl status sing-box.service --no-pager -l
}

view_logs() {
  require_installed
  journalctl -u sing-box.service -n 120 --no-pager
}



# ---------------------------------------------------------------------------
# Cautious cleanup for 233boy-style and similar proxy services
# ---------------------------------------------------------------------------

proxy_candidate_units() {
  # Explicit list first. Then include matching installed unit files if systemd can list them.
  {
    printf '%s\n' \
      "sing-box.service" "singbox.service" "xray.service" "v2ray.service" \
      "sing-box@.service" "xray@.service" "v2ray@.service"

    if command -v systemctl >/dev/null 2>&1; then
      systemctl list-unit-files --no-legend --no-pager 2>/dev/null \
        | awk '{print $1}' \
        | grep -E '^(sing-box|singbox|xray|v2ray)(@.*)?\.service$' || true
    fi
  } | awk 'NF' | sort -u
}

proxy_candidate_paths() {
  cat <<'EOF'
/etc/sing-box
/etc/singbox
/usr/local/etc/sing-box
/usr/local/etc/xray
/usr/local/etc/v2ray
/etc/xray
/etc/v2ray
/var/log/sing-box
/var/log/xray
/var/log/v2ray
/var/lib/sing-box
/var/lib/xray
/var/lib/v2ray
/opt/sing-box
/opt/xray
/opt/v2ray
/usr/local/share/xray
/usr/share/xray
/usr/local/bin/sing-box
/usr/local/bin/singbox
/usr/bin/sing-box
/usr/bin/singbox
/usr/local/bin/xray
/usr/bin/xray
/usr/local/bin/v2ray
/usr/bin/v2ray
/usr/local/bin/sb
EOF
}

proxy_unit_file_paths_for() {
  local unit="$1"
  printf '%s\n' \
    "/etc/systemd/system/${unit}" \
    "/lib/systemd/system/${unit}" \
    "/usr/lib/systemd/system/${unit}"
}

is_safe_sb_candidate() {
  local p="$1" target
  [[ -e "${p}" ]] || return 1
  if [[ -L "${p}" ]]; then
    target="$(readlink "${p}" || true)"
    [[ "${target}" == *"sing-box"* || "${target}" == *"/etc/sing-box"* || "${target}" == *"233boy"* ]]
    return
  fi
  if [[ -f "${p}" ]]; then
    grep -aqE 'sing-box|/etc/sing-box|233boy' "${p}" 2>/dev/null
    return
  fi
  return 1
}

proxy_audit_cleanup_candidates() {
  echo
  echo "Proxy cleanup audit: 233boy / sing-box / xray / v2ray candidates"
  echo "This audit intentionally excludes unrelated services such as nginx, caddy, ssh, databases and firewall rules."
  echo

  echo "[Systemd units]"
  local unit found_unit=0
  while IFS= read -r unit; do
    if systemctl list-unit-files --no-legend --no-pager 2>/dev/null | awk '{print $1}' | grep -qx "${unit}" \
      || systemctl status "${unit}" >/dev/null 2>&1; then
      echo "  ${unit}"
      found_unit=1
    fi
  done < <(proxy_candidate_units)
  [[ "${found_unit}" -eq 0 ]] && echo "  none detected"

  echo
  echo "[Files/directories]"
  local path found_path=0
  while IFS= read -r path; do
    [[ -e "${path}" ]] || continue
    if [[ "${path}" == "/usr/local/bin/sb" ]]; then
      if is_safe_sb_candidate "${path}"; then
        echo "  ${path}"
        found_path=1
      else
        echo "  ${path}  (exists but not recognized as sing-box/233boy; will be skipped)"
      fi
    else
      echo "  ${path}"
      found_path=1
    fi
  done < <(proxy_candidate_paths)
  [[ "${found_path}" -eq 0 ]] && echo "  none detected"

  echo
  echo "[Root shell aliases / references]"
  if [[ -f /root/.bashrc ]] && grep -nE '(/usr/local/bin/sb|/usr/local/bin/sing-box|alias[[:space:]]+sb=|233boy|sing-box)' /root/.bashrc; then
    true
  else
    echo "  none detected in /root/.bashrc"
  fi

  echo
  echo "[Cron references, report only]"
  if grep -RInE '233boy|sing-box|xray|v2ray' /etc/cron* /var/spool/cron 2>/dev/null; then
    echo "  Cron references above are report-only and will not be edited automatically."
  else
    echo "  none detected"
  fi
}

stop_disable_proxy_units() {
  local unit
  while IFS= read -r unit; do
    [[ -n "${unit}" ]] || continue
    if systemctl status "${unit}" >/dev/null 2>&1 || systemctl list-unit-files --no-legend --no-pager 2>/dev/null | awk '{print $1}' | grep -qx "${unit}"; then
      info "Stopping/disabling ${unit}"
      systemctl stop "${unit}" 2>/dev/null || true
      systemctl disable "${unit}" 2>/dev/null || true
      systemctl reset-failed "${unit}" 2>/dev/null || true
    fi
  done < <(proxy_candidate_units)
}

remove_proxy_unit_files() {
  local unit file
  while IFS= read -r unit; do
    while IFS= read -r file; do
      if [[ -e "${file}" ]]; then
        info "Removing unit file ${file}"
        rm -f -- "${file}"
      fi
    done < <(proxy_unit_file_paths_for "${unit}")
  done < <(proxy_candidate_units)
}

remove_proxy_paths() {
  local path
  while IFS= read -r path; do
    [[ -e "${path}" ]] || continue

    if [[ "${path}" == "/usr/local/bin/sb" ]]; then
      if is_safe_sb_candidate "${path}"; then
        info "Removing recognized 233boy/sing-box helper ${path}"
        rm -rf -- "${path}"
      else
        warn "Skipping ${path}; it exists but is not recognized as a sing-box/233boy helper."
      fi
      continue
    fi

    info "Removing ${path}"
    rm -rf -- "${path}"
  done < <(proxy_candidate_paths)
}

clean_root_bashrc_proxy_entries() {
  local file="/root/.bashrc" backup
  [[ -f "${file}" ]] || return 0

  if ! grep -qE '(/usr/local/bin/sb|/usr/local/bin/sing-box|alias[[:space:]]+sb=|233boy|sing-box)' "${file}"; then
    return 0
  fi

  backup="/root/.bashrc.proxy-cleanup.$(date +%Y%m%d%H%M%S).bak"
  cp -a "${file}" "${backup}"
  info "Backed up /root/.bashrc to ${backup}"

  # Remove only lines that are strongly tied to 233boy/sing-box helpers.
  grep -vE '(/usr/local/bin/sb|/usr/local/bin/sing-box|alias[[:space:]]+sb=|233boy|sing-box)' "${backup}" > "${file}"
  chmod --reference="${backup}" "${file}" 2>/dev/null || chmod 600 "${file}"
}

clean_uninstall_proxy_stack() {
  require_systemd

  echo
  echo "This will clean known proxy installations only:"
  echo "  - 233boy-style sing-box files, especially /etc/sing-box and /usr/local/bin/sb"
  echo "  - sing-box / singbox systemd units, binaries, configs and logs"
  echo "  - xray / v2ray systemd units, binaries, configs and logs"
  echo
  echo "It will NOT remove unrelated services such as nginx, caddy, ssh, databases, firewall rules, or cloud security-group settings."
  echo "Cron references will be reported by the audit but not edited automatically."
  echo

  proxy_audit_cleanup_candidates

  echo
  warn "This is destructive. Review the audit above before continuing."
  confirm "Proceed with clean uninstall of detected proxy stack" || return

  stop_disable_proxy_units
  remove_proxy_unit_files
  remove_proxy_paths
  clean_root_bashrc_proxy_entries

  systemctl daemon-reload
  systemctl reset-failed 2>/dev/null || true

  echo
  echo "Proxy cleanup completed. Run the audit again to verify."
  echo "If cron references were reported, inspect them manually before editing."

  prompt_optional_web_cleanup_after_proxy
}


# ---------------------------------------------------------------------------
# Optional web server cleanup: nginx / caddy
# ---------------------------------------------------------------------------

web_candidate_units_for() {
  local name="$1"
  case "${name}" in
    nginx) printf '%s\n' "nginx.service" ;;
    caddy) printf '%s\n' "caddy.service" ;;
    *) return 1 ;;
  esac
}

web_candidate_paths_for() {
  local name="$1"
  case "${name}" in
    nginx)
      cat <<'EOF'
/etc/nginx
/var/log/nginx
/var/cache/nginx
/run/nginx.pid
/usr/local/nginx
/usr/local/sbin/nginx
/usr/local/bin/nginx
/usr/sbin/nginx
/usr/bin/nginx
EOF
      ;;
    caddy)
      cat <<'EOF'
/etc/caddy
/var/log/caddy
/var/lib/caddy
/var/cache/caddy
/usr/local/bin/caddy
/usr/bin/caddy
/usr/sbin/caddy
/etc/apt/sources.list.d/caddy-stable.list
/etc/apt/sources.list.d/caddy.list
/usr/share/keyrings/caddy-stable-archive-keyring.gpg
/usr/share/keyrings/caddy-archive-keyring.gpg
EOF
      ;;
    *)
      return 1
      ;;
  esac
}

web_package_names_for() {
  local name="$1"
  case "${name}" in
    nginx) printf '%s\n' "nginx" "nginx-common" "nginx-core" "nginx-full" ;;
    caddy) printf '%s\n' "caddy" ;;
    *) return 1 ;;
  esac
}

web_existing_paths_for() {
  local name="$1" path
  while IFS= read -r path; do
    # Support limited glob expansion for caddy repo files.
    if [[ "${path}" == *"*"* ]]; then
      for expanded in ${path}; do
        [[ -e "${expanded}" ]] && printf '%s\n' "${expanded}"
      done
    else
      [[ -e "${path}" ]] && printf '%s\n' "${path}"
    fi
  done < <(web_candidate_paths_for "${name}")

  if [[ "${name}" == "caddy" ]]; then
    for expanded in /etc/yum.repos.d/caddy*.repo /etc/zypp/repos.d/caddy*.repo; do
      [[ -e "${expanded}" ]] && printf '%s\n' "${expanded}"
    done
  fi
}

backup_web_server_config() {
  local name="$1" backup_root src found=0
  backup_root="$(mktemp -d "/root/web-cleanup-backup.${name}.$(date +%Y%m%d%H%M%S).XXXXXX")"
  chmod 700 "${backup_root}"

  case "${name}" in
    nginx)
      for src in /etc/nginx; do
        if [[ -e "${src}" ]]; then
          cp -a "${src}" "${backup_root}/"
          found=1
        fi
      done
      ;;
    caddy)
      for src in /etc/caddy /var/lib/caddy; do
        if [[ -e "${src}" ]]; then
          cp -a "${src}" "${backup_root}/"
          found=1
        fi
      done
      ;;
  esac

  if [[ "${found}" -eq 1 ]]; then
    info "Backed up ${name} config/state to ${backup_root}"
  else
    rmdir "${backup_root}" 2>/dev/null || true
  fi
}

audit_web_server_cleanup_candidates() {
  local name="$1" unit path found=0

  echo
  echo "[Optional ${name} cleanup candidates]"
  echo "  Web roots such as /var/www are intentionally not listed and will not be removed."

  echo "  Systemd units:"
  while IFS= read -r unit; do
    if systemctl status "${unit}" >/dev/null 2>&1 || systemctl list-unit-files --no-legend --no-pager 2>/dev/null | awk '{print $1}' | grep -qx "${unit}"; then
      echo "    ${unit}"
      found=1
    fi
  done < <(web_candidate_units_for "${name}")
  [[ "${found}" -eq 0 ]] && echo "    none detected"

  found=0
  echo "  Files/directories:"
  while IFS= read -r path; do
    echo "    ${path}"
    found=1
  done < <(web_existing_paths_for "${name}")
  [[ "${found}" -eq 0 ]] && echo "    none detected"

  local found_pkg=0
  echo "  Packages:"
  if command -v dpkg >/dev/null 2>&1; then
    while IFS= read -r pkg; do
      if dpkg -s "${pkg}" >/dev/null 2>&1; then
        echo "    ${pkg}"
        found_pkg=1
      fi
    done < <(web_package_names_for "${name}") || true
    [[ "${found_pkg}" -eq 0 ]] && echo "    none detected"
  elif command -v rpm >/dev/null 2>&1; then
    while IFS= read -r pkg; do
      if rpm -q "${pkg}" >/dev/null 2>&1; then
        echo "    ${pkg}"
        found_pkg=1
      fi
    done < <(web_package_names_for "${name}") || true
    [[ "${found_pkg}" -eq 0 ]] && echo "    none detected"
  elif command -v pacman >/dev/null 2>&1; then
    while IFS= read -r pkg; do
      if pacman -Qi "${pkg}" >/dev/null 2>&1; then
        echo "    ${pkg}"
        found_pkg=1
      fi
    done < <(web_package_names_for "${name}") || true
    [[ "${found_pkg}" -eq 0 ]] && echo "    none detected"
  elif command -v apk >/dev/null 2>&1; then
    while IFS= read -r pkg; do
      if apk info -e "${pkg}" >/dev/null 2>&1; then
        echo "    ${pkg}"
        found_pkg=1
      fi
    done < <(web_package_names_for "${name}") || true
    [[ "${found_pkg}" -eq 0 ]] && echo "    none detected"
  else
    echo "    package manager detection skipped"
  fi
}

remove_web_packages() {
  local name="$1"
  local pkg
  local installed_pkgs=()

  if command -v dpkg >/dev/null 2>&1; then
    while IFS= read -r pkg; do
      [[ -n "${pkg}" ]] || continue
      dpkg -s "${pkg}" >/dev/null 2>&1 && installed_pkgs+=("${pkg}")
    done < <(web_package_names_for "${name}")

    if [[ "${#installed_pkgs[@]}" -gt 0 ]]; then
      info "Removing installed ${name} package(s): ${installed_pkgs[*]}"
      DEBIAN_FRONTEND=noninteractive apt-get purge -y "${installed_pkgs[@]}" || warn "Some ${name} packages may not have been removed cleanly."
      DEBIAN_FRONTEND=noninteractive apt-get autoremove -y || true
    else
      info "No installed ${name} packages detected by dpkg."
    fi
    return 0
  fi

  if command -v rpm >/dev/null 2>&1; then
    while IFS= read -r pkg; do
      [[ -n "${pkg}" ]] || continue
      rpm -q "${pkg}" >/dev/null 2>&1 && installed_pkgs+=("${pkg}")
    done < <(web_package_names_for "${name}")

    if [[ "${#installed_pkgs[@]}" -gt 0 ]]; then
      info "Removing installed ${name} package(s): ${installed_pkgs[*]}"
      if command -v dnf >/dev/null 2>&1; then
        dnf remove -y "${installed_pkgs[@]}" || warn "Some ${name} packages may not have been removed cleanly."
      elif command -v yum >/dev/null 2>&1; then
        yum remove -y "${installed_pkgs[@]}" || warn "Some ${name} packages may not have been removed cleanly."
      elif command -v zypper >/dev/null 2>&1; then
        zypper --non-interactive remove "${installed_pkgs[@]}" || warn "Some ${name} packages may not have been removed cleanly."
      else
        rpm -e "${installed_pkgs[@]}" || warn "Some ${name} packages may not have been removed cleanly."
      fi
    else
      info "No installed ${name} packages detected by rpm."
    fi
    return 0
  fi

  if command -v pacman >/dev/null 2>&1; then
    while IFS= read -r pkg; do
      [[ -n "${pkg}" ]] || continue
      pacman -Qi "${pkg}" >/dev/null 2>&1 && installed_pkgs+=("${pkg}")
    done < <(web_package_names_for "${name}")

    if [[ "${#installed_pkgs[@]}" -gt 0 ]]; then
      info "Removing installed ${name} package(s): ${installed_pkgs[*]}"
      pacman -Rns --noconfirm "${installed_pkgs[@]}" || warn "Some ${name} packages may not have been removed cleanly."
    else
      info "No installed ${name} packages detected by pacman."
    fi
    return 0
  fi

  if command -v apk >/dev/null 2>&1; then
    while IFS= read -r pkg; do
      [[ -n "${pkg}" ]] || continue
      apk info -e "${pkg}" >/dev/null 2>&1 && installed_pkgs+=("${pkg}")
    done < <(web_package_names_for "${name}")

    if [[ "${#installed_pkgs[@]}" -gt 0 ]]; then
      info "Removing installed ${name} package(s): ${installed_pkgs[*]}"
      apk del "${installed_pkgs[@]}" || warn "Some ${name} packages may not have been removed cleanly."
    else
      info "No installed ${name} packages detected by apk."
    fi
    return 0
  fi

  warn "No supported package manager found for removing ${name}; removing known files only."
}

clean_uninstall_web_server() {
  local name="$1" unit path

  case "${name}" in
    nginx|caddy) ;;
    *) die "unsupported web server cleanup target: ${name}" ;;
  esac

  audit_web_server_cleanup_candidates "${name}"

  echo
  warn "Optional cleanup target: ${name}"
  warn "This may remove ${name} package(s), service unit(s), configs, logs, cache, and binaries."
  warn "Config/state will be backed up first; logs are NOT backed up automatically."
  warn "If you need logs, copy them manually before continuing."
  warn "If manually installed binaries are in standard nginx/caddy paths, they will also be removed."
  warn "It will NOT remove /var/www or other web roots."
  confirm "Explicitly remove ${name}" || return

  backup_web_server_config "${name}"

  while IFS= read -r unit; do
    info "Stopping/disabling ${unit}"
    systemctl stop "${unit}" 2>/dev/null || true
    systemctl disable "${unit}" 2>/dev/null || true
    systemctl reset-failed "${unit}" 2>/dev/null || true
  done < <(web_candidate_units_for "${name}")

  remove_web_packages "${name}"

  while IFS= read -r unit; do
    rm -f -- "/etc/systemd/system/${unit}" "/lib/systemd/system/${unit}" "/usr/lib/systemd/system/${unit}"
  done < <(web_candidate_units_for "${name}")

  while IFS= read -r path; do
    [[ -e "${path}" ]] || continue
    info "Removing ${path}"
    rm -rf -- "${path}"
  done < <(web_existing_paths_for "${name}")

  systemctl daemon-reload
  systemctl reset-failed 2>/dev/null || true
  echo "${name} cleanup completed."
}

prompt_optional_web_cleanup_after_proxy() {
  echo
  echo "Optional extra cleanup:"
  echo "  nginx and caddy are not proxy-stack targets by default."
  echo "  You can manually choose to remove either one now."
  echo "  /var/www and other web roots will be preserved."
  echo

  if confirm "Also remove nginx"; then
    clean_uninstall_web_server nginx
  fi

  if confirm "Also remove caddy"; then
    clean_uninstall_web_server caddy
  fi
}

# ---------------------------------------------------------------------------
# Optional independent mieru/mita support
# ---------------------------------------------------------------------------

init_mieru_state() {
  mkdir -p "${MIERU_MANAGED_DIR}"
  chown root:root "${MIERU_MANAGED_DIR}"
  chmod 700 "${MIERU_MANAGED_DIR}"

  if [[ ! -f "${MIERU_NODES_FILE}" ]]; then
    printf '[]\n' > "${MIERU_NODES_FILE}"
  fi
  if [[ ! -f "${MIERU_LINKS_FILE}" ]]; then
    : > "${MIERU_LINKS_FILE}"
  fi

  chown root:root "${MIERU_NODES_FILE}" "${MIERU_LINKS_FILE}"
  chmod 600 "${MIERU_NODES_FILE}" "${MIERU_LINKS_FILE}"
}

require_mita() {
  command -v mita >/dev/null 2>&1 || die "mita is not installed; choose Install / update mita first"
}

detect_mita_package() {
  local arch pkg url installer

  case "$(uname -m)" in
    x86_64|amd64)
      if command -v dpkg >/dev/null 2>&1; then
        arch="amd64"
        pkg="mita_${MIERU_VERSION}_${arch}.deb"
        installer="deb"
      elif command -v rpm >/dev/null 2>&1; then
        arch="x86_64"
        pkg="mita-${MIERU_VERSION}-1.${arch}.rpm"
        installer="rpm"
      else
        die "neither dpkg nor rpm is available for installing mita"
      fi
      ;;
    aarch64|arm64)
      if command -v dpkg >/dev/null 2>&1; then
        arch="arm64"
        pkg="mita_${MIERU_VERSION}_${arch}.deb"
        installer="deb"
      elif command -v rpm >/dev/null 2>&1; then
        arch="aarch64"
        pkg="mita-${MIERU_VERSION}-1.${arch}.rpm"
        installer="rpm"
      else
        die "neither dpkg nor rpm is available for installing mita"
      fi
      ;;
    *)
      die "unsupported architecture for mita package: $(uname -m)"
      ;;
  esac

  url="https://github.com/enfein/mieru/releases/download/v${MIERU_VERSION}/${pkg}"
  printf '%s\n%s\n%s\n' "${pkg}" "${url}" "${installer}"
}

install_mita() {
  require_systemd
  command -v curl >/dev/null 2>&1 || install_deps

  local pkg url installer package_file actual
  local -a pkg_info

  mapfile -t pkg_info < <(detect_mita_package)
  pkg="${pkg_info[0]}"
  url="${pkg_info[1]}"
  installer="${pkg_info[2]}"

  if [[ -z "${MIERU_EXPECTED_SHA256}" && "${ALLOW_UNVERIFIED_DOWNLOAD}" -ne 1 ]]; then
    warn "MIERU_EXPECTED_SHA256 is not set. This weakens supply-chain protection."
    confirm "Proceed with mita package download without SHA256 verification" || die "aborted; set --mieru-sha256 or --allow-unverified-download"
  fi

  tmpdir="$(mktemp -d)"
  package_file="${tmpdir}/${pkg}"

  info "Downloading mita ${MIERU_VERSION}: ${url}"
  curl --fail --location --proto '=https' --tlsv1.2 --retry 3 --output "${package_file}" "${url}"

  if [[ -n "${MIERU_EXPECTED_SHA256}" ]]; then
    actual="$(sha256sum "${package_file}" | awk '{print $1}')"
    [[ "${actual}" == "${MIERU_EXPECTED_SHA256}" ]] || die "mita package SHA256 mismatch: expected ${MIERU_EXPECTED_SHA256}, got ${actual}"
    info "mita package SHA256 verified"
  else
    warn "Skipped mita package SHA256 verification by explicit opt-in"
  fi

  if [[ "${installer}" == "deb" ]]; then
    dpkg -i "${package_file}" || {
      command -v apt-get >/dev/null 2>&1 || die "dpkg install failed and apt-get is unavailable"
      DEBIAN_FRONTEND=noninteractive apt-get install -f -y
    }
  else
    rpm -Uvh --force "${package_file}"
  fi

  init_mieru_state
  systemctl enable mita >/dev/null 2>&1 || true
  command -v mita >/dev/null 2>&1 && mita version || true
  info "mita installation complete"
}

prompt_mieru_protocol() {
  local choice
  echo "Select mieru transport protocol:"
  echo "1) TCP"
  echo "2) UDP"
  read -r -p "Select [1-2, default 1]: " choice
  case "${choice:-1}" in
    1) echo "TCP" ;;
    2) echo "UDP" ;;
    *) die "invalid protocol selection" ;;
  esac
}

prompt_mieru_port() {
  local port
  while true; do
    read -r -p "mita listen port [1025-65535]: " port
    if [[ "${port}" =~ ^[0-9]+$ ]] && (( port >= 1025 && port <= 65535 )); then
      printf '%s\n' "${port}"
      return
    fi
    echo "Invalid port. mieru/mita requires 1025-65535."
  done
}

mieru_host_client_fields() {
  local public_host="$1"
  if is_ipv4 "${public_host}" || is_ipv6 "${public_host}"; then
    jq -n --arg ip "${public_host}" '{ipAddress:$ip, domainName:""}'
  else
    jq -n --arg domain "${public_host}" '{ipAddress:"", domainName:$domain}'
  fi
}

mieru_rebuild_links_file() {
  jq -r '.[].link' "${MIERU_NODES_FILE}" > "${MIERU_LINKS_FILE}.tmp"
  mv "${MIERU_LINKS_FILE}.tmp" "${MIERU_LINKS_FILE}"
  chown root:root "${MIERU_LINKS_FILE}"
  chmod 600 "${MIERU_LINKS_FILE}"
}

mieru_write_apply_config() {
  local tmp_config
  tmp_config="$(mktemp "${MIERU_MANAGED_DIR}/mita_config.XXXXXX.json")"

  jq '{
    portBindings: [.[] | {port: .port, protocol: .protocol}],
    users: [.[] | {name: .username, password: .password}],
    loggingLevel: "INFO",
    mtu: 1400
  }' "${MIERU_NODES_FILE}" > "${tmp_config}"

  mv "${tmp_config}" "${MIERU_CONFIG_FILE}"
  chmod 600 "${MIERU_CONFIG_FILE}"

  mita apply config "${MIERU_CONFIG_FILE}"
}

add_mieru_node() {
  require_mita
  init_mieru_state

  local public_host uri_host protocol proto_lower port username password tag link client_json node_json nodes_backup
  local host_fields

  echo
  echo "Add mieru/mita node"
  public_host="$(prompt_public_host "Public server domain/IP used by clients")"
  uri_host="$(format_uri_host "${public_host}")"
  protocol="$(prompt_mieru_protocol)"
  proto_lower="$(printf '%s' "${protocol}" | tr '[:upper:]' '[:lower:]')"
  port="$(prompt_mieru_port)"

  check_port_free "${port}" "${proto_lower}"

  if jq -e --argjson p "${port}" --arg proto "${protocol}" '.[]? | select(.port == $p and .protocol == $proto)' "${MIERU_NODES_FILE}" >/dev/null; then
    die "mieru node already exists for ${protocol} port ${port}"
  fi

  username="u_$(random_hex 4)"
  password="$(random_b64url 24)"
  tag="mieru-${proto_lower}-${port}-$(random_hex 3)"

  link="mierus://$(urlencode "${username}"):$(urlencode "${password}")@${uri_host}?profile=default&mtu=1400&multiplexing=MULTIPLEXING_HIGH&handshake-mode=HANDSHAKE_STANDARD&port=${port}&protocol=${protocol}"

  host_fields="$(mieru_host_client_fields "${public_host}")"
  client_json="$(jq -n \
    --arg profile "default" \
    --arg username "${username}" \
    --arg password "${password}" \
    --argjson host "${host_fields}" \
    --argjson port "${port}" \
    --arg protocol "${protocol}" \
    '{
      profiles: [
        {
          profileName: $profile,
          user: {name: $username, password: $password},
          servers: [
            ($host + {portBindings: [{port: $port, protocol: $protocol}]})
          ],
          mtu: 1400,
          multiplexing: {level: "MULTIPLEXING_HIGH"},
          handshakeMode: "HANDSHAKE_STANDARD"
        }
      ],
      activeProfile: $profile,
      rpcPort: 8964,
      socks5Port: 1080,
      loggingLevel: "INFO",
      socks5ListenLAN: false
    }')"

  node_json="$(jq -n \
    --arg type "mieru" --arg tag "${tag}" --arg host "${public_host}" \
    --argjson port "${port}" --arg protocol "${protocol}" \
    --arg username "${username}" --arg password "${password}" \
    --arg link "${link}" --argjson client "${client_json}" \
    '{type:$type,tag:$tag,host:$host,port:$port,protocol:$protocol,username:$username,password:$password,link:$link,client:$client,created_at:(now|todateiso8601)}')"

  nodes_backup="$(mktemp "${MIERU_MANAGED_DIR}/nodes.$(date +%Y%m%d%H%M%S).XXXXXX.bak")"
  cp -a "${MIERU_NODES_FILE}" "${nodes_backup}"

  local mita_config_backup=""
  if [[ -f "${MIERU_CONFIG_FILE}" ]]; then
    mita_config_backup="$(mktemp "${MIERU_MANAGED_DIR}/mita_config.$(date +%Y%m%d%H%M%S).XXXXXX.bak")"
    cp -a "${MIERU_CONFIG_FILE}" "${mita_config_backup}"
  fi

  jq --argjson node "${node_json}" '. += [$node]' "${MIERU_NODES_FILE}" > "${MIERU_NODES_FILE}.tmp"
  mv "${MIERU_NODES_FILE}.tmp" "${MIERU_NODES_FILE}"
  chmod 600 "${MIERU_NODES_FILE}"

  if ! mieru_write_apply_config; then
    warn "mita apply config failed; rolling back mieru node metadata and managed config file"
    cp -a "${nodes_backup}" "${MIERU_NODES_FILE}"
    if [[ -n "${mita_config_backup}" && -f "${mita_config_backup}" ]]; then
      cp -a "${mita_config_backup}" "${MIERU_CONFIG_FILE}"
    else
      rm -f "${MIERU_CONFIG_FILE}"
    fi
    return 1
  fi

  if ! mieru_rebuild_links_file; then
    warn "mieru links cache rebuild failed; nodes.json remains the source of truth"
  fi

  mita stop >/dev/null 2>&1 || true
  mita start

  echo
  echo "Sensitive output. Do not paste this into public logs."
  echo
  echo "mieru simple URI:"
  echo "${link}"
  echo
  echo "Note: mierus:// simple links may not contain every local client setting. For mieru clients, prefer the JSON below for a complete starting config."
  echo
  echo "mieru client JSON:"
  jq . <<< "${client_json}"

  firewall_hint "${port}" "${proto_lower}"
}

list_mieru_nodes_safe() {
  init_mieru_state
  echo
  if [[ ! -s "${MIERU_NODES_FILE}" ]] || [[ "$(jq 'length' "${MIERU_NODES_FILE}")" -eq 0 ]]; then
    echo "No mieru nodes."
    return
  fi
  jq -r 'to_entries[] | "\(.key+1). [mieru] \(.value.tag) \(.value.protocol)/\(.value.port) host=\(.value.host)"' "${MIERU_NODES_FILE}"
}

list_mieru_links() {
  init_mieru_state
  echo
  echo "Sensitive output: mieru links contain credentials."
  if [[ ! -s "${MIERU_NODES_FILE}" ]] || [[ "$(jq 'length' "${MIERU_NODES_FILE}")" -eq 0 ]]; then
    echo "No mieru nodes."
    return
  fi
  jq -r 'to_entries[] | "\(.key+1). [mieru] \(.value.tag)\n   \(.value.link)"' "${MIERU_NODES_FILE}"
}

mita_start() {
  require_mita
  mita start
  mita status || true
  systemctl status mita --no-pager -l || true
}

mita_stop() {
  require_mita
  mita stop || true
  mita status || true
}

mita_status() {
  require_mita
  systemctl status mita --no-pager -l || true
  mita status || true
  mita describe config || true
}

uninstall_mita() {
  require_systemd

  echo
  warn "This will remove the mita/mieru server package, service, and managed state."
  warn "Target managed directory: ${MIERU_MANAGED_DIR}"
  warn "This does not remove sing-box, xray, v2ray, nginx, caddy, firewall rules, or cloud security-group settings."
  confirm "Uninstall mita / mieru managed state" || return

  local backup_dir=""
  if [[ -d "${MIERU_MANAGED_DIR}" ]]; then
    backup_dir="$(mktemp -d "/root/mieru-managed-backup.$(date +%Y%m%d%H%M%S).XXXXXX")"
    cp -a "${MIERU_MANAGED_DIR}" "${backup_dir}/"
    chmod 700 "${backup_dir}"
    info "Backed up ${MIERU_MANAGED_DIR} to ${backup_dir}"
  fi

  if command -v mita >/dev/null 2>&1; then
    mita stop >/dev/null 2>&1 || true
  fi
  systemctl stop mita.service 2>/dev/null || true
  systemctl disable mita.service 2>/dev/null || true
  systemctl reset-failed mita.service 2>/dev/null || true

  if command -v dpkg >/dev/null 2>&1 && dpkg -s mita >/dev/null 2>&1; then
    DEBIAN_FRONTEND=noninteractive apt-get purge -y mita || warn "mita package may not have been removed cleanly."
    DEBIAN_FRONTEND=noninteractive apt-get autoremove -y || true
  elif command -v rpm >/dev/null 2>&1 && rpm -q mita >/dev/null 2>&1; then
    if command -v dnf >/dev/null 2>&1; then
      dnf remove -y mita || warn "mita package may not have been removed cleanly."
    elif command -v yum >/dev/null 2>&1; then
      yum remove -y mita || warn "mita package may not have been removed cleanly."
    elif command -v zypper >/dev/null 2>&1; then
      zypper --non-interactive remove mita || warn "mita package may not have been removed cleanly."
    else
      rpm -e mita || warn "mita package may not have been removed cleanly."
    fi
  else
    warn "No installed mita package detected by dpkg/rpm; removing common service files and managed state only."
  fi

  rm -f /etc/systemd/system/mita.service /lib/systemd/system/mita.service /usr/lib/systemd/system/mita.service
  rm -rf -- "${MIERU_MANAGED_DIR}"

  systemctl daemon-reload
  systemctl reset-failed 2>/dev/null || true

  echo "mita / mieru uninstall completed."
}

uninstall_singbox() {
  require_systemd
  warn "This stops the service and removes ${INSTALL_ROOT}, ${SERVICE_FILE}."
  warn "Config, certs, node links and secrets are kept at ${CONFIG_DIR}."
  confirm "Continue uninstall" || return
  systemctl stop sing-box.service 2>/dev/null || true
  systemctl disable sing-box.service 2>/dev/null || true
  rm -f "${SERVICE_FILE}" "${BIN_LINK}"
  rm -rf "${INSTALL_ROOT}"
  systemctl daemon-reload
  echo "Uninstalled binary and service. Config kept at ${CONFIG_DIR}."
}

purge_all() {
  require_systemd
  warn "DANGEROUS: this removes service, binary, config, certs, node links and backups."
  warn "Target paths: ${INSTALL_ROOT}, ${CONFIG_DIR}, ${SERVICE_FILE}, ${BIN_LINK}"
  confirm "Permanently purge all sing-box files managed by this script" || return
  systemctl stop sing-box.service 2>/dev/null || true
  systemctl disable sing-box.service 2>/dev/null || true
  rm -f "${SERVICE_FILE}" "${BIN_LINK}"
  rm -rf "${INSTALL_ROOT}" "${CONFIG_DIR}"
  systemctl daemon-reload
  echo "Purged managed files."
}

menu() {
  while true; do
    cat <<EOF

================ sing-box hardened deploy v15 ================
1) Install / update sing-box binary and systemd service
2) Add VLESS + Reality node
3) Add Trojan + TLS node
4) Add Hysteria2 + TLS node
5) Add TUIC + TLS node
6) Add Shadowsocks node
7) List nodes, safe view
8) List node links, sensitive
9) Delete node
10) Check config
11) Show full config JSON, sensitive
12) Start service
13) Stop service
14) Restart service
15) View logs
16) Uninstall binary and service, keep config
17) Purge all managed files, dangerous
18) Install / update mita server for mieru
19) Add mieru node, independent mita service
20) List mieru nodes, safe view
21) List mieru links, sensitive
22) Start mita
23) Stop mita
24) Show mita status/config
25) Audit 233boy/sing-box/xray/v2ray cleanup candidates
26) Clean uninstall proxy candidates, then optionally nginx/caddy
27) Audit optional nginx cleanup candidates
28) Clean uninstall nginx, optional explicit
29) Audit optional caddy cleanup candidates
30) Clean uninstall caddy, optional explicit
31) Uninstall mita / mieru managed state
0) Exit
EOF
    local choice
    read -r -p "Select: " choice
    case "${choice}" in
      1) install_all ;;
      2) add_vless_reality ;;
      3) add_trojan_tls ;;
      4) add_hysteria2_tls ;;
      5) add_tuic_tls ;;
      6) add_shadowsocks ;;
      7) list_nodes_safe ;;
      8) list_nodes ;;
      9) delete_node ;;
      10) check_config ;;
      11) show_config ;;
      12) service_start ;;
      13) service_stop ;;
      14) service_restart ;;
      15) view_logs ;;
      16) uninstall_singbox ;;
      17) purge_all ;;
      18) install_mita ;;
      19) add_mieru_node ;;
      20) list_mieru_nodes_safe ;;
      21) list_mieru_links ;;
      22) mita_start ;;
      23) mita_stop ;;
      24) mita_status ;;
      25) proxy_audit_cleanup_candidates ;;
      26) clean_uninstall_proxy_stack ;;
      27) audit_web_server_cleanup_candidates nginx ;;
      28) clean_uninstall_web_server nginx ;;
      29) audit_web_server_cleanup_candidates caddy ;;
      30) clean_uninstall_web_server caddy ;;
      31) uninstall_mita ;;
      0) exit 0 ;;
      *) echo "Invalid option." ;;
    esac
  done
}

main() {
  parse_args "$@"
  require_root
  menu
}

main "$@"
