import { Routes, Route, Navigate } from "react-router-dom"
import { AppLayout } from "@/layout/AppLayout"
import { LoginPage } from "@/pages/login/LoginPage"
import { TenantListPage } from "@/pages/tenants/TenantListPage"
import { TenantDetailLayout } from "@/pages/tenants/TenantDetailLayout"
import { UserListPage } from "@/pages/users/UserListPage"
import { ConnectionListPage } from "@/pages/connections/ConnectionListPage"
import { OrganizationListPage } from "@/pages/organizations/OrganizationListPage"
import { MigrateDialog } from "@/pages/migrate/MigrateDialog"

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route element={<AppLayout />}>
        <Route index element={<Navigate to="/tenants" replace />} />
        <Route path="/tenants" element={<TenantListPage />} />
        <Route path="/tenants/:tenantName" element={<TenantDetailLayout />}>
          <Route index element={<Navigate to="users" replace />} />
          <Route path="users"         element={<UserListPage />} />
          <Route path="connections"   element={<ConnectionListPage />} />
          <Route path="organizations" element={<OrganizationListPage />} />
          <Route path="migrate"       element={<MigrateDialog />} />
        </Route>
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  )
}
