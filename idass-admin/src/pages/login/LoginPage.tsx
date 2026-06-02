import { useState, type FormEvent } from "react"
import { useNavigate } from "react-router-dom"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { setKey } from "@/lib/auth"
import { fetchTenants } from "@/api/tenants"

export function LoginPage() {
  const [apiKey, setApiKey]   = useState("")
  const [error, setError]     = useState("")
  const [loading, setLoading] = useState(false)
  const navigate              = useNavigate()

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault()
    setError("")
    setLoading(true)
    try {
      setKey(apiKey.trim())
      await fetchTenants()
      navigate("/tenants", { replace: true })
    } catch {
      setKey("")
      setError("Invalid API key or server unreachable.")
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-slate-100">
      <div className="bg-white rounded-xl shadow-sm border border-slate-200 p-8 w-full max-w-sm">
        <h1 className="text-xl font-bold text-slate-900 mb-1">IDaaS Admin</h1>
        <p className="text-sm text-slate-500 mb-6">Enter your management API key to continue.</p>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <Label htmlFor="key">Management API Key</Label>
            <Input
              id="key"
              type="password"
              placeholder="Paste your MANAGEMENT_API_KEY"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              required
              className="mt-1"
            />
          </div>
          {error && <p className="text-sm text-red-600">{error}</p>}
          <Button type="submit" className="w-full" disabled={loading}>
            {loading ? "Verifying…" : "Sign in"}
          </Button>
        </form>
      </div>
    </div>
  )
}
