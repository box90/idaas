import { test, expect } from './fixtures/base'
import { apiCreateTenant, uniqueTenantName } from './fixtures/api'

test.describe('Tenant management', () => {
  let tenantName: string

  test.beforeAll(async ({ request }) => {
    tenantName = uniqueTenantName('e2e-tenants')
    await apiCreateTenant(request, tenantName)
  })

  test('tenant list page shows existing tenants', async ({ page }) => {
    await page.goto('/tenants')
    await expect(page.getByRole('heading', { name: 'Tenants' })).toBeVisible()
    await expect(page.getByText(tenantName)).toBeVisible()
  })

  test('clicking a tenant row navigates to its users page', async ({ page }) => {
    await page.goto('/tenants')
    await page.getByText(tenantName).first().click()
    await expect(page).toHaveURL(`/tenants/${tenantName}/users`)
  })

  test('sidebar shows tenant sub-navigation when tenant is selected', async ({ page }) => {
    await page.goto(`/tenants/${tenantName}/users`)
    // Sidebar links include emoji prefixes: "👤 Users", "🔗 Connections", "🏛 Organizations", "⚡ Migrate Region"
    await expect(page.getByRole('link', { name: /Users/ })).toBeVisible()
    await expect(page.getByRole('link', { name: /Connections/ })).toBeVisible()
    await expect(page.getByRole('link', { name: /Organizations/ })).toBeVisible()
    await expect(page.getByRole('link', { name: /Migrate/ })).toBeVisible()
  })

  test('create tenant dialog creates a new tenant', async ({ page }) => {
    const newName = uniqueTenantName('e2e-create')
    await page.goto('/tenants')

    await page.getByRole('button', { name: 'Create Tenant' }).click()
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 3_000 })

    // Label is not associated via htmlFor; use placeholder to target the Name input
    await page.getByPlaceholder('acme-corp').fill(newName)
    await page.getByRole('button', { name: 'Create' }).click()

    await expect(page.getByText(newName)).toBeVisible({ timeout: 8_000 })
  })
})
