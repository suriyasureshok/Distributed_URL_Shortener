# Shorten.io: Distributed URL Shortener (Rust + Actix)

> This project is a distributed URL shortener that combines deterministic routing, Redis cache sharding, PostgreSQL shard pairs, and realtime WebSocket observability.

## Highlights

- Consistent hash routing (Redis shard and DB shard alignment)
- Write path: DB primary + Redis cache population
- Read path: Redis first, then replica, then primary fallback
- Retry + timeout + fallback for Redis and DB operations
- Redis-backed rate limiting
- WebSocket event stream for reads/writes
- Backend-owned cache hit/miss counters and rates
- Frontend dashboard that only displays backend-provided cache metrics

## Architecture Diagram

![Architecture Diagram](diagrams\URL_Shortener_v2.png)

## Project Structure

```text
src/
  main.rs                # Composition root: wiring, startup, routes
  bootstrap.rs           # Env parsing + DB/Redis/ring initialization
  handlers.rs            # HTTP handlers (create/read orchestration)
  storage.rs             # Redis/DB access with retry/fallback
  events.rs              # WebSocket sessions + Redis pub/sub fanout
  domain.rs              # Hash ring, Snowflake, routing helpers
  cache_metrics.rs       # Cache metrics abstraction + atomic tracker
  state.rs               # Shared AppState
  models.rs              # Request/response models + aliases
  reliability.rs         # Retry/timeout constants + backoff

frontend/
  index.html             # Dashboard shell
  styles.css             # Dashboard styling
  app.js                 # UI controller + websocket client
  cache-metrics-service.js  # Backend metrics parsing/state store
```

## Core APIs

- POST /create
- GET /{short_code}
- GET /ws

## Request Flows

### Write flow (POST /create)

1. Rate-limit key check in Redis
2. Generate short code with Snowflake + Base62
3. Route key using hash ring
4. Insert into DB primary
5. Best-effort populate Redis cache
6. Broadcast write event over WebSocket/pubsub

### Read flow (GET /{short_code})

1. Route key using hash ring
2. Attempt Redis read
3. On miss, attempt DB replica, then DB primary fallback
4. If DB hit, refill Redis
5. Increment backend cache metrics:
   - cache HIT if Redis returned value
   - cache MISS if Redis missed
6. Broadcast read event including cache metrics snapshot

## WebSocket Event Schema

Common fields:

- event_id
- type (write | read)
- status (OK | ERROR | NOT_FOUND)
- node
- redis
- db
- short_code
- latency_ms

Read event fields:

- cache (HIT | MISS)
- source (redis | replica | primary | none | error)

Backend cache metrics fields (read events):

- cache_hit_count
- cache_miss_count
- cache_total
- cache_hit_rate
- cache_miss_rate
- cache_metrics object with the same values

## Configuration

Environment variables:

- DATABASE_NODES (preferred) or DB_NODES
  - comma-separated URLs
  - must contain even count (primary,replica per shard)
- REDIS_NODES
  - comma-separated URLs
- PUBLIC_BASE_URL (optional)

Routing alignment rule:

- DB shard count = DB_NODES count / 2
- Redis node count = REDIS_NODES count
- counts must match

## Local Run

### Prerequisites

- Rust toolchain
- PostgreSQL instances available for configured DB_NODES pairs
- Redis instances available for configured REDIS_NODES

### Start backend

```bash
cargo run
```

Backend binds to 127.0.0.1:8080.

### Start frontend

```bash
cd frontend
python -m http.server 5500
```

Open http://localhost:5500.

## Build Commands

```bash
cargo check
cargo build --release
```

## Docker Compose Notes

The repository includes docker-compose.yml and haproxy.cfg. Keep these aligned:

- backend listen address/port
- compose exposed/internal ports
- HAProxy backend target ports
- DB/Redis hostnames (service names in containers, not localhost)

### Start only Redis and DB services

```bash
docker-compose up -d redis0 redis1 redis2 db0 db1 db2
```

### Check status

```bash
docker-compose ps redis0 redis1 redis2 db0 db1 db2
```

### Clear Redis and DB data (PowerShell)

```powershell
$redisContainers = @('url_shortener-redis0-1','url_shortener-redis1-1','url_shortener-redis2-1'); foreach ($c in $redisContainers) { docker exec $c redis-cli FLUSHALL }
$dbContainers = @('url_shortener-db0-1','url_shortener-db1-1','url_shortener-db2-1'); foreach ($c in $dbContainers) { docker exec $c psql -U postgres -d shortener_db -c "CREATE TABLE IF NOT EXISTS urls (id BIGINT PRIMARY KEY, short_code TEXT UNIQUE, long_url TEXT UNIQUE NOT NULL); TRUNCATE TABLE urls RESTART IDENTITY;" }
```

### Stop only Redis and DB services

```bash
docker-compose stop redis0 redis1 redis2 db0 db1 db2
```

## Troubleshooting

### cargo run exits with "AddrInUse"

Port 8080 is already occupied.

PowerShell quick check:

```powershell
Get-NetTCPConnection -LocalPort 8080 -State Listen
```

Stop the owning process (replace PID):

```powershell
Stop-Process -Id <PID> -Force
```

Then run cargo run again.

# Current Limitations

- WebSocket fanout is process-local (no cross-in **Keep** Undo)  
- DB primary/replica is modeled by URL pairing, but real replica  
- No automatic ring rebalancing/migration workflow when adding  
- Rate limiter is basic key-window logic and currently keyed b  
- Data reconciliation and repair jobs
- Rebalancing/migration tooling for dynamic shard changes
- Advanced per-IP or token bucket rate limiting
- OpenTelemetry metrics + tracing
- Authenticated custom aliases and abuse protection

# Future Enhancements

- Cross-instance event bus (Redis pub/sub or NATS) for unified  
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

This repository now reflects a modular backend architecture with explicit boundaries and backend-owned observability metrics that are consumed directly by the frontend.
