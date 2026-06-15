# Deployment

Step-by-step setup guides for running reverse-proxy.

## Docker Deployment (Recommended)

This is the easiest way to get started and provides container-level isolation
as a defense-in-depth measure.

### 1. Build the image

```bash
cd /path/to/reverse-proxy
docker build -t reverse-proxy .
```

### 2. Create directories on the host

```bash
sudo mkdir -p /etc/reverse-proxy
sudo mkdir -p /var/lib/reverse-proxy/acme-cache
sudo mkdir -p /var/log/reverse-proxy
```

For the admin API, create the admin key file:

```bash
openssl rand -hex 32 | sudo tee /etc/reverse-proxy/admin-key
sudo chmod 600 /etc/reverse-proxy/admin-key
```

### 3. Create the config file

Create `/etc/reverse-proxy/config.toml`. For a basic single-domain setup with
Let's Encrypt:

```toml
allow_wildcard_bind = true
health_check_port = 9900
admin_key_path = "/etc/reverse-proxy/admin-key"

[logging]
level = "info"
format = "text"
log_file_path = "/var/log/reverse-proxy/access.log"

[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

[[listeners]]
bind_addr = "0.0.0.0"
http_port = 80
https_port = 443

[listeners.tls]
mode = "acme"
acme_domains = ["yourdomain.example.com"]
acme_cache_dir = "/var/lib/reverse-proxy/acme-cache"
acme_directory = "production"
acme_contact = "mailto:admin@yourdomain.example.com"

[[listeners.sites]]
host = "yourdomain.example.com"
upstream = "your-backend:8080"
```

**Important:** Replace `yourdomain.example.com` with your actual domain and
`your-backend:8080` with your upstream service address. For initial testing,
use `acme_directory = "staging"` to avoid Let's Encrypt rate limits.

### 4. Set up Docker Compose

Copy and customize `docker-compose.yml`:

```bash
cp deploy/docker-compose.yml /opt/reverse-proxy/docker-compose.yml
```

Edit the compose file to:
- Replace `203.0.113.10` with your server's public IP
- Update upstream service definitions to match your infrastructure
- Adjust the `DB_PASSWORD` environment variable (use Docker secrets or `.env`
  file, never commit real passwords)

### 5. Start the proxy

```bash
cd /opt/reverse-proxy
docker compose up -d
```

### 6. Verify

```bash
# Check container health
docker compose ps

# Test health endpoint (from the host)
curl -s http://127.0.0.1:9900/health

# Check logs
docker compose logs reverse-proxy

# Test TLS
curl -v https://yourdomain.example.com/
```

### 7. Set up fail2ban

If you want automated IP banning for rate limit violations:

```bash
sudo cp deploy/fail2ban/filter.d/reverse-proxy.conf /etc/fail2ban/filter.d/
sudo cp deploy/fail2ban/jail.d/reverse-proxy.conf /etc/fail2ban/jail.d/
sudo systemctl restart fail2ban
```

Verify fail2ban is watching the logs:

```bash
sudo fail2ban-client status reverse-proxy
```

## Bare Metal / systemd Deployment

For running directly on a host without Docker.

### 1. Build

```bash
cargo build --release
# For a fully static binary (no libc dependency):
# cargo build --release --target x86_64-unknown-linux-musl
```

### 2. Install

```bash
sudo cp target/release/reverse-proxy /usr/local/bin/
sudo cp deploy/reverse-proxy.service /etc/systemd/system/
```

### 3. Create config and directories

```bash
sudo mkdir -p /etc/reverse-proxy
sudo mkdir -p /var/lib/reverse-proxy/acme-cache
sudo mkdir -p /var/log/reverse-proxy
```

Create `/etc/reverse-proxy/config.toml` (see example configs in the main
README). With a bare metal deployment, use the server's actual IP as
`bind_addr` instead of `0.0.0.0`:

```toml
# Single-domain bare metal example
health_check_port = 9900
admin_key_path = "/etc/reverse-proxy/admin-key"

[logging]
level = "info"
format = "text"
log_file_path = "/var/log/reverse-proxy/access.log"

[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

[[listeners]]
bind_addr = "203.0.113.10"
http_port = 80
https_port = 443

[listeners.tls]
mode = "acme"
acme_domains = ["yourdomain.example.com"]
acme_cache_dir = "/var/lib/reverse-proxy/acme-cache"
acme_directory = "production"
acme_contact = "mailto:admin@yourdomain.example.com"

[[listeners.sites]]
host = "yourdomain.example.com"
upstream = "127.0.0.1:3000"
```

### 4. Start

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now reverse-proxy
```

### 5. Verify

```bash
# Check service status
systemctl status reverse-proxy

# Test health endpoint
curl -s http://127.0.0.1:9900/health

# Check logs
journalctl -u reverse-proxy -f
```

### 6. Reload config

```bash
# Via SIGHUP (no feedback)
sudo kill -SIGHUP $(pidof reverse-proxy)

# Via admin HTTP API (returns success/failure JSON)
ADMIN_KEY=$(cat /etc/reverse-proxy/admin-key)
curl -X POST -H "Authorization: Bearer $ADMIN_KEY" http://127.0.0.1:9900/admin/reload

# Check status
curl -H "Authorization: Bearer $ADMIN_KEY" http://127.0.0.1:9900/admin/status

# Rotate admin key (returns new key, old key is invalidated)
curl -X POST -H "Authorization: Bearer $ADMIN_KEY" http://127.0.0.1:9900/admin/rotate-key
```

## Multi-Domain Setup

### Shared IP with SAN certificate

One IP, one listener, multiple domains on a single Let's Encrypt SAN certificate:

```toml
[[listeners]]
bind_addr = "203.0.113.10"

[listeners.tls]
mode = "acme"
acme_domains = ["git.example.com", "www.example.com"]
acme_cache_dir = "/var/lib/reverse-proxy/acme-cache"
acme_directory = "production"
acme_contact = "mailto:admin@example.com"

[[listeners.sites]]
host = "git.example.com"
upstream = "127.0.0.1:3000"

[[listeners.sites]]
host = "www.example.com"
upstream = "127.0.0.1:8080"
```

### Dedicated IP per domain

Multiple listeners, each with its own IP and certificate:

```toml
[[listeners]]
bind_addr = "203.0.113.10"

[listeners.tls]
mode = "acme"
acme_domains = ["git.example.com"]
acme_cache_dir = "/var/lib/reverse-proxy/acme-cache-git"
acme_directory = "production"
acme_contact = "mailto:admin@example.com"

[[listeners.sites]]
host = "git.example.com"
upstream = "127.0.0.1:3000"

[[listeners]]
bind_addr = "203.0.113.11"

[listeners.tls]
mode = "manual"
cert_path = "/etc/ssl/www.example.com/fullchain.pem"
key_path = "/etc/ssl/www.example.com/privkey.pem"

[[listeners.sites]]
host = "www.example.com"
upstream = "127.0.0.1:8080"
```

## HTTPS Upstream

If your upstream service uses TLS, set `upstream_scheme = "https"`:

```toml
[[listeners.sites]]
host = "secure.example.com"
upstream = "10.0.0.5:8443"
upstream_scheme = "https"
```

The proxy validates upstream TLS certificates using the system's native
certificate store.

## Security Notes

- The proxy binds to explicit IP addresses by default. `0.0.0.0` is rejected
  unless `--allow-wildcard-bind` or `allow_wildcard_bind = true` is set.
  This prevents accidental exposure on unintended interfaces.
- The health check endpoint binds to `127.0.0.1` only and is never exposed on
  public ports.
- The admin API requires Bearer token authentication on port 9900 (localhost
  only). Set `admin_key_path` in config to enable, or leave it empty to disable
  admin endpoints entirely. See ADR-028 for details.
- Rate limiting is global per-IP (IPv4: /32, IPv6: /64) in the current
  version. Per-site rate limits may be added later.
- All log output disables ANSI escape codes for fail2ban and container
  compatibility.
- The `Server` header is stripped from upstream responses and not added by the
  proxy, reducing server fingerprinting.