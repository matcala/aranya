FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3 python3-pip iproute2 iputils-ping net-tools socat tcpdump netcat-openbsd ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY app.py /app/app.py

# Defaults: listen for CMD on 6100, send TLM to 6101
ENV CMD_LISTEN_PORT=6100
ENV TLM_DEST_HOST=host.docker.internal
ENV TLM_DEST_PORT=6101
# Optional “tap/forward” destination (if set, we also send each TLM to this host:port)
ENV FORWARD_TLM_HOST=
ENV FORWARD_TLM_PORT=

CMD ["python3", "/app/app.py"]
