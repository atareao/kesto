//! El error del módulo: lo que puede fallar al hablar con la biblioteca de copia.
//!
//! Es **un** error para todo `src/rustic` y no uno por fichero, y la razón es que quien
//! llama —el motor— trata con un solo flujo: poner el repositorio en marcha, archivar y
//! retener. Un tipo distinto por submódulo le obligaría a envolver y desenvolver errores
//! para contarlos, y en un motor cuyo trabajo es decidir si una copia se da por buena o no,
//! lo que importa es **qué** falló, no de qué fichero salió.
//!
//! Este tipo vivía en `origen.rs`, junto al puente, cuando solo el puente podía rechazar
//! algo. Con los desenlaces del destino, los de la copia, los de la retención y los del
//! listado dentro, el puente ha dejado de ser su dueño: lo que hay aquí es el error del
//! módulo entero, y por eso tiene su propio fichero.
//!
//! # Lo que un motivo **no** puede llevar
//!
//! Ni la clave secreta ni la passphrase, en ninguna variante. Los dos secretos del §3 son
//! [`SecretString`](crate::secret::SecretString), que redacta su `Debug` y su `Display`,
//! así que no pueden colarse por formateo; y los motivos que vienen de la biblioteca se
//! componen con [`display_log`](rustic_core::RusticError::display_log), que interpola el
//! contexto del fallo —rutas, nombres de destino, códigos— y nunca el material de una
//! clave: es el mismo texto que la biblioteca escribe en su registro.

use std::time::Duration;

/// Error del trato con la biblioteca de copia.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// El nombre del volcado no sirve como nombre de fichero.
    #[error("el nombre del volcado no puede estar vacío ni llevar separadores: {nombre:?}")]
    NombreDeVolcadoInvalido { nombre: String },

    /// El productor dejaría de esperar a un lector que ya no lee.
    #[error(
        "el archivado del volcado `{nombre}` se rindió y ya nadie lee del túnel: el productor \
         tiene que terminar su bucle y morir, sin entrar en pánico —un pánico aquí deja el hilo \
         envenenado y la unión de la copia se cuelga—"
    )]
    ElArchivadoSeRindio { nombre: String },

    /// El lector llevaba demasiado tiempo sin recibir un solo byte.
    #[error(
        "el volcado `{nombre}` lleva más de {inactividad:?} sin entregar un solo byte: el \
         productor puede estar muerto o atascado, y el canal sigue abierto porque su extremo vive \
         en alguna parte —archivarlo así daría por buena una copia incompleta—"
    )]
    VolcadoEstancado {
        nombre: String,
        inactividad: Duration,
    },

    /// La fila del repositorio no describe un destino que pueda funcionar.
    ///
    /// Es lo que se comprueba **antes** de hablar con nadie: un endpoint, un cubo o una
    /// credencial vacíos no son una configuración floja —esas se avisan y se siguen—, son
    /// una fila que no puede abrir nada.
    #[error("la fila del repositorio no puede ponerse en marcha: `{campo}` está sin informar")]
    AccesoInvalido { campo: String },

    /// El destino no se alcanza, o se alcanza y no se puede preparar.
    ///
    /// Los tres desenlaces se deciden preguntándole al destino, así que esta variante
    /// recoge las dos caras de la misma pregunta: que la pregunta misma falle —es lo que
    /// devuelve S3 cuando el cubo no está o la credencial no vale— y que, habiendo
    /// respondido que el repositorio no existe, no se pueda crear. El `motivo` dice cuál de
    /// las dos fue, porque el destino se nombra en los dos casos y lo que cambia es lo que
    /// hay que hacer después.
    ///
    /// **No** recoge el caso de una passphrase que no abre: ése tiene su variante, porque
    /// el operador lo arregla de otra manera.
    #[error("no se ha podido poner en marcha el repositorio en `{destino}`: {motivo}")]
    DestinoNoAlcanzable { destino: String, motivo: String },

    /// El repositorio existe y la passphrase no lo abre.
    #[error("la passphrase no abre el repositorio de `{destino}`: {motivo}")]
    PassphraseNoAbre { destino: String, motivo: String },

    /// La copia no declara ninguna ruta.
    ///
    /// La biblioteca indexa la primera ruta sin preguntar, así que una copia sin rutas la
    /// haría entrar en pánico dentro de una dependencia. Se rechaza antes.
    #[error("la copia de la tarea `{etiqueta}` no declara ninguna ruta que copiar")]
    CopiaSinRutas { etiqueta: String },

    /// El archivado de la copia falló.
    ///
    /// **Que no aparezca no significa que la copia esté completa**: el archivado de la
    /// biblioteca silencia el fallo del lector y termina sin error, así que la condición
    /// de éxito la compone el motor y no se hereda de aquí.
    #[error("ha fallado el archivado de la copia de la tarea `{etiqueta}`: {motivo}")]
    CopiaFallida { etiqueta: String, motivo: String },

    /// Falló una de las tres operaciones de la retención.
    ///
    /// El motivo dice cuál —leer las instantáneas, retirarlas o purgar—. Una retención que
    /// falla **no** cambia el resultado de la copia: la copia ya está hecha y es correcta.
    #[error("ha fallado la retención: {motivo}")]
    RetencionFallida { motivo: String },

    /// No se han podido listar las instantáneas del repositorio.
    #[error("no se han podido listar las instantáneas de `{destino}`: {motivo}")]
    ListadoFallido { destino: String, motivo: String },
}
