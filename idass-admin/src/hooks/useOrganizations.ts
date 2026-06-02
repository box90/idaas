import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query"
import { fetchOrganizations, createOrganization, deleteOrganization } from "@/api/organizations"
import { toast } from "sonner"

export const orgsKey = (tenant: string) => ["organizations", tenant]

export function useOrganizations(tenant: string) {
  return useQuery({ queryKey: orgsKey(tenant), queryFn: () => fetchOrganizations(tenant), enabled: !!tenant })
}

export function useCreateOrganization(tenant: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (p: { name: string; display_name?: string }) => createOrganization(tenant, p),
    onSuccess: () => { qc.invalidateQueries({ queryKey: orgsKey(tenant) }); toast.success("Organization created") },
    onError: () => toast.error("Failed to create organization"),
  })
}

export function useDeleteOrganization(tenant: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => deleteOrganization(tenant, id),
    onSuccess: () => { qc.invalidateQueries({ queryKey: orgsKey(tenant) }); toast.success("Organization deleted") },
    onError: () => toast.error("Failed to delete organization"),
  })
}
