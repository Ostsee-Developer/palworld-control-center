# DynaCat API

The first integration is read-only and is served directly by the Control Center on an optional local Unix socket. A separately privileged `palworldd` daemon remains the target for future scoped write operations.

## Transport

- local Unix socket only in the functional alpha
- socket mode `0660`; access is controlled by its owning group
- one small HTTP/1.1 request per connection
- `Cache-Control: no-store` on every response
- no TCP listener, TLS termination or public exposure

## Initial endpoints

```text
GET /v1/health
GET /v1/status
GET /v1/resources
GET /v1/players
GET /v1/settings
GET /v1/mods
GET /v1/backups
GET /v1/jobs
GET /v1/events
```

Player objects contain only display name, level, ping and building count. Settings marked secret are omitted. No endpoint returns IP addresses, full player identifiers, Palworld REST credentials, admin/server passwords, raw logs or raw environment files.

Example:

```bash
curl --unix-socket /run/palworld-control-center/dynacat.sock \
  http://localhost/v1/status
```

Unknown paths return `404`; every non-GET request returns `405`. Future write operations will use a separate daemon and dedicated scopes such as `server.restart`, `backup.create` and `settings.write`. They are not accepted by this socket.
