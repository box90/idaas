import { Outlet, useParams } from "react-router-dom"
import { useQuery } from "@tanstack/react-query"
import { fetchTenants } from "@/api/tenants"
import type { Tenant } from "@/api/types"

export function TenantDetailLayout() {
  const { tenantName = "" } = useParams()
  const { data: tenants } = useQuery({
    queryKey: ["tenants"],
    queryFn: fetchTenants,
    staleTime: 10_000,
  })
  const tenant: Tenant | undefined = tenants?.find((t) => t.name === tenantName)
  const isMigrating = tenant?.status === "migrating" || tenant?.status === "read_only"

  return (
    <div>
      {isMigrating && (
        <div className="bg-amber-50 border-b border-amber-200 px-8 py-3 text-amber-800 text-sm">
          ⚠ This tenant is locked — a migration is in progress. Most operations are unavailable.
        </div>
      )}
      <Outlet />
    </div>
  )
}
