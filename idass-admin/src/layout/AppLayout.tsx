import { Outlet, Navigate } from "react-router-dom"
import { Sidebar } from "./Sidebar"
import { getKey } from "@/lib/auth"

export function AppLayout() {
  if (!getKey()) return <Navigate to="/login" replace />
  return (
    <div className="flex min-h-screen bg-slate-50">
      <Sidebar />
      <main className="flex-1 overflow-auto">
        <Outlet />
      </main>
    </div>
  )
}
