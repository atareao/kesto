# FEATURE-056 — Parada opcional de contenedores antes del backup

Issue #210

## Qué es

Una copia de una base que sigue escribiendo puede no ser consistente. Esta feature permite declarar una lista de contenedores que se detienen **antes** de la copia y se reinician **después**, incluso si la copia falla o el hilo de la ejecución entra en pánico.

No hay ninguna bandera de línea de comandos: Kesto es un servidor con API HTTP, no un ejecutable con comando `backup`. El único control es la variable `KESTO_STOP_CONTAINERS`.

## Ficheros implicados

| Fichero | Papel |
|---|---|
| `src/config.rs` | Lee y valida `KESTO_STOP_CONTAINERS` en la API. |
| `src/motor/config.rs` | Lee la misma variable en `kesto-motor` y la guarda como lista autorizada. |
| `src/engine/mod.rs` | `Ajustes::con_stop_containers` lleva la lista al contexto del motor. |
| `src/engine/ejecucion.rs` | Orquesta stop → copia → reinicio, con guardia RAII y cierre de la fila. |
| `src/docker/cliente.rs` | `stop_container`/`start_container` sobre Bollard, en el proceso privilegiado. |
| `src/docker/ipc_cliente.rs`, `src/ipc/mensaje.rs` | Solicitudes IPC: una nueva (`DetenerContenedor`) y `IniciarContenedor`, que ya existía. |
| `src/motor/servidor.rs` | Atiende las solicitudes y comprueba la lista autorizada antes de detener. |
| `src/main.rs` | Pasa la configuración efectiva a los ajustes del motor. |
| `compose.yaml` | Declara la misma variable en los dos servicios. |
| `docs/ENTORNO.md` | Documentación de la variable. |

## Cómo funciona

1. Al arrancar, la API y el motor leen la misma lista. Cada nombre se recorta, se valida —ASCII alfanumérico; `_`, `.` y `-` después del primer carácter— y se deduplica conservando el orden. Un nombre inválido impide el arranque y nombra la variable sin publicar el valor.
2. Cada ejecución entra en `ejecutar_con`. Con la lista vacía la copia sigue exactamente como antes. Con contenedores declarados se toma un candado de proceso —la lista es global, pero hay un worker por repositorio— y se pide el control del motor por IPC.
3. Se detiene cada contenedor en orden. Si uno falla, se reinician los que ya se detuvieron, la fila acaba en `failed` con `finished_at` y el motivo nombra el contenedor; la copia no empieza.
4. Si los stops salen bien, se crea una guardia RAII y se hace la copia. Al terminar —éxito, fallo o pánico— la guardia reinicia los contenedores detenidos. En el camino normal devuelve un aviso si alguno no volvió; en el pánico lo registra en el traza y la cola se encarga de cerrar la fila.
5. Un aviso de reinicio no cambia el veredicto de la copia: la ejecución sigue siendo `success`, pero el texto se guarda en `error_message` y se publica como evento `execution_progress` con el aviso en `current_action`, seguido del evento final.

El motor es el único proceso con acceso al socket del daemon (`DOCKER_HOST`). La API no lo lee: solo conoce el socket IPC y la lista blanca de siete operaciones, dos de ellas nuevas.

## Configuración y despliegue

```yaml
# .env
KESTO_STOP_CONTAINERS=web,db
```

El `compose.yaml` del repositorio ya pasa la variable —con valor vacío por defecto— a los servicios `kesto` y `kesto-motor`. Declárala en el entorno del despliegue para activar la feature; omítela o déjala vacía para no detener nada. Si se cambia el valor hay que reiniciar ambos servicios: se lee una sola vez, al arrancar.

Los contenedores declarados tienen que existir y el motor tiene que poder detenerlos y arrancarlos. No se comprueba su estado antes de la copia: `stop` sobre un contenedor ya detenido cuenta como éxito, y `start` lo deja en marcha.

## Decisiones

- **Variable, no CLI.** No existe un comando `backup` al que añadir una bandera; la configuración del servidor es la única frontera.
- **lista autorizada en el motor.** La API ya valida los nombres, pero el motor repite la comprobación: un mensaje IPC forjado o un fallo de validación en la API no puede detener un contenedor arbitrario.
- **Guardia RAII.** El reinicio en el camino normal no cubre un pánico de la biblioteca de copias; el `Drop` sí corre durante el unwind.
- **Aviso, no fallo.** Si la copia terminó bien, convertir un reinicio fallido en `failed` mentiría sobre la copia; en cambio ocultarlo dejaría un contenedor parado sin que nadie lo supiera. Se guarda y se publica.
- **Candado de proceso.** `stop` es global; sin serialización, dos ejecuciones de repositorios distintos podrían reiniciarse mutuamente los contenedores a mitad de la copia.

## Seguimiento

- Los errores de SQL dentro de una sección de restauración PostgreSQL siguen sin detectarse (#175): asunto ajeno a esta feature.
- No se guarda el estado previo de cada contenedor: la operación asume que los contenedores declarados deben estar en marcha después de la copia.
