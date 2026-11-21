//! El troceador del volcado de PostgreSQL: dónde acaba una base y empieza la siguiente.
//!
//! Un volcado de `pg_dumpall` separa las bases con metacomandos `\connect <base>`, y la
//! restauración necesita entregar **cada base a su propio `psql`**. Este módulo es la pieza que
//! decide, línea a línea, cuál de las tres cosas es cada una: el **contenido** de la sección en
//! curso, la **frontera** que abre la siguiente, o un **metacomando** de primer nivel que se
//! descarta.
//!
//! Es **puro**: no habla con contenedores, ni con la red, ni con la base, ni toca el sistema de
//! ficheros. Entra una línea —`&[u8]`, con su terminador— y sale una [`Instruccion`], y por eso se
//! prueba sin motor de contenedores. El contenido se devuelve **prestado** de la línea de entrada:
//! el troceador no copia el volcado, que es lo que permite entregarlo según llega y sin
//! materializarlo.
//!
//! # El contenido es **no confiable**, y de ahí las dos guardas
//!
//! El volcado sale de un repositorio que configura el operador, así que lo que traiga dentro no se
//! da por bueno. Las dos reglas que lo tratan como tal:
//!
//! - **La frontera se reconoce con el estado léxico delante.** Un `\connect` dentro de un literal,
//!   de un cuerpo de función o de un comentario **no** es una frontera: un troceador que partiera
//!   por `^\connect` rompería un volcado legítimo. Por eso el troceador rastrea comillas simples
//!   (`''`), comillas dobles (`""`), dólares (`$etiqueta$…$etiqueta$`), comentarios de línea
//!   (`--`) y de bloque (`/* */`, **anidable**), y el bloque `COPY … FROM stdin` —cuyos datos
//!   pueden empezar por `\`—. El estado **atraviesa líneas**: una comilla, un dólar o un
//!   comentario abiertos siguen abiertos en la siguiente.
//! - **El metacomando se reconoce por el primer byte no blanco, no por la columna cero.**
//!   Verificado con `psql 18.6`: el cliente ejecuta el metacomando precedido de **cualquier**
//!   blanco, y su juego de blancos es el de `psqlscan.l` —`[ \t\n\r\f\v]`, es decir espacio,
//!   tabulador, salto de línea, retorno de carro, avance de página (`\f`) y tabulador vertical
//!   (`\v`)—: `   \! echo …`, `\t\! echo …` y `\f\connect zz` ejecutan y conectan de verdad, así
//!   que un descarte que exigiera la barra en la columna cero sería **evadible con un espacio o con
//!   un `\f`**. Fuera de un literal, de un comentario o de un bloque `COPY`, `\` no es un comienzo
//!   de token SQL válido, de modo que descartar la línea no pierde SQL.
//! - **El bloque `COPY … FROM stdin` se reconoce como lo reconoce `psql`.** No basta con buscar la
//!   subcadena: el cliente registra la **letra inicial de los primeros identificadores de primer
//!   nivel** de la sentencia —con un tope de ocho, y **sin** registrar los de dentro de
//!   paréntesis— y decide si entra en modo `COPY` cuando la sentencia **termina**, al ver el `;` a
//!   profundidad de paréntesis cero. Un `;` dentro de paréntesis no termina nada, y sin `;` no hay
//!   modo `COPY`. Un troceador que buscara la subcadena daría **falsos positivos** —`COPY nope TO
//!   STDOUT; -- FROM stdin` no entra en modo `COPY`, pero la subcadena sí— y se quedaría en él
//!   hasta un `\.` que puede no llegar, tragándose las fronteras siguientes y desviando los datos
//!   a la base equivocada; y **falsos negativos** con una cabecera partida en dos líneas, cuyas
//!   líneas de datos que empiezan por `\` se descartarían o se tomarían por frontera.
//!
//! # El literal `E'…'` escapa con barra
//!
//! Un literal precedido de `E` o `e` —`E'…'`— se rige por `standard_conforming_strings=on`, que es
//! el defecto y lo que emite `pg_dump`: dentro, la barra invertida escapa el carácter siguiente y
//! `\'` **no** cierra el literal. Sin reconocerlo, una comilla escapada cerraría el literal antes
//! de tiempo y el `\connect` de una línea siguiente se tomaría por una frontera.
//!
//! # La divergencia conocida de `E'…'`, y por qué no es fail-open
//!
//! El reconocimiento del prefijo `E` **no** es la tokenización de `flex` del cliente, y diverge en
//! las dos direcciones: `es_caracter_de_palabra` excluye los bytes ≥ 0x80 —que el
//! `ident_start`/`ident_cont` de `psql` **sí** incluye— y la heurística «el byte anterior es de
//! palabra» no es la regla del lexer, que decide por el **token** anterior —para `flex`, `1E'…'` es
//! un solo token y aquí se lee como prefijo—. No se cambia el comportamiento: la divergencia no es
//! **fail-open**, porque la capa 2 la cubre —cada sección va precedida de `\restrict` y el cliente
//! **rechaza** el metacomando que se le colara (verificado)—, y **no** es alcanzable en un
//! `pg_dumpall` real: las comillas de apertura que emite van precedidas de `(`, `,`, espacio o `=`,
//! nunca de un carácter de palabra ni de un byte ≥ 0x80. Queda dicho aquí como divergencia conocida.
//!
//! # El residuo aceptado: la memoria está acotada a la línea en curso
//!
//! El troceador procesa **una línea** cada vez y no materializa el volcado —el contenido se
//! devuelve prestado—, así que la memoria que retiene no crece con el tamaño total. Pero el búfer
//! que acumula hasta el `\n` **sí** es de la línea en curso: un volcado que trajera una línea
//! gigante sin terminador la acumularía entera. Es un residuo **aceptado** —su sitio es un tope por
//! línea, no este módulo— y aquí solo se deja dicho.

/// Lo que sale de procesar una línea del volcado.
///
/// El [`Contenido`](Instruccion::Contenido) es un **préstamo** de la línea de entrada y no una
/// copia: quien lo recibe lo entrega al `stdin` del `psql` de su sección tal cual, byte a byte, y
/// la memoria queda acotada a la línea en curso.
#[derive(Debug)]
pub enum Instruccion<'a> {
    /// Contenido de la sección en curso: hay que entregarlo al `psql` tal cual, byte a byte.
    Contenido(&'a [u8]),
    /// La línea abre una sección nueva, para esta base.
    Frontera(String),
    /// Metacomando de primer nivel: se descarta.
    Descartada,
}

/// El estado léxico del volcado, que **atraviesa las líneas**.
///
/// Es el subconjunto de `psqlscan` que decide dónde empieza una línea de metacomando: mientras el
/// estado no sea [`Normal`](Estado::Normal), una barra invertida como primer byte no blanco no es
/// un metacomando, sino contenido de un literal, de un comentario o de los datos de un `COPY`.
#[derive(Debug)]
enum Estado {
    /// Ni dentro de un literal, ni de un comentario, ni de un bloque `COPY`.
    Normal,
    /// Dentro de una comilla simple. `escapa` distingue el literal `E'…'`, donde la barra
    /// invertida escapa el carácter siguiente.
    ComillaSimple { escapa: bool },
    /// Dentro de una comilla doble (un identificador entrecomillado).
    ComillaDoble,
    /// Dentro de un dólar `$etiqueta$…$etiqueta$`, con la etiqueta que lo abrió —vacía para `$$`—.
    Dolar(Vec<u8>),
    /// Dentro de un comentario de bloque `/* … */`, con el número de niveles abiertos: es
    /// **anidable**, como en PostgreSQL.
    ComentarioDeBloque(usize),
    /// Dentro de los datos de un `COPY … FROM stdin`: todo es contenido hasta el `\.`.
    ///
    /// Se entra aquí cuando la sentencia `COPY … FROM stdin` **termina** —al ver su `;` a
    /// profundidad de paréntesis cero—, y el estado **atraviesa las líneas** hasta el `\.`. Los
    /// datos de un `COPY` pueden empezar por `\`, así que dentro no se reconoce ni frontera ni
    /// metacomando.
    Copy,
}

/// La máquina de estados que trocea el volcado, línea a línea.
///
/// Se construye con la base de la **primera** sección —el `db_name` del target o, si es nulo,
/// `postgres`— y a partir de ahí cada [`Frontera`](Instruccion::Frontera) mueve
/// [`base_en_curso`](Troceador::base_en_curso). El estado léxico vive dentro y **atraviesa las
/// líneas**, así que el orden de las llamadas a [`linea`](Troceador::linea) es el orden del
/// volcado.
#[derive(Debug)]
pub struct Troceador {
    /// La base de la sección en curso.
    base_en_curso: String,
    /// El estado léxico que viene de las líneas anteriores.
    estado: Estado,
    /// La profundidad de paréntesis de la sentencia en curso, que **atraviesa las líneas**.
    ///
    /// Es el `paren_depth` de `psql`: dentro de paréntesis no se registran identificadores
    /// iniciales, y un `;` a profundidad mayor que cero **no** termina la sentencia, así que no
    /// decide nada.
    parentesis: usize,
    /// Las letras iniciales de los primeros identificadores de **primer nivel** de la sentencia en
    /// curso, con el tope de ocho de `psql` —su `init_idents[8]`—.
    ///
    /// Cada identificador aporta **una letra**, la que registra `psql`: `b'C'` para `copy`, `b'F'`
    /// para `from`, `b'S'` para `stdin`, `b'O'` para `stdout` y `0` para cualquier otro. Es lo que
    /// decide si la sentencia abre un bloque `COPY … FROM stdin`, con la misma regla que el cliente
    /// —ver [`es_un_copy_from_stdin`]—. Se vacía al terminar cada sentencia y al entrar en
    /// [`Estado::Copy`].
    iniciales: Vec<u8>,
}

impl Troceador {
    /// Prepara un troceador con la base de la primera sección.
    ///
    /// `base_inicial` es el `db_name` del target o, si es nulo, `postgres`: la primera sección del
    /// volcado no viene precedida de ningún `\connect`, y hay que saber a qué base entregarla.
    pub fn nuevo(base_inicial: &str) -> Self {
        Self {
            base_en_curso: base_inicial.to_owned(),
            estado: Estado::Normal,
            parentesis: 0,
            iniciales: Vec::new(),
        }
    }

    /// Procesa **una línea**, con su terminador incluido (`\n`, o `\r\n` si lo trae).
    ///
    /// Devuelve qué hay que hacer con ella. El préstamo del [`Contenido`](Instruccion::Contenido)
    /// sale de la propia `linea`, no del troceador, así que se puede llamar en bucle sin copiar el
    /// volcado.
    pub fn linea<'a>(&mut self, linea: &'a [u8]) -> Instruccion<'a> {
        // Dentro de un `COPY`, todo es contenido —incluidas las líneas que parezcan un `\connect`
        // o un `\!`— hasta el `\.`, que también se entrega y cierra el bloque.
        if matches!(self.estado, Estado::Copy) {
            if cierra_el_copy(linea) {
                self.estado = Estado::Normal;
            }
            return Instruccion::Contenido(linea);
        }

        // En estado normal, una barra invertida como primer byte no blanco es un metacomando: una
        // frontera si es `\connect`, un descarte en cualquier otro caso.
        if matches!(self.estado, Estado::Normal)
            && let Some(posicion) = primer_no_blanco(linea)
            && linea[posicion] == b'\\'
        {
            let resto = &linea[posicion + 1..];

            if let Some(base) = nombre_si_es_connect(resto) {
                self.base_en_curso.clone_from(&base);
                return Instruccion::Frontera(base);
            }

            return Instruccion::Descartada;
        }

        // Fuera de un `COPY` y sin ser metacomando: se actualiza el estado léxico —que puede quedar
        // abierto para la línea siguiente— y la línea es contenido. `recorrer` es quien entra en
        // [`Estado::Copy`] cuando la sentencia termina en `;` y resulta ser un `COPY … FROM stdin`.
        self.recorrer(linea);

        Instruccion::Contenido(linea)
    }

    /// La base de la sección en curso.
    pub fn base_en_curso(&self) -> &str {
        &self.base_en_curso
    }

    /// Recorre la línea actualizando el estado léxico, que puede quedar abierto para la siguiente.
    ///
    /// El estado se saca a una variable local y se devuelve al final: así el bucle lo puede mover
    /// —la etiqueta de un dólar es un `Vec`— sin clonarlo en cada byte, y sin que el préstamo del
    /// `match` choque con la escritura.
    fn recorrer(&mut self, linea: &[u8]) {
        let mut estado = std::mem::replace(&mut self.estado, Estado::Normal);
        let mut i = 0;

        while i < linea.len() {
            let byte = linea[i];
            let siguiente = linea.get(i + 1).copied();

            match estado {
                Estado::Normal => {
                    if byte == b'\'' {
                        estado = Estado::ComillaSimple {
                            escapa: abre_con_escape_e(linea, i),
                        };
                        i += 1;
                    } else if byte == b'"' {
                        estado = Estado::ComillaDoble;
                        i += 1;
                    } else if byte == b'$' {
                        match etiqueta_de_dolar(linea, i) {
                            Some((etiqueta, despues)) => {
                                estado = Estado::Dolar(etiqueta);
                                i = despues;
                            }
                            None => {
                                estado = Estado::Normal;
                                i += 1;
                            }
                        }
                    } else if byte == b'-' && siguiente == Some(b'-') {
                        // Comentario de línea: se come el resto de la línea y no deja estado.
                        estado = Estado::Normal;
                        break;
                    } else if byte == b'/' && siguiente == Some(b'*') {
                        estado = Estado::ComentarioDeBloque(1);
                        i += 2;
                    } else if es_inicio_de_identificador(byte) {
                        // Un identificador entero. `psql` registra la **letra inicial** del token
                        // —no el token— entre los primeros de la sentencia, y solo **fuera** de
                        // paréntesis; el tope de ocho es suyo. `$` no es inicio de identificador y
                        // lo resuelve `etiqueta_de_dolar` antes, así que el orden no se cruza.
                        let inicio = i;
                        i += 1;
                        while i < linea.len() && es_continuacion_de_identificador(linea[i]) {
                            i += 1;
                        }

                        if self.parentesis == 0 && self.iniciales.len() < TOPE_DE_IDENTIFICADORES {
                            self.iniciales.push(letra_registrada(&linea[inicio..i]));
                        }

                        estado = Estado::Normal;
                    } else {
                        // Ni identificador ni comienzo de literal: paréntesis, fin de sentencia o
                        // cualquier otro signo.
                        match byte {
                            b'(' => self.parentesis += 1,
                            b')' => self.parentesis = self.parentesis.saturating_sub(1),
                            b';' if self.parentesis == 0 => {
                                // La sentencia termina: es **ahora** cuando `psql` decide si entra
                                // en modo `COPY`; un `;` a profundidad de paréntesis mayor que cero
                                // no decide nada. La lista de identificadores se vacía en cualquier
                                // caso, para que la sentencia siguiente se rastree desde cero.
                                let abre_un_copy = es_un_copy_from_stdin(&self.iniciales);
                                self.iniciales.clear();

                                if abre_un_copy {
                                    estado = Estado::Copy;
                                    break;
                                }
                            }
                            _ => {}
                        }

                        estado = Estado::Normal;
                        i += 1;
                    }
                }
                Estado::ComillaSimple { escapa } => {
                    estado = Estado::ComillaSimple { escapa };
                    if escapa && byte == b'\\' {
                        // En `E'…'`, la barra escapa el carácter siguiente: `\'` no cierra.
                        i += 2;
                    } else if byte == b'\'' {
                        if siguiente == Some(b'\'') {
                            // Comilla escapada: no cierra.
                            i += 2;
                        } else {
                            estado = Estado::Normal;
                            i += 1;
                        }
                    } else {
                        i += 1;
                    }
                }
                Estado::ComillaDoble => {
                    if byte == b'"' {
                        if siguiente == Some(b'"') {
                            i += 2;
                        } else {
                            estado = Estado::Normal;
                            i += 1;
                        }
                    } else {
                        estado = Estado::ComillaDoble;
                        i += 1;
                    }
                }
                Estado::Dolar(etiqueta) => {
                    if byte == b'$' && cierra_la_etiqueta(linea, i, &etiqueta) {
                        let largo = etiqueta.len() + 2;
                        estado = Estado::Normal;
                        i += largo;
                    } else {
                        estado = Estado::Dolar(etiqueta);
                        i += 1;
                    }
                }
                Estado::ComentarioDeBloque(nivel) => {
                    if byte == b'/' && siguiente == Some(b'*') {
                        estado = Estado::ComentarioDeBloque(nivel + 1);
                        i += 2;
                    } else if byte == b'*' && siguiente == Some(b'/') {
                        estado = if nivel == 1 {
                            Estado::Normal
                        } else {
                            Estado::ComentarioDeBloque(nivel - 1)
                        };
                        i += 2;
                    } else {
                        estado = Estado::ComentarioDeBloque(nivel);
                        i += 1;
                    }
                }
                // Inalcanzable: [`Troceador::linea`] resuelve el bloque `COPY` —y `recorrer` entra
                // en él— antes de que el bucle vuelva a evaluar este brazo.
                Estado::Copy => break,
            }
        }

        self.estado = estado;

        // Al entrar en `COPY`, la sentencia siguiente —la que viene tras el `\.`— se rastrea desde
        // cero: los identificadores de la cabecera ya cumplieron su función.
        if matches!(self.estado, Estado::Copy) {
            self.iniciales.clear();
        }
    }
}

/// El primer byte no blanco de la línea, si lo hay.
///
/// Blanco es el juego completo de `psqlscan.l` —`[ \t\n\r\f\v]`—, que es lo que `psql` salta delante
/// de un metacomando, así que «primer nivel» es este byte y nunca la columna cero.
fn primer_no_blanco(linea: &[u8]) -> Option<usize> {
    linea.iter().position(|&byte| !es_blanco(byte))
}

/// Si un byte es blanco para `psql`: `[ \t\n\r\f\v]`, el `space` de `psqlscan.l`.
///
/// Es el juego **completo** y no solo espacio y tabulador: el cliente salta los seis antes de un
/// metacomando, así que `\f\!`, `\v\!` y `\r\!` ejecutan (verificado con `psql 18.6`) y un
/// reconocimiento que solo mirara espacio y tabulador los dejaría pasar como contenido.
fn es_blanco(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// El tope de identificadores iniciales que `psql` recuerda por sentencia: su `init_idents[8]`.
const TOPE_DE_IDENTIFICADORES: usize = 8;

/// Si un byte puede **empezar** un identificador: el `ident_start = [A-Za-z\200-\377_]` de `psql`.
fn es_inicio_de_identificador(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

/// Si un byte puede **continuar** un identificador: el `ident_cont = [A-Za-z\200-\377_0-9$]` de
/// `psql`.
fn es_continuacion_de_identificador(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' || byte >= 0x80
}

/// La letra que `psql` registra para un identificador, o `0` si no es ninguna de las que mira.
///
/// Es el `psqlscan_record_initial_keyword` del cliente: `create`, `function`, `procedure`, `or` y
/// `replace` se registran en **minúscula**; `copy`, `from`, `stdin` y `stdout` en **mayúscula**; y
/// cualquier otro identificador deja un `0`. La comparación no distingue mayúsculas.
fn letra_registrada(identificador: &[u8]) -> u8 {
    const EN_MINUSCULA: [&[u8]; 5] = [b"create", b"function", b"procedure", b"or", b"replace"];
    const EN_MAYUSCULA: [&[u8]; 4] = [b"copy", b"from", b"stdin", b"stdout"];

    let Some(&primera) = identificador.first() else {
        return 0;
    };

    if EN_MINUSCULA
        .iter()
        .any(|palabra| identificador.eq_ignore_ascii_case(palabra))
    {
        primera.to_ascii_lowercase()
    } else if EN_MAYUSCULA
        .iter()
        .any(|palabra| identificador.eq_ignore_ascii_case(palabra))
    {
        primera.to_ascii_uppercase()
    } else {
        0
    }
}

/// El nombre de la base si `resto` —lo que sigue a la barra invertida— es un `\connect`.
///
/// Acepta las dos formas del cliente: `connect` y su alias `c` —`\c` es alias real de `\connect`,
/// verificado—. La palabra tiene que **terminar** ahí, así que `\connectx`, `\cd`, `\conninfo`,
/// `\copy` y `\copyright` NO son un `\connect`.
///
/// Devuelve [`None`] si no lo es —para que quien llama lo trate como un metacomando a descartar— y
/// también cuando el nombre queda **vacío**: `\connect` sin nombre reconecta a la **misma** base
/// (verificado con `psql 18.6`: imprime «You are now connected to database "postgres"»), así que
/// **no** es una frontera, y tratarlo como tal abriría una sección hacia la base por defecto de
/// `psql -d ""`, que diverge del cliente.
fn nombre_si_es_connect(resto: &[u8]) -> Option<String> {
    let despues_de_connect = resto
        .strip_prefix(b"connect")
        .or_else(|| resto.strip_prefix(b"c"))?;

    // La palabra tiene que terminar ahí: el byte siguiente es blanco —con el juego completo de
    // `psql`— o fin de línea.
    if let Some(&byte) = despues_de_connect.first()
        && !es_blanco(byte)
    {
        return None;
    }

    let base = nombre_de_la_base(despues_de_connect);

    if base.is_empty() { None } else { Some(base) }
}

/// El nombre de base que sigue a un `\connect`.
///
/// Termina en el primer blanco —con el juego completo de `psql`— o en el fin de línea; si viene
/// entre comillas dobles, se quitan **y** se resuelve la comilla escapada `""` como el cliente:
/// `\connect "a""b"` conecta a la base `a"b`. En el volcado real de `pg_dumpall` no lleva comillas,
/// pero `psql` las admite para una base con espacios.
fn nombre_de_la_base(resto: &[u8]) -> String {
    let inicio = resto
        .iter()
        .position(|&byte| !es_blanco(byte))
        .unwrap_or(resto.len());
    let resto = &resto[inicio..];

    if resto.first() == Some(&b'"') {
        let mut nombre = Vec::new();
        let mut i = 1;

        while let Some(&byte) = resto.get(i) {
            if byte == b'"' {
                if resto.get(i + 1) == Some(&b'"') {
                    // `""` dentro del nombre es una comilla escapada: no cierra.
                    nombre.push(b'"');
                    i += 2;
                } else {
                    break;
                }
            } else {
                nombre.push(byte);
                i += 1;
            }
        }

        return String::from_utf8_lossy(&nombre).into_owned();
    }

    let fin = resto
        .iter()
        .position(|&byte| es_blanco(byte))
        .unwrap_or(resto.len());

    String::from_utf8_lossy(&resto[..fin]).into_owned()
}

/// Si los identificadores iniciales de la sentencia abren un bloque `COPY … FROM stdin`.
///
/// Es el `psqlscan_is_copy_from_stdin` de `psql`, portado tal cual: el **primer** identificador ha
/// de ser `COPY` (`b'C'`) y, buscando el **primer** `FROM` (`b'F'`) entre los que le siguen, el
/// siguiente identificador ha de ser `stdin` (`b'S'`). No es una búsqueda de subcadena: el cliente
/// registra una letra por identificador de primer nivel y decide **al terminar la sentencia**, así
/// que un `FROM stdin` dentro de un comentario o de un paréntesis no cuenta, y una cabecera partida
/// en dos líneas sí.
fn es_un_copy_from_stdin(iniciales: &[u8]) -> bool {
    if iniciales.first() != Some(&b'C') {
        return false;
    }

    for i in 1..TOPE_DE_IDENTIFICADORES - 1 {
        if iniciales.get(i) != Some(&b'F') {
            continue;
        }

        return iniciales.get(i + 1) == Some(&b'S');
    }

    false
}

/// Si la línea es el `\.` que cierra un bloque `COPY`.
///
/// Se quita el terminador y los espacios o tabuladores finales antes de comparar. El recorte **no**
/// viene de que un volcado deje espacios detrás: no los deja, y `psql 18.6` **rechaza** ese marcador
/// —emite `end-of-copy marker is not alone on its line`— (verificado). Es una tolerancia que se
/// conserva para no cambiar dónde acaba el bloque, y no abre ninguna boca: lo único que decide es
/// que las líneas siguientes dejen de ser datos del `COPY`.
fn cierra_el_copy(linea: &[u8]) -> bool {
    let mut fin = linea.len();
    while fin > 0 && matches!(linea[fin - 1], b'\n' | b'\r' | b' ' | b'\t') {
        fin -= 1;
    }

    &linea[..fin] == b"\\."
}

/// Si la comilla simple de `posicion` abre un literal `E'…'`, donde la barra escapa.
///
/// El prefijo `E` o `e` cuenta solo como prefijo de palabra: no puede venir pegado a un
/// identificador —`aE'…'` no es un literal con escapes—.
fn abre_con_escape_e(linea: &[u8], posicion: usize) -> bool {
    if posicion == 0 {
        return false;
    }

    let anterior = linea[posicion - 1];
    if anterior != b'E' && anterior != b'e' {
        return false;
    }

    match posicion.checked_sub(2).and_then(|antes| linea.get(antes)) {
        Some(&byte) => !es_caracter_de_palabra(byte),
        None => true,
    }
}

/// Si un byte puede formar parte de una palabra —un identificador—.
fn es_caracter_de_palabra(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

/// La etiqueta de un dólar en `posicion` —donde hay un `$`— y dónde sigue el texto.
///
/// Devuelve [`None`] si el `$` no abre un dólar —no le sigue una etiqueta válida y otro `$`—, en
/// cuyo caso es un carácter normal. La etiqueta vacía —`$$`— es un caso válido.
///
/// El juego de caracteres de la etiqueta es el de `psql` —`dolq_start = [A-Za-z\200-\377_]` y
/// `dolq_cont = [A-Za-z\200-\377_0-9]`—, **no ASCII**: un `$café$…$café$` válido para el cliente
/// tiene que reconocerse aquí, o el `\connect` que llevara dentro se tomaría por una frontera y se
/// perdería el contenido de la sección.
fn etiqueta_de_dolar(linea: &[u8], posicion: usize) -> Option<(Vec<u8>, usize)> {
    let inicio = posicion + 1;

    if linea.get(inicio) == Some(&b'$') {
        return Some((Vec::new(), inicio + 1));
    }

    let primera = linea.get(inicio).copied()?;
    if !es_inicio_de_identificador(primera) {
        return None;
    }

    let mut fin = inicio + 1;
    while let Some(&byte) = linea.get(fin) {
        // `dolq_cont` no admite `$`, a diferencia de `ident_cont`: por eso no se reutiliza
        // [`es_continuacion_de_identificador`].
        if byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80 {
            fin += 1;
        } else {
            break;
        }
    }

    if linea.get(fin) == Some(&b'$') {
        Some((linea[inicio..fin].to_vec(), fin + 1))
    } else {
        None
    }
}

/// Si en `posicion` —donde hay un `$`— cierra el dólar abierto con `etiqueta`.
fn cierra_la_etiqueta(linea: &[u8], posicion: usize, etiqueta: &[u8]) -> bool {
    let inicio = posicion + 1;
    let fin = inicio + etiqueta.len();

    linea.get(inicio..fin) == Some(etiqueta) && linea.get(fin) == Some(&b'$')
}

#[cfg(test)]
mod tests {
    use super::{Instruccion, Troceador};

    /// Comprueba que la instrucción es contenido y que sus bytes son los esperados.
    fn contenido(instruccion: Instruccion<'_>, esperado: &[u8]) {
        match instruccion {
            Instruccion::Contenido(bytes) => assert_eq!(bytes, esperado, "se esperaba contenido"),
            otro => panic!("se esperaba contenido, llegó {otro:?}"),
        }
    }

    /// Comprueba que la instrucción abre la frontera de la base esperada.
    fn frontera(instruccion: Instruccion<'_>, esperada: &str) {
        match instruccion {
            Instruccion::Frontera(base) => {
                assert_eq!(base, esperada, "la frontera no es la esperada")
            }
            otro => panic!("se esperaba la frontera `{esperada}`, llegó {otro:?}"),
        }
    }

    /// Comprueba que la instrucción es un metacomando descartado.
    fn descartada(instruccion: Instruccion<'_>) {
        match instruccion {
            Instruccion::Descartada => {}
            otro => panic!("se esperaba un descarte, llegó {otro:?}"),
        }
    }

    #[test]
    fn un_connect_de_primer_nivel_abre_una_frontera() {
        let mut troceador = Troceador::nuevo("postgres");

        frontera(troceador.linea(b"\\connect tienda\n"), "tienda");
    }

    #[test]
    fn un_connect_con_espacios_delante_sigue_siendo_frontera() {
        let mut troceador = Troceador::nuevo("postgres");

        frontera(troceador.linea(b"   \\connect tienda\n"), "tienda");
    }

    #[test]
    fn un_connect_con_tabulador_delante_sigue_siendo_frontera() {
        let mut troceador = Troceador::nuevo("postgres");

        frontera(troceador.linea(b"\t\\connect tienda\n"), "tienda");
    }

    #[test]
    fn un_connect_dentro_de_una_comilla_simple_no_es_frontera() {
        let mut troceador = Troceador::nuevo("postgres");

        contenido(
            troceador.linea(b"INSERT INTO t VALUES ('\n"),
            b"INSERT INTO t VALUES ('\n",
        );
        contenido(troceador.linea(b"\\connect otra\n"), b"\\connect otra\n");
        contenido(troceador.linea(b"');\n"), b"');\n");

        assert_eq!(
            troceador.base_en_curso(),
            "postgres",
            "un `\\connect` dentro de un literal no cambia la base"
        );
    }

    #[test]
    fn un_connect_dentro_de_una_comilla_doble_no_es_frontera() {
        let mut troceador = Troceador::nuevo("postgres");

        contenido(troceador.linea(b"SELECT \"col\n"), b"SELECT \"col\n");
        contenido(troceador.linea(b"\\connect otra\n"), b"\\connect otra\n");
        contenido(troceador.linea(b"\" FROM t;\n"), b"\" FROM t;\n");

        assert_eq!(troceador.base_en_curso(), "postgres");
    }

    #[test]
    fn un_connect_dentro_de_un_dolar_no_es_frontera() {
        let mut troceador = Troceador::nuevo("postgres");

        contenido(
            troceador.linea(b"CREATE FUNCTION f() RETURNS void AS $fn$\n"),
            b"CREATE FUNCTION f() RETURNS void AS $fn$\n",
        );
        contenido(troceador.linea(b"\\connect otra\n"), b"\\connect otra\n");
        contenido(
            troceador.linea(b"$fn$ LANGUAGE sql;\n"),
            b"$fn$ LANGUAGE sql;\n",
        );

        assert_eq!(troceador.base_en_curso(), "postgres");
    }

    #[test]
    fn un_connect_dentro_de_un_comentario_de_linea_no_es_frontera() {
        let mut troceador = Troceador::nuevo("postgres");

        contenido(
            troceador.linea(b"-- \\connect otra\n"),
            b"-- \\connect otra\n",
        );
        // El comentario de línea termina con la línea: la siguiente vuelve a ser normal.
        frontera(troceador.linea(b"\\connect real\n"), "real");
    }

    #[test]
    fn un_connect_dentro_de_un_comentario_de_bloque_anidable_no_es_frontera() {
        let mut troceador = Troceador::nuevo("postgres");

        // Dos niveles abiertos: el `*/` de la primera línea solo cierra el de dentro.
        contenido(
            troceador.linea(b"/* /* anidado */\n"),
            b"/* /* anidado */\n",
        );
        contenido(troceador.linea(b"\\connect otra\n"), b"\\connect otra\n");
        contenido(troceador.linea(b"*/ ya fuera\n"), b"*/ ya fuera\n");

        frontera(troceador.linea(b"\\connect real\n"), "real");
    }

    #[test]
    fn un_connect_en_medio_de_un_cuerpo_multilinea_no_es_frontera() {
        let mut troceador = Troceador::nuevo("postgres");

        contenido(
            troceador.linea(b"CREATE FUNCTION g() RETURNS void AS $cuerpo$\n"),
            b"CREATE FUNCTION g() RETURNS void AS $cuerpo$\n",
        );
        contenido(troceador.linea(b"BEGIN\n"), b"BEGIN\n");
        contenido(troceador.linea(b"\\connect otra\n"), b"\\connect otra\n");
        contenido(troceador.linea(b"END\n"), b"END\n");
        contenido(
            troceador.linea(b"$cuerpo$ LANGUAGE plpgsql;\n"),
            b"$cuerpo$ LANGUAGE plpgsql;\n",
        );

        assert_eq!(troceador.base_en_curso(), "postgres");
    }

    #[test]
    fn una_comilla_simple_abierta_atraviesa_lineas() {
        let mut troceador = Troceador::nuevo("postgres");

        contenido(troceador.linea(b"SELECT 'uno\n"), b"SELECT 'uno\n");
        contenido(troceador.linea(b"dos',\n"), b"dos',\n");
        // Ya cerrada, un metacomando vuelve a ser frontera.
        frontera(troceador.linea(b"\\connect otra\n"), "otra");
    }

    #[test]
    fn una_comilla_escapada_no_cierra_el_literal() {
        let mut troceador = Troceador::nuevo("postgres");

        // La `''` es una comilla escapada: el literal no se cierra en ella.
        contenido(
            troceador.linea(b"SELECT 'a''b' AS c;\n"),
            b"SELECT 'a''b' AS c;\n",
        );
        frontera(troceador.linea(b"\\connect otra\n"), "otra");
    }

    #[test]
    fn un_literal_e_con_barra_escapada_no_cierra_antes_de_tiempo() {
        let mut troceador = Troceador::nuevo("postgres");

        // `E'…'` es un literal con escapes de barra invertida: la comilla que sigue a la barra
        // **no** cierra el literal. Sin tratar el prefijo `E`, el estado volvería a «normal» al
        // acabar la línea y el `\connect` de la siguiente se tomaría por una frontera.
        contenido(troceador.linea(b"E'a\\''b'\n"), b"E'a\\''b'\n");
        contenido(troceador.linea(b"\\connect otra\n"), b"\\connect otra\n");

        assert_eq!(
            troceador.base_en_curso(),
            "postgres",
            "la base no cambia si el `\\connect` está dentro de un literal"
        );
    }

    /// El caso hostil **real**, tal como lo verifiqué contra `psql 18.6`: un literal `E'…'` que
    /// escapa una comilla con barra invertida y sigue abierto al acabar la línea. `psql` imprime el
    /// `\connect` como **texto** —no lo ejecuta—, así que un troceador que cerrara el literal en
    /// `\'` vería una frontera falsa y desviaría el resto de la sección a otra base.
    #[test]
    fn un_literal_e_abierto_no_convierte_el_connect_de_la_linea_siguiente_en_frontera() {
        let mut troceador = Troceador::nuevo("a1");

        // Las tres líneas del volcado hostil, tal cual salen de `psql`:
        //   select E'escapada \' aqui
        //   \connect a2
        //   ' as texto;
        contenido(
            troceador.linea(b"select E'escapada \\' aqui\n"),
            b"select E'escapada \\' aqui\n",
        );
        contenido(troceador.linea(b"\\connect a2\n"), b"\\connect a2\n");
        contenido(troceador.linea(b"' as texto;\n"), b"' as texto;\n");

        assert_eq!(
            troceador.base_en_curso(),
            "a1",
            "el `\\connect` va dentro del literal: la base no cambia"
        );
    }

    /// El **control** del anterior, sin el prefijo `E`: ahí la barra invertida **no** escapa, el
    /// literal se cierra antes de tiempo, y el `\connect` de la línea siguiente **sí** es frontera.
    /// Sin este control, el test de arriba pasaría también con un troceador que no distinguiera el
    /// prefijo `E`, que es justo lo que hay que fijar.
    #[test]
    fn sin_el_prefijo_e_la_barra_no_escapa_y_el_connect_si_es_frontera() {
        let mut troceador = Troceador::nuevo("a1");

        contenido(
            troceador.linea(b"select 'escapada \\' aqui\n"),
            b"select 'escapada \\' aqui\n",
        );
        frontera(troceador.linea(b"\\connect a2\n"), "a2");
    }

    #[test]
    fn los_datos_de_un_copy_no_se_interpretan() {
        let mut troceador = Troceador::nuevo("postgres");

        contenido(
            troceador.linea(b"COPY t (a) FROM stdin;\n"),
            b"COPY t (a) FROM stdin;\n",
        );
        // Datos del bloque: ni frontera ni descarte, aunque lo parezcan.
        contenido(troceador.linea(b"\\connect otra\n"), b"\\connect otra\n");
        contenido(troceador.linea(b"\\! echo x\n"), b"\\! echo x\n");
        contenido(troceador.linea(b"\\.\n"), b"\\.\n");

        // Cerrado el bloque, un metacomando vuelve a descartarse.
        descartada(troceador.linea(b"\\! echo y\n"));
        assert_eq!(troceador.base_en_curso(), "postgres");
    }

    #[test]
    fn el_terminador_del_copy_se_conserva() {
        let mut troceador = Troceador::nuevo("postgres");

        contenido(
            troceador.linea(b"COPY t FROM stdin;\n"),
            b"COPY t FROM stdin;\n",
        );
        contenido(troceador.linea(b"1\n"), b"1\n");
        contenido(troceador.linea(b"\\.\n"), b"\\.\n");

        // El `\.` cierra el bloque y, además, se entrega como contenido.
        frontera(troceador.linea(b"\\connect otra\n"), "otra");
    }

    #[test]
    fn un_metacomando_de_primer_nivel_se_descarta() {
        let mut troceador = Troceador::nuevo("postgres");

        descartada(troceador.linea(b"\\! echo hola\n"));
        descartada(troceador.linea(b"\\restrict k\n"));
        descartada(troceador.linea(b"\\unrestrict k\n"));
    }

    #[test]
    fn un_metacomando_con_espacios_o_tabulador_delante_se_descarta() {
        let mut troceador = Troceador::nuevo("postgres");

        // El cliente lo ejecuta igual: la detección es por primer byte no blanco, no por columna.
        descartada(troceador.linea(b"   \\! echo hola\n"));
        descartada(troceador.linea(b"\t\\! echo hola\n"));
    }

    #[test]
    fn una_linea_de_sql_normal_es_contenido() {
        let mut troceador = Troceador::nuevo("postgres");

        contenido(
            troceador.linea(b"CREATE TABLE t (a text);\n"),
            b"CREATE TABLE t (a text);\n",
        );
    }

    #[test]
    fn una_linea_de_copy_que_no_es_de_stdin_no_entra_en_el_bloque() {
        let mut troceador = Troceador::nuevo("postgres");

        contenido(
            troceador.linea(b"COPY t TO STDOUT;\n"),
            b"COPY t TO STDOUT;\n",
        );
        // El estado sigue normal: un `\connect` posterior sí es frontera.
        frontera(troceador.linea(b"\\connect otra\n"), "otra");
    }

    #[test]
    fn el_copy_se_reconoce_con_mayusculas_y_minusculas() {
        let mut troceador = Troceador::nuevo("postgres");

        contenido(
            troceador.linea(b"copy t from stdin;\n"),
            b"copy t from stdin;\n",
        );
        contenido(troceador.linea(b"\\connect otra\n"), b"\\connect otra\n");
        contenido(troceador.linea(b"\\.\n"), b"\\.\n");
    }

    #[test]
    fn la_base_en_curso_empieza_en_la_inicial_y_cambia_con_cada_frontera() {
        let mut troceador = Troceador::nuevo("postgres");

        assert_eq!(troceador.base_en_curso(), "postgres");
        frontera(troceador.linea(b"\\connect uno\n"), "uno");
        assert_eq!(troceador.base_en_curso(), "uno");
        frontera(troceador.linea(b"\\connect dos\n"), "dos");
        assert_eq!(troceador.base_en_curso(), "dos");
    }

    #[test]
    fn un_volcado_real_de_dos_bases_se_trocea_en_tres_secciones() {
        let mut troceador = Troceador::nuevo("postgres");

        let volcado: &[&[u8]] = &[
            b"--\n",
            b"-- volcado de pg_dumpall\n",
            b"\\connect uno\n",
            b"CREATE TABLE uno_t (a text);\n",
            b"COPY uno_t (a) FROM stdin;\n",
            b"\\connect dos\n",
            b"\\.\n",
            b"\\connect dos\n",
            b"CREATE TABLE dos_t (b text);\n",
            b"\\restrict x\n",
        ];

        let mut secciones: Vec<(String, Vec<u8>)> =
            vec![(troceador.base_en_curso().to_owned(), Vec::new())];
        let mut fronteras: Vec<String> = Vec::new();

        for linea in volcado {
            match troceador.linea(linea) {
                Instruccion::Frontera(base) => {
                    fronteras.push(base.clone());
                    secciones.push((base, Vec::new()));
                }
                Instruccion::Contenido(bytes) => {
                    if let Some((_, contenido)) = secciones.last_mut() {
                        contenido.extend_from_slice(bytes);
                    }
                }
                Instruccion::Descartada => {}
            }
        }

        assert_eq!(
            fronteras,
            vec!["uno", "dos"],
            "las fronteras salen en orden"
        );
        assert_eq!(secciones.len(), 3, "la inicial y una por `\\connect`");
        assert_eq!(secciones[0].0, "postgres");
        assert_eq!(secciones[1].0, "uno");
        assert_eq!(secciones[2].0, "dos");

        assert_eq!(secciones[0].1, b"--\n-- volcado de pg_dumpall\n");
        assert_eq!(
            secciones[1].1,
            b"CREATE TABLE uno_t (a text);\nCOPY uno_t (a) FROM stdin;\n\\connect dos\n\\.\n",
            "el `\\connect dos` de dentro del `COPY` es un dato y no se pierde"
        );
        assert_eq!(secciones[2].1, b"CREATE TABLE dos_t (b text);\n");
    }

    // ── El juego de blancos completo de `psql` (`\f`, `\v`, `\r`) ────────────

    /// `psql` salta `[ \t\n\r\f\v]` antes de un metacomando, así que `\f\!`, `\v\!` y `\r\!`
    /// **ejecutan** (verificado con `psql 18.6`). Un reconocimiento que solo mirara espacio y
    /// tabulador los dejaría pasar como contenido y el cliente los ejecutaría.
    #[test]
    fn un_metacomando_con_avance_de_pagina_tabulador_vertical_o_retorno_se_descarta() {
        for linea in [
            b"\x0c\\! echo hola\n".as_slice(),
            b"\x0b\\! echo hola\n".as_slice(),
            b"\r\\! echo hola\n".as_slice(),
        ] {
            let mut troceador = Troceador::nuevo("postgres");
            descartada(troceador.linea(linea));
        }
    }

    /// El mismo juego de blancos delante de un `\connect`: `\f\connect postgres` **conecta** en el
    /// cliente (verificado), así que aquí tiene que ser una frontera y no contenido.
    #[test]
    fn un_connect_con_avance_de_pagina_delante_es_frontera() {
        let mut troceador = Troceador::nuevo("postgres");

        frontera(troceador.linea(b"\x0c\\connect tienda\n"), "tienda");
    }

    // ── `\connect`: palabra, alias y nombre ──────────────────────────────────

    /// `\connectx` es otro metacomando: la palabra tiene que terminar ahí.
    #[test]
    fn un_connect_pegado_a_otra_palabra_no_es_frontera() {
        let mut troceador = Troceador::nuevo("postgres");

        descartada(troceador.linea(b"\\connectx tienda\n"));
    }

    /// `\connect` sin nombre reconecta a la **misma** base (verificado), así que no es una
    /// frontera: tratarlo como tal mandaría la sección a la base por defecto de `psql -d ""`.
    #[test]
    fn un_connect_sin_nombre_no_es_frontera() {
        for linea in [b"\\connect\n".as_slice(), b"\\connect   \n".as_slice()] {
            let mut troceador = Troceador::nuevo("postgres");
            descartada(troceador.linea(linea));
            assert_eq!(
                troceador.base_en_curso(),
                "postgres",
                "un `\\connect` sin nombre no cambia la base"
            );
        }
    }

    /// Un nombre entre comillas dobles admite espacios; se quitan las comillas.
    #[test]
    fn un_connect_con_el_nombre_entrecomillado_con_espacios_es_frontera() {
        let mut troceador = Troceador::nuevo("postgres");

        frontera(
            troceador.linea(b"\\connect \"base con espacios\"\n"),
            "base con espacios",
        );
    }

    /// Dentro del nombre entrecomillado, `""` es una comilla escapada: `\connect "a""b"` conecta a
    /// la base `a"b`.
    #[test]
    fn un_connect_con_comilla_escapada_en_el_nombre_es_frontera() {
        let mut troceador = Troceador::nuevo("postgres");

        frontera(troceador.linea(b"\\connect \"a\"\"b\"\n"), "a\"b");
    }

    /// `\c` es alias real de `\connect` (verificado).
    #[test]
    fn el_alias_c_abre_una_frontera() {
        let mut troceador = Troceador::nuevo("postgres");

        frontera(troceador.linea(b"\\c tienda\n"), "tienda");
    }

    /// `\cd` es otro metacomando —cambia de directorio—: la palabra `c` no termina en blanco.
    #[test]
    fn el_metacomando_cd_no_es_frontera() {
        let mut troceador = Troceador::nuevo("postgres");

        descartada(troceador.linea(b"\\cd /tmp\n"));
    }

    // ── La detección de `COPY … FROM stdin`, como `psql` ─────────────────────

    /// `FROM stdin` dentro de un **comentario** no abre modo `COPY` en `psql` (verificado), aunque
    /// la subcadena esté ahí. Un troceador que buscara la subcadena se quedaría en `COPY` hasta un
    /// `\.` que puede no llegar y se tragaría las fronteras siguientes.
    #[test]
    fn un_from_stdin_en_un_comentario_no_abre_el_copy() {
        let mut troceador = Troceador::nuevo("postgres");

        contenido(
            troceador.linea(b"COPY nope TO STDOUT; -- FROM stdin\n"),
            b"COPY nope TO STDOUT; -- FROM stdin\n",
        );
        // El estado sigue normal: un `\connect` posterior sí es frontera.
        frontera(troceador.linea(b"\\connect otra\n"), "otra");
    }

    /// `FROM stdin` dentro de **paréntesis** no cuenta: `psql` no registra los identificadores de
    /// dentro de paréntesis, así que `COPY (SELECT 1 FROM stdin_x) TO STDOUT;` no abre `COPY`
    /// (verificado).
    #[test]
    fn un_from_stdin_dentro_de_parentesis_no_abre_el_copy() {
        let mut troceador = Troceador::nuevo("postgres");

        contenido(
            troceador.linea(b"COPY (SELECT 1 FROM stdin_x) TO STDOUT;\n"),
            b"COPY (SELECT 1 FROM stdin_x) TO STDOUT;\n",
        );
        frontera(troceador.linea(b"\\connect otra\n"), "otra");
    }

    /// `COPY t (a) FROM stdin;` sí abre el bloque, y los datos se entregan como contenido.
    #[test]
    fn un_copy_de_stdin_de_verdad_abre_el_bloque() {
        let mut troceador = Troceador::nuevo("postgres");

        contenido(
            troceador.linea(b"COPY t (a) FROM stdin;\n"),
            b"COPY t (a) FROM stdin;\n",
        );
        contenido(troceador.linea(b"\\connect otra\n"), b"\\connect otra\n");
        contenido(troceador.linea(b"\\.\n"), b"\\.\n");

        assert_eq!(troceador.base_en_curso(), "postgres");
    }

    /// Una cabecera `COPY` partida en **dos líneas** la reconoce `psql` (verificado): los
    /// identificadores iniciales **atraviesan las líneas** y la decisión es al `;`. Un troceador
    /// que solo mirara una línea perdería el bloque y descartaría sus datos.
    #[test]
    fn una_cabecera_de_copy_partida_en_dos_lineas_abre_el_bloque() {
        let mut troceador = Troceador::nuevo("postgres");

        contenido(troceador.linea(b"COPY t (a)\n"), b"COPY t (a)\n");
        contenido(troceador.linea(b"FROM stdin;\n"), b"FROM stdin;\n");
        // Si el bloque está abierto, esta línea es un **dato**, no una frontera.
        contenido(troceador.linea(b"\\connect otra\n"), b"\\connect otra\n");
        contenido(troceador.linea(b"\\.\n"), b"\\.\n");

        assert_eq!(troceador.base_en_curso(), "postgres");
    }

    /// `COPY t FROM stdin` **sin `;`** no entra en modo `COPY` en `psql` (verificado): la sentencia
    /// no ha terminado, así que el cliente no decide todavía.
    #[test]
    fn un_copy_de_stdin_sin_punto_y_coma_no_abre_el_bloque() {
        let mut troceador = Troceador::nuevo("postgres");

        contenido(
            troceador.linea(b"COPY t FROM stdin\n"),
            b"COPY t FROM stdin\n",
        );
        frontera(troceador.linea(b"\\connect otra\n"), "otra");
    }

    /// Dos sentencias en una línea: la primera —`SELECT`— no abre `COPY`, y la segunda —`COPY t
    /// FROM stdin;`— sí, porque la lista de identificadores se vacía en cada `;`.
    #[test]
    fn dos_sentencias_en_una_linea_deciden_por_separado() {
        let mut troceador = Troceador::nuevo("postgres");

        contenido(
            troceador.linea(b"SELECT 1; COPY t FROM stdin;\n"),
            b"SELECT 1; COPY t FROM stdin;\n",
        );
        // La segunda sentencia abrió el bloque: esto es un dato, no una frontera.
        contenido(troceador.linea(b"\\connect otra\n"), b"\\connect otra\n");
        contenido(troceador.linea(b"\\.\n"), b"\\.\n");

        assert_eq!(troceador.base_en_curso(), "postgres");
    }

    // ── La etiqueta de dólar no ASCII ────────────────────────────────────────

    /// `psql` admite `[A-Za-z\200-\377_]` en la etiqueta de un dólar, así que un `$café$…$café$`
    /// es válido y el `\connect` que llevara dentro **no** es una frontera. Sin el juego no ASCII,
    /// el troceador cerraría el dólar mal y se perdería el contenido de la sección.
    #[test]
    fn un_dolar_con_etiqueta_no_ascii_no_deja_pasar_el_connect() {
        let mut troceador = Troceador::nuevo("postgres");

        contenido(
            troceador.linea("CREATE FUNCTION f() RETURNS void AS $café$\n".as_bytes()),
            "CREATE FUNCTION f() RETURNS void AS $café$\n".as_bytes(),
        );
        contenido(troceador.linea(b"\\connect otra\n"), b"\\connect otra\n");
        contenido(
            troceador.linea("$café$ LANGUAGE sql;\n".as_bytes()),
            "$café$ LANGUAGE sql;\n".as_bytes(),
        );

        assert_eq!(
            troceador.base_en_curso(),
            "postgres",
            "el `\\connect` va dentro del dólar: la base no cambia"
        );
    }
}
