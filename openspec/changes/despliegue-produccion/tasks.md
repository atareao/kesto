# Tasks

## 0. Preparación

- [x] 0.1 Confirmar que el issue [#17](https://github.com/atareao/kesto/issues/17) existe y describe el alcance del despliegue; si no existe, crearlo con `gh issue create` y asignarlo al usuario. Verificación: `gh issue view 17` devuelve el alcance acordado.
- [x] 0.2 Medir la línea base antes de tocar nada: `just check` en verde, `cargo run` sirviendo panel y API en local, y `release.yml` saltándose el job `image` por falta de `Dockerfile`. Verificación: los tres hechos quedan anotados en el plan `plans/PLAN-008.md`.
- [x] 0.3 Fijar el contrato de arranque leyendo `docs/ENTORNO.md` y `src/config.rs`, y anotar en el plan los nombres exactos de las variables (las tres sin prefijo, las `KESTO_*` y las `OIDC_*`) y cuáles son obligatorias por modo. Verificación: la lista coincide con lo que lee `src/config.rs` y con `docs/ENTORNO.md`.

## 1. Imagen (Dockerfile y .dockerignore)

- [x] 1.1 Revisar y completar el `.dockerignore` para que el contexto no arrastre `target/`, `ui/node_modules/`, `ui/dist/`, `data/`, `.env*`, `plans/`, `docs/` ni el propio `Dockerfile`. Verificación: `podman build` no copia esos directorios (el tamaño del contexto queda en kilobytes, no en gigabytes) y el árbol de `ui/` sí entra en la etapa de UI.
- [x] 1.2 RED (medición previa): dejar escrito que hoy `_require-image` aborta por falta de `Dockerfile`, de modo que la primera tarea de implementación tenga un antes y un después. Verificación: `just build` falla con el mensaje de `_require-image` antes de crear el fichero.
- [x] 1.3 GREEN: escribir el `Dockerfile` `multi-stage` con la etapa de UI (`pnpm install --frozen-lockfile` + build de `ui/`), la etapa de Rust (`cargo build --release --locked` del binario) y la etapa de runtime. Verificación: `just build` termina sin errores y produce la imagen.
- [x] 1.4 Recortar la etapa final a runtime puro: imagen base mínima, sin `cargo`/`rustc`/`node`/`pnpm` ni `node_modules`, con el binario y `ui/dist` en su sitio. Verificación: `podman run --rm <imagen> sh -c 'command -v cargo rustc node pnpm'` no encuentra ninguno y `ls` del panel responde.
- [x] 1.5 Fijar el usuario sin privilegios `10001` (uid/gid, el mismo que `just run`) y la propiedad de los directorios que el proceso necesita (`/app/data`). Verificación: `podman run --rm <imagen> id` muestra `uid=10001` y no `root`.
- [x] 1.6 Exponer el puerto `8080` y declarar un `HEALTHCHECK` sobre `GET /api/v1/health`; confirmar que Podman conserva la sonda con `BUILDAH_FORMAT=docker`. Verificación: `podman inspect` de la imagen muestra el `HEALTHCHECK` y el puerto expuesto.
- [x] 1.7 Comprobar que ningún secreto queda en la imagen: el `Dockerfile` no declara `KESTO_MASTER_KEY`, `KESTO_SECRET` ni las `OIDC_*`, y ninguna capa las contiene. Verificación: `podman history` de la imagen y `rg 'KESTO_MASTER_KEY|KESTO_SECRET|OIDC_' Dockerfile` no devuelven valores de secreto.

## 2. Compose (compose.yaml)

- [x] 2.1 RED (medición previa): confirmar que `_require-compose` aborta hoy por falta de fichero. Verificación: `just deploy` falla con el mensaje de `_require-compose` antes de crear el fichero.
- [x] 2.2 GREEN: escribir el `compose.yaml` con el servicio `kesto` del §8 —`image: kesto:latest`, `container_name: kesto-app`, `restart: always`, `ports: "8080:8080"`— y las tres variables de entorno `RUST_LOG=info,kesto=debug`, `DATABASE_URL=sqlite:///app/data/kesto.db` y `BIND_ADDRESS=0.0.0.0:8080`. Verificación: `podman compose config` valida el fichero y muestra esos valores literales.
- [x] 2.3 Añadir los cuatro montajes del §8 y declarar el volumen nombrado `kesto_data`: socket `/var/run/docker.sock:/var/run/docker.sock:ro`, `/var/www:/backup_sources/var_www:ro`, `/etc/nginx:/backup_sources/nginx_config:ro` y `kesto_data:/app/data`. Verificación: `podman compose config` lista los cuatro montajes con `ro` en los tres de solo lectura y el volumen `kesto_data` declarado.
- [x] 2.4 Cablear los secretos por el entorno, sin escribir ninguno: referenciar `KESTO_MASTER_KEY`, `KESTO_SECRET` y las `OIDC_*` como variables de entorno cuya procedencia es el `.env` del despliegue, y dejar un `.env.example` sin valores reales. Verificación: `rg 'KESTO_MASTER_KEY|KESTO_SECRET|OIDC_' compose.yaml` no muestra ningún valor de secreto y el `.env.example` documenta las claves sin rellenarlas.
- [x] 2.5 Alinear la imagen del compose con lo que produce `just build` (etiqueta `kesto:latest`) para que `just deploy` no dependa de un tag manual extra. Verificación: tras `just build`, `podman image inspect kesto:latest` existe y `just deploy` no falla por imagen ausente.

## 3. Build, arranque y verificación

- [x] 3.1 Construir la imagen en limpio y medir: `just build-nocache` termina y la imagen final pesa menos que la suma de toolchain + panel sin compilar. Verificación: `podman images` con el tamaño anotado en el plan.
- [x] 3.2 Arrancar el contenedor en modo `disabled` (solo `KESTO_MASTER_KEY`) y comprobar que levanta y escucha. Verificación: `podman compose up -d` deja el contenedor `Up` y `just health` responde `200`.
- [x] 3.3 Verificar la salud y el panel desde el mismo origen. Verificación: `curl -s http://127.0.0.1:8080/api/v1/health` contesta `200` con `auth_mode:"disabled"` y `curl -sI http://127.0.0.1:8080/` contesta `200` con `content-type: text/html`.
- [x] 3.4 Verificar la persistencia: escribir un registro (o dejar que la app cree la base) y recrear el contenedor. Verificación: tras `podman compose up -d --force-recreate` los datos siguen en el volumen `kesto_data` y `/app/data/kesto.db` no se ha perdido.
- [x] 3.5 Verificar el rechazo por configuración inválida: sin `KESTO_MASTER_KEY`, con `OIDC_*` incompletas y con `BIND_ADDRESS=` vacía el arranque falla con causa en el registro; con una credencial corta arranca igualmente avisando. Verificación: los cuatro desenlaces quedan anotados (contenedor que no levanta y su último log, frente a contenedor que arranca con aviso).
- [x] 3.6 Verificar que el proceso corre sin privilegios y que el socket va en solo lectura. Verificación: `podman exec kesto-app id` da `uid=10001` y `podman inspect` muestra el montaje del socket con `ro`.
- [x] 3.7 Comprobar que el job `image` de `release.yml` se enciende solo con la existencia de `Dockerfile` y `ui/package.json`, sin editar el workflow. Verificación: `git diff -- .github/` está vacío y el `preflight` del CI detecta `has_image=true`.

## 4. Documentación y cierre

- [x] 4.1 Escribir `docs/FEATURE-008-despliegue-produccion.md`: qué es, ficheros implicados (`Dockerfile`, `.dockerignore`, `compose.yaml`), cómo se construye y se despliega, variables y secretos, decisiones (usuario `10001`, `BUILDAH_FORMAT=docker`, secretos por entorno) y follow-ups. Verificación: el documento existe y los comandos que cita (`just build`, `just deploy`, `just health`) funcionan tal cual están escritos.
- [x] 4.2 Ejecutar `just check` y `just check-all` y anotar los números de pruebas del backend y del panel. Verificación: los checks en verde y el recuento escrito en el plan.
- [x] 4.3 Repasar la spec `deploy` requisito a requisito y marcar cada uno con la evidencia que lo cubre (build, inspección de imagen, `compose config`, `curl`). Verificación: cada escenario tiene evidencia medida y no queda ninguno sin comprobar.
- [x] 4.4 Actualizar `HANDOFF.md` con el estado medido y marcar las tareas del plan `plans/PLAN-008.md`; renombrarlo a `PLAN-008-COMPLETED.md` solo cuando todo esté cerrado. Verificación: `HANDOFF.md` describe el estado real y el plan no tiene casillas pendientes.
- [x] 4.5 Archivar el change con `openspec archive despliegue-produccion` tras la aprobación, comprobar que el delta `specs/deploy` se fusiona y que `openspec validate --specs --strict` pasa. Verificación: `openspec list --specs` muestra `deploy` y `openspec validate --specs --strict` termina en verde.
- [x] 4.6 Abrir el PR de `feature/despliegue-produccion` a `development`, referenciando el issue #17 y con el resumen de la verificación de arranque. Verificación: el PR existe, el CI está en verde y el issue queda enlazado.
