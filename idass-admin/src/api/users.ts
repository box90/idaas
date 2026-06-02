import { api } from "./client"
import type { User } from "./types"

export const fetchUsers = (tenant: string) =>
  api.get<User[]>(`/mgmt/${tenant}/users`).then((r) => r.data)

export const createUser = (
  tenant: string,
  payload: { connection_id: string; email: string; password?: string; organization_id?: string }
) => api.post<User>(`/mgmt/${tenant}/users`, payload).then((r) => r.data)

export const deleteUser = (tenant: string, userId: string) =>
  api.delete(`/mgmt/${tenant}/users/${userId}`)
