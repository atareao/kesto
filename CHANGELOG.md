# Changelog
## [0.2.0] - 2026-11-11

### Bug Fixes

- *(api,auth)* Envuelve el rechazo del extractor y candadea la vigencia
- *(just)* Arregla las dos recetas rotas de openspec
- *(auth)* Tolera un desfase de reloj en el iat del ID token
- *(db)* Cierra los nueve hallazgos de la auditoría del cifrado
- *(db)* Cierra los hallazgos de la auditoría de las entidades
- *(api)* El rechazo del cuerpo sale por la envoltura y no publica la credencial
- *(api)* Los manejadores dejan traza de lo que ocurre, y nunca del cuerpo
- *(engine)* Cierra los dos hallazgos de la auditoria del transporte
- *(rustic)* Cierra los siete hallazgos de la auditoria del puente
- *(rustic)* [**breaking**] Rediseña el puente y lleva al contrato los dos fallos de archivado
- *(rustic)* Cierra los diez hallazgos de la auditoria del repositorio
- *(engine)* Cierra los seis hallazgos de la auditoria de la extraccion
- *(engine)* Cierra los dos hallazgos que bloqueaban el modulo 4
- *(engine)* Cierra la parada colgada y los seis hallazgos de la auditoria
- *(engine)* Espera a la boca del servidor antes de abrir el stream
- *(scheduler)* El tick real usa el job_id de Kesto y la puerta de parada cierra siempre

### Documentation

- Change proposal bootstrap-core
- Change proposal auth-capability
- *(auth)* Corrige la ruta que el comentario del 1.38 apuntaba mal
- *(plans)* Abre el plan 002, el de auth-capability
- *(auth)* Escribe el documento de la feature 002
- *(plan-002)* Tilda el PR #7 y pone las cifras en 93 tests
- *(openspec)* Abre el change db-schema con su spec y su plan
- *(plan-003)* El orden de entrega de los módulos es 1 → 3 → 2
- *(plan-003)* Tilda el PR #8, su CI y las referencias al change
- *(api-endpoints)* Declara el añadido al SRS y cierra la auditoría del módulo 1
- *(api-endpoints)* Cierra la contabilidad del módulo 2 y corrige una decisión del 1
- *(api-endpoints)* Cierra la contabilidad del módulo 3
- *(api-endpoints)* Cierra las fases 3 y 4 con la evidencia medida
- *(plan-004)* Tilda el PR #9, su CI y las referencias al change
- *(engine)* El change backup-engine, con 28 requisitos, 89 escenarios y su plan
- *(engine)* Lleva al contrato los hallazgos de la auditoria del archivado
- *(engine)* Declara el cierre del reparto en el enunciado del cierre ordenado
- *(engine)* Quita lo que tres textos prometian de mas
- *(engine)* El cierre del reparto llega al diseno y a la propuesta, y 78 tareas quedan tildadas
- *(engine)* Archiva backup-engine, fusiona sus specs y pone el estado al día
- *(scheduler)* El change proposal y el plan del reloj que faltaba
- *(scheduler)* La fase 0 medida enmienda D3 y la parada del requisito 6
- *(scheduler)* El plan y las tareas al día tras la fase 2
- *(scheduler)* El documento de la feature y el plan cerrado
- *(openspec)* Archiva el change scheduler y publica sus capacidades

### Features

- *(config,secrets,api-shell)* Arranque, configuración y envoltura HTTP
- *(error,auth)* La sesión como JWT firmado y el 401/502 del modelo de error
- *(api,auth)* El gate de sesión y la partición del router
- *(auth)* El descubrimiento del proveedor y la redirección de entrada
- *(auth)* El callback, el canje del codigo y la verificacion del ID token
- *(db)* Abre el pool de SQLite con WAL, claves foráneas y migraciones
- *(db)* Sobre v1:<nonce>:<cifrado> con HKDF-SHA256 y AES-256-GCM
- *(db)* Entidad y capa de acceso de repositories
- *(db)* Entidades y capa de acceso de targets, tareas y ejecuciones
- *(api)* Andamiaje del RED del módulo 1 y el change api-endpoints
- *(api)* Módulo 1 del change api-endpoints en verde
- *(api)* /repositories completo, con el listado, la actualización y el borrado
- *(api)* /targets completo, con la coherencia del tipo en el borde HTTP
- *(api)* /jobs completo, con la validación de referencias antes de escribir
- *(api)* /executions completo, con filtros, ventana acotada y orden estable
- *(docker)* El transporte de la API, modulo 1 del change backup-engine
- *(rustic)* El puente del volcado, primera mitad del modulo 2
- *(rustic)* El repositorio y la retencion, segunda mitad del modulo 2
- *(engine)* La extraccion, primera mitad del modulo 3
- *(db)* La guarda atomica del 409, en una sola sentencia
- *(engine)* El motor, la segunda mitad del módulo 3
- *(engine)* La capa HTTP del motor, la ultima pieza del change
- *(scheduler)* El reloj que faltaba — validación, planificación, reconciliación y disparo
- *(api)* El alta y la edición de una tarea validan su expresión cron

### Miscellaneous Tasks

- *(openspec)* Archiva bootstrap-core y cierra el plan 001
- *(openspec)* Archiva auth-capability y cierra el plan 002
- *(openspec)* Marca el módulo 1 de db-schema, 34 tareas y 6 de operador
- *(openspec)* Marca la fase 3 (REFACTOR) y la fase 4 (operador)
- *(openspec)* Archiva db-schema y cierra el plan 003
- *(openspec)* Archiva api-endpoints y cierra el plan 004
- *(engine)* Resuelve el arbol con bollard y rustic, y commitea el lock
- *(scheduler)* Las dos dependencias del reloj y su lock resuelto con red

### Testing

- *(auth)* Candadea los ataques del ID token y el registro del flujo
- *(test_support)* Captura el registro con un suscriptor global por hilo
- *(api)* Las propiedades del cable y la conservación del secreto en /targets
- *(api)* La partición del router sobre las quince rutas nuevas
- *(api)* El Debug del DTO del cable no publica la credencial
- *(engine)* Deja de competir con el trabajador al mirar el estado
- *(engine)* Deja de exigir que el nacimiento sea el primer evento
- *(arranque)* El planificador arranca detrás del motor y se para antes que él
## [0.1.0] - 2026-10-24

### Documentation

- Add README and MIT license
- Documentar el pipeline en AGENTS.md y GIT_FLOW.md
- Documentar las recetas del .justfile
- Reescribir el README con el estado real del proyecto

### Miscellaneous Tasks

- Bootstrap project base
- Prepare release tooling
- Add CI, release and container image pipelines
- *(ci)* Quitar la expresión no interpolada del nombre del job frontend
- Excluir los metadatos del repo del crate publicado
- Añadir .justfile con las recetas de desarrollo
- Añadir gf-merge al .justfile
- Release v0.1.0
