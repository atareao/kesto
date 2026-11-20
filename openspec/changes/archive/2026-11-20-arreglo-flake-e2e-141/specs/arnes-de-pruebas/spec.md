# arnes-de-pruebas — delta

## Purpose

El arnés de pruebas de Kesto es el conjunto de ayudantes que crean, aíslan y recogen lo que una prueba necesita del sistema —un directorio temporal, una base SQLite migrada, una clave—. Su trabajo es que cada prueba corra sobre recursos propios y que, al terminar la batería, no quede nada suyo acumulándose fuera de su sitio: ni un directorio que enturbie la siguiente corrida, ni un recurso del sistema —un inodo— agotado por no haberse recogido.

## ADDED Requirements

### Requirement: El alta de un repositorio tolera un destino aún no listo bajo carga

El arnés SHALL reintentar de forma **acotada** el alta de un repositorio ante una respuesta `502` del servidor, y NO SHALL reintentar ante ningún otro código ni tras un `2xx`. El reintento SHALL usar un número de intentos pequeño y una espera con backoff entre intentos, reenviando la misma petición cada vez.

Reintentar el alta SHALL ser seguro porque un `502` deja **cero** filas escritas: la fila del repositorio se escribe después de abrir el destino, así que un `502` no deja estado parcial que un segundo intento pudiera duplicar.

Agotados los intentos con `502`, el arnés SHALL fallar en voz alta nombrando el código de la última respuesta y su cuerpo, en vez de devolver un valor vacío.

#### Scenario: Un destino recién arrancado se reintenta hasta que responde
- **GIVEN** un arnés cuyo destino responde `502` en el primer intento y `2xx` en el siguiente
- **WHEN** el arnés da de alta un repositorio
- **THEN** SHALL reenviar la petición tras el `502`
- **AND** SHALL devolver los datos del `2xx`

#### Scenario: Un `2xx` no se reintenta
- **GIVEN** un arnés cuyo destino responde `2xx` al primer intento
- **WHEN** el arnés da de alta un repositorio
- **THEN** SHALL hacer una sola petición
- **AND** SHALL devolver sus datos

#### Scenario: Un `502` persistente falla en voz alta
- **GIVEN** un arnés cuyo destino responde `502` a todos los intentos
- **WHEN** el arnés agota los intentos
- **THEN** SHALL fallar nombrando el código de la última respuesta y su cuerpo
- **AND** NO SHALL devolver un valor vacío
