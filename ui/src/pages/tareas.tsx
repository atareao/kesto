import { useState } from 'react'
import { Alert, Badge, Button, Empty, Popconfirm, Space, Table, Typography } from 'antd'
import type { TableProps } from 'antd'
import { ModalDeTarea } from '../components/ModalDeTarea/ModalDeTarea'
import { useBorrarTarea, useDispararTarea, useTareas } from '../hooks/useTareas'
import { useRepositorios } from '../hooks/useRepositorios'
import { useTargets } from '../hooks/useTargets'
import { ErrorDeApi } from '../services/api/envelope'
import type { Job } from '../services/api/types'
import { mensajeDeError } from '../services/errores'
import { formatearFecha, formatearUuid } from '../services/format'

interface Aviso {
  tipo: 'success' | 'info' | 'error'
  texto: string
}

/**
 * Tareas (§6): listado, alta y edición en un `Modal`, borrado con confirmación
 * y disparo manual.
 *
 * El disparo distingue los tres desenlaces del contrato: un `202` deja una
 * ejecución `pending`, un `409` dice que ya hay una en curso y un `404` avisa de
 * que la tarea ya no existe (la mutación refresca la lista).
 */
export default function Tareas() {
  const tareas = useTareas()
  const targets = useTargets()
  const repositorios = useRepositorios()
  const borrar = useBorrarTarea()
  const disparar = useDispararTarea()

  const [modalAbierto, setModalAbierto] = useState(false)
  const [enEdicion, setEnEdicion] = useState<Job | null>(null)
  const [aviso, setAviso] = useState<Aviso | null>(null)

  const nombresDeTarget = new Map((targets.data ?? []).map((target) => [target.id, target.name]))
  const nombresDeRepositorio = new Map(
    (repositorios.data ?? []).map((repositorio) => [repositorio.id, repositorio.name]),
  )

  const abrirAlta = () => {
    setEnEdicion(null)
    setModalAbierto(true)
  }

  const abrirEdicion = (tarea: Job) => {
    setEnEdicion(tarea)
    setModalAbierto(true)
  }

  const alBorrar = async (tarea: Job) => {
    try {
      await borrar.mutateAsync(tarea.id)
      setAviso({ tipo: 'success', texto: 'Tarea borrada.' })
    } catch (error) {
      setAviso({ tipo: 'error', texto: mensajeDeError(error) })
    }
  }

  const alDisparar = async (tarea: Job) => {
    try {
      const encolada = await disparar.mutateAsync(tarea.id)
      setAviso({
        tipo: 'success',
        texto: `Ejecución encolada (${encolada.status === 'pending' ? 'pendiente' : encolada.status}).`,
      })
    } catch (error) {
      if (error instanceof ErrorDeApi && error.status === 409) {
        setAviso({
          tipo: 'error',
          texto: 'Ya hay una ejecución en curso para esta tarea.',
        })
        return
      }

      if (error instanceof ErrorDeApi && error.status === 404) {
        setAviso({
          tipo: 'info',
          texto: 'La tarea ya no existe; se ha refrescado la lista.',
        })
        return
      }

      setAviso({ tipo: 'error', texto: mensajeDeError(error) })
    }
  }

  const columnas: TableProps<Job>['columns'] = [
    {
      title: 'Tarea',
      key: 'target',
      render: (_valor, tarea) =>
        nombresDeTarget.get(tarea.target_id) ?? formatearUuid(tarea.target_id),
    },
    {
      title: 'Repositorio',
      key: 'repositorio',
      render: (_valor, tarea) =>
        nombresDeRepositorio.get(tarea.repository_id) ?? formatearUuid(tarea.repository_id),
    },
    {
      title: 'Cron',
      key: 'cron',
      render: (_valor, tarea) => <Typography.Text code>{tarea.cron_expression}</Typography.Text>,
    },
    {
      title: 'Retención',
      key: 'retencion',
      render: (_valor, tarea) =>
        `D ${tarea.keep_daily} · S ${tarea.keep_weekly} · M ${tarea.keep_monthly}`,
    },
    {
      title: 'Estado',
      key: 'estado',
      render: (_valor, tarea) => (
        <Badge
          status={tarea.enabled ? 'success' : 'default'}
          text={tarea.enabled ? 'Activa' : 'Pausada'}
        />
      ),
    },
    {
      title: 'Creada',
      key: 'creada',
      render: (_valor, tarea) => formatearFecha(tarea.created_at),
    },
    {
      title: 'Acciones',
      key: 'acciones',
      render: (_valor, tarea) => (
        <Space size="small" wrap>
          <Button size="small" onClick={() => abrirEdicion(tarea)}>
            Editar
          </Button>
          <Button size="small" onClick={() => void alDisparar(tarea)}>
            Ejecutar
          </Button>
          <Popconfirm
            title="¿Borrar la tarea?"
            description="Esta acción no se puede deshacer."
            okText="Borrar"
            cancelText="Cancelar"
            onConfirm={() => void alBorrar(tarea)}
          >
            <Button size="small" danger>
              Borrar
            </Button>
          </Popconfirm>
        </Space>
      ),
    },
  ]

  return (
    <section aria-labelledby="titulo-de-la-pantalla">
      <Space
        align="center"
        style={{ width: '100%', justifyContent: 'space-between', marginBottom: 16 }}
      >
        <Typography.Title id="titulo-de-la-pantalla" level={1} style={{ margin: 0 }}>
          Tareas
        </Typography.Title>
        <Button type="primary" onClick={abrirAlta}>
          Nueva tarea
        </Button>
      </Space>

      {aviso !== null && (
        <Alert
          type={aviso.tipo}
          showIcon
          message={aviso.texto}
          closable
          onClose={() => setAviso(null)}
          style={{ marginBottom: 16 }}
        />
      )}

      {tareas.error !== null && (
        <Alert
          type="error"
          showIcon
          message="No se pudieron cargar las tareas"
          description={mensajeDeError(tareas.error)}
          action={
            <Button size="small" onClick={() => void tareas.refetch()}>
              Reintentar
            </Button>
          }
          style={{ marginBottom: 16 }}
        />
      )}

      <Table<Job>
        aria-label="Tareas programadas"
        rowKey="id"
        columns={columnas}
        dataSource={tareas.data ?? []}
        loading={tareas.isPending}
        pagination={false}
        locale={{ emptyText: <Empty description="No hay tareas programadas" /> }}
      />

      <ModalDeTarea
        abierto={modalAbierto}
        tarea={enEdicion}
        targets={targets.data ?? []}
        repositorios={repositorios.data ?? []}
        onCerrar={() => setModalAbierto(false)}
        onGuardado={() => setAviso({ tipo: 'success', texto: 'Tarea guardada.' })}
      />
    </section>
  )
}
