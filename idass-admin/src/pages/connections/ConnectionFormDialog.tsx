import { useState } from "react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import type { ConnectionSummary } from "@/api/types"
import type { ConnectionOptions, OIDCOptions } from "@/api/connections"

const OAUTH2_FIELDS = ["client_id", "client_secret", "redirect_uri", "token_endpoint"]
const SAML_FIELDS   = ["idp_sso_url", "idp_entity_id", "idp_certificate_pem", "sp_entity_id", "acs_url"]
const SECRET_FIELDS = new Set(["client_secret", "idp_certificate_pem"])

interface Props {
  open: boolean
  onClose: () => void
  existing?: ConnectionSummary
  onSubmit: (data: {
    name: string
    strategy: string
    options: ConnectionOptions
    webhook_url?: string
  }) => void
  isPending: boolean
}

export function ConnectionFormDialog({ open, onClose, existing, onSubmit, isPending }: Props) {
  const [name, setName]          = useState(existing?.name ?? "")
  const [strategy, setStrategy]  = useState<string>(existing?.strategy ?? "database")
  const [webhookUrl, setWebhook] = useState(existing?.webhook_url ?? "")
  const [options, setOptions]    = useState<Record<string, string>>({})
  const [reveal, setReveal]      = useState<Record<string, boolean>>({})

  // OIDC-specific state
  const [oidcMode, setOidcMode]           = useState<"discover" | "custom">("discover")
  const [issuerUrl, setIssuerUrl]         = useState("")
  const [discoveryJson, setDiscoveryJson] = useState("")
  const [oidcClientId, setOidcClientId]   = useState("")
  const [oidcSecret, setOidcSecret]       = useState("")
  const [oidcRedirect, setOidcRedirect]   = useState("")
  const [showOidcSecret, setShowOidcSecret] = useState(false)
  const [jsonError, setJsonError]         = useState("")

  const isEdit = !!existing

  const setOption = (k: string, v: string) => setOptions((p) => ({ ...p, [k]: v }))
  const toggleReveal = (k: string) => setReveal((p) => ({ ...p, [k]: !p[k] }))

  const fields =
    strategy === "oauth2" ? OAUTH2_FIELDS :
    strategy === "saml"   ? SAML_FIELDS   : []

  const buildOidcOptions = (): OIDCOptions | null => {
    if (oidcMode === "discover") {
      if (!issuerUrl) return null
      return {
        mode: "discover",
        issuer_url: issuerUrl,
        client_id: oidcClientId,
        client_secret: oidcSecret,
        redirect_uri: oidcRedirect,
      }
    } else {
      try {
        const doc = JSON.parse(discoveryJson)
        return {
          mode: "custom",
          discovery_document: doc,
          client_id: oidcClientId,
          client_secret: oidcSecret,
          redirect_uri: oidcRedirect,
        }
      } catch {
        setJsonError("Invalid JSON — please check the discovery document.")
        return null
      }
    }
  }

  const handleSubmit = () => {
    setJsonError("")
    let opts: ConnectionOptions
    if (strategy === "oidc") {
      const oidcOpts = buildOidcOptions()
      if (!oidcOpts) return
      opts = oidcOpts
    } else {
      opts = options
    }
    onSubmit({ name, strategy, options: opts, webhook_url: webhookUrl || undefined })
  }

  const canSubmit =
    !!name &&
    !isPending &&
    (strategy !== "oidc" || (
      !!oidcClientId && !!oidcSecret && !!oidcRedirect &&
      (oidcMode === "discover" ? !!issuerUrl : !!discoveryJson)
    ))

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{isEdit ? "Edit Connection" : "New Connection"}</DialogTitle>
        </DialogHeader>

        <div className="space-y-4 pt-2 max-h-[70vh] overflow-y-auto pr-1">
          {/* Name */}
          <div>
            <Label>Name</Label>
            <Input value={name} onChange={(e) => setName(e.target.value)} className="mt-1" />
          </div>

          {/* Strategy selector (hidden on edit) */}
          {!isEdit && (
            <div>
              <Label>Strategy</Label>
              <select
                value={strategy}
                onChange={(e) => setStrategy(e.target.value)}
                className="mt-1 w-full border rounded-md px-3 py-2 text-sm"
              >
                <option value="database">Database</option>
                <option value="oauth2">OAuth2 (Google)</option>
                <option value="saml">SAML 2.0</option>
                <option value="oidc">OpenID Connect (OIDC)</option>
              </select>
            </div>
          )}

          {/* ── OIDC fields ────────────────────────────────────────────── */}
          {strategy === "oidc" && (
            <>
              <div>
                <Label>Discovery</Label>
                <div className="flex gap-4 mt-2">
                  <label className="flex items-center gap-2 cursor-pointer text-sm">
                    <input
                      type="radio"
                      name="oidc-mode"
                      checked={oidcMode === "discover"}
                      onChange={() => setOidcMode("discover")}
                    />
                    Auto-discover from issuer URL
                  </label>
                  <label className="flex items-center gap-2 cursor-pointer text-sm">
                    <input
                      type="radio"
                      name="oidc-mode"
                      checked={oidcMode === "custom"}
                      onChange={() => setOidcMode("custom")}
                    />
                    Custom document
                  </label>
                </div>
              </div>

              {oidcMode === "discover" ? (
                <div>
                  <Label>Issuer URL</Label>
                  <Input
                    value={issuerUrl}
                    onChange={(e) => setIssuerUrl(e.target.value)}
                    className="mt-1"
                    placeholder="https://keycloak.example.com/realms/myrealm"
                  />
                  <p className="text-xs text-slate-400 mt-1">
                    Server fetches{" "}
                    <code>{"<issuer>"}/.well-known/openid-configuration</code> at save time.
                  </p>
                </div>
              ) : (
                <div>
                  <Label>Discovery Document (JSON)</Label>
                  <textarea
                    value={discoveryJson}
                    onChange={(e) => { setDiscoveryJson(e.target.value); setJsonError("") }}
                    className="mt-1 w-full border rounded-md px-3 py-2 text-xs font-mono h-32 resize-y"
                    placeholder={'{\n  "issuer": "…",\n  "authorization_endpoint": "…",\n  "token_endpoint": "…",\n  "jwks_uri": "…"\n}'}
                  />
                  {jsonError && <p className="text-xs text-red-600 mt-1">{jsonError}</p>}
                </div>
              )}

              <div>
                <Label>Client ID</Label>
                <Input value={oidcClientId} onChange={(e) => setOidcClientId(e.target.value)} className="mt-1" />
              </div>

              <div>
                <div className="flex items-center justify-between">
                  <Label>Client Secret</Label>
                  {isEdit && (
                    <button
                      type="button"
                      onClick={() => setShowOidcSecret((v) => !v)}
                      className="text-xs text-blue-600 hover:underline"
                    >
                      {showOidcSecret ? "Hide" : "Replace secret"}
                    </button>
                  )}
                </div>
                {isEdit && !showOidcSecret ? (
                  <Input value="••••••••••••" disabled className="mt-1 text-slate-400" />
                ) : (
                  <Input
                    type="password"
                    value={oidcSecret}
                    onChange={(e) => setOidcSecret(e.target.value)}
                    className="mt-1"
                  />
                )}
              </div>

              <div>
                <Label>Redirect URI</Label>
                <Input
                  value={oidcRedirect}
                  onChange={(e) => setOidcRedirect(e.target.value)}
                  className="mt-1"
                  placeholder="https://app.example.com/callback"
                />
              </div>
            </>
          )}

          {/* ── oauth2 / saml fields (unchanged) ─────────────────────── */}
          {fields.map((field) => {
            const isSecret = SECRET_FIELDS.has(field)
            const revealed = reveal[field]
            return (
              <div key={field}>
                <div className="flex items-center justify-between">
                  <Label>{field}</Label>
                  {isEdit && isSecret && (
                    <button
                      type="button"
                      onClick={() => toggleReveal(field)}
                      className="text-xs text-blue-600 hover:underline"
                    >
                      {revealed ? "Hide" : "Replace secret"}
                    </button>
                  )}
                </div>
                {isEdit && isSecret && !revealed ? (
                  <Input value="••••••••••••" disabled className="mt-1 text-slate-400" />
                ) : (
                  <Input
                    type={isSecret ? "password" : "text"}
                    value={options[field] ?? ""}
                    onChange={(e) => setOption(field, e.target.value)}
                    className="mt-1"
                  />
                )}
              </div>
            )
          })}

          {/* Webhook URL */}
          <div>
            <Label>Webhook URL (optional)</Label>
            <Input
              value={webhookUrl}
              onChange={(e) => setWebhook(e.target.value)}
              className="mt-1"
              placeholder="https://your-service.com/enrich"
            />
          </div>

          <Button onClick={handleSubmit} disabled={!canSubmit} className="w-full">
            {isPending ? "Saving…" : isEdit ? "Save Changes" : "Create Connection"}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
