import type {
  AltaJob,
  AltaRepository,
  AltaTarget,
  Estado,
  EventoDeEjecucion,
  EjecucionEncolada,
  Execution,
  Health,
  Job,
  Repository,
  Sesion,
  Snapshot,
  Target,
} from './types'

// Estas pruebas son el candado de los literales del cable: si alguien cambia
// una variante del enum, el `satisfies` deja de compilar y `tsc` falla antes
// que el backend y el panel se desalineen en silencio.

describe('tipos del cable', () => {
  it('los cuatro estados de una ejecución son los del backend', () => {
    const estados = ['pending', 'running', 'success', 'failed'] satisfies Estado[]

    expect(estados).toHaveLength(4)
    expect(new Set(estados).size).toBe(4)
  })

  it('los tipos de target son los tres que sabe extraer el motor', () => {
    const tipos = ['directory', 'mariadb', 'postgresql'] satisfies Array<Target['target_type']>

    expect(tipos).toHaveLength(3)
  })

  it('las lecturas traen el booleano del secreto y nunca el secreto', () => {
    const target: Target = {
      id: '0199f0aa-0000-7000-8000-000000000000',
      name: 'web',
      target_type: 'mariadb',
      container_name: 'mariadb',
      db_user: 'root',
      has_db_password: true,
      db_name: 'wordpress',
      host_path: null,
      created_at: '2026-01-02T03:04:05Z',
    }
    const repository: Repository = {
      id: '0199f0aa-0000-7000-8000-000000000001',
      name: 'b2',
      endpoint: 'https://s3.eu-central-003.backblazeb2.com',
      bucket: 'kesto',
      region: 'eu-central-003',
      access_key_id: 'AKIA',
      has_secret_access_key: true,
      has_passphrase: true,
      created_at: '2026-01-02T03:04:05Z',
    }

    // El secreto solo existe como booleano en la lectura: el objeto no tiene
    // ninguna propiedad con el valor.
    expect(Object.keys(target)).not.toContain('db_password')
    expect(Object.keys(repository)).not.toContain('secret_access_key')
    expect(Object.keys(repository)).not.toContain('passphrase')
  })

  it('los tipos de escritura sí llevan los campos secretos, opcionales', () => {
    const target: AltaTarget = { name: 'web', target_type: 'directory', host_path: '/srv/web' }
    const repository: AltaRepository = {
      name: 'b2',
      endpoint: 'https://s3.example',
      bucket: 'kesto',
      region: 'eu-central-003',
      access_key_id: 'AKIA',
    }

    expect(target.db_password).toBeUndefined()
    expect(repository.secret_access_key).toBeUndefined()
    expect(repository.passphrase).toBeUndefined()
  })

  it('el resto de DTOs encajan con el JSON del backend', () => {
    const job: Job = {
      id: 'j',
      target_id: 't',
      repository_id: 'r',
      cron_expression: '0 3 * * *',
      keep_daily: 7,
      keep_weekly: 4,
      keep_monthly: 6,
      enabled: true,
      created_at: '2026-01-02T03:04:05Z',
    }
    const altaJob: AltaJob = {
      target_id: 't',
      repository_id: 'r',
      cron_expression: '0 3 * * *',
      keep_daily: 7,
      keep_weekly: 4,
      keep_monthly: 6,
      enabled: true,
    }
    const encolada: EjecucionEncolada = { id: 'e', job_id: 'j', status: 'pending' }
    const execution: Execution = {
      id: 'e',
      job_id: 'j',
      status: 'running',
      snapshot_id: null,
      bytes_processed: 1024,
      bytes_uploaded: 512,
      error_message: null,
      started_at: '2026-01-02T03:04:05Z',
      finished_at: null,
    }
    const snapshot: Snapshot = {
      id: 'a'.repeat(64),
      time: '2026-01-02T03:04:05Z',
      paths: ['/srv/web'],
      label: 'manual',
      files: 12,
      bytes: 4096,
    }
    const health: Health = { version: '0.1.0', auth_mode: 'disabled' }
    const sesion: Sesion = { sub: 'u', email: 'u@example.org' }
    const evento: EventoDeEjecucion = {
      execution_id: 'e',
      job_id: 'j',
      status: 'running',
      bytes_processed: 10,
      bytes_uploaded: 5,
      percentage: null,
      current_action: 'subiendo',
    }

    expect([job.enabled, altaJob.keep_daily, encolada.status]).toEqual([true, 7, 'pending'])
    expect([execution.finished_at, snapshot.files, health.auth_mode, sesion.email, evento.percentage]).toEqual([
      null,
      12,
      'disabled',
      'u@example.org',
      null,
    ])
  })
})
