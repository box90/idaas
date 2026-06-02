import { test, expect } from './fixtures/base'
import {
  apiCreateTenant, apiCreateConnection, apiDeleteConnection, uniqueTenantName,
} from './fixtures/api'

test.describe('Connection management', () => {
  let tenantName: string

  test.beforeAll(async ({ request }) => {
    tenantName = uniqueTenantName('e2e-conns')
    await apiCreateTenant(request, tenantName)
  })

  test('empty connection list shows placeholder', async ({ page }) => {
    await page.goto(`/tenants/${tenantName}/connections`)
    await expect(page.getByText(/No connections/)).toBeVisible()
  })

  test('can create a database connection', async ({ page }) => {
    await page.goto(`/tenants/${tenantName}/connections`)
    await page.getByRole('button', { name: 'Add Connection' }).click()
    const dialog = page.getByRole('dialog')
    await expect(dialog).toBeVisible({ timeout: 3_000 })

    // Label lacks htmlFor association; target the first textbox (input) in the dialog
    await dialog.getByRole('textbox').first().fill('my-db-conn')
    // strategy defaults to "database" — no extra fields
    await page.getByRole('button', { name: 'Create Connection' }).click()
    await expect(page.getByText('my-db-conn')).toBeVisible({ timeout: 8_000 })
  })

  test('edit form masks secret fields for oauth2 connection', async ({ page, request }) => {
    const conn = await apiCreateConnection(request, tenantName, 'secret-conn', 'oauth2')

    await page.goto(`/tenants/${tenantName}/connections`)
    await page.getByRole('row', { name: /secret-conn/ })
      .getByRole('button', { name: 'Edit' })
      .click()
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 3_000 })

    // Secret field should show masked placeholder (disabled input with bullet chars)
    await expect(page.locator('input[value="••••••••••••"]')).toBeVisible()

    // "Replace secret" toggle visible
    await expect(page.getByText('Replace secret')).toBeVisible()

    await page.keyboard.press('Escape')
    await apiDeleteConnection(request, tenantName, conn.id)
  })

  test('delete connection removes it from list', async ({ page, request }) => {
    const conn = await apiCreateConnection(request, tenantName, 'to-delete-conn')
    await page.goto(`/tenants/${tenantName}/connections`)
    await expect(page.getByText('to-delete-conn')).toBeVisible()

    await page.getByRole('row', { name: /to-delete-conn/ })
      .getByRole('button', { name: 'Delete' })
      .click()
    await expect(page.getByRole('alertdialog')).toBeVisible({ timeout: 3_000 })
    await page.getByRole('button', { name: 'Confirm' }).click()

    await expect(page.getByText('to-delete-conn')).not.toBeVisible({ timeout: 5_000 })
  })
})
