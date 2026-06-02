import { useState } from "react"
import { useParams } from "react-router-dom"
import { useOrganizations, useCreateOrganization, useDeleteOrganization } from "@/hooks/useOrganizations"
import { ConfirmDialog } from "@/components/ConfirmDialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import type { Organization } from "@/api/types"

export function OrganizationListPage() {
  const { tenantName = "" } = useParams()
  const { data: orgs, isLoading } = useOrganizations(tenantName)
  const createMutation = useCreateOrganization(tenantName)
  const deleteMutation = useDeleteOrganization(tenantName)
  const [createOpen, setCreateOpen] = useState(false)
  const [name, setName]             = useState("")
  const [displayName, setDisplayName] = useState("")
  const [deleteTarget, setDeleteTarget] = useState<Organization | null>(null)

  if (isLoading) return <div className="p-8 text-slate-500">Loading…</div>

  return (
    <div className="p-8">
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-bold text-slate-900">Organizations — {tenantName}</h1>
        <Dialog open={createOpen} onOpenChange={setCreateOpen}>
          <DialogTrigger render={<Button />}>Add Organization</DialogTrigger>
          <DialogContent>
            <DialogHeader><DialogTitle>New Organization</DialogTitle></DialogHeader>
            <div className="space-y-4 pt-2">
              <div><Label>Name</Label><Input value={name} onChange={(e) => setName(e.target.value)} className="mt-1" /></div>
              <div><Label>Display Name (optional)</Label><Input value={displayName} onChange={(e) => setDisplayName(e.target.value)} className="mt-1" /></div>
              <Button onClick={() => { createMutation.mutate({ name, display_name: displayName || undefined }); setCreateOpen(false); setName(""); setDisplayName("") }}
                disabled={!name || createMutation.isPending} className="w-full">
                {createMutation.isPending ? "Creating…" : "Create"}
              </Button>
            </div>
          </DialogContent>
        </Dialog>
      </div>
      <Table>
        <TableHeader><TableRow><TableHead>Name</TableHead><TableHead>Display Name</TableHead><TableHead>Created</TableHead><TableHead /></TableRow></TableHeader>
        <TableBody>
          {(orgs ?? []).map((o) => (
            <TableRow key={o.id}>
              <TableCell className="font-medium">{o.name}</TableCell>
              <TableCell className="text-slate-500">{o.display_name ?? "—"}</TableCell>
              <TableCell className="text-slate-500 text-sm">{new Date(o.created_at).toLocaleDateString()}</TableCell>
              <TableCell className="text-right">
                <Button variant="ghost" size="sm" className="text-red-600" onClick={() => setDeleteTarget(o)}>Delete</Button>
              </TableCell>
            </TableRow>
          ))}
          {(orgs ?? []).length === 0 && (<TableRow><TableCell colSpan={4} className="text-center text-slate-400 py-8">No organizations yet.</TableCell></TableRow>)}
        </TableBody>
      </Table>
      <ConfirmDialog open={!!deleteTarget} title="Delete organization" description={`Delete "${deleteTarget?.name}"?`}
        onConfirm={() => { if (deleteTarget) deleteMutation.mutate(deleteTarget.id); setDeleteTarget(null) }}
        onCancel={() => setDeleteTarget(null)} />
    </div>
  )
}
