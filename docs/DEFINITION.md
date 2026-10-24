Aquí tienes la especificación técnica completa y actualizada para **Kesto**, incorporando **React + Ant Design (Antd)** en el Frontend y adaptando los componentes visuales a la biblioteca UI.

---

# Especificación Técnica de Software (SRS): Kesto

## 1. Visión General del Sistema

**Kesto** ("caja" o "contenedor" en esperanto) es una solución ligera en contenedor Docker para la automatización, orquestación y monitorización de copias de seguridad de bases de datos (MariaDB y PostgreSQL) y volúmenes de datos hacia almacenamiento S3 (RusticFS/MinIO/AWS S3).

El sistema utiliza `rustic_core` embebido en un motor escrito en Rust, interactúa directamente con el demonio de Docker del host mediante la API POSIX/socket, y expone un panel de control web reactivo desarrollado en **React** con la librería de componentes **Ant Design (Antd)**, alimentado en tiempo real mediante Server-Sent Events (SSE).

---

## 2. Arquitectura de Componentes

```
┌─────────────────────────────────────────────────────────────────────────┐
│                          KESTO CONTAINER                                │
│                                                                         │
│  ┌─────────────────┐      ┌───────────────────┐     ┌────────────────┐  │
│  │   Axum Web      │      │  Task Scheduler   │     │ Job Execution  │  │
│  │   REST / SSE    │──────│  (tokio-cron)     │────►│ Engine (Queue) │  │
│  └────────┬────────┘      └───────────────────┘     └───────┬────────┘  │
│           │                                                 │           │
│           ▼                                                 ▼           │
│  ┌─────────────────┐                              ┌──────────────────┐  │
│  │  SQLite (SQLx)  │                              │ Docker SDK       │  │
│  │  (App State)    │                              │ (bollard)        │  │
│  └─────────────────┘                              └────────┬─────────┘  │
│                                                            │            │
│                                                            ▼            │
│                                                   ┌──────────────────┐  │
│                                                   │   rustic_core    │  │
│                                                   └────────┬─────────┘  │
└────────────────────────────────────────────────────────────┼────────────┘
                                                             │
                                      ┌──────────────────────┴──────────────┐
                                      │                                     │
                                      ▼                                     ▼
                       ┌──────────────────────────────┐     ┌───────────────┐
                       │ Socket Docker Host           │     │ S3 Target     │
                       │ /var/run/docker.sock         │     │ (RusticFS)    │
                       └──────────────────────────────┘     └───────────────┘

```

### Componentes del Backend

* **Web Framework:** Axum 0.7+ (soporte nativo para Tokio, compatibilidad con SSE).
* **Base de Datos de Estado:** SQLite 3 vía `sqlx` (modo WAL activo, migraciones embebidas).
* **Motor de Repositorio:** `rustic_core` (vía crates directos: `rustic_core`, `rustic_backend`).
* **Integración Docker:** `bollard` con conexión por socket UNIX local (`/var/run/docker.sock`).
* **Planificador:** `tokio-cron-scheduler`.
* **Cola de Trabajo (Job Queue):** MPSC Channels de Tokio con un worker secuencial por repositorio S3 para garantizar el aislamiento de *locks*.

### Componentes del Frontend

* **Core Framework:** React 18+ con TypeScript.
* **UI Component Library:** Ant Design (Antd v5+).
* **Gestión de Estado y Data Fetching:** React Query (TanStack Query) + EventSource API para SSE.
* **Librería de Iconos:** `@ant-design/icons`.

---

## 3. Modelo de Datos (SQLite Schema)

```sql
-- Repositorios S3 de destino
CREATE TABLE IF NOT EXISTS repositories (
    id TEXT PRIMARY KEY NOT NULL, -- UUIDv4
    name TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    bucket TEXT NOT NULL,
    region TEXT NOT NULL,
    access_key_id TEXT NOT NULL,
    secret_access_key TEXT NOT NULL,
    passphrase TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Fuentes de Backup (Targets)
CREATE TABLE IF NOT EXISTS backup_targets (
    id TEXT PRIMARY KEY NOT NULL, -- UUIDv4
    name TEXT NOT NULL,
    target_type TEXT NOT NULL CHECK(target_type IN ('mariadb', 'postgresql', 'directory')),
    container_name TEXT,          -- Nullable si es un directorio local
    db_user TEXT,                 -- Requerido para DBs
    db_password TEXT,             -- Encrypted/Plain para DBs
    db_name TEXT,                 -- Opcional (NULL implica dump global)
    host_path TEXT,               -- Requerido para tipo 'directory'
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Tareas Programadas (Jobs)
CREATE TABLE IF NOT EXISTS backup_jobs (
    id TEXT PRIMARY KEY NOT NULL,
    target_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    cron_expression TEXT NOT NULL,
    keep_daily INTEGER NOT NULL DEFAULT 7,
    keep_weekly INTEGER NOT NULL DEFAULT 4,
    keep_monthly INTEGER NOT NULL DEFAULT 12,
    enabled BOOLEAN NOT NULL DEFAULT 1,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(target_id) REFERENCES backup_targets(id) ON DELETE CASCADE,
    FOREIGN KEY(repository_id) REFERENCES repositories(id) ON DELETE CASCADE
);

-- Histórico de Ejecuciones
CREATE TABLE IF NOT EXISTS job_executions (
    id TEXT PRIMARY KEY NOT NULL,
    job_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'success', 'failed')),
    snapshot_id TEXT,             -- ID devuelto por rustic_core
    bytes_processed INTEGER NOT NULL DEFAULT 0,
    bytes_uploaded INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    started_at DATETIME NOT NULL,
    finished_at DATETIME,
    FOREIGN KEY(job_id) REFERENCES backup_jobs(id) ON DELETE CASCADE
);

```

---

## 4. Definición de Flujos de Ejecución

### A. Extracción e Ingesta Directa por Streaming

La extracción de datos procesa la salida estándar (`stdout`) del proceso ejecutado dentro del contenedor de la base de datos y la reconduce a un buffer `AsyncRead` en memoria consumido directamente por `rustic_core`.

#### Comando Exec por Tipo de Servicio:

1. **MariaDB:**
```bash
mariadb-dump --single-transaction --quick --lock-tables=false -u<db_user> -p<db_password> --all-databases

```


2. **PostgreSQL:**
```bash
pg_dumpall -U <db_user>

```



```
[Target DB Container] 
       │ (stdout stream / bollard exec)
       ▼
[Tokio AsyncPipe (Memory Buffer)]
       │ (impl AsyncRead)
       ▼
[Rustic Core (NodeWriter / TreeBuilder)]
       │ (Chunking + Encryption AES-256-CTR)
       ▼
[RusticFS S3 Storage]

```

### B. Gestión de Retención y Prune

Posterior a cada snapshot exitoso, el motor de ejecuciones desencadena la purga de snapshots según los parámetros `keep_*`:

1. Adquisición del bloqueo exclusivo sobre el repositorio (`repository_id`).
2. Evaluación del árbol de snapshots utilizando las reglas de retención de `rustic_core`.
3. Eliminación de datos huérfanos mediante la ejecución interna del proceso `prune`.
4. Liberación del bloqueo del repositorio.

---

## 5. Especificación de API REST & SSE

### Endpoints

* `GET /api/v1/targets` — Lista de fuentes de datos configuradas.
* `POST /api/v1/targets` — Registrar nueva fuente (DB o directorio).
* `GET /api/v1/repositories` — Lista de repositorios S3.
* `POST /api/v1/repositories` — Inicializar o conectar repositorio S3/Rustic.
* `GET /api/v1/jobs` — Tareas programadas activas.
* `POST /api/v1/jobs` — Crear/editar tarea de backup (cron, target, repo, pruning).
* `POST /api/v1/jobs/:id/run` — Disparo manual inmediato de una tarea.
* `GET /api/v1/executions` — Consulta del historial de ejecuciones y métricas.
* `GET /api/v1/snapshots?repository_id=<id>` — Explorador de snapshots en el almacenamiento S3.
* `GET /api/v1/events` — Stream SSE (`text/event-stream`) de eventos globales en tiempo real.

### Estructura de Mensaje SSE

```json
{
  "event": "execution_progress",
  "data": {
    "execution_id": "f81d4fae-7dec-11d0-a765-00a0c91e6bf6",
    "job_id": "c9bf9e57-1685-4c89-bafb-ff5af830be8a",
    "status": "running",
    "bytes_processed": 104857600,
    "bytes_uploaded": 42107200,
    "percentage": 40.15,
    "current_action": "Uploading chunks to S3"
  }
}

```

---

## 6. Frontend y Experiencia de Usuario (React + Antd)

La interfaz se estructura mediante un Layout con menú lateral utilizando `antd`:

1. **Dashboard Principal:**
* **Tarjetas de Estadísticas (`Statistic` / `Card`):** Espacio consumido acumulado en S3, total de repositorios, total de respaldos activos.
* **Indicador de Estado en Vivo:** `Progress` (barra/círculo) mostrando el avance porcentual del backup en curso alimentado vía SSE.
* **Tabla de Tareas Activas (`Table`):** Lista con `Badge` de colores según el estado (`success`, `processing`, `error`).


2. **Gestión de Tareas y Formularios (Modal / Drawer / Form):**
* **Selector de Contenedores (`Select`):** Autocompletado con la lista de contenedores activos detectados vía Docker Socket.
* **Generador Cron (`Form.Item`):** Selector visual con opciones comunes o entrada directa de expresión cron.
* **Parámetros de Prune (`InputNumber`):** Configuración intuitiva de `keep-daily`, `keep-weekly`, `keep-monthly`.
* **Credenciales (`Input.Password`):** Para claves secretas S3 y frase de paso del repositorio.


3. **Histórico y Consola de Eventos:**
* **Línea de Tiempo (`Timeline` / `List`):** Histórico de ejecuciones con fechas, duraciones y consumo de datos.
* **Visor de Logs (`Typography.Text` en bloque de código):** Muestra el output en tiempo real del proceso `docker exec` capturado vía SSE.


4. **Explorador de Snapshots (S3 Browser):**
* **Tabla de Snapshots (`Table` con `Tag` y `Space`):** ID del snapshot, etiquetas asignadas, fecha de creación, tamaño total y botón de acción para purga manual.



---

## 7. Estructura de Proyecto Recomendada

```text
kesto/
├── Cargo.toml
├── src/                  # Backend en Rust (Axum)
│   ├── main.rs
│   ├── api/              # Rutas REST y SSE
│   ├── db/               # SQLite + SQLx
│   ├── docker/           # Bollard (Docker Socket exec)
│   ├── engine/           # Tokio Job Queue
│   ├── rustic/           # Wrapper sobre rustic_core
│   └── scheduler/        # tokio-cron-scheduler
├── ui/                   # Frontend en React + Antd
│   ├── package.json
│   ├── src/
│   │   ├── components/   # Layout, Stats, StatusBadge
│   │   ├── pages/        # Dashboard, Jobs, Snapshots, Settings
│   │   ├── services/     # API Client & SSE Listener
│   │   └── App.tsx
│   └── vite.config.ts
└── Dockerfile            # Multi-stage build (UI build + Rust binary)

```

---

## 8. Despliegue en Producción (Docker Compose)

```yaml
services:
  kesto:
    image: kesto:latest
    container_name: kesto-app
    restart: always
    ports:
      - "8080:8080"
    environment:
      - RUST_LOG=info,kesto=debug
      - DATABASE_URL=sqlite:///app/data/kesto.db
      - BIND_ADDRESS=0.0.0.0:8080
    volumes:
      # Acceso al socket de Docker del host
      - /var/run/docker.sock:/var/run/docker.sock:ro
      # Directorios del host a respaldar (modo lectura)
      - /var/www:/backup_sources/var_www:ro
      - /etc/nginx:/backup_sources/nginx_config:ro
      # Persistencia interna de la app (DB SQLite)
      - kesto_data:/app/data

volumes:
  kesto_data:

```
