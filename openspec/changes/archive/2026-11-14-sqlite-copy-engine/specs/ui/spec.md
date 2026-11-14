## ADDED Requirements

### Requirement: El formulario de fuente ofrece SQLite y sus campos propios

El selector de tipo de fuente de la pantalla de Ajustes SHALL ofrecer los cuatro valores `directory`, `mariadb`, `postgresql` y `sqlite`. Al elegir `sqlite` SHALL mostrarse `container_name` y `db_path`, y SHALL ocultarse `host_path`, `db_user`, `db_password` y `db_name`. Los tipos de la API en TypeScript SHALL incluir `'sqlite'` en la unión del tipo de target y `db_path` en las interfaces de lectura y de alta. El campo `db_path` SHALL ser un texto y NO SHALL tratarse como secreto.

El porqué: el formulario es el espejo del contrato de la API y del `CHECK` de la base. Si el selector no ofrece `sqlite`, nadie puede dar de alta una fuente SQLite desde el panel, y la feature queda a medias por la puerta de delante.

#### Scenario: El selector ofrece SQLite
- **GIVEN** un panel conectado y el formulario de fuente abierto
- **WHEN** se despliega el selector de tipo
- **THEN** SHALL ofrecerse `directory`, `mariadb`, `postgresql` y `sqlite`

#### Scenario: Elegir sqlite muestra solo sus campos
- **GIVEN** el formulario de fuente con el tipo `sqlite` elegido
- **WHEN** se pintan los campos
- **THEN** SHALL verse `container_name` y `db_path`
- **AND** NO SHALL verse `host_path`, `db_user`, `db_password` ni `db_name`

#### Scenario: Una fuente sqlite se da de alta desde el panel
- **GIVEN** el formulario relleno con un `container_name` y un `db_path` válidos
- **WHEN** se envía el alta
- **THEN** SHALL llamarse a la API con `target_type = 'sqlite'` y el `db_path`
- **AND** la fuente SHALL aparecer en el listado