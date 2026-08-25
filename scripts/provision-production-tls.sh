#!/usr/bin/env bash
set -euo pipefail

host="${1:?usage: provision-production-tls.sh IP_ADDRESS}"
certbot_root=/opt/plant-ui-certbot
tls_conf=/etc/nginx/conf.d/plant-ui-tls.conf

# IP certificates use Let's Encrypt's short-lived profile. Certbot 5.4+
# supports webroot validation for IP identifiers.
apt-get update -qq
apt-get install -y -qq python3-venv
if [[ ! -x "$certbot_root/bin/certbot" ]]; then
  python3 -m venv "$certbot_root"
fi
"$certbot_root/bin/pip" install --quiet --upgrade 'certbot>=5.4'
"$certbot_root/bin/certbot" certonly \
  --non-interactive \
  --agree-tos \
  --register-unsafely-without-email \
  --preferred-profile shortlived \
  --webroot \
  --webroot-path /var/www/plant3d-web \
  --ip-address "$host" \
  --keep-until-expiring

cat > "$tls_conf" <<'NGINX'
server {
    listen 443 ssl;
    listen [::]:443 ssl;
    server_name __DEPLOY_HOST__;

    ssl_certificate /etc/letsencrypt/live/__DEPLOY_HOST__/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/__DEPLOY_HOST__/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_session_cache shared:PlantUiTLS:10m;

    # SurrealDB WebSocket endpoint on the same secure origin.
    location = /rpc {
        proxy_pass http://127.0.0.1:8020/rpc;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
        proxy_read_timeout 3600s;
    }

    # Preserve the established static, API and file routes behind port 80.
    location / {
        proxy_pass http://127.0.0.1:80;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection $http_connection;
    }
}
NGINX
sed -i "s/__DEPLOY_HOST__/$host/g" "$tls_conf"

cat > /etc/systemd/system/plant-ui-cert-renew.service <<'SYSTEMD'
[Unit]
Description=Renew Plant UI short-lived TLS certificate

[Service]
Type=oneshot
ExecStart=/opt/plant-ui-certbot/bin/certbot renew --quiet --deploy-hook "systemctl reload nginx"
SYSTEMD

cat > /etc/systemd/system/plant-ui-cert-renew.timer <<'SYSTEMD'
[Unit]
Description=Renew Plant UI TLS certificate twice daily

[Timer]
OnBootSec=15min
OnUnitActiveSec=12h
Persistent=true

[Install]
WantedBy=timers.target
SYSTEMD

nginx -t
systemctl daemon-reload
systemctl enable --now plant-ui-cert-renew.timer
systemctl reload nginx

curl --fail --silent --show-error \
  --resolve "$host:443:127.0.0.1" \
  "https://$host/plant-ui/" >/dev/null
openssl x509 -in "/etc/letsencrypt/live/$host/fullchain.pem" \
  -noout -subject -issuer -dates -ext subjectAltName
