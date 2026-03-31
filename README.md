# Shorten.io: Distributed URL Shortener (Rust + Actix)

This project is a distributed URL shortener with:

- Consistent hashing for deterministic request routing
- Redis sharded cache
- PostgreSQL sharded storage
- Primary/replica read-write split per shard
- Retry, timeout, and fallback logic for resilience
- Rate limiting
- Real-time WebSocket observability
- Modern frontend dashboard for metrics, logs, and shard visibility

The result is a practical distributed systems demo that shows how traffic is routed, where data is stored, and how failures are handled.

## What Is Implemented

### Core backend capabilities

- Create short URL: `POST /create`
- Redirect short URL: `GET /{short_code}`
- Real-time event stream: `GET /ws`

### Data routing architecture

- A single consistent hash ring maps each short code to a logical node.
- Redis node and DB shard index are aligned using the same ring index.
- This keeps cache and persistent data locality for a key.

### Caching and storage strategy

- Write path:
	- Generate short code with Snowflake-like ID + Base62 encoding.
	- Route to shard using consistent hashing.
	- Write to DB primary.
	- Populate Redis cache.
- Read path:
	- Try Redis first.
	- On miss, try DB replica.
	- If replica fails/misses, fallback to DB primary.
	- Refill Redis cache after DB hit.

### Reliability features

- IO timeout guard on Redis and DB operations.
- Retry with exponential backoff.
- Redis fallback tries alternate nodes in deterministic order.
- DB fallback from replica to primary.

### Protection and observability

- Rate limiting in Redis using increment + expiry.
- Structured broadcast events over WebSocket for every read/write outcome.
- Frontend consumes only real backend event fields for metrics.

## System Design

## Components

![Architecture Diagram](diagrams/URL_Shortener_v2.png)

- API service: Rust + Actix Web
- Cache: 3 Redis nodes (sharded)
- Database: PostgreSQL shards with primary/replica pair model
- Load balancing: HAProxy (round robin across API instances)
- Frontend: static HTML/CSS/JS dashboard

### Routing model

- Hash ring uses virtual nodes (`replicas = 100`) to improve key distribution.
- Ring node names are logical (`redis0`, `redis1`, `redis2`).
- The same index selects:
	- Redis node for cache operations
	- DB shard pair for persistence

Pseudo flow:

1. `short_code -> hash ring -> shard index`
2. Redis index = shard index
3. DB shard index = shard index

## Backend Event Schema (WebSocket)

The backend sends JSON events such as:

- Write success/failure
- Read hit/miss
- Source of read (`redis`, `replica`, `primary`, `none`, `error`)

Typical fields:

- `type`: `write` or `read`
- `status`: `OK`, `ERROR`, or `NOT_FOUND`
- `node`: API node identity (`HOSTNAME`)
- `redis`: routed Redis logical node
- `db`: routed DB shard logical name
- `short_code`: key identifier
- `cache`: `HIT` or `MISS` (read events)
- `source`: read source (read events)
- `latency_ms`: measured per-request backend latency

## Frontend Dashboard

The frontend provides:

- URL shortening form
- Live log stream from WebSocket
- Dashboard stats (request count, hit rate, avg latency)
- Analytics tab (reads/writes/hits/misses)
- Nodes tab (API nodes, Redis nodes, DB shards seen in events)
- History tab (event summary timeline)

Runtime backend target:

- `SHORTENER_BACKEND_BASE` in browser localStorage (optional)
- default: `http://localhost:8080`

If needed, in browser console:

```js
localStorage.setItem("SHORTENER_BACKEND_BASE", "http://localhost:8080");
location.reload();
```

## Configuration

Environment variables used by backend:

- `DATABASE_NODES` (preferred) or `DB_NODES`
	- Comma-separated URLs
	- Must contain an even number of URLs
	- Each pair is interpreted as `(primary, replica)`
- `REDIS_NODES`
	- Comma-separated Redis URLs
- `PUBLIC_BASE_URL` (optional)
	- Used for generated short links
	- If absent, backend uses incoming request scheme/host

Important alignment rule:

- Number of DB shards = `DB_NODES count / 2`
- Number of Redis nodes = `REDIS_NODES count`
- These must be equal for routing alignment.

## API Reference

### Create short URL

- Method: `POST /create`
- Body:

```json
{ "long_url": "https://example.com/some/very/long/path" }
```

- Success response:

```json
{ "short_url": "http://localhost:8080/abc123" }
```

### Redirect

- Method: `GET /{short_code}`
- Behavior:
	- `302 Found` with `Location` header on success
	- `404 Not Found` when key does not exist
	- `500 Internal Server Error` on internal failure

### Realtime stream

- Method: `GET /ws`
- WebSocket endpoint for event telemetry

## Local Run Guide (Recommended)

### Prerequisites

- Rust toolchain
- PostgreSQL instances reachable on configured ports
- Redis instances reachable on configured ports

### Start backend

```bash
cargo run
```

Backend currently binds to `127.0.0.1:8080`.

### Start frontend

Use any static file server from project root or `frontend` folder.

Example:

```bash
cd frontend
python -m http.server 5500
```

Open `http://localhost:5500`.

## Docker Compose Notes

The repository includes `docker-compose.yml` and `haproxy.cfg`, but keep these practical points in mind:

- The current backend process binds to `127.0.0.1:8080`.
- Current compose/HAProxy entries target port `8000` for API containers.
- `.env` values using `localhost` work for local host mode, not container-to-container service discovery.

For fully containerized execution, align:

- API listen address/port
- Compose exposed and internal API ports
- HAProxy backend target ports
- DB/Redis hostnames (use service names, not localhost)

### Start only Redis and DB services

From the project root, run:

```bash
docker-compose up -d redis0 redis1 redis2 db0 db1 db2
```

Check status:

```bash
docker-compose ps redis0 redis1 redis2 db0 db1 db2
```

Stop only Redis and DB services:

```bash
docker-compose stop redis0 redis1 redis2 db0 db1 db2
```

Remove the same stopped containers:

```bash
docker-compose rm -f redis0 redis1 redis2 db0 db1 db2
```

## Validation and Testing Scenarios

### Functional smoke test

```bash
curl -X POST http://localhost:8080/create \
	-H "Content-Type: application/json" \
	-d "{\"long_url\":\"https://example.com\"}"
```

Open the returned short URL and verify redirect.

### Distribution test (many keys)

Create 50 URLs and observe logs/events:

- Routed Redis node and DB shard should vary by key
- Distribution should be reasonably balanced due to virtual nodes

### Failure behavior test

- Stop one DB shard instance.
- Verify that:
	- Some requests still succeed on healthy shards
	- Read path can fallback replica -> primary
	- Errors are surfaced in WebSocket logs for failed operations

### Cache behavior test

Request the same short link multiple times:

- first read: likely DB source + cache refill
- later reads: Redis HIT with lower latency

## Current Limitations

- WebSocket fanout is process-local (no cross-instance pub/sub bus).
- DB primary/replica is modeled by URL pairing, but real replication orchestration is external.
- No automatic ring rebalancing/migration workflow when adding or removing nodes.
- Rate limiter is basic key-window logic and currently keyed by long URL.

## Future Enhancements

- Cross-instance event bus (Redis pub/sub or NATS) for unified observability
- Circuit breaker and health-aware shard routing
- Data reconciliation and repair jobs
- Rebalancing/migration tooling for dynamic shard changes
- Advanced per-IP or token bucket rate limiting
- OpenTelemetry metrics + tracing
- Authenticated custom aliases and abuse protection

## Tech Stack

- Rust
- Actix Web + Actix Actors + Actix WebSocket
- SQLx (PostgreSQL)
- Redis (async)
- Tokio
- dotenvy
- HAProxy
- Docker Compose
- Vanilla HTML/CSS/JS frontend

---

This repository now demonstrates a complete distributed URL shortener workflow: deterministic shard routing, resilient cache/DB access, and real-time operational visibility from backend to frontend.