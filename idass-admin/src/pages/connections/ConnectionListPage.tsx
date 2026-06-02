import { useState } from "react"
import { useParams } from "react-router-dom"
import { useConnections, useCreateConnection, useUpdateConnection, useDeleteConnection } from "@/hooks/useConnections"
import { ConnectionFormDialog } from "./ConnectionFormDialog"
import { ConfirmDialog } from "@/components/ConfirmDialog"
import { StrategyBadge } from "@/components/StrategyBadge"
import { Button } from "@/components/ui/button"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import type { ConnectionSummary } from "@/api/types"

export function ConnectionListPage() {
  const { tenantName = "" } = useParams()
  const { data: connections, isLoading } = useConnections(tenantName)
  const createMutation = useCreateConnection(tenantName)
  const updateMutation = useUpdateConnection(tenantName)
  const deleteMutation = useDeleteConnection(tenantName)
  const [createOpen, setCreateOpen] = useState(false)
  const [editTarget, setEditTarget] = useState<ConnectionSummary | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<ConnectionSummary | null>(null)

  if (isLoading) return <div className="p-8 text-slate-500">Loading…</div>

  return (
    <div className="p-8">
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-bold text-slate-900">Connections — {tenantName}</h1>
        <Button onClick={() => setCreateOpen(true)}>Add Connection</Button>
      </div>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Name</TableHead><TableHead>Strategy</TableHead><TableHead>Webhook</TableHead><TableHead />
          </TableRow>
        </TableHeader>
        <TableBody>
          {(connections ?? []).map((c) => (
            <TableRow key={c.id}>
              <TableCell className="font-medium">{c.name}</TableCell>
              <TableCell><StrategyBadge strategy={c.strategy} /></TableCell>
              <TableCell className="text-slate-500 text-sm">{c.webhook_url ?? "—"}</TableCell>
              <TableCell className="text-right space-x-2">
                <Button variant="ghost" size="sm" onClick={() => setEditTarget(c)}>Edit</Button>
                <Button variant="ghost" size="sm" className="text-red-600" onClick={() => setDeleteTarget(c)}>Delete</Button>
              </TableCell>
            </TableRow>
          ))}
          {(connections ?? []).length === 0 && (
            <TableRow><TableCell colSpan={4} className="text-center text-slate-400 py-8">No connections yet.</TableCell></TableRow>
          )}
        </TableBody>
      </Table>
      <ConnectionFormDialog open={createOpen} onClose={() => setCreateOpen(false)} isPending={createMutation.isPending}
        onSubmit={(data) => { createMutation.mutate(data); setCreateOpen(false) }} />
      {editTarget && (
        <ConnectionFormDialog open existing={editTarget} onClose={() => setEditTarget(null)} isPending={updateMutation.isPending}
          onSubmit={(data) => { updateMutation.mutate({ id: editTarget.id, ...data }); setEditTarget(null) }} />
      )}
      <ConfirmDialog open={!!deleteTarget} title="Delete connection" description={`Delete "${deleteTarget?.name}"?`}
        onConfirm={() => { if (deleteTarget) deleteMutation.mutate(deleteTarget.id); setDeleteTarget(null) }}
        onCancel={() => setDeleteTarget(null)} />
    </div>
  )
}
