# Spec Delta — SEC‑22 (resto): socket :ro y validación de contenedores

## Contexto
Parte del change `grupo-5-sec-15-sec-22-resto` (refs #168). El socket Docker (`/var/run/docker.sock`) está montado en modo `:ro` en `compose.yaml`, pero el código no lo valida en el borde HTTP. Adicionalmente, los contenedores objetivo deben declarar la etiqueta `io.kesto.backup-source=true`; sin ella, el motor no debe permitir `exec`. El issue #173 (SEC‑06 2/2) requiere separar el privilegio del motor de los de la API.

## Contrato (POST‑change)
**Endpoint:** `POST /api/v1/repositories` y `PUT /api/v1/repositories` (creación/edición de repositorio con `exec`)  
**Autenticación:** requerida (modo OIDC o `KESTO_ALLOW_INSECURE_ENDPOINT=1`)  
**Respuesta exitosa (201/200):**  
- El endpoint acepta un `endpoint` URL de destino.  
- **Validación:** Si el `endpoint` apunta a `/var/run/docker.sock` o a cualquier socket Docker, debe ser `https` o `http` solo si `KESTO_ALLOW_INSECURE_ENDPOINT=1` y el socket es `:ro`. Cualquier otra URL de socket se rechaza.  
- **Etiqueta requerida:** El contenedor destino debe tener la etiqueta `io.kesto.backup-source=true`; si falta, respuesta `400` con `error: "missing_label", message: "El contenedor debe tener la etiqueta io.kesto.backup-source=true"`.

**Errores:**
- 400 si el `endpoint` es un socket sin el permiso `KESTO_ALLOW_INSECURE_ENDPOINT=1`, o si el socket es `rw` (lectura/escritura), o si el contenedor carece de la etiqueta requerida.
- 401 si no hay autenticación.
- 500 para fallos internos.

## Escenarios BDD

### Escenario 1 — Happy path: endpoint Docker :ro con etiqueta correcta
**Dado** una petición para crear un repositorio con un endpoint `https://...` que apunta a un contenedor con etiqueta `io.kesto.backup-source=true`  
**Cuando** el cliente hace `POST /api/v1/repositories` con el payload válido  
**Entonces** la respuesta es `201` y el repositorio se crea  
**Y** el motor valida la etiqueta antes de crear `exec`  
**Y** el socket usado es de solo lectura

### Escenario 2 — Sin etiqueta: contenedor sin `io.kesto.backup-source=true`
**Dado** una petición con un contenedor que carece de la etiqueta `io.kesto.backup-source=true`  
**Cuando** el cliente hace `POST /api/v1/repositories` con ese endpoint  
**Entonces** la respuesta es `400` con `error: "missing_label", message: "El contenedor debe tener la etiqueta io.kesto.backup-source=true"`  
**Y** el `exec` **no** se crea

### Escenario 3 — Socket sin permiso: endpoint http sin `KESTO_ALLOW_INSECURE_ENDPOINT=1`
**Dado** una petición con `endpoint` que apunta a `http://host/docker.sock` y sin `KESTO_ALLOW_INSECURE_ENDPOINT=1`  
**Cuando** el cliente hace `POST /api/v1/repositories`  
**Entonces** la respuesta es `400` con `error: "insecure_endpoint", message: "Endpoint Docker debe ser https o tener KESTO_ALLOW_INSECURE_ENDPOINT=1"`  
**Y** no se crea el `exec`

### Escenario 4 — Edge case: endpoint localhost con loopback
**Dado** una petición con `endpoint` `http://localhost:2375` y `KESTO_ALLOW_INSECURE_ENDPOINT=1`  
**Cuando** el cliente hace `POST /api/v1/repositories`  
**Entonces** la respuesta es `201` (el permiso concede endpoint local en claro)  
**Y** el motor valida la etiqueta del contenedor

### Escenario 5 — Compatibilidad: probes que validan salud del motor
**Dado** un probe que verifica que la API rechace endpoints inseguros  
**Cuando** el probe consulta `/api/v1/repositories` con payloads inválidos  
**Entonces** el comportamiento del probe **no cambia** (sigue validando códigos y respuestas esperadas)

## Notas de implementación
- Validar el `endpoint` en `src/api/repositories.rs` (único consumidor del borde HTTP) antes de hablar con el motor (`src/rustic/endpoint.rs`).  
- Usar la función `endpoint_admisible` existente (documentada en `src/api/repositories.rs`) y extenderla con la chequeo de etiqueta.  
- La edición (PUT) también debe validar; el alta conserva su `502` para «el destino no responde».  
- No modificar la configuración de compose (`:ro` ya está); el código debe reforzar la validación.  
- Actualizar `docs/DEFINITION.md` §5 y los docs de feature correspondientes.
