#!/usr/bin/env bash
# Security Monitor Plugin for Lukan
# Monitors: SSH auth, network, file integrity, processes, sudo, Docker security.
# Only emits events for anomalies — normal activity is silently ignored.
# Protocol: JSON lines over stdin/stdout (lukan plugin protocol v1)
set -euo pipefail

###############################################################################
# Helpers
###############################################################################
send_json() { printf '%s\n' "$1"; }

send_log() {
  local level="$1" message="$2"
  send_json "{\"type\":\"log\",\"level\":\"${level}\",\"message\":$(jq -Rn --arg m "$message" '$m')}"
}

send_event() {
  local level="$1" detail="$2"
  send_json "{\"type\":\"systemEvent\",\"source\":\"security-monitor\",\"level\":\"${level}\",\"detail\":$(jq -Rn --arg d "$detail" '$d')}"
}

send_error() {
  local message="$1" recoverable="${2:-true}"
  send_json "{\"type\":\"error\",\"message\":$(jq -Rn --arg m "$message" '$m'),\"recoverable\":${recoverable}}"
}

###############################################################################
# Read Init message
###############################################################################
read -r init_line
init_type=$(echo "$init_line" | jq -r '.type // empty' 2>/dev/null || true)

if [[ "$init_type" != "init" ]]; then
  send_error "Expected init message, got: ${init_type:-empty}" false
  exit 1
fi

# Parse config
cfg_val() {
  echo "$init_line" | jq -r ".config.${1} // empty" 2>/dev/null || true
}
cfg_arr() {
  local raw
  raw=$(echo "$init_line" | jq -r ".config.${1} // empty" 2>/dev/null || true)
  if [[ -n "$raw" && "$raw" != "null" ]]; then
    echo "$raw" | jq -r '.[]' 2>/dev/null || true
  fi
}

WATCH_SSH=$(cfg_val watch_ssh)
WATCH_FIREWALL=$(cfg_val watch_firewall)
WATCH_FILES=$(cfg_val watch_files)
WATCH_PROCESSES=$(cfg_val watch_processes)
WATCH_SUDO=$(cfg_val watch_sudo)
WATCH_DOCKER=$(cfg_val watch_docker)
SSH_FAIL_THRESHOLD=$(cfg_val ssh_fail_threshold)
SCAN_INTERVAL=$(cfg_val scan_interval)

# Defaults — all modules on, threshold 5, interval 30s
[[ -z "$WATCH_SSH" || "$WATCH_SSH" == "null" ]] && WATCH_SSH=true
[[ -z "$WATCH_FIREWALL" || "$WATCH_FIREWALL" == "null" ]] && WATCH_FIREWALL=true
[[ -z "$WATCH_FILES" || "$WATCH_FILES" == "null" ]] && WATCH_FILES=true
[[ -z "$WATCH_PROCESSES" || "$WATCH_PROCESSES" == "null" ]] && WATCH_PROCESSES=true
[[ -z "$WATCH_SUDO" || "$WATCH_SUDO" == "null" ]] && WATCH_SUDO=true
[[ -z "$WATCH_DOCKER" || "$WATCH_DOCKER" == "null" ]] && WATCH_DOCKER=true
[[ -z "$SSH_FAIL_THRESHOLD" || "$SSH_FAIL_THRESHOLD" == "null" ]] && SSH_FAIL_THRESHOLD=5
[[ -z "$SCAN_INTERVAL" || "$SCAN_INTERVAL" == "null" ]] && SCAN_INTERVAL=30

# Arrays
TRUSTED_USERS=()
while IFS= read -r u; do [[ -n "$u" ]] && TRUSTED_USERS+=("$u"); done < <(cfg_arr trusted_users)

TRUSTED_IPS=()
while IFS= read -r ip; do [[ -n "$ip" ]] && TRUSTED_IPS+=("$ip"); done < <(cfg_arr trusted_ips)

ALLOWED_PORTS=()
while IFS= read -r p; do [[ -n "$p" ]] && ALLOWED_PORTS+=("$p"); done < <(cfg_arr allowed_ports)

###############################################################################
# Self-awareness: build lukan PID set to exclude our own activity
###############################################################################
LUKAN_ROOT_PID=$$

# Walk up to find the top-level lukan process
_pid=$LUKAN_ROOT_PID
while [[ "$_pid" -gt 1 ]]; do
  _ppid=$(ps -o ppid= -p "$_pid" 2>/dev/null | tr -d ' ' || echo 1)
  _name=$(ps -o comm= -p "$_ppid" 2>/dev/null | tr -d ' ' || echo "")
  if [[ "$_name" == "lukan" || "$_name" == "lukan-tui" ]]; then
    LUKAN_ROOT_PID=$_ppid
  fi
  _pid=$_ppid
done

# Rebuild the set of lukan-owned PIDs (refreshed periodically)
declare -A LUKAN_PIDS
refresh_lukan_pids() {
  LUKAN_PIDS=()
  LUKAN_PIDS[$LUKAN_ROOT_PID]=1
  # All descendants of lukan root
  while IFS= read -r pid; do
    [[ -n "$pid" ]] && LUKAN_PIDS[$pid]=1
  done < <(pgrep -P "$LUKAN_ROOT_PID" --ns $$ 2>/dev/null || true;
           # Also get grandchildren recursively
           pgrep -P "$LUKAN_ROOT_PID" --ns $$ 2>/dev/null | while read -r cpid; do
             pgrep -P "$cpid" --ns $$ 2>/dev/null || true
           done)
  # Also include any process with LUKAN_AGENT=1 env var
  while IFS= read -r pid; do
    pid=$(echo "$pid" | tr -d ' ')
    [[ -n "$pid" ]] && LUKAN_PIDS[$pid]=1
  done < <(grep -rl 'LUKAN_AGENT=1' /proc/*/environ 2>/dev/null | grep -oP '/proc/\K[0-9]+' || true)
}

is_lukan_pid() {
  [[ -n "${LUKAN_PIDS[$1]+x}" ]] && return 0
  # Fallback: check env var on the process
  if grep -q 'LUKAN_AGENT=1' "/proc/$1/environ" 2>/dev/null; then
    LUKAN_PIDS[$1]=1
    return 0
  fi
  return 1
}

is_trusted_user() {
  local user="$1"
  for tu in "${TRUSTED_USERS[@]+"${TRUSTED_USERS[@]}"}"; do
    [[ "$tu" == "$user" ]] && return 0
  done
  return 1
}

is_trusted_ip() {
  local ip="$1"
  for tip in "${TRUSTED_IPS[@]+"${TRUSTED_IPS[@]}"}"; do
    [[ "$tip" == "$ip" ]] && return 0
  done
  return 1
}

is_allowed_port() {
  local port="$1"
  for ap in "${ALLOWED_PORTS[@]+"${ALLOWED_PORTS[@]}"}"; do
    [[ "$ap" == "$port" ]] && return 0
  done
  return 1
}

# Processes that are known-safe to open network ports
SAFE_LISTENER_PROCS=(
  sshd systemd-resolve rpcbind cupsd cups-browsed  # system services
  code node electron chrome chromium firefox        # dev tools & browsers
  docker dockerd containerd kubelet                 # containers
  postgres mysqld mariadbd redis-server mongod      # databases
  nginx apache2 httpd caddy                         # web servers
  ollama                                            # local AI
  dnsmasq avahi-daemon NetworkManager               # network services
  pulseaudio pipewire wireplumber                   # audio
  Xwayland Xorg                                    # display
)

# Processes that should never trigger suspicious-pattern alerts
# (desktop apps whose complex cmdlines can false-positive on shell patterns)
SAFE_PROCESS_NAMES=(
  code node electron                                # VS Code / Electron apps
  chrome chromium chrome_crashpad firefox            # browsers
  gnome-shell gnome-terminal nautilus               # desktop environment
  Xwayland Xorg                                    # display servers
  pulseaudio pipewire wireplumber                   # audio
  docker dockerd containerd                         # containers
  systemd journald                                  # system daemons
  snapd snap-store                                  # snap
  flatpak                                           # flatpak
)

is_safe_listener() {
  local proc_name="$1"
  for safe in "${SAFE_LISTENER_PROCS[@]}"; do
    [[ "$proc_name" == "$safe" ]] && return 0
  done
  return 1
}

is_safe_process() {
  local proc_name="$1"
  for safe in "${SAFE_PROCESS_NAMES[@]}"; do
    [[ "$proc_name" == "$safe" ]] && return 0
  done
  return 1
}

###############################################################################
# Ready
###############################################################################
send_json '{"type":"ready","version":"0.1.0","capabilities":["systemEvent"]}'

modules_active=()
[[ "$WATCH_SSH" == "true" ]] && modules_active+=("ssh")
[[ "$WATCH_FIREWALL" == "true" ]] && modules_active+=("firewall")
[[ "$WATCH_FILES" == "true" ]] && modules_active+=("files")
[[ "$WATCH_PROCESSES" == "true" ]] && modules_active+=("processes")
[[ "$WATCH_SUDO" == "true" ]] && modules_active+=("sudo")
[[ "$WATCH_DOCKER" == "true" ]] && modules_active+=("docker")
send_log "info" "Security monitor started — modules: ${modules_active[*]:-none}"

###############################################################################
# Shutdown handling
###############################################################################
CHILD_PIDS=()
cleanup() {
  for pid in "${CHILD_PIDS[@]+"${CHILD_PIDS[@]}"}"; do
    kill "$pid" 2>/dev/null || true
  done
  [[ -n "${STDIN_PID:-}" ]] && kill "$STDIN_PID" 2>/dev/null || true
  exit 0
}
trap cleanup SIGTERM SIGINT SIGHUP

# Read stdin in background to detect shutdown messages
(
  while IFS= read -r line; do
    msg_type=$(echo "$line" | jq -r '.type // empty' 2>/dev/null || true)
    if [[ "$msg_type" == "shutdown" ]]; then
      kill $$ 2>/dev/null || true
      break
    fi
  done
) &
STDIN_PID=$!

###############################################################################
# MODULE 1: SSH / Authentication monitoring
###############################################################################
monitor_ssh() {
  send_log "info" "SSH module: watching authentication logs"

  # Determine log source — prefer file-based logs (more reliable across sandboxes)
  local log_source="none"
  if [[ -f /var/log/auth.log ]]; then
    log_source="auth.log"
  elif [[ -f /var/log/secure ]]; then
    log_source="secure"
  elif command -v journalctl &>/dev/null && journalctl --no-pager -n 1 -t sshd &>/dev/null 2>&1; then
    log_source="journal"
  fi

  # Track failed attempts per IP: ip -> count
  declare -A fail_counts
  # Track alerted IPs (don't repeat alerts)
  declare -A alerted_ips

  process_auth_line() {
    local line="$1"

    # ── Failed password ──
    if echo "$line" | grep -qiE 'failed password|authentication failure'; then
      local ip user
      ip=$(echo "$line" | grep -oP 'from \K[0-9a-f.:]+' | head -1 || true)
      user=$(echo "$line" | grep -oP 'for (invalid user )?\K\S+' | head -1 || true)
      [[ -z "$ip" ]] && return

      # Ignore trusted IPs
      is_trusted_ip "$ip" && return

      fail_counts[$ip]=$(( ${fail_counts[$ip]:-0} + 1 ))

      if [[ ${fail_counts[$ip]} -ge $SSH_FAIL_THRESHOLD && -z "${alerted_ips[$ip]+x}" ]]; then
        alerted_ips[$ip]=1
        send_event "error" "Brute force detected: ${fail_counts[$ip]} failed SSH attempts from ${ip} (last user: ${user:-unknown})"
      fi
      return
    fi

    # ── Accepted login ──
    if echo "$line" | grep -qi 'accepted'; then
      local ip user method
      user=$(echo "$line" | grep -oP 'for \K\S+' | head -1 || true)
      ip=$(echo "$line" | grep -oP 'from \K[0-9a-f.:]+' | head -1 || true)
      method=$(echo "$line" | grep -oP '^Accepted \K\S+' || true)
      [[ -z "$user" ]] && return

      # Root login is always suspicious
      if [[ "$user" == "root" ]]; then
        send_event "error" "Root SSH login detected from ${ip:-unknown} via ${method:-unknown}"
        return
      fi

      # Trusted user from trusted IP → silent
      if is_trusted_user "$user" && is_trusted_ip "$ip"; then
        return
      fi

      # Known user from unknown IP → warn
      if is_trusted_user "$user" && ! is_trusted_ip "$ip"; then
        send_event "warn" "SSH login by trusted user '${user}' from unknown IP ${ip:-unknown}"
        return
      fi

      # Unknown user → warn
      if ! is_trusted_user "$user"; then
        send_event "warn" "SSH login by unexpected user '${user}' from ${ip:-unknown}"
        return
      fi
    fi

    # ── New user created ──
    if echo "$line" | grep -qiE 'new user|useradd'; then
      local user
      user=$(echo "$line" | grep -oP "name=\K\S+" | tr -d "'" || true)
      [[ -z "$user" ]] && user=$(echo "$line" | grep -oP "new user: name=\K\w+" || true)
      send_event "error" "New system user created: ${user:-unknown}"
    fi

    # ── Password changed ──
    if echo "$line" | grep -qiE 'password changed|passwd.*changed'; then
      local user
      user=$(echo "$line" | grep -oP 'for (user )?\K\S+' | head -1 || true)
      send_event "warn" "Password changed for user: ${user:-unknown}"
    fi
  }

  case "$log_source" in
    auth.log)
      send_log "info" "SSH module: tailing /var/log/auth.log"
      tail -F -n 0 /var/log/auth.log 2>/dev/null | while IFS= read -r line; do
        process_auth_line "$line"
      done
      ;;
    secure)
      send_log "info" "SSH module: tailing /var/log/secure"
      tail -F -n 0 /var/log/secure 2>/dev/null | while IFS= read -r line; do
        process_auth_line "$line"
      done
      ;;
    journal)
      send_log "info" "SSH module: following journalctl -t sshd"
      journalctl -f -n 0 -t sshd -t systemd-logind --no-pager 2>/dev/null | while IFS= read -r line; do
        process_auth_line "$line"
      done
      ;;
    *)
      send_log "warn" "SSH module: no auth log source found"
      ;;
  esac
}

###############################################################################
# MODULE 2: Firewall / Network monitoring (periodic)
###############################################################################
monitor_firewall() {
  send_log "info" "Firewall module: scanning listening ports every ${SCAN_INTERVAL}s"

  # Baseline: capture current listening ports
  declare -A known_ports
  while IFS= read -r line; do
    local port proto pid_prog
    port=$(echo "$line" | awk '{print $4}' | grep -oP ':\K[0-9]+$' || true)
    proto=$(echo "$line" | awk '{print $1}' || true)
    pid_prog=$(echo "$line" | awk '{print $NF}' || true)
    [[ -n "$port" ]] && known_ports["${proto}:${port}"]="$pid_prog"
  done < <(ss -tlnp 2>/dev/null | tail -n +2; ss -ulnp 2>/dev/null | tail -n +2)

  send_log "info" "Firewall baseline: ${#known_ports[@]} listening ports"

  while true; do
    sleep "$SCAN_INTERVAL"
    refresh_lukan_pids

    # Check for new listening ports
    while IFS= read -r line; do
      local port proto pid_prog pid
      port=$(echo "$line" | awk '{print $4}' | grep -oP ':\K[0-9]+$' || true)
      proto=$(echo "$line" | awk '{print $1}' || true)
      pid_prog=$(echo "$line" | awk '{print $NF}' || true)
      [[ -z "$port" ]] && continue

      local key="${proto}:${port}"
      if [[ -z "${known_ports[$key]+x}" ]]; then
        # New port detected
        pid=$(echo "$pid_prog" | grep -oP 'pid=\K[0-9]+' || true)
        local proc_name=""
        [[ -n "$pid" ]] && proc_name=$(ps -o comm= -p "$pid" 2>/dev/null | tr -d ' ' || true)

        # Skip lukan's own processes
        if [[ -n "$pid" ]] && is_lukan_pid "$pid"; then
          known_ports[$key]="$pid_prog"
          continue
        fi

        # Skip known-safe processes (VS Code, Chrome, databases, etc.)
        if [[ -n "$proc_name" ]] && is_safe_listener "$proc_name"; then
          known_ports[$key]="$pid_prog"
          continue
        fi

        # Skip explicitly allowed ports
        if is_allowed_port "$port"; then
          known_ports[$key]="$pid_prog"
          continue
        fi

        # Unknown process on unknown port → suspicious
        known_ports[$key]="$pid_prog"
        send_event "warn" "New listening port detected: ${proto}/${port} — process: ${proc_name:-unknown} (${pid_prog})"
      fi
    done < <(ss -tlnp 2>/dev/null | tail -n +2; ss -ulnp 2>/dev/null | tail -n +2)

    # Check for port scan patterns: many connections from same IP to different ports
    # (Look at recent conntrack or ss state entries)
    if command -v conntrack &>/dev/null; then
      declare -A scan_ips
      while IFS= read -r line; do
        local src dst_port
        src=$(echo "$line" | grep -oP 'src=\K[0-9.]+' | head -1 || true)
        dst_port=$(echo "$line" | grep -oP 'dport=\K[0-9]+' | head -1 || true)
        [[ -z "$src" || -z "$dst_port" ]] && continue
        is_trusted_ip "$src" && continue
        # Count unique destination ports per source IP
        scan_ips["$src"]=$(( ${scan_ips[$src]:-0} + 1 ))
      done < <(conntrack -L 2>/dev/null | grep -E 'SYN_RECV|SYN_SENT' | head -200 || true)

      for src_ip in "${!scan_ips[@]}"; do
        if [[ ${scan_ips[$src_ip]} -ge 20 ]]; then
          send_event "error" "Possible port scan from ${src_ip}: ${scan_ips[$src_ip]} connection attempts"
        fi
      done
      unset scan_ips
    fi
  done
}

###############################################################################
# MODULE 3: File integrity monitoring (periodic)
###############################################################################
monitor_files() {
  send_log "info" "File integrity module: monitoring critical system files"

  # Critical files to monitor
  local -a watch_files=(
    /etc/passwd
    /etc/shadow
    /etc/sudoers
    /etc/ssh/sshd_config
    /etc/hosts
    /etc/resolv.conf
    /etc/crontab
    /etc/pam.d/sshd
  )

  # Also watch /etc/sudoers.d/* and /etc/cron.d/* if they exist
  for f in /etc/sudoers.d/* /etc/cron.d/*; do
    [[ -f "$f" ]] && watch_files+=("$f")
  done

  # Baseline checksums
  declare -A file_checksums
  for f in "${watch_files[@]}"; do
    if [[ -f "$f" ]]; then
      file_checksums[$f]=$(sha256sum "$f" 2>/dev/null | awk '{print $1}' || true)
    fi
  done

  send_log "info" "File integrity baseline: ${#file_checksums[@]} files tracked"

  # Try inotifywait first (realtime), fall back to polling
  if command -v inotifywait &>/dev/null; then
    # Build args for existing files
    local -a inotify_args=(-m -e modify,create,delete,move --format '%w%f %e')
    local -a existing_files=()
    for f in "${watch_files[@]}"; do
      [[ -e "$f" ]] && existing_files+=("$f")
    done
    # Also watch directories for new files
    for d in /etc/sudoers.d /etc/cron.d /etc/systemd/system; do
      [[ -d "$d" ]] && existing_files+=("$d")
    done

    if [[ ${#existing_files[@]} -gt 0 ]]; then
      inotifywait "${inotify_args[@]}" "${existing_files[@]}" 2>/dev/null | while IFS= read -r event; do
        local filepath event_type
        filepath=$(echo "$event" | awk '{print $1}')
        event_type=$(echo "$event" | awk '{print $2}')

        # Verify it actually changed (not just metadata)
        if [[ -f "$filepath" ]]; then
          local new_sum
          new_sum=$(sha256sum "$filepath" 2>/dev/null | awk '{print $1}' || true)
          local old_sum="${file_checksums[$filepath]:-}"
          if [[ "$new_sum" != "$old_sum" && -n "$new_sum" ]]; then
            file_checksums[$filepath]="$new_sum"
            send_event "critical" "Critical file modified: ${filepath} (${event_type})"
          fi
        elif [[ "$event_type" == *"DELETE"* ]]; then
          send_event "critical" "Critical file DELETED: ${filepath}"
        elif [[ "$event_type" == *"CREATE"* ]]; then
          send_event "warn" "New file created in monitored directory: ${filepath}"
          if [[ -f "$filepath" ]]; then
            file_checksums[$filepath]=$(sha256sum "$filepath" 2>/dev/null | awk '{print $1}' || true)
          fi
        fi
      done
    else
      send_log "warn" "File integrity: no watchable files found"
    fi
  else
    # Fallback: polling
    send_log "info" "File integrity: inotifywait not found, using polling (every ${SCAN_INTERVAL}s)"
    while true; do
      sleep "$SCAN_INTERVAL"
      for f in "${watch_files[@]}"; do
        if [[ -f "$f" ]]; then
          local new_sum
          new_sum=$(sha256sum "$f" 2>/dev/null | awk '{print $1}' || true)
          local old_sum="${file_checksums[$f]:-MISSING}"
          if [[ "$old_sum" == "MISSING" ]]; then
            # Newly appeared file
            file_checksums[$f]="$new_sum"
          elif [[ "$new_sum" != "$old_sum" ]]; then
            file_checksums[$f]="$new_sum"
            send_event "critical" "Critical file modified: ${f}"
          fi
        else
          if [[ -n "${file_checksums[$f]+x}" ]]; then
            unset "file_checksums[$f]"
            send_event "critical" "Critical file DELETED: ${f}"
          fi
        fi
      done
      # Check for new crontab/sudoers entries
      for d in /etc/sudoers.d /etc/cron.d; do
        if [[ -d "$d" ]]; then
          for f in "$d"/*; do
            [[ -f "$f" ]] || continue
            if [[ -z "${file_checksums[$f]+x}" ]]; then
              file_checksums[$f]=$(sha256sum "$f" 2>/dev/null | awk '{print $1}' || true)
              send_event "warn" "New file in ${d}: ${f}"
            fi
          done
        fi
      done
    done
  fi
}

###############################################################################
# MODULE 4: Suspicious process detection (periodic)
###############################################################################
monitor_processes() {
  send_log "info" "Process module: scanning every ${SCAN_INTERVAL}s"

  # Patterns that indicate reverse shells or suspicious activity
  # Use \b word boundaries to avoid matching substrings in long cmdlines
  local -a suspicious_patterns=(
    '\bnc\s+-\w*l'        # netcat listener (nc -lp, nc -lvp, etc.)
    '\bncat\s+-\w*l'      # ncat listener
    '\bsocat\b.*TCP.*EXEC'  # socat reverse shell
    '\bbash\s+-i\b'       # interactive bash (common in reverse shells)
    '/dev/tcp/'           # bash /dev/tcp reverse shell
    '/dev/udp/'           # bash /dev/udp
    '\bpython[23]?\s.*\bsocket\b'  # python reverse shell
    '\bpython[23]?\s.*pty\.spawn'  # python pty spawn
    '\bperl\b.*\bsocket\b'  # perl reverse shell
    '\bruby\b.*\bTCPSocket\b'  # ruby reverse shell
    '\bphp\b.*\bfsockopen\b'  # php reverse shell
    '\bxmrig\b'           # crypto miner
    '\bminerd\b'          # crypto miner
    '\bcpuminer\b'        # crypto miner
    '\bstratum\+tcp\b'    # mining pool connection
    '\bkworkerds\b'       # common crypto miner disguise
    '\bkdevtmpfsi\b'      # common crypto miner
  )

  # Track already-alerted PIDs to avoid spam
  declare -A alerted_pids

  while true; do
    sleep "$SCAN_INTERVAL"
    refresh_lukan_pids

    # Scan all processes
    while IFS= read -r line; do
      local pid user cmd
      pid=$(echo "$line" | awk '{print $1}')
      user=$(echo "$line" | awk '{print $2}')
      cmd=$(echo "$line" | awk '{$1=$2=""; print $0}' | sed 's/^ *//')

      [[ -z "$pid" || -z "$cmd" ]] && continue
      [[ -n "${alerted_pids[$pid]+x}" ]] && continue

      # Skip lukan's own processes
      is_lukan_pid "$pid" && continue

      # Skip known-safe processes by their short name (comm)
      # This prevents false positives from desktop apps with complex cmdlines
      local proc_comm
      proc_comm=$(ps -o comm= -p "$pid" 2>/dev/null | tr -d ' ' || true)
      [[ -n "$proc_comm" ]] && is_safe_process "$proc_comm" && continue

      # Check against suspicious patterns
      for pattern in "${suspicious_patterns[@]}"; do
        if echo "$cmd" | grep -qiE "$pattern"; then
          alerted_pids[$pid]=1
          send_event "critical" "Suspicious process detected (PID ${pid}, user ${user}): ${cmd}"
          break
        fi
      done
    done < <(ps -eo pid,user,args --no-headers 2>/dev/null || true)

    # Check for root processes actually listening on a network socket
    # Uses ss to verify the PID truly has a listening socket (avoids false positives)
    while IFS= read -r line; do
      local pid comm cmd
      pid=$(echo "$line" | awk '{print $1}')
      comm=$(echo "$line" | awk '{print $2}')
      cmd=$(echo "$line" | awk '{$1=$2=""; print $0}' | sed 's/^ *//')

      [[ -z "$pid" || -z "$comm" ]] && continue
      [[ -n "${alerted_pids[$pid]+x}" ]] && continue
      is_lukan_pid "$pid" && continue

      # Only alert on shell/interpreter binaries
      case "$comm" in
        bash|sh|dash|zsh|python|python3|perl|ruby|php|php-fpm)
          # Verify the process actually has a listening socket via ss
          if ss -ltnp 2>/dev/null | grep -q "pid=${pid},"; then
            alerted_pids[$pid]=1
            send_event "error" "Root process with network listener (PID ${pid}, ${comm}): ${cmd}"
          fi
          ;;
      esac
    done < <(ps -eo pid,comm,args --no-headers -U root 2>/dev/null || true)

    # Clean up alerted_pids for dead processes
    for pid in "${!alerted_pids[@]}"; do
      [[ -d "/proc/$pid" ]] || unset "alerted_pids[$pid]"
    done
  done
}

###############################################################################
# MODULE 5: Sudo / privilege escalation monitoring
###############################################################################
monitor_sudo() {
  send_log "info" "Sudo module: watching for privilege escalation"

  local use_journal=false
  if command -v journalctl &>/dev/null && journalctl --no-pager -n 1 -t sudo &>/dev/null 2>&1; then
    use_journal=true
  fi

  process_sudo_line() {
    local line="$1"

    # ── Sudo auth failure ──
    if echo "$line" | grep -qiE 'auth(entication)? failure.*sudo|sudo.*auth.*fail|sudo.*incorrect password'; then
      local user
      user=$(echo "$line" | grep -oP '(user=|USER=)\K\S+' || true)
      send_event "warn" "Sudo authentication failure by user: ${user:-unknown}"
      return
    fi

    # ── Sudo command executed ──
    if echo "$line" | grep -qE 'COMMAND='; then
      local user cmd run_as
      user=$(echo "$line" | grep -oP 'USER=\K\S+' || true)
      run_as=$(echo "$line" | grep -oP 'USER=\K\S+' || true)
      cmd=$(echo "$line" | grep -oP 'COMMAND=\K.*' || true)
      user=$(echo "$line" | grep -oP '^\S+ \S+ \S+ \K\S+' | sed 's/:.*//' || true)

      # Ignore trusted users' normal sudo
      is_trusted_user "$user" && return

      # Non-trusted user using sudo → always alert
      send_event "warn" "Sudo used by user '${user}': ${cmd:-unknown command}"
      return
    fi

    # ── su command ──
    if echo "$line" | grep -qiE 'su\[|su:.*session opened|su:.*to root'; then
      local from_user to_user
      from_user=$(echo "$line" | grep -oP '(by |for )\K\S+' | head -1 || true)
      to_user=$(echo "$line" | grep -oP 'to \K\S+' || true)

      if [[ "$to_user" == "root" ]]; then
        is_trusted_user "$from_user" && return
        send_event "error" "su to root by user: ${from_user:-unknown}"
      fi
      return
    fi

    # ── Sudo configuration changes ──
    if echo "$line" | grep -qiE 'visudo|sudoers'; then
      send_event "warn" "Sudoers configuration change detected: ${line}"
    fi
  }

  if [[ "$use_journal" == "true" ]]; then
    journalctl -f -n 0 -t sudo -t su --no-pager 2>/dev/null | while IFS= read -r line; do
      process_sudo_line "$line"
    done
  elif [[ -f /var/log/auth.log ]]; then
    tail -F -n 0 /var/log/auth.log 2>/dev/null | grep --line-buffered -iE 'sudo|su\[|su:' | while IFS= read -r line; do
      process_sudo_line "$line"
    done
  elif [[ -f /var/log/secure ]]; then
    tail -F -n 0 /var/log/secure 2>/dev/null | grep --line-buffered -iE 'sudo|su\[|su:' | while IFS= read -r line; do
      process_sudo_line "$line"
    done
  else
    send_log "warn" "Sudo module: no log source found"
  fi
}

###############################################################################
# MODULE 6: Docker security monitoring (periodic)
###############################################################################
monitor_docker() {
  if ! command -v docker &>/dev/null; then
    send_log "warn" "Docker module: docker not found, skipping"
    return
  fi
  if ! docker info &>/dev/null 2>&1; then
    send_log "warn" "Docker module: cannot connect to Docker daemon, skipping"
    return
  fi

  send_log "info" "Docker security module: scanning every ${SCAN_INTERVAL}s"

  # Track already-alerted containers
  declare -A alerted_containers

  while true; do
    sleep "$SCAN_INTERVAL"

    # Check for privileged containers
    while IFS= read -r container_id; do
      [[ -z "$container_id" ]] && continue
      local name privileged
      name=$(docker inspect --format '{{.Name}}' "$container_id" 2>/dev/null | sed 's/^\//' || true)
      [[ -n "${alerted_containers[priv:${name}]+x}" ]] && continue

      privileged=$(docker inspect --format '{{.HostConfig.Privileged}}' "$container_id" 2>/dev/null || true)
      if [[ "$privileged" == "true" ]]; then
        alerted_containers["priv:${name}"]=1
        local image
        image=$(docker inspect --format '{{.Config.Image}}' "$container_id" 2>/dev/null || true)
        send_event "warn" "Privileged container running: '${name}' (image: ${image:-unknown})"
      fi
    done < <(docker ps -q 2>/dev/null)

    # Check for dangerous host mounts
    while IFS= read -r container_id; do
      [[ -z "$container_id" ]] && continue
      local name
      name=$(docker inspect --format '{{.Name}}' "$container_id" 2>/dev/null | sed 's/^\//' || true)
      [[ -n "${alerted_containers[mount:${name}]+x}" ]] && continue

      local mounts
      mounts=$(docker inspect --format '{{range .Mounts}}{{.Source}}:{{.Destination}} {{end}}' "$container_id" 2>/dev/null || true)

      # Check for dangerous source mounts
      for mount in $mounts; do
        local src
        src=$(echo "$mount" | cut -d: -f1)
        case "$src" in
          /|/etc|/etc/*|/root|/var/run/docker.sock)
            alerted_containers["mount:${name}"]=1
            local image
            image=$(docker inspect --format '{{.Config.Image}}' "$container_id" 2>/dev/null || true)
            send_event "warn" "Container '${name}' has dangerous host mount: ${mount} (image: ${image:-unknown})"
            break
            ;;
        esac
      done
    done < <(docker ps -q 2>/dev/null)

    # Check for containers running as root with network access
    while IFS= read -r container_id; do
      [[ -z "$container_id" ]] && continue
      local name user network_mode
      name=$(docker inspect --format '{{.Name}}' "$container_id" 2>/dev/null | sed 's/^\//' || true)
      [[ -n "${alerted_containers[net:${name}]+x}" ]] && continue

      user=$(docker inspect --format '{{.Config.User}}' "$container_id" 2>/dev/null || true)
      network_mode=$(docker inspect --format '{{.HostConfig.NetworkMode}}' "$container_id" 2>/dev/null || true)

      # Container with host network and running as root (or no user set = root)
      if [[ "$network_mode" == "host" && ( -z "$user" || "$user" == "root" || "$user" == "0" ) ]]; then
        alerted_containers["net:${name}"]=1
        local image
        image=$(docker inspect --format '{{.Config.Image}}' "$container_id" 2>/dev/null || true)
        send_event "warn" "Container '${name}' running as root with host network (image: ${image:-unknown})"
      fi
    done < <(docker ps -q 2>/dev/null)
  done
}

###############################################################################
# Launch modules in background
###############################################################################
refresh_lukan_pids

if [[ "$WATCH_SSH" == "true" ]]; then
  monitor_ssh &
  CHILD_PIDS+=($!)
fi

if [[ "$WATCH_SUDO" == "true" ]]; then
  monitor_sudo &
  CHILD_PIDS+=($!)
fi

if [[ "$WATCH_FIREWALL" == "true" ]]; then
  monitor_firewall &
  CHILD_PIDS+=($!)
fi

if [[ "$WATCH_FILES" == "true" ]]; then
  monitor_files &
  CHILD_PIDS+=($!)
fi

if [[ "$WATCH_PROCESSES" == "true" ]]; then
  monitor_processes &
  CHILD_PIDS+=($!)
fi

if [[ "$WATCH_DOCKER" == "true" ]]; then
  monitor_docker &
  CHILD_PIDS+=($!)
fi

# Wait for all modules (they run forever until killed)
wait
