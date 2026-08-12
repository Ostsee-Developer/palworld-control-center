# DynaCat API draft

The DynaCat integration starts read-only and is served by the future local `palworldd` daemon.

## Transport

- local Unix socket by default
- optional HTTPS listener only on an explicitly configured management network
- scoped bearer tokens or mutual TLS for remote DynaCat access
- Server-Sent Events for status and job streams; WebSocket may be added only when bidirectional traffic is required

## Initial endpoints

```text
GET /v1/health
GET /v1/status
GET /v1/resources
GET /v1/players
GET /v1/mods
GET /v1/backups
GET /v1/jobs
GET /v1/events
```

No endpoint returns Palworld REST credentials, admin passwords or raw environment files.

Write operations will use dedicated scopes such as `server.restart`, `backup.create` and `settings.write`. They are not part of the read-only alpha.

