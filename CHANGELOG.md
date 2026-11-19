# Changelog
## [0.15.0] - 2026-11-19

### Bug Fixes

- *(ci)* El guard de release.yml acepta el arco arm64 pausado

### Documentation

- *(feature-032)* Documenta la migración de identidad visual
- *(plan-032)* Cierra el plan como completado

### Features

- *(ui)* Migra la identidad visual al cubo K oscuro/naranja

### Miscellaneous Tasks

- Elimina del .gitignore las entradas de artefactos LocalAI
- *(ci)* Pausa temporalmente el build arm64 de la imagen
- *(ui)* Alinea el copy de la web con la tagline del lockup
## [0.14.0] - 2026-11-18

### Features

- *(ui)* Portada pública con logo y barra lateral solo con sesión

### Miscellaneous Tasks

- Release v0.14.0
## [0.13.1] - 2026-11-18

### Bug Fixes

- *(docker)* Cargar el workspace completo en el build de la imagen
- *(docker)* Crear src dummy del miembro en la capa final

### Miscellaneous Tasks

- Release v0.13.1
## [0.13.0] - 2026-11-18

### Documentation

- Cerrar purpose de verificacion-e2e y actualizar estado del repositorio
- *(openspec)* Propuesta y plan de la portada pública (#118)
- *(openspec)* Propuesta y plan del MCP y tokens de API (#119)
- *(mcp)* Documentar el servidor MCP y cerrar tareas de la fase 2 (#119)

### Features

- *(ui)* Portada pública para visitantes sin sesión (#118)
- *(api)* Tokens de API con scopes para el servidor MCP (#119)
- *(mcp)* Servidor MCP (crate kesto-mcp) sobre rmcp (#119)

### Miscellaneous Tasks

- Ignorar artefactos de LocalAI ajenos al proyecto
- *(plans)* Añadir PLAN-028 de limpieza del repositorio
- Ignorar backends/ de LocalAI
- *(plans)* Cerrar PLAN-028
- *(openspec)* Archivar portada y MCP y cerrar PLAN-029/030
- Release v0.13.0
## [0.12.0] - 2026-11-17

### Documentation

- *(agents)* El handoff es local y no se versiona
- *(agents)* El handoff es local y no se versiona

### Features

- *(repositorios)* Salud por repositorio (estado, comprobación e indicador)

### Miscellaneous Tasks

- Sacar HANDOFF.md del control de versiones
- Release v0.12.0
## [0.11.0] - 2026-11-17

### Documentation

- Relevo tras v0.10.0 y la release del panel
- *(specs)* Archivar renders-y-resiliencia-del-panel y cerrar PLAN-025
- *(specs)* Proponer recogida-de-temporales-de-prueba (#94)
- *(specs)* Archivar recogida-de-temporales-de-prueba y cerrar PLAN-026

### Features

- *(panel)* Renders correctos y resiliencia ante un S3 caído

### Miscellaneous Tasks

- Handoff tras cerrar #99/#100/#101 y #94
- Release v0.11.0

### Testing

- *(pruebas)* El arnés recoge sus temporales y no agota los inodos
## [0.10.0] - 2026-11-16

### Bug Fixes

- *(restauracion)* El directorio restaurado deja el contenido en la raíz del destino (#86)
- *(e2e)* La batería ignorada mide el contrato, no el canal del motor (#86)
- *(e2e)* Esperar a que la base exista, no a que el servidor escuche
- *(db)* La 0007 desambigua los nombres repetidos antes de exigir unicidad
- *(db)* La migración descarta la conexión si no reenciende las claves foráneas

### Documentation

- Relevo tras v0.9.0 y el desbloqueo de release-prepare
- *(spec)* Nombre de tarea obligatorio y único, y renders del panel (#87 #89 #93)
- *(spec)* Propuesta de batería e2e de copia y restauración (#86)
- *(spec)* Archiva el cambio de la batería e2e y su plan (#86)
- Feature 023, comandos y entorno de la batería e2e (#86)
- *(spec)* La 0007 desambigua los homónimos y trata el blanco como ausente
- El §3 y los docstrings de `job_name`, al día con la 0007
- *(spec)* Archivar contratos-y-renders-del-panel
- Feature 024, tareas del change cerradas (#87 #89 #93)
- Cerrar el PLAN-024 (PR #96)

### Features

- *(tareas)* El nombre de la tarea es obligatorio y único (#87)
- *(ui)* Nombre obligatorio, renders del panel y poda de directorio (#87 #89 #93)

### Miscellaneous Tasks

- Job e2e con motor de contenedores y los tests ignorados (#86)
- Release v0.10.0

### Refactor

- *(ui)* Una sola condición de confirmación y limpieza de la auditoría (#93)

### Testing

- *(e2e)* Batería de punta a punta de copia y restauración (#86)
- *(db)* `nombres` resuelve el lote y la ausencia es falta de clave
## [0.9.0] - 2026-11-15

### Bug Fixes

- *(ci)* Descargar vampus fuera del árbol y acotar el commit de release

### Documentation

- *(plan)* Spec SDD y PLAN-022 del filtro por target en Restauraciones
- Relevo tras el cierre de #67 y v0.8.0 (issue #79 en curso)
- *(restauracion)* FEATURE-022, archivo del change y cierre del plan 022
- Relevo tras la entrega de #79 (PR #80 abierto)
- Relevo tras la entrega de #79 (release v0.9.0 pendiente)

### Features

- *(restauracion)* Selector de target en la pantalla de Restauraciones

### Miscellaneous Tasks

- Release v0.9.0

### Testing

- *(restauracion)* Fijar el filtro por target en la pantalla de Restauraciones
- *(restauracion)* Reforzar el reinicio de paginación por estado
## [0.8.0] - 2026-11-15

### Documentation

- Actualizar el relevo tras la release v0.7.0
- *(sdd)* Change ajustes-de-sidebar y PLAN-019 (issue #66)
- FEATURE-019, cierre del plan y archivo del change SDD
- *(sdd)* Change nombre-de-la-tarea y PLAN-020 (issue #65)
- FEATURE-020, cierre del plan y archivo del change SDD (issue #65)
- Relevo tras #65 (nombre de la tarea) y el chore de vampus
- *(restauracion)* Change SDD, plan 021 y FEATURE-021
- *(plan)* Marcar el cierre del plan 021 (PR #77)
- Relevo tras #67 (fase 2 completada, PR #77)

### Features

- *(db,api)* Nombre legible de la tarea en el esquema y en los listados (issue #65)
- *(ui)* Muestra el nombre de la tarea en Tareas, Histórico y Snapshots (issue #65)
- *(restauracion)* Restaurar bases de datos en su contenedor y persistir la ejecución

### Miscellaneous Tasks

- *(ci)* Descargar vampus precompilado en lugar de compilarlo
- Release v0.8.0

### Styling

- *(ui)* Mandos al pie de la barra lateral y marca oculta al plegar (issue #66)
- *(ui)* Renombra a «Target» la columna del target en Tareas
## [0.7.0] - 2026-11-15

### Documentation

- *(handoff)* Relevo de la FEATURE-015 y release v0.6.1
- *(handoff)* Relevo para la FEATURE-016 (identidad visual y tema)
- Glosar el nombre Kesto (en esperanto: arca, caja, cesto o cajón)
- *(sdd)* Change identidad-visual-y-tema y PLAN-016 (issue #60)
- FEATURE-016, archivo del change y cierre del PLAN-016
- *(handoff)* Relevo con la FEATURE-016 entregada y el lab con el panel nuevo
- Cerrar PLAN-017 y follow-ups de FEATURE-016
- FEATURE-018, cierre del plan y del change SDD
- Reflejar el PR #68 abierto en el plan y el relevo
- Actualizar el relevo tras el merge del PR #68

### Features

- *(ui)* Identidad de marca y tema claro/oscuro (oscuro por defecto)
- *(engine)* Restaurar instantáneas sobre el target de origen
- *(api)* Endpoint de restauración y listado con el target
- *(ui)* Restaurar desde el panel con confirmación y fase

### Miscellaneous Tasks

- *(ui)* Separar tsconfig de aplicación, tests y tooling
- Type-checkear los tests del panel
- *(db)* Índice sobre job_executions.snapshot_id
- Release v0.7.0

### Refactor

- *(ui)* Eliminar memoización redundante del tema

### Testing

- *(ui)* Añadir test de frontera de TypeScript
## [0.6.1] - 2026-11-14

### Bug Fixes

- *(copia)* No dejar la instantánea de una copia fallida y hacer legible el volcado restaurado

### Documentation

- Relevo tras release v0.6.0
- *(sdd)* Change proposal de higiene de la copia (#52, #53)
- *(handoff)* Relevo de FEATURE-015 (higiene de la copia)

### Miscellaneous Tasks

- *(sdd)* Archivar higiene-de-la-copia y cerrar PLAN-015 (#52, #53)
- Release v0.6.1
## [0.6.0] - 2026-11-14

### Bug Fixes

- *(sqlite)* Copia en solo lectura y erratas de spec (#11)
- *(ui)* Db_path como campo obligatorio de lectura (#11)

### Documentation

- FEATURE-012 del hotfix del mtime y relevo de sesión
- *(plan)* Renombrar PLAN-012 a COMPLETED (#41)
- Relevo de sesión (FEATURE-013 en development; #11 pendiente)
- Refrescar el relevo (hash de development y #44 cerrado)
- *(sdd)* Change proposal de SQLite como motor de copia (#11)
- *(sqlite)* SRS, compose de ejemplo y feature 014 (#11)
- *(sqlite)* Cierre del plan, tasks y relevo (#11)
- Relevo tras merge de FEATURE-014 (#11)

### Features

- *(deploy)* HSTS opcional (KESTO_HSTS_MAX_AGE) y receta just backup-datos
- *(sqlite)* Tipo de origen SQLite con copia en caliente (#11)
- *(ui)* Fuente SQLite en el panel de fuentes (#11)

### Miscellaneous Tasks

- *(openspec)* Archivar dockerfile-copy-mtime y fundir el delta en specs/deploy (#41)
- *(sdd)* Archivar sqlite-copy-engine y cerrar PLAN-014 (#11)
- Release v0.6.0
## [0.5.2] - 2026-11-13

### Bug Fixes

- *(release)* Invalidar la huella de Cargo tras COPY para que la imagen compile el binario real

### Documentation

- *(openspec)* Change proposal del hotfix del mtime con COPY (#41)

### Miscellaneous Tasks

- Release v0.5.2
## [0.5.1] - 2026-11-13

### Documentation

- *(release-image-rapida)* Change proposal, spec delta y PLAN-011
- Documentación del proyecto (AGENTS, GIT_FLOW, arquitectura, entorno) y assets
- *(release-image-rapida)* Doc de feature, cierre del plan y archivo del change

### Miscellaneous Tasks

- *(release)* Build nativo por arquitectura, manifest list y sccache
- Release v0.5.1

### Testing

- *(ci)* Prueba estructural del pipeline de release + receta just y CI
## [0.5.0] - 2026-11-13

### Documentation

- *(latest-tag-guard)* Cierra el change con doc de feature y specs

### Features

- *(ci)* Publicar latest solo en la release más alta

### Miscellaneous Tasks

- Permitir disparo manual de release-prepare (workflow_dispatch)
- Release v0.5.0
## [0.4.0] - 2026-11-13

### Documentation

- *(permiso-socket-daemon)* Cierra el change con doc de feature y specs

### Features

- *(deploy)* Group_add configurable por DOCKER_GID para el socket

### Miscellaneous Tasks

- Release v0.4.0
## [0.3.3] - 2026-11-13

### Miscellaneous Tasks

- Release v0.3.3
## [0.3.2] - 2026-11-13

### Dependencies

- *(ui)* Fuerza minimatch >=11.0.1 por GHSA-g3hv-75wg-m249

### Miscellaneous Tasks

- Release v0.3.2
## [0.3.1] - 2026-11-12

### Bug Fixes

- *(ui)* Aísla el formulario por fila y exige sesión en las rutas
- *(panel)* Cabeceras de seguridad y un único texto para el 404
- *(release)* Pasa KESTO_MASTER_KEY al smoke test de la imagen

### Documentation

- *(ui-panel)* Propuesta OpenSpec y plan 007 del panel web
- *(ui-panel)* Cierra el change con doc de feature y specs fusionadas
- *(despliegue-produccion)* Propuesta OpenSpec y plan 008
- *(despliegue-produccion)* Doc de la feature y spec al día
- *(despliegue-produccion)* Archiva el change y cierra el plan

### Features

- *(ui)* Andamiaje del panel React + Antd
- *(ui)* Servicios del panel — contrato, cliente, formato, SSE y sesión
- *(ui)* Layout, rutas y pantalla de sesión
- *(ui)* Dashboard y Tareas
- *(ui)* Histórico, Snapshots y Ajustes
- *(panel)* Sirve ui/dist desde Axum con fallback SPA
- *(deploy)* Dockerfile multi-stage y compose de producción

### Miscellaneous Tasks

- *(deploy)* Versiona .env.example pese a la regla .env.*
- Release v0.3.0
- Release v0.3.1
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
- Release v0.2.0

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
