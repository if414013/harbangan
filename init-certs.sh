#!/bin/sh
# Initialize Let's Encrypt certificates for the first time.
#
# Usage:
#   DOMAIN=gateway.example.com EMAIL=you@example.com ./init-certs.sh
#
# This creates a temporary self-signed cert so nginx can start,
# then obtains a real Let's Encrypt certificate via certbot.

set -e

DOMAIN="${DOMAIN:?Set DOMAIN env var (e.g. gateway.example.com)}"
EMAIL="${EMAIL:?Set EMAIL env var for Let's Encrypt notifications}"

echo "==> Creating temporary self-signed certificate for $DOMAIN..."
docker compose run --rm --entrypoint "" certbot sh -c "
  mkdir -p /etc/letsencrypt/live/$DOMAIN
  openssl req -x509 -nodes -newkey rsa:2048 -days 1 \
    -keyout /etc/letsencrypt/live/$DOMAIN/privkey.pem \
    -out /etc/letsencrypt/live/$DOMAIN/fullchain.pem \
    -subj '/CN=$DOMAIN'
"

echo "==> Starting nginx with temporary certificate..."
docker compose up -d frontend

echo "==> Requesting Let's Encrypt certificate for $DOMAIN..."
docker compose run --rm certbot certonly \
  --webroot -w /var/www/certbot \
  --email "$EMAIL" \
  --agree-tos \
  --no-eff-email \
  -d "$DOMAIN"

echo "==> Reloading nginx with real certificate..."
docker compose exec frontend nginx -s reload

echo "==> Done! Certificate obtained for $DOMAIN"
echo "    Run 'docker compose up -d' to start all services."
