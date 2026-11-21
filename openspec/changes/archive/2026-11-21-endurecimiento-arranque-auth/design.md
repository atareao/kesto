# Design — endurecimiento del arranque y de la autenticación

## D1 · El permiso explícito es `KESTO_ALLOW_INSECURE`, y su valor es `1` o `0`

Se adopta el nombre que propone el hallazgo, con el prefijo `KESTO_` que este repositorio reserva para todo lo que no son las tres variables del §8, `DOCKER_HOST` y las `OIDC_*`.

Semántica: `1` permite el modo `Disabled`, `0` lo niega, y **cualquier otro valor —vacío incluido— es error de formato**. Se descarta interpretar `true`/`yes`/`on`: en este repositorio un valor presente pero inutilizable ya es error (`RUST_LOG=""`, `BIND_ADDRESS=""`, `KESTO_HSTS_MAX_AGE=""`), y una tabla de sinónimos es superficie de confusión. `0` y ausente significan lo mismo, que es lo que cualquiera espera de un booleano de entorno.

Se descarta exigir el permiso **solo cuando el bind no sea de loopback**. La tentación es razonable —ahorra fricción en local— pero el caso que el hallazgo describe es precisamente el `.env` que no se carga y el compose que interpola a vacío, y ahí nadie mira el bind. Además, con el permiso incondicional el operador recibe una señal única y explícita («estoy levantando algo sin autenticación») en vez de una regla condicional que hay que reconstruir mentalmente cada vez.

## D2 · La regla de loopback, y su consecuencia en contenedores

En modo `Disabled` la dirección efectiva debe ser de loopback. Consecuencia no evidente, y motivo de la decisión A del smoke test: **un proceso atado a loopback dentro de un contenedor no es alcanzable por el puerto publicado**. El reenvío del puerto llega a la IP del contenedor, no a su loopback, así que un contenedor sin autenticación no puede verificar su puerto publicado ni aunque se publique.

De ahí que la verificación de la imagen publicada pase a la **forma de producción** (modo OIDC, puerto publicado, `/api/v1/health` desde el runner) en vez de mantenerse en el modo de desarrollo: es la forma que de verdad se publica, y el descubrimiento OIDC caído solo avisa —comportamiento ya fijado por `tests/arranque.rs:114`—, así que no hace falta un proveedor real en CI. El modo de desarrollo queda verificado donde se usa: el e2e del binario y el arnés de pruebas, que declaran el permiso explícito.

Se conserva el puerto `"8080:8080"` en el compose: en modo OIDC el bind es `0.0.0.0:8080` y publicarlo es legítimo.

## D3 · La KDF no se toca en este change

El hallazgo propone Argon2id/scrypt con sal persistida para claves recordables. No entra aquí: cambiar la KDF invalida **todos** los secretos ya cifrados con el sobre `v1:<nonce>:<cifrado>` —`repositories.secret_access_key`, `repositories.passphrase` y `backup_targets.db_password`— y exige una migración con re-cifrado que no cabe en este bloque. Se abre issue de seguimiento.

Lo que sí se cierra aquí es lo que hace inútil cualquier KDF: una clave **vacía**, con la que HKDF deriva una clave pública y determinista. Con el mínimo de 32 caracteres exigido, la fuerza de la clave deja de ser cero.

## D4 · La credencial débil pasa a ser puerta de arranque: coste aceptado

`openspec/specs/secrets/spec.md:45` defiende hoy lo contrario con un argumento de disponibilidad: «negarse a arrancar por una variable floja convertiría un error de configuración en ausencia de copias». El argumento es bueno, y por eso el coste se asume **a conciencia** y se mitiga:

- El mensaje nombra la variable **y el mínimo**, así que la reparación es de segundos y no exige leer el código.
- `docs/ENTORNO.md` y el aviso de release explican el cambio, porque un despliegue existente con una clave corta **dejará de arrancar al actualizarse**. Es un cambio incompatible y se documenta como tal.
- El modo de desarrollo conserva su puerta (`KESTO_ALLOW_INSECURE`), pero con clave válida: el atajo no es «una clave floja», es «sin autenticación».

Lo que no se acepta es el silencio: una clave vacía o corta no degrada una función accesoria, hace **públicamente calculable** la clave con la que se cifran las credenciales de los repositorios. Un aviso que arranca igual deja el sistema en producción con los secretos al alcance de quien lea el fichero, y eso no se avisa: se impide.

## D5 · La gestión de tokens no se monta sin autenticación

En `Disabled` el resto de la superficie sigue abierta —es el modo de desarrollo, ahora opt-in y sin exposición— pero `tokens::router()` no: un token de API emitido sin credencial sobrevive al cambio de modo y sigue sirviendo cuando la autenticación se enciende. Se descarta «montarla pero exigir sesión»: sin proveedor de identidad no hay sesión que exigir, así que la ruta solo puede contestar `404`, igual que `/api/v1/auth/*` en ese mismo modo.

El panel no usa esas rutas (no hay ninguna llamada a `/api/v1/tokens` en `ui/`), así que no hay trabajo de frontend.

## D6 · Dónde se valida

La configuración se valida **inline** en `from_lookup` (`src/config.rs:186`), en orden: clave maestra, bind, base de datos, filtro de registro, HSTS, modo de autenticación, secreto de sesión, socket de Docker. La regla cruzada «sin autenticación no vale cualquier bind» necesita las dos piezas, así que se comprueba **después** de resolver el modo, sin reordenar el resto de la lectura: el orden actual es el que fija qué error se ve primero cuando hay varios problemas, y cambiarlo alteraría mensajes ya probados.

Variantes nuevas de `ConfigError`, en la línea de las tres que ya existen (`MissingVariable`, `InvalidValue`, `IncompleteOidc`):

- `InsecureModeNotAllowed { name }` — nombra `KESTO_ALLOW_INSECURE`.
- `InsecureBindAddress { name }` — nombra `BIND_ADDRESS`.
- `WeakCredential { name, min }` — nombra la variable y el mínimo, **nunca el valor**.

## D7 · Versión y forma de los commits

El change rompe compatibilidad de dos maneras: un despliegue con credencial corta deja de arrancar, y el bind por defecto cambia. Aun así los commits se marcan `feat:`/`fix:` **sin** `!` y sin `BREAKING CHANGE`, porque el pipeline deduce `major` con esa marca y este proyecto va por `0.x`: el bump que corresponde es `0.18.0`. La incompatibilidad se describe en el cuerpo del PR, en el CHANGELOG y en la documentación, que es donde el operador la va a leer.
