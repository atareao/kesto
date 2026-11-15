# Proposal — restauracion-de-bases-de-datos

## Why

La fase 1 de la restauración (FEATURE-018) solo devuelve targets `directory`. Un target de base de datos —`mariadb`, `postgresql`, `sqlite`—, cuya **copia** sí sabe hacerse, no se puede restaurar: la API contesta `422` y el panel deshabilita la fila. Con eso, la mitad de las copias del producto no cierran el ciclo y el operador no tiene forma de recuperar una base desde el panel.

Además, la ejecución de restauración es hoy **efímera**: el `200` devuelve un `execution_id` que no se persiste, así que no queda rastro de qué se restauró, cuándo ni con qué desenlace. El issue #67 cubre las dos cosas.

## What Changes

- **Restauración de un target de BD dentro de su contenedor**, el reverso del `exec` de volcado del §4: `mariadb` y `psql` leen el volcado restaurado por `stdin`; `sqlite` reimporta su `.dump`. Sin fichero temporal: el volcado sale del repositorio con `Repository::dump` (que escribe en cualquier `impl Write`) y viaja por el `stdin` del `exec` (bollard `attach_stdin`, hoy sin usar a propósito).
- **Modo seguro por defecto**: para un target de BD la API exige `sobrescribir: true` en el cuerpo; sin él responde un precondition. El panel pide confirmación **tecleando el nombre del target** antes de enviar la bandera.
- **Ejecución de restauración persistida**: nueva tabla `restore_executions` (migración `0006`), el `execution_id` del `200` pasa a ser su fila, y un listado `GET /api/v1/restorations` con filtros y paginación.
- **Panel**: la acción de restaurar se levanta para los targets de BD —con la confirmación tipada— y una pantalla nueva de **Restauraciones** muestra el histórico.

## Fuera de alcance

- Restauración **parcial por rutas** y **borrado/purga** de snapshots (siguen fuera).
- Restauración a un **destino alternativo** o copia de seguridad previa automática: esta fase restaura sobre los datos vivos, autorizada por la bandera explícita.
- Progreso **incremental** de la restauración (`rustic_core` 0.13 sigue sin callback de progreso).

## Impacto

- Backend: `db` (tabla y migración `0006`), `api-resources` (endpoint y listado), `engine` (motor y persistencia), `docker` (entrada `stdin` en el transporte) y una capability **nueva** `restauracion` (el flujo inverso).
- Frontend: `ui` (pantalla de snapshots y pantalla nueva de restauraciones).
- SRS: se declaran **añadidos** el reverso del §4 y la tabla de restauraciones —el §3 modela la copia, no la restauración—.

## Referencias

- Issue #67 (fase 2), issue #64 (fase 1)
- `plans/PLAN-018-COMPLETED.md`, `plans/PLAN-021.md`
- `docs/FEATURE-018-restauracion-de-snapshots.md`, `docs/DEFINITION.md` §3-§5
