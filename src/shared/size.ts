const amount = new Intl.NumberFormat('en-GB', { maximumFractionDigits: 1 })

/** A file's size in units of 1024 bytes, as file managers show it. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 ** 2) return `${amount.format(bytes / 1024)} KB`
  if (bytes < 1024 ** 3) return `${amount.format(bytes / 1024 ** 2)} MB`
  return `${amount.format(bytes / 1024 ** 3)} GB`
}
