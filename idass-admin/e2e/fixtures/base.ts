import { test as base, expect } from '@playwright/test'

const MGMT_KEY = process.env.MANAGEMENT_API_KEY ?? ''

/** Extended test that pre-injects the management API key into sessionStorage
 *  so every test starts already authenticated. */
export const test = base.extend({
  page: async ({ page }, use) => {
    await page.addInitScript((key: string) => {
      sessionStorage.setItem('idass_mgmt_key', key)
    }, MGMT_KEY)
    await use(page)
  },
})

export { expect }
