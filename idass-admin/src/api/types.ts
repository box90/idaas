export interface Tenant {
  id: string
  name: string
  current_region: string
  status: "active" | "read_only" | "migrating"
  updated_at: string
}

export interface User {
  id: string
  tenant_id: string
  organization_id: string | null
  connection_id: string
  email: string
  external_provider_id: string | null
  created_at: string
  updated_at: string
}

export interface ConnectionSummary {
  id: string
  tenant_id: string
  name: string
  strategy: "database" | "oauth2" | "saml"
  webhook_url: string | null
  created_at: string
}

export interface Organization {
  id: string
  tenant_id: string
  name: string
  display_name: string | null
  created_at: string
}
