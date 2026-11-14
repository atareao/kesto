import { useState } from 'react'
import {
  Alert,
  Button,
  Descriptions,
  Empty,
  Popconfirm,
  Space,
  Table,
  Typography,
} from 'antd'
import type { TableProps } from 'antd'
import { Cargando } from '../components/Cargando/Cargando'
import { EstadoDeError } from '../components/EstadoDeError/EstadoDeError'
import { ModalDeRepositorio } from '../components/ModalDeRepositorio/ModalDeRepositorio'
import { ModalDeTarget } from '../components/ModalDeTarget/ModalDeTarget'
import { useRepositorios, useBorrarRepositorio } from '../hooks/useRepositorios'
import { useBorrarTarget, useTargets } from '../hooks/useTargets'
import type { Repository, Target } from '../services/api/types'
import { mensajeDeError } from '../services/errores'
import { formatearFecha } from '../services/format'
import { useSalud } from '../services/session'

interface Aviso {
  tipo: 'success' | 'info' | 'error'
  texto: string
}

const NOMBRE_DEL_TIPO: Record<Target['target_type'], string> = {
  directory: 'Directorio',
  mariadb: 'MariaDB',
  postgresql: 'PostgreSQL',
  sqlite: 'SQLite',
}

/** El destino de una copia, según su tipo. */
function destinoDe(target: Target): string {
  if (target.target_type === 'directory') return target.host_path ?? '—'
  const contenedor = target.container_name ?? '—'
  // Un sqlite no tiene nombre de base: lo que lo sitúa es la ruta del `.db`.
  if (target.target_type === 'sqlite') return `${contenedor} · ${target.db_path ?? '—'}`
  const base = target.db_name ?? '—'
  return `${contenedor} · ${base}`
}

/** Una lectura nunca trae el secreto: solo se puede decir si existe. */
function credencialesDe(target: Target): string {
  return target.has_db_password ? 'Contraseña guardada' : 'Sin contraseña'
}

function credencialesDelRepositorio(repositorio: Repository): string {
  const partes: string[] = []
  if (repositorio.has_secret_access_key) partes.push('clave secreta')
  if (repositorio.has_passphrase) partes.push('passphrase')
  return partes.length === 0 ? 'Sin secretos' : `Guardados: ${partes.join(' y ')}`
}

/**
 * Ajustes (§6): salud del despliegue y gestión de targets y repositorios.
 *
 * La salud enseña la versión y el modo de autenticación tal como los publica
 * `/health`. El CRUD de targets y repositorios va en modales; los secretos se
 * escriben con `Input.Password` y **nunca** se pintan al leer: lo único que una
 * lectura dice es si existen.
 */
export default function Ajustes() {
  const salud = useSalud()
  const targets = useTargets()
  const repositorios = useRepositorios()
  const borrarTarget = useBorrarTarget()
  const borrarRepositorio = useBorrarRepositorio()

  const [modalTarget, setModalTarget] = useState(false)
  const [targetEnEdicion, setTargetEnEdicion] = useState<Target | null>(null)
  const [modalRepositorio, setModalRepositorio] = useState(false)
  const [repositorioEnEdicion, setRepositorioEnEdicion] = useState<Repository | null>(null)
  const [aviso, setAviso] = useState<Aviso | null>(null)

  const modo =
    salud.data?.auth_mode === 'oidc' ? 'OIDC' : 'Desactivada (desarrollo)'

  const abrirAltaDeTarget = () => {
    setTargetEnEdicion(null)
    setModalTarget(true)
  }

  const abrirEdicionDeTarget = (target: Target) => {
    setTargetEnEdicion(target)
    setModalTarget(true)
  }

  const alBorrarTarget = async (target: Target) => {
    try {
      await borrarTarget.mutateAsync(target.id)
      setAviso({ tipo: 'success', texto: 'Target borrado.' })
    } catch (error) {
      setAviso({ tipo: 'error', texto: mensajeDeError(error) })
    }
  }

  const abrirAltaDeRepositorio = () => {
    setRepositorioEnEdicion(null)
    setModalRepositorio(true)
  }

  const abrirEdicionDeRepositorio = (repositorio: Repository) => {
    setRepositorioEnEdicion(repositorio)
    setModalRepositorio(true)
  }

  const alBorrarRepositorio = async (repositorio: Repository) => {
    try {
      await borrarRepositorio.mutateAsync(repositorio.id)
      setAviso({ tipo: 'success', texto: 'Repositorio borrado.' })
    } catch (error) {
      setAviso({ tipo: 'error', texto: mensajeDeError(error) })
    }
  }

  const columnasDeTargets: TableProps<Target>['columns'] = [
    { title: 'Nombre', dataIndex: 'name', key: 'nombre' },
    {
      title: 'Tipo',
      key: 'tipo',
      render: (_valor, target) => NOMBRE_DEL_TIPO[target.target_type],
    },
    { title: 'Destino', key: 'destino', render: (_valor, target) => destinoDe(target) },
    {
      title: 'Credenciales',
      key: 'credenciales',
      render: (_valor, target) => credencialesDe(target),
    },
    {
      title: 'Acciones',
      key: 'acciones',
      render: (_valor, target) => (
        <Space size="small" wrap>
          <Button size="small" onClick={() => abrirEdicionDeTarget(target)}>
            Editar
          </Button>
          <Popconfirm
            title="¿Borrar el target?"
            description="Sus tareas asociadas se borran también."
            okText="Borrar"
            cancelText="Cancelar"
            onConfirm={() => void alBorrarTarget(target)}
          >
            <Button size="small" danger>
              Borrar
            </Button>
          </Popconfirm>
        </Space>
      ),
    },
  ]

  const columnasDeRepositorios: TableProps<Repository>['columns'] = [
    { title: 'Nombre', dataIndex: 'name', key: 'nombre' },
    { title: 'Endpoint', dataIndex: 'endpoint', key: 'endpoint' },
    { title: 'Bucket', dataIndex: 'bucket', key: 'bucket' },
    { title: 'Región', dataIndex: 'region', key: 'region' },
    {
      title: 'Secretos',
      key: 'secretos',
      render: (_valor, repositorio) => credencialesDelRepositorio(repositorio),
    },
    {
      title: 'Creado',
      key: 'creado',
      render: (_valor, repositorio) => formatearFecha(repositorio.created_at),
    },
    {
      title: 'Acciones',
      key: 'acciones',
      render: (_valor, repositorio) => (
        <Space size="small" wrap>
          <Button size="small" onClick={() => abrirEdicionDeRepositorio(repositorio)}>
            Editar
          </Button>
          <Popconfirm
            title="¿Borrar el repositorio?"
            description="Sus tareas asociadas se borran también."
            okText="Borrar"
            cancelText="Cancelar"
            onConfirm={() => void alBorrarRepositorio(repositorio)}
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
      <Typography.Title id="titulo-de-la-pantalla" level={1} style={{ marginTop: 0 }}>
        Ajustes
      </Typography.Title>

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

      <Typography.Title level={2} style={{ marginTop: 0 }}>
        Salud
      </Typography.Title>
      {salud.error !== null ? (
        <EstadoDeError
          titulo="No se pudo consultar la salud del despliegue"
          mensaje={mensajeDeError(salud.error)}
          onReintentar={() => void salud.refetch()}
        />
      ) : salud.isPending || salud.data === undefined ? (
        <Cargando />
      ) : (
        <Descriptions bordered size="small" column={2}>
          <Descriptions.Item label="Versión">{salud.data.version}</Descriptions.Item>
          <Descriptions.Item label="Autenticación">{modo}</Descriptions.Item>
        </Descriptions>
      )}

      <Space
        align="center"
        style={{ width: '100%', justifyContent: 'space-between', marginTop: 24, marginBottom: 16 }}
      >
        <Typography.Title level={2} style={{ margin: 0 }}>
          Targets
        </Typography.Title>
        <Button type="primary" onClick={abrirAltaDeTarget}>
          Nuevo target
        </Button>
      </Space>

      {targets.error !== null ? (
        <EstadoDeError
          titulo="No se pudieron cargar los targets"
          mensaje={mensajeDeError(targets.error)}
          onReintentar={() => void targets.refetch()}
        />
      ) : targets.isPending ? (
        <Cargando />
      ) : (targets.data ?? []).length === 0 ? (
        <Empty description="No hay targets configurados" />
      ) : (
        <Table<Target>
          aria-label="Targets"
          rowKey="id"
          columns={columnasDeTargets}
          dataSource={targets.data ?? []}
          pagination={false}
          size="small"
        />
      )}

      <Space
        align="center"
        style={{ width: '100%', justifyContent: 'space-between', marginTop: 24, marginBottom: 16 }}
      >
        <Typography.Title level={2} style={{ margin: 0 }}>
          Repositorios
        </Typography.Title>
        <Button type="primary" onClick={abrirAltaDeRepositorio}>
          Nuevo repositorio
        </Button>
      </Space>

      {repositorios.error !== null ? (
        <EstadoDeError
          titulo="No se pudieron cargar los repositorios"
          mensaje={mensajeDeError(repositorios.error)}
          onReintentar={() => void repositorios.refetch()}
        />
      ) : repositorios.isPending ? (
        <Cargando />
      ) : (repositorios.data ?? []).length === 0 ? (
        <Empty description="No hay repositorios configurados" />
      ) : (
        <Table<Repository>
          aria-label="Repositorios"
          rowKey="id"
          columns={columnasDeRepositorios}
          dataSource={repositorios.data ?? []}
          pagination={false}
          size="small"
        />
      )}

      <ModalDeTarget
        abierto={modalTarget}
        target={targetEnEdicion}
        onCerrar={() => setModalTarget(false)}
        onGuardado={() => setAviso({ tipo: 'success', texto: 'Target guardado.' })}
      />

      <ModalDeRepositorio
        abierto={modalRepositorio}
        repositorio={repositorioEnEdicion}
        onCerrar={() => setModalRepositorio(false)}
        onGuardado={() => setAviso({ tipo: 'success', texto: 'Repositorio guardado.' })}
      />
    </section>
  )
}
