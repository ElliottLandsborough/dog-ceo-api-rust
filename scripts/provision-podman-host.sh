#!/usr/bin/env bash
# Provision a CoreOS host for rootless Podman deploys for an existing deploy user.
# Runs as root; invoked from the Makefile as:  ssh <host> 'sudo bash -s' < this
# Idempotent — safe to re-run.
set -euo pipefail

DEPLOY_USER="${DEPLOY_USER:-deploy}"
OPEN_PORTS="${OPEN_PORTS:-80}"
SERVER_NAME="${SERVER_NAME:-dog.ceo}"
IMAGES_SERVER_NAME="${IMAGES_SERVER_NAME:-images.dog.ceo}"
WWW_SERVER_NAME="${WWW_SERVER_NAME:-www.dog.ceo}"
STATUS_SERVER_NAMES="${STATUS_SERVER_NAMES:-stats.dog.ceo status.dog.ceo}"
API_UPSTREAM_PORTS="${API_UPSTREAM_PORTS:-10081 10082}"
IMAGES_UPSTREAM_PORT="${IMAGES_UPSTREAM_PORT:-10080}"
NGINX_IMAGE="${NGINX_IMAGE:-nginxinc/nginx-unprivileged:stable-alpine}"
NGINX_CONTAINER_NAME="${NGINX_CONTAINER_NAME:-dog_ceo_nginx}"
NGINX_CONF_DIR="/home/${DEPLOY_USER}/nginx"
NGINX_CONF_FILE="${NGINX_CONF_DIR}/default.conf"

echo "== [1/7] user: $DEPLOY_USER =="
id "$DEPLOY_USER" &>/dev/null || {
  echo "ERROR: user '$DEPLOY_USER' does not exist; create it before running this script" >&2
  exit 1
}

echo "== [2/7] subuid/subgid ranges =="
grep -q "^$DEPLOY_USER:" /etc/subuid || usermod --add-subuids 200000-265535 "$DEPLOY_USER"
grep -q "^$DEPLOY_USER:" /etc/subgid || usermod --add-subgids 200000-265535 "$DEPLOY_USER"

echo "== [3/7] allow rootless bind on low ports (e.g. :80) =="
cat >/etc/sysctl.d/90-rootless-low-ports.conf <<'EOF'
net.ipv4.ip_unprivileged_port_start=80
EOF
sysctl -p /etc/sysctl.d/90-rootless-low-ports.conf >/dev/null

echo "== [4/7] honour --restart policies across reboots (rootless) =="
DEPLOY_UID="$(id -u "$DEPLOY_USER")"
sudo -iu "$DEPLOY_USER" \
  env XDG_RUNTIME_DIR="/run/user/$DEPLOY_UID" \
      DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$DEPLOY_UID/bus" \
  systemctl --user enable podman-restart.service

echo "== [5/7] nginx config =="
install -d -m 700 -o "$DEPLOY_USER" -g "$DEPLOY_USER" "$NGINX_CONF_DIR"
cat >"$NGINX_CONF_FILE" <<EOF
# CoreOS Podman nginx config for dog.ceo.
#
# Containers expected on the host:
#   api    -> 127.0.0.1:${API_UPSTREAM_PORTS} (round robin through nginx)
#   extra  -> one additional runtime can run outside the nginx upstream set
#   images -> 127.0.0.1:10080

upstream dog_api_runtime {
$(for p in $API_UPSTREAM_PORTS; do printf '    server 127.0.0.1:%s;\n' "$p"; done)
    keepalive 32;
}

upstream dog_images {
    server 127.0.0.1:${IMAGES_UPSTREAM_PORT};
    keepalive 16;
}

server {
    listen 8080;
    listen [::]:8080;
    server_name ${SERVER_NAME};

    if (\$request_method !~ ^(GET|HEAD|OPTIONS)$) {
        return 405;
    }

    location ~ /\.(?!well-known) {
        deny all;
        access_log off;
        log_not_found off;
        return 404;
    }

    location = /api {
        return 301 /api/;
    }

    location ^~ /api/ {
        if (\$request_method = OPTIONS) {
            add_header Access-Control-Allow-Origin "*" always;
            add_header Access-Control-Allow-Methods "GET, OPTIONS" always;
            add_header Access-Control-Allow-Headers "*" always;
            add_header Access-Control-Max-Age 86400 always;
            add_header Content-Type "text/plain; charset=utf-8";
            add_header Content-Length 0;
            return 204;
        }

        if (\$request_method !~ ^(GET)$) {
            return 405;
        }

        add_header Access-Control-Allow-Origin "*" always;
        add_header Access-Control-Allow-Methods "GET, OPTIONS" always;
        add_header Access-Control-Expose-Headers "*" always;

        proxy_http_version 1.1;
        proxy_set_header Connection "";
        proxy_set_header Host \$host;
        proxy_set_header X-Real-IP \$remote_addr;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto \$scheme;
        proxy_set_header X-Forwarded-Prefix /api;

        proxy_connect_timeout 5s;
        proxy_read_timeout 30s;
        proxy_next_upstream error timeout http_502 http_503;
        proxy_next_upstream_tries 2;

        proxy_pass http://dog_api_runtime/;
    }

    location / {
        return 404;
    }
}

server {
    listen 8080;
    listen [::]:8080;
    server_name ${IMAGES_SERVER_NAME};

    if (\$request_method !~ ^(GET|HEAD|OPTIONS)$) {
        return 405;
    }

    add_header X-Content-Type-Options "nosniff" always;
    add_header X-Frame-Options "SAMEORIGIN" always;

    location = /health {
        access_log off;
        default_type text/plain;
        return 200 "OK\n";
    }

    location ~ /\. {
        deny all;
        access_log off;
        log_not_found off;
    }

    location ~* \.(jpg|jpeg)$ {
        access_log off;

        if (\$request_method = OPTIONS) {
            add_header Access-Control-Allow-Origin "*" always;
            add_header Access-Control-Allow-Methods "GET, OPTIONS" always;
            add_header Access-Control-Allow-Headers "Origin, Content-Type, Accept" always;
            add_header Access-Control-Max-Age 86400 always;
            add_header Content-Length 0;
            return 204;
        }

        rewrite ^/breeds/(.*)$ /$1 break;

        add_header Cache-Control "public, max-age=31536000, immutable" always;
        add_header Access-Control-Allow-Origin "*" always;
        add_header Access-Control-Allow-Methods "GET, OPTIONS" always;
        add_header Access-Control-Allow-Headers "Origin, Content-Type, Accept" always;
        add_header X-Content-Type-Options "nosniff" always;
        add_header X-Frame-Options "SAMEORIGIN" always;

        proxy_hide_header Cache-Control;
        proxy_hide_header Expires;

        proxy_http_version 1.1;
        proxy_set_header Connection "";
        proxy_set_header Host \$host;
        proxy_set_header X-Real-IP \$remote_addr;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto \$scheme;

        proxy_pass http://dog_images;
    }

    location / {
        rewrite ^/breeds/(.*)$ /$1 break;

        proxy_http_version 1.1;
        proxy_set_header Connection "";
        proxy_set_header Host \$host;
        proxy_set_header X-Real-IP \$remote_addr;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto \$scheme;

        proxy_pass http://dog_images;
    }
}

server {
    listen 8080;
    listen [::]:8080;
    server_name ${WWW_SERVER_NAME};
    return 301 https://${SERVER_NAME}\$request_uri;
}

server {
    listen 8080;
    listen [::]:8080;
    server_name ${STATUS_SERVER_NAMES};
    return 301 https://stats.uptimerobot.com/70H4CPut5F;
}
EOF

sudo -iu "$DEPLOY_USER" \
  env XDG_RUNTIME_DIR="/run/user/$DEPLOY_UID" \
      DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$DEPLOY_UID/bus" \
  podman run --rm --entrypoint nginx \
  -v "$NGINX_CONF_FILE:/etc/nginx/conf.d/default.conf:ro,Z" \
  "$NGINX_IMAGE" -t >/dev/null

echo "== [6/7] start nginx container =="
sudo -iu "$DEPLOY_USER" \
  env XDG_RUNTIME_DIR="/run/user/$DEPLOY_UID" \
      DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$DEPLOY_UID/bus" \
  podman pull "$NGINX_IMAGE"
sudo -iu "$DEPLOY_USER" \
  env XDG_RUNTIME_DIR="/run/user/$DEPLOY_UID" \
      DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$DEPLOY_UID/bus" \
  podman rm -f dog_ceo_api_rust_1 dog_ceo_api_rust_2 dog_ceo_api_rust_3 dog_ceo_api_rust_4 dog_ceo_api_images "$NGINX_CONTAINER_NAME" >/dev/null 2>&1 || true
sudo -iu "$DEPLOY_USER" \
  env XDG_RUNTIME_DIR="/run/user/$DEPLOY_UID" \
      DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$DEPLOY_UID/bus" \
  podman run -d --restart unless-stopped \
    --name "$NGINX_CONTAINER_NAME" \
    -p 127.0.0.1:80:8080 \
    -v "$NGINX_CONF_FILE:/etc/nginx/conf.d/default.conf:ro,Z" \
    "$NGINX_IMAGE"

echo "== [7/7] firewall =="
if systemctl is-active -q firewalld; then
  for p in $OPEN_PORTS; do
    firewall-cmd --permanent --add-port="${p}/tcp"
  done
  firewall-cmd --reload
else
  echo "firewalld not active — skipping"
fi

echo "== sanity check: rootless podman as $DEPLOY_USER =="
sudo -iu "$DEPLOY_USER" \
  env XDG_RUNTIME_DIR="/run/user/$DEPLOY_UID" \
      DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$DEPLOY_UID/bus" \
  podman info --format 'rootless={{.Host.Security.Rootless}} storage={{.Store.GraphDriverName}}'

echo "OK: $DEPLOY_USER provisioned for rootless podman on $(hostname)"
