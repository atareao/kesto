# Relevo — FEATURE-016: identidad visual y tema claro/oscuro

`main` en **`dffb9cf`** y `development` en **`5fe61f6`**, versión **0.6.1**, tag **`v0.6.1`**. **Sin issues ni PRs abiertos.** El laboratorio sigue **levantado** (ver §Lab).

## Tarea pendiente (empezar por aquí)

**Mejorar el aspecto visual de Kesto.** El usuario lo pidió con estos assets y decisiones:

- Los assets están en **`~/Descargas/kesto-web-assets/`**.
- **Tema claro/oscuro con oscuro por defecto**, y un **toggle** que persiste la elección (`localStorage`). No sigue al sistema: oscuro salvo que el usuario cambie a claro.
- Alcance acordado: **marca + tema + pulido de páginas** (no solo marca).

### Flujo obligatorio (SDD + TDD)

1. **Abrir el GitHub Issue** de la feature (`gh issue create ... --assignee @me`): es el punto de entrada, ninguna feature empieza sin él.
2. `plans/PLAN-016.md` (siguiente número libre; `plans/` es la única fuente del contador).
3. Change SDD: `openspec new change <nombre>` y rellenar `proposal.md` + `specs/` + `tasks.md`.
4. **Parar y esperar aprobación** del usuario antes de escribir código.
5. TDD rojo→verde→refactor. Al cerrar: `docs/FEATURE-016-<slug>.md` y renombrar el plan a `PLAN-016-COMPLETED.md`.

> `@plan-writer` escribe `PLAN.md` en la raíz por defecto: pásale **ruta explícita** (`plans/PLAN-016.md`) y el **contenido completo** del plan en el prompt.

## Reconocimiento ya hecho (no repetirlo)

### Assets (`~/Descargas/kesto-web-assets/`)

- `favicons/`: `favicon.ico` + pngs **16/32/48/64**, `apple-touch-icon.png` (180), `android-chrome-192/512`, `maskable-icon-512`, `mstile-150x150`, más **`site.webmanifest`** (`name`/`short_name` Kesto, `theme_color #123048`, `background_color #F8FAFC`, `display standalone`) y **`head-tags.html`** (bloque listo con `<link rel="icon">`, apple-touch, manifest, `msapplication-*`, `theme-color` y OG).
- `logos/`: `kesto-icon.png|webp`, `kesto-icon-dark.png|webp`, `kesto-icon-512/1024.png`, `kesto-icon-mono-dark.png`, `kesto-icon-mono-white.png`, `kesto-logo-horizontal.png|webp`, `kesto-logo-horizontal-dark.png|webp`.
- `social/`: `og-image-light.png`, `og-image-dark.png`, `social-avatar-light/dark-800x800.png`.
- `brand/brand-tokens.json` y `brand/kesto-theme.css` (misma fuente): navy **`#123048`**, cyan **`#2194AA`**, teal **`#33998D`**, verde **`#7EBC50`**, naranja **`#F08724`**, fondo claro **`#F8FAFC`**, fondo oscuro **`#0D1B2A`**, texto claro `#123048`, texto oscuro `#F8FAFC`.

### Panel actual (`ui/`)

- Stack: **Antd v5.29**, React 19, Vite 7, React Router 7, TanStack Query 5. Tests con vitest + Testing Library.
- **No hay `ConfigProvider` ni tema configurado**: se pinta el **tema claro por defecto** de Antd. No hay ni un token de marca.
- `src/components/Layout/Layout.tsx`: `Sider` con `theme="light"`; cabecera y `marca-del-panel` sin logo (solo el texto «Kesto»).
- `src/estilos.css`: **colores hardcodeados** (`#ffffff`, `#f0f0f0`, `#1677ff`) en `.cabecera-del-panel`, `.enlace-de-salto`, `:focus-visible` → habrá que tokenizarlos/adaptarlos a ambos temas.
- `ui/index.html`: **sin favicons, sin manifest, sin meta/OG**.
- **No existe `ui/public/`** todavía: los assets de marca aún no se sirven. Habrá que crearlo (favicons y manifest a la raíz de `public/`, logos a `src/assets/` o `public/`).
- Rutas y pantallas del panel: Dashboard, Tareas, Histórico, Snapshots, Ajustes, Sesión, No encontrada (todas bajo `Layout` → `SoloConSesion`). Candidatas al pulido: espaciados, tarjetas y Dashboard.

## Lab (dejado arriba)

Red podman `kesto-lab`, todo alcanzable por nombre:

- `rustfs-lab` — RustFS (S3), puertos 9000/9001, creds `rustfsadmin`/`rustfsadmin`, bucket **`kesto`**. Datos en `lab/rustfs/`.
- `mariadb-src` (`pruebas`), `postgres-src` (`pruebas`), `sqlite-src` (`/datos/app.db`, WAL).
- `kesto` — imagen **`ghcr.io/atareao/kesto:0.6.1`**, red `kesto-lab`, `-p 8080:8080`, `--userns=keep-id:uid=10001,gid=10001`, volumen `kesto_data:/app/data`, socket podman en `/var/run/docker.sock`, `-v /data/rust/kesto/lab/directorio:/datos:ro`. Clave maestra en `/tmp/opencode/kesto-master-key`. Panel: `http://127.0.0.1:8080`.
- Parar el lab: `podman rm -f kesto rustfs-lab mariadb-src postgres-src sqlite-src`.
- **Recordatorio:** credenciales de Docker Hub del host caducadas → usar `podman pull --authfile /tmp/opencode/anon.json <img>`.

## Recordatorios heredados

- El CI **no** levanta daemon de contenedores: los e2e van `#[ignore]` (`cargo test -- --ignored`). `DOCKER_HOST=unix:///run/user/1000/podman/podman.sock` en esta máquina.
- El binario del host (glibc nueva) **no** corre en la imagen `bookworm`: para probar contra RustFS, usar la imagen publicada del tag.
- Antes de cada commit: `just check` (backend) y, en `ui/`, `pnpm run lint` → `npx tsc --noEmit` → `pnpm test`. Git flow con gitmoji; nada directo a `main`/`development`.

## Cierre de la sesión anterior

FEATURE-015 (higiene de la copia, issues #52/#53) entregada y publicada en **v0.6.1**; detalles en `docs/FEATURE-015-higiene-de-la-copia.md` y `plans/PLAN-015-COMPLETED.md`.
