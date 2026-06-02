import { test, expect } from '@playwright/test'

const KEY = process.env.MANAGEMENT_API_KEY ?? ''

test.describe('Authentication', () => {
  test('valid key logs in and redirects to /tenants', async ({ page }) => {
    await page.goto('/login')
    await page.locator('input[type="password"]').fill(KEY)
    await page.getByRole('button', { name: 'Sign in' }).click()

    await expect(page).toHaveURL('/tenants', { timeout: 10_000 })
    await expect(page.getByRole('heading', { name: 'Tenants' })).toBeVisible()
  })

  test('invalid key stays on /login page', async ({ page }) => {
    await page.goto('/login')
    await page.locator('input[type="password"]').fill('not-a-valid-key-xyz')
    await page.getByRole('button', { name: 'Sign in' }).click()

    // The app receives a 401 and stays (redirects back) to /login
    await expect(page).toHaveURL('/login', { timeout: 8_000 })
    // Login form is visible (not navigated to /tenants)
    await expect(page.locator('input[type="password"]')).toBeVisible()
  })

  test('accessing /tenants without a key redirects to /login', async ({ page }) => {
    await page.goto('/tenants')
    await expect(page).toHaveURL('/login')
  })

  test('logout clears key and redirects to /login', async ({ page }) => {
    await page.goto('/login')
    await page.locator('input[type="password"]').fill(KEY)
    await page.getByRole('button', { name: 'Sign in' }).click()
    await expect(page).toHaveURL('/tenants', { timeout: 10_000 })

    await page.getByRole('button', { name: 'Log out' }).click()
    await expect(page).toHaveURL('/login')

    await page.goto('/tenants')
    await expect(page).toHaveURL('/login')
  })
})
