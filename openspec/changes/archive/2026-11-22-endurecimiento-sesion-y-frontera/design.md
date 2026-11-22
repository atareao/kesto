# Design — endurecimiento de la sesión y de la frontera de confianza

## Contexto

La sesión es un JWT HS256 **sin estado**: se emite en el callback OIDC (`src/auth/session.rs`) firmado con `KESTO_SECRET`, y el *gate* (`src/auth/middleware.rs`) lo verifica en cada petición sin consultar nada más que la firma y `exp`. `Claims` no tiene identificador de sesión. `AppState` ya tiene estado en memoria acotado (el mapa de `logins` pendientes, `src/state.rs`, con `MAX_LOGINS` y purga por caducidad), pero el estado pendiente de un login es transitorio por naturaleza, mientras que una revocación tiene que sobrevivir a un reinicio para significar algo.

## SEC-17 — dónde vive la revocación

### Alternativas

1. **Store de sesiones con estado** (sustituir el JWT por una fila por sesión). Da control total y revocación trivial, pero cambia el modelo entero —la firma deja de bastar, hay una consulta por petición y una tabla que crece sin fin— y es una reescritura fuera del alcance de un endurecimiento. Descartada.
2. **Revocación solo en memoria** (un `HashMap<jti, exp>` como el de `logins`). No toca la base, pero un reinicio borra las revocaciones y una cookie revocada vuelve a servir hasta su `exp`: el arreglo se deshace solo y en silencio, que es justo lo que este repositorio no admite. Descartada.
3. **Revocación por sesión, persistida en SQLite.** El `jti` identifica la sesión y una tabla guarda los revocados hasta su caducidad. Sobrevive al reinicio, revoca solo la sesión que se cierra y no cambia el modelo (la firma sigue bastando; la tabla se consulta además). **Elegida.**
4. **Época de sesión global** (una fila con un contador; el logout la sube y caen todas las sesiones del operador). Baratísima y encaja con el modelo mono-operador, pero un logout en una pestaña cerraría también las demás: cambia la semántica de «cerrar sesión», que la spec describe como cerrar *esta* sesión. Descartada; queda anotada por si algún día se quiere un «cerrar todo».

### Decisión

- `Claims` gana `jti: String`, generado al emitir con un identificador aleatorio (el mismo generador que el `state`/`nonce` del login, o su equivalente). El `jti` no viaja a ninguna URL: solo está en el JWT firmado.
- Tabla `sesiones_revocadas (jti TEXT PRIMARY KEY NOT NULL, exp INTEGER NOT NULL)`. `exp` es el `exp` del token —pasado ese instante la revocación no protege nada— y es lo que permite purgar. No hace falta un índice sobre `exp`: la purga es un `DELETE ... WHERE exp <= ?` y la lectura es por la clave primaria.
- Repositorio `SesionesRevocadas` (patrón de `Tokens`, `src/db/entities/api_token.rs`), colgado de `EstadoDeBase` como `estado.db.sesiones`, con dos operaciones: `revocar(jti, exp)` —idempotente, purga lo caducado antes de insertar— y `esta_revocada(jti) -> bool`.
- `con_cookie` (`src/auth/middleware.rs`) pasa a ser **asíncrona**: tras verificar la firma, consulta `esta_revocada(&claims.jti)` y rechaza con el mismo `401` si está. El camino del token ya consulta la base en el *gate*, así que una lectura más no cambia la naturaleza del middleware.
- `logout` (`src/api/auth.rs`) pasa a leer el `jti` de la cookie que se va a borrar y a llamar a `revocar` antes de responder. El cierre no vuelve a leer ni a verificar la cookie: la ruta está tras el *gate*, que ya dejó la identidad de la sesión en las *extensions*, así que el manejador revoca el `jti` de esa identidad verificada y devuelve la cookie de borrado.

### Por qué `jti` obligatorio y no opcional

Si `jti` fuera opcional (`#[serde(default)]`), una cookie sin él no se podría revocar y el logout sería un no-op silencioso para las sesiones viejas. Al exigirlo, una cookie vieja no decodifica y el *gate* la rechaza con `401`: el despliegue cierra las sesiones vivas, que es un efecto previsto y documentado, no una sorpresa.

## SEC-16 — la frontera mono-operador

No hay nada que construir: la decisión es **documentarla**. Va en `docs/ARQUITECTURA.md` (la sección de identidad) y como requisito en la spec `auth`, para que la ausencia de roles se lea como deliberada y no como un hueco. El requisito deja escrito qué habría que ampliar el día que aparezcan varios operadores: un modelo de autorización por rol o scope, que hoy no existe.

## Impacto en la compatibilidad

- Un despliegue cierra las sesiones vivas (una cookie sin `jti` deja de aceptarse).
- `POST /api/v1/auth/logout` invalida la sesión: reutilizar su cookie contesta `401`.
- Sin cambios de forma en las respuestas ni en las rutas.
