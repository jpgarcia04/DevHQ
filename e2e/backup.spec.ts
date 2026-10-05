import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'

// The preview has no database to copy: Back up now is refused like every write, and says so,
// while the sample backups folder is listed as found, an interrupted backup among its files.
test('Back up now is refused in the preview, which lists a sample backups folder', async ({
  page,
}) => {
  await page.goto('/preview.html#/settings')
  const files = page.getByRole('list', { name: 'Files in the backups folder' })
  await expect(files.getByRole('listitem')).toHaveCount(3)
  await expect(files.getByRole('listitem').nth(1)).toContainText(
    'Incomplete: a backup was writing it or was interrupted. It is not a backup.',
  )

  await page.getByRole('button', { name: 'Back up now' }).click()
  await expect(page.getByRole('alert')).toHaveText(
    'No backup was made. This preview shows sample data and has no database: nothing was saved.',
  )
  await expect(page.getByRole('status', { name: 'Backup' })).toBeEmpty()
  await expect(page.getByRole('button', { name: 'Back up now' })).toBeEnabled()

  const { violations } = await new AxeBuilder({ page }).analyze()
  expect(violations.map(({ id, help }) => ({ id, help }))).toEqual([])
})
