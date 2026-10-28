# tcp-kernel-slowdown

Delay TCP handshakes to one port inside a Docker network, so a TCP ping looks slower than a WebSocket upgrade on the same server.

A TCP ping, such as `tcping` or `nc -z`, times only the handshake. That is the client's SYN and the server's SYN-ACK. A WebSocket upgrade is an HTTP `GET` with `Upgrade: websocket` that the server answers with `101 Switching Protocols`, and it travels on a connection that is already open. `tcp-kernel-slowdown` holds each incoming SYN for a fixed delay and lets every other packet through. The TCP ping pays the delay and the upgrade exchange does not.

## Why the delay is in the kernel

The kernel completes the handshake before the server calls `accept()`. A userspace proxy in front of the service gets its connections the same way, so it can delay data but not the handshake.

`tcp-kernel-slowdown` runs as a sidecar container that shares the network namespace of the service container. On start, it adds this rule in that namespace, unless the rule is already there:

```sh
iptables -I INPUT -p tcp --dport <port> --syn -j NFQUEUE --queue-num <port> --queue-bypass
```

The rule sends each incoming SYN for the port to the netfilter queue with the same number as the port. The sidecar reads that queue and holds each SYN for the delay. When the sidecar accepts the SYN, the kernel sends the SYN-ACK.

The rule stays after the sidecar exits. While no process reads the queue, `--queue-bypass` makes the kernel accept SYNs without a delay. When the sidecar starts again, it finds the rule with `iptables -C` and does not add a second copy. If you change the port, the rule for the old port stays too, but nothing reads its queue, so the old port gets no delay.

## Attach the sidecar to an existing compose service

Add a service next to the one you want to slow down, and point `build` at a checkout of this repository:

```yaml
services:
  api:
    image: example/api

  api-slowdown:
    build: ../tcp-kernel-slowdown
    network_mode: service:api
    cap_add: [NET_ADMIN]
    init: true
    command: ["8080", "300"]
```

The arguments are `<port> <delay-ms>`. Use the port that the service listens on inside its container, not a published port. `NET_ADMIN` lets the sidecar add the rule and read the queue. `init: true` lets `docker compose stop` end the sidecar without waiting for the 10-second stop timeout.

## Run the demo

`compose.yaml` runs `jmalloc/echo-server`, a WebSocket echo server on port 8080, with a sidecar that delays its SYNs by 300 ms. The `probe` service runs the `probe` binary from the sidecar's image.

```sh
docker compose up -d --build
docker compose run --rm probe
docker compose down
```

`probe <host:port> [samples]` opens one connection per sample, 5 by default. It times `connect`, which is the TCP ping. Then it sends a WebSocket upgrade request on the open connection and times the wait for the `HTTP/1.1 101` status line. It prints each sample and the median of each column. If a response is not `101`, it exits with status 1.

Expect a `tcp ping` median of at least 300 ms and an `upgrade` median under 5 ms. To take 20 samples, run `docker compose run --rm probe server:8080 20`.

`cargo build` works only on Linux, because the `nfq` crate uses Linux netlink sockets. On other systems, build through Docker.

## Caveats

- A client that times the full WebSocket connect, TCP handshake plus upgrade, also pays the delay. Only the upgrade exchange on an open connection stays fast.
- Only clients inside the Docker network see the delay. A client on the host that connects to a published port completes its handshake with the Docker Desktop port proxy, not with the container.
- The delay applies to IPv4 only. The sidecar adds no `ip6tables` rule, so a connection over IPv6 gets no delay.
- A delay above about 1 second makes clients retransmit the SYN, because Linux waits 1 second for the first SYN-ACK. The sidecar delays each retransmitted SYN too.
- The sidecar holds at most 65,536 SYNs at a time. Past that, or when many SYNs arrive at the same instant and the sidecar cannot read them fast enough, the kernel accepts the extra SYNs without a delay. In a test, 1,200 connections opened at once let up to 117 of them through without the delay.
- Stopping the sidecar drops the SYNs it holds. Those clients connect on their SYN retransmit, about 1 second later.
