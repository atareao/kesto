-- Añadido al SRS: el §3 declara tres `ON DELETE CASCADE` y ningún índice.
--
-- SQLite **no** crea índices para las claves foráneas. Sin índice en la columna
-- hija, cada borrado en cascada recorre la tabla entera; con el histórico de
-- ejecuciones creciendo a cada copia, ese recorrido es el que convierte un
-- `DELETE` en un problema. Los tres primeros índices son las columnas hijas.
--
-- El cuarto no es de clave foránea: cubre el listado del dashboard —las
-- ejecuciones de un estado, ordenadas por fecha—, que sin él ordena recorriendo.

CREATE INDEX IF NOT EXISTS idx_backup_jobs_target_id
    ON backup_jobs(target_id);

CREATE INDEX IF NOT EXISTS idx_backup_jobs_repository_id
    ON backup_jobs(repository_id);

CREATE INDEX IF NOT EXISTS idx_job_executions_job_id
    ON job_executions(job_id);

CREATE INDEX IF NOT EXISTS idx_job_executions_status_started_at
    ON job_executions(status, started_at);
