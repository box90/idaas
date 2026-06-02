import { useState } from "react"
import { useParams } from "react-router-dom"
import { useUsers, useDeleteUser } from "@/hooks/useUsers"
import { ConfirmDialog } from "@/components/ConfirmDialog"
import { Button } from "@/components/ui/button"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"

export function UserListPage() {
  const { tenantName = "" } = useParams()
  const { data: users, isLoading } = useUsers(tenantName)
  const deleteMutation = useDeleteUser(tenantName)
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; email: string } | null>(null)

  if (isLoading) return <div className="p-8 text-slate-500">Loading…</div>

  return (
    <div className="p-8">
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-bold text-slate-900">Users — {tenantName}</h1>
      </div>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Email</TableHead>
            <TableHead>Connection</TableHead>
            <TableHead>Created</TableHead>
            <TableHead />
          </TableRow>
        </TableHeader>
        <TableBody>
          {(users ?? []).map((u) => (
            <TableRow key={u.id}>
              <TableCell className="font-medium">{u.email}</TableCell>
              <TableCell className="text-slate-500 text-xs font-mono">{u.connection_id.slice(0, 8)}…</TableCell>
              <TableCell className="text-slate-500 text-sm">{new Date(u.created_at).toLocaleDateString()}</TableCell>
              <TableCell className="text-right">
                <Button variant="ghost" size="sm" className="text-red-600" onClick={() => setDeleteTarget({ id: u.id, email: u.email })}>
                  Delete
                </Button>
              </TableCell>
            </TableRow>
          ))}
          {(users ?? []).length === 0 && (
            <TableRow><TableCell colSpan={4} className="text-center text-slate-400 py-8">No users in this tenant.</TableCell></TableRow>
          )}
        </TableBody>
      </Table>
      <ConfirmDialog
        open={!!deleteTarget}
        title="Delete user"
        description={`Permanently delete ${deleteTarget?.email}?`}
        onConfirm={() => { if (deleteTarget) deleteMutation.mutate(deleteTarget.id); setDeleteTarget(null) }}
        onCancel={() => setDeleteTarget(null)}
      />
    </div>
  )
}
