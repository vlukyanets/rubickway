# The one image for development and CI: the devcontainer and every CI run build it from this
# file, so what passes in one passes in the other. Hardware checks (quickstart) stay on the host.
FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2

# Packages come from a fixed snapshot of the Debian archive, so the image is the same whenever it
# is built. Moving to newer packages is a change of this date.
ARG DEBIAN_SNAPSHOT=20261119T000000Z

RUN printf '%s\n' \
        "Types: deb" \
        "URIs: http://snapshot.debian.org/archive/debian/${DEBIAN_SNAPSHOT}" \
        "Suites: trixie trixie-updates" \
        "Components: main" \
        "Signed-By: /usr/share/keyrings/debian-archive-keyring.pgp" \
        "" \
        "Types: deb" \
        "URIs: http://snapshot.debian.org/archive/debian-security/${DEBIAN_SNAPSHOT}" \
        "Suites: trixie-security" \
        "Components: main" \
        "Signed-By: /usr/share/keyrings/debian-archive-keyring.pgp" \
        > /etc/apt/sources.list.d/debian.sources \
    && echo 'Acquire::Check-Valid-Until "false";' > /etc/apt/apt.conf.d/99snapshot \
    && apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        # Wayland and xkbcommon to link, EGL/GLES + Mesa (llvmpipe) for the rendering tests,
        # xkb-data for the keymap tests; libinput, libseat, libgbm and libudev for the
        # standalone backend.
        libxkbcommon-dev libegl-dev libgles-dev libegl-mesa0 xkb-data \
        libinput-dev libseat-dev libgbm-dev libudev-dev \
        # Building; ps for the spawn tests.
        build-essential pkg-config git ca-certificates rustup procps \
        # The devcontainer's network filter (init-firewall.sh).
        iptables ipset iproute2 dnsutils jq curl sudo \
    && rm -rf /var/lib/apt/lists/*

# The toolchain pinned in rust-toolchain.toml, installed into the image so no run downloads it.
ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:$PATH
COPY rust-toolchain.toml /tmp/rust-toolchain.toml
RUN channel="$(sed -n 's/^channel = "\(.*\)"$/\1/p' /tmp/rust-toolchain.toml)" \
    && test -n "$channel" \
    && rustup toolchain install "$channel" --profile minimal -c rustfmt -c clippy \
    && rustup default "$channel" \
    && rm /tmp/rust-toolchain.toml

# The development user, with the host's usual UID and GID. The cargo registry and the build
# output are volumes in the devcontainer; a new volume takes the owner of the directory it covers.
# The user may run the network filter as root, and nothing else.
ARG USERNAME=dev
ARG USER_UID=1000
ARG USER_GID=1000
RUN groupadd --gid "$USER_GID" "$USERNAME" \
    && useradd --uid "$USER_UID" --gid "$USER_GID" --create-home --shell /bin/bash "$USERNAME" \
    && mkdir -p /opt/cargo/registry "/home/$USERNAME/target" \
    && chown -R "$USERNAME:$USERNAME" /opt/cargo/registry "/home/$USERNAME/target" \
    && echo "$USERNAME ALL=(root) NOPASSWD: /usr/local/bin/init-firewall.sh" \
        > /etc/sudoers.d/init-firewall \
    && chmod 0440 /etc/sudoers.d/init-firewall

COPY .devcontainer/init-firewall.sh /usr/local/bin/init-firewall.sh
RUN chmod 0755 /usr/local/bin/init-firewall.sh
