/**
 * Constructores de DTO para las pruebas de pantalla. Rellenan los campos que no
 * importan a la prueba con valores válidos del contrato, para que cada test solo
 * escriba lo que está afirmando.
 */

import type {
  Execution,
  Job,
  Repository,
  Snapshot,
  Target,
} from '../services/api/types'

export function repositorio(campos: Partial<Repository> & { id: string }): Repository {
  return {
    name: campos.id,
    endpoint: 'https://s3.example.com',
    bucket: 'copias',
    region: 'eu-west-1',
    access_key_id: 'AKIA',
    has_secret_access_key: true,
    has_passphrase: true,
    created_at: '2026-01-01T00:00:00Z',
    ...campos,
  }
}

export function tarea(campos: Partial<Job> & { id: string }): Job {
  return {
    target_id: 't1',
    repository_id: 'r1',
    cron_expression: '0 0 3 * * *',
    keep_daily: 7,
    keep_weekly: 4,
    keep_monthly: 6,
    enabled: true,
    created_at: '2026-01-01T00:00:00Z',
    ...campos,
  }
}

export function target(campos: Partial<Target> & { id: string }): Target {
  return {
    name: campos.id,
    target_type: 'directory',
    container_name: null,
    db_user: null,
    has_db_password: false,
    db_name: null,
    host_path: '/srv/datos',
    created_at: '2026-01-01T00:00:00Z',
    ...campos,
  }
}

export function ejecucion(campos: Partial<Execution> & { id: string; job_id: string }): Execution {
  return {
    status: 'success',
    snapshot_id: 'a'.repeat(64),
    bytes_processed: 1024,
    bytes_uploaded: 1024,
    error_message: null,
    started_at: '2026-01-02T03:00:00Z',
    finished_at: '2026-01-02T03:05:00Z',
    ...campos,
  }
}

export function snapshot(campos: Partial<Snapshot> & { id: string }): Snapshot {
  return {
    time: '2026-01-02T03:00:00Z',
    paths: ['/srv/datos'],
    label: null,
    files: 10,
    bytes: 1024,
    ...campos,
  }
}
