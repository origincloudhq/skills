#!/bin/sh
# Starts tinyproxy allowing CONNECT on port 443 to the hosts in EVAL_ALLOW
# (space-separated), and nothing else.
set -eu
: > /tmp/filter
for host in ${EVAL_ALLOW}; do
  printf '^%s$\n' "$(printf '%s' "${host}" | sed 's/\./\\./g')" >> /tmp/filter
done
cat > /tmp/tinyproxy.conf <<CONF
Port 3128
Listen 0.0.0.0
Timeout 600
MaxClients 50
LogLevel Notice
ConnectPort 443
Filter "/tmp/filter"
FilterDefaultDeny Yes
FilterExtended On
CONF
exec tinyproxy -d -c /tmp/tinyproxy.conf
