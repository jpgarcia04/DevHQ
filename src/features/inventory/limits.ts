/**
 * The longest texts DevHQ stores, counted in UTF-16 code units as maxLength counts them. The
 * native process enforces the same limits (MAX_* in inventory.rs, MAX_URL_LENGTH in
 * http_check.rs) and refuses longer text instead of truncating it.
 */
export const textLimits = {
  name: 120,
  title: 160,
  summary: 500,
  description: 1000,
  version: 60,
  url: 2048,
} as const
