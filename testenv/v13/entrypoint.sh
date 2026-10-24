#!/bin/bash
set -e
ini=/data/murmur.ini
cp /opt/mumcord/mumble-server.ini "$ini"
echo "serverpassword=${SERVER_PASSWORD}" >> "$ini"
murmurd -ini "$ini" -supw "$SUPERUSER_PASSWORD"
exec murmurd -fg -ini "$ini"
