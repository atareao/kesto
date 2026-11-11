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
    ///
    /// Dice «la passphrase no lo abre» y **no** «la passphrase es la equivocada», porque lo
    /// único medido es lo primero: lo que la biblioteca sabe decir es que **no hay ninguna
    /// clave del repositorio que se deje abrir con esta contraseña**, y eso cubre dos cosas
    /// que se arreglan de forma distinta.
    ///
    /// 1. La passphrase no es la del repositorio: el operador la revisa y ya está.
    /// 2. El repositorio **no tiene ninguna clave utilizable** —le falta el fichero de
    ///    claves, o lo tiene corrupto, o quedó a medio inicializar—. Aquí la passphrase
    ///    puede ser la correcta y no haber forma de comprobarlo: no hay clave contra la que
    ///    contrastarla.
    ///
    /// Está **medido** con un destino que conserva su `config` y al que se le han retirado
    /// los ficheros de `keys/`: la biblioteca cae por el mismo camino que con una
    /// contraseña equivocada y devuelve el mismo código `C002`, así que este módulo —que
    /// distingue los desenlaces por ese código y no por el texto— reporta el caso 2 como si
    /// fuera el 1. La variante dice, por eso, lo único que se puede afirmar sin mentir: que
    /// **no hay ninguna clave que abra con esta passphrase**. El `motivo` es el de la
    /// biblioteca, y nombra la comprobación que falló —«no suitable key found»—, no la
    /// causa que la hizo fallar.
    ///
    /// La consecuencia, para quien lea esto en un informe: este error manda a mirar la
    /// passphrase **y** el destino, en ese orden; no autoriza a dar por buena la passphrase
    /// ni a suponer que el repositorio está entero.
    #[error("la passphrase no abre el repositorio de `{destino}`: {motivo}")]
    PassphraseNoAbre { destino: String, motivo: String },

    /// El nombre del volcado de la copia y el del origen **no coinciden**.
    ///
    /// Es el fallo que deja una instantánea que **declara** contener una cosa y contiene
    /// otra: [`Copia::nombre`](crate::rustic::Copia) es el `path` que la instantánea
    /// publica, y el del origen es el del nodo que el puente fabrica de verdad dentro del
    /// árbol. Si los dos nombres divergen, el listado dice que hay un
    /// `volcado-de-tienda.sql` donde hay un `volcado-de-otra-cosa.sql`, y quien vaya a
    /// restaurar buscará un fichero que no está.
    ///
    /// Importa porque es la versión en metadatos del fallo que este módulo existe para no
    /// cometer: la copia existe, ocupa sitio y devuelve algo que no es lo que su ficha
    /// promete. Se comprueba **antes** de archivar, así que de aquí no sale nada escrito.
    ///
    /// El error es de la pareja, no de uno de los dos nombres: cada uno puede ser
    /// perfectamente válido por separado —los dos pasan la validación de
    /// [`tunel_del_volcado`](crate::rustic::tunel_del_volcado)— y lo que está mal es que
    /// sean distintos.
    #[error(
        "la copia declara el volcado `{en_la_copia}` y el origen que se le entrega trae \
         `{en_el_origen}`: el nombre que la instantánea publica tiene que ser el del nodo que \
         se archiva, o la copia quedará diciendo que contiene lo que no tiene"
    )]
    NombreDeVolcadoIncoherente {
        /// El nombre que la copia declara: el que queda escrito en la instantánea.
        en_la_copia: String,
        /// El nombre del origen: el del nodo que el puente archiva de verdad.
        en_el_origen: String,
    },

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

    /// Las tres reglas de retención valen cero, así que la retención no conserva **nada**.
    ///
    /// Es un guardarraíl y no una validación de estilo: un `0` suelto es una instrucción
    /// legítima —`keep_daily = 0` con `keep_monthly = 12` es «no me guardes nada diario,
    /// guárdame lo mensual»—, pero **las tres a la vez** no son una retención que conserva
    /// de menos: son la destrucción de todo el historial del repositorio, incluida la copia
    /// que se acaba de hacer. Las reglas son válidas para la biblioteca y la biblioteca las
    /// obedece, así que sin este rechazo el motor retiraría y purgaría todo sin que nada lo
    /// delatara.
    ///
    /// Y es alcanzable: el §3 pone `DEFAULT 7` en las tres columnas pero **sin cota**, así
    /// que un `0` entra por la API y entra a mano con `sqlite3`.
    ///
    /// Lo que **no** se hace aquí es corregir el valor: no se sube un cero ni se cambia por
    /// el defecto. Corregir en silencio sería decidir por el operador qué copias se tiran,
    /// y la dirección de la corrección —conservar de más o de menos— no es la misma en los
    /// dos casos. El motor rechaza y quien lo lea decide.
    #[error(
        "las tres reglas de retención valen cero (`keep_daily`, `keep_weekly` y \
         `keep_monthly`): eso no conserva de menos, borra el historial entero del \
         repositorio —incluida la copia recién hecha—, así que no se aplica"
    )]
    RetencionQueLoBorraTodo,

    /// No se han podido listar las instantáneas del repositorio.
    #[error("no se han podido listar las instantáneas de `{destino}`: {motivo}")]
    ListadoFallido { destino: String, motivo: String },
}
