import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query"
import { fetchUsers, deleteUser } from "@/api/users"
import { toast } from "sonner"

export const usersKey = (tenant: string) => ["users", tenant]

export function useUsers(tenant: string) {
  return useQuery({ queryKey: usersKey(tenant), queryFn: () => fetchUsers(tenant), enabled: !!tenant })
}

export function useDeleteUser(tenant: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (userId: string) => deleteUser(tenant, userId),
    onSuccess: () => { qc.invalidateQueries({ queryKey: usersKey(tenant) }); toast.success("User deleted") },
    onError: () => toast.error("Failed to delete user"),
  })
}
