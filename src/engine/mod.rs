//! El motor de copias: qué se copia, por dónde sale y en qué orden.
//!
//! Este módulo es la lógica del §4 y del §2 del SRS. Hoy tiene **la mitad de la extracción**
//! —[`extraccion`]—, que es la primera de las tres piezas del motor y la que va de la fila
//! del §3 hasta los bytes que hay que copiar: la elección de la ruta, el comando de volcado
//! de cada tipo de target, la credencial por el entorno del `exec` y la conducción de ese
//! proceso hasta el puente de la biblioteca de copia.
//!
//! La otra mitad —la cola y el worker por repositorio, el ciclo de vida de una ejecución en
//! `job_executions`, el hub de eventos del SSE y el arranque y la parada ordenada— llega con
//! el resto del change. Lo que hay aquí no la necesita para estar en pie y tiene su contrato
//! entero en `openspec/changes/backup-engine/specs/extraction/`.
//!
//! # El nombre del error
//!
//! Cada mitad del motor tendrá su error —la extracción ya lo tiene—, y ninguno se llama
//! `Error` a secas: cuando este módulo reexporte los de la orquestación junto a éste, y el
//! motor tenga que nombrar a la vez su error, el del transporte
//! ([`crate::docker::Error`]) y el de la biblioteca ([`crate::rustic::Error`]), un `Error`
//! pelado obligaría a alias en cada import y dejaría el nombre más corto para el que menos lo
//! merece. Ver [`ErrorDeExtraccion`], que es donde está escrito con detalle.
//!
//! # Por dónde va el volcado, en una línea
//!
//! ```text
//! fila del §3 → Objetivo → ruta() → { PorContenedor → volcar() → puente → Rustic
//!                                  { PorFichero    → el origen local de la biblioteca
//! ```
//!
//! El puente —[`crate::rustic::tunel_del_volcado`]— vive en `src/rustic` y **no** aquí: es un
//! tipo que existe para la API de la biblioteca, y traerlo al motor obligaría al motor a
//! importar `rustic_core`, que es justo lo que la separación evita. Lo que sí vive aquí es
//! quién lo alimenta y con qué plazo: ver [`INACTIVIDAD_MAXIMA`].

pub mod extraccion;

pub use extraccion::{
    CAPACIDAD_DEL_CANAL, ErrorDeExtraccion, INACTIVIDAD_MAXIMA, Objetivo, Resultado, Ruta, ruta,
    volcar,
};

// Las dos constantes del puente, que el motor pasa a las dos esperas del volcado. Son las
// **mismas** para el transporte y para el lector del canal —es la misma pregunta vista desde
// dos sitios— y por eso se publican desde aquí: quien orqueste la copia las pasa tal cual en
// vez de elegir un número por su cuenta. `pub` y no `pub(crate)` por la regla de la casa:
// hasta que llegue la orquestación, lo único que toca estos dos números es esta mitad, y un
// `pub(crate)` sin usuario es código muerto para el compilador.
