import { api } from "./client"
import type { Organization } from "./types"

export const fetchOrganizations = (tenant: string) =>
  api.get<Organization[]>(`/mgmt/${tenant}/organizations`).then((r) => r.data)

export const createOrganization = (
  tenant: string,
  payload: { name: string; display_name?: string }
) => api.post<Organization>(`/mgmt/${tenant}/organizations`, payload).then((r) => r.data)

export const deleteOrganization = (tenant: string, orgId: string) =>
  api.delete(`/mgmt/${tenant}/organizations/${orgId}`)
