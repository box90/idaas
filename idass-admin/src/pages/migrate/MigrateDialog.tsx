import { useState, useEffect } from "react"
import { useParams, useNavigate } from "react-router-dom"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { migrateTenant, fetchTenants } from "@/api/tenants"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { TENANTS_KEY } from "@/hooks/useTenants"
import type { Tenant } from "@/api/types"

const REGIONS: string[] = ((import.meta.env.VITE_REGIONS ?? "eu-west-1") as string).split(",")
type Step = "select" | "confirm" | "progress" | "done"

export function MigrateDialog() {
  const { tenantName = "" } = useParams()
  const navigate = useNavigate()
  const qc = useQueryClient()

  const [step, setStep]           = useState<Step>("select")
  const [targetRegion, setTarget] = useState(REGIONS[0])
  const [confirmText, setConfirm] = useState("")

  const { data: tenants, refetch } = useQuery({ queryKey: TENANTS_KEY, queryFn: fetchTenants, refetchInterval: step === "progress" ? 3000 : false })
  const tenant: Tenant | undefined = tenants?.find((t) => t.name === tenantName)

  const mutation = useMutation({
    mutationFn: () => migrateTenant(tenant?.id ?? "", targetRegion),
    onSuccess: () => setStep("progress"),
  })

  useEffect(() => {
    if (step === "progress" && tenant?.status === "active" && tenant?.current_region === targetRegion) {
      qc.invalidateQueries({ queryKey: TENANTS_KEY })
      setStep("done")
    }
  }, [step, tenant, targetRegion, qc])

  // suppress unused warning for refetch — it is used implicitly via refetchInterval
  void refetch

  const currentRegion = tenant?.current_region ?? "unknown"
  const availableRegions = REGIONS.filter((r) => r !== currentRegion)

  return (
    <div className="p-8 max-w-lg">
      <h1 className="text-2xl font-bold text-slate-900 mb-1">Migrate Region</h1>
      <p className="text-slate-500 text-sm mb-6">Move <strong>{tenantName}</strong> to a different regional database. Source data will be purged after transfer (GDPR).</p>

      {step === "select" && (
        <div className="space-y-4">
          <div>
            <label className="text-sm font-medium">Target region</label>
            <select value={targetRegion} onChange={(e) => setTarget(e.target.value)} className="mt-1 w-full border rounded-md px-3 py-2 text-sm">
              {availableRegions.map((r) => <option key={r}>{r}</option>)}
            </select>
            <p className="text-xs text-slate-400 mt-1">Current region: {currentRegion}</p>
          </div>
          <Button onClick={() => setStep("confirm")} className="w-full">Continue →</Button>
        </div>
      )}

      {step === "confirm" && (
        <div className="space-y-4">
          <div className="bg-amber-50 border border-amber-200 rounded-md p-4 text-sm text-amber-800">
            <strong>This will:</strong>
            <ul className="mt-1 list-disc list-inside space-y-1">
              <li>Lock {tenantName} during migration</li>
              <li>Copy all data to <strong>{targetRegion}</strong></li>
              <li>Permanently delete data from {currentRegion}</li>
            </ul>
          </div>
          <div>
            <label className="text-sm font-medium">Type <strong>{tenantName}</strong> to confirm</label>
            <Input value={confirmText} onChange={(e) => setConfirm(e.target.value)} className="mt-1" placeholder={tenantName} />
          </div>
          <div className="flex gap-2">
            <Button variant="outline" onClick={() => setStep("select")} className="flex-1">Back</Button>
            <Button onClick={() => mutation.mutate()} disabled={confirmText !== tenantName || mutation.isPending} className="flex-1 bg-red-600 hover:bg-red-700">
              {mutation.isPending ? "Starting…" : "Migrate"}
            </Button>
          </div>
        </div>
      )}

      {step === "progress" && (
        <div className="text-center py-8">
          <div className="animate-spin w-8 h-8 border-4 border-blue-600 border-t-transparent rounded-full mx-auto mb-4" />
          <p className="text-slate-600">Migration in progress…</p>
          <p className="text-slate-400 text-sm mt-1">Status: {tenant?.status ?? "checking"}</p>
        </div>
      )}

      {step === "done" && (
        <div className="text-center py-8">
          <div className="text-4xl mb-4">✅</div>
          <p className="text-slate-800 font-medium">Migration complete</p>
          <p className="text-slate-500 text-sm mt-1">{tenantName} is now active in {tenant?.current_region}</p>
          <Button onClick={() => navigate(`/tenants/${tenantName}/users`)} className="mt-4">Back to tenant</Button>
        </div>
      )}
    </div>
  )
}
