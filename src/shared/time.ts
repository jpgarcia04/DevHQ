export function relativeTime(timestamp: string, reference: string): string {
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

export function formatTime(timestamp: string, timeZone: string): string {
  return new Intl.DateTimeFormat('en-GB', { hour: '2-digit', minute: '2-digit', timeZone }).format(
    new Date(timestamp),
  )
}

export function formatDate(timestamp: string, timeZone: string): string {
  return new Intl.DateTimeFormat('en-GB', {
    day: 'numeric',
    month: 'short',
    year: 'numeric',
    timeZone,
  }).format(new Date(timestamp))
}
