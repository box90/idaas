import { test, expect } from './fixtures/base'
import {
  apiCreateTenant, apiCreateConnection, apiCreateUser,
  apiDeleteUser, uniqueTenantName,
} from './fixtures/api'

test.describe('User management', () => {
  let tenantName: string
  let connectionId: string

  test.beforeAll(async ({ request }) => {
    tenantName = uniqueTenantName('e2e-users')
    await apiCreateTenant(request, tenantName)
    const conn = await apiCreateConnection(request, tenantName, 'default-db')
    connectionId = conn.id
  })

  test('empty user list shows placeholder', async ({ page }) => {
    await page.goto(`/tenants/${tenantName}/users`)
    await expect(page.getByText(/No users/)).toBeVisible()
  })

  test('user list shows users created via API', async ({ page, request }) => {
    const email = `alice-${Date.now()}@example.com`
    const user = await apiCreateUser(request, tenantName, email, connectionId)
    await page.goto(`/tenants/${tenantName}/users`)
    await expect(page.getByText(email)).toBeVisible()
    await apiDeleteUser(request, tenantName, user.id)
  })

  test('delete button removes user after confirmation', async ({ page, request }) => {
    const email = `delete-me-${Date.now()}@example.com`
    const user = await apiCreateUser(request, tenantName, email, connectionId)
    await page.goto(`/tenants/${tenantName}/users`)
    await expect(page.getByText(email)).toBeVisible()

    // Click delete on this specific user's row
    await page.getByRole('row', { name: new RegExp(email.split('@')[0]) })
      .getByRole('button', { name: 'Delete' })
      .click()

    // Confirm in alert dialog
    await expect(page.getByRole('alertdialog')).toBeVisible({ timeout: 3_000 })
    await page.getByRole('button', { name: 'Confirm' }).click()

    await expect(page.getByText(email)).not.toBeVisible({ timeout: 5_000 })
  })
})
