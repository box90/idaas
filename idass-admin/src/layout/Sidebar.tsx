import { NavLink, useParams } from "react-router-dom"
import { getKeyTail, clearKey } from "@/lib/auth"
import { queryClient } from "@/lib/queryClient"
import { Button } from "@/components/ui/button"

const base = "flex items-center px-3 py-2 rounded-md text-sm text-slate-400 hover:bg-slate-800 hover:text-white transition-colors"
const active = "bg-slate-700 text-white"

function NavItem({ to, label }: { to: string; label: string }) {
  return (
    <NavLink to={to} end className={({ isActive }) => `${base} ${isActive ? active : ""}`}>
      {label}
    </NavLink>
  )
}

export function Sidebar() {
  const { tenantName } = useParams<{ tenantName?: string }>()

  const handleLogout = () => {
    clearKey()
    queryClient.clear()
    window.location.replace("/login")
  }

  return (
    <aside className="w-52 shrink-0 bg-slate-900 flex flex-col h-screen sticky top-0">
      <div className="px-4 py-5">
        <span className="text-white font-bold text-base">IDaaS Admin</span>
      </div>

      <nav className="flex-1 px-2 space-y-1 overflow-y-auto">
        <p className="px-3 text-xs font-medium text-slate-500 uppercase tracking-wider mb-1">
          Platform
        </p>
        <NavItem to="/tenants" label="🏢 Tenants" />

        {tenantName && (
          <>
            <p className="px-3 text-xs font-medium text-slate-500 uppercase tracking-wider mt-4 mb-1 truncate">
              {tenantName}
            </p>
            <NavItem to={`/tenants/${tenantName}/users`}        label="👤 Users" />
            <NavItem to={`/tenants/${tenantName}/connections`}   label="🔗 Connections" />
            <NavItem to={`/tenants/${tenantName}/organizations`} label="🏛 Organizations" />
            <NavItem to={`/tenants/${tenantName}/migrate`}       label="⚡ Migrate Region" />
          </>
        )}
      </nav>

      <div className="px-4 py-3 border-t border-slate-700 text-xs text-slate-500">
        <p className="truncate mb-2">{getKeyTail()}</p>
        <Button
          variant="ghost"
          size="sm"
          className="w-full text-slate-400 hover:text-white"
          onClick={handleLogout}
        >
          Log out
        </Button>
      </div>
    </aside>
  )
}
