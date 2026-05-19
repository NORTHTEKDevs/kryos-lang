# Deploying Kryos as a systemd service

A Kryos binary is a regular ELF executable — no runtime to install, no
interpreter, no `LD_LIBRARY_PATH` setup. The systemd unit is conventional.

## Service unit

`/etc/systemd/system/myapp.service`:

```ini
[Unit]
Description=My Kryos app
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=myapp
Group=myapp
ExecStart=/usr/local/bin/myapp
Restart=on-failure
RestartSec=5

# Sandboxing — Kryos has no JIT, no FFI loader, so we can lock down hard.
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=/var/lib/myapp
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
LockPersonality=true
MemoryDenyWriteExecute=true

# Resource limits
LimitNOFILE=65535
MemoryMax=512M

# Logging — kryos std::log goes to stderr → captured by journald.
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

## Install + enable

```bash
# 1. Build the binary
kryos build --release src/main.kry -o /tmp/myapp

# 2. Place it
sudo install -o root -g root -m 0755 /tmp/myapp /usr/local/bin/myapp

# 3. Create the runtime user
sudo useradd --system --no-create-home --shell /usr/sbin/nologin myapp

# 4. Create the data dir
sudo install -d -o myapp -g myapp -m 0755 /var/lib/myapp

# 5. Install the unit
sudo cp myapp.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now myapp
```

## Watching logs

```bash
journalctl -u myapp -f
journalctl -u myapp --since '5 minutes ago' --output=cat | jq -R 'split(" ") | .'
```

If you used `std::log`, every line is `LEVEL ts=<n> msg="..." k=v` — easy to
post-process.

## Health endpoint pattern

Expose `/healthz` returning 200 OK as soon as your TCP listener is up.
Then add a `systemd-notify` hook from inside Kryos via an FFI shim:

```kryos
@capabilities(io, ffi)
extern "C" {
    fn sd_notify(unset_environment: i64, state_ptr: i64) -> i64
}

fn ready() {
    let msg = "READY=1\n"
    let p = str_to_ptr(msg)
    sd_notify(0, p)
}
```

And in the unit:

```ini
Type=notify
```

systemd will wait for the `READY=1` notification before marking the service
active, so liveness probes and dependent units see a consistent state.
