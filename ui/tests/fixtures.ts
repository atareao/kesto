/**
 * Constructores de DTO para las pruebas de pantalla. Rellenan los campos que no
 * importan a la prueba con valores válidos del contrato, para que cada test solo
 * escriba lo que está afirmando.
 */

import type {
  Execution,
  Job,
  Repository,
  RestauracionRegistrada,
  Snapshot,
  Target,
} from '../src/services/api/types'

export function repositorio(campos: Partial<Repository> & { id: string }): Repository {
  return {
    name: campos.id,
    endpoint: 'https://s3.example.com',
    bucket: 'copias',
    region: 'eu-west-1',
    access_key_id: 'AKIA',
    has_secret_access_key: true,
    has_passphrase: true,
    // Sin comprobar: el defecto del contrato es «desconocido» con la fecha a
    // nulo, así que quien afirme salud la pasa en `campos`.
    health: 'desconocido',
    health_checked_at: null,
    health_detail: null,
    created_at: '2026-01-01T00:00:00Z',
    ...campos,
  }
}

export function tarea(campos: Partial<Job> & { id: string }): Job {
  return {
    // El contrato exige un nombre no vacío: el defecto es uno válido y quien
    // afirme otro lo pasa en `campos`.
    name: 'copia de prueba',
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
    db_path: null,
    created_at: '2026-01-01T00:00:00Z',
    ...campos,
  }
}

export function ejecucion(campos: Partial<Execution> & { id: string; job_id: string }): Execution {
  return {
    // La tarea de la ejecución no tiene nombre por defecto; quien afirme el
    // nombre lo pasa en `campos`.
    job_name: null,
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
    change_manifest: null,
    // Igual que en la ejecución: sin tarea con nombre, la clave va a `null`.
    job_name: null,
    files: 10,
    bytes: 1024,
    // El listado publica el target de origen de cada instantánea; el defecto es
    // el caso que ofrece restaurar, y quien pruebe otra cosa lo pone a mano.
    target_id: 't1',
    target_name: 'web',
    target_type: 'directory',
    ...campos,
  }
}

export function restauracionRegistrada(
  campos: Partial<RestauracionRegistrada> & { id: string },
): RestauracionRegistrada {
  return {
    snapshot_id: 'a'.repeat(64),
    // El target de origen del histórico: su nombre y su tipo los resuelve el
    // backend; quien pruebe un target borrado los pone a `null` con `target_id`.
    target_id: 't1',
    target_name: 'web',
    target_type: 'directory',
    // El histórico enseña el desenlace real: por defecto, una restauración
    // terminada sin motivo de error.
    status: 'success',
    error_message: null,
    started_at: '2026-01-02T03:00:00Z',
    finished_at: '2026-01-02T03:05:00Z',
    ...campos,
  }
}
