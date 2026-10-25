//! La retención: decidir qué instantáneas sobran, retirarlas y purgar sus datos huérfanos.
//!
//! El §4.B del SRS describe la retención en dos tiempos —«evaluación del árbol de
//! snapshots» y después «eliminación de datos huérfanos»—, y la biblioteca la expone en
//! **tres** operaciones. No es un reparto de gusto y quien las confunda escribe un motor
//! que purga datos que nunca dejaron de estar referenciados, o que retira instantáneas sin
//! reclamar nada:
//!
//! 1. **Decidir** qué instantáneas se retiran, evaluando las **reglas de retención** sobre
//!    las instantáneas del repositorio. Los `keep_*` no son parámetros del `prune`, y
//!    `Repository` no tiene ningún método `forget`: lo que hay es
//!    [`KeepOptions`] —las reglas— y [`ForgetGroups`], que las aplica y dice de cada
//!    instantánea si se conserva y por qué.
//! 2. **Retirar** esas instantáneas, que es una operación aparte:
//!    `Repository::delete_snapshots`.
//! 3. **Purgar** lo que quedó huérfano al retirarlas, calculando el **plan antes** de
//!    ejecutarlo: `prune_plan` y después `prune`. El plan se calcula antes y no se
//!    ejecuta a ciegas porque es la única forma de que quede constancia de lo que se va a
//!    borrar, y porque es la interfaz que da la biblioteca: `PruneOptions::get_plan` está
//!    deprecado y el camino bueno es `PrunePlan::from_prune_options`, que es lo que hace
//!    `prune_plan`.
//!
//! Y la retención va **por grupos**: se agrupan las instantáneas por host, etiqueta y
//! rutas —el criterio por defecto de la biblioteca— antes de aplicar las reglas. No es un
//! adorno: la retención es una propiedad de la **tarea** —el §3 la guarda en
//! `backup_jobs`—, así que dos tareas que escriban en el mismo repositorio tienen cada una
//! su retención. Aplicando las reglas sobre la lista entera, la tarea que copia cada hora
//! decidiría cuáles de las instantáneas de la otra sobran, que es justo lo que nadie ha
//! pedido.
//!
//! De ahí sale una obligación para el motor que conviene tener escrita: los tres campos del
//! grupo tienen que ser **estables** entre copias de la misma tarea. Si la etiqueta, el host
//! o las rutas cambiaran de una copia a la siguiente, cada instantánea caería en su propio
//! grupo y ninguna regla retiraría nada —nunca habría nada que sobrara dentro de un grupo de
//! uno—, y el síntoma sería un repositorio que crece para siempre sin que nada lo diga. El
//! host y la etiqueta los declara el motor, y las rutas salen del target, que no cambia
//! entre copias.
//!
//! # Lo que el §3 no guarda se queda en su valor por defecto, y se dice
//!
//! De las once familias de `keep_*` de la biblioteca, el esquema del §3 solo tiene tres
//! columnas: `keep_daily`, `keep_weekly` y `keep_monthly`. Las otras ocho —`keep_last`,
//! `keep_minutely`, `keep_hourly`, `keep_quarter_yearly`, `keep_half_yearly`, `keep_yearly`
//! y las dos familias de `keep_within`— se quedan **sin informar**, que en la biblioteca
//! significa «no conservar nada por esta regla». Dejarlo implícito sería peor que
//! escribirlo: quien lea este módulo dentro de un año tiene que saber si esas reglas están
//! apagadas por decisión o por olvido, y la prueba
//! `las_reglas_rechazan_un_keep_que_la_biblioteca_no_acepta` lo fija.
//!
//! # Los valores de las columnas
//!
//! - **cero** significa «no conservar ninguna por esta regla», y se respeta: el §3 admite
//!   ese valor y la retención que lo escriba se lleva por delante todas las instantáneas
//!   que no conserve otra regla. Es una instrucción, no un error.
//! - **-1** significa «conservar todas», que es como lo lee la biblioteca.
//! - **cualquier otro negativo** se rechaza con un motivo que nombra la columna, porque la
//!   biblioteca lo leería como «conservar todas» sin decirlo y una columna de retención con
//!   un número imposible dentro es una errata, no una instrucción.
//! - un valor que no quepa en el entero de 32 bits de la biblioteca se rechaza igual, y por
//!   lo mismo: no hay ninguna tarea con una retención de más de dos mil millones de
//!   instantáneas, así que lo que hay es una fila rota.
//!
//! Nada de esto se valida «hacia abajo» —no se corrige un cero ni se sube un negativo—:
//! corregir un valor de retención en silencio es decidir por el operador, y la dirección de
//! la corrección —conservar de más o de menos— no es la misma en los dos casos.
//!
//! # Una retención que falla no invalida la copia
//!
//! La copia ya está hecha y es correcta cuando la retención se aplica. Si falla, lo que hay
//! que reportar es que la copia está y la purga no, así que el motor decide qué hacer con
//! este error y **no** lo confunde con el resultado de la copia.

use rustic_core::jiff::Zoned;
use rustic_core::repofile::SnapshotId;
use rustic_core::{
    ForgetGroups, Grouped, KeepOptions, Open, PruneOptions, Repository, SnapshotGroupCriterion,
};

use super::Error;

/// Los tres parámetros de retención que el esquema guarda.
///
/// Entran como `i64` porque es lo que devuelve la capa de acceso —la columna del §3 es un
/// `INTEGER`— y se convierten a lo que la biblioteca espera en [`reglas`], que
/// es donde se rechaza lo que no significa nada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retencion {
    /// Instantáneas diarias que se conservan, una por día.
    pub keep_daily: i64,
    /// Instantáneas semanales que se conservan, una por semana.
    pub keep_weekly: i64,
    /// Instantáneas mensuales que se conservan, una por mes.
    pub keep_monthly: i64,
}

/// Lo que hizo la retención.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InformeDeRetencion {
    /// Los identificadores de las instantáneas que se retiraron, en hex completo.
    pub retirados: Vec<String>,
    /// Si además se purgó lo que quedó huérfano.
    ///
    /// `true` significa que el plan de purga se calculó y se ejecutó, no que los bytes se
    /// hayan reclamado en este instante: la biblioteca marca los paquetes para borrar y los
    /// conserva un tiempo —su opción `keep_delete`—, así que el espacio se recupera en una
    /// purga posterior. Lo que este campo afirma es que la tercera operación ocurrió.
    pub purgado: bool,
}

/// Aplica la retención sobre un repositorio abierto, en sus tres operaciones.
///
/// Genérica sobre el estado del repositorio y acotada a [`Open`] a propósito: lo que hace
/// falta para las tres operaciones —listar instantáneas, resolverlas, retirarlas y
/// purgar— está en el estado abierto, y no necesita ni el índice en memoria ni el estado
/// indexado. Así, esta función vale lo mismo para el repositorio que abre el motor que para
/// cualquier otro que esté abierto, y no hay que cambiar nada el día que el estado cambie.
///
/// # Errors
///
/// Devuelve [`Error::RetencionFallida`] con el motivo de la operación que falló. El motivo
/// nombra la operación porque las tres se arreglan de distinta manera: no es lo mismo no
/// poder leer las instantáneas que no poder borrar una.
pub(crate) fn aplicar<S: Open>(
    repositorio: &Repository<S>,
    retencion: &Retencion,
) -> Result<InformeDeRetencion, Error> {
    let reglas = reglas(retencion)?;
    let ahora = Zoned::now();

    // 1. Decidir. Las instantáneas se leen **del repositorio** y se agrupan por host,
    //    etiqueta y rutas antes de aplicar las reglas: la retención es de la tarea, y dos
    //    tareas que compartan repositorio no tienen por qué decidir la una sobre las
    //    instantáneas de la otra.
    let instantaneas = repositorio.get_all_snapshots().map_err(|error| {
        fallo(format!(
            "no se han podido leer las instantáneas del repositorio: {}",
            error.display_log()
        ))
    })?;
    let grupos = Grouped::from_items(instantaneas, SnapshotGroupCriterion::default());
    let a_retirar = ForgetGroups::from_grouped_snapshots_with_retention(grupos, &reglas, &ahora)
        .map_err(|error| {
            fallo(format!(
                "no se han podido evaluar las reglas de retención: {}",
                error.display_log()
            ))
        })?
        .into_forget_ids();

    // 2. Retirar.
    if !a_retirar.is_empty() {
        repositorio.delete_snapshots(&a_retirar).map_err(|error| {
            fallo(format!(
                "no se han podido retirar las instantáneas: {}",
                error.display_log()
            ))
        })?;
    }

    // 3. Purgar, con el plan **calculado antes** de ejecutarlo.
    let opciones = PruneOptions::default();
    let plan = repositorio.prune_plan(&opciones).map_err(|error| {
        fallo(format!(
            "no se ha podido calcular el plan de purga: {}",
            error.display_log()
        ))
    })?;
    repositorio
        .prune(&opciones, plan)
        .map_err(|error| fallo(format!("no se ha podido purgar: {}", error.display_log())))?;

    Ok(InformeDeRetencion {
        retirados: identificadores(&a_retirar),
        purgado: true,
    })
}

/// El fallo de la retención, con el motivo de la operación que falló.
fn fallo(motivo: String) -> Error {
    Error::RetencionFallida { motivo }
}

/// Los identificadores en hex **completo**.
///
/// Nunca en la forma corta del `Display` de [`SnapshotId`] —sus ocho primeros caracteres—:
/// es un prefijo ambiguo, y estos identificadores se guardan en `job_executions` y se le
/// pasan al listado.
fn identificadores(ids: &[SnapshotId]) -> Vec<String> {
    ids.iter()
        .map(|id| id.to_hex().as_str().to_owned())
        .collect()
}

/// Traduce los tres parámetros del esquema a las reglas de la biblioteca.
///
/// Las ocho familias de `keep_*` que el §3 no guarda se quedan **sin informar**, y eso es
/// una decisión que el encabezado del módulo explica: aquí no se rellenan con un defecto
/// propio porque un defecto inventado por este módulo sería una segunda política de
/// retención que nadie ha pedido.
///
/// # Errors
///
/// Devuelve [`Error::RetencionFallida`] si alguno de los tres valores no significa nada
/// como regla. Ver [`regla`].
pub(crate) fn reglas(retencion: &Retencion) -> Result<KeepOptions, Error> {
    Ok(KeepOptions::default()
        .keep_daily(regla("keep_daily", retencion.keep_daily)?)
        .keep_weekly(regla("keep_weekly", retencion.keep_weekly)?)
        .keep_monthly(regla("keep_monthly", retencion.keep_monthly)?))
}

/// Convierte una columna de retención del §3 en una regla de la biblioteca.
///
/// Dos comprobaciones y ninguna corrección: un valor imposible se rechaza con un motivo que
/// nombra la columna, porque corregirlo en silencio sería decidir por el operador y la
/// dirección de la corrección no es la misma en los dos casos —subir un cero conserva de
/// más, y bajar un negativo conserva de menos—.
///
/// # Errors
///
/// Devuelve [`Error::RetencionFallida`] si el valor no cabe en el entero de 32 bits que usa
/// la biblioteca o si es un negativo que no sea el `-1` que ella lee como «conservar todo».
fn regla(columna: &str, valor: i64) -> Result<i32, Error> {
    let acotado = i32::try_from(valor).map_err(|_| {
        fallo(format!(
            "`{columna}` vale {valor}, que no cabe en una regla de retención: el §3 guarda un \
             entero de 64 bits y la biblioteca usa uno de 32"
        ))
    })?;

    if acotado < -1 {
        return Err(fallo(format!(
            "`{columna}` vale {valor}: el -1 es la forma que tiene la biblioteca de decir \
             «conservar todas», y cualquier otro negativo lo leería igual sin decirlo, así que \
             una columna con ese número dentro es una errata y no una instrucción"
        )));
    }

    Ok(acotado)
}

#[cfg(test)]
mod tests {
    use rustic_core::repofile::SnapshotFile;

    use crate::test_support::{
        PASSPHRASE_DE_PRUEBA, SEMILLA_CONTENIDO, abrir_local, directorio_de_prueba,
        sembrar_snapshots, volcado_guardado,
    };

    use super::{Error, Retencion, aplicar, reglas};

    /// Tres fechas en **tres días distintos**, que es lo que hace determinista a una regla
    /// diaria: una prueba de retención que dependiera del reloj conservaría dos
    /// instantáneas una vez cada mil corridas, y ese fallo no se parece a su causa.
    const FECHAS: [&str; 3] = [
        "2026-01-01T10:00:00Z",
        "2026-01-02T10:00:00Z",
        "2026-01-03T10:00:00Z",
    ];

    /// Una retención de prueba: la regla que se quiera medir en `daily` y las otras dos
    /// apagadas, que es lo que aísla lo que se está midiendo.
    fn retencion_con_diarias(diarias: i64) -> Retencion {
        Retencion {
            keep_daily: diarias,
            keep_weekly: 0,
            keep_monthly: 0,
        }
    }

    /// El identificador de la única instantánea que queda en el repositorio.
    fn la_que_queda(repositorio: &rustic_core::Repository<rustic_core::OpenStatus>) -> String {
        let instantaneas: Vec<SnapshotFile> = repositorio
            .get_all_snapshots()
            .expect("el repositorio se puede listar");

        assert_eq!(
            instantaneas.len(),
            1,
            "la retención tenía que dejar una sola instantánea y dejó {}",
            instantaneas.len()
        );

        instantaneas[0].id.to_hex().as_str().to_owned()
    }

    /// **1.** La retención **retira** lo que sobra y **purga** lo que queda huérfano.
    ///
    /// Con tres instantáneas en tres días distintos y una regla diaria de una sola, la
    /// respuesta no admite discusión: se conserva la más reciente y se retiran las otras
    /// dos. Y se comprueba además que la que se conserva sigue **intacta** —su contenido se
    /// puede restaurar—, que es lo que separa una purga correcta de una purga que se lleva
    /// por delante datos que seguían referenciados.
    #[test]
    fn la_retencion_retira_los_que_sobran_y_purga() {
        let directorio = directorio_de_prueba();
        let ruta = directorio.ruta().join("repositorio");
        let siembra = sembrar_snapshots(&ruta, PASSPHRASE_DE_PRUEBA, &FECHAS);
        let sembrados = &siembra.identificadores;

        let repositorio = abrir_local(&ruta, PASSPHRASE_DE_PRUEBA);
        let informe = aplicar(&repositorio, &retencion_con_diarias(1))
            .expect("aplicar la retención no puede fallar");

        let mut retirados = informe.retirados.clone();
        retirados.sort();
        let mut esperados = vec![sembrados[0].clone(), sembrados[1].clone()];
        esperados.sort();
        assert_eq!(
            retirados, esperados,
            "la retención tenía que retirar las dos instantáneas más antiguas y retiró {retirados:?}"
        );
        assert!(
            informe.purgado,
            "la retención retiró instantáneas y no purgó lo que quedó huérfano: la tercera \
             operación no ocurrió"
        );

        assert_eq!(
            la_que_queda(&repositorio),
            sembrados[2],
            "la instantánea que se conserva no es la más reciente"
        );
        assert_eq!(
            volcado_guardado(&ruta, PASSPHRASE_DE_PRUEBA, &sembrados[2], &siembra.nodo),
            Some(SEMILLA_CONTENIDO.to_vec()),
            "la purga se llevó por delante el contenido de la instantánea que se conserva"
        );
    }

    /// **2.** Los tres `keep_` del esquema **llegan a las reglas**: con uno se conserva una
    /// instantánea y con tres se conservan las tres.
    ///
    /// Es la prueba que distingue una retención aplicada de una retención decorativa. Un
    /// motor que ignorase el valor de la columna —o que lo sustituyera por un defecto
    /// suyo— pasaría la primera mitad de la prueba y fallaría la segunda.
    #[test]
    fn los_tres_keep_llegan_a_las_reglas() {
        let una = directorio_de_prueba();
        let ruta_una = una.ruta().join("repositorio");
        sembrar_snapshots(&ruta_una, PASSPHRASE_DE_PRUEBA, &FECHAS);
        let con_una = abrir_local(&ruta_una, PASSPHRASE_DE_PRUEBA);

        let informe = aplicar(&con_una, &retencion_con_diarias(1))
            .expect("aplicar la retención no puede fallar");

        assert_eq!(
            informe.retirados.len(),
            2,
            "con `keep_daily = 1` sobre tres días distintos tenían que sobrar dos \
             instantáneas, y sobraron {}",
            informe.retirados.len()
        );
        la_que_queda(&con_una);

        let tres = directorio_de_prueba();
        let ruta_tres = tres.ruta().join("repositorio");
        sembrar_snapshots(&ruta_tres, PASSPHRASE_DE_PRUEBA, &FECHAS);
        let con_tres = abrir_local(&ruta_tres, PASSPHRASE_DE_PRUEBA);

        let informe = aplicar(&con_tres, &retencion_con_diarias(3))
            .expect("aplicar la retención no puede fallar");

        assert!(
            informe.retirados.is_empty(),
            "con `keep_daily = 3` sobre tres días distintos no sobra ninguna instantánea, y se \
             retiraron {:?}: el valor de la columna no está llegando a las reglas",
            informe.retirados
        );
        assert_eq!(
            con_tres
                .get_all_snapshots()
                .expect("el repositorio se puede listar")
                .len(),
            3,
            "con `keep_daily = 3` tienen que quedar las tres instantáneas"
        );
        assert!(
            informe.purgado,
            "la purga se aplica aunque no se retire nada: es la operación que reclama lo que \
             quedó huérfano de retenciones anteriores"
        );
    }

    /// **3.** Las columnas del §3 se traducen a reglas, se rechaza lo que la biblioteca no
    /// acepta, y **lo que el §3 no guarda se queda sin informar** en vez de quedar
    /// implícito.
    ///
    /// Las tres mitades importan por separado: la traducción es la que hace que los tres
    /// valores lleguen; el rechazo es lo que convierte una fila rota en un error con motivo
    /// en vez de en una retención que conserva todo para siempre sin que nadie se entere; y
    /// la lista de las ocho familias que **no** se informan es la que documenta —y fija—
    /// que su apagado es una decisión y no un olvido.
    #[test]
    fn las_reglas_rechazan_un_keep_que_la_biblioteca_no_acepta() {
        let traducidas = reglas(&Retencion {
            keep_daily: 7,
            keep_weekly: 4,
            keep_monthly: 12,
        })
        .expect("los valores del esquema son reglas válidas");

        assert_eq!(traducidas.keep_daily, Some(7), "`keep_daily` no llegó");
        assert_eq!(traducidas.keep_weekly, Some(4), "`keep_weekly` no llegó");
        assert_eq!(traducidas.keep_monthly, Some(12), "`keep_monthly` no llegó");

        // Las ocho familias que el §3 no guarda, sin informar. Se comprueban una a una y
        // no con un `matches!` que las resumiera: lo que se afirma es que **ninguna** está
        // informada, y una lista incompleta pasaría por alto la que se colara.
        for (familia, informada) in [
            ("keep_last", traducidas.keep_last.is_some()),
            ("keep_minutely", traducidas.keep_minutely.is_some()),
            ("keep_hourly", traducidas.keep_hourly.is_some()),
            (
                "keep_quarter_yearly",
                traducidas.keep_quarter_yearly.is_some(),
            ),
            ("keep_half_yearly", traducidas.keep_half_yearly.is_some()),
            ("keep_yearly", traducidas.keep_yearly.is_some()),
            ("keep_within", traducidas.keep_within.is_some()),
            ("keep_within_daily", traducidas.keep_within_daily.is_some()),
        ] {
            assert!(
                !informada,
                "`{familia}` viaja informada y el §3 no le da ninguna columna: su apagado \
                 tiene que ser explícito, y esto lo deja de ser"
            );
        }

        // El `-1` de la biblioteca —«conservar todas»— pasa; cualquier otro negativo no.
        let todas = reglas(&retencion_con_diarias(-1)).expect("el -1 de la biblioteca se acepta");
        assert_eq!(
            todas.keep_daily,
            Some(-1),
            "el -1 tiene que llegar tal cual: es la forma de la biblioteca de decir \
             «conservar todas»"
        );

        let error = reglas(&retencion_con_diarias(-2)).expect_err(
            "-2 no es una regla de retención: la biblioteca lo leería como «conservar todo» sin \
             decirlo, y una columna con un número imposible es una errata",
        );

        let Error::RetencionFallida { motivo } = &error else {
            panic!("un valor de retención imposible tiene que ser un fallo de retención: {error}");
        };
        assert!(
            motivo.contains("keep_daily"),
            "el motivo del rechazo no nombra la columna que lo provocó: {motivo}"
        );
        assert!(
            motivo.contains("-2"),
            "el motivo del rechazo no dice el valor que lo provocó: {motivo}"
        );
    }
}
