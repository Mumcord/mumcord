FROM debian:bullseye-slim
# Bullseye is EOL: its packages only live on archive.debian.org now.
RUN sed -i "s|deb.debian.org|archive.debian.org|; /security.debian.org/d; /bullseye-updates/d" /etc/apt/sources.list \
 && apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends mumble-server \
 && rm -rf /var/lib/apt/lists/*
COPY mumble-server.ini entrypoint.sh /opt/mumcord/
RUN chmod +x /opt/mumcord/entrypoint.sh
EXPOSE 64738/tcp 64738/udp
ENTRYPOINT ["/opt/mumcord/entrypoint.sh"]
