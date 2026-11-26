-- Sesiones revocadas (issue #168, hallazgo SEC-17): el registro de las sesiones que el
-- cierre de sesión invalida antes de su caducidad.
--
-- La sesión es un JWT **sin estado**: su firma sola no permite cerrarla antes de que su
-- `exp` llegue. `jti` es el identificador único que la emisión pone en el token —aleatorio
-- y distinto por emisión—, y esta tabla guarda los que se han revocado para que el *gate*
-- los rechace en cada petición. La revocación vive en la base y no solo en memoria: un
-- reinicio borraría un mapa en memoria y una cookie revocada volvería a servir hasta su
-- `exp`, que es justo el arreglo que se desharía solo y en silencio.
--
-- `exp` es el `exp` del token, en segundos desde la época: pasado ese instante la sesión ya
-- no vale por sí sola, así que la revocación no protege nada y es lo que permite purgar la
-- tabla (`DELETE ... WHERE exp <= ?`) para que no crezca sin fin. No hace falta un índice
-- sobre `exp`: la lectura es por la clave primaria y la purga recorre lo que haya.
--
-- La fila **no se conserva** al caducar: a diferencia de `api_tokens` —que se guarda para
-- auditar cuándo se emitió y cuándo se revocó—, aquí la revocación solo significa algo
-- mientras la sesión viviría, y una fila caducada es una fila que ya no protege nada.

CREATE TABLE IF NOT EXISTS sesiones_revocadas (
    jti TEXT PRIMARY KEY NOT NULL,   -- el `jti` del JWT revocado
    exp INTEGER NOT NULL             -- el `exp` del token, en segundos desde la época
);
