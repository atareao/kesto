import { useImperativeHandle, useRef, useState } from 'react'
import type { Ref } from 'react'
import { Alert, Form, Input, Modal, Space, Typography } from 'antd'
import type { FormInstance } from 'antd'
import { useActualizarRepositorio, useCrearRepositorio } from '../../hooks/useRepositorios'
import type { AltaRepository, Repository } from '../../services/api/types'
import { mensajeDeError } from '../../services/errores'

/** Valores del formulario. Los secretos siempre empiezan vacíos al editar. */
export interface ValoresDeRepositorio {
  name: string
  endpoint: string
  bucket: string
  region: string
  access_key_id: string
  secret_access_key?: string
  passphrase?: string
}

export interface PropsDeModalDeRepositorio {
  abierto: boolean
  /** Repositorio a editar; `null` para un alta. */
  repositorio: Repository | null
  onCerrar: () => void
  onGuardado: (repositorio: Repository) => void
}

const VALORES_POR_DEFECTO: ValoresDeRepositorio = {
  name: '',
  endpoint: '',
  bucket: '',
  region: '',
  access_key_id: '',
  secret_access_key: '',
  passphrase: '',
}

function valoresDe(repositorio: Repository): ValoresDeRepositorio {
  return {
    name: repositorio.name,
    endpoint: repositorio.endpoint,
    bucket: repositorio.bucket,
    region: repositorio.region,
    access_key_id: repositorio.access_key_id,
    // Los secretos no se leen: llegan vacíos y vacío significa «conserva».
    secret_access_key: '',
    passphrase: '',
  }
}

interface PropsDelFormularioDeRepositorio {
  repositorio: Repository | null
  formulario: Ref<FormInstance<ValoresDeRepositorio>>
}

/**
 * El formulario, dueño de su propia instancia (`Form.useForm`).
 *
 * Se pinta con `key={repositorio?.id ?? 'nuevo'}`: cada entidad estrena
 * instancia, así que `initialValues` se aplica de verdad y un secreto tecleado
 * para una fila **nunca** viaja a otra.
 */
function FormularioDeRepositorio({
  repositorio,
  formulario,
}: PropsDelFormularioDeRepositorio) {
  const [form] = Form.useForm<ValoresDeRepositorio>()
  useImperativeHandle(formulario, () => form, [form])

  return (
    <Form
      form={form}
      layout="vertical"
      requiredMark
      initialValues={repositorio === null ? VALORES_POR_DEFECTO : valoresDe(repositorio)}
    >
      <Form.Item
        label="Nombre"
        name="name"
        rules={[{ required: true, message: 'Escribe un nombre.' }]}
      >
        <Input placeholder="p. ej. copias-s3" />
      </Form.Item>

      <Form.Item
        label="Endpoint"
        name="endpoint"
        rules={[{ required: true, message: 'Escribe el endpoint.' }]}
      >
        <Input placeholder="https://s3.example.com" />
      </Form.Item>

      <Space size="middle" wrap align="start">
        <Form.Item
          label="Bucket"
          name="bucket"
          rules={[{ required: true, message: 'Escribe el bucket.' }]}
        >
          <Input placeholder="copias" />
        </Form.Item>
        <Form.Item
          label="Región"
          name="region"
          rules={[{ required: true, message: 'Escribe la región.' }]}
        >
          <Input placeholder="eu-west-1" />
        </Form.Item>
      </Space>

      <Form.Item
        label="ID de la clave de acceso"
        name="access_key_id"
        rules={[{ required: true, message: 'Escribe el identificador de la clave.' }]}
      >
        <Input placeholder="AKIA…" />
      </Form.Item>

      <Form.Item
        label="Clave secreta"
        name="secret_access_key"
        extra={
          repositorio?.has_secret_access_key === true
            ? 'Ya hay una clave secreta guardada; déjala vacía para conservarla.'
            : undefined
        }
      >
        <Input.Password autoComplete="new-password" />
      </Form.Item>

      <Form.Item
        label="Passphrase"
        name="passphrase"
        extra={
          repositorio?.has_passphrase === true
            ? 'Ya hay una passphrase guardada; déjala vacía para conservarla.'
            : undefined
        }
      >
        <Input.Password autoComplete="new-password" />
      </Form.Item>

      {repositorio !== null &&
        (repositorio.has_secret_access_key || repositorio.has_passphrase) && (
          <Typography.Text type="secondary">
            Los secretos guardados no se muestran: solo se indica que existen.
          </Typography.Text>
        )}
    </Form>
  )
}

/**
 * Alta y edición de un repositorio.
 *
 * `has_secret_access_key` y `has_passphrase` son lo único que una lectura dice
 * de los secretos: el valor no se pinta nunca. Al editar, un campo vacío se
 * omite del `PUT` para que el backend conserve el guardado.
 *
 * El formulario es un subcomponente con `key` por entidad (ver
 * `FormularioDeRepositorio`): editar una fila no arrastra el estado de la
 * anterior, ni el secreto tecleado para otro recurso.
 */
export function ModalDeRepositorio({
  abierto,
  repositorio,
  onCerrar,
  onGuardado,
}: PropsDeModalDeRepositorio) {
  const formularioRef = useRef<FormInstance<ValoresDeRepositorio>>(null)
  const [errorGeneral, setErrorGeneral] = useState<string | null>(null)
  const crear = useCrearRepositorio()
  const actualizar = useActualizarRepositorio()
  const guardando = crear.isPending || actualizar.isPending

  const cerrar = () => {
    setErrorGeneral(null)
    onCerrar()
  }

  const alGuardar = async () => {
    setErrorGeneral(null)

    const form = formularioRef.current
    if (form === null) return

    let valores: ValoresDeRepositorio
    try {
      valores = await form.validateFields()
    } catch {
      return
    }

    const cuerpo: AltaRepository = {
      name: valores.name,
      endpoint: valores.endpoint,
      bucket: valores.bucket,
      region: valores.region,
      access_key_id: valores.access_key_id,
    }
    if (valores.secret_access_key !== undefined && valores.secret_access_key !== '') {
      cuerpo.secret_access_key = valores.secret_access_key
    }
    if (valores.passphrase !== undefined && valores.passphrase !== '') {
      cuerpo.passphrase = valores.passphrase
    }

    try {
      const guardado =
        repositorio === null
          ? await crear.mutateAsync(cuerpo)
          : await actualizar.mutateAsync({ id: repositorio.id, cuerpo })

      onGuardado(guardado)
      onCerrar()
    } catch (error) {
      setErrorGeneral(mensajeDeError(error))
    }
  }

  return (
    <Modal
      open={abierto}
      title={repositorio === null ? 'Nuevo repositorio' : 'Editar repositorio'}
      okText="Guardar"
      cancelText="Cancelar"
      confirmLoading={guardando}
      onOk={() => void alGuardar()}
      onCancel={cerrar}
      destroyOnHidden
    >
      {errorGeneral !== null && (
        <Alert type="error" showIcon message={errorGeneral} style={{ marginBottom: 16 }} />
      )}

      <FormularioDeRepositorio
        key={repositorio?.id ?? 'nuevo'}
        repositorio={repositorio}
        formulario={formularioRef}
      />
    </Modal>
  )
}
