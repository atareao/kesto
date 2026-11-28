# Optional container stop/start before backup

## Why

Issue #210. Add optional capability to stop one or more Docker containers that point to a database before a backup runs, then restart them after the backup completes, so that the snapshot is fully consistent without leaving containers stopped.

## What Changes

- Configuración `KESTO_STOP_CONTAINERS` (sin bandera CLI: el servidor no tiene comando `backup`)
- List of container names/IDs
- Bollard stop/start integration
- Error handling
- Documentation
- Tests

## Summary

Add optional capability to stop one or more Docker containers that point to a database before a backup runs, then restart them after the backup completes.

## Motivation

Ensure a fully consistent snapshot without leaving containers stopped.

## Scope

- Configuración `KESTO_STOP_CONTAINERS` (sin bandera CLI: el servidor no tiene comando `backup`)
- List of container names/IDs
- Bollard stop/start integration
- Error handling
- Documentation
- Tests

## Alternatives considered

- No container stop (current behavior)
- Using DB-native point-in-time recovery

## Impacts

Backend only (Docker client + engine), documentation.

## Seguimiento

- Plan: `plans/PLAN-056.md`.
- Documentación de la feature: `docs/FEATURE-056-backup-container-stop.md`.
