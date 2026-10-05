export function relativeTime(timestamp: string, reference: string | number): string {
  const minutes = Math.round(
    (new Date(reference).getTime() - new Date(timestamp).getTime()) / 60_000,
  )
  if (minutes < 0) return 'Scheduled'
  if (minutes < 1) return 'Just now'
  if (minutes < 60) return `${minutes} min ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours} ${hours === 1 ? 'hour' : 'hours'} ago`
  const days = Math.floor(hours / 24)
  return `${days} ${days === 1 ? 'day' : 'days'} ago`
}

/** The remaining time until a later moment, such as the end of an observation's validity. */
export function timeUntil(timestamp: string, reference: number): string {
  const minutes = Math.floor((new Date(timestamp).getTime() - reference) / 60_000)
  if (minutes < 1) return 'in under a minute'
  if (minutes < 60) return `in ${minutes} min`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `in ${hours} ${hours === 1 ? 'hour' : 'hours'}`
  const days = Math.floor(hours / 24)
  return `in ${days} ${days === 1 ? 'day' : 'days'}`
}

// Without a time zone these format in the device's zone, which is what real records use.
export function formatTime(timestamp: string, timeZone?: string): string {
  return new Intl.DateTimeFormat('en-GB', { hour: '2-digit', minute: '2-digit', timeZone }).format(
    new Date(timestamp),
  )
}

export function formatDate(timestamp: string, timeZone?: string): string {
  return new Intl.DateTimeFormat('en-GB', {
    day: 'numeric',
    month: 'short',
    year: 'numeric',
    timeZone,
  }).format(new Date(timestamp))
}

export function formatDateTime(timestamp: string): string {
  return `${formatDate(timestamp)}, ${formatTime(timestamp)}`
}

/** To the second, for moments that can be a minute apart, such as two backups. */
export function formatMoment(timestamp: string): string {
  return new Intl.DateTimeFormat('en-GB', {
    day: 'numeric',
    month: 'short',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  }).format(new Date(timestamp))
}

export const deviceTimeZone = () => Intl.DateTimeFormat().resolvedOptions().timeZone

/** The value a datetime-local input expects, in the device's time zone. */
export function toDateTimeLocal(time: number): string {
  const date = new Date(time)
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`
}
