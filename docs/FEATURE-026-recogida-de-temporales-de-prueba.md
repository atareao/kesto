# FEATURE-026 — Recogida de temporales de prueba

- **Issue**: #94 · **Plan**: `plans/PLAN-026.md` · **Change SDD**: `recogida-de-temporales-de-prueba` · **Capacidad nueva**: `arnes-de-pruebas`

## Qué es

El arnés de las pruebas dejaba directorios `/tmp/kesto-prueba-*` huérfanos hasta agotar los inodos de la máquina. Este change lo corrige: una raíz única por ejecución, un barrido al arrancar que retira lo de procesos ya muertos y una receta `just clean-tmp` para limpiar a mano. La acumulación queda **acotada a ≤1 raíz** —la de la corrida en curso—.

## El problema (medido)

`/tmp` llegó a **17.375 directorios huérfanos** y `df -i` al **100 %** (1.048.115/1.048.576 inodos) con disco de sobra. El fallo es acumulativo, **miente** —se manifiesta como un `panic` en `src/test_support.rs` con `StorageFull`, que parece un problema del test que lo sufre y no de la máquina— y **tumba tests ajenos**. Solo golpea en local: el CI usa un runner limpio en cada trabajo, así que nunca lo vio.

Causa, en dos partes:

1. **Las guardas del arnés se retienen en `static GUARDAS_DEL_ARNES`**, que **nunca** ejecuta `Drop` mientras vive el proceso. El grueso se filtra **en el camino feliz** —≈64 guardas por corrida—.
2. **La muerte abrupta** (panic, `Ctrl-C`, `timeout`) tampoco ejecuta los `Drop` pendientes, así que ni siquiera una salida ordenada habría bastado.

## Qué cambia

- **Raíz única por ejecución**: `/tmp/kesto-prueba-<pid>/`, con un subdirectorio por prueba. El aislamiento se mantiene intacto y la recogida se hace de un golpe. La materializan `raiz_de_la_ejecucion()` y `directorio_de_prueba()` en `src/test_support.rs`.
- **Barrido al arrancar** (perezoso, una vez por proceso con `OnceLock`, al primer uso de la raíz): borra las entradas `kesto-prueba-<pid>` cuyo pid **no** está vivo (`/proc/<pid>`) y los restos `<uuid>` heredados; **conserva** las raíces de procesos vivos. Es el mismo patrón que ya usa el arnés e2e (`kesto-e2e-pid`).
- **La limpieza ya no se apoya en el `Drop`**: la retención de guardas se mantiene —evita un `404` intermitente al retirar el directorio antes de que la prueba termine de leerlo— pero deja de ser la única limpieza. La garantía la da el barrido.
- **Aviso de inodos, no bloqueo**: si quedan menos de `INODOS_LIBRES_MINIMOS = 50_000` inodos libres en el directorio temporal, el arnés lo **dice** por `eprintln!` y **no** falla. Se lee con `std::process::Command` sobre `df -P -i` (`LC_ALL=C`); si eso falla, no se avisa.
- **Sin dependencias nuevas**: solo `std`. `libc` y `rustix` estaban en el lock pero **no** son dependencias directas, y no se añadieron.
- **`just clean-tmp`**: receta + `scripts/clean-tmp.sh` (base inyectable; conserva pids vivos, borra muertos y restos heredados, no toca nada sin el prefijo) + test `scripts/clean-tmp.test.sh` + entrada en `docs/COMANDOS.md`.

## Ficheros implicados

| Fichero | Papel |
|---|---|
| `src/test_support.rs` | `raiz_de_la_ejecucion()`, `directorio_de_prueba()`, barrido por pid vivo, aviso de inodos (`INODOS_LIBRES_MINIMOS`) |
| `scripts/clean-tmp.sh` | Limpieza manual con base inyectable; conserva pids vivos, borra muertos y restos heredados |
| `scripts/clean-tmp.test.sh` | Test del script anterior |
| `.justfile` | Receta `clean-tmp` |
| `docs/COMANDOS.md` | Documenta la receta |

## Cómo funciona / comportamiento

- **El barrido es perezoso**: se dispara al primer uso de la raíz, **no** en `main`. Así no cuesta nada a los procesos que no tocan el arnés.
- **Conserva la raíz de cualquier proceso vivo**: por eso dos `cargo test` simultáneos no se pisan —cada uno barre solo lo muerto y respeta lo del otro—.
- **Un sufijo numérico de un pid reciclado y vivo se conserva** (resto ajeno): es aceptable y **deliberado** —preferimos conservar de más que borrar algo vivo—.

## Decisiones

- **D1 — raíz por pid con subdirectorio por prueba.** Agrupa todo lo de una corrida bajo un único directorio y permite la recogida de un golpe sin perder el aislamiento entre pruebas.
- **D2 — barrido por pid vivo con `/proc`.** Ataca la causa raíz: ni un `static` ni una muerte abrupta ejecutan `Drop`, así que la limpieza tiene que decidirla un tercero que arranque después. `/proc/<pid>` es la comprobación de vida sin dependencias.
- **D3 — la retención de guardas se mantiene; deja de ser la única limpieza.** Retirar el directorio en el `Drop` producía un `404` intermitente; se conserva la retención y la garantía de recogida pasa al barrido.
- **D4 — receta `clean-tmp` documentada.** El barrido automático no cubre el caso de querer limpiar sin correr pruebas; la receta lo cierra y se documenta en `docs/COMANDOS.md`.
- **D5 — el aviso de inodos avisa y no bloquea.** La falta de inodos es un síntoma de la máquina, no un fallo de la prueba; abortar por ella rompería corridas legítimas. Se avisa por `eprintln!` y se sigue.

## Variables y configuración

**Ninguna nueva.** `INODOS_LIBRES_MINIMOS` es una constante de implementación, no una variable de entorno. La tabla completa de variables sigue en `docs/ENTORNO.md`.

## Cómo verificar

- **Puertas**: `just check-all` en verde (Rust + panel + scripts).
- **Prueba manual del criterio de #94**: crear raíces de pids muertos en `/tmp`, correr `cargo test` y comprobar que se barren y que el número de `kesto-prueba-*` **no crece** entre ejecuciones; `df -i /tmp` no crece.
- **Limpieza a mano**: `just clean-tmp`.

## Follow-ups

1. **El aviso de inodos depende de `df`** (coreutils) y de `-P -i` (GNU/POSIX); si algún día se quiere sin depender de `df`, hará falta `statvfs` —y con él una dependencia directa tipo `libc`/`rustix`—, que este change evitó **a propósito**.
2. **Las raíces de pids vivos se conservan**: si un proceso muere **y** su pid se recicla, su raíz sobrevive hasta la siguiente corrida con ese pid muerto. Es raro y benigno.

## Referencias

- Issue: **#94** (temporales de prueba que agotan los inodos).
- Change SDD: `openspec/changes/recogida-de-temporales-de-prueba/`.
- Plan: `plans/PLAN-026.md`.
