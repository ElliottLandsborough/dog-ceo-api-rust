#!/usr/bin/env bash
# Provision an AlmaLinux host for rootless podman deploys as the 'deploy' user.
# Runs as root; invoked from the Makefile as:  ssh <host> 'sudo bash -s' < this
# Idempotent — safe to re-run.
set -euo pipefail

DEPLOY_USER="${DEPLOY_USER:-deploy}"
OPEN_PORTS="${OPEN_PORTS:-80}"
SERVER_NAME="${SERVER_NAME:-_}"
# Defaults target production-style fanout.
# For test hosts, pass API_UPSTREAM_PORTS="10081 10082 10083 10084" and IMAGES_UPSTREAM_PORT="10080" via sudo env.
API_UPSTREAM_PORTS="${API_UPSTREAM_PORTS:-10081 10082 10083 10084}"
IMAGES_UPSTREAM_PORT="${IMAGES_UPSTREAM_PORT:-10080}"
EXTRA_DEPLOY_PUBKEY="${EXTRA_DEPLOY_PUBKEY:-ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAII//hN7PjreSkKnnYuDh2kzRKnCoooSGjJIxede1nuR8 elliott@Elliotts-MacBook-Air.local}"

echo "== [1/10] podman + nginx + rootless plumbing =="
dnf -y install podman nginx htop shadow-utils slirp4netns fuse-overlayfs

echo "== [2/10] user: $DEPLOY_USER =="
id "$DEPLOY_USER" &>/dev/null || useradd -m -s /bin/bash "$DEPLOY_USER"

echo "== [3/10] subuid/subgid ranges =="
# useradd normally allocates these; this is a fallback for pre-existing users.
grep -q "^$DEPLOY_USER:" /etc/subuid || usermod --add-subuids 200000-265535 "$DEPLOY_USER"
grep -q "^$DEPLOY_USER:" /etc/subgid || usermod --add-subgids 200000-265535 "$DEPLOY_USER"

echo "== [4/10] ssh key for $DEPLOY_USER =="
# Reuse the provisioning login's authorized_keys so the same key that
# provisions the box can deploy to it.
SRC_KEYS=""
if [ -n "${SUDO_USER:-}" ] && [ -f "/home/$SUDO_USER/.ssh/authorized_keys" ]; then
  SRC_KEYS="/home/$SUDO_USER/.ssh/authorized_keys"
elif [ -f /root/.ssh/authorized_keys ]; then
  SRC_KEYS=/root/.ssh/authorized_keys
fi
install -d -m 700 -o "$DEPLOY_USER" -g "$DEPLOY_USER" "/home/$DEPLOY_USER/.ssh"
AUTHORIZED_KEYS="/home/$DEPLOY_USER/.ssh/authorized_keys"

if [ -n "$SRC_KEYS" ]; then
  install -m 600 -o "$DEPLOY_USER" -g "$DEPLOY_USER" "$SRC_KEYS" "$AUTHORIZED_KEYS"
else
  touch "$AUTHORIZED_KEYS"
  chown "$DEPLOY_USER:$DEPLOY_USER" "$AUTHORIZED_KEYS"
  chmod 600 "$AUTHORIZED_KEYS"
  echo "WARN: no authorized_keys found to copy — creating empty one" >&2
fi

if [ -n "$EXTRA_DEPLOY_PUBKEY" ] && ! grep -Fxq "$EXTRA_DEPLOY_PUBKEY" "$AUTHORIZED_KEYS"; then
  printf '%s\n' "$EXTRA_DEPLOY_PUBKEY" >> "$AUTHORIZED_KEYS"
fi

echo "== [5/10] linger (containers survive ssh logout + start at boot) =="
loginctl enable-linger "$DEPLOY_USER"
DEPLOY_UID="$(id -u "$DEPLOY_USER")"
systemctl start "user@$DEPLOY_UID" 2>/dev/null || true

echo "== [6/10] allow rootless bind on low ports (e.g. :80) =="
cat >/etc/sysctl.d/90-rootless-low-ports.conf <<'EOF'
net.ipv4.ip_unprivileged_port_start=80
EOF
sysctl -p /etc/sysctl.d/90-rootless-low-ports.conf >/dev/null

echo "== [7/10] honour --restart policies across reboots (rootless) =="
sudo -iu "$DEPLOY_USER" \
  env XDG_RUNTIME_DIR="/run/user/$DEPLOY_UID" \
      DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$DEPLOY_UID/bus" \
  systemctl --user enable podman-restart.service

echo "== [8/10] nginx reverse proxy config =="
cat >/etc/nginx/conf.d/dog-ceo-api.conf <<EOF
upstream dog_api_upstream {
$(for p in $API_UPSTREAM_PORTS; do echo "    server 127.0.0.1:${p};"; done)
    keepalive 64;
}

upstream dog_images_upstream {
    server 127.0.0.1:${IMAGES_UPSTREAM_PORT};
    keepalive 32;
}

server {
    listen 80;
    listen [::]:80;
    server_name ${SERVER_NAME};

    access_log /var/log/nginx/dog-ceo-access.log;
    error_log /var/log/nginx/dog-ceo-error.log warn;

    client_max_body_size 2m;

    location = /api {
        return 301 /api/;
    }

    location /api/ {
        proxy_http_version 1.1;
        proxy_set_header Host \$host;
        proxy_set_header X-Real-IP \$remote_addr;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto \$scheme;
        proxy_set_header Connection "";
        proxy_pass http://dog_api_upstream/;
    }

    # Optional image passthrough on same host (no separate images subdomain needed).
    location /breeds/ {
        proxy_http_version 1.1;
        proxy_set_header Host \$host;
        proxy_set_header X-Real-IP \$remote_addr;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto \$scheme;
        proxy_set_header Connection "";
        proxy_pass http://dog_images_upstream;
    }

    location / {
        return 404;
    }
}
EOF

nginx -t
systemctl enable --now nginx
systemctl restart nginx

if command -v getenforce >/dev/null 2>&1 && command -v setsebool >/dev/null 2>&1; then
  if [ "$(getenforce)" != "Disabled" ]; then
    setsebool -P httpd_can_network_connect 1 || true
  fi
fi

echo "== [9/10] firewall =="
if systemctl is-active -q firewalld; then
  for p in $OPEN_PORTS; do
    firewall-cmd --permanent --add-port="${p}/tcp"
  done
  for p in $API_UPSTREAM_PORTS; do
    firewall-cmd --permanent --remove-port="${p}/tcp" >/dev/null 2>&1 || true
  done
  firewall-cmd --permanent --remove-port="${IMAGES_UPSTREAM_PORT}/tcp" >/dev/null 2>&1 || true
  firewall-cmd --reload
else
  echo "firewalld not active — skipping"
fi

echo "== [10/10] sanity checks =="
curl -fsS -o /dev/null http://127.0.0.1/ || true
sudo -iu "$DEPLOY_USER" podman info \
  --format 'rootless={{.Host.Security.Rootless}} storage={{.Store.GraphDriverName}}'

echo "OK: $DEPLOY_USER provisioned for rootless podman on $(hostname)"