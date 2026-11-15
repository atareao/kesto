# Tasks — restauracion-de-bases-de-datos

> Spike 0.1 ya resuelto: `rustic_core` 0.13 expone `Repository::dump(&Node, &mut impl Write)` y `bollard` 0.18.1 soporta `attach_stdin`; el streaming repo→stdin es viable sin fichero temporal.

## 0. Preparación

- [x] 0.1 `just check-all` en verde antes de tocar código.
- [x] 0.2 Migración `0006_restauraciones.sql` con la tabla `restore_executions`, su `CHECK`, su FK y su índice.
- [x] 0.3 Entidad `Restauracion` + capa de acceso (`alta`, `actualizar`, `listar_filtrado`) en `src/db/entities/`.
- [x] 0.4 Fijar con tests que fallen los contratos: tabla/índice, comando inverso por tipo, `sobrescribir`, `428`, listado y panel.

## 1. RED

- [x] 1.1 Test db: `restore_executions` existe con su `CHECK`, su FK y su índice; estado inválido rechazado; borrar target borra su histórico.
- [x] 1.2 Test `restauracion`: el comando inverso de `mariadb` (con y sin `db_name`) y de `postgresql` (`postgres` / base nombrada) es el reverso del volcado; la contraseña va al entorno y no al `cmd`; sin contraseña no se añade variable.
- [x] 1.3 Test `restauracion`: `sqlite` retira `<db_path>`/`-wal`/`-shm` antes de reimportar con `sqlite3`.
- [x] 1.4 Test `restauracion`: el volcado sale por `Repository::dump` a un `Write` puente acotado y llega al `stdin`; no hay fichero temporal; el `stdin` se cierra después del último byte. **Hecho**: cinco tests del puente en `src/engine/restauracion.rs` (orden y totalidad, cota de memoria, volcado a medias → error, cierre sin final → truncado, `Write` sin lector → `BrokenPipe`), dos de `volcar_a` en `src/rustic/repositorio.rs` y uno de las opciones del `exec` en `src/docker/cliente.rs`; el de punta a punta con motor va `#[ignore]`.
- [x] 1.5 Test API: `POST …/restore` de un target de base **sin** `sobrescribir` → `428`, sin ejecución ni aviso.
- [x] 1.6 Test API: `POST …/restore` de un target de base **con** `sobrescribir: true` → `200` con `execution_id` persistido; se publica `restore_progress`.
- [x] 1.7 Test API: la fila de `restore_executions` existe en `pending` antes del `200` y acaba en `success`/`failed` con `error_message`. **Hecho**: las dos mitades están cubiertas en su nivel —el `pending` **antes del `200`** lo fija `api::snapshots::tests::restaurar_un_target_de_base_con_confirmacion_persiste_la_ejecucion`, que lee la fila por el `execution_id` que devolvió el `200`; el desenlace lo fijan `engine` (`la_restauracion_que_llega_a_termino_deja_la_fila_en_success` y `la_restauracion_que_falla_deja_la_fila_en_failed_con_su_motivo`). No se duplica en el borde HTTP porque exigiría una restauración de verdad contra un repositorio.
- [x] 1.8 Test API: `GET /api/v1/restorations` lista con filtro `status`, orden y paginación; `target_name`/`target_type` a `null` si el target no existe; ruta protegida. **Hecho**: siete tests en `api::restorations` —listado y orden, filtro por `status`, filtro por `target_id`, ventana con total, `null` del target, `400` de un `status` fuera del vocabulario y `401` sin sesión—, más la ruta añadida a `las_cinco_rutas_sueltas` de `api::particion` para fijar que **no** entra en la lista blanca.
- [x] 1.9 Test `engine`: dos restauraciones sobre el mismo destino de base no caben; destinos distintos sí. **Hecho**: `engine::tests::la_reserva_de_un_destino_de_base_excluye_la_segunda_y_admite_otro_destino`.
- [x] 1.10 Test UI: la acción de restaurar se ofrece para targets de BD; la confirmación exige teclear el nombre y solo entonces envía `sobrescribir: true`; nombre que no coincide deshabilita.
- [x] 1.11 Test UI: pantalla de Restauraciones lista, filtra por estado y muestra el motivo de las fallidas.

## 2. GREEN

- [x] 2.1 `bollard`: añadir `attach_stdin` al transporte y un método que inyecte un `AsyncRead`/canal en el `stdin` del `exec`, drenando `stderr` en paralelo, con plazo de inactividad. **Hecho**: `Cliente::ejecutar_con_entrada` + `opciones_con_entrada` en `src/docker/cliente.rs`; error `EntradaNoEntregada` (causa del puente) y `RestauracionEstancada` en `src/docker/mod.rs`.
- [x] 2.2 `rustic`: método que lee un nodo del snapshot con `Repository::dump` hacia un `Write` puente (canal acotado, bloqueante en el hilo del motor). **Hecho**: `Repositorio::volcar_a` en `src/rustic/repositorio.rs`, con `Error::VolcadoQueNoEsUno` para la instantánea que no trae exactamente un nodo de fichero; el puente es `tunel_de_restauracion` (`TunelDeRestauracion`/`EscritorDelVolcado`/`LectorDelVolcado`) en `src/engine/restauracion.rs`.
- [x] 2.3 `restauracion`: construcción del comando inverso por tipo y ejecución del flujo (2 execs para `sqlite`). **Hecho**: el comando inverso está construido y probado, y el flujo lo conduce `ejecutar_las_ordenes`/`ejecutar_una` (`src/engine/restauracion.rs`): la última orden recibe el volcado por su `stdin` y las anteriores corren con la entrada cerrada, se para en la primera que no sale con cero y su canal de error se acumula acotado y depurado en `ErrorDelMotor::Restauracion`. Tres pruebas de punta a punta con motor (`#[ignore]`): el volcado solo a la última orden, el corte en la primera que falla y el cuelgue que acaba en `RestauracionEstancada` sin dejar colgado el hilo que volcaba.
- [x] 2.4 `engine`: rama por tipo en `Motor::restaurar`, reserva de destino por base y persistencia `pending→running→success|failed`. **Hecho**: `CaminoDeRestauracion` decide la rama (`restaurar_en_el_directorio` / `restaurar_en_el_contenedor`), la reserva por clave de destino es `ReservaDeRestauracion` y el ciclo de la fila lo escriben `marcar_en_curso` y `cerrar_la_restauracion`; las cuatro pruebas de `engine::tests` cubren la rama, la reserva y los dos desenlaces de la persistencia.
- [x] 2.5 `api-resources`: `sobrescribir` en el cuerpo del `POST`, `428` sin confirmación, `execution_id` persistido y `GET /api/v1/restorations`. **Hecho**: `sobrescribir: bool` con `#[serde(default)]` y `deny_unknown_fields` en `CuerpoDeLaRestauracion`, el orden de comprobaciones del contrato (repositorio → vínculo → existencia real de la instantánea → puerta por tipo), la fila de `restore_executions` **antes** del `200` con su `id` como `execution_id`, y el módulo nuevo `src/api/restorations.rs` montado en las dos ramas del router.
- [x] 2.6 `AppError::PreconditionRequired` → `428`.
- [x] 2.7 UI: levantar la limitación por tipo, confirmación tipada y `sobrescribir: true` condicional.
- [x] 2.8 UI: pantalla de Restauraciones + entrada de navegación + hook y cliente de API.

## 3. REFACTOR y verificación

- [x] 3.1 `just check-all` en verde (backend fmt/clippy/tests; UI lint/typecheck/test/build).
- [x] 3.2 Auditar con `@rust-reviewer` y `@react-reviewer`; corregir bloqueantes.
- [x] 3.3 Documentar en `docs/FEATURE-021-restauracion-de-bases-de-datos.md`.
- [x] 3.4 Comprobar que delta, plan, tasks y doc dicen lo mismo.

## 4. Cierre

- [x] 4.1 Archivar el change con `openspec archive`.
- [ ] 4.2 Renombrar `plans/PLAN-021.md` a `plans/PLAN-021-COMPLETED.md`.
- [ ] 4.3 PR a `development` con gitmoji y `Closes #67`.
