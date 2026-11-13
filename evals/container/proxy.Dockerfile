# The only way out of an eval's network: a proxy that lets through HTTPS
# to the hosts EVAL_ALLOW names, the agent's model provider, and nothing
# else.
FROM debian:trixie-slim
RUN apt-get update \
  && apt-get install -y --no-install-recommends tinyproxy \
  && rm -rf /var/lib/apt/lists/*
COPY evals/container/proxy.sh /usr/local/bin/eval-proxy
ENTRYPOINT ["/usr/local/bin/eval-proxy"]
