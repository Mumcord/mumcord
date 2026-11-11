FROM debian:bullseye-slim@sha256:e5b6442dd2e9684cf5e87d8338b5968f3b348636fc0be6d7850a381e3731a2bd
# Bullseye is EOL: its packages only live on archive.debian.org now.
RUN sed -i "s|deb.debian.org|archive.debian.org|; /security.debian.org/d; /bullseye-updates/d" /etc/apt/sources.list \
 && apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends mumble-server \
 && rm -rf /var/lib/apt/lists/*
COPY mumble-server.ini entrypoint.sh /opt/mumcord/
RUN chmod +x /opt/mumcord/entrypoint.sh
EXPOSE 64738/tcp 64738/udp
ENTRYPOINT ["/opt/mumcord/entrypoint.sh"]
