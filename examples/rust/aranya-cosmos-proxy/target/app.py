import os, socket, threading, time, datetime

CMD_LISTEN_PORT = int(os.getenv("CMD_LISTEN_PORT", 6100))
TLM_DEST_HOST   = os.getenv("TLM_DEST_HOST", "host.docker.internal")
TLM_DEST_PORT   = int(os.getenv("TLM_DEST_PORT", 6101))
FWD_HOST        = os.getenv("FORWARD_TLM_HOST", "").strip()
FWD_PORT        = int(os.getenv("FORWARD_TLM_PORT", "0") or 0)

def log(msg):
    ts = datetime.datetime.utcnow().isoformat() + "Z"
    print(f"[udptgt] {ts} {msg}", flush=True)

def cmd_listener():
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.bind(("0.0.0.0", CMD_LISTEN_PORT))
    log(f"listening for commands on UDP :{CMD_LISTEN_PORT}")
    while True:
        data, addr = s.recvfrom(4096)
        # Minimal decode: first byte is ID (must be 1 for NOOP)
        pkt_id = data[0] if data else None
        log(f"cmd from {addr} len={len(data)} id={pkt_id}")
        # No side-effect; it’s a NOOP.

def tlm_sender():
    s_main = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s_fwd  = None
    if FWD_HOST and FWD_PORT:
        s_fwd = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        log(f"forwarding enabled -> {FWD_HOST}:{FWD_PORT}")

    log(f"sending telemetry to {TLM_DEST_HOST}:{TLM_DEST_PORT} at 1 Hz")
    while True:
        # Packet format: [ID=1][timestamp string][NUL]
        ts = datetime.datetime.utcnow().isoformat() + "Z"
        payload = bytes([1]) + ts.encode("utf-8") + b"\x00"
        s_main.sendto(payload, (TLM_DEST_HOST, TLM_DEST_PORT))
        if s_fwd:
            s_fwd.sendto(payload, (FWD_HOST, FWD_PORT))
        log(f"tlm sent len={len(payload)} ts='{ts}'")
        time.sleep(1.0)

if __name__ == "__main__":
    threading.Thread(target=cmd_listener, daemon=True).start()
    tlm_sender()

# target still listens for NOOP on 6100/udp
# docker run --rm --name udptgt \
#   -p 6100:6100/udp \
#   -e TLM_DEST_HOST=host.docker.internal \
#   -e TLM_DEST_PORT=6202 \
#   udptgt
# 