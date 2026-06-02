import { test, expect } from './fixtures/base'
import {
  apiCreateTenant, apiCreateOrganization, apiDeleteOrganization, uniqueTenantName,
} from './fixtures/api'

test.describe('Organization management', () => {
  let tenantName: string

  test.beforeAll(async ({ request }) => {
    tenantName = uniqueTenantName('e2e-orgs')
    await apiCreateTenant(request, tenantName)
  })

  test('empty org list shows placeholder', async ({ page }) => {
    await page.goto(`/tenants/${tenantName}/organizations`)
    await expect(page.getByText(/No organizations/)).toBeVisible()
  })

  test('org created via API appears in the list', async ({ page, request }) => {
    const org = await apiCreateOrganization(request, tenantName, 'engineering', 'Engineering Team')
    await page.goto(`/tenants/${tenantName}/organizations`)
    await expect(page.getByText('engineering', { exact: true })).toBeVisible()
    await expect(page.getByText('Engineering Team')).toBeVisible()
    await apiDeleteOrganization(request, tenantName, org.id)
  })

  test('can create an org via the UI dialog', async ({ page }) => {
    const orgName = `e2e-org-${Date.now()}`
    await page.goto(`/tenants/${tenantName}/organizations`)

    await page.getByRole('button', { name: 'Add Organization' }).click()
    const dialog = page.getByRole('dialog')
    await expect(dialog).toBeVisible({ timeout: 3_000 })

    // Label lacks htmlFor association; target the first textbox in the dialog
    await dialog.getByRole('textbox').first().fill(orgName)
    await page.getByRole('button', { name: 'Create' }).click()
    await expect(page.getByText(orgName)).toBeVisible({ timeout: 8_000 })
  })

  test('delete org removes it from list', async ({ page, request }) => {
    const orgName = `del-org-${Date.now()}`
    const org = await apiCreateOrganization(request, tenantName, orgName)
    await page.goto(`/tenants/${tenantName}/organizations`)
    await expect(page.getByText(orgName)).toBeVisible()

    await page.getByRole('row', { name: new RegExp(orgName) })
      .getByRole('button', { name: 'Delete' })
      .click()
    await expect(page.getByRole('alertdialog')).toBeVisible({ timeout: 3_000 })
    await page.getByRole('button', { name: 'Confirm' }).click()
    await expect(page.getByText(orgName)).not.toBeVisible({ timeout: 5_000 })
  })
})
