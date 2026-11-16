import { useImperativeHandle, useRef, useState } from 'react'
import type { Ref } from 'react'
import { Alert, Form, Input, InputNumber, Modal, Select, Space, Switch } from 'antd'
import type { FormInstance } from 'antd'
import { ErrorDeApi } from '../../services/api/envelope'
import type { AltaJob, Job, Repository, Target } from '../../services/api/types'
import { mensajeDeError } from '../../services/errores'
import { CRON_POR_DEFECTO, GeneradorCron } from '../GeneradorCron/GeneradorCron'
import { useActualizarTarea, useCrearTarea } from '../../hooks/useTareas'

/** Valores del formulario, que coinciden con el cuerpo del alta. */
export interface ValoresDeTarea {
  /** El nombre, recortado al enviarlo; **obligatorio**: no vacío tras recortar. */
  name: string
  target_id: string
  repository_id: string
  cron_expression: string
  keep_daily: number
  keep_weekly: number
  keep_monthly: number
  enabled: boolean
}

export interface PropsDeModalDeTarea {
  abierto: boolean
  /** Tarea a editar; `null` para un alta. */
  tarea: Job | null
  targets: readonly Target[]
  repositorios: readonly Repository[]
  onCerrar: () => void
  onGuardado: (tarea: Job) => void
}

/** Valores de un alta: todos los días a las 03:00 y retención 7/4/6. */
const VALORES_POR_DEFECTO: ValoresDeTarea = {
  name: '',
  target_id: '',
  repository_id: '',
  cron_expression: CRON_POR_DEFECTO,
  keep_daily: 7,
  keep_weekly: 4,
  keep_monthly: 6,
  enabled: true,
}

function valoresDe(tarea: Job): ValoresDeTarea {
  return {
    // El contrato ya no admite `null`, pero un panel que hable con un backend
    // anterior todavía puede recibirlo: `Input` no acepta `null` y el campo
    // arranca vacío, que el `required` del formulario no dejará guardar.
    name: tarea.name ?? '',
    target_id: tarea.target_id,
    repository_id: tarea.repository_id,
    cron_expression: tarea.cron_expression,
    keep_daily: tarea.keep_daily,
    keep_weekly: tarea.keep_weekly,
    keep_monthly: tarea.keep_monthly,
    enabled: tarea.enabled,
  }
}

/**
 * Convierte los valores del formulario en el cuerpo del alta y de la edición.
 *
 * El nombre es **obligatorio** y viaja **siempre**, recortado: el formulario ya
 * ha impedido el envío si queda vacío, así que la clave no se omite en ningún
 * caso y el servidor nunca recibe una tarea sin nombre.
 */
function cuerpoDeTarea(valores: ValoresDeTarea): AltaJob {
  return { ...valores, name: valores.name.trim() }
}

/**
 * El campo que nombra un rechazo de la API, o `null` si no cita ninguno.
 *
 * Los mensajes del backend entrecomillan el campo con backticks —«el campo
 * `name` es obligatorio», «el campo `cron_expression` no es una expresión cron
 * válida»—, así que se extrae **lo que va entre backticks** y se compara por el
 * nombre **completo**. Buscar la subcadena desnuda «name» casaría con cualquier
 * mensaje futuro que mencionara la palabra por otro motivo —«el `repository_id`
 * no apunta a ningún repositorio con ese name»— y señalaría el campo equivocado.
 */
function campoCitado(mensaje: string): string | null {
  const cita = /`([^`]+)`/.exec(mensaje)
  return cita?.[1] ?? null
}

interface PropsDelFormularioDeTarea {
  tarea: Job | null
  targets: readonly Target[]
  repositorios: readonly Repository[]
  formulario: Ref<FormInstance<ValoresDeTarea>>
}

/**
 * El formulario, dueño de su propia instancia (`Form.useForm`).
 *
 * Vive en un subcomponente que el padre pinta con `key={tarea?.id ?? 'nueva'}`:
 * cada entidad estrena instancia, así que `initialValues` se aplica de verdad y
 * editar una fila **nunca** arrastra lo que se escribió en otra.
 */
function FormularioDeTarea({
  tarea,
  targets,
  repositorios,
  formulario,
}: PropsDelFormularioDeTarea) {
  const [form] = Form.useForm<ValoresDeTarea>()
  useImperativeHandle(formulario, () => form, [form])

  return (
    <Form
      form={form}
      layout="vertical"
      requiredMark
      initialValues={tarea === null ? VALORES_POR_DEFECTO : valoresDe(tarea)}
    >
      <Form.Item
        label="Nombre"
        name="name"
        // `required` con `whitespace: true`: un `required` a secas deja pasar una
        // cadena de solo espacios, que el servidor rechazaría con un `400`.
        rules={[{ required: true, whitespace: true, message: 'El nombre es obligatorio.' }]}
      >
        {/* El mismo tope que la API: más de 120 caracteres es un `400` que nombra
            `name`, y aquí se evita antes de salir. */}
        <Input allowClear maxLength={120} />
      </Form.Item>

      <Form.Item
        label="Target"
        name="target_id"
        rules={[{ required: true, message: 'Elige un target.' }]}
      >
        <Select
          placeholder="Elige un target"
          options={targets.map((target) => ({ value: target.id, label: target.name }))}
        />
      </Form.Item>

      <Form.Item
        label="Repositorio"
        name="repository_id"
        rules={[{ required: true, message: 'Elige un repositorio.' }]}
      >
        <Select
          placeholder="Elige un repositorio"
          options={repositorios.map((repositorio) => ({
            value: repositorio.id,
            label: repositorio.name,
          }))}
        />
      </Form.Item>

      <Form.Item
        label="Expresión cron"
        name="cron_expression"
        rules={[{ required: true, message: 'Escribe una expresión cron.' }]}
      >
        <GeneradorCron />
      </Form.Item>

      <Space size="middle" wrap>
        <Form.Item label="Diarias" name="keep_daily">
          <InputNumber min={0} precision={0} />
        </Form.Item>
        <Form.Item label="Semanales" name="keep_weekly">
          <InputNumber min={0} precision={0} />
        </Form.Item>
        <Form.Item label="Mensuales" name="keep_monthly">
          <InputNumber min={0} precision={0} />
        </Form.Item>
      </Space>

      <Form.Item label="Activa" name="enabled" valuePropName="checked">
        <Switch />
      </Form.Item>
    </Form>
  )
}

/**
 * Alta y edición de una tarea en un `Modal` con `Form`.
 *
 * - El target y el repositorio se eligen con `Select` sobre las listas que ya
 *   tienen las pantallas; no hay peticiones propias.
 * - La retención son tres `InputNumber` no negativos y **no se envía** una tarea
 *   con las tres a cero: no conservaría nada.
 * - El «Nombre» es obligatorio y viaja siempre recortado. Un `400` que nombre
 *   `name` y el `409` del nombre duplicado se señalan en su campo; un `400` del
 *   cron se pinta junto a `cron_expression`. En todos los casos el modal **no** se
 *   cierra: la validación del cliente es una ayuda, no la última palabra.
 *
 * El formulario es un subcomponente con `key` por entidad (ver
 * `FormularioDeTarea`): la instancia de `rc-field-form` se recrea en cada fila y
 * el `destroyOnHidden` del modal no deja valores de una edición anterior.
 */
export function ModalDeTarea({
  abierto,
  tarea,
  targets,
  repositorios,
  onCerrar,
  onGuardado,
}: PropsDeModalDeTarea) {
  const formularioRef = useRef<FormInstance<ValoresDeTarea>>(null)
  const [errorGeneral, setErrorGeneral] = useState<string | null>(null)
  const crear = useCrearTarea()
  const actualizar = useActualizarTarea()
  const guardando = crear.isPending || actualizar.isPending

  const cerrar = () => {
    setErrorGeneral(null)
    onCerrar()
  }

  const alGuardar = async () => {
    setErrorGeneral(null)

    const form = formularioRef.current
    if (form === null) return

    let valores: ValoresDeTarea
    try {
      valores = await form.validateFields()
    } catch {
      return
    }

    if (valores.keep_daily === 0 && valores.keep_weekly === 0 && valores.keep_monthly === 0) {
      form.setFields([
        {
          name: 'keep_daily',
          errors: ['Al menos una regla de retención tiene que conservar algo.'],
        },
      ])
      return
    }

    const cuerpo: AltaJob = cuerpoDeTarea(valores)

    try {
      const guardada =
        tarea === null
          ? await crear.mutateAsync(cuerpo)
          : await actualizar.mutateAsync({ id: tarea.id, cuerpo })

      onGuardado(guardada)
      onCerrar()
    } catch (error) {
      if (error instanceof ErrorDeApi) {
        // Los rechazos se reparten **por el campo que nombran**: el `409` del
        // nombre duplicado y el `400` que cita `name` son del campo «Nombre»
        // —es su nombre el que hay que corregir—, y el `400` del cron sigue
        // señalándose en la expresión cron. Señalar el nombre sobre el cron
        // manda a corregir el sitio equivocado.
        if (error.status === 409) {
          form.setFields([{ name: 'name', errors: [error.message] }])
          return
        }

        if (error.status === 400) {
          // El campo se lee **entre backticks** y se compara por el nombre
          // completo, no por una subcadena suelta que casaría con cualquier
          // mensaje que mencionara la palabra de pasada.
          const campo = campoCitado(error.message)

          if (campo === 'cron_expression') {
            form.setFields([{ name: 'cron_expression', errors: [error.message] }])
            return
          }

          if (campo === 'name') {
            form.setFields([{ name: 'name', errors: [error.message] }])
            return
          }
        }
      }

      setErrorGeneral(mensajeDeError(error))
    }
  }

  return (
    <Modal
      open={abierto}
      title={tarea === null ? 'Nueva tarea' : 'Editar tarea'}
      okText="Guardar"
      cancelText="Cancelar"
      confirmLoading={guardando}
      onOk={() => void alGuardar()}
      onCancel={cerrar}
      destroyOnHidden
    >
      {errorGeneral !== null && (
        <Alert
          type="error"
          showIcon
          message={errorGeneral}
          style={{ marginBottom: 16 }}
        />
      )}

      <FormularioDeTarea
        key={tarea?.id ?? 'nueva'}
        tarea={tarea}
        targets={targets}
        repositorios={repositorios}
        formulario={formularioRef}
      />
    </Modal>
  )
}
