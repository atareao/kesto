# FEATURE-039 — Endurecimiento del arranque y de la autenticación (#157, #158, #159)

- **Issues:** #157 (SEC-01), #158 (SEC-02) y #159 (SEC-03), hijos de la auditoría de seguridad **#156**. **Change OpenSpec:** `endurecimiento-arranque-auth`. **Plan:** `plans/PLAN-039.md`.
- **Tipo:** producto (Rust + plantilla de despliegue + CI + documentación). **No** toca el panel ni la KDF.
- **Incompatible hacia atrás en dos puntos**, documentados abajo y en `docs/ENTORNO.md`.

## Qué es

El modo sin autenticación deja de ser lo que ocurre por omisión y una credencial débil deja de arrancar:

1. **Permiso explícito del modo sin autenticación.** Sin las tres `OIDC_*` el arranque exige `KESTO_ALLOW_INSECURE=1`. `0` o ausente lo niegan; cualquier otro valor —vacío incluido— es error de formato. En modo OIDC la variable ni se consulta.
2. **Loopback obligatorio en ese modo.** En modo `Disabled` la dirección de escucha tiene que ser de loopback (`127.0.0.0/8` o `::1`); cualquier otra es error de arranque. El defecto de `BIND_ADDRESS` pasa a `127.0.0.1:8080`.
3. **Credencial débil = puerta de arranque.** `KESTO_MASTER_KEY` (siempre) y `KESTO_SECRET` (en modo OIDC) con menos de 32 caracteres, vacía incluida, impiden el arranque nombrando la variable y el mínimo, nunca el valor.
4. **La gestión de tokens no se monta sin autenticación.** En modo `Disabled`, `/api/v1/tokens*` contesta `404` con la envoltura, como `/api/v1/auth/*`.
5. **La plantilla de producción exige proveedor.** `compose.yaml` declara `KESTO_MASTER_KEY`, las tres `OIDC_*` y `KESTO_SECRET` con `${VAR:?}`: sin cualquiera de ellas no levanta el servicio. El smoke test del release pasa a la forma de producción.

## Causa / motivo

Los tres hallazgos comparten raíz: la configuración comprobaba **presencia** y trataba la fortaleza de una credencial como un aviso, y el modo sin autenticación era lo que pasaba cuando faltaban las `OIDC_*`.

- **SEC-01:** un `.env` que no se carga o un `compose` que interpola a vacío dejaba el panel —y el motor, que dirige el socket del daemon— abierto en todas las interfaces, con `POST /api/v1/snapshots/{id}/restore` entre las rutas alcanzables.
- **SEC-02:** `env_lookup` devuelve `Some("")` para una variable definida pero vacía. Con `KESTO_MASTER_KEY` vacía, HKDF-SHA256 con sal `None` y la etiqueta constante `kesto/secrets/v1` deriva una clave AES-256 determinista y **públicamente calculable**: quien leyera el `.db` descifraba `repositories.secret_access_key`, `repositories.passphrase` y `backup_targets.db_password`.
- **SEC-03:** en modo OIDC, una `KESTO_SECRET` vacía o corta firmaba las sesiones con una clave reproducible; un JWT de sesión forjado es un bypass de autenticación.

## Ficheros implicados

- `src/config.rs`: `ENV_KESTO_ALLOW_INSECURE`, `DEFAULT_BIND_ADDRESS` a loopback, variantes `InsecureModeNotAllowed`, `InsecureBindAddress` y `WeakCredential`, la comprobación cruzada tras `resolve_auth`, el umbral de credenciales y `warnings()` sin credenciales.
- `src/api/mod.rs`: `tokens::router()` sale de la rama `Disabled`.
- `src/api/tokens.rs`, `src/api/health.rs`, `src/state.rs`, `src/db/mod.rs`, `src/test_support.rs`: tests y entornos de prueba con el permiso explícito.
- `tests/arranque.rs`, `tests/e2e/arnes.rs`: el permiso donde el proceso arranca sin OIDC.
- `compose.yaml`, `.env.example`, `.github/workflows/release.yml`: plantilla de producción y smoke test.
- `docs/DEFINITION.md` (§8), `docs/ENTORNO.md`, `docs/ARQUITECTURA.md`, `AGENTS.md`, `docs/FEATURE-008-despliegue-produccion.md`.
- `openspec/changes/endurecimiento-arranque-auth/` · `plans/PLAN-039.md`.
- `ui/`: **sin cambios** (verificado: ninguna llamada a `/api/v1/tokens` en el panel).

## Cómo funciona

- **Orden de validación.** En `from_lookup`, la comprobación cruzada —permiso y loopback— va **después** de `resolve_auth`, sin reordenar el resto: el orden actual fija qué error se ve primero cuando hay varios problemas. Primero el permiso (`Disabled` sin permiso → `InsecureModeNotAllowed`), después el bind (`Disabled` con permiso y dirección no loopback → `InsecureBindAddress`). El permiso solo se consulta en `Disabled`.
- **La clave maestra no se recorta.** Se mide y se envuelve con el valor crudo, porque recortarla cambiaría la clave derivada. La longitud se mide en **caracteres** (`chars().count()`), que es la unidad que anuncia el mensaje y espera el operador.
- **`KESTO_SECRET` no se lee en modo `Disabled`** (`Config.secret` queda `None`): sin proveedor no hay sesión que firmar, así que una variable inservible no puede impedir el arranque del modo de desarrollo.
- **Por qué el loopback.** Un proceso atado a loopback dentro de un contenedor **no es alcanzable por el puerto publicado** —el reenvío llega a la IP del contenedor, no a su loopback—, así que el modo de desarrollo no puede verificar un puerto publicado ni aunque se publique. De ahí que el smoke test de la imagen use la forma de producción.
- **Por qué la gestión de tokens no se monta.** Sin proveedor no hay sesión que exigir, así que la ruta solo puede quedarse abierta —emitiendo credenciales sin credencial alguna— o no existir. Y un token emitido así **sobrevive al cambio de modo**: su hash queda en la base y no depende de quién lo pidió, de modo que seguiría sirviendo cuando la autenticación se encienda.

## Cómo verificar

```sh
just check                 # fmt + clippy -D warnings + cargo test --locked
just check-all             # lo anterior + panel (typecheck, lint, vitest, build)
cargo test --locked --bin kesto 'config::tests::'   # 35 tests
podman compose config      # falla sin OIDC_*/KESTO_SECRET; valida con ellas
```

## Decisiones

- **Permiso incondicional, no condicionado al bind.** Se descartó exigir `KESTO_ALLOW_INSECURE` solo cuando el bind no fuera de loopback: el caso del hallazgo es el `.env` que no se carga, y ahí nadie mira el bind. El operador recibe una señal única y explícita en vez de una regla condicional.
- **Valores `1`/`0`, sin sinónimos.** `true`/`yes`/`on` no se interpretan: en este repositorio un valor presente pero inutilizable ya es error (`RUST_LOG=""`, `BIND_ADDRESS=""`, `KESTO_HSTS_MAX_AGE=""`), y una tabla de sinónimos es superficie de confusión. Los espacios sobrantes se recortan, como en `RUST_LOG` (decisión de estilo, no de seguridad: no hay valor con espacios significativos).
- **Credencial débil como puerta, con el coste asumido.** `openspec/specs/secrets/spec.md` defendía el aviso por disponibilidad; se invierte a conciencia porque una clave vacía hace **públicamente calculable** la clave de los secretos guardados. Se mitiga con un mensaje que nombra la variable y el mínimo, y con la nota de actualización en `docs/ENTORNO.md`.
- **La KDF no se toca.** Pasar a Argon2id/scrypt con sal persistida invalidaría los secretos ya cifrados con el sobre `v1:` y exige migración con re-cifrado: fuera de este bloque, con issue de seguimiento.
- **Smoke test en forma de producción.** El descubrimiento OIDC caído solo avisa, así que el workflow arranca con un emisor inalcanzable y verifica `/api/v1/health` → `200` con `auth_mode: "oidc"` desde el runner, sin necesitar un proveedor real.
- **Commits `feat:`/`fix:` sin `!`.** El cambio rompe compatibilidad, pero el pipeline deduce `major` con esa marca y este proyecto va por `0.x`: el bump que corresponde es `0.18.0`. La incompatibilidad se describe en el PR, el CHANGELOG y la documentación.

## Incompatibilidades (nota de actualización)

1. **Un despliegue con `KESTO_MASTER_KEY` —o, en modo OIDC, `KESTO_SECRET`— de menos de 32 caracteres deja de arrancar** al actualizarse. La reparación es poner una credencial de 32 caracteres o más. Ojo con cuál se toca: **rotar la clave maestra deja ilegibles los secretos ya guardados** (los tres del §3), así que hay que volver a darlos de alta; rotar `KESTO_SECRET` solo invalida las sesiones vivas.
2. **El bind por defecto cambia** de `0.0.0.0:8080` a `127.0.0.1:8080`. Un despliegue que no declare `BIND_ADDRESS` deja de escuchar en todas las interfaces; el `compose.yaml` del repositorio lo declara, así que no le afecta.

## Follow-ups

- **KDF con sal persistida (Argon2id/scrypt)** para claves recordables: fuera de alcance aquí porque invalida los secretos ya cifrados y exige migración. Issue de seguimiento abierto desde #158.
- **Resto de la campaña de seguridad #156:** bloques B (#160/#161/#162), C (#164), D (#163/#166/#165), E (#167) y F (#168).
