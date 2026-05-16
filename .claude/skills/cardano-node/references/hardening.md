# Server Hardening for Cardano SPOs

A compromised relay or block producer can mean missed blocks at best, lost funds
at worst. Apply this checklist to **every** Cardano host: relays, block
producer, and monitoring node.

Authoritative source: `references/sources/deployment-hardening-server.md` plus
`references/sources/deployment-improve-grafana-security.md` and
`references/sources/deployment-audit-your-node.md`.

Target OS: Ubuntu Server 22.04 LTS or 20.04 LTS. Steps adapt to Debian 12 with
minor changes.

## Table of Contents
1. [Create a non-root user](#1-non-root-user)
2. [Disable root login](#2-disable-root)
3. [System updates](#3-system-updates)
4. [Unattended upgrades](#4-unattended-upgrades)
5. [SSH key auth](#5-ssh-keys)
6. [SSH hardening](#6-ssh-hardening)
7. [UFW firewall](#7-firewall)
8. [fail2ban](#8-fail2ban)
9. [sysctl tuning](#9-sysctl)
10. [Shared memory](#10-shared-memory)
11. [KES Agent extra hardening](#11-kes-agent-hardening)
12. [Grafana security](#12-grafana-security)
13. [Audit your node](#13-audit)

---

## 1. Non-root user

```bash
sudo useradd -m -s /bin/bash cardano
sudo passwd cardano
sudo usermod -aG sudo cardano

# Reconnect as cardano
ssh cardano@server-ip

# Optional: remove default user
sudo userdel <defaultuser>
```

---

## 2. Disable root

```bash
sudo passwd -l root
```

---

## 3. System updates

```bash
sudo apt-get update -y
sudo apt-get upgrade -y
sudo apt-get autoremove
sudo apt-get autoclean
sudo reboot
```

---

## 4. Unattended upgrades

Auto-install security patches:
```bash
sudo apt-get install unattended-upgrades
sudo dpkg-reconfigure -plow unattended-upgrades
```

Default config installs security updates only and does **not** auto-reboot.

---

## 5. SSH keys

On your local workstation:
```bash
ssh-keygen -t ed25519
ssh-copy-id -i $HOME/.ssh/<keyfile> cardano@server-ip
```

Verify keyless login works, then proceed to disable password auth. Back up your
private key to encrypted cold storage.

---

## 6. SSH hardening

Edit `/etc/ssh/sshd_config`:

```
Port <custom-port>                  # 1024–49150
PubkeyAuthentication yes
PasswordAuthentication no
PermitRootLogin prohibit-password
PermitEmptyPasswords no
X11Forwarding no
TCPKeepAlive no
Compression no
AllowAgentForwarding no
AllowTcpForwarding no
KbdInteractiveAuthentication no
```

Validate and restart:
```bash
sudo sshd -t
sudo systemctl restart sshd
ssh cardano@server-ip -p <custom-port>
```

---

## 7. Firewall (UFW)

### Relay
```bash
sudo ufw default deny incoming
sudo ufw default allow outgoing
sudo ufw allow to any proto tcp port <SSH_PORT>
sudo ufw allow to any proto tcp port <CARDANO_NODE_PORT>     # e.g. 3001
sudo ufw enable
```

### Block producer
Only your relays may reach the node port:
```bash
sudo ufw default deny incoming
sudo ufw default allow outgoing
sudo ufw allow to any proto tcp port <SSH_PORT>
sudo ufw allow from <RELAY1_IP> to any proto tcp port <CARDANO_NODE_PORT>
sudo ufw allow from <RELAY2_IP> to any proto tcp port <CARDANO_NODE_PORT>
sudo ufw enable
```

### Monitoring host
Open only what's needed for the new tracing system:
```bash
sudo ufw allow to any proto tcp port <SSH_PORT>
sudo ufw allow to any proto tcp port 3000      # Grafana web UI (restrict by source if exposed)
sudo ufw enable
```

If the tracer socket is forwarded over SSH (recommended), no extra inbound
ports needed on the monitored nodes. Otherwise, allow from the monitoring host
only.

### Mithril relay (if running one)
```bash
sudo ufw allow from <BP_INTERNAL_IP> to any proto tcp port 3132
```

### Restrict SSH to your management IP
```bash
sudo ufw allow from <YOUR_IP> to any proto tcp port <SSH_PORT>
```

---

## 8. fail2ban

```bash
sudo apt-get install fail2ban -y
sudo systemctl enable --now fail2ban
sudo cp /etc/fail2ban/jail.conf /etc/fail2ban/jail.local
```

In `/etc/fail2ban/jail.local`, `[DEFAULT]`:
```ini
bantime  = 1h
bantime.increment = true
bantime.factor = 2
bantime.maxtime = 5w
findtime  = 10m
maxretry = 2
```

`[sshd]`:
```ini
mode    = aggressive
enabled = true
port    = <SSH_PORT>
filter  = sshd
maxretry = 2
logpath = /var/log/auth.log
backend = %(sshd_backend)s
```

In `/etc/fail2ban/filter.d/sshd.conf`, set `mode = aggressive`.

```bash
sudo systemctl restart fail2ban
```

Adjust `maxretry` and `bantime` to taste — the above is aggressive.

---

## 9. sysctl

Append to `/etc/sysctl.conf`:

```ini
# Smurf attack
net.ipv4.icmp_echo_ignore_broadcasts = 1

# Bogus ICMP error responses
net.ipv4.icmp_ignore_bogus_error_responses = 1

# SYN flood protection
net.ipv4.tcp_syncookies = 1

# Log spoofed / source-routed / redirect packets
net.ipv4.conf.all.log_martians = 1
net.ipv4.conf.default.log_martians = 1

# Reject source-routed packets
net.ipv4.conf.all.accept_source_route = 0
net.ipv4.conf.default.accept_source_route = 0

# TCP buffer sizes
net.ipv4.tcp_rmem = 4096 87380 8388608
net.ipv4.tcp_wmem = 4096 87380 8388608

# No redirects
net.ipv4.conf.all.accept_redirects = 0
net.ipv4.conf.default.accept_redirects = 0
net.ipv4.conf.all.secure_redirects = 0

# Disable forwarding (this host is not a router)
net.ipv4.ip_forward = 0

# SYN-ACK retries
net.ipv4.tcp_synack_retries = 5
```

Apply:
```bash
sudo sysctl -p
```

---

## 10. Shared memory

`/etc/fstab`:
```
tmpfs   /run/shm   tmpfs   ro,noexec,nosuid   0 0
```

```bash
sudo reboot
```

---

## 11. KES Agent hardening

If you're running the KES Agent on the block producer, the forward-secrecy
guarantee requires the signing key to never reach disk. Add these steps
**on top of** steps 1–10:

### Disable swap
```bash
sudo swapoff -a
# Remove the swap entry from /etc/fstab
sudo sed -i.bak '/\sswap\s/d' /etc/fstab
```

### Disable hibernation
```bash
sudo systemctl mask hibernate.target hybrid-sleep.target suspend-then-hibernate.target
```

### Disable core dumps
`/etc/security/limits.conf`:
```
* hard core 0
* soft core 0
```

`/etc/sysctl.conf`:
```
fs.suid_dumpable = 0
kernel.core_pattern = |/bin/false
```

`/etc/systemd/coredump.conf`:
```ini
[Coredump]
Storage=none
ProcessSizeMax=0
```

```bash
sudo systemctl daemon-reload
sudo sysctl -p
```

### Encrypt persisted data
The OpCert (`node.cert`), VRF key (`vrf.skey`), and `cold.vkey` still live on
disk. Put them on a LUKS volume that requires manual unlock at boot.

See the full KES Agent hardening guide:
<https://github.com/input-output-hk/kes-agent/blob/main/doc/guide.markdown>

---

## 12. Grafana security

If you expose Grafana for SPO monitoring, harden it per
`references/sources/deployment-improve-grafana-security.md`. Key points:

- Run behind a reverse proxy (nginx, Caddy) with TLS
- Use OAuth (GitHub, Google, custom OIDC) instead of local users
- Disable anonymous and basic auth
- Restrict admin role; use viewer roles for delegates
- Set strong session cookie flags (`secure`, `httponly`, `samesite=strict`)
- Firewall: only allow port 3000 (or 443 via reverse proxy) from your IP

---

## 13. Audit

Cardano Developer Portal's "Audit your node" checklist:
`references/sources/deployment-audit-your-node.md`.

Run periodically:
```bash
# Confirm root cannot SSH in
sudo grep -i "permitrootlogin" /etc/ssh/sshd_config

# Confirm password auth disabled
sudo grep -i "passwordauthentication" /etc/ssh/sshd_config

# Active services audit
systemctl list-unit-files --type=service --state=enabled

# Inbound rules
sudo ufw status verbose

# Active connections
sudo ss -tnlp

# Auth log review
sudo tail -100 /var/log/auth.log
```

Consider AIDE for filesystem integrity:
```bash
sudo apt-get install aide
sudo aideinit
sudo cp /var/lib/aide/aide.db.new /var/lib/aide/aide.db
# Run sudo aide --check periodically
```

---

## Key security reminders

- **Cold keys never touch a hot node.** Period.
- **Payment keys stay on the air-gapped machine.** Fund the address from a
  different wallet, then sign transactions on the air-gapped machine. For
  routine governance votes and pool operations, this is the cold-signing flow.
- **KES + VRF keys must be on the block producer** (or in the KES Agent for KES).
  This is why the BP must be isolated behind relays.
- **Back up all keys encrypted.** Encrypted USB sticks, ≥2 physical locations.
- **Sneakernet between online and air-gapped machines.** Encrypt files for
  transfer (`gpg --symmetric`, age, encrypted 7z).
- **Monitor logs.** Review `/var/log/auth.log`, journald, fail2ban.client status.
- **Subscribe to cardano-node release alerts** on GitHub for security updates
  and hard-fork coordination.
