#!/bin/bash
# The devcontainer's network filter, run as root at every start (postStartCommand): outgoing
# connections only to GitHub, crates.io, the Rust toolchain servers, the Debian archive and the
# editor's servers, plus DNS, the host network and loopback. Modelled on Anthropic's reference
# devcontainer. Addresses are resolved once, at start; a restart picks up new ones.
#
# To allow another domain, add it to ALLOWED_DOMAINS and rebuild the container.
set -euo pipefail
IFS=$'\n\t'

ALLOWED_DOMAINS=(
    crates.io
    index.crates.io
    static.crates.io
    static.rust-lang.org
    deb.debian.org
    snapshot.debian.org
    update.code.visualstudio.com
    marketplace.visualstudio.com
    vscode.blob.core.windows.net
)

# Docker's own DNS rules live in the nat table: keep them across the flush.
docker_dns_rules=$(iptables-save -t nat | grep "127\.0\.0\.11" || true)

iptables -F
iptables -X
iptables -t nat -F
iptables -t nat -X
iptables -t mangle -F
iptables -t mangle -X
ipset destroy allowed-domains 2>/dev/null || true

if [ -n "$docker_dns_rules" ]; then
    iptables -t nat -N DOCKER_OUTPUT 2>/dev/null || true
    iptables -t nat -N DOCKER_POSTROUTING 2>/dev/null || true
    echo "$docker_dns_rules" | xargs -L 1 iptables -t nat
fi

# DNS and loopback, before anything is resolved.
iptables -A OUTPUT -p udp --dport 53 -j ACCEPT
iptables -A INPUT -p udp --sport 53 -j ACCEPT
iptables -A OUTPUT -p tcp --dport 53 -j ACCEPT
iptables -A INPUT -p tcp --sport 53 -m state --state ESTABLISHED -j ACCEPT
iptables -A INPUT -i lo -j ACCEPT
iptables -A OUTPUT -o lo -j ACCEPT

ipset create allowed-domains hash:net

# GitHub publishes its address ranges; only the IPv4 ones go in (IPv6 is closed below).
github_meta=$(curl -fsS --connect-timeout 10 https://api.github.com/meta)
if ! echo "$github_meta" | jq -e '.web and .api and .git' >/dev/null; then
    echo "init-firewall: GitHub's address list is missing fields" >&2
    exit 1
fi
while read -r cidr; do
    ipset add -exist allowed-domains "$cidr"
done < <(echo "$github_meta" | jq -r '(.web + .api + .git)[]' | grep -v ':')

for domain in "${ALLOWED_DOMAINS[@]}"; do
    ips=$(dig +noall +answer A "$domain" | awk '$4 == "A" {print $5}')
    if [ -z "$ips" ]; then
        echo "init-firewall: could not resolve $domain" >&2
        exit 1
    fi
    while read -r ip; do
        ipset add -exist allowed-domains "$ip"
    done <<< "$ips"
done

# The host network: the editor talks to its server in the container through it.
host_ip=$(ip route | awk '/^default/ {print $3; exit}')
if [ -z "$host_ip" ]; then
    echo "init-firewall: no default route" >&2
    exit 1
fi
host_network="${host_ip%.*}.0/24"
iptables -A INPUT -s "$host_network" -j ACCEPT
iptables -A OUTPUT -d "$host_network" -j ACCEPT

iptables -P INPUT DROP
iptables -P FORWARD DROP
iptables -P OUTPUT DROP
iptables -A INPUT -m state --state ESTABLISHED,RELATED -j ACCEPT
iptables -A OUTPUT -m state --state ESTABLISHED,RELATED -j ACCEPT
iptables -A OUTPUT -m set --match-set allowed-domains dst -j ACCEPT
iptables -A OUTPUT -j REJECT --reject-with icmp-admin-prohibited

# IPv6: loopback only.
if command -v ip6tables >/dev/null && ip6tables -L >/dev/null 2>&1; then
    ip6tables -F
    ip6tables -A INPUT -i lo -j ACCEPT
    ip6tables -A OUTPUT -o lo -j ACCEPT
    ip6tables -P INPUT DROP
    ip6tables -P FORWARD DROP
    ip6tables -P OUTPUT DROP
fi

# Self-check: a domain off the list is refused, one on it answers.
if curl -fsS --connect-timeout 5 https://example.com >/dev/null 2>&1; then
    echo "init-firewall: example.com is reachable, the filter does not work" >&2
    exit 1
fi
if ! curl -fsS --connect-timeout 10 https://index.crates.io/config.json >/dev/null; then
    echo "init-firewall: index.crates.io is not reachable" >&2
    exit 1
fi
echo "init-firewall: outgoing connections limited to the allowed domains"
