import { api } from "./client"
import type { Tenant } from "./types"

export const fetchTenants = () =>
  api.get<Tenant[]>("/mgmt/tenants").then((r) => r.data)

export const fetchTenant = (name: string) =>
  api.get<Tenant[]>(`/mgmt/tenants`).then((r) =>
    r.data.find((t: Tenant) => t.name === name) ?? Promise.reject(new Error("not found"))
  )

export const createTenant = (name: string, region: string) =>
  api.post<Tenant>("/mgmt/tenants", { name, region }).then((r) => r.data)

export const updateTenant = (id: string, name: string) =>
  api.put<Tenant>(`/mgmt/tenants/${id}`, { name }).then((r) => r.data)

export const migrateTenant = (id: string, target_region: string) =>
  api.post(`/mgmt/tenants/${id}/migrate`, { target_region })
