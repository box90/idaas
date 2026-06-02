import { api } from "./client"
import type { ConnectionSummary } from "./types"

export type OIDCOptions =
  | {
      mode: "discover"
      issuer_url: string
      client_id: string
      client_secret: string
      redirect_uri: string
    }
  | {
      mode: "custom"
      discovery_document: Record<string, string>
      client_id: string
      client_secret: string
      redirect_uri: string
    }

export type ConnectionOptions =
  | Record<string, string>   // database, oauth2, saml
  | OIDCOptions              // oidc

export const fetchConnections = (tenant: string) =>
  api.get<ConnectionSummary[]>(`/mgmt/${tenant}/connections`).then((r) => r.data)

export const createConnection = (
  tenant: string,
  payload: {
    name: string
    strategy: string
    options: ConnectionOptions
    webhook_url?: string
  }
) => api.post<ConnectionSummary>(`/mgmt/${tenant}/connections`, payload).then((r) => r.data)

export const updateConnection = (
  tenant: string,
  connId: string,
  payload: {
    name?: string
    options?: ConnectionOptions
    webhook_url?: string | null
  }
) =>
  api.put<ConnectionSummary>(`/mgmt/${tenant}/connections/${connId}`, payload).then(
    (r) => r.data
  )

export const deleteConnection = (tenant: string, connId: string) =>
  api.delete(`/mgmt/${tenant}/connections/${connId}`)
