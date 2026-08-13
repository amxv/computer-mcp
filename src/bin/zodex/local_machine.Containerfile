FROM ubuntu:24.04

ENV container=container
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
      bash \
      ca-certificates \
      curl \
      dbus \
      git \
      iproute2 \
      iputils-ping \
      openssl \
      procps \
      sudo \
      systemd && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

RUN : > /etc/machine-id && \
    mkdir -p /var/lib/dbus && \
    : > /var/lib/dbus/machine-id && \
    systemctl set-default multi-user.target && \
    systemctl mask \
      dev-hugepages.mount \
      sys-fs-fuse-connections.mount \
      systemd-update-utmp.service \
      systemd-tmpfiles-setup.service \
      console-getty.service && \
    systemctl disable networkd-dispatcher.service 2>/dev/null || true

STOPSIGNAL SIGRTMIN+3

CMD ["/sbin/init"]
