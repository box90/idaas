import { useState } from "react"
import { useNavigate } from "react-router-dom"
import { useTenants, useCreateTenant } from "@/hooks/useTenants"
import { StatusBadge } from "@/components/StatusBadge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"

const REGIONS = (import.meta.env.VITE_REGIONS ?? "eu-west-1").split(",")

export function TenantListPage() {
  const { data: tenants, isLoading } = useTenants()
  const createMutation = useCreateTenant()
  const navigate = useNavigate()
  const [open, setOpen]     = useState(false)
  const [name, setName]     = useState("")
  const [region, setRegion] = useState(REGIONS[0])

  if (isLoading) return <div className="p-8 text-slate-500">Loading…</div>

  return (
    <div className="p-8">
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-bold text-slate-900">Tenants</h1>
        <Dialog open={open} onOpenChange={setOpen}>
          <DialogTrigger render={<Button />}>Create Tenant</DialogTrigger>
          <DialogContent>
            <DialogHeader><DialogTitle>New Tenant</DialogTitle></DialogHeader>
            <div className="space-y-4 pt-2">
              <div>
                <Label>Name</Label>
                <Input value={name} onChange={(e) => setName(e.target.value)} className="mt-1" placeholder="acme-corp" />
              </div>
              <div>
                <Label>Region</Label>
                <select value={region} onChange={(e) => setRegion(e.target.value)} className="mt-1 w-full border rounded-md px-3 py-2 text-sm">
                  {REGIONS.map((r: string) => <option key={r}>{r}</option>)}
                </select>
              </div>
              <Button
                onClick={() => { createMutation.mutate({ name, region }); setOpen(false); setName("") }}
                disabled={!name || createMutation.isPending}
                className="w-full"
              >
                {createMutation.isPending ? "Creating…" : "Create"}
              </Button>
            </div>
          </DialogContent>
        </Dialog>
      </div>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Name</TableHead>
            <TableHead>Region</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Updated</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {(tenants ?? []).map((t) => (
            <TableRow key={t.id} className="cursor-pointer hover:bg-slate-50" onClick={() => navigate(`/tenants/${t.name}/users`)}>
              <TableCell className="font-medium">{t.name}</TableCell>
              <TableCell>{t.current_region}</TableCell>
              <TableCell><StatusBadge status={t.status} /></TableCell>
              <TableCell className="text-slate-500 text-sm">{new Date(t.updated_at).toLocaleDateString()}</TableCell>
            </TableRow>
          ))}
          {(tenants ?? []).length === 0 && (
            <TableRow><TableCell colSpan={4} className="text-center text-slate-400 py-8">No tenants yet.</TableCell></TableRow>
          )}
        </TableBody>
      </Table>
    </div>
  )
}
