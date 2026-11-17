# Design

## Context

Ver `proposal.md` — Why. Estado actual: ni la entidad `Repositorio` (`src/db/entities/repository.rs`) ni la tabla `repositories` (`migrations/0001_init.sql`) tienen campo de salud, y `src/api/repositories.rs` solo expone CRUD. Ya existe la primitiva que distingue los desenlaces del destino: poner en marcha el repositorio (bajo el tope de 10 s) con los desenlaces «inicializado/creado» y «conectado», y el listado de instantáneas. El motor envuelve ambas. En el frontend, `ui/src/hooks/useEspacioEnS3.ts` ya modela estado por repositorio con fallo parcial.

## Goals / Non-Goals

**Goals:**

- Estado de salud por repositorio, persistido y servido por la API.
- Comprobación real bajo demanda (por repositorio y global).
- Indicador honesto en Ajustes y en el listado del Dashboard.

**Non-Goals:**

- Comprobación periódica en segundo plano (scheduler): fuera de este change.
- Histórico de comprobaciones: solo se guarda la última.
- Reintentos o remarcado automático de un repositorio caído.

## Decisions

### D1 — Estado persistido, no calculado al vuelo

Se persisten estado, fecha y motivo en `repositories`. Alternativas: campo derivado en el JSON (obligaría a comprobar en cada listado y arrastraría el todo-o-nada); estado solo en el frontend (se pierde al recargar). El operador eligió persistido.

### D2 — La comprobación es de solo lectura: no inicializa

La puesta en marcha normal **inicializa** el destino si no hay configuración, lo que **crearía** un repositorio al comprobarlo. La comprobación debe consultar el destino y distinguir «no hay configuración» sin inicializarlo. Alternativa considerada: reutilizar la puesta en marcha tal cual (muta el destino; rechazada).

### D3 — Profundidad: poner en marcha + listar instantáneas

Elegida por el operador: detecta caído, passphrase errónea y también un destino alcanzable sin permiso de lectura. Alternativa: solo poner en marcha (no detecta destinos alcanzables pero inutilizables).

### D4 — Endpoint por repositorio y global; el listado no comprueba

`POST` (acción con efecto de escritura, pues persiste) en `/{id}/health` y en `/health`. El `GET` del recurso sirve el estado persistido y NO dispara comprobaciones.

### D5 — Motivo saneado y un único traductor

El mapeo de errores tipados a estado y el saneado del motivo viven en un solo traductor; el motivo no contiene credenciales ni el texto crudo de la dependencia.

## Risks / Trade-offs

- Coste del endpoint global: N repositorios × tope de 10 s. Se documenta.
- Estado rancio: la fecha de comprobación lo delata y el panel la muestra.
- Fuga de secretos en el motivo: saneado + test que lo verifica.

## Open Questions

(none)
