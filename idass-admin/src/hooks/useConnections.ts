import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query"
import { fetchConnections, createConnection, updateConnection, deleteConnection } from "@/api/connections"
import type { ConnectionOptions } from "@/api/connections"
import { toast } from "sonner"

export const connectionsKey = (tenant: string) => ["connections", tenant]

export function useConnections(tenant: string) {
  return useQuery({ queryKey: connectionsKey(tenant), queryFn: () => fetchConnections(tenant), enabled: !!tenant })
}

export function useCreateConnection(tenant: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (p: { name: string; strategy: string; options: ConnectionOptions; webhook_url?: string }) =>
      createConnection(tenant, p),
    onSuccess: () => { qc.invalidateQueries({ queryKey: connectionsKey(tenant) }); toast.success("Connection created") },
    onError: () => toast.error("Failed to create connection"),
  })
}

export function useUpdateConnection(tenant: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ id, ...p }: { id: string; name?: string; options?: ConnectionOptions; webhook_url?: string | null }) =>
      updateConnection(tenant, id, p),
    onSuccess: () => { qc.invalidateQueries({ queryKey: connectionsKey(tenant) }); toast.success("Connection updated") },
    onError: () => toast.error("Failed to update connection"),
  })
}

export function useDeleteConnection(tenant: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => deleteConnection(tenant, id),
    onSuccess: () => { qc.invalidateQueries({ queryKey: connectionsKey(tenant) }); toast.success("Connection deleted") },
    onError: () => toast.error("Failed to delete connection"),
  })
}
