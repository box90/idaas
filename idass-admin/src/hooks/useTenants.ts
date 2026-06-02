import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query"
import { fetchTenants, createTenant } from "@/api/tenants"
import { toast } from "sonner"

export const TENANTS_KEY = ["tenants"]

export function useTenants() {
  return useQuery({ queryKey: TENANTS_KEY, queryFn: fetchTenants })
}

export function useCreateTenant() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ name, region }: { name: string; region: string }) => createTenant(name, region),
    onSuccess: () => { qc.invalidateQueries({ queryKey: TENANTS_KEY }); toast.success("Tenant created") },
    onError: () => toast.error("Failed to create tenant"),
  })
}
