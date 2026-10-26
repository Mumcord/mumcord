# Sourced by up.sh, down.sh and reset.sh. Derives compose variables from two optional overrides:
#   MUMCORD_PORT_BASE   host port of v15 (default 64738); v14/v13/limit/v15b follow at +1..+4
#   MUMCORD_PROJECT     compose project name (default testenv); a non-default name also gets its
#                       own data dir (data/_<name>, git-ignored) so parallel instances never share databases
# Defaults reproduce the original layout exactly.
MUMCORD_PORT_BASE="${MUMCORD_PORT_BASE:-64738}"
MUMCORD_PROJECT="${MUMCORD_PROJECT:-testenv}"
export COMPOSE_PROJECT_NAME="$MUMCORD_PROJECT"
export PORT_V15=$((MUMCORD_PORT_BASE)) PORT_V14=$((MUMCORD_PORT_BASE + 1)) PORT_V13=$((MUMCORD_PORT_BASE + 2))
export PORT_LIMIT=$((MUMCORD_PORT_BASE + 3)) PORT_V15B=$((MUMCORD_PORT_BASE + 4))
if [ "$MUMCORD_PROJECT" = testenv ]; then DATA_ROOT=data; else DATA_ROOT="data/_$MUMCORD_PROJECT"; fi
export DATA_ROOT
