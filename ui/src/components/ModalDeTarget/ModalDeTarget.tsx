import { useState } from 'react'
import { Alert, Form, Input, Modal, Select, Space, Typography } from 'antd'
import { useActualizarTarget, useCrearTarget } from '../../hooks/useTargets'
import type { AltaTarget, Target, TipoDeTarget } from '../../services/api/types'
import { mensajeDeError } from '../../services/errores'

/** Valores del formulario. El secreto siempre empieza vacío al editar. */
export interface ValoresDeTarget {
  name: string
  target_type: TipoDeTarget
  container_name?: string
  db_user?: string
  db_password?: string
  db_name?: string
  host_path?: string
}

export interface PropsDeModalDeTarget {
  abierto: boolean
  /** Target a editar; `null` para un alta. */
  target: Target | null
  onCerrar: () => void
  onGuardado: (target: Target) => void
}

const VALORES_POR_DEFECTO: ValoresDeTarget = {
  name: '',
  target_type: 'directory',
  host_path: '',
}

function valoresDe(target: Target): ValoresDeTarget {
  return {
    name: target.name,
    target_type: target.target_type,
    container_name: target.container_name ?? '',
    db_user: target.db_user ?? '',
    db_name: target.db_name ?? '',
    host_path: target.host_path ?? '',
    // El secreto no se lee nunca: el campo llega vacío y vacío significa
    // «conserva el que hay».
    db_password: '',
  }
}

/**
 * Alta y edición de un target.
 *
 * El `target_type` condiciona los campos: un directorio pide `host_path`, y una
 * base de datos pide contenedor, usuario, contraseña y nombre de la base. La
 * contraseña **nunca** se pinta: en una lectura solo se conoce
 * `has_db_password`, y en un `PUT` un campo vacío se omite para que el backend
 * conserve la guardada.
 */
export function ModalDeTarget({
  abierto,
  target,
  onCerrar,
  onGuardado,
}: PropsDeModalDeTarget) {
  const [form] = Form.useForm<ValoresDeTarget>()
  const [errorGeneral, setErrorGeneral] = useState<string | null>(null)
  const crear = useCrearTarget()
  const actualizar = useActualizarTarget()
  const guardando = crear.isPending || actualizar.isPending

  const tipo = Form.useWatch('target_type', form) ?? 'directory'
  const esDirectorio = tipo === 'directory'

  const cerrar = () => {
    setErrorGeneral(null)
    onCerrar()
  }

  const alGuardar = async () => {
    setErrorGeneral(null)

    let valores: ValoresDeTarget
    try {
      valores = await form.validateFields()
    } catch {
      return
    }

    const cuerpo: AltaTarget = { name: valores.name, target_type: valores.target_type }

    if (valores.target_type === 'directory') {
      cuerpo.host_path = valores.host_path ?? ''
    } else {
      cuerpo.container_name = valores.container_name ?? ''
      cuerpo.db_user = valores.db_user ?? ''
      cuerpo.db_name = valores.db_name ?? ''
      // Un secreto vacío no viaja: mandarlo borraría el guardado.
      if (valores.db_password !== undefined && valores.db_password !== '') {
        cuerpo.db_password = valores.db_password
      }
    }

    try {
      const guardado =
        target === null
          ? await crear.mutateAsync(cuerpo)
          : await actualizar.mutateAsync({ id: target.id, cuerpo })

      onGuardado(guardado)
      onCerrar()
    } catch (error) {
      setErrorGeneral(mensajeDeError(error))
    }
  }

  return (
    <Modal
      open={abierto}
      title={target === null ? 'Nuevo target' : 'Editar target'}
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

      <Form
        key={target?.id ?? 'nuevo'}
        form={form}
        layout="vertical"
        requiredMark
        initialValues={target === null ? VALORES_POR_DEFECTO : valoresDe(target)}
      >
        <Form.Item
          label="Nombre"
          name="name"
          rules={[{ required: true, message: 'Escribe un nombre.' }]}
        >
          <Input placeholder="p. ej. datos-web" />
        </Form.Item>

        <Form.Item label="Tipo" name="target_type" rules={[{ required: true }]}>
          <Select
            options={[
              { value: 'directory', label: 'Directorio' },
              { value: 'mariadb', label: 'MariaDB' },
              { value: 'postgresql', label: 'PostgreSQL' },
            ]}
          />
        </Form.Item>

        {esDirectorio ? (
          <Form.Item
            label="Ruta del host"
            name="host_path"
            rules={[{ required: true, message: 'Escribe la ruta del host.' }]}
          >
            <Input placeholder="/srv/datos" />
          </Form.Item>
        ) : (
          <Space size="middle" wrap align="start">
            <Form.Item
              label="Contenedor"
              name="container_name"
              rules={[{ required: true, message: 'Escribe el contenedor.' }]}
            >
              <Input placeholder="nombre-del-contenedor" />
            </Form.Item>
            <Form.Item
              label="Usuario de la base de datos"
              name="db_user"
              rules={[{ required: true, message: 'Escribe el usuario.' }]}
            >
              <Input placeholder="kesto" />
            </Form.Item>
            <Form.Item
              label="Contraseña de la base de datos"
              name="db_password"
              extra={
                target?.has_db_password === true
                  ? 'Ya hay una contraseña guardada; déjala vacía para conservarla.'
                  : undefined
              }
            >
              <Input.Password autoComplete="new-password" />
            </Form.Item>
            <Form.Item
              label="Base de datos"
              name="db_name"
              rules={[{ required: true, message: 'Escribe el nombre de la base.' }]}
            >
              <Input placeholder="kesto" />
            </Form.Item>
          </Space>
        )}

        {target !== null && target.has_db_password && (
          <Typography.Text type="secondary">
            Las credenciales guardadas no se muestran: solo se indica que existen.
          </Typography.Text>
        )}
      </Form>
    </Modal>
  )
}
