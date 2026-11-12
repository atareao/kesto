import { ErrorDeApi, ErrorDeProtocolo, esErrorDeSesion, desenvolver } from './envelope'

/** Ejecuta `accion` y devuelve lo que lance; falla si no lanza nada. */
function capturar(accion: () => unknown): unknown {
  try {
    accion()
  } catch (error) {
    return error
  }
  throw new Error('se esperaba que la acción lanzase un error')
}

describe('desenvolver', () => {
  it('devuelve el `data` de una respuesta atendida', () => {
    const data = desenvolver<{ id: string }>({
      http: 200,
      cuerpo: { status: 200, message: 'ok', data: { id: '0199f0' } },
    })

    expect(data).toEqual({ id: '0199f0' })
  })

  it('devuelve una lista cuando el `data` es un array (listados)', () => {
    const data = desenvolver<number[]>({
      http: 200,
      cuerpo: { status: 200, message: 'ok', data: [1, 2, 3] },
    })

    expect(data).toEqual([1, 2, 3])
  })

  it('devuelve `null` cuando la respuesta atendida no trae contenido', () => {
    const data = desenvolver<null>({
      http: 200,
      cuerpo: { status: 200, message: 'target borrado', data: null },
    })

    expect(data).toBeNull()
  })

  it('lanza un error tipado con el `message` del backend si la respuesta es de error', () => {
    const error = capturar(() =>
      desenvolver({
        http: 400,
        cuerpo: {
          status: 400,
          message: 'el campo `limit` tiene que ser un número entre 1 y 200',
        },
      }),
    )

    expect(error).toBeInstanceOf(ErrorDeApi)
    expect((error as ErrorDeApi).status).toBe(400)
    expect((error as ErrorDeApi).message).toBe('el campo `limit` tiene que ser un número entre 1 y 200')
  })

  it('lanza un error de protocolo si el cuerpo no trae la envoltura', () => {
    const error = capturar(() => desenvolver({ http: 200, cuerpo: '<html>proxy</html>' }))

    expect(error).toBeInstanceOf(ErrorDeProtocolo)
    expect((error as ErrorDeProtocolo).message).toContain('envoltura')
  })

  it('lanza un error de protocolo si el cuerpo es un objeto sin envoltura', () => {
    const error = capturar(() => desenvolver({ http: 200, cuerpo: { snapshots: [] } }))

    expect(error).toBeInstanceOf(ErrorDeProtocolo)
  })

  it('lanza un error de protocolo si el `status` del cuerpo no cuadra con el HTTP', () => {
    const error = capturar(() =>
      desenvolver({
        http: 200,
        cuerpo: { status: 500, message: 'algo se rompió', data: null },
      }),
    )

    expect(error).toBeInstanceOf(ErrorDeProtocolo)
    expect((error as ErrorDeProtocolo).status).toBe(500)
    // El mensaje del cuerpo, si lo hay, viaja dentro del error de protocolo: es
    // lo único que explica el desajuste.
    expect((error as ErrorDeProtocolo).message).toContain('algo se rompió')
  })

  it('lanza un error de protocolo si el `status` del cuerpo no es un número', () => {
    const error = capturar(() =>
      desenvolver({ http: 200, cuerpo: { status: 'OK', message: 'ok', data: null } }),
    )

    expect(error).toBeInstanceOf(ErrorDeProtocolo)
  })
})

describe('esErrorDeSesion', () => {
  it('reconoce un 401 propagado como error de API', () => {
    expect(esErrorDeSesion(new ErrorDeApi(401, 'no hay sesión'))).toBe(true)
  })

  it('no confunde otros errores de API con una sesión ausente', () => {
    expect(esErrorDeSesion(new ErrorDeApi(400, 'mal'))).toBe(false)
    expect(esErrorDeSesion(new ErrorDeProtocolo(401, 'sin envoltura'))).toBe(false)
    expect(esErrorDeSesion(new Error('cualquier cosa'))).toBe(false)
  })
})
