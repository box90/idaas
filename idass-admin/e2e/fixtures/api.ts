import type { APIRequestContext } from '@playwright/test'

const BASE = process.env.VITE_API_URL ?? 'http://localhost:8080'
const KEY  = process.env.MANAGEMENT_API_KEY ?? ''

const mgmtHeaders = {
  Authorization: `Bearer ${KEY}`,
  'Content-Type': 'application/json',
}

export async function apiCreateTenant(
  request: APIRequestContext,
  name: string,
  region = 'eu-west-1'
) {
  const res = await request.post(`${BASE}/api/v1/mgmt/tenants`, {
    headers: mgmtHeaders,
    data: { name, region },
  })
  if (!res.ok()) throw new Error(`apiCreateTenant failed: ${await res.text()}`)
  return res.json() as Promise<{ id: string; name: string; current_region: string; status: string }>
}

export async function apiCreateConnection(
  request: APIRequestContext,
  tenantName: string,
  name: string,
  strategy = 'database'
) {
  const res = await request.post(`${BASE}/api/v1/mgmt/${tenantName}/connections`, {
    headers: mgmtHeaders,
    data: { name, strategy, options: {} },
  })
  if (!res.ok()) throw new Error(`apiCreateConnection failed: ${await res.text()}`)
  return res.json() as Promise<{ id: string; name: string; strategy: string }>
}

export async function apiCreateUser(
  request: APIRequestContext,
  tenantName: string,
  email: string,
  connectionId: string
) {
  const res = await request.post(`${BASE}/api/v1/mgmt/${tenantName}/users`, {
    headers: mgmtHeaders,
    data: { email, connection_id: connectionId },
  })
  if (!res.ok()) throw new Error(`apiCreateUser failed: ${await res.text()}`)
  return res.json() as Promise<{ id: string; email: string }>
}

export async function apiDeleteUser(
  request: APIRequestContext,
  tenantName: string,
  userId: string
) {
  await request.delete(`${BASE}/api/v1/mgmt/${tenantName}/users/${userId}`, {
    headers: mgmtHeaders,
  })
}

export async function apiCreateOrganization(
  request: APIRequestContext,
  tenantName: string,
  name: string,
  displayName?: string
) {
  const res = await request.post(`${BASE}/api/v1/mgmt/${tenantName}/organizations`, {
    headers: mgmtHeaders,
    data: { name, display_name: displayName },
  })
  if (!res.ok()) throw new Error(`apiCreateOrganization failed: ${await res.text()}`)
  return res.json() as Promise<{ id: string; name: string }>
}

export async function apiDeleteOrganization(
  request: APIRequestContext,
  tenantName: string,
  orgId: string
) {
  await request.delete(`${BASE}/api/v1/mgmt/${tenantName}/organizations/${orgId}`, {
    headers: mgmtHeaders,
  })
}

export async function apiDeleteConnection(
  request: APIRequestContext,
  tenantName: string,
  connId: string
) {
  await request.delete(`${BASE}/api/v1/mgmt/${tenantName}/connections/${connId}`, {
    headers: mgmtHeaders,
  })
}

export function uniqueTenantName(prefix: string): string {
  return `${prefix}-${Date.now()}`
}
