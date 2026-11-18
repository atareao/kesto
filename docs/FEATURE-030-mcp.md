# FEATURE-030 — Servidor MCP

**Issue:** [#119](https://github.com/atareao/kesto/issues/119) · **Plan:** `plans/PLAN-030.md` · **Change SDD:** `openspec/changes/mcp/`

## Qué es

Un servidor **MCP** (Model Context Protocol) que deja a un agente de IA operar Kesto con criterio: **consultar** el estado de las copias, **lanzar** una copia cuando toca y **comprobar** que el repositorio responde. Vive en el crate `kesto-mcp`, sobre el SDK oficial `rmcp`, y es un **cliente de la API por HTTP**: no abre la base de datos, no habla con Podman ni con rustic y no decide nada por su cuenta.

El catálogo de herramientas es **cerrado y corto a propósito**: siete lecturas y dos acciones. **No hay ninguna herramienta que restaure, borre, edite ni cancele** —y su ausencia es una defensa, no una tarea pendiente—: un agente no puede deshacer una copia por mucho que se lo pidan. Ampliarlo exige un change nuevo.

La credencial que lo habilita es un **token de API de Kesto** (Fase 1 de este mismo change): Kesto guarda solo su hash, el token se enseña una vez al crearlo y cada token lleva un nombre y unos scopes acotados `read`/`run`. El servidor MCP **reenvía** el token que le llega y deja que la API aplique los scopes.

## Ficheros implicados

- `kesto-mcp/src/lib.rs` — superficie del crate y su doctest de intención (es un puente, y **solo** eso).
- `kesto-mcp/src/config.rs` — lectura del entorno (`Config::from_env`/`from_lookup`), los nombres de las variables, el defecto de la dirección HTTP y el tipo `SecretToken` (un `String` cuyo `Debug` no publica el valor y que se borra de memoria al soltarlo).
- `kesto-mcp/src/client.rs` — cliente HTTP de la API (`ApiClient`): únicos dos verbos, `GET` y `POST`; devuelve el JSON de la API **sin reinterpretar la envoltura** `{status, message, data}`.
- `kesto-mcp/src/error.rs` — `ApiError`: los fallos con el mensaje que hay que leer (`sin_token`, `token_invalido`, `scope_insuficiente`, `respuesta_de_la_api`, `transporte`, `respuesta_ilegible`). Ninguna variante lleva el token.
- `kesto-mcp/src/server.rs` — las **nueve herramientas** y el servidor `KestoMcp`.
- `kesto-mcp/src/http.rs` — montaje del transporte `Streamable HTTP` bajo `/mcp` sobre `axum`.
- `kesto-mcp/src/main.rs` — el binario: elige el transporte (`stdio` por defecto, `--transport http`), monta el registro por **stderr** y arranca.
- `Cargo.toml` (raíz) — el repositorio pasa a **workspace** (`members = ["kesto-mcp"]`, `resolver = "3"`); el paquete raíz sigue siendo `kesto`.
- `kesto-mcp/Cargo.toml` — `rmcp` 3.5.1 con `transport-io` + `transport-streamable-http-server`, `axum`, `reqwest` 0.12 (el mismo del paquete raíz) y `wiremock` en pruebas.
- `kesto-mcp/tests/` — `herramientas.rs`, `transportes.rs` y `common/`, contra una API falsa (`wiremock`).

> El lado de la API (migración `0009_api_tokens.sql`, `src/api/tokens.rs`, `src/auth/`, el filtro `id` de ejecuciones) es de la Fase 1 del mismo change; se documenta en su spec (`openspec/changes/mcp/specs/api-tokens/`).

## Cómo funciona

1. **Es un reenvío.** Cada herramienta compone la petición, elige el token y devuelve lo que la API contestó como resultado **estructurado**. No hay lógica de negocio: los scopes los aplica la API.
2. **Token: cabecera primero, entorno después.** En `stdio` no hay petición HTTP, así que manda `KESTO_API_TOKEN`. En `HTTP`, si la petición trae `Authorization: Bearer <token>`, ese token gana sobre el del entorno. Un token elegido que la API rechaza no tumba al servidor: la llamada devuelve un **error de herramienta**, no un `401` crudo, y la siguiente llamada con un token válido funciona.
3. **Errores que dicen qué falta.** Un fallo de credencial se traduce a un resultado estructurado con la clase (`sin_token` → «define `KESTO_API_TOKEN`», `token_invalido` → «emite uno nuevo y actualízalo», `scope_insuficiente` → «emite un token con scope `run`»), además de `status`, para que el agente decida sin analizar texto.

### Herramientas

| Herramienta | Scope | Qué hace |
|---|---|---|
| `list_repositories` | `read` | Lista los repositorios con su destino y su salud |
| `list_targets` | `read` | Lista los targets (de dónde se extrae) |
| `list_jobs` | `read` | Lista las tareas programadas, con su cron y su retención |
| `list_snapshots` | `read` | Lista las instantáneas de un repositorio (no restaura ninguna) |
| `list_executions` | `read` | Histórico de ejecuciones, con filtros `job_id`/`status`/`limit`/`offset` |
| `get_execution` | `read` | Una ejecución por su identificador (un id inexistente da error, no lista vacía) |
| `get_health` | `read` | Salud del servicio y su `auth_mode` |
| `run_backup` | `run` | Lanza una copia de una tarea ahora mismo |
| `check_repository_health` | `run` | Comprueba que un repositorio responde y persiste el resultado |

Ninguna otra. En particular **no** existe `restore_snapshot`, `delete_*`, `update_repository` ni `cancel_execution`; hay un test que fija que el catálogo tiene exactamente nueve y ninguna destructiva.

### Transportes

- **`stdio` (defecto).** `kesto-mcp` sin argumentos habla el protocolo por la entrada/salida estándar —así lo lanzan opencode y Claude Desktop—. El **registro va a stderr**: en `stdio` el protocolo *es* stdout, y una traza escrita ahí corrompe el flujo.
- **`Streamable HTTP`.** `kesto-mcp --transport http [--bind <dirección>]` sirve el mismo catálogo por HTTP bajo **`/mcp`** (no como comodín de la raíz). En este transporte el token puede viajar por cabecera en cada petición.

Ambos transportes exponen **el mismo catálogo** y el mismo comportamiento.

### Autenticación y scopes (lado API)

- `read` cubre las **lecturas**; `run` cubre las lecturas **y** las acciones que disparan trabajo (`run_backup`, `check_repository_health`).
- Restaurar y borrar quedan **fuera del alcance de cualquier token**: no existe scope que las habilite.
- La **gestión** de tokens (`/api/v1/tokens`) exige **sesión**, no un token: un token que pudiera emitir otro sería una vía de escalada.
- En modo `Disabled` (sin ninguna `OIDC_*`) no se exige nada: la superficie sigue abierta. Es el modo de desarrollo, no un despliegue expuesto.

## Variables / configuración

Todo llega por entorno. El token **no** se escribe en el código ni en un fichero versionado.

| Variable | Obligatoria | Valor por defecto | Para qué |
|---|---|---|---|
| `KESTO_URL` | **sí** | — | URL base de la API de Kesto. Se recorta y pierde la barra final. Ausente o vacía → **error de arranque** que nombra la variable |
| `KESTO_API_TOKEN` | no | — | Token por defecto cuando la petición no trae `Authorization`. Un valor vacío o en blanco se trata como **ausente**, no como error |
| `KESTO_MCP_BIND_ADDRESS` | no | `127.0.0.1:8931` | Dirección del transporte HTTP (solo con `--transport http`). El defecto es bucle local **a propósito**; publicarlo es una decisión explícita |
| `KESTO_MCP_ALLOWED_HOSTS` | no | los del SDK (bucle local) | Hosts admitidos por HTTP, separados por comas (solo con `--transport http`). Un despliegue con nombre propio la pone |
| `RUST_LOG` | no | `info` | Nivel de registro (va a stderr). A diferencia del servidor Kesto, un `RUST_LOG` ilegible **no** impide arrancar: se cae al nivel por defecto |

Argumentos del binario:

- `--transport <stdio|http>` — el transporte; `stdio` si no se dice nada.
- `--bind <dirección>` — dirección del HTTP. **Sustituye** a `KESTO_MCP_BIND_ADDRESS`; con `--bind` y sin `--transport http` se rechaza (no se ignora en silencio).
- `-h` / `--help` — la ayuda.

La precedencia es: `--bind` > `KESTO_MCP_BIND_ADDRESS` > defecto `127.0.0.1:8931`.

## Cómo se despliega

El crate no publica a crates.io (`publish = false`). Basta compilar y tener a mano el binario:

```bash
cargo build --release -p kesto-mcp
# binario: target/release/kesto-mcp
```

Necesita una API de Kesto viva (`KESTO_URL`) y, salvo en modo `Disabled`, un token (`KESTO_API_TOKEN`). No necesita base de datos propia, ni socket de contenedores, ni acceso a rustic.

### Crear un token en Kesto

`POST /api/v1/tokens` devuelve el valor en claro **una sola vez** (después la base solo guarda su hash: un token perdido se revoca y se emite otro). La ruta exige **sesión** —o queda abierta en modo `Disabled`—:

```bash
# Con sesión: guarda la cookie del flujo OIDC en un fichero y reúsala.
curl -sS -X POST "$KESTO_URL/api/v1/tokens" \
  -b cookies.txt \
  -H 'Content-Type: application/json' \
  -d '{"name": "opencode", "scopes": ["read", "run"]}'
```

La respuesta trae el valor en claro en `data.token` (con prefijo `kesto_`). Cópialo al transporte que uses:

- **`stdio`** → a `KESTO_API_TOKEN` del cliente MCP.
- **`Streamable HTTP`** → a la cabecera `Authorization: Bearer <token>` de cada petición (o a `KESTO_API_TOKEN` como defecto del servidor).

Usa `["read"]` si el agente solo debe consultar: `run` es lo que permite lanzar copias y comprobar salud. Para revocar: `DELETE /api/v1/tokens/{id}` (surte efecto de inmediato).

### opencode (`opencode.jsonc`)

Transporte `stdio` local (un servidor por proyecto; añade `--global` o usa la configuración global para todos):

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "servers": {
      "kesto": {
        "type": "local",
        "command": ["kesto-mcp"],
        "environment": {
          "KESTO_URL": "https://kesto.example",
          "KESTO_API_TOKEN": "{env:KESTO_API_TOKEN}"
        }
      }
    }
  }
}
```

> Los nombres de servidor van bajo `mcp.servers` (novedad de la V2 de opencode). En versiones anteriores el nombre colgaba directamente de `mcp`.

Se puede añadir por CLI en vez de a mano:

```bash
opencode mcp add kesto \
  --env KESTO_URL=https://kesto.example \
  --env KESTO_API_TOKEN=kesto_... \
  -- kesto-mcp
```

Alternativa **remota** (arranca `kesto-mcp --transport http` por tu cuenta y conecta por HTTP con el token en cabecera):

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "servers": {
      "kesto": {
        "type": "remote",
        "url": "http://127.0.0.1:8931/mcp",
        "oauth": false,
        "headers": { "Authorization": "Bearer {env:KESTO_API_TOKEN}" }
      }
    }
  }
}
```

### Claude Desktop (`claude_desktop_config.json`)

Arranque por `stdio`:

```json
{
  "mcpServers": {
    "kesto": {
      "command": "kesto-mcp",
      "env": {
        "KESTO_URL": "https://kesto.example",
        "KESTO_API_TOKEN": "kesto_..."
      }
    }
  }
}
```

Si el binario no está en el `PATH`, usa la ruta absoluta en `command`. Tras editar el fichero, reinicia Claude Desktop.

## Decisiones

- **D1 — Crate aparte, no módulo.** `kesto-mcp` vive en su propio crate porque su ciclo de vida no es el del motor: habla con la API por HTTP y no comparte dependencias pesadas. Por eso la raíz pasa a ser un **workspace** con `resolver = "3"` (el que casa con `edition = "2024"`; el v2 unificaría features-distintas-por-destino como si fueran el mismo destino).
- **D2 — Solo HTTP.** No abre la base, ni el socket de Podman, ni un repositorio rustic: todo pasa por la API, que es quien aplica scopes y contraseñas.
- **D3 — Catálogo cerrado y sin herramientas destructivas.** La ausencia de `restore`/`delete` es una defensa, no un pendiente; un test la fija.
- **D4 — El token no vive en el servidor.** Entra en cada llamada (cabecera > entorno) y `SecretToken` no se imprime ni en `Debug` ni en el registro.
- **D5 — Errores como resultado estructurado**, no `Err(ErrorData)`: el primero lo ve el agente con el mensaje intacto; el segundo el cliente suele taparlo. Con `error` y `status` para decidir sin analizar texto.
- **D6 — Registro a stderr** en ambos transportes: en `stdio` stdout es el protocolo.
- **D7 — `127.0.0.1:8931` por defecto en HTTP**: exponer a toda la red es una decisión explícita, no un descuido.

## Follow-ups

- **Herramientas destructivas** (restauración, borrado, cancelación): fuera de alcance por diseño; exigirían un change nuevo y un scope nuevo.
- **Modo `Disabled`**: el servidor MCP funciona sin token, pero es para desarrollo; en un despliegue expuesto hay que encender OIDC y usar tokens.
- **Despliegue del binario**: no hay imagen de contenedor ni receta `just` de publicación para `kesto-mcp`; hoy se compila con `cargo build --release -p kesto-mcp`.
- **Comprobación periódica vía MCP**: `check_repository_health` existe; programarla automáticamente sigue fuera de alcance (ver FEATURE-027).
