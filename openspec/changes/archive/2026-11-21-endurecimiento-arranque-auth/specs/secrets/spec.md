# secrets — delta

## Purpose

Cómo se tratan los secretos de Kesto —cifrado en reposo, volcado de arranque y credenciales del entorno— sin publicarlos por accidente. Este delta invierte el requisito de la credencial débil: deja de ser un aviso que acompaña al arranque y pasa a ser una puerta que lo impide.

## REMOVED Requirements

### Requirement: Una credencial débil se denuncia sin impedir el arranque

**Reason**: el aviso deja el sistema en producción con la clave de cifrado en reposo al alcance de cualquiera. Con `KESTO_MASTER_KEY` vacía —un `.env` que no se carga, un `compose` que interpola a vacío— HKDF-SHA256 deriva con sal `None` y la etiqueta constante `kesto/secrets/v1` una clave AES-256 determinista y públicamente calculable: quien lea el `.db` descifra `repositories.secret_access_key`, `repositories.passphrase` y `backup_targets.db_password`. En modo OIDC, una `KESTO_SECRET` vacía o corta firma las sesiones con una clave que cualquiera puede reproducir, y un JWT de sesión forjado es un bypass de autenticación. No es una función accesoria degradada: es el secreto que protege todos los demás.

**Migration**: `KESTO_MASTER_KEY` (siempre) y `KESTO_SECRET` (en modo `Oidc`) SHALL medir al menos 32 caracteres. Un despliegue existente con una credencial más corta dejará de arrancar al actualizarse; el mensaje nombra la variable y el mínimo, y `docs/ENTORNO.md` explica el cambio. Rotar `KESTO_MASTER_KEY` deja ilegibles los secretos ya cifrados, así que la reparación de un despliegue con datos es rotar **y** volver a dar de alta las credenciales de los repositorios.

## ADDED Requirements

### Requirement: Una credencial presente y débil impide el arranque

`KESTO_MASTER_KEY` —obligatoria siempre— SHALL medir al menos **32 caracteres**, y una que esté presente y mida menos SHALL ser un error de arranque que nombre la variable y el mínimo. La credencial **vacía** cuenta como débil: una variable definida pero vacía NO SHALL tratarse como ausente ni aceptarse por el solo hecho de estar.

`KESTO_SECRET` SHALL medir esos mismos 32 caracteres **cuando el modo sea `Oidc`**, que es cuando se usa y cuando es obligatoria. Sin proveedor de identidad la variable NO SHALL leerse ni validarse: no hay sesión que firmar, así que no SHALL poder impedir el arranque del modo de desarrollo.

El error NO SHALL incluir el valor de la credencial.

La credencial **ausente** NO SHALL confundirse con una débil: la ausencia la decide `config`, y en modo `Disabled` `KESTO_SECRET` no hace falta.

#### Scenario: Una clave maestra corta no arranca
- **GIVEN** un entorno con `KESTO_MASTER_KEY` de 12 caracteres
- **WHEN** se carga la configuración
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `KESTO_MASTER_KEY` y el mínimo de 32 caracteres
- **AND** el mensaje NO SHALL incluir el valor de la clave

#### Scenario: Una clave maestra vacía no arranca
- **GIVEN** un entorno con `KESTO_MASTER_KEY=` presente pero vacía
- **WHEN** se carga la configuración
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `KESTO_MASTER_KEY`

#### Scenario: En modo OIDC una clave de firma corta no arranca
- **GIVEN** un entorno con las tres variables `OIDC_*` y `KESTO_SECRET` de 12 caracteres
- **WHEN** se carga la configuración
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `KESTO_SECRET` y el mínimo de 32 caracteres

#### Scenario: Dos credenciales largas arrancan sin aviso
- **GIVEN** `KESTO_MASTER_KEY` y `KESTO_SECRET` de 64 caracteres cada una
- **WHEN** la configuración se carga
- **THEN** SHALL ser válida
- **AND** NO SHALL emitir ningún aviso sobre credenciales

#### Scenario: La ausencia no es debilidad
- **GIVEN** un entorno en modo `Disabled` con `KESTO_MASTER_KEY` de 64 caracteres y sin `KESTO_SECRET`
- **WHEN** la configuración se carga
- **THEN** SHALL ser válida
- **AND** NO SHALL fallar por `KESTO_SECRET`
